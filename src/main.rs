use std::sync::Arc;

use ai_assist::agent::routine_engine::{self, RoutineEngine};
use ai_assist::agent::{Agent, AgentDeps};
use ai_assist::cards::generator::{CardGenerator, GeneratorConfig};
use ai_assist::cards::queue::{self, CardQueue};
use ai_assist::cards::ws::card_routes;
use ai_assist::channels::email::EmailConfig;
use ai_assist::channels::{ChannelManager, CliChannel, IosChannel, TelegramChannel};
use ai_assist::config::{AgentConfig, RoutineConfig};
use ai_assist::llm::{LlmBackend, LlmConfig, create_provider};
use ai_assist::safety::SafetyLayer;
use ai_assist::store::{Database, LibSqlBackend};
use ai_assist::todos::activity::{ActivityState, TodoActivityMessage, activity_routes};
use ai_assist::todos::ws::{TodoState, todo_routes};
use ai_assist::tools::ToolRegistry;
use ai_assist::worker::{ContextManager, Scheduler};
use ai_assist::workspace::Workspace;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install rustls crypto provider before any TLS usage
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Read API key from environment
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("Error: ANTHROPIC_API_KEY not set");
        eprintln!("  export ANTHROPIC_API_KEY=sk-ant-...");
        std::process::exit(1);
    });

    let model =
        std::env::var("AI_ASSIST_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

    let ws_port: u16 = std::env::var("AI_ASSIST_WS_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let card_expire_min: u32 = std::env::var("AI_ASSIST_CARD_EXPIRE_MIN")
        .unwrap_or_else(|_| "15".to_string())
        .parse()
        .unwrap_or(15);

    eprintln!("🤖 AI Assist v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("   Model: {}", model);
    eprintln!("   Card WS: ws://0.0.0.0:{}/ws", ws_port);
    eprintln!("   Chat WS: ws://0.0.0.0:{}/ws/chat", ws_port);
    eprintln!("   Chat API: http://0.0.0.0:{}/api/chat/history", ws_port);
    eprintln!("   Card API: http://0.0.0.0:{}/api/cards", ws_port);
    eprintln!("   Type a message and press Enter. /quit to exit.\n");

    // Create LLM provider
    let llm_config = LlmConfig {
        backend: LlmBackend::Anthropic,
        api_key: secrecy::SecretString::from(api_key),
        model,
    };
    let llm = create_provider(&llm_config)?;

    // ── Database ─────────────────────────────────────────────────────────
    let db_path =
        std::env::var("AI_ASSIST_DB_PATH").unwrap_or_else(|_| "./data/ai-assist.db".to_string());

    let db_path_ref = std::path::Path::new(&db_path);
    let db: Arc<dyn Database> = Arc::new(
        LibSqlBackend::new_local(db_path_ref)
            .await
            .unwrap_or_else(|e| {
                eprintln!("Error: Failed to open database at {}: {}", db_path, e);
                std::process::exit(1);
            }),
    );

    eprintln!("   Database: {}", db_path);

    // ── Card System ─────────────────────────────────────────────────────
    let card_queue = CardQueue::with_db(Arc::clone(&db)).await;

    let generator_config = GeneratorConfig {
        expire_minutes: card_expire_min,
        ..Default::default()
    };
    let card_generator = Arc::new(CardGenerator::new(
        llm.clone(),
        card_queue.clone(),
        generator_config,
    ));

    // ── Startup Recovery: reload unanswered messages as cards ──────────
    {
        let pending_messages = db.get_pending_messages().await.unwrap_or_default();
        let mut recovered = 0;
        for msg in &pending_messages {
            // Check if there's already an active card for this message
            if db
                .has_pending_card_for_message(&msg.id)
                .await
                .unwrap_or(false)
            {
                continue;
            }
            // No active card — create a placeholder card for the UI
            let card = ai_assist::cards::model::ApprovalCard::new_reply(
                &msg.channel,
                &msg.sender,
                &msg.content,
                "(pending re-generation)",
                0.0,
                &msg.sender,
                card_expire_min,
            )
            .with_message_id(&msg.id);
            card_queue.push(card).await;
            recovered += 1;
        }
        if recovered > 0 {
            eprintln!("   Recovered {} unanswered messages from DB", recovered);
        }
    }

    // Spawn card expiry sweep task (runs every 60s)
    let _expiry_handle = queue::spawn_expiry_task(card_queue.clone());

    // Build EmailConfig for the card server (so approve/edit can send replies)
    let email_config_for_cards = EmailConfig::from_env();

    // ── Agent Config (created early — Scheduler needs it) ──────────────
    let system_prompt = std::env::var("AI_ASSIST_SYSTEM_PROMPT")
        .ok()
        .or_else(|| Some(ai_assist::config::DEFAULT_SYSTEM_PROMPT.to_string()));

    let agent_config = AgentConfig {
        system_prompt,
        ..AgentConfig::default()
    };

    // ── Safety (shared between Scheduler and Agent) ──────────────────
    let safety = Arc::new(SafetyLayer::new());

    // ── Workspace ─────────────────────────────────────────────────────────
    let workspace_path = std::env::var("AI_ASSIST_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home).join(".ai-assist/workspace")
        });
    let workspace = Arc::new(Workspace::new(workspace_path.clone()));
    if let Err(e) = workspace.ensure_dirs().await {
        eprintln!("   Warning: Could not create workspace dirs: {}", e);
    }
    eprintln!("   Workspace: {}", workspace_path.display());

    // ── Tools ────────────────────────────────────────────────────────────
    let tools = Arc::new(ToolRegistry::new());
    // Shell + file tools
    tools.register_sync(Arc::new(ai_assist::tools::builtin::shell::ShellTool::new()));
    tools.register_sync(Arc::new(ai_assist::tools::builtin::file::ReadFileTool::new()));
    tools.register_sync(Arc::new(ai_assist::tools::builtin::file::WriteFileTool::new()));
    tools.register_sync(Arc::new(ai_assist::tools::builtin::file::ListDirTool::new()));
    tools.register_sync(Arc::new(ai_assist::tools::builtin::file::ApplyPatchTool::new()));
    // Memory tools
    tools.register_memory_tools(Arc::clone(&workspace));

    // ── Worker System (Scheduler + ContextManager) ───────────────────
    let (activity_tx, _activity_rx) = tokio::sync::broadcast::channel::<TodoActivityMessage>(256);
    let context_manager = Arc::new(ContextManager::new(agent_config.max_parallel_jobs));
    let scheduler = Arc::new(Scheduler::new(
        agent_config.clone(),
        Arc::clone(&context_manager),
        llm.clone(),
        Arc::clone(&safety),
        Arc::clone(&tools),
        Some(Arc::clone(&db)),
        activity_tx.clone(),
    ));
    eprintln!(
        "   Worker: enabled (max {} parallel jobs, {}s timeout)",
        agent_config.max_parallel_jobs,
        agent_config.job_timeout.as_secs(),
    );

    // ── Routine Engine ────────────────────────────────────────────────────
    let routine_config = RoutineConfig::from_env();
    let routine_engine = if routine_config.enabled {
        let (notify_tx, mut notify_rx) =
            tokio::sync::mpsc::channel::<ai_assist::channels::OutgoingResponse>(256);
        let engine = Arc::new(RoutineEngine::new(
            routine_config.clone(),
            Arc::clone(&db),
            llm.clone(),
            None, // Workspace not yet implemented
            notify_tx,
            Some(Arc::clone(&scheduler)),
        ));

        // Refresh event cache on startup
        engine.refresh_event_cache().await;

        // Spawn cron ticker
        let cron_interval = std::time::Duration::from_secs(routine_config.cron_interval_secs);
        let _cron_handle = routine_engine::spawn_cron_ticker(Arc::clone(&engine), cron_interval);

        // Spawn notification consumer (routes routine notifications to channels)
        tokio::spawn(async move {
            while let Some(response) = notify_rx.recv().await {
                tracing::info!(
                    routine_notification = %response.content.chars().take(100).collect::<String>(),
                    "Routine notification"
                );
                // TODO: Route to channel manager when available in this scope
            }
        });

        eprintln!(
            "   Routines: enabled (cron every {}s, max {} concurrent)",
            routine_config.cron_interval_secs, routine_config.max_concurrent_routines,
        );

        // Register routine tools (needs engine reference)
        tools.register_routine_tools(Arc::clone(&db), Arc::clone(&engine));

        Some(engine)
    } else {
        eprintln!("   Routines: disabled");
        None
    };

    eprintln!("   Tools: {} registered", tools.count());

    // ── Todo + Activity WebSocket State ──────────────────────────────
    let todo_state = TodoState::with_scheduler(Arc::clone(&db), Arc::clone(&scheduler));
    let activity_state = ActivityState::new(Arc::clone(&db), activity_tx.clone());

    // ── Todo Pickup Loop (auto-schedules AgentStartable todos) ──────
    let _pickup_handle = ai_assist::todos::pickup::spawn_todo_pickup_loop(
        Arc::clone(&db),
        Arc::clone(&scheduler),
        todo_state.tx.clone(),
    );
    eprintln!("   Todo pickup: enabled (every 15m, immediate on create)");

    // Create iOS channel (needs to exist before router build)
    let ios_channel = IosChannel::new(Some(Arc::clone(&db)));
    let ios_router = ios_channel.router();

    // Spawn Axum WS/REST server — cards + iOS chat + todos + activity
    let app = card_routes(
        card_queue.clone(),
        email_config_for_cards,
        card_generator.clone(),
    )
    .merge(ios_router)
    .merge(todo_routes(todo_state))
    .merge(activity_routes(activity_state));
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", ws_port))
            .await
            .expect("Failed to bind card server port");
        tracing::info!(port = ws_port, "Card WebSocket server started");
        axum::serve(listener, app).await.ok();
    });

    // ── Agent ───────────────────────────────────────────────────────────
    let llm_for_pipeline = llm.clone();
    let deps = AgentDeps {
        store: Some(Arc::clone(&db)),
        llm,
        safety,
        tools,
        workspace: Some(Arc::clone(&workspace)),
        extension_manager: None,
        card_generator: Some(card_generator),
        routine_engine,
    };

    // Set up channels
    let mut channels = ChannelManager::new();
    let mut active_channels = vec!["cli", "ios"];

    // Always add CLI
    channels.add(Box::new(CliChannel::new()));

    // Always add iOS (WebSocket chat at /ws/chat)
    channels.add(Box::new(ios_channel));

    // Conditionally add Telegram if bot token is set
    if let Ok(telegram_token) = std::env::var("TELEGRAM_BOT_TOKEN") {
        let allowed_users: Vec<String> = std::env::var("TELEGRAM_ALLOWED_USERS")
            .unwrap_or_else(|_| "*".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        eprintln!(
            "   Telegram: enabled (allowed: {})",
            if allowed_users.iter().any(|u| u == "*") {
                "everyone".to_string()
            } else {
                allowed_users.join(", ")
            }
        );

        channels.add(Box::new(TelegramChannel::new(
            telegram_token,
            allowed_users,
        )));
        active_channels.push("telegram");
    }

    // Conditionally add Email pipeline if IMAP host is set
    // Email no longer goes through the agent loop — it uses the standalone pipeline:
    //   IMAP poller → messages DB → email processor → pipeline → cards
    if let Some(email_config) = EmailConfig::from_env() {
        let senders = &email_config.allowed_senders;
        eprintln!(
            "   Email: enabled (IMAP: {}, SMTP: {}, allowed: {})",
            email_config.imap_host,
            email_config.smtp_host,
            if senders.iter().any(|s| s == "*") {
                "everyone".to_string()
            } else if senders.is_empty() {
                "none (deny all)".to_string()
            } else {
                senders.join(", ")
            }
        );

        // Spawn IMAP poller (persists to DB, marks \Seen)
        let (_poller_handle, _poller_shutdown) =
            ai_assist::channels::email_poller::spawn_email_poller(
                email_config.clone(),
                Arc::clone(&db),
            );

        // Create pipeline processor for emails
        let rules = ai_assist::pipeline::rules::RulesEngine::default_rules();
        let email_pipeline = Arc::new(ai_assist::pipeline::processor::MessageProcessor::new(
            llm_for_pipeline.clone(),
            card_queue.clone(),
            rules,
        ));

        // Spawn background email processor (timer-based)
        let (_processor_handle, _processor_shutdown) =
            ai_assist::pipeline::email_processor::spawn_email_processor(
                Arc::clone(&db),
                email_pipeline,
                None, // Uses EMAIL_PROCESS_INTERVAL_SECS env var or 2h default
            );

        active_channels.push("email (pipeline)");
    }

    eprintln!("   Channels: {}\n", active_channels.join(", "));

    let agent = Agent::new(agent_config, deps, channels, None);
    agent.run().await?;

    Ok(())
}
