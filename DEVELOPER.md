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
- **`ring`**: Cryptographic hashing (SHA-256 over passphrase keycode sequences for storage)
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

### Configuration Dependencies

- **`toml`**: TOML file parsing for config.toml
- **`serde`**: Serialization/deserialization framework
- **`dirs`**: Standard config directory paths
- **`rpassword`**: Non-echoing text input for setup confirmations

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
├── config.rs               # Environment variable parsing (optional overrides)
├── setup.rs                # Keycode-sequence capture for --setup (event tap)
├── config_file.rs          # Config file management (hashed passphrase)
└── bin/                    # Binary entry points
    ├── handsoff.rs         # CLI binary
    └── handsoff-tray.rs    # Tray App binary
```

**Architecture:**
- **Core Library** (`lib.rs`): Shared functionality (input blocking, state management, auth)
- **CLI Binary** (`bin/handsoff.rs`): Terminal-based interface with clap argument parsing
- **Tray App Binary** (`bin/handsoff-tray.rs`): Native macOS menu bar app with tray-icon and notifications

---

## Passphrase Hashing

The application stores a SHA-256 hash of the passphrase's **physical keycode sequence** in `config.toml` (format `keycode-v1`, per `specs/deep-design-review-v2-2026-09.md` §3). Passphrases are layout-independent: what matters is which keys are pressed, not the characters they produce.

### Key Features

- **Keycode-sequence capture**: setup uses a temporary event tap (interactive console sessions only — refused over SSH)
- **Layout-independent**: no char decoding in the unlock path; raw keycodes are hashed and compared
- **Reserved keys rejected**: Escape, Backspace, and the hotkey keys chosen during setup cannot be passphrase members (setup prompts for hotkeys before capture, so the reserved set always matches what the runtime will register); minimum 4 keys
- **No plaintext**: only the SHA-256 hex hash is stored; Reset is an explicit user recovery action that clears lock state and restarts the backoff schedule (logged as `Reset: state cleared…`, distinct from passphrase auth), so no plaintext is ever retained

### Implementation Details

**File:** `src/utils/mod.rs`

- Hash = SHA-256 over the big-endian encoding of each u32 keycode
- Comparison of hex digests; length check + fold-XOR compare

**File:** `src/setup.rs`

- `capture_passphrase()` installs a throwaway `CGEventTap` during `--setup`, runs a nested CFRunLoop, blocks captured keys from reaching apps, tears down the tap before returning

**File:** `src/config_file.rs`

Configuration management:
- Location: `~/Library/Application Support/handsoff/config.toml`
- Format: TOML with `passphrase_hash` field
- Permissions: 600 (user read/write only)
- Fields: `passphrase_hash`, `passphrase_format`, `auto_lock_timeout`, `auto_unlock_mode`, `lock_hotkey`, `talk_hotkey`

### Security Considerations

**What this protects against:**
- ✅ Casual interference (child/colleague/screenshare — the V5 threat model)
- ✅ Keyboard layout mismatches (legacy weakness L-1) — sequences are layout-independent
- ✅ Untypeable passphrases — what is captured is what must be typed back

**What this does NOT protect against (accepted residuals, spec §3.1/§5):**
- ❌ Offline brute force of the hash by a local account (4-key sequences ≈ 50⁴)
- ❌ Determined local actors (reboot bypass, killing the app)

### Migration

Legacy configs with `encrypted_passphrase` (or any `passphrase_format` other than `keycode-v1`) are **rejected on load** and force a one-time re-setup. The app never silently keeps an unverifiable passphrase.

---

## Auto-Unlock Safety Feature

The auto-unlock feature is an **exponential backoff schedule**, not a single timeout (per `specs/deep-design-review-v2-2026-09.md` §2). It prevents permanent lockouts from bugs, forgotten passphrases, or other unexpected issues.

**Enabled by default** (V2). The first unlock window opens at the base interval (60 min) of **awake time** after lock, then the interval doubles each window: 60 min → 2 h → 4 h → 8 h … capped at 24 h. `Instant` clocks pause during sleep, so the schedule counts awake-time only (§2.4) — a locked-then-slept machine does not unlock on wall-clock.

**Reset rule (§2.3, the linchpin):** the backoff counter advances across a locked stretch. Auto-lock re-engagements and fired windows do NOT reset it — **only a successful passphrase unlock resets the schedule to the base interval**. Without this rule, every 120 s auto-lock re-engagement would restart the schedule at 60 min and the doubling would never engage. The tray Reset menu additionally restarts the schedule by explicit user action (logged distinctly).

**Window semantics (§2.2):** a window is `auto_lock_timeout` (default 120 s) of no input; any input resets the idle timer and extends it. The effective lifetime of the lock against stray input is the base interval.

**Relaunch/reboot (§2.5):** the app starts unlocked after a relaunch — a reboot ends the locked state (accepted bypass).

### Configuring Auto-Unlock

Set the `HANDS_OFF_AUTO_UNLOCK` environment variable to override the **base interval** in seconds:

```bash
# Override base interval to 5 minutes (for quick testing)
HANDS_OFF_AUTO_UNLOCK=300 cargo run

# Override base interval to 2 hours
HANDS_OFF_AUTO_UNLOCK=7200 ./handsoff

# Disable auto-unlock entirely
HANDS_OFF_AUTO_UNLOCK=0 ./handsoff

# Unset (default): base interval from config schema (60 min), enabled
./handsoff
```

### Valid Configuration Values

- **Base interval minimum:** 60 seconds
- **Base interval maximum:** 86400 seconds (24 h — the window ceiling)
- **Disabled:** `0`
- **Invalid values** will be ignored with a warning

In `config.toml`, auto-unlock is stored as a **mode** (`auto_unlock_mode: "backoff"` or `"disabled"`), not a scalar timeout (§2.7).

### How It Works

1. When you lock the device, the schedule is anchored and the pending window interval is `base × 2^window_index`
2. Every 10 seconds, the auto-unlock thread checks whether the window has opened (awake-time)
3. When a window opens:
   - The counter advances (next window doubles)
   - Input interception is automatically disabled — **silently** (V10: no unlock notification)
   - The menu bar icon updates to unlocked state
   - The event is logged at WARNING level for audit purposes
4. Only a successful passphrase unlock resets the counter to the base interval

### Use Cases

**Development/Testing:**
```bash
# Quick testing with a 5-minute base interval
HANDS_OFF_AUTO_UNLOCK=300 cargo run
```

**Personal Use (Emergency Failsafe):**
```bash
# First window at ~1 hour of awake time; doubling up to 24 h
HANDS_OFF_AUTO_UNLOCK=3600 ./handsoff
```

**Launch Agent (Permanent Configuration):**
```xml
<!-- ~/Library/LaunchAgents/com.handsoff.inputlock.plist -->
<key>EnvironmentVariables</key>
<dict>
    <key>HANDS_OFF_AUTO_UNLOCK</key>
    <string>3600</string>  <!-- 60-minute base interval -->
</dict>
```

### Security Implications

**Benefits:**
- Prevents permanent lockout from bugs or forgotten passphrases
- The doubling schedule means an unauthenticated unlock gets progressively harder to rely on
- Logged for audit purposes

**Risks:**
- An attacker who knows the feature exists could wait for a window
- Windows extend under input (idle-capped) — an at-keyboard masher can hold a window open, but only within the first 60-min stretch (§2.2 accepted consequence)
- Not suitable for public/shared computers

### Verification

When auto-unlock is enabled, check the logs at startup:

```bash
# You should see this in the logs
INFO  Auto-unlock backoff enabled: first window at 3600s, doubling up to 86400s
INFO  Auto-unlock backoff monitoring thread started
```

When a window fires:

```bash
WARN  Auto-unlock window opened - releasing input (unauthenticated)
WARN  AUTO-UNLOCK WINDOW FIRED after Ns awake-time
```

### Troubleshooting Auto-Unlock

#### Auto-unlock not triggering

**Check if feature is enabled:**
```bash
# Verify environment variable is set
echo $HANDS_OFF_AUTO_UNLOCK

# Run with logging to see status
RUST_LOG=info HANDS_OFF_AUTO_UNLOCK=300 ./handsoff
```

**Common issues:**
- Auto-unlock disabled via `HANDS_OFF_AUTO_UNLOCK=0` or `auto_unlock_mode = "disabled"` in config
- Device was not actually locked (check menu bar icon)
- Machine is asleep — the schedule counts awake-time only (§2.4)

**Expected behavior:**
- Auto-unlock thread logs "Auto-unlock backoff monitoring thread started" at startup
- Triggers within 10 seconds of the window opening (thread sleeps 10s between checks)

#### Auto-unlock timer seems inaccurate

This is **expected behavior**, not a bug:
- The monitoring thread sleeps for 10 seconds between checks
- A window fires within 0–10 seconds after it opens
- Sleep/wake pauses the schedule (awake-time semantics) — a locked-then-slept machine will unlock later than wall-clock predicts

#### Locked out despite auto-unlock being enabled

**Emergency recovery options:**

1. **SSH from another device** (if SSH is enabled):
   ```bash
   ssh user@your-mac
   pkill -f HandsOff
   ```

2. **Hard Reboot** (last resort, accepted bypass per §2.5):
   - Hold power button until Mac shuts down
   - The app relaunches unlocked

---

## Contributing

Contributions are welcome! Please ensure:
- Code follows Rust best practices
- All tests pass: `cargo test`
- Build succeeds for both binaries: `cargo build --release`
- No clippy warnings: `cargo clippy`

## License

See LICENSE file for details.
