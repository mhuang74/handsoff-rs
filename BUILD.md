# Building HandsOff as a macOS Application

## TL;DR

On a Mac, build the distributable installer with one command:

```bash
make pkg
```

Produces `dist/HandsOff-v{VERSION}.pkg` — the recommended distribution
artifact. The installer installs HandsOff.app and creates a Launch Agent that
starts the app at login (the postinstall is non-interactive); the user then
runs the one-time setup wizard from the installed app (see
[installer/INSTALLER-GUIDE.md](installer/INSTALLER-GUIDE.md) for the exact
post-install steps):

```bash
~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
```

Caveats:

- **Bundling and packaging are macOS-only** (`cargo-bundle`, `pkgbuild`,
  `productsign`); the Linux path below only cross-compiles/validates binaries.
- **`productsign` needs a signing identity**: `installer/build-pkg.sh` signs the
  package with `--sign "Installer Signing Self-Signed"`. On a fresh Mac this
  identity doesn't exist, so `make pkg` fails at the final step. Either create
  a self-signed *Installer* certificate with that name (Keychain Access →
  Certificate Assistant → Create a Certificate) or drop the `productsign` step
  — the unsigned package before it is still valid.
- For plain binaries without bundling: `cargo build --release` (macOS) or the
  cross-compile route below (Linux). Details: [Quick Start](#quick-start) and
  the rest of this document.

## Scope

This document covers building HandsOff as a proper macOS application bundle, building natively on macOS, and cross-compiling/validating the macOS build from a Linux host.

## Prerequisites

1. **Rust toolchain**: Install from [rustup.rs](https://rustup.rs/)
2. **Apple targets** (for explicit-target or universal builds):
   ```bash
   rustup target add aarch64-apple-darwin x86_64-apple-darwin
   ```
3. **cargo-bundle** (bundle creation only): Install with `cargo install cargo-bundle`

Note: the Linux host `cargo test` currently fails with
`error[E0455]: link kind "framework" is only supported on Apple targets`
(`core-graphics-types` emits a framework link attribute; pre-existing, not a regression).
Run tests on macOS or via the Linux cross-compile path below.

---

## Building on macOS

On a macOS machine, the standard toolchain is enough — no cross-compilation setup required:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin  # optional; only needed for cross-arch/universal builds

cargo build --release
cargo test
```

Binaries land at `target/release/handsoff` and `target/release/handsoff-tray`.
For per-architecture and universal (`lipo`) builds, see the "Build Architecture"
section below. For the distributable `.app` bundle and `.pkg` installer, use the
Makefile workflow that follows (`make all`, `make pkg`) — bundling uses
macOS-only tools (`cargo-bundle`, `plutil`, `codesign`), so it must run on macOS.

## Quick Start

**Building an installable macOS package** (on a Mac) — the primary build entry
point is the Makefile, and the package is its most complete target:

```bash
# Create the .pkg installer (builds the release binaries, the .app bundle,
# and packages them with Launch Agent setup)
make pkg
```

Output: `dist/HandsOff-v{VERSION}.pkg` — a double-clickable installer that
installs HandsOff.app and creates a Launch Agent so the app starts at login
(the postinstall is non-interactive; the passphrase is set afterwards via the
setup wizard). The exact post-install steps are in
[installer/INSTALLER-GUIDE.md](installer/INSTALLER-GUIDE.md); see also
[Distribution → Option 1](#option-1-pkg-installer-with-launch-agent-recommended).

Other useful entry points:

```bash
# Build the .app bundle only (no installer)
make            # or: make all

# Development feedback loop (tests, lints)
make test && make clippy

# Show all targets
make help
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
- **LSMinimumSystemVersion**: `13.0` - Minimum macOS version

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

## Cross-Compiling and Validating the macOS Build on Linux

A Linux box can compile and link **full macOS binaries** (including
`src/setup.rs`, which plain Linux builds never see) using
[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) with zig as the
cross toolchain, plus a macOS SDK for framework headers/stubs. This is
**compile-and-link validation only**: you cannot execute Mach-O test binaries
on Linux, and you cannot bundle/sign (needs macOS tools). Tests must still be
*run* on macOS or via CI (`.github/workflows/rust.yml` on `macos-latest`).

### One-Time Setup

1. **rustup with the stable toolchain and Apple std libraries** — required even
   with zig, because the Rust *standard library* for `*-apple-darwin` ships only
   via rustup (zig supplies libc/framework linking, not rust std):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile minimal
   source "$HOME/.cargo/env"
   rustup target add aarch64-apple-darwin x86_64-apple-darwin
   ```

2. **zig** (any 0.13+ release):
   ```bash
   curl -sSfL https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz \
     | tar -xJ -C "$HOME/.local" \
   && mv "$HOME/.local/zig-linux-x86_64-0.13.0" "$HOME/.local/zig"
   export PATH="$PATH:$HOME/.local/zig"
   ```

3. **cargo-zigbuild**:
   ```bash
   cargo install --locked cargo-zigbuild
   ```

4. **A macOS SDK** for the Apple framework headers (`Cocoa/Cocoa.h`) and
   `.tbd` stubs. Zig ships only libSystem — without an SDK,
   `mac-notification-sys`'s C build script fails with
   `'Cocoa/Cocoa.h' file not found`, and linking fails with
   `framework ... not found`. Download a mirrored SDK, e.g.:
   ```bash
   # ~50 MB; resume-capable retry loop recommended on flaky links
   until tar -tf /tmp/sdk.tar.xz >/dev/null 2>&1; do
     curl -sSfL --retry 5 --retry-all-errors -C - \
       https://github.com/phracker/MacOSX-SDKs/releases/download/11.3/MacOSX11.3.sdk.tar.xz \
       -o /tmp/sdk.tar.xz
   done
   mkdir -p "$HOME/.local/macos-sdk"
   tar -xf /tmp/sdk.tar.xz -C "$HOME/.local/macos-sdk" && rm /tmp/sdk.tar.xz
   ```

### Build and Validate

```bash
source "$HOME/.cargo/env"
export PATH="$PATH:$HOME/.local/zig"
export SDKROOT="$HOME/.local/macos-sdk/MacOSX11.3.sdk"

# Release binaries (both architectures)
cargo zigbuild --target aarch64-apple-darwin --release
cargo zigbuild --target x86_64-apple-darwin  --release

# Compile test binaries against the real macOS SDK — this is what validates
# macOS-only source (e.g. src/setup.rs) and cfg(test) code locally:
cargo zigbuild --target aarch64-apple-darwin --tests
cargo zigbuild --target x86_64-apple-darwin  --tests
```

**Verify the outputs** (each should report `Mach-O 64-bit arm64 executable` or
`x86_64 executable`):

```bash
file target/aarch64-apple-darwin/release/handsoff
file target/x86_64-apple-darwin/release/handsoff
```

Notes:
- `cargo zigbuild` proxies `cargo build`, so use `--tests` (it does not accept
  `cargo test`'s `--no-run`). Compiled test binaries *cannot be executed* on
  the Linux host — they are Mach-O. Executing them requires macOS/CI.
- `SDKROOT` must be exported in every build session — cargo-zigbuild passes it
  to zig cc for framework/header resolution.
- Watch out for piped commands masking the exit code (`cargo zigbuild ... |
  tail` succeeds even when the build fails); use `set -o pipefail` or
  `${PIPESTATUS[0]}` when scripting.
- The resulting binaries are unsigned; Gatekeeper behavior is covered in the
  "Code Signing and Distribution" section above.

### What Linux Validation Covers

| Step | Coverage |
|---|---|
| `cargo zigbuild --release` | Full compile + link of both binaries against real SDK — catches macOS-only compile errors (`src/setup.rs`, `event_tap.rs`) locally |
| `cargo zigbuild --tests` | Compiles unit + integration test harnesses (same coverage as above) |
| Executing tests / running the app | **Not possible on Linux** — macOS machine or CI required |

---

## References

- [Spec Document](specs/build_as_macos_app.md) - Complete implementation specification
- [cargo-bundle GitHub](https://github.com/burtonageo/cargo-bundle)
- [Apple Bundle Documentation](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)
- [Info.plist Keys Reference](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Introduction/Introduction.html)
