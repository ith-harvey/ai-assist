#!/usr/bin/env bash
set -euo pipefail

# test-ios.sh — Run iOS unit tests and optionally UI tests
#
# Usage:
#   ./test-ios.sh              # Run SPM unit tests only
#   ./test-ios.sh --ui         # Run unit tests + UI tests in simulator
#   ./test-ios.sh --ui-only    # Run only UI tests in simulator
#   ./test-ios.sh --all        # Run unit tests + UI tests + build check

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$SCRIPT_DIR/ios"
XCODE_PROJECT="$IOS_DIR/AIAssistApp/AIAssistApp.xcodeproj"
SCHEME="AIAssistApp"
SIMULATOR="iPhone 16"
RUN_UNIT=true
RUN_UI=false
RUN_BUILD=false

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --ui)     RUN_UI=true ;;
        --ui-only) RUN_UNIT=false; RUN_UI=true ;;
        --all)    RUN_UI=true; RUN_BUILD=true ;;
        --help|-h)
            echo "Usage: $0 [--ui|--ui-only|--all]"
            echo ""
            echo "Options:"
            echo "  (default)   Run SPM unit tests only (fast, no simulator needed)"
            echo "  --ui        Run unit tests + UI tests in simulator"
            echo "  --ui-only   Run only UI tests in simulator"
            echo "  --all       Run unit + UI tests + full build validation"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Run $0 --help for usage"
            exit 1
            ;;
    esac
done

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓ $1${NC}"; }
fail() { echo -e "${RED}✗ $1${NC}"; }
info() { echo -e "${YELLOW}→ $1${NC}"; }

FAILURES=0

# ──────────────────────────────────────────────────
# Step 1: SPM Unit Tests (fast, no Xcode required)
# ──────────────────────────────────────────────────
if [ "$RUN_UNIT" = true ]; then
    info "Running SPM unit tests..."
    cd "$IOS_DIR"
    if swift test 2>&1; then
        pass "SPM unit tests passed"
    else
        fail "SPM unit tests failed"
        FAILURES=$((FAILURES + 1))
    fi
    cd "$SCRIPT_DIR"
fi

# ──────────────────────────────────────────────────
# Step 2: Build Check
# ──────────────────────────────────────────────────
if [ "$RUN_BUILD" = true ]; then
    info "Building Xcode project..."
    if xcodebuild build \
        -project "$XCODE_PROJECT" \
        -scheme "$SCHEME" \
        -destination "platform=iOS Simulator,name=$SIMULATOR" \
        -quiet 2>&1; then
        pass "Xcode build succeeded"
    else
        fail "Xcode build failed"
        FAILURES=$((FAILURES + 1))
    fi
fi

# ──────────────────────────────────────────────────
# Step 3: UI Tests (requires simulator)
# ──────────────────────────────────────────────────
if [ "$RUN_UI" = true ]; then
    info "Running UI tests in simulator ($SIMULATOR)..."

    # Check if UITest target exists
    if xcodebuild -list -project "$XCODE_PROJECT" 2>/dev/null | grep -q "AIAssistAppUITests"; then
        if xcodebuild test \
            -project "$XCODE_PROJECT" \
            -scheme "$SCHEME" \
            -destination "platform=iOS Simulator,name=$SIMULATOR" \
            -only-testing:AIAssistAppUITests \
            -quiet 2>&1; then
            pass "UI tests passed"
        else
            fail "UI tests failed"
            FAILURES=$((FAILURES + 1))
        fi
    else
        echo -e "${YELLOW}⚠ UI test target not yet added to Xcode project.${NC}"
        echo "  To add: Open AIAssistApp.xcodeproj → File → New → Target → UI Testing Bundle"
        echo "  Name it 'AIAssistAppUITests'. Test files are in AIAssistApp/AIAssistAppUITests/."
    fi
fi

# ──────────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────────
echo ""
if [ "$FAILURES" -eq 0 ]; then
    pass "All tests passed!"
    exit 0
else
    fail "$FAILURES test suite(s) failed"
    exit 1
fi
