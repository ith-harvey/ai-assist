#!/usr/bin/env bash
# Dev launcher for AI Assist — nuke DB, build, run, seed.
# Usage: ./dev.sh [port]
#   If no port given, auto-assigns from range 8080-8089.
#   Each session gets its own port via lock files in data/ports/.

set -euo pipefail

PORTS_DIR="./data/ports"
mkdir -p "$PORTS_DIR"

# ── Step 0: Source environment ─────────────────────────────────────
if [ -f .env ]; then
  echo "📋 Loading .env"
  set -a; source .env; set +a
fi

# ── Step 1: Determine port ────────────────────────────────────────
if [ -n "${1:-}" ]; then
  PORT="$1"
else
  # Auto-assign: find first available port in range
  PORT=""
  for p in $(seq 8080 8089); do
    LOCK="$PORTS_DIR/$p.lock"
    if [ -f "$LOCK" ]; then
      LOCKED_PID=$(cat "$LOCK" 2>/dev/null || true)
      if [ -n "$LOCKED_PID" ] && kill -0 "$LOCKED_PID" 2>/dev/null; then
        continue  # port in use by a live process
      fi
      # Stale lock — clean it up
      rm -f "$LOCK"
    fi
    PORT="$p"
    break
  done
  if [ -z "$PORT" ]; then
    echo "❌ No available ports in range 8080-8089. Kill an existing server first."
    exit 1
  fi
fi

DB_PATH="./data/ai-assist-${PORT}.db"

echo "🔧 AI Assist — Dev Mode (port ${PORT})"
echo ""

# ── Step 2: Kill existing server on this port ─────────────────────
EXISTING_PID=$(lsof -ti :"$PORT" 2>/dev/null || true)
if [ -n "$EXISTING_PID" ]; then
  echo "🛑 Killing existing process on port ${PORT} (PID: ${EXISTING_PID})"
  kill $EXISTING_PID 2>/dev/null || true
  sleep 1
fi

# ── Step 3: Nuke the database ──────────────────────────────────────
if [ -f "$DB_PATH" ]; then
  echo "🗑  Removing old database: ${DB_PATH}"
  rm -f "$DB_PATH"
fi
mkdir -p "$(dirname "$DB_PATH")"

# ── Step 4: Build ──────────────────────────────────────────────────
echo "🔨 Building..."
cargo build 2>&1 | tail -1
echo ""

# ── Step 5: Start server in background ─────────────────────────────
echo "🚀 Starting server on port ${PORT}..."
AI_ASSIST_WS_PORT="$PORT" AI_ASSIST_DB_PATH="$DB_PATH" cargo run &
SERVER_PID=$!

# Write lock file and .dev-port for session discovery
echo "$SERVER_PID" > "$PORTS_DIR/$PORT.lock"
echo "$PORT" > .dev-port

# Clean up lock file on exit
cleanup() {
  rm -f "$PORTS_DIR/$PORT.lock" .dev-port
}
trap cleanup EXIT

# Wait for server to be ready
echo -n "   Waiting for server"
for i in $(seq 1 30); do
  if curl -s "http://localhost:${PORT}/api/cards" >/dev/null 2>&1; then
    echo " ✅"
    break
  fi
  echo -n "."
  sleep 1
  if [ "$i" -eq 30 ]; then
    echo " ❌ Timeout — server didn't start in 30s"
    kill "$SERVER_PID" 2>/dev/null
    exit 1
  fi
done

echo ""

# ── Step 6: Seed ───────────────────────────────────────────────────
echo "🌱 Seeding database..."
./seed.sh localhost "$PORT"

# ── Step 7: Foreground the server ──────────────────────────────────
echo ""
echo "════════════════════════════════════════════════════"
echo "  AI Assist running on http://localhost:${PORT}"
echo "  Cards WS:    ws://localhost:${PORT}/ws"
echo "  Chat WS:     ws://localhost:${PORT}/ws/chat"
echo "  Todos WS:    ws://localhost:${PORT}/ws/todos"
echo "  Activity WS: ws://localhost:${PORT}/ws/todos/:id/activity"
echo "  Ctrl+C to stop"
echo "════════════════════════════════════════════════════"
echo ""

# Bring server to foreground (Ctrl+C kills it)
wait "$SERVER_PID"
