# User Context

## Who I Am

- **Name:** Ian Harvey
- **Pronouns:** he/him
- **Age:** 34
- **Location:** Las Vegas, NV (Pacific Time)
- **GitHub:** [ith-harvey](https://github.com/ith-harvey)

## What I'm Building

- **AI Assist** — A personal AI assistant with an iOS app (SwiftUI), Rust backend (Axum + libsql), and a multi-agent worker system. The core loop: ingest messages from email/chat/etc → generate approval cards → I swipe to approve/dismiss → agents execute work → todos track everything.
- **Mission Control** — Task management and orchestration dashboard for AI agents. Convex backend, React/Vite/Tailwind frontend. Kanban board, live feed, agent status, PR tracking.
- **M0** — Stablecoin protocol. My day job.

## How I Work

- **Ship first, polish second.** Working beats perfect. But sloppy ≠ fast — clean code matters.
- **Direct communication.** Don't hedge, don't over-explain, don't sugarcoat. Say what you mean.
- **Don't oversimplify.** I'm a programmer — talk to me like one. Skip the tutorials.
- **Read before you write.** Understand existing patterns and match them. Don't reinvent what's already there.
- **Small, focused changes.** One concern per PR. Clear commit messages. No drive-by refactors.
- **I review iOS/Swift myself** (`manual-rev`). Rex handles infrastructure and backend reviews (`agent-rev`).

## What I Value

- **Resourcefulness** — figure it out, look it up, try things. Don't ask me what you can find yourself.
- **Thoughtfulness** — think through edge cases and tradeoffs before shipping.
- **Curiosity** — I care about how things work under the hood, not just that they work.
- **Speed with intention** — move fast but know why you're making each decision.

## Technical Preferences

- **Rust:** Axum for HTTP/WS, libsql/SQLite for storage, tokio for async, serde for serialization. Avoid unnecessary dependencies.
- **Swift/iOS:** SwiftUI, `@Observable` (not ObservableObject), native APIs over third-party libs. `.searchable`, `.swipeActions`, `NavigationStack` — use the platform.
- **TypeScript/React:** Convex for backend, Tailwind for styling, Vite for bundling. Keep components focused.
- **Git:** Feature branches, never commit to main, worktrees for mission-control, direct branches for ai-assist.
- **Testing:** Test where it matters — data layers, serialization, edge cases. Don't write tests for the sake of coverage.

## Personal Context

- **Partner:** Christina
- **Creative projects:** Slasher film production (Atlanta shoot)
- **Interests:** AI research, agent architectures, Karpathy, Lex Fridman podcast
- **Runs a team of AI agents:** Codie-1, Codie-2, Codie-3 (developers), Clark (local Ollama), Rex (reviewer), Libby (researcher), Jarvis (orchestrator) — all coordinated through OpenClaw
