# AI Assist

Server-side AI agent that manages your messaging. Connects to Telegram (WhatsApp coming soon), generates smart reply suggestions via Claude, and presents them as swipeable cards on iOS.

You never type — just swipe to approve, dismiss, or edit.

## UX North Star

These principles guide every design decision. When in doubt, refer here.

1. **Human UX over functionality.** This system will always trade feature count for accurate responses and useful suggestions. We'd rather do three things flawlessly than ten things with rough edges. Every interaction the user has should feel considered and intentional.

2. **No configuration battles.** The user should never find themselves debugging agent behavior, tweaking prompts, or fighting a system that's spinning out. The harness robustly handles irrational agent output, confused context, and edge cases — silently, without surfacing the mess. If something breaks, the system recovers; the user never notices.

3. **Less is more.** The user can do fewer things on this system than on others — and that's the point. What they *can* do feels flawless and bugless. No half-built features, no "works sometimes" flows. Every surface is polished.

4. **It just works. It feels like magic.** The user is generally unaware of the multiple background models, the failover logic, the context compaction, the safety checks. It all disappears behind a single experience that feels like the system is reading their mind. The suggestions arrive at the right time, with the right tone, about the right thing.

5. **Proactive by default.** The default interaction is the system prompting the human — not the other way around. The system surfaces suggested actions, draft replies, and nudges before the user asks. The user's primary job is to approve, dismiss, or adjust — not to initiate. The system drives; the human steers.

## Architecture

```
┌─────────────┐     ┌──────────────────────────────────┐     ┌──────────────┐
│  Telegram    │────▶│         AI Assist Server          │◀───│  iOS Client  │
│  (Bot API)   │◀────│                                    │───▶│  (SwiftUI)   │
└─────────────┘     │  ┌────────────┐  ┌─────────────┐  │     └──────────────┘
                    │  │ Agent Loop  │  │ Card System  │  │          ▲
                    │  │ (LLM+Tools) │  │ (Queue+WS)   │  │          │
                    │  └──────┬─────┘  └──────┬──────┘  │      WebSocket
                    │         │               │          │     /api/cards
                    │         ▼               ▼          │
                    │     ┌───────┐    ┌──────────┐     │
                    │     │Claude │    │ Broadcast │     │
                    │     │(Anthropic) │ (tokio)   │     │
                    │     └───────┘    └──────────┘     │
                    └──────────────────────────────────┘
```

## Quick Start

```bash
# Clone
git clone https://github.com/ith-harvey/ai-assist.git
cd ai-assist

# Configure
cat > .env << 'EOF'
export ANTHROPIC_API_KEY=sk-ant-...
export AI_ASSIST_MODEL=claude-sonnet-4-20250514
export TELEGRAM_BOT_TOKEN=123456:ABC...        # optional
export TELEGRAM_ALLOWED_USERS=your_username     # optional, default: *
EOF

# Run
source .env && cargo run
```

## Features

### Channels
- **CLI** — stdin/stdout REPL for development
- **Telegram** — Bot API with long-polling, typing indicators, message splitting, rich media (photos, documents, audio, video, voice)

### Agent Loop
- Full agentic loop: LLM call → tool execution → repeat
- Tool approval flow (approve/reject/always-approve per session)
- Context auto-compaction when window gets full
- Undo/redo with checkpoints
- Session management with thread isolation
- `/compact`, `/clear`, `/undo`, `/redo`, `/quit` commands

### Card System
- Reply suggestion cards generated via separate LLM call
- In-memory queue with `tokio::broadcast` fan-out
- WebSocket server for real-time card streaming
- REST API for card management
- Auto-expiry (configurable, default 15 min)

### LLM Provider
- Anthropic (Claude) and OpenAI via `rig-core`
- Multi-provider failover
- Retry with exponential backoff + jitter
- Token cost tracking

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | ✅ | — | Anthropic API key |
| `AI_ASSIST_MODEL` | — | `claude-sonnet-4-20250514` | Model to use |
| `AI_ASSIST_SYSTEM_PROMPT` | — | Built-in conversational prompt | Custom system prompt |
| `TELEGRAM_BOT_TOKEN` | — | — | Telegram bot token from @BotFather |
| `TELEGRAM_ALLOWED_USERS` | — | `*` | Comma-separated usernames or user IDs |
| `AI_ASSIST_WS_PORT` | — | `8080` | WebSocket/REST server port |
| `AI_ASSIST_CARD_EXPIRE_MIN` | — | `15` | Card expiry in minutes |

## API Endpoints

### WebSocket
```
ws://localhost:8080/ws
```
Streams: `new_card`, `card_update`, `card_expired`, `cards_sync`

### REST
```
GET  /api/cards              — List pending cards
POST /api/cards/:id/approve  — Approve a card
POST /api/cards/:id/dismiss  — Dismiss a card
POST /api/cards/:id/edit     — Edit card text (JSON body: {"text": "..."})
```

## Project Structure

```
src/
├── main.rs                 # Entry point — wires everything together
├── lib.rs                  # Module declarations
├── config.rs               # AgentConfig + defaults
├── context.rs              # JobContext for tool execution
├── db.rs                   # Database trait (stub)
├── error.rs                # Error types
├── extensions.rs           # Extension manager (stub)
├── safety.rs               # Safety layer (no-op stub)
├── workspace.rs            # Workspace (stub)
├── agent/
│   ├── agent_loop.rs       # Core agent: run(), handle_message(), agentic loop
│   ├── session.rs          # Session, Thread, Turn, PendingApproval
│   ├── session_manager.rs  # Session lifecycle + thread resolution
│   ├── compaction.rs       # Context window compaction via LLM summarization
│   ├── context_monitor.rs  # Context usage tracking + compaction triggers
│   ├── submission.rs       # Input parser (/commands, approvals, etc.)
│   ├── router.rs           # Command routing
│   └── undo.rs             # Undo/redo with checkpoints
├── cards/
│   ├── model.rs            # ReplyCard, CardStatus, CardAction, WsMessage
│   ├── queue.rs            # CardQueue with broadcast fan-out
│   ├── generator.rs        # LLM-powered reply suggestion generation
│   └── ws.rs               # Axum WebSocket + REST endpoints
├── channels/
│   ├── channel.rs          # Channel trait, IncomingMessage, OutgoingResponse
│   ├── manager.rs          # Multi-channel routing + stream merging
│   ├── cli.rs              # stdin/stdout REPL
│   └── telegram.rs         # Telegram Bot API (long-polling)
├── llm/
│   ├── provider.rs         # LlmProvider trait, ChatMessage, ToolCall types
│   ├── reasoning.rs        # Reasoning engine (respond_with_tools, plan, evaluate)
│   ├── rig_adapter.rs      # rig-core → LlmProvider bridge
│   ├── costs.rs            # Token cost lookup tables
│   ├── retry.rs            # Exponential backoff with jitter
│   └── failover.rs         # Multi-provider failover
└── tools/
    ├── tool.rs             # Tool trait, ToolOutput, ToolDomain
    └── registry.rs         # ToolRegistry (register, lookup, definitions)
```

## Roadmap

- [x] Agent loop (LLM + tool execution cycle)
- [x] LLM provider (Anthropic + OpenAI)
- [x] CLI channel
- [x] Telegram channel
- [x] System prompt + conversational behavior
- [x] Card system + WebSocket server
- [ ] Wire card generation into message flow
- [ ] Ghost detection (unanswered message alerts)
- [ ] iOS client (SwiftUI)
- [ ] WhatsApp integration
- [ ] Persistent storage (SQLite)
- [ ] Calendar + email bridges

## Stats

- **~12k lines** of Rust across 38 files
- **241 tests** passing
- Zero unsafe code

## License

Private — not open source.
