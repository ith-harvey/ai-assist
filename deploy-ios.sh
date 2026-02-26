#!/bin/bash
# Build and deploy AI Assist iOS app to Ian's iPhone
set -e

REPO_ROOT="$(cd "$(dirname "$0")" && pwd -P)"
DEVICE_ID="00008110-000E50611451A01E"
TEAM_ID="TQBXL45729"
DERIVED_DATA="$REPO_ROOT/ios/.build/DerivedData"

echo "🧹 Cleaning..."
cd "$REPO_ROOT/ios/AIAssistApp"
xcodebuild clean -scheme AIAssistApp -quiet 2>/dev/null || true

echo "🔨 Building..."
xcodebuild -scheme AIAssistApp \
  -destination "platform=iOS,id=$DEVICE_ID" \
  -derivedDataPath "$DERIVED_DATA" \
  -allowProvisioningUpdates \
  DEVELOPMENT_TEAM="$TEAM_ID" \
  build -quiet

APP_PATH="$DERIVED_DATA/Build/Products/Debug-iphoneos/AIAssistApp.app"

echo "📲 Installing..."
xcrun devicectl device install app --device "$DEVICE_ID" "$APP_PATH"

echo "✅ Done!"
