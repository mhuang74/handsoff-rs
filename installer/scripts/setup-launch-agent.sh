#!/bin/bash
# Setup script for HandsOff Launch Agent
# This script prompts for the passphrase and creates the Launch Agent plist

set -e

APP_NAME="HandsOff"
BUNDLE_ID="com.handsoff.inputlock"
LAUNCH_AGENT_DIR="${HOME}/Library/LaunchAgents"
LAUNCH_AGENT_PLIST="${LAUNCH_AGENT_DIR}/${BUNDLE_ID}.plist"
APP_EXECUTABLE="/Applications/${APP_NAME}.app/Contents/MacOS/handsoff-tray"

echo ""
echo "========================================"
echo "  HandsOff Launch Agent Setup"
echo "========================================"
echo ""

# Check if app is installed
if [ ! -f "${APP_EXECUTABLE}" ]; then
    echo "ERROR: HandsOff not found at ${APP_EXECUTABLE}"
    echo ""
    echo "Please install HandsOff to /Applications first."
    exit 1
fi

# Check if Accessibility permissions are granted
echo "Checking Accessibility permissions..."
# Test by trying to query the TCC database for Accessibility permission
# This is a heuristic check - we try to see if the app has been granted permissions
APP_BUNDLE_PATH="/Applications/${APP_NAME}.app"

# Check if the app is listed in the TCC database for Accessibility
# We use tccutil to check this (only works on newer macOS versions)
# For older versions, we'll just show a warning
if command -v tccutil &> /dev/null; then
    # On macOS Monterey and later, we can check if the app has the permission
    # For now, just show a warning since checking programmatically is tricky
    :
fi

# Always show a warning to ensure permissions are granted
echo ""
echo "⚠️  IMPORTANT: Accessibility Permissions Required"
echo ""
echo "Before continuing, please ensure you have granted Accessibility permissions:"
echo ""
echo "1. Open: System Preferences → Security & Privacy → Privacy → Accessibility"
echo "2. Click the lock icon to make changes"
echo "3. Add HandsOff from: ${APP_BUNDLE_PATH}"
echo "4. Ensure the checkbox next to HandsOff is checked"
echo ""
echo "If you haven't done this yet, HandsOff will fail to start."
echo ""
read -p "Have you granted Accessibility permissions? (y/N): " -r
echo ""
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Please grant Accessibility permissions first, then run this script again."
    exit 1
fi

# Check if already configured
if [ -f "${LAUNCH_AGENT_PLIST}" ]; then
    echo "Launch Agent is already configured."
    echo ""
    read -p "Do you want to reconfigure with a new passphrase? (y/N): " -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Setup cancelled."
        exit 0
    fi

    # Unload existing agent if running
    if launchctl list | grep -q "${BUNDLE_ID}"; then
        echo "Stopping existing Launch Agent..."
        launchctl unload "${LAUNCH_AGENT_PLIST}" 2>/dev/null || true
    fi
fi

# Prompt for passphrase
echo "Enter your secret passphrase for HandsOff:"
echo "(This will be used to unlock your input when locked)"
echo ""
read -s -p "Passphrase: " PASSPHRASE
echo ""

# Validate passphrase
if [ -z "${PASSPHRASE}" ]; then
    echo ""
    echo "ERROR: Passphrase cannot be empty."
    exit 1
fi

# Confirm passphrase
read -s -p "Confirm passphrase: " PASSPHRASE_CONFIRM
echo ""

if [ "${PASSPHRASE}" != "${PASSPHRASE_CONFIRM}" ]; then
    echo ""
    echo "ERROR: Passphrases do not match."
    exit 1
fi

# Create LaunchAgents directory if it doesn't exist
mkdir -p "${LAUNCH_AGENT_DIR}"

# Generate the Launch Agent plist
echo ""
echo "Creating Launch Agent configuration..."

cat > "${LAUNCH_AGENT_PLIST}" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${BUNDLE_ID}</string>

    <key>ProgramArguments</key>
    <array>
        <string>${APP_EXECUTABLE}</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>HANDS_OFF_SECRET_PHRASE</key>
        <string>${PASSPHRASE}</string>
    </dict>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <false/>

    <key>StandardOutPath</key>
    <string>${HOME}/Library/Logs/${APP_NAME}.log</string>

    <key>StandardErrorPath</key>
    <string>${HOME}/Library/Logs/${APP_NAME}.error.log</string>
</dict>
</plist>
EOF

# Set proper permissions (readable only by user)
chmod 600 "${LAUNCH_AGENT_PLIST}"

echo "✓ Launch Agent plist created at:"
echo "  ${LAUNCH_AGENT_PLIST}"
echo ""

# Load the Launch Agent
echo "Loading Launch Agent..."
launchctl load "${LAUNCH_AGENT_PLIST}"

echo "✓ Launch Agent loaded successfully"
echo ""

# Verify it's running
sleep 2
if launchctl list | grep -q "${BUNDLE_ID}"; then
    echo "✓ HandsOff is now running!"
    echo ""
    echo "Look for the lock icon in your menu bar."
else
    echo "WARNING: Launch Agent loaded but may not be running."
    echo "Check the logs at:"
    echo "  ${HOME}/Library/Logs/${APP_NAME}.log"
    echo "  ${HOME}/Library/Logs/${APP_NAME}.error.log"
fi

echo ""
echo "========================================"
echo "  Setup Complete!"
echo "========================================"
echo ""
echo "HandsOff will now:"
echo "  • Start automatically at login"
echo "  • Run in the background (menu bar only)"
echo "  • Use your passphrase to unlock"
echo ""
echo "Hotkeys:"
echo "  • Ctrl+Cmd+Shift+L: Lock input"
echo "  • Type your passphrase: Unlock input"
echo ""
echo "To uninstall the Launch Agent:"
echo "  launchctl unload ${LAUNCH_AGENT_PLIST}"
echo "  rm ${LAUNCH_AGENT_PLIST}"
echo ""

exit 0