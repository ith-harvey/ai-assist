#!/bin/bash
# Pulls ai-assist changes from remote dev server → local machine over Tailscale
# Run this on your LOCAL machine (the one with the phone)
#
# Required env vars:
#   STUDIO_IP    — Tailscale IP of the remote machine
#   STUDIO_USER  — SSH username on the remote machine
#
# Usage:
#   export STUDIO_IP=100.99.236.80
#   export STUDIO_USER=onlinegrocery
#   ./pull-from-studio.sh
#
# Polls every 2 seconds, rsync only transfers diffs so it's cheap
# Press Ctrl+C to stop

if [ -z "$STUDIO_IP" ] || [ -z "$STUDIO_USER" ]; then
  echo "❌ Missing env vars. Set STUDIO_IP and STUDIO_USER first:"
  echo "   export STUDIO_IP=100.99.236.80"
  echo "   export STUDIO_USER=onlinegrocery"
  exit 1
fi

REMOTE_PATH="~/Projects/ai-assist/"
LOCAL_PATH="$HOME/Projects/ai-assist/"

mkdir -p "$LOCAL_PATH"

echo "🔄 Pulling from $STUDIO_USER@$STUDIO_IP:$REMOTE_PATH"
echo "📂 Into $LOCAL_PATH"
echo "⏱  Polling every 2s — Ctrl+C to stop"
echo ""

while true; do
  rsync -avz --delete \
    --exclude '.git' \
    --exclude '.build' \
    --exclude 'DerivedData' \
    --exclude 'build' \
    --exclude 'target' \
    "$STUDIO_USER@$STUDIO_IP:$REMOTE_PATH" "$LOCAL_PATH"
  sleep 2
done
