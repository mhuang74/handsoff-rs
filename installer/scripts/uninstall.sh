#!/bin/bash
# Complete uninstall script for HandsOff
# This removes all traces of HandsOff for clean reinstallation testing
#
# Features:
#   - Unregisters package from system (pkgutil --forget)
#   - Stops and removes Launch Agent
#   - Removes application bundle
#   - Removes log files, preferences, and cache files
#   - Resets Accessibility permissions
#   - Optional advanced TCC database cleanup
#
# Usage:
#   ./uninstall.sh              # Interactive mode
#   ./uninstall.sh --force      # Skip all prompts (for automated scripts)
#   ./uninstall.sh --skip-advanced  # Skip advanced TCC cleanup

set -e

APP_NAME="HandsOff"
APP_BUNDLE_PATH="${HOME}/Applications/${APP_NAME}.app"
BUNDLE_ID="com.handsoff.inputlock"
LAUNCH_AGENT_PLIST="${HOME}/Library/LaunchAgents/${BUNDLE_ID}.plist"
LOG_FILE="${HOME}/Library/Logs/${APP_NAME}.log"
ERROR_LOG_FILE="${HOME}/Library/Logs/${APP_NAME}.error.log"

# Parse command line arguments
FORCE_MODE=0
SKIP_ADVANCED=0
for arg in "$@"; do
    case $arg in
        -f|--force)
            FORCE_MODE=1
            shift
            ;;
        --skip-advanced)
            SKIP_ADVANCED=1
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -f, --force         Skip confirmation prompts (for automated uninstall)"
            echo "  --skip-advanced     Skip advanced TCC cleanup (for quick uninstall)"
            echo "  -h, --help          Show this help message"
            echo ""
            exit 0
            ;;
    esac
done

echo ""
echo "========================================"
echo "  HandsOff Complete Uninstall"
echo "========================================"
echo ""
echo "This script will completely remove HandsOff from your system:"
echo ""
echo "  • Unregister package from system (pkgutil)"
echo "  • Stop and remove Launch Agent"
echo "  • Remove application from ~/Applications"
echo "  • Remove log files"
echo "  • Reset Accessibility permissions"
echo ""

# Confirm before proceeding (unless force mode)
if [ $FORCE_MODE -eq 0 ]; then
    read -p "Do you want to completely uninstall HandsOff? (y/N): " -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Cancelled."
        exit 0
    fi
fi

echo "Starting uninstall..."
echo ""

# Step 0: Unregister package from system
echo "Step 0: Unregistering package from system..."
if pkgutil --pkgs | grep -q "^${BUNDLE_ID}$" 2>/dev/null; then
    echo "  - Found registered package: ${BUNDLE_ID}"

    # Check installed version
    INSTALLED_VERSION=$(pkgutil --pkg-info "${BUNDLE_ID}" 2>/dev/null | grep "^version:" | awk '{print $2}')
    if [ -n "${INSTALLED_VERSION}" ]; then
        echo "  - Installed version: ${INSTALLED_VERSION}"
    fi

    # Try to forget the package
    if sudo -n pkgutil --forget "${BUNDLE_ID}" 2>/dev/null; then
        echo "  ✓ Package unregistered successfully"
    else
        echo "  - Attempting to forget package (may require password)..."
        if sudo pkgutil --forget "${BUNDLE_ID}" 2>&1 | grep -q "Forgot package"; then
            echo "  ✓ Package unregistered successfully"
        else
            echo "  ⚠️  Warning: Could not unregister package"
            echo "     This may cause issues during reinstallation"
            echo "     Try manually: sudo pkgutil --forget ${BUNDLE_ID}"
        fi
    fi
else
    echo "  - No registered package found (already removed or never installed via pkg)"
fi
echo ""

# Step 1: Stop and unload Launch Agent
if [ -f "${LAUNCH_AGENT_PLIST}" ]; then
    echo "Step 1: Stopping Launch Agent..."

    # Check if it's loaded
    if launchctl list | grep -q "${BUNDLE_ID}" 2>/dev/null; then
        echo "  - Unloading Launch Agent..."
        launchctl unload "${LAUNCH_AGENT_PLIST}" 2>/dev/null || true
        echo "  ✓ Launch Agent unloaded"
    else
        echo "  - Launch Agent not running"
    fi

    # Remove the plist file
    echo "  - Removing Launch Agent plist..."
    rm -f "${LAUNCH_AGENT_PLIST}"
    echo "  ✓ Launch Agent plist removed"
else
    echo "Step 1: No Launch Agent found (skipping)"
fi
echo ""

# Step 2: Kill any running HandsOff processes
echo "Step 2: Stopping running processes..."
if pgrep -x handsoff-tray > /dev/null 2>&1 || pgrep -x handsoff > /dev/null 2>&1; then
    echo "  - Killing HandsOff processes..."
    killall handsoff-tray 2>/dev/null || true
    killall handsoff 2>/dev/null || true
    sleep 1
    echo "  ✓ Processes stopped"
else
    echo "  - No running processes found"
fi
echo ""

# Step 3: Remove application bundle
echo "Step 3: Removing application..."
if [ -d "${APP_BUNDLE_PATH}" ]; then
    # Check if any files are owned by root (from previous installation)
    if [ "$(find "${APP_BUNDLE_PATH}" -user root 2>/dev/null | wc -l)" -gt 0 ]; then
        echo "  - Found root-owned files, using sudo..."
        sudo rm -rf "${APP_BUNDLE_PATH}"
    else
        rm -rf "${APP_BUNDLE_PATH}"
    fi
    echo "  ✓ Application removed from ~/Applications"
else
    echo "  - Application not found (already removed)"
fi
echo ""

# Step 4: Remove log files
echo "Step 4: Removing log files..."
LOGS_REMOVED=0
if [ -f "${LOG_FILE}" ]; then
    rm -f "${LOG_FILE}"
    echo "  ✓ Removed ${LOG_FILE}"
    LOGS_REMOVED=1
fi
if [ -f "${ERROR_LOG_FILE}" ]; then
    rm -f "${ERROR_LOG_FILE}"
    echo "  ✓ Removed ${ERROR_LOG_FILE}"
    LOGS_REMOVED=1
fi
if [ $LOGS_REMOVED -eq 0 ]; then
    echo "  - No log files found"
fi
echo ""

# Step 4b: Remove preferences and cache files
echo "Step 4b: Removing preferences and cache files..."
PREFS_REMOVED=0

# Check for preference files
PREF_FILE="${HOME}/Library/Preferences/${BUNDLE_ID}.plist"
if [ -f "${PREF_FILE}" ]; then
    rm -f "${PREF_FILE}"
    echo "  ✓ Removed preference file"
    PREFS_REMOVED=1
fi

# Check for Application Support directory
APP_SUPPORT_DIR="${HOME}/Library/Application Support/HandsOff"
if [ -d "${APP_SUPPORT_DIR}" ]; then
    rm -rf "${APP_SUPPORT_DIR}"
    echo "  ✓ Removed Application Support directory"
    PREFS_REMOVED=1
fi

# Check for cache directory
CACHE_DIR="${HOME}/Library/Caches/${BUNDLE_ID}"
if [ -d "${CACHE_DIR}" ]; then
    rm -rf "${CACHE_DIR}"
    echo "  ✓ Removed cache directory"
    PREFS_REMOVED=1
fi

if [ $PREFS_REMOVED -eq 0 ]; then
    echo "  - No preference or cache files found"
fi
echo ""

# Step 5: Reset Accessibility permissions
echo "Step 5: Resetting Accessibility permissions..."
echo ""

# Try using tccutil (works on macOS Monterey 12.0+)
if command -v tccutil &> /dev/null; then
    echo "  Attempting to reset using tccutil..."

    # Try both bundle ID and bundle path
    RESET_SUCCESS=0

    if tccutil reset Accessibility "${BUNDLE_ID}" 2>/dev/null; then
        echo "  ✓ Reset Accessibility for ${BUNDLE_ID}"
        RESET_SUCCESS=1
    fi

    if tccutil reset Accessibility "${APP_BUNDLE_PATH}" 2>/dev/null; then
        echo "  ✓ Reset Accessibility for ${APP_BUNDLE_PATH}"
        RESET_SUCCESS=1
    fi

    if [ $RESET_SUCCESS -eq 0 ]; then
        echo "  ⚠️  tccutil reset failed (may not be supported on this macOS version)"
        echo ""
        echo "  Please manually remove Accessibility permission:"
        echo "  1. Open: System Preferences → Security & Privacy → Privacy → Accessibility"
        echo "  2. Click the lock icon to make changes"
        echo "  3. Find HandsOff in the list (if present)"
        echo "  4. Click the '-' button to remove it"
    fi
else
    echo "  ⚠️  tccutil not available on this system"
    echo ""
    echo "  Please manually remove Accessibility permission:"
    echo "  1. Open: System Preferences → Security & Privacy → Privacy → Accessibility"
    echo "  2. Click the lock icon to make changes"
    echo "  3. Find HandsOff in the list (if present)"
    echo "  4. Click the '-' button to remove it"
fi
echo ""

# Step 6: Advanced cleanup options
echo "Step 6: Advanced cleanup (optional)..."
echo ""

# Skip advanced cleanup if flag is set
if [ $SKIP_ADVANCED -eq 1 ]; then
    echo "  - Skipping advanced TCC cleanup (--skip-advanced flag set)"
    echo ""
else
    echo "  ⚠️  WARNING: This requires either:"
    echo "     1. SIP (System Integrity Protection) disabled, OR"
    echo "     2. Terminal has Full Disk Access permission"
    echo ""
    echo "  Disabling SIP reduces system security."
    echo "  This is intended for development/testing only."
    echo ""

    # Skip prompt if in force mode
    DO_ADVANCED=0
    if [ $FORCE_MODE -eq 1 ]; then
        echo "  - Skipping advanced TCC cleanup (force mode)"
        echo ""
    else
        read -p "Do you want to try advanced TCC database cleanup? (y/N): " -r
        echo ""
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            DO_ADVANCED=1
        fi
    fi

    if [ $DO_ADVANCED -eq 1 ]; then
    echo "  Attempting TCC database cleanup..."
    echo ""

    # User TCC database
    USER_TCC_DB="${HOME}/Library/Application Support/com.apple.TCC/TCC.db"
    if [ -f "${USER_TCC_DB}" ]; then
        if sqlite3 "${USER_TCC_DB}" "DELETE FROM access WHERE client='${BUNDLE_ID}';" 2>/dev/null; then
            echo "  ✓ Removed from user TCC database"
        else
            echo "  ⚠️  Could not modify user TCC database (permission denied)"
        fi
    fi

    # System TCC database (requires sudo)
    SYSTEM_TCC_DB="/Library/Application Support/com.apple.TCC/TCC.db"
    if [ -f "${SYSTEM_TCC_DB}" ]; then
        if sudo sqlite3 "${SYSTEM_TCC_DB}" "DELETE FROM access WHERE client='${BUNDLE_ID}';" 2>/dev/null; then
            echo "  ✓ Removed from system TCC database"
        else
            echo "  ⚠️  Could not modify system TCC database (permission denied or SIP enabled)"
        fi
    fi
    echo ""
    fi
fi

echo ""
echo "========================================"
echo "  Uninstall Complete!"
echo "========================================"
echo ""
echo "HandsOff has been completely removed from your system."
echo ""

if [ $FORCE_MODE -eq 1 ]; then
    echo "Uninstall completed in force mode (no prompts)."
    echo "System is now ready for clean reinstallation."
else
    echo "To reinstall:"
    echo "  1. Build a new package: make pkg"
    echo "  2. Install: installer -pkg dist/HandsOff-v*.pkg -target CurrentUserHomeDirectory"
    echo "  3. Grant Accessibility permissions in System Preferences"
    echo "  4. Run setup: ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup"
fi
echo ""

exit 0
