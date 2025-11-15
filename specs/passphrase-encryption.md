# Passphrase Security Enhancement

**Status:** Planned
**Date:** 2025-11-05
**Priority:** High - Security Enhancement

## Problem Statement

Currently, the secret passphrase is stored in plaintext in two locations:
1. **Tray App**: Stored in `~/Library/LaunchAgents/com.handsoff.inputlock.plist` as an environment variable
2. **CLI**: Read from environment variable `HANDS_OFF_SECRET_PHRASE`

While the plist file has 600 permissions (user-only), the passphrase is still stored in plaintext and could be exposed through:
- File system access by malicious applications running as the user
- Backup systems that archive the LaunchAgents directory
- Process inspection tools that can view environment variables

## Solution Overview

Replace plaintext storage with AES-256-GCM encrypted storage in a centralized `config.toml` file. Both CLI and Tray App will read from this encrypted configuration file.

### Key Features
- **AES-256-GCM encryption** for passphrase at rest
- **Static key derivation** - configs portable across versions/builds
- **Interactive setup command** with non-echoing password input
- **Unified configuration** for both CLI and Tray App modes
- **Standard config directory** (`~/Library/Application Support/handsoff/config.toml`)
- **Simplified installer** - automatic LaunchAgent setup (no manual script execution)
- **Seamless updates** - config survives version upgrades

### Architecture Change Highlights

**Before:**
1. Install .pkg → postinstall shows instructions
2. User runs `setup-launch-agent.sh` → prompts for passphrase → creates plist with plaintext env var
3. App reads passphrase from environment variable

**After:**
1. Install .pkg → postinstall automatically creates plist (no secrets) and loads launch agent
2. User runs `--setup` command → prompts for passphrase → saves to encrypted config.toml
3. App reads from encrypted config file

**Benefits:**
- No plaintext secrets in plist file
- Simpler installation (no separate setup script)
- Automatic launch agent configuration
- Better separation of concerns (installer handles system config, --setup handles user secrets)

---

## Implementation Plan

### 1. Add Dependencies

Update `Cargo.toml` to include:

```toml
[dependencies]
# Existing dependencies...

# New dependencies for encryption
aes-gcm = "0.10"           # AES-GCM authenticated encryption
rpassword = "7.3"          # Non-echoing password input
dirs = "5.0"               # Standard config directory paths
base64 = "0.22"            # Encode/decode encrypted data
sha2 = "0.10"              # SHA-256 for key derivation

# Verify these exist (should already be present):
toml = "0.8"               # TOML parsing
serde = { version = "1.0", features = ["derive"] }
```

### 2. Create Encryption Module

**File:** `src/crypto.rs`

**Purpose:** Provide AES-256-GCM encryption/decryption functions with static key derivation.

**Implementation details:**

```rust
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce
};
use sha2::{Sha256, Digest};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

// Static seed for key derivation (consistent across all builds/versions)
const KEY_SEED: &str = "com.handsoff.inputlock.config.encryption.v1";

// Derive 32-byte AES-256 key from static seed
fn derive_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_SEED.as_bytes());
    hasher.finalize().into()
}

// Encrypt plaintext passphrase
pub fn encrypt_passphrase(plaintext: &str) -> Result<String, Box<dyn std::error::Error>> {
    let key = derive_key();
    let cipher = Aes256Gcm::new(&key.into());

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    getrandom::getrandom(&mut nonce_bytes)?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes())?;

    // Concatenate: nonce || ciphertext (ciphertext includes auth tag)
    let mut result = Vec::new();
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    // Return base64-encoded
    Ok(BASE64.encode(&result))
}

// Decrypt encrypted passphrase
pub fn decrypt_passphrase(encrypted: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Decode base64
    let data = BASE64.decode(encrypted)?;

    if data.len() < 12 {
        return Err("Invalid encrypted data".into());
    }

    // Extract nonce (first 12 bytes) and ciphertext (rest)
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let key = derive_key();
    let cipher = Aes256Gcm::new(&key.into());
    let plaintext = cipher.decrypt(nonce, ciphertext)?;

    // Convert to string
    Ok(String::from_utf8(plaintext)?)
}
```

**Security considerations:**
- **Static key**: Uses SHA-256(constant string) for consistent key across versions
- **Random nonces**: Each encryption uses a unique random nonce
- **AES-256-GCM**: Provides both encryption and authentication
- **Config portability**: Same key means configs work across updates
- Generate cryptographically secure random nonces via `getrandom`
- Include authentication tag for integrity verification (part of GCM mode)

### 3. Create Config File Module

**File:** `src/config_file.rs`

**Purpose:** Handle loading/saving encrypted configuration.

**Config structure:**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub encrypted_passphrase: String,  // Base64 AES-encrypted
    pub auto_lock_timeout: u64,        // Seconds (default: 120)
    pub auto_unlock_timeout: u64,      // Seconds (default: 0/disabled in Release, 60 in Debug)
}

impl Config {
    // Load config from standard location
    pub fn load() -> Result<Self, Error>;

    // Save config to standard location
    pub fn save(&self) -> Result<(), Error>;

    // Get config file path
    pub fn config_path() -> PathBuf;

    // Decrypt and return plaintext passphrase
    pub fn get_passphrase(&self) -> Result<String, Error>;

    // Create new config with encrypted passphrase
    pub fn new(plaintext_passphrase: &str, auto_lock: u64, auto_unlock: u64) -> Result<Self, Error>;
}
```

**Config file location:**
- macOS: `~/Library/Application Support/handsoff/config.toml`
- Linux: `~/.config/handsoff/config.toml`
- Windows: `%APPDATA%\handsoff\config.toml`

**File permissions:**
- Set to 600 (user read/write only) when creating
- Verify permissions on load, warn if too permissive

**Example config.toml:**
```toml
encrypted_passphrase = "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY3ODkw..."
auto_lock_timeout = 120
auto_unlock_timeout = 0  # 0 = disabled (default for Release builds)
```

### 4. Update CLI Binary

**File:** `src/bin/handsoff.rs`

#### 4.1 Add `--setup` Argument

Update the `Args` struct:

```rust
#[derive(Parser, Debug)]
#[command(name = "handsoff")]
#[command(about = "Keyboard and mouse input blocker", long_about = None)]
struct Args {
    /// Start with input locked immediately
    #[arg(short, long)]
    locked: bool,

    /// Override auto-lock timeout in seconds
    #[arg(long)]
    auto_lock: Option<u64>,

    /// Run interactive setup to configure passphrase and timeouts
    #[arg(long)]
    setup: bool,
}
```

#### 4.2 Implement Setup Logic

```rust
fn run_setup() -> Result<(), Box<dyn std::error::Error>> {
    use rpassword::prompt_password;

    println!("HandsOff Setup");
    println!("==============\n");

    // Prompt for passphrase (non-echoing)
    let passphrase = prompt_password("Enter passphrase: ")?;
    if passphrase.is_empty() {
        eprintln!("Error: Passphrase cannot be empty");
        std::process::exit(1);
    }

    // Confirm passphrase
    let confirm = prompt_password("Confirm passphrase: ")?;
    if passphrase != confirm {
        eprintln!("Error: Passphrases do not match");
        std::process::exit(1);
    }

    // Prompt for timeouts
    let auto_lock = prompt_number("Auto-lock timeout in seconds (default: 120): ", 120)?;
    let auto_unlock = prompt_number("Auto-unlock timeout in seconds (default: 0/disabled): ", 0)?;

    // Create and save config
    let config = Config::new(&passphrase, auto_lock, auto_unlock)?;
    config.save()?;

    println!("\nConfiguration saved to: {}", Config::config_path().display());
    println!("Setup complete!");

    Ok(())
}
```

#### 4.3 Update Main Logic

Replace environment variable reading with config file:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Handle setup command
    if args.setup {
        return run_setup();
    }

    // Load configuration
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error: Could not load configuration: {}", e);
            eprintln!("\nRun 'handsoff --setup' to configure the application.");
            std::process::exit(1);
        }
    };

    // Decrypt passphrase
    let passphrase = config.get_passphrase()?;

    // Use config timeouts, allow CLI args to override
    let auto_lock_timeout = args.auto_lock.unwrap_or(config.auto_lock_timeout);

    // Initialize HandsOffCore with decrypted passphrase
    let mut core = HandsOffCore::new(&passphrase)?;

    // ... rest of existing logic
}
```

### 5. Update Tray App Binary

**File:** `src/bin/handsoff-tray.rs`

#### 5.1 Add `--setup` Support

Similar to CLI, add setup argument and logic:

```rust
#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    setup: bool,
}

fn main() {
    let args = Args::parse();

    if args.setup {
        // Run setup in terminal mode
        if let Err(e) = run_setup() {
            show_error_dialog(&format!("Setup failed: {}", e));
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    // ... existing tray app logic
}
```

#### 5.2 Update Configuration Loading

Replace environment variable with config file:

```rust
// Remove this:
let passphrase = match env::var("HANDS_OFF_SECRET_PHRASE") {
    Ok(p) if !p.is_empty() => p,
    _ => {
        show_alert_dialog(...);
        return;
    }
};

// Replace with this:
let config = match Config::load() {
    Ok(cfg) => cfg,
    Err(_) => {
        show_alert_dialog(
            "Configuration Not Found",
            "Please run setup first:\n\n\
             Option 1: Open Terminal and run:\n\
             ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup\n\n\
             Option 2: Run the CLI setup:\n\
             handsoff --setup"
        );
        return;
    }
};

let passphrase = match config.get_passphrase() {
    Ok(p) => p,
    Err(e) => {
        show_alert_dialog(
            "Configuration Error",
            &format!("Failed to decrypt passphrase: {}", e)
        );
        return;
    }
};
```

### 6. Simplify Installer - Automatic LaunchAgent Setup

**Key improvement:** Since the plist no longer contains user-specific secrets, we can install it automatically via the postinstall script and eliminate `setup-launch-agent.sh` entirely.

#### 6.1 Create Plist Template

**File:** `installer/resources/com.handsoff.inputlock.plist.template`

Create a template plist that will be bundled in the app resources:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.handsoff.inputlock</string>

    <key>ProgramArguments</key>
    <array>
        <string>HOME_PLACEHOLDER/Applications/HandsOff.app/Contents/MacOS/handsoff-tray</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <true/>

    <key>StandardOutPath</key>
    <string>HOME_PLACEHOLDER/Library/Logs/handsoff.log</string>

    <key>StandardErrorPath</key>
    <string>HOME_PLACEHOLDER/Library/Logs/handsoff.error.log</string>
</dict>
</plist>
```

**Note:** `HOME_PLACEHOLDER` will be substituted with actual `$HOME` during installation.

#### 6.2 Update Postinstall Script

**File:** `installer/scripts/postinstall`

Replace the current postinstall script to automatically create and load the launch agent:

```bash
#!/bin/bash
# PostInstall script for HandsOff
# Automatically sets up LaunchAgent (no user interaction needed)

set -e

APP_NAME="HandsOff"
APP_PATH="${HOME}/Applications/${APP_NAME}.app"
LAUNCH_AGENT_DIR="${HOME}/Library/LaunchAgents"
LAUNCH_AGENT_PLIST="${LAUNCH_AGENT_DIR}/com.handsoff.inputlock.plist"
BUNDLE_ID="com.handsoff.inputlock"
PLIST_TEMPLATE="${APP_PATH}/Contents/Resources/com.handsoff.inputlock.plist.template"

echo "HandsOff PostInstall Script"
echo "============================"
echo ""

# Create LaunchAgents directory if it doesn't exist
mkdir -p "${LAUNCH_AGENT_DIR}"

# Check if plist already exists
if [ -f "${LAUNCH_AGENT_PLIST}" ]; then
    echo "Launch Agent already exists. Updating..."
    # Unload existing agent
    launchctl unload "${LAUNCH_AGENT_PLIST}" 2>/dev/null || true
fi

# Copy template and substitute HOME path
if [ -f "${PLIST_TEMPLATE}" ]; then
    sed "s|HOME_PLACEHOLDER|${HOME}|g" "${PLIST_TEMPLATE}" > "${LAUNCH_AGENT_PLIST}"
    chmod 644 "${LAUNCH_AGENT_PLIST}"
    echo "✓ Launch Agent plist created"
else
    echo "⚠️  Warning: Plist template not found at ${PLIST_TEMPLATE}"
    echo "   Launch Agent not configured."
    exit 1
fi

# Load the launch agent
if launchctl load "${LAUNCH_AGENT_PLIST}" 2>/dev/null; then
    echo "✓ Launch Agent loaded successfully"
else
    echo "⚠️  Could not load Launch Agent (may need to grant permissions first)"
fi

echo ""
echo "=========================================="
echo "NEXT STEPS - Important!"
echo "=========================================="
echo ""
echo "1. Grant Accessibility Permissions:"
echo "   Go to: System Preferences > Security & Privacy"
echo "            > Privacy > Accessibility"
echo "   Click the lock to make changes"
echo "   Click '+' and add: ~/Applications/HandsOff.app"
echo "   Check the box next to HandsOff"
echo ""
echo "2. Configure Your Passphrase:"
echo "   Run this command in Terminal:"
echo ""
echo "   ~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup"
echo ""
echo "   This will prompt you for:"
echo "   - Secret passphrase (typing hidden)"
echo "   - Auto-lock timeout (default: 120s)"
echo "   - Auto-unlock timeout (default: 0s/disabled)"
echo ""
echo "3. Restart the App:"
echo "   The tray app should start automatically at login."
echo "   To start it now:"
echo ""
echo "   launchctl start com.handsoff.inputlock"
echo ""
echo "=========================================="
echo "Installation complete!"
echo "=========================================="
echo ""

exit 0
```

#### 6.3 Update build.sh to Include Template

**File:** `build.sh`

Make sure the plist template is copied into the app bundle resources:

```bash
# Copy plist template to Resources
cp installer/resources/com.handsoff.inputlock.plist.template \
   target/release/bundle/osx/HandsOff.app/Contents/Resources/
```

#### 6.4 Delete setup-launch-agent.sh

**File to delete:** `installer/scripts/setup-launch-agent.sh`

This script is no longer needed since:
- Plist creation is handled by postinstall
- Passphrase configuration is handled by `--setup` command
- No user interaction required for launch agent setup

### 7. Remove Old Environment Variable Logic

#### 7.1 Update `src/config.rs`

Remove passphrase-related environment variable code while keeping auto-lock/auto-unlock env var support as optional overrides:

```rust
// Keep these as optional overrides:
pub fn parse_auto_unlock_env() -> Option<AutoUnlockConfig> { ... }
pub fn parse_auto_lock_env() -> Option<u64> { ... }

// Remove or update passphrase env var parsing
```

#### 7.2 Update Error Messages

Update any error messages that reference `HANDS_OFF_SECRET_PHRASE` environment variable to instead reference the config file and `--setup` command.

### 8. Update Documentation

#### 8.1 README.md Updates

**Installation section:**
```markdown
## Installation

### Installing the App

1. **Download and install** the HandsOff.pkg installer
2. **Grant Accessibility permissions** when prompted (or manually via System Preferences > Security & Privacy > Privacy > Accessibility)
3. **Run the setup command** to configure your passphrase

The installer automatically sets up the LaunchAgent to start HandsOff at login.

### Initial Setup

Configure your passphrase by running the setup command:

**For Tray App users:**
```bash
~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
```

**For CLI users:**
```bash
handsoff --setup
```

The setup wizard will prompt you for:
- Secret passphrase (typing hidden for security)
- Auto-lock timeout (default: 120 seconds)
- Auto-unlock timeout (default: 0 seconds/disabled)

Configuration is stored encrypted at:
`~/Library/Application Support/handsoff/config.toml`

**Note:** The LaunchAgent is configured automatically during installation. You only need to run `--setup` once to configure your passphrase.
```

**Remove:**
- All references to `HANDS_OFF_SECRET_PHRASE` environment variable
- References to `setup-launch-agent.sh` script
- Manual plist editing instructions

**Add security section:**
```markdown
## Security

Your passphrase is stored encrypted using AES-256-GCM encryption. The encryption key is derived from a static seed compiled into the binary. While this provides protection against casual file inspection, be aware that:

- The encryption key can be extracted from the binary through reverse engineering
- This provides obfuscation rather than cryptographic security against determined attackers
- The config file has 600 permissions (readable only by your user account)
- **Config files are portable** across versions and builds (same encryption key)

For maximum security:
- Use a strong, unique passphrase
- Keep your system and user account secure
- Enable FileVault disk encryption on macOS
```

#### 8.2 DEVELOPMENT.md Updates

Add encryption documentation:

```markdown
## Passphrase Encryption

The application uses AES-256-GCM encryption to protect the passphrase stored in `config.toml`. The encryption key is derived from a static seed (`com.handsoff.inputlock.config.encryption.v1`) using SHA-256, ensuring config files remain compatible across different builds and versions.

### Key Features
- Static encryption key across all versions
- Config files portable between updates
- No need to reconfigure after upgrading

### Building

```bash
cargo build --release
```

No special build configuration required - encryption key is static.
```

### 9. Testing Strategy

#### 9.1 Unit Tests

Create `src/crypto.rs` tests:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // Test that encrypt->decrypt returns original
    }

    #[test]
    fn test_different_nonces() {
        // Test that same plaintext produces different ciphertexts
    }

    #[test]
    fn test_tampered_ciphertext() {
        // Test that tampered data fails authentication
    }

    #[test]
    fn test_static_key_consistency() {
        // Test that key derivation is deterministic
    }
}
```

Create `src/config_file.rs` tests:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_config_save_load() {
        // Test save and load roundtrip
    }

    #[test]
    fn test_config_permissions() {
        // Test that saved file has 600 permissions
    }

    #[test]
    fn test_config_portability() {
        // Test that config created in one session works in another
    }
}
```

#### 9.2 Integration Tests

Test scenarios:
1. Fresh install with `--setup`
2. CLI operation after setup
3. Tray app operation after setup
4. Invalid passphrase handling
5. Missing config file handling
6. Corrupted config file handling
7. **Config portability across "builds"** (verify same config works after rebuild)

#### 9.3 Migration Testing

Test that:
1. Old installations with plaintext plist fail gracefully
2. Error messages guide users to run `--setup`
3. Setup process completes successfully
4. Application works correctly after migration
5. **Config persists across version updates** (no reconfiguration needed)

---

## Migration Path for Existing Users

### First-Time Migration (Upgrading TO This Version)

Since this version changes how configuration is stored (from environment variables in plist to encrypted config.toml), existing users need to reconfigure **one time**.

#### Automated by Installer

The postinstall script will automatically:
1. Detect existing plist file
2. Unload old launch agent
3. Replace plist with new version (without environment variables)
4. Load new launch agent

#### User Action Required

After installing this update, users must configure their passphrase **once**:

**For Tray App Users:**
```bash
~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup
```

**For CLI Users:**
1. Remove `HANDS_OFF_SECRET_PHRASE` from shell profile (`.bashrc`, `.zshrc`, etc.)
2. Run setup:
   ```bash
   handsoff --setup
   ```

#### What Happens to Old Config

- Old plist with environment variables is replaced by new plist (automatically)
- No automatic passphrase migration (users must re-enter)
- Users should note their current passphrase before upgrading (can't be recovered)

### Future Updates (After This Version)

**No reconfiguration needed!** Once users have configured their passphrase with `--setup`:
- ✅ Config file works across all future versions
- ✅ Static encryption key ensures portability
- ✅ Simply install new .pkg and the app continues working
- ✅ No need to run `--setup` again unless changing passphrase

---

## Security Analysis

### Threat Model

**What this protects against:**
- ✅ Casual file inspection (cat, less, grep)
- ✅ Accidental exposure in backups (encrypted data)
- ✅ Process listing showing plaintext in environment variables
- ✅ Other applications reading LaunchAgent plist files

**What this does NOT protect against:**
- ❌ Attacker with binary access + reverse engineering skills
- ❌ Memory dumps while application is running (decrypted in RAM)
- ❌ Root/admin access to the system
- ❌ Keyloggers or screen capture malware

### Trade-offs

**Advantages:**
- No external dependencies (no Keychain prompts)
- Works offline, no network required
- Simple deployment (just the binary)
- Consistent across CLI and Tray App
- File permissions provide OS-level access control
- **Config files portable between versions** (seamless updates)
- **One-time setup** (no reconfiguration after updates)

**Disadvantages:**
- Key embedded in binary (extractable via reverse engineering)
- Same key across all installations (not user-specific)
- Provides obfuscation, not military-grade security

### Alternative Approaches Considered

1. **macOS Keychain (via security-framework)**
   - Pros: OS-level encryption, user-specific
   - Cons: Requires user prompts, macOS-only, more complex

2. **Per-build unique keys (BUILD_TIMESTAMP)**
   - Pros: Different key per build
   - Cons: Config breaks on update, terrible UX

3. **User-provided encryption key**
   - Pros: User controls key
   - Cons: Another secret to manage, poor UX

**Decision:** Static key derivation provides the best balance of security and usability, prioritizing user experience (seamless updates) while significantly improving security over plaintext storage.

---

## Implementation Checklist

### Core Functionality
- [ ] Add dependencies to Cargo.toml (aes-gcm, rpassword, dirs, base64, sha2, getrandom)
- [ ] Create src/crypto.rs with static key derivation and AES-256-GCM encryption/decryption
- [ ] Create src/config_file.rs with Config struct and file management
- [ ] Update src/bin/handsoff.rs with --setup argument and logic
- [ ] Update src/bin/handsoff-tray.rs with --setup argument and logic
- [ ] Remove HANDS_OFF_SECRET_PHRASE env var code from src/config.rs

### Installer Simplification
- [ ] Create installer/resources/com.handsoff.inputlock.plist.template
- [ ] Update installer/scripts/postinstall to auto-create plist from template
- [ ] Update build.sh to copy plist template to app Resources
- [ ] Delete installer/scripts/setup-launch-agent.sh (no longer needed)

### Documentation
- [ ] Update README.md with new --setup instructions
- [ ] Remove environment variable references from README.md
- [ ] Add security section to README.md emphasizing config portability
- [ ] Update DEVELOPMENT.md with encryption details (static key)
- [ ] Update any error messages referencing HANDS_OFF_SECRET_PHRASE

### Testing
- [ ] Write unit tests for crypto module (encrypt/decrypt roundtrip, nonce uniqueness, tamper detection, key consistency)
- [ ] Write unit tests for config module (save/load, permissions, portability)
- [ ] Integration testing (fresh install, CLI after setup, Tray after setup)
- [ ] Test migration path from old to new version
- [ ] Test config portability across "builds" (rebuild and verify config still works)
- [ ] Test error handling (missing config, corrupted config, wrong passphrase)

---

## Timeline Estimate

- **Phase 1:** Dependencies + Crypto module with static key (1-2 hours)
- **Phase 2:** Config file module (1-2 hours)
- **Phase 3:** CLI binary updates (--setup command) (2-3 hours)
- **Phase 4:** Tray app binary updates (--setup command) (1-2 hours)
- **Phase 5:** Installer updates (template + postinstall + build.sh) (1 hour)
- **Phase 6:** Documentation updates (README + DEVELOPMENT) (1-2 hours)
- **Phase 7:** Testing (unit + integration + migration + portability) (2-3 hours)

**Total:** ~9-15 hours

**Notes:**
- Simplified installer approach (auto-setup via postinstall) saves time
- No build script needed (static key) simplifies implementation
- Config portability means no future migration work needed

---

## Future Enhancements

1. **Optional Keychain integration:** Add `--use-keychain` flag for users who prefer macOS Keychain storage (maximum security)

2. **Config export/import:** Allow users to export encrypted config (for backup) with password protection

3. **Multiple profiles:** Support multiple passphrase profiles for different security levels

4. **Audit logging:** Log config access attempts to detect unauthorized access

5. **Passphrase rotation:** Add `--change-passphrase` command to update passphrase without reconfiguring timeouts
