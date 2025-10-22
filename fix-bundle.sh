#!/bin/bash
#
# fix-bundle.sh - Post-processing script for cargo-bundle
#
# This script fixes the Info.plist to add keys that cargo-bundle doesn't support.
# Run this after `cargo bundle --release`
#

set -e

BUNDLE_PATH="target/release/bundle/osx/HandsOff.app"
INFO_PLIST="$BUNDLE_PATH/Contents/Info.plist"

if [ ! -f "$INFO_PLIST" ]; then
    echo "Error: Bundle not found at $BUNDLE_PATH"
    echo "Please run 'cargo bundle --release' first"
    exit 1
fi

echo "Fixing Info.plist for menu bar app..."

# Add LSUIElement key (menu bar only, no Dock icon)
plutil -insert LSUIElement -bool true "$INFO_PLIST" 2>/dev/null || {
    echo "LSUIElement key already exists, skipping"
}

echo "✓ Info.plist fixed"
echo ""
echo "Key settings:"
plutil -p "$INFO_PLIST" | grep -E "(CFBundleDisplayName|LSUIElement|NSHighResolutionCapable)" || true
echo ""
echo "Bundle ready at: $BUNDLE_PATH"
echo "Test with: open $BUNDLE_PATH"
