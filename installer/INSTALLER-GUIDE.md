# HandsOff PKG Installer Guide

## The Problem We're Solving

HandsOff Tray App requires initial configuration including a secret passphrase, timeout settings, and hotkey preferences. This configuration must be completed before the app can function properly.

**The Challenge**: After installing a macOS .app bundle, users need an easy way to configure the application with their preferences. The configuration includes sensitive data (passphrase) that shouldn't be stored in plain text or in easily accessible locations.

## The Solution: PKG Installer with Setup Wizard

We've created an automated installer package (.pkg) that:
1. Installs HandsOff.app to ~/Applications
2. Provides a setup wizard (`handsoff-tray --setup`) that prompts for configuration
3. Stores encrypted configuration in `~/Library/Application Support/handsoff/config.toml`
4. Creates a Launch Agent to start the app automatically at login

## Building the Installer

### Quick Start

```bash
make pkg
```

This creates: `dist/HandsOff-v{VERSION}.pkg`

### What Happens During Build

1. **Builds the app bundle** (`make fix-plist`)
2. **Copies setup script** into the app bundle at:
   - `HandsOff.app/Contents/MacOS/setup-launch-agent.sh`
3. **Creates component package** with pkgbuild:
   - Includes the .app bundle
   - Includes postinstall script
4. **Creates final .pkg** with productbuild:
   - Adds welcome/license/conclusion screens
   - Professional installer UI

## End-User Installation Experience

### Step 1: Run the Installer

User double-clicks `HandsOff-v{VERSION}.pkg`

The installer shows:
- **Welcome screen** - Overview of what will be installed
- **License** - Shows LICENSE file
- **Installation** - Installs to /Applications
- **Postinstall script runs** - Checks if Launch Agent already configured
- **Conclusion screen** - Instructions for completing setup

### Step 2: Complete Setup

After installation, the user opens Terminal and runs:

```bash
~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
```

The setup wizard prompts for:
1. Secret passphrase (with confirmation, typing hidden)
2. Lock hotkey - the last key for Cmd+Ctrl+Shift+? (default: L)
3. Talk hotkey - the last key for Cmd+Ctrl+Shift+? (default: T)
4. Auto-lock timeout (default: 30 seconds)
5. Auto-unlock timeout (default: 60 seconds)

The configuration is saved to `~/Library/Application Support/handsoff/config.toml`

### Step 3: Automatic Startup

From now on:
- HandsOff starts automatically at login
- The Launch Agent provides the passphrase via environment variable
- No need to launch from terminal or set shell environment variables
- User never needs to click the app in /Applications

## How It Works: The Launch Agent

The setup script creates a Launch Agent plist at:
```
~/Library/LaunchAgents/com.handsoff.inputlock.plist
```

This plist contains:

```xml
<key>ProgramArguments</key>
<array>
    <string>/Applications/HandsOff.app/Contents/MacOS/handsoff</string>
</array>

<key>EnvironmentVariables</key>
<dict>
    <key>HANDS_OFF_SECRET_PHRASE</key>
    <string>user-passphrase-here</string>
</dict>

<key>RunAtLoad</key>
<true/>
```

**Key points:**
- `ProgramArguments`: Path to the actual executable (not the .app bundle)
- `EnvironmentVariables`: Sets `HANDS_OFF_SECRET_PHRASE` for the launched process
- `RunAtLoad`: Automatically starts at login

When macOS boots up or the user logs in:
1. `launchd` reads all plists in `~/Library/LaunchAgents/`
2. `launchd` launches the HandsOff executable with the specified environment variables
3. The app runs in the background (menu bar only, no Dock icon)

## Files Created by the Installer

```
installer/
├── README.md                    # Detailed documentation
├── build-pkg.sh                 # Main build script
└── scripts/
    ├── postinstall              # Runs after .pkg installation
    └── setup-launch-agent.sh    # User runs to configure passphrase
```

**Generated during build (cleaned up after):**
- `installer/pkg-root/` - Staging directory for package contents
- `installer/HandsOff-component.pkg` - Component package
- `installer/distribution.xml` - Package metadata
- `installer/welcome.html` - Welcome screen HTML
- `installer/conclusion.html` - Conclusion screen HTML

## Important: Don't Click the App!

Once the Launch Agent is configured, users should **NOT** double-click the app in /Applications. Here's why:

| Launch Method | Has Environment Variables? | Result |
|---------------|---------------------------|--------|
| Double-click in Finder | ❌ No | Shows error dialog |
| Launch via Spotlight | ❌ No | Shows error dialog |
| Login Items | ❌ No | Shows error dialog |
| **Launch Agent (plist)** | ✅ **Yes** | **Works correctly** |

The Launch Agent **is** the proper launch mechanism. The .app in /Applications is just the binary that the Launch Agent references.

## Comparison to Other Approaches

### ❌ Setting in .zshrc
```bash
export HANDS_OFF_SECRET_PHRASE='my-passphrase'
```
**Problem**: GUI apps don't inherit shell environment variables

### ❌ Keychain Storage
**Problem**: Prompts for Keychain password every time (annoying)

### ❌ UserDefaults
**Problem**: Stores passphrase in plain text, easily readable by any process

### ✅ Launch Agent with Environment Variables (Our Solution)
**Benefits**:
- No password prompts
- Passphrase stored in user-only readable file (600 permissions)
- Standard macOS practice for background apps
- Automatic startup at login
- No code changes needed

## Security Considerations

1. **Passphrase Storage**:
   - Stored in `~/Library/LaunchAgents/com.handsoff.inputlock.plist`
   - File permissions: 600 (user read/write only)
   - Same security level as storing in a shell script

2. **Who Can Read It?**:
   - Only your user account
   - Root user (like any file on your system)
   - Processes running as your user (same as environment variables)

3. **Is This Secure Enough?**:
   - For personal use: Yes
   - For enterprise: Consider additional security layers
   - More secure than plain text in UserDefaults
   - Less annoying than Keychain prompts

## Makefile Integration

The PKG installer is integrated into the Makefile:

```bash
make pkg     # Build the .pkg installer
make clean   # Removes intermediate files
make help    # Shows all available targets
```

## Testing the Installer

### Build and Test Locally

```bash
# Build the installer
make pkg

# Open it (runs the installer)
open dist/HandsOff-v{VERSION}.pkg

# Follow the installer, then complete setup:
/Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh

# Verify it's running:
launchctl list | grep handsoff

# Check logs if there are issues:
cat ~/Library/Logs/HandsOff.log
```

### Inspect Package Contents

```bash
# List all files in the package
pkgutil --payload-files dist/HandsOff-v{VERSION}.pkg

# Verify setup script is included
pkgutil --payload-files dist/HandsOff-v{VERSION}.pkg | grep setup-launch-agent

# Check package signature
pkgutil --check-signature dist/HandsOff-v{VERSION}.pkg

# Expand for detailed inspection
pkgutil --expand dist/HandsOff-v{VERSION}.pkg /tmp/pkg-inspect
```

## Uninstalling

To completely remove HandsOff:

```bash
# Stop and remove Launch Agent
launchctl unload ~/Library/LaunchAgents/com.handsoff.inputlock.plist
rm ~/Library/LaunchAgents/com.handsoff.inputlock.plist

# Remove app
rm -rf /Applications/HandsOff.app

# Optional: Remove logs
rm ~/Library/Logs/HandsOff.log
rm ~/Library/Logs/HandsOff.error.log
```

## Distribution

### For Personal Use

The unsigned .pkg works fine for personal use:

```bash
make pkg
# Share dist/HandsOff-v{VERSION}.pkg
```

### For Public Distribution

Sign with a Developer ID certificate:

```bash
# Build the package
make pkg

# Sign the package
productsign --sign "Developer ID Installer: Your Name (TEAM_ID)" \
  dist/HandsOff-v{VERSION}.pkg \
  dist/HandsOff-v{VERSION}-signed.pkg

# Notarize for Gatekeeper
xcrun notarytool submit dist/HandsOff-v{VERSION}-signed.pkg \
  --apple-id "your@email.com" \
  --password "app-specific-password" \
  --team-id "TEAM_ID" \
  --wait

# Staple the notarization ticket
xcrun stapler staple dist/HandsOff-v{VERSION}-signed.pkg
```

## Troubleshooting

### Setup script not found after installation

Check if the script exists:
```bash
ls -la /Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh
```

If missing, rebuild the package:
```bash
make clean
make pkg
```

### Launch Agent not starting

Check if plist was created:
```bash
ls -la ~/Library/LaunchAgents/com.handsoff.inputlock.plist
```

Check if it's loaded:
```bash
launchctl list | grep handsoff
```

View error logs:
```bash
cat ~/Library/Logs/HandsOff.error.log
```

### App shows "passphrase not set" error

This means the app was launched WITHOUT the Launch Agent (e.g., by double-clicking in Finder).

**Solution**: Let the Launch Agent handle launching. Don't click the app manually.

To restart via Launch Agent:
```bash
launchctl unload ~/Library/LaunchAgents/com.handsoff.inputlock.plist
launchctl load ~/Library/LaunchAgents/com.handsoff.inputlock.plist
```

## Summary

The PKG installer provides a complete, automated solution for distributing HandsOff with proper environment variable configuration. It solves the fundamental problem that macOS GUI apps don't inherit shell environment variables by using a Launch Agent to provide the passphrase at launch time.

This is the **recommended distribution method** for HandsOff.

## See Also

- [installer/README.md](installer/README.md) - Detailed installer documentation
- [BUILD.md](BUILD.md) - Complete build instructions
- [Makefile](Makefile) - All available build targets
