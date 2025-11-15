# HandsOff Installer Package

This directory contains the scripts and resources needed to build a macOS `.pkg` installer for HandsOff.

## What the Installer Does

The `.pkg` installer provides a complete installation solution that:

1. **Installs HandsOff.app** to `/Applications/`
2. **Includes a setup script** that configures the Launch Agent with your passphrase
3. **Guides users** through post-installation setup

## Building the Installer

### Quick Build

```bash
# From the project root
make pkg
```

This creates: `dist/HandsOff-v{VERSION}.pkg`

### Manual Build

```bash
./installer/build-pkg.sh
```

## How It Works

### Installation Process

1. **User runs the .pkg installer**
   - macOS Installer.app displays welcome screen
   - Shows license agreement
   - Installs HandsOff.app to /Applications
   - Runs postinstall script

2. **Postinstall script** (`scripts/postinstall`)
   - Verifies app installation
   - Checks if Launch Agent already configured
   - Shows instructions to complete setup

3. **User completes setup** (after installation)
   ```bash
   /Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh
   ```
   - Prompts for secret passphrase
   - Creates Launch Agent plist at `~/Library/LaunchAgents/com.handsoff.inputlock.plist`
   - Loads the Launch Agent
   - Starts HandsOff automatically

### Why Two-Step Installation?

The installer cannot prompt for user input (the passphrase) during installation. Instead:

1. The `.pkg` installs the app and setup script
2. The user runs the setup script to configure their passphrase
3. This approach is more secure than storing a default passphrase

## Files in This Directory

```
installer/
├── README.md                    # This file
├── build-pkg.sh                 # Main build script (creates the .pkg)
└── scripts/
    ├── postinstall              # Runs after .pkg installation
    └── setup-launch-agent.sh    # User runs this to configure passphrase
```

## User Experience Flow

### Developer (You)

```bash
make pkg
# Distribute dist/HandsOff-v0.3.0.pkg
```

### End User

1. **Download and open** `HandsOff-v{VERSION}.pkg`
2. **Follow installer** - click through welcome, license, install
3. **Grant Accessibility permissions** in System Preferences > Security & Privacy > Privacy > Accessibility
4. **After installation completes**, open Terminal and run:
   ```bash
   ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
   ```
5. **Configure during setup** when prompted:
   - Secret passphrase (typing hidden)
   - Lock hotkey (default: L for Cmd+Ctrl+Shift+L)
   - Talk hotkey (default: T for Cmd+Ctrl+Shift+T)
   - Auto-lock timeout (default: 30s)
   - Auto-unlock timeout (default: 60s)
6. **HandsOff starts** automatically and appears in menu bar
7. **At next login**, HandsOff starts automatically

## Launch Agent Details

The setup script creates `~/Library/LaunchAgents/com.handsoff.inputlock.plist`:

```xml
<key>EnvironmentVariables</key>
<dict>
    <key>HANDS_OFF_SECRET_PHRASE</key>
    <string>user-passphrase-here</string>
</dict>
```

This allows HandsOff to:
- Access the passphrase without prompting
- Start automatically at login
- Run in the background (no Dock icon)

## Advanced Usage

### Signing the Package

For distribution, sign with a Developer ID Installer certificate:

```bash
# After building
productsign --sign "Developer ID Installer: Your Name (TEAM_ID)" \
  dist/HandsOff-v0.3.0.pkg \
  dist/HandsOff-v0.3.0-signed.pkg
```

### Verifying Package Contents

```bash
# List files in package
pkgutil --payload-files dist/HandsOff-v0.3.0.pkg

# Check package info
pkgutil --pkg-info-plist dist/HandsOff-v0.3.0.pkg

# Expand package for inspection
pkgutil --expand dist/HandsOff-v0.3.0.pkg /tmp/pkg-inspect
```

### Testing Installation

```bash
# Install to custom location for testing
sudo installer -pkg dist/HandsOff-v0.3.0.pkg -target / -verbose

# Or just double-click the .pkg file
open dist/HandsOff-v0.3.0.pkg
```

## Troubleshooting

### Build fails with "command not found"

Make sure you have Xcode Command Line Tools:
```bash
xcode-select --install
```

### Package won't install

Check signature:
```bash
pkgutil --check-signature dist/HandsOff-v0.3.0.pkg
```

### Setup script not found after installation

The script should be at:
```
/Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh
```

Check if app was installed correctly:
```bash
ls -la /Applications/HandsOff.app/Contents/MacOS/
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

View logs:
```bash
cat ~/Library/Logs/HandsOff.log
cat ~/Library/Logs/HandsOff.error.log
```

## Uninstallation

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

## Security Notes

1. **Passphrase Storage**: The passphrase is stored in plain text in the Launch Agent plist at `~/Library/LaunchAgents/`. The file has 600 permissions (user-only read/write).

2. **Alternative**: If you prefer more security, consider implementing Keychain storage, but note that this may prompt for the Keychain password.

3. **Distribution**: For public distribution, sign both the app and the package with valid Developer ID certificates.

## References

- [pkgbuild man page](https://ss64.com/osx/pkgbuild.html)
- [productbuild man page](https://ss64.com/osx/productbuild.html)
- [Launch Agents Guide](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)