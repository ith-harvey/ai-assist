## Feature Development Workflow

All new feature work and bug fixes MUST use git worktrees for isolation. This allows multiple Claude Code sessions to work on different features in parallel without conflicts.

### Starting Feature Work

When asked to work on a new feature, fix, or task:
1. Use the EnterWorktree tool with a descriptive name matching the feature (e.g., `add-dark-mode`, `fix-login-bug`)
2. Copy `.env` into the worktree: `cp /Users/onlinegrocery/Projects/ai-assist/.env .env`
3. Create a feature branch within the worktree
4. Complete all development within the worktree
5. Commit and push from the worktree

### Why Worktrees

- Each Claude Code session gets its own isolated working directory
- No merge conflicts between parallel sessions
- All worktrees share the same git history
- Clean separation of concerns per feature

### When NOT to Use Worktrees

- Quick one-line fixes or typo corrections
- Reading/exploring code without making changes
- If the user explicitly says to work in the current directory

### Cleanup

On completion, remind the user they can clean up with:
`git worktree remove .claude/worktrees/<name>`
Or list active worktrees with: `git worktree list`

## iOS Testing (Required)

After ANY code change that touches the iOS client (`ios/` directory), you MUST run the Swift test suite:

```bash
cd ios && swift test
```

This runs all unit tests in `ios/Tests/AIAssistClientTests/` via Swift Package Manager. The test suite covers:
- **Model serialization**: JSON encode/decode for all models (TodoItem, CalendarEvent, Document, ActivityMessage, ApprovalCard, etc.)
- **Enum properties**: Labels, icons, colors, and computed properties for all enum types
- **WebSocket state**: Card and todo WebSocket message parsing, action encoding, state management
- **UX logic**: Card queue progression, approval flows, deliverable item routing

### When writing new iOS code

- **New models/enums**: Add corresponding tests in `ios/Tests/AIAssistClientTests/` covering JSON decode, computed properties, and edge cases
- **Modified models**: Update existing tests to cover the changes and verify nothing regresses
- **WebSocket changes**: Test message decoding and action encoding
- Follow TDD: write the test first, then implement

### Test runner script

Use `./test-ios.sh` for comprehensive testing:
```bash
./test-ios.sh              # SPM unit tests only (fast)
./test-ios.sh --ui         # Unit tests + UI tests in simulator
./test-ios.sh --all        # Unit + UI tests + build validation
```

### UI Tests

XCUITest scaffolding lives in `ios/AIAssistApp/AIAssistAppUITests/`. To activate:
1. Open `AIAssistApp.xcodeproj` in Xcode
2. File → New → Target → UI Testing Bundle → name it `AIAssistAppUITests`
3. The test files auto-sync from the `AIAssistAppUITests/` directory

## Visual Testing

After any code change that touches iOS UI, you MUST visually verify the changes:

1. **Start the server**: `./dev.sh` (auto-assigns a unique port from 8080-8089, clears DB, seeds test data). Read `.dev-port` to get the assigned port.
2. **Build and run the app**: Use `build_run_sim` with scheme `AIAssistApp` on an iPhone simulator
3. **Configure the app**: Open Settings (gear icon) in the app and set the port to match your `.dev-port` value
4. **Verify the UI**: Take screenshots and tap/swipe through affected screens to confirm the changes look correct
5. **Check for regressions**: Navigate to related screens to ensure nothing else broke visually

### Parallel Sessions

Multiple Claude Code sessions can run simultaneously. Each session gets its own server port and database automatically via `./dev.sh`. Port locks are stored in `data/ports/` and cleaned up on exit.
