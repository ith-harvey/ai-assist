#!/usr/bin/env bash
#
# dev-loop.sh — Pull, build, and relaunch AI Assist in the iOS Simulator
#
# Watches the git remote for changes on the current branch.
# When new commits arrive: pulls, rebuilds, installs, and relaunches the app.
#
# Usage:
#   ./dev-loop.sh              # defaults: poll every 10s, iPhone 17 Pro sim
#   ./dev-loop.sh --interval 5 # poll every 5 seconds
#   ./dev-loop.sh --device "iPhone 16e"
#   ./dev-loop.sh --once       # pull + build + launch once, then exit
#
set -euo pipefail

# ─── Config ───────────────────────────────────────────────────────────
REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_REL="ios/AIAssistApp/AIAssistApp.xcodeproj"
SCHEME="AIAssistApp"
CONFIGURATION="Debug"
BUNDLE_ID="theassist.AIAssistApp"
POLL_INTERVAL=15
DEVICE_NAME="iPhone 17 Pro"
ONCE=false

# ─── Parse args ───────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --interval) POLL_INTERVAL="$2"; shift 2 ;;
    --device)   DEVICE_NAME="$2";   shift 2 ;;
    --scheme)   SCHEME="$2";        shift 2 ;;
    --bundle)   BUNDLE_ID="$2";     shift 2 ;;
    --once)     ONCE=true;          shift   ;;
    -h|--help)
      echo "Usage: $0 [--interval N] [--device NAME] [--scheme NAME] [--bundle ID] [--once]"
      exit 0 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

PROJECT="$REPO_DIR/$PROJECT_REL"

# ─── Colors ───────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${CYAN}[$(date +%H:%M:%S)]${NC} $*"; }
ok()   { echo -e "${GREEN}[$(date +%H:%M:%S)] ✅ $*${NC}"; }
warn() { echo -e "${YELLOW}[$(date +%H:%M:%S)] ⚠️  $*${NC}"; }
err()  { echo -e "${RED}[$(date +%H:%M:%S)] ❌ $*${NC}"; }

# ─── Helpers ──────────────────────────────────────────────────────────
get_sim_udid() {
  xcrun simctl list devices available | grep "$DEVICE_NAME" | head -1 \
    | sed -E 's/.*\(([A-F0-9-]+)\).*/\1/'
}

ensure_sim_booted() {
  local udid="$1"
  local state
  state=$(xcrun simctl list devices | grep "$udid" | sed -E 's/.*\((Booted|Shutdown)\).*/\1/')
  if [[ "$state" != "Booted" ]]; then
    log "Booting simulator ($DEVICE_NAME)..."
    xcrun simctl boot "$udid" 2>/dev/null || true
    # Open Simulator.app so you can see it
    open -a Simulator
    sleep 3
  fi
}

get_local_head() {
  git -C "$REPO_DIR" rev-parse HEAD
}

get_remote_head() {
  local branch
  branch=$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD)
  git -C "$REPO_DIR" fetch origin "$branch" --quiet 2>/dev/null
  git -C "$REPO_DIR" rev-parse "origin/$branch"
}

pull_changes() {
  local branch
  branch=$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD)
  log "Pulling latest from origin/$branch..."
  git -C "$REPO_DIR" pull --ff-only origin "$branch"
}

build_app() {
  local udid="$1"
  local derived_data="$REPO_DIR/ios/.build/DerivedData"
  
  log "Building $SCHEME (${CONFIGURATION})..."
  
  xcodebuild \
    -project "$PROJECT" \
    -scheme "$SCHEME" \
    -configuration "$CONFIGURATION" \
    -destination "platform=iOS Simulator,id=$udid" \
    -derivedDataPath "$derived_data" \
    -quiet \
    build 2>&1 | tail -5

  local exit_code=${PIPESTATUS[0]}
  if [[ $exit_code -ne 0 ]]; then
    err "Build failed (exit $exit_code)"
    return 1
  fi
  ok "Build succeeded"
  
  # Find the .app bundle
  APP_PATH=$(find "$derived_data/Build/Products/${CONFIGURATION}-iphonesimulator" \
    -name "*.app" -maxdepth 1 | head -1)
  
  if [[ -z "$APP_PATH" ]]; then
    err "Could not find .app bundle in DerivedData"
    return 1
  fi
}

start_ssh_tunnel() {
  if lsof -i :8080 -sTCP:LISTEN &>/dev/null; then
    return 0
  fi
  log "Opening SSH tunnel to 100.99.236.80:8080..."
  ssh -N -o ServerAliveInterval=10 -o ServerAliveCountMax=3 \
    -o ConnectTimeout=5 -o ExitOnForwardFailure=yes \
    -L 8080:localhost:8080 onlinegrocery@100.99.236.80 &
  SSH_TUNNEL_PID=$!
  sleep 1
  if kill -0 "$SSH_TUNNEL_PID" 2>/dev/null; then
    ok "SSH tunnel established (PID $SSH_TUNNEL_PID)"
  else
    warn "SSH tunnel failed to start — continuing without it"
    SSH_TUNNEL_PID=""
  fi
}

ensure_ssh_tunnel() {
  # If we have a PID, check if it's still alive
  if [[ -n "${SSH_TUNNEL_PID:-}" ]] && kill -0 "$SSH_TUNNEL_PID" 2>/dev/null; then
    return 0
  fi
  # Tunnel died or was never started — try to reconnect
  if [[ -n "${SSH_TUNNEL_PID:-}" ]]; then
    warn "SSH tunnel lost — reconnecting..."
    SSH_TUNNEL_PID=""
  fi
  start_ssh_tunnel
}

install_and_launch() {
  local udid="$1"
  
  # Kill existing instance
  xcrun simctl terminate "$udid" "$BUNDLE_ID" 2>/dev/null || true
  sleep 0.5
  
  # Install
  log "Installing $APP_PATH..."
  xcrun simctl install "$udid" "$APP_PATH"
  ok "Installed"
  
  # Launch
  log "Launching $BUNDLE_ID..."
  xcrun simctl launch "$udid" "$BUNDLE_ID"
  ok "Launched in simulator"
}

do_build_cycle() {
  local udid="$1"
  
  echo ""
  log "═══════════════════════════════════════"
  log "  New changes detected — rebuilding"
  log "═══════════════════════════════════════"
  
  pull_changes
  
  if build_app "$udid"; then
    install_and_launch "$udid"
    echo ""
    ok "App is running! Watching for changes..."
    echo ""
  else
    err "Build failed — will retry on next change"
  fi
}

# ─── Main ─────────────────────────────────────────────────────────────
main() {
  log "AI Assist Dev Loop"
  log "  Repo:     $REPO_DIR"
  log "  Project:  $PROJECT_REL"
  log "  Scheme:   $SCHEME"
  log "  Device:   $DEVICE_NAME"
  log "  Interval: ${POLL_INTERVAL}s"
  echo ""
  
  # Start SSH tunnel for backend access
  SSH_TUNNEL_PID=""
  start_ssh_tunnel

  # Resolve simulator
  local udid
  udid=$(get_sim_udid)
  if [[ -z "$udid" ]]; then
    err "No simulator found matching '$DEVICE_NAME'"
    echo "Available:"
    xcrun simctl list devices available | grep -i iphone
    exit 1
  fi
  log "Simulator UDID: $udid"
  
  # Boot sim
  ensure_sim_booted "$udid"
  ok "Simulator ready"
  
  # Initial build
  APP_PATH=""
  if build_app "$udid"; then
    install_and_launch "$udid"
    ok "Initial build complete — app running"
  else
    warn "Initial build failed — will watch for fixes"
  fi
  
  if [[ "$ONCE" == true ]]; then
    ok "Single run complete (--once)"
    exit 0
  fi
  
  echo ""
  log "Watching for changes (Ctrl+C to stop)..."
  echo ""
  
  local last_head
  last_head=$(get_local_head)
  
  while true; do
    sleep "$POLL_INTERVAL"

    # Reconnect SSH tunnel if it dropped
    ensure_ssh_tunnel

    local remote_head
    remote_head=$(get_remote_head 2>/dev/null) || {
      warn "Failed to fetch remote — retrying..."
      continue
    }
    
    if [[ "$remote_head" != "$last_head" ]]; then
      local new_commits
      new_commits=$(git -C "$REPO_DIR" log --oneline "$last_head..$remote_head" 2>/dev/null | wc -l | tr -d ' ')
      log "📦 $new_commits new commit(s) detected"
      
      do_build_cycle "$udid"
      last_head=$(get_local_head)
    fi
  done
}

cleanup() {
  echo ""
  if [[ -n "${SSH_TUNNEL_PID:-}" ]] && kill -0 "$SSH_TUNNEL_PID" 2>/dev/null; then
    log "Closing SSH tunnel (PID $SSH_TUNNEL_PID)..."
    kill "$SSH_TUNNEL_PID" 2>/dev/null || true
  fi
  log "Stopped."
  exit 0
}
trap cleanup INT TERM
main
