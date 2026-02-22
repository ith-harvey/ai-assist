#!/bin/bash
# Build and deploy AI Assist iOS app to Ian's iPhone
set -e

DEVICE_ID="00008110-000E50611451A01E"
TEAM_ID="TQBXL45729"
APP_PATH="$HOME/Library/Developer/Xcode/DerivedData/AIAssistApp-ahlggorcybsyyafxcgndpbklngjv/Build/Products/Debug-iphoneos/AIAssistApp.app"

echo "🧹 Cleaning..."
cd ~/projects/ai-assist/ios/AIAssistApp
xcodebuild clean -scheme AIAssistApp -quiet 2>/dev/null || true

echo "🔨 Building..."
xcodebuild -scheme AIAssistApp -destination "platform=iOS,id=$DEVICE_ID" -allowProvisioningUpdates DEVELOPMENT_TEAM="$TEAM_ID" build -quiet

echo "📲 Installing..."
xcrun devicectl device install app --device "$DEVICE_ID" "$APP_PATH"

echo "✅ Done!"
