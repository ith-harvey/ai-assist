#!/bin/bash
# Syncs ai-assist changes from Mac Studio → Laptop over Tailscale
# Usage: ./sync-to-laptop.sh
#
# Prerequisites:
#   brew install fswatch
#   SSH access to laptop via Tailscale (ssh keys recommended)
#
# TODO: Replace LAPTOP with your laptop's Tailscale hostname or IP
#       Replace the remote path if it differs from ~/Projects/ai-assist

LAPTOP="100.85.206.118"
REMOTE_PATH="~/Projects/ai-assist"
LOCAL_PATH="$HOME/Projects/ai-assist"

echo "🔄 Watching $LOCAL_PATH for changes..."
echo "📡 Syncing to $LAPTOP:$REMOTE_PATH"
echo "Press Ctrl+C to stop"
echo ""

# Initial sync
rsync -avz --delete \
  --exclude '.git' \
  --exclude '.build' \
  --exclude 'DerivedData' \
  --exclude 'build' \
  "$LOCAL_PATH/" "$LAPTOP:$REMOTE_PATH/"

echo "✅ Initial sync done. Watching for changes..."

# Watch and sync on change
fswatch -o "$LOCAL_PATH" \
  --exclude '\.git' \
  --exclude '\.build' \
  --exclude 'DerivedData' \
  | while read; do
    rsync -avz --delete \
      --exclude '.git' \
      --exclude '.build' \
      --exclude 'DerivedData' \
      --exclude 'build' \
      "$LOCAL_PATH/" "$LAPTOP:$REMOTE_PATH/"
    echo "✅ Synced at $(date +%H:%M:%S)"
done
