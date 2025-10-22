#!/bin/bash
# Debug script for HandsOff Accessibility permissions

APP_BUNDLE="/Users/mhuang/Projects/Development/handsoff-rs/target/release/bundle/osx/HandsOff.app"
BUNDLE_ID="com.handsoff.inputlock"

echo "=== HandsOff Permission Debugger ==="
echo ""

echo "1. Checking app bundle..."
if [ -d "$APP_BUNDLE" ]; then
    echo "   ✓ App bundle exists at: $APP_BUNDLE"
else
    echo "   ✗ App bundle not found!"
    exit 1
fi

echo ""
echo "2. Checking bundle identifier..."
PLIST_BUNDLE_ID=$(defaults read "$APP_BUNDLE/Contents/Info" CFBundleIdentifier 2>/dev/null)
if [ "$PLIST_BUNDLE_ID" = "$BUNDLE_ID" ]; then
    echo "   ✓ Bundle ID: $BUNDLE_ID"
else
    echo "   ⚠ Bundle ID mismatch: Expected '$BUNDLE_ID', got '$PLIST_BUNDLE_ID'"
fi

echo ""
echo "3. Checking code signature..."
codesign -dv "$APP_BUNDLE" 2>&1 | head -5

echo ""
echo "4. Checking executable permissions..."
EXEC="$APP_BUNDLE/Contents/MacOS/handsoff"
if [ -x "$EXEC" ]; then
    echo "   ✓ Executable is present and has execute permissions"
    ls -l "$EXEC"
else
    echo "   ✗ Executable missing or not executable"
fi

echo ""
echo "5. Resetting Accessibility permissions..."
tccutil reset Accessibility "$BUNDLE_ID"
echo "   ✓ Permissions reset"

echo ""
echo "6. Opening System Settings..."
echo "   Please go to: System Settings > Privacy & Security > Accessibility"
echo "   and grant permission to HandsOff"
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"

echo ""
echo "7. Running the app with debug output..."
echo "   Press Ctrl+C to stop"
echo ""
echo "--- App Output ---"

# Run the app and capture output
"$EXEC"
