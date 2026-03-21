# Feature Specs

> Registry of feature specification documents. Each spec lives in `docs/features/<name>.md`.

| Feature | File | Status | Summary |
|---|---|---|---|
| Next Steps Approval Queue | [next-steps-approval-queue.md](next-steps-approval-queue.md) | shipped | Continuous swipe-through queue for all pending approval cards across silos |
| Voice Recording UX | [voice-recording.md](voice-recording.md) | shipped | Push-to-talk mic button with orange glow, on-device transcription, and 2s trailing audio buffer |
| Dev Mode Server Prompt | [dev-mode-server-prompt.md](dev-mode-server-prompt.md) | planned | Always show server address screen in Debug builds; pre-fill last address; print LAN IP in dev.sh |
| Messages View Filtering | [messages-view-filtering.md](messages-view-filtering.md) | planned | Filter Messages tab to only show message drafts (reply/compose); hide agent action cards |
| Task Deliverables | [task-deliverables.md](task-deliverables.md) | in-progress | Rename Documents to Deliverables; add message-type deliverables with approval card integration |
| Calendar Google Calendar Setup | [calendar-google-calendar-setup.md](calendar-google-calendar-setup.md) | planned | Setup flow to connect Google Calendar via OAuth; connected placeholder state |
| Todo Agent Workflow | [todo-agent-workflow.md](todo-agent-workflow.md) | in-progress | End-to-end todo agent lifecycle, activity streaming, and parallel execution (multiple agents on different todos simultaneously) |
| AI Status Overlay | [ai-status-overlay.md](ai-status-overlay.md) | planned | Background strip + spring animation for AI status above input bar; hidden on Brain tab |
| Calendar Daily View | [calendar-daily-view.md](calendar-daily-view.md) | in-progress | Google Calendar-style daily timeline with swipe navigation; AI agent tools for create/update/delete events |
| Todo List Tab Filtering | [todo-list-tab-filtering.md](todo-list-tab-filtering.md) | planned | Segmented control to filter todo list by Active / Snoozed / Completed instead of scrollable sections |
| Unified Approval Card UX | [unified-approval-card-ux.md](unified-approval-card-ux.md) | in-progress | Unify Messages and Todo approval card UX via generic parent view hierarchy; ContentView embeds ApprovalQueueView |
| Unified TodoWebSocket Lifecycle | [unified-todo-websocket-lifecycle.md](unified-todo-websocket-lifecycle.md) | planned | Lift TodoWebSocket to MainTabView as single shared instance; fix todos disappearing after creation |
| Universal Todo Activity Thread | [universal-todo-activity-thread.md](universal-todo-activity-thread.md) | planned | Show activity feed on all todos; chatting with a human todo spawns an advisor agent in the activity thread |
