# Building HandsOff as a Proper macOS Application

## Problem Statement

Currently, the `handsoff` binary built by Cargo is a standard Unix executable (Mach-O 64-bit). When copied to `/Applications` and double-clicked:

- macOS opens Terminal.app to run the executable
- The app doesn't appear as a proper GUI application
- No app icon appears in the Dock
- It doesn't integrate properly with macOS UI conventions

This happens because macOS requires applications to be packaged as **application bundles** (`.app` directories) with specific metadata and structure.

## macOS Application Bundle Structure

A proper macOS application requires the following directory structure:

```
HandsOff.app/
├── Contents/
    ├── Info.plist              # Required: App metadata
    ├── MacOS/
    │   └── handsoff            # The actual executable binary
    ├── Resources/
    │   ├── AppIcon.icns        # Optional: Application icon
    │   └── ...                 # Other resources
    ├── _CodeSignature/         # Created during code signing
    └── ...                     # Other optional directories
```

### Info.plist Requirements

The `Info.plist` file must contain essential metadata that tells macOS how to handle the application:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- Required Keys -->
    <key>CFBundleExecutable</key>
    <string>handsoff</string>

    <key>CFBundleIdentifier</key>
    <string>com.handsoff.inputlock</string>

    <key>CFBundlePackageType</key>
    <string>APPL</string>

    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>

    <key>CFBundleName</key>
    <string>HandsOff</string>

    <key>CFBundleDisplayName</key>
    <string>HandsOff</string>

    <key>CFBundleVersion</key>
    <string>0.1.0</string>

    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>

    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>

    <!-- Application-Specific Keys -->
    <key>LSMinimumSystemVersion</key>
    <string>10.11</string>

    <key>LSUIElement</key>
    <true/>  <!-- Menu bar only app, no Dock icon -->

    <key>NSHighResolutionCapable</key>
    <true/>

    <!-- Privacy & Security Permissions -->
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2024 Michael S. Huang</string>
</dict>
</plist>
```

### Key Info.plist Fields Explained

- **CFBundleExecutable**: Name of the binary in `Contents/MacOS/` to execute
- **CFBundleIdentifier**: Unique identifier (reverse domain notation) - matches what's in Cargo.toml
- **CFBundlePackageType**: Must be "APPL" for applications
- **CFBundleName**: Internal name (max 16 chars recommended)
- **CFBundleDisplayName**: User-visible name
- **LSMinimumSystemVersion**: Minimum macOS version (10.11 for this app)
- **LSUIElement**: Set to `true` for menu bar-only apps (no Dock icon when running)
- **NSHighResolutionCapable**: Enables Retina display support

## Implementation Options

### Option 1: cargo-bundle (Recommended)

`cargo-bundle` is a Cargo subcommand that automates the creation of macOS `.app` bundles from Rust projects.

**Pros:**
- Automated bundle creation
- Handles Info.plist generation
- Supports icon conversion
- Well-maintained and widely used
- Integrates with Cargo workflow

**Cons:**
- Additional dependency
- Less control over bundle details

### Option 2: Manual Bundle Creation

Create the `.app` structure manually using shell scripts or build scripts.

**Pros:**
- Full control over bundle structure
- No additional dependencies
- Can customize every detail

**Cons:**
- More maintenance overhead
- Error-prone
- Need to manually update Info.plist

### Option 3: tauri-bundler

Part of the Tauri project, provides advanced bundling capabilities.

**Pros:**
- Very feature-rich
- Supports DMG creation
- Code signing integration
- Cross-platform bundling

**Cons:**
- Heavyweight for simple apps
- More complex configuration

## Recommended Approach: cargo-bundle

### Installation

```bash
cargo install cargo-bundle
```

### Cargo.toml Configuration

Update `Cargo.toml` with complete bundle metadata:

```toml
[package]
name = "handsoff"
version = "0.1.0"
edition = "2021"
authors = ["Michael S. Huang"]
description = "macOS menu bar app to prevent accidental input during video calls"

[package.metadata.bundle]
name = "HandsOff"
identifier = "com.handsoff.inputlock"
version = "0.1.0"
copyright = "Copyright © 2024 Michael S. Huang"
category = "public.app-category.utilities"
short_description = "Prevent accidental input during video calls"
long_description = """
A macOS menu bar application that prevents accidental or unsolicited input from
keyboard, trackpad, and mouse devices during video conferencing, presentations,
or when leaving your laptop unattended.
"""

# Icon file (if you have one)
icon = ["assets/AppIcon.icns"]

# macOS-specific bundle configuration
osx_minimum_system_version = "10.11"
osx_frameworks = []
osx_url_schemes = []

# Resources to include in the bundle
resources = []
```

### Info.plist Customization

For additional Info.plist keys not supported by cargo-bundle, create a custom template:

1. Create `Info.plist.template` in the project root:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- cargo-bundle will populate these -->
    <key>CFBundleExecutable</key>
    <string>{{executable_name}}</string>

    <key>CFBundleIdentifier</key>
    <string>{{bundle_identifier}}</string>

    <key>CFBundlePackageType</key>
    <string>APPL</string>

    <key>CFBundleVersion</key>
    <string>{{version}}</string>

    <!-- Custom keys for menu bar app -->
    <key>LSUIElement</key>
    <true/>

    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
```

2. Reference it in `Cargo.toml`:

```toml
[package.metadata.bundle]
# ... other settings ...
osx_info_plist_template = "Info.plist.template"
```

## Build Process

### Step 1: Build the Release Binary

```bash
cargo build --release
```

For specific architectures:
```bash
# Apple Silicon
cargo build --release --target aarch64-apple-darwin

# Intel
cargo build --release --target x86_64-apple-darwin

# Universal binary (both architectures)
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create \
  target/aarch64-apple-darwin/release/handsoff \
  target/x86_64-apple-darwin/release/handsoff \
  -output target/release/handsoff-universal
```

### Step 2: Create the Bundle

```bash
cargo bundle --release
```

This creates: `target/release/bundle/osx/HandsOff.app/`

### Step 3: Verify Bundle Structure

```bash
ls -la target/release/bundle/osx/HandsOff.app/Contents/
```

Expected output:
```
drwxr-xr-x  Info.plist
drwxr-xr-x  MacOS/
drwxr-xr-x  Resources/
```

### Step 4: Test the Application

```bash
# Run directly
open target/release/bundle/osx/HandsOff.app

# Or copy to Applications and test
cp -r target/release/bundle/osx/HandsOff.app /Applications/
open /Applications/HandsOff.app
```

## Creating an Application Icon

### Icon Requirements

macOS icons should be in `.icns` format with multiple resolutions:

- 16x16 (1x and 2x)
- 32x32 (1x and 2x)
- 128x128 (1x and 2x)
- 256x256 (1x and 2x)
- 512x512 (1x and 2x)

### Creating .icns from PNG

1. Create a 1024x1024 PNG icon: `AppIcon.png`

2. Create an iconset:

```bash
mkdir AppIcon.iconset
sips -z 16 16     AppIcon.png --out AppIcon.iconset/icon_16x16.png
sips -z 32 32     AppIcon.png --out AppIcon.iconset/icon_16x16@2x.png
sips -z 32 32     AppIcon.png --out AppIcon.iconset/icon_32x32.png
sips -z 64 64     AppIcon.png --out AppIcon.iconset/icon_32x32@2x.png
sips -z 128 128   AppIcon.png --out AppIcon.iconset/icon_128x128.png
sips -z 256 256   AppIcon.png --out AppIcon.iconset/icon_128x128@2x.png
sips -z 256 256   AppIcon.png --out AppIcon.iconset/icon_256x256.png
sips -z 512 512   AppIcon.png --out AppIcon.iconset/icon_256x256@2x.png
sips -z 512 512   AppIcon.png --out AppIcon.iconset/icon_512x512.png
sips -z 1024 1024 AppIcon.png --out AppIcon.iconset/icon_512x512@2x.png
```

3. Convert to .icns:

```bash
iconutil -c icns AppIcon.iconset -o assets/AppIcon.icns
```

4. Reference in `Cargo.toml`:

```toml
[package.metadata.bundle]
icon = ["assets/AppIcon.icns"]
```

## Code Signing (Optional but Recommended)

### Development Signing

```bash
codesign --force --deep --sign - target/release/bundle/osx/HandsOff.app
```

### Distribution Signing

For distribution outside the App Store, you need a Developer ID certificate:

```bash
codesign --force --deep \
  --sign "Developer ID Application: Your Name (TEAM_ID)" \
  --options runtime \
  target/release/bundle/osx/HandsOff.app
```

### Notarization (for Gatekeeper)

```bash
# Create a ZIP for notarization
ditto -c -k --keepParent HandsOff.app HandsOff.zip

# Submit for notarization
xcrun notarytool submit HandsOff.zip \
  --apple-id "your@email.com" \
  --password "app-specific-password" \
  --team-id "TEAM_ID" \
  --wait

# Staple the notarization ticket
xcrun stapler staple HandsOff.app
```

## Distribution

### Option 1: Direct .app Distribution

Simply distribute the `HandsOff.app` bundle:

```bash
zip -r HandsOff-v0.1.0-macos.zip HandsOff.app
```

Users can extract and copy to `/Applications`.

### Option 2: DMG Installer

Create a disk image for professional distribution:

```bash
# Create a temporary directory
mkdir -p dmg-contents
cp -r HandsOff.app dmg-contents/
ln -s /Applications dmg-contents/Applications

# Create the DMG
hdiutil create -volname "HandsOff" \
  -srcfolder dmg-contents \
  -ov -format UDZO \
  HandsOff-v0.1.0.dmg

# Clean up
rm -rf dmg-contents
```

### Option 3: Homebrew Cask

For easier installation via Homebrew:

```ruby
cask "handsoff" do
  version "0.1.0"
  sha256 "..."

  url "https://github.com/username/handsoff-rs/releases/download/v#{version}/HandsOff-v#{version}.dmg"
  name "HandsOff"
  desc "Prevent accidental input during video calls"
  homepage "https://github.com/username/handsoff-rs"

  app "HandsOff.app"
end
```

## Build Automation

### Makefile

Create a `Makefile` for common build tasks:

```makefile
.PHONY: build bundle sign dmg clean

build:
	cargo build --release

bundle: build
	cargo bundle --release

sign: bundle
	codesign --force --deep --sign - \
		target/release/bundle/osx/HandsOff.app

dmg: sign
	mkdir -p dmg-contents
	cp -r target/release/bundle/osx/HandsOff.app dmg-contents/
	ln -s /Applications dmg-contents/Applications
	hdiutil create -volname "HandsOff" \
		-srcfolder dmg-contents \
		-ov -format UDZO \
		dist/HandsOff-$(shell cargo pkgid | cut -d\# -f2).dmg
	rm -rf dmg-contents

clean:
	cargo clean
	rm -rf target/release/bundle
	rm -f dist/*.dmg

install: bundle
	cp -r target/release/bundle/osx/HandsOff.app /Applications/
```

Usage:
```bash
make bundle    # Build and create .app bundle
make sign      # Build, bundle, and sign
make dmg       # Build, bundle, sign, and create DMG
make install   # Build, bundle, and copy to /Applications
```

### GitHub Actions

Automate building and releasing:

```yaml
name: Build macOS App

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3

      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Install cargo-bundle
        run: cargo install cargo-bundle

      - name: Build release
        run: cargo build --release

      - name: Create bundle
        run: cargo bundle --release

      - name: Sign app
        run: |
          codesign --force --deep --sign - \
            target/release/bundle/osx/HandsOff.app

      - name: Create DMG
        run: |
          mkdir -p dmg-contents
          cp -r target/release/bundle/osx/HandsOff.app dmg-contents/
          ln -s /Applications dmg-contents/Applications
          hdiutil create -volname "HandsOff" \
            -srcfolder dmg-contents \
            -ov -format UDZO \
            HandsOff-${{ github.ref_name }}.dmg

      - name: Upload Release Asset
        uses: actions/upload-release-asset@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          upload_url: ${{ github.event.release.upload_url }}
          asset_path: ./HandsOff-${{ github.ref_name }}.dmg
          asset_name: HandsOff-${{ github.ref_name }}.dmg
          asset_content_type: application/x-apple-diskimage
```

## Testing & Verification

### Checklist

After creating the bundle, verify:

- [ ] Bundle structure is correct (`Info.plist`, `MacOS/`, `Resources/`)
- [ ] Double-clicking opens the app without Terminal
- [ ] Menu bar icon appears (🔓/🔒)
- [ ] No Dock icon appears (LSUIElement working)
- [ ] App requests Accessibility permissions on first run
- [ ] All hotkeys work correctly
- [ ] Touch ID unlock works (if available)
- [ ] App can be quit and reopened
- [ ] App survives system restarts

### Testing Commands

```bash
# Check bundle structure
find HandsOff.app -type f

# Verify Info.plist
plutil -lint HandsOff.app/Contents/Info.plist

# Check executable permissions
ls -l HandsOff.app/Contents/MacOS/handsoff

# View Info.plist contents
plutil -p HandsOff.app/Contents/Info.plist

# Check code signature
codesign -dvvv HandsOff.app

# Verify architecture
lipo -info HandsOff.app/Contents/MacOS/handsoff
```

### Common Issues

**App doesn't start:**
- Check executable permissions: `chmod +x HandsOff.app/Contents/MacOS/handsoff`
- Verify CFBundleExecutable matches actual binary name
- Check for crashes in Console.app

**Still opens Terminal:**
- Ensure bundle has `.app` extension
- Verify Info.plist exists and is valid
- Check LSUIElement value if Dock icon appears

**No menu bar icon:**
- Verify Accessibility permissions are granted
- Check console logs for errors
- Ensure app isn't crashing on startup

**Gatekeeper blocks app:**
- Sign the app with codesign
- Consider notarization for distribution
- Users can override: System Settings > Privacy & Security > "Open Anyway"

## Future Enhancements

### Automatic Updates

Consider integrating Sparkle framework for automatic updates:
- https://sparkle-project.org/
- Requires code signing and HTTPS hosting

### Launch at Login

Add a launch agent or use the macOS Login Items API:
- https://developer.apple.com/documentation/servicemanagement

### Preferences Window

Consider adding a preferences UI using native Cocoa APIs or a Rust GUI framework like `iced` or `egui`.

## References

- [Apple Bundle Documentation](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)
- [cargo-bundle GitHub](https://github.com/burtonageo/cargo-bundle)
- [Info.plist Keys Reference](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Introduction/Introduction.html)
- [Code Signing Guide](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [macOS Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/macos)

## Summary

Converting `handsoff` to a proper macOS application requires:

1. **Install cargo-bundle**: `cargo install cargo-bundle`
2. **Update Cargo.toml**: Add complete `[package.metadata.bundle]` section
3. **Build bundle**: `cargo bundle --release`
4. **Test**: `open target/release/bundle/osx/HandsOff.app`
5. **Distribute**: Create DMG or ZIP for users

This transforms the raw binary into a double-clickable macOS application that integrates properly with the operating system's UI conventions and user expectations.
