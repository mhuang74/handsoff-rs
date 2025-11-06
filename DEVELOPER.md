# Developer Guide

This guide is for developers who want to build HandsOff from source, understand the technical implementation, or use the auto-unlock safety feature during development.

## Table of Contents

- [Building from Source](#building-from-source)
- [Tech Stack](#tech-stack)
- [Auto-Unlock Safety Feature](#auto-unlock-safety-feature)
- [Project Structure](#project-structure)

---

## Building from Source

### Build Both Binaries

```bash
# Clone the repository
git clone https://github.com/your-repo/handsoff-rs.git
cd handsoff-rs

# Build both CLI and Tray App
cargo build --release

# The binaries will be at:
# - target/release/handsoff (CLI)
# - target/release/handsoff-tray (Tray App)
```

### Build Individual Binaries

```bash
# CLI only
cargo build --release --bin handsoff

# Tray App only
cargo build --release --bin handsoff-tray
```

### Build for Specific Architecture

For Apple Silicon Macs:
```bash
cargo build --release --target aarch64-apple-darwin
```

For Intel Macs:
```bash
cargo build --release --target x86_64-apple-darwin
```

### Universal Binary (Both Architectures)

```bash
# Install targets
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin

# Build for both architectures
cargo build --release --target x86_64-apple-darwin --bin handsoff
cargo build --release --target aarch64-apple-darwin --bin handsoff

# Combine with lipo
lipo -create \
  target/x86_64-apple-darwin/release/handsoff \
  target/aarch64-apple-darwin/release/handsoff \
  -output target/release/handsoff-universal
```

---

## Tech Stack

HandsOff is built with Rust and leverages the following libraries:

### Core Dependencies

- **`core-graphics`**: CoreGraphics event handling (CGEventTap implementation)
- **`core-foundation`**: CFRunLoop integration for event tap
- **`security-framework`**: macOS Security Framework bindings
- **`ring`**: Cryptographic hashing (SHA-256 for passphrase verification)
- **`parking_lot`**: Fast mutex implementation for shared state
- **`anyhow`**: Error handling and context
- **`log`** / **`env_logger`**: Logging infrastructure

### Tray App Dependencies

- **`tray-icon`**: Native macOS menu bar icon
- **`tao`**: Cross-platform event loop
- **`notify-rust`**: Native macOS notifications
- **`image`**: PNG decoder for app icons

### CLI Dependencies

- **`clap`**: Command-line argument parsing

### Input Handling

- **`global-hotkey`**: Global hotkey registration (Ctrl+Cmd+Shift+L, Ctrl+Cmd+Shift+T)

### Project Structure

```
src/
├── lib.rs                  # Core library (HandsOffCore)
├── app_state.rs            # Shared application state
├── auth/                   # Authentication modules
│   └── mod.rs              # Passphrase verification
├── input_blocking/         # Input blocking modules
│   ├── mod.rs              # Event handling and passphrase entry
│   ├── event_tap.rs        # CGEventTap implementation
│   └── hotkeys.rs          # Global hotkey handling
├── utils/                  # Utility modules
│   ├── mod.rs              # SHA-256 hashing utilities
│   └── keycode.rs          # Keycode to character mapping
├── config.rs               # Configuration parsing
└── bin/                    # Binary entry points
    ├── handsoff.rs         # CLI binary
    └── handsoff-tray.rs    # Tray App binary
```

**Architecture:**
- **Core Library** (`lib.rs`): Shared functionality (input blocking, state management, auth)
- **CLI Binary** (`bin/handsoff.rs`): Terminal-based interface with clap argument parsing
- **Tray App Binary** (`bin/handsoff-tray.rs`): Native macOS menu bar app with tray-icon and notifications

---

## Auto-Unlock Safety Feature

The auto-unlock feature provides a fail-safe mechanism that automatically disables input interception after a configurable timeout. This prevents permanent lockouts due to bugs, forgotten passphrases during development, or other unexpected issues.

**⚠️ Important:** This feature is designed for **development, testing, and personal emergency use only**. It should NOT be enabled in production environments where security is critical.

### Enabling Auto-Unlock

Set the `HANDS_OFF_AUTO_UNLOCK` environment variable to the desired timeout in seconds:

```bash
# Enable with 30-second timeout (for quick testing)
HANDS_OFF_AUTO_UNLOCK=30 cargo run

# Enable with 5-minute timeout (for development)
HANDS_OFF_AUTO_UNLOCK=300 ./handsoff

# Enable with 10-minute timeout (more conservative)
HANDS_OFF_AUTO_UNLOCK=600 ./handsoff

# Disabled (default behavior - no auto-unlock)
./handsoff
```

### Valid Configuration Values

- **Minimum:** 60 seconds
- **Maximum:** 900 seconds (15 minutes)
- **Disabled:** 0 or unset (default)
- **Invalid values** (below 60 or above 900) will disable the feature with a warning

### How It Works

1. When you lock the device, a timer starts counting
2. Every 10 seconds, the app checks if the timeout has been exceeded
3. If the timeout expires while locked:
   - Input interception is automatically disabled
   - A prominent notification appears: "HandsOff Auto-Unlock Activated"
   - The menu bar icon updates to unlocked state
   - The event is logged at WARNING level for audit purposes
4. If you manually unlock before the timeout, the timer resets

### Use Cases

**Development/Testing:**
```bash
# Quick testing during development
HANDS_OFF_AUTO_UNLOCK=30 cargo run
```

**Personal Use (Emergency Failsafe):**
```bash
# Set a 10-minute failsafe in case you forget your passphrase
HANDS_OFF_AUTO_UNLOCK=600 ./handsoff
```

**Launch Agent (Permanent Configuration):**
```xml
<!-- ~/Library/LaunchAgents/com.handsoff.inputlock.plist -->
<key>EnvironmentVariables</key>
<dict>
    <key>HANDS_OFF_AUTO_UNLOCK</key>
    <string>300</string>  <!-- 5 minutes -->
</dict>
```

### Security Implications

**Benefits:**
- Prevents denial-of-service if bugs occur
- Provides emergency access during development
- Logged for audit purposes

**Risks:**
- Reduces security if timeout is too short
- An attacker who knows the feature exists could wait for auto-unlock
- Not suitable for public/shared computers

**Recommendations:**
- ✅ Use for development and testing
- ✅ Use with longer timeouts (5-10 minutes) for personal devices
- ❌ Do NOT use in production/public environments
- ❌ Do NOT set timeouts shorter than 60 seconds for actual use
- ❌ Do NOT enable on shared computers

### Verification

When auto-unlock is enabled, check the logs at startup:

```bash
# You should see this in the logs
INFO  Auto-unlock safety feature enabled: 30 seconds
INFO  Auto-unlock monitoring thread started
```

When auto-unlock triggers:

```bash
WARN  Auto-unlock timeout expired - disabling input interception
WARN  AUTO-UNLOCK TRIGGERED after 30 seconds
INFO  Auto-unlock notification delivered
```

### Troubleshooting Auto-Unlock

#### Auto-unlock not triggering

**Check if feature is enabled:**
```bash
# Verify environment variable is set
echo $HANDS_OFF_AUTO_UNLOCK

# Run with logging to see status
RUST_LOG=info HANDS_OFF_AUTO_UNLOCK=30 ./handsoff
```

**Common issues:**
- Environment variable not set or set to invalid value
- Value is outside valid range (60-900 seconds)
- Device was not actually locked (check menu bar icon)
- Manual unlock occurred before timeout expired

**Expected behavior:**
- Feature logs "Auto-unlock safety feature enabled: X seconds" at startup
- Auto-unlock thread logs "Auto-unlock monitoring thread started"
- Triggers within 10 seconds of configured timeout (thread sleeps 10s between checks)

#### Auto-unlock notification not appearing

**Check notification permissions:**
```bash
# Ensure app can send notifications
# System Settings > Notifications > HandsOff
```

**Check logs for errors:**
```bash
RUST_LOG=debug ./handsoff
# Look for: "Failed to get notification center" or similar errors
```

**Try manually opening Notification Center** while the app is running to ensure notifications are enabled

#### Auto-unlock timer seems inaccurate

This is **expected behavior**, not a bug:
- The monitoring thread sleeps for 10 seconds between checks
- Auto-unlock will trigger within 0-10 seconds after the configured timeout
- Example: With `HANDS_OFF_AUTO_UNLOCK=30`, unlock will occur between 30-40 seconds after lock
- This design balances accuracy with CPU efficiency

#### Locked out despite auto-unlock being enabled

**Emergency recovery options:**

1. **SSH from another device** (if SSH is enabled):
   ```bash
   ssh user@your-mac
   pkill -f HandsOff
   ```

2. **Hard Reboot** (last resort):
   - Hold power button until Mac shuts down
   - If HandsOff locks right away after login, try booting into Safe Mode

**Prevention:**
- Always test auto-unlock works before relying on it
- Start with short timeout (30s) for testing
- Increase to longer timeout (5-10 minutes) for actual use
- Keep terminal window visible to see logs during testing

---

## Contributing

Contributions are welcome! Please ensure:
- Code follows Rust best practices
- All tests pass: `cargo test`
- Build succeeds for both binaries: `cargo build --release`
- No clippy warnings: `cargo clippy`

## License

See LICENSE file for details.
