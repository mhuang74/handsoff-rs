# Building HandsOff as a macOS Application

This document describes how to build HandsOff as a proper macOS application bundle.

## Prerequisites

1. **Rust toolchain**: Install from [rustup.rs](https://rustup.rs/)
2. **cargo-bundle**: Install with `cargo install cargo-bundle`

## Quick Start

The easiest way to build the app is using the provided Makefile:

```bash
# Build and create the .app bundle with all fixes applied
make all

# Or just run make (all is the default target)
make
```

## Makefile Targets

### Primary Targets

- `make` or `make all` - Create .app bundle with LSUIElement fix (default)
- `make fix-plist` - Create .app bundle with LSUIElement fix (menu bar only)
- `make pkg` - Create .pkg installer with Launch Agent setup (recommended for distribution)

### Developer Tools

- `make test` - Run cargo tests
- `make check` - Run cargo check
- `make clippy` - Run cargo clippy
- `make clean` - Remove all build artifacts

### Other Targets

- `make build` - Build release binary only (intermediate step)
- `make bundle` - Create .app bundle without LSUIElement fix (intermediate step)
- `make install` - Install to `/Applications` for local testing
- `make help` - Show all available targets

## Manual Build Process

If you prefer to build manually without the Makefile:

### Step 1: Build Release Binary

```bash
cargo build --release
```

### Step 2: Create Bundle

```bash
cargo bundle --release
```

This creates: `target/release/bundle/osx/HandsOff.app/`

### Step 3: Fix Info.plist

cargo-bundle doesn't support all Info.plist keys, so we need to manually add `LSUIElement`:

```bash
plutil -insert LSUIElement -bool true \
  target/release/bundle/osx/HandsOff.app/Contents/Info.plist
```

This key makes the app a menu bar-only application (no Dock icon).

### Step 4: Test the Application

```bash
open target/release/bundle/osx/HandsOff.app
```

The app should:
- Launch without opening Terminal
- Show a menu bar icon (🔓 or 🔒)
- Not appear in the Dock
- Request Accessibility permissions on first run

## Bundle Structure

The created bundle has the following structure:

```
HandsOff.app/
└── Contents/
    ├── Info.plist           # App metadata
    └── MacOS/
        └── handsoff         # The executable binary
```

## Info.plist Configuration

The bundle's Info.plist includes:

- **CFBundleExecutable**: `handsoff`
- **CFBundleIdentifier**: `com.handsoff.inputlock`
- **CFBundleName**: `HandsOff`
- **CFBundleVersion**: `0.1.0`
- **LSUIElement**: `true` - Menu bar only, no Dock icon
- **NSHighResolutionCapable**: `true` - Retina display support
- **LSMinimumSystemVersion**: `10.11` - Minimum macOS version

## Distribution

### Option 1: PKG Installer with Launch Agent (Recommended)

Create a complete installer package that includes setup tooling:

```bash
make pkg
```

This creates `dist/HandsOff-v{VERSION}.pkg` with:
- The HandsOff.app bundle
- Built-in setup script for configuring the Launch Agent
- Professional installer UI with welcome and instructions
- Postinstall script that guides users through setup

**Why use PKG instead of DMG?**

The PKG installer solves the environment variable problem by:
1. Installing the app to /Applications
2. Including a setup script that prompts for your passphrase
3. Automatically creating the Launch Agent plist with the passphrase
4. Configuring the app to start at login

**User Experience:**
1. User runs the .pkg installer
2. After installation, user runs the setup script:
   ```bash
   /Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh
   ```
3. Setup script prompts for passphrase
4. Launch Agent is configured and app starts automatically
5. App starts at every login with correct environment variables

For detailed information, see [installer/INSTALLER-GUIDE.md](installer/INSTALLER-GUIDE.md).

### Option 2: Direct .app Distribution

For simple distribution without Launch Agent setup, simply distribute the `.app` bundle:

```bash
cd target/release/bundle/osx
zip -r HandsOff-v0.1.0.zip HandsOff.app
```

Users can extract and drag to `/Applications`.

**Note**: Users will need to manually configure the Launch Agent. The PKG installer (Option 1) handles this automatically.

### Option 3: Install Locally for Testing

To test the installed version on your local machine:

```bash
make install
```

This copies the bundle to `/Applications/HandsOff.app`.

## Code Signing and Distribution

Code signing and notarization for public distribution are handled automatically by the GitHub Actions CI/CD pipeline. See `.github/workflows/release.yml` for details.

For local development and testing, unsigned builds work fine. macOS will prompt users to allow the app in System Settings > Privacy & Security if needed.

## Adding an Application Icon

To add an app icon:

1. Create a 1024x1024 PNG: `assets/AppIcon.png`
2. Follow the instructions in `assets/README.md` to create `AppIcon.icns`
3. Update `Cargo.toml`:
   ```toml
   [package.metadata.bundle]
   icon = ["assets/AppIcon.icns"]
   ```
4. Rebuild: `make all`

## Verification Commands

### Check Bundle Structure
```bash
ls -la target/release/bundle/osx/HandsOff.app/Contents/
```

### Verify Info.plist
```bash
plutil -lint target/release/bundle/osx/HandsOff.app/Contents/Info.plist
plutil -p target/release/bundle/osx/HandsOff.app/Contents/Info.plist
```

### Check Executable
```bash
ls -l target/release/bundle/osx/HandsOff.app/Contents/MacOS/handsoff
lipo -info target/release/bundle/osx/HandsOff.app/Contents/MacOS/handsoff
```

### Verify Code Signature
```bash
codesign -dvvv target/release/bundle/osx/HandsOff.app
```

## Troubleshooting

### App doesn't start
- Check executable permissions: `chmod +x HandsOff.app/Contents/MacOS/handsoff`
- Verify CFBundleExecutable matches binary name
- Check Console.app for crash logs

### Still opens Terminal
- Ensure bundle has `.app` extension
- Verify Info.plist exists and is valid
- Ensure LSUIElement fix was applied

### No menu bar icon
- Verify Accessibility permissions are granted
- Check Console.app for errors
- Ensure app isn't crashing on startup

### Gatekeeper blocks app
- For local development, this is normal for unsigned apps
- Users can override: System Settings > Privacy & Security > "Open Anyway"
- Official releases are signed and notarized via CI/CD

## Build Architecture

By default, the build creates a binary for the current architecture:
- Apple Silicon: `arm64`
- Intel: `x86_64`

To create a universal binary (both architectures), see the full guide in `specs/build_as_macos_app.md`.

## References

- [Spec Document](specs/build_as_macos_app.md) - Complete implementation specification
- [cargo-bundle GitHub](https://github.com/burtonageo/cargo-bundle)
- [Apple Bundle Documentation](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)
- [Info.plist Keys Reference](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Introduction/Introduction.html)
