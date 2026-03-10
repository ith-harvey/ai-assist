#!/usr/bin/env bash
# Dev launcher for AI Assist — nuke DB, build, run, seed.
# Usage: ./dev.sh [port]
#   default port: 8080

set -euo pipefail

PORT="${1:-8080}"
DB_PATH="./data/ai-assist.db"

echo "🔧 AI Assist — Dev Mode"
echo ""

# ── Step 0: Source environment ─────────────────────────────────────
if [ -f .env ]; then
  echo "📋 Loading .env"
  set -a; source .env; set +a
fi

# ── Step 1: Nuke the database ──────────────────────────────────────
if [ -f "$DB_PATH" ]; then
  echo "🗑  Removing old database: ${DB_PATH}"
  rm -f "$DB_PATH"
fi
mkdir -p "$(dirname "$DB_PATH")"

# ── Step 2: Build ──────────────────────────────────────────────────
echo "🔨 Building..."
cargo build 2>&1 | tail -1
echo ""

# ── Step 3: Start server in background ─────────────────────────────
echo "🚀 Starting server on port ${PORT}..."
AI_ASSIST_WS_PORT="$PORT" AI_ASSIST_DB_PATH="$DB_PATH" cargo run &
SERVER_PID=$!

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

# ── Step 4: Seed ───────────────────────────────────────────────────
echo "🌱 Seeding database..."
./seed.sh localhost "$PORT"

# ── Step 5: Foreground the server ──────────────────────────────────
echo ""
LAN_IP=""
for iface in en0 en1 en2 en3 en4 en5 en6 en7 en8; do
  ip=$(ipconfig getifaddr "$iface" 2>/dev/null)
  if [ -n "$ip" ]; then
    LAN_IP="$ip"
    break
  fi
done

echo "════════════════════════════════════════════════════"
echo "  AI Assist running on http://localhost:${PORT}"
if [ -n "$LAN_IP" ]; then
echo "  Remote:      http://${LAN_IP}:${PORT}"
fi
echo "  Cards WS:    ws://localhost:${PORT}/ws"
echo "  Chat WS:     ws://localhost:${PORT}/ws/chat"
echo "  Todos WS:    ws://localhost:${PORT}/ws/todos"
echo "  Activity WS: ws://localhost:${PORT}/ws/todos/:id/activity"
echo "  Ctrl+C to stop"
echo "════════════════════════════════════════════════════"
echo ""

# Bring server to foreground (Ctrl+C kills it)
wait "$SERVER_PID"
