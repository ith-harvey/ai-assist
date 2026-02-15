use std::sync::Arc;

use ai_assist::agent::{Agent, AgentDeps};
use ai_assist::cards::generator::{CardGenerator, GeneratorConfig};
use ai_assist::cards::queue::{self, CardQueue};
use ai_assist::cards::ws::card_routes;
use ai_assist::channels::{ChannelManager, CliChannel, TelegramChannel};
use ai_assist::config::AgentConfig;
use ai_assist::llm::{create_provider, LlmBackend, LlmConfig};
use ai_assist::safety::SafetyLayer;
use ai_assist::tools::ToolRegistry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let model = std::env::var("AI_ASSIST_MODEL")
        .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

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
    eprintln!("   Card API: http://0.0.0.0:{}/api/cards", ws_port);
    eprintln!("   Type a message and press Enter. /quit to exit.\n");

    // Create LLM provider
    let llm_config = LlmConfig {
        backend: LlmBackend::Anthropic,
        api_key: secrecy::SecretString::from(api_key),
        model,
    };
    let llm = create_provider(&llm_config)?;

    // ── Card System ─────────────────────────────────────────────────────
    let card_queue = CardQueue::new();

    let generator_config = GeneratorConfig {
        expire_minutes: card_expire_min,
        ..Default::default()
    };
    let _card_generator = Arc::new(CardGenerator::new(
        llm.clone(),
        card_queue.clone(),
        generator_config,
    ));

    // Spawn card expiry sweep task (runs every 60s)
    let _expiry_handle = queue::spawn_expiry_task(card_queue.clone());

    // Spawn Axum WS/REST server for cards
    let app = card_routes(card_queue.clone());
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", ws_port))
            .await
            .expect("Failed to bind card server port");
        tracing::info!(port = ws_port, "Card WebSocket server started");
        axum::serve(listener, app).await.ok();
    });

    // ── Agent ───────────────────────────────────────────────────────────
    let deps = AgentDeps {
        store: None,
        llm,
        safety: Arc::new(SafetyLayer::new()),
        tools: Arc::new(ToolRegistry::new()),
        workspace: None,
        extension_manager: None,
    };

    // Set up channels
    let mut channels = ChannelManager::new();
    let mut active_channels = vec!["cli"];

    // Always add CLI
    channels.add(Box::new(CliChannel::new()));

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

        channels.add(Box::new(TelegramChannel::new(telegram_token, allowed_users)));
        active_channels.push("telegram");
    }

    eprintln!("   Channels: {}\n", active_channels.join(", "));

    // Create and run agent
    let config = AgentConfig::default();
    let agent = Agent::new(config, deps, channels, None);
    agent.run().await?;

    Ok(())
}
