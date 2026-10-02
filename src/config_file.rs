//! Configuration file management with hashed passphrase storage
//!
//! This module handles loading and saving the application configuration file,
//! which includes the passphrase hash (over the physical keycode sequence) and
//! timeout settings.
//!
//! See specs/deep-design-review-v2-2026-09.md §2.7 and §3:
//! - Passphrases are stored as SHA-256 hashes over raw keycodes (`keycode-v1`).
//! - Auto-unlock is a backoff mode (`backoff` | `disabled`), not a scalar.

use crate::constants::{
    AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, CONFIG_FILE_PERMISSIONS,
    CONFIG_PERMISSION_MASK_GROUP_OTHER,
};
use crate::utils::{hash_keycodes, KEYCODE_SEQUENCE_FORMAT};
use anyhow::{anyhow, Context, Result};
use global_hotkey::hotkey::Code;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Auto-unlock modes (§2.7)
pub const AUTO_UNLOCK_MODE_BACKOFF: &str = "backoff";
pub const AUTO_UNLOCK_MODE_DISABLED: &str = "disabled";

/// Application configuration stored in config.toml
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// SHA-256 hex hash of the passphrase keycode sequence (keycode-v1 format).
    /// Optional on load so legacy configs (which have `encrypted_passphrase`
    /// instead) parse successfully and hit the explicit re-setup error below,
    /// rather than an opaque "missing field" parse error.
    #[serde(default)]
    pub passphrase_hash: Option<String>,
    /// Passphrase format tag; only "keycode-v1" is loadable. Any other value
    /// (including legacy AES-encrypted configs) forces a re-setup.
    #[serde(default = "default_passphrase_format")]
    pub passphrase_format: String,
    /// Auto-lock timeout in seconds (default: 180)
    pub auto_lock_timeout: u64,
    /// Auto-unlock mode: "backoff" (enabled-by-default exponential schedule)
    /// or "disabled" (§2.7)
    #[serde(default = "default_auto_unlock_mode")]
    pub auto_unlock_mode: String,
    /// Base interval in seconds for the exponential-backoff auto-unlock
    /// schedule (§2.7). Present only when `auto_unlock_mode` is "backoff";
    /// older configs without this field fall back to `AUTO_UNLOCK_BASE_SECONDS`.
    #[serde(default)]
    pub auto_unlock_base_interval: Option<u64>,
    /// Lock hotkey last key (A-Z, default: L)
    #[serde(default)]
    pub lock_hotkey: Option<String>,
    /// Talk hotkey last key (A-Z, default: T)
    #[serde(default)]
    pub talk_hotkey: Option<String>,
}

fn default_passphrase_format() -> String {
    KEYCODE_SEQUENCE_FORMAT.to_string()
}

fn default_auto_unlock_mode() -> String {
    AUTO_UNLOCK_MODE_BACKOFF.to_string()
}

impl Config {
    /// Create a new config from a captured keycode sequence
    ///
    /// # Arguments
    ///
    /// * `keycodes` - Physical keycodes captured during setup (validated
    ///   here: at least `MIN_PASSPHRASE_KEYS` entries)
    /// * `auto_lock` - Auto-lock timeout in seconds
    /// * `auto_unlock_backoff` - Whether the backoff auto-unlock schedule is enabled
    /// * `auto_unlock_base` - Base interval in seconds for the backoff schedule
    ///   (used only when `auto_unlock_backoff` is true; must be within
    ///   `AUTO_UNLOCK_MIN_BASE_SECONDS..=AUTO_UNLOCK_CEILING_SECONDS` — this
    ///   constructor is authoritative for the range check)
    /// * `lock_key` - Optional lock hotkey (A-Z), defaults to None (which becomes L)
    /// * `talk_key` - Optional talk hotkey (A-Z), defaults to None (which becomes T)
    pub fn new(
        keycodes: &[u32],
        auto_lock: u64,
        auto_unlock_backoff: bool,
        auto_unlock_base: u64,
        lock_key: Option<String>,
        talk_key: Option<String>,
    ) -> Result<Self> {
        // Passphrase length is the constructor's contract: a too-short
        // sequence would produce a hash that can never verify.
        if keycodes.len() < crate::utils::MIN_PASSPHRASE_KEYS {
            anyhow::bail!(
                "Passphrase must contain at least {} keys (got {})",
                crate::utils::MIN_PASSPHRASE_KEYS,
                keycodes.len()
            );
        }

        // Validate hotkeys if provided
        if let Some(key) = &lock_key {
            Self::validate_hotkey(key)?;
        }
        if let Some(key) = &talk_key {
            Self::validate_hotkey(key)?;
        }

        // Auto-lock timeout must be in range — a value of 0 re-locks within
        // one poll interval of every unlock (near-permanent self-lockout),
        // and an over-max value silently disables auto-lock.
        if !(AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&auto_lock) {
            anyhow::bail!(
                "Invalid auto_lock_timeout '{}' (must be {}-{})",
                auto_lock,
                AUTO_LOCK_MIN_SECONDS,
                AUTO_LOCK_MAX_SECONDS
            );
        }

        // Base interval must be in range when backoff is enabled — the
        // constructor is authoritative (callers must not rely on the runtime
        // clamp in resolve_auto_unlock_internal, which is a backstop only).
        if auto_unlock_backoff
            && !(crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS
                ..=crate::app_state::AUTO_UNLOCK_CEILING_SECONDS)
                .contains(&auto_unlock_base)
        {
            anyhow::bail!(
                "Invalid auto_unlock_base_interval '{}' (must be {}-{})",
                auto_unlock_base,
                crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS,
                crate::app_state::AUTO_UNLOCK_CEILING_SECONDS
            );
        }

        // Validate that lock and talk keys are different
        if let (Some(lock), Some(talk)) = (&lock_key, &talk_key) {
            if lock.to_uppercase() == talk.to_uppercase() {
                return Err(anyhow!(
                    "Lock and Talk hotkeys must be different (both set to '{}')",
                    lock
                ));
            }
        }

        Ok(Self {
            passphrase_hash: Some(hash_keycodes(keycodes)),
            passphrase_format: KEYCODE_SEQUENCE_FORMAT.to_string(),
            auto_lock_timeout: auto_lock,
            auto_unlock_mode: if auto_unlock_backoff {
                AUTO_UNLOCK_MODE_BACKOFF.to_string()
            } else {
                AUTO_UNLOCK_MODE_DISABLED.to_string()
            },
            auto_unlock_base_interval: if auto_unlock_backoff {
                Some(auto_unlock_base)
            } else {
                None
            },
            lock_hotkey: lock_key,
            talk_hotkey: talk_key,
        })
    }

    /// Get the standard config file path
    ///
    /// - macOS: `~/Library/Application Support/handsoff/config.toml`
    /// - Linux: `~/.config/handsoff/config.toml`
    /// - Windows: `%APPDATA%\handsoff/config.toml`
    pub fn config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .expect("Failed to determine config directory")
            .join("handsoff");

        config_dir.join("config.toml")
    }

    /// Load config from standard location
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Config file doesn't exist
    /// - Failed to read file
    /// - TOML parsing fails
    /// - File permissions are too permissive (warning only)
    /// - Passphrase format is not `keycode-v1` (legacy configs must re-setup)
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        Self::load_from_path(&path)
    }

    /// Load config from a specific path
    ///
    /// This is primarily intended for testing and advanced scenarios.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Config file doesn't exist
    /// - Failed to read file
    /// - TOML parsing fails
    /// - The file is group/other-readable AND chmod 0600 fails (a usable
    ///   config with a wrong mode is repaired in place, issue #37 N7 — a
    ///   repair failure is a hard error, NOT a Setup-Wizard case)
    /// - Passphrase format is not `keycode-v1` (legacy configs must re-setup)
    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!(
                "Configuration file not found at: {}\n\nRun 'handsoff --setup' to create it.",
                path.display()
            );
        }

        // Config-file permission enforcement (issue #37 N7): the file holds
        // the passphrase hash — a group/other-readable config is a USABLE
        // config with a wrong mode, so load repairs it in place (chmod 0600)
        // and continues; only a FAILED repair is fatal. Never route this to
        // the Setup Wizard: re-setup would discard a working Passphrase.
        #[cfg(unix)]
        {
            let metadata = fs::metadata(path).context("Failed to read config file metadata")?;
            let permissions = metadata.permissions();
            let mode = permissions.mode();

            // Check if readable by group or others
            if mode & CONFIG_PERMISSION_MASK_GROUP_OTHER != 0 {
                log::warn!(
                    "Config file has permissive permissions: {:o}. Repairing to {:o} (user read/write only).",
                    mode & 0o777,
                    CONFIG_FILE_PERMISSIONS
                );
                let mut repaired = permissions;
                repaired.set_mode(CONFIG_FILE_PERMISSIONS);
                fs::set_permissions(path, repaired).with_context(|| {
                    format!(
                        "Config file is group/other-readable and could not be repaired.\n\
                         Fix it manually with: chmod 600 {}",
                        path.display()
                    )
                })?;
                log::info!(
                    "Config file permissions repaired to {:o}",
                    CONFIG_FILE_PERMISSIONS
                );
            }
        }

        // Read and parse config file
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let mut config: Config =
            toml::from_str(&contents).context("Failed to parse config file")?;

        // Validate loaded config
        // 1. Passphrase format must be keycode-v1 (legacy formats are rejected:
        //    never silently keep an unverifiable passphrase — spec §3/§6)
        if config.passphrase_format != KEYCODE_SEQUENCE_FORMAT {
            anyhow::bail!(
                "Unsupported passphrase format '{}' (expected '{}').\n\
                 A one-time re-setup is required: run 'handsoff --setup'.\n\
                 (Existing passphrases cannot be migrated — they were stored in an \
                 unverifiable format.)",
                config.passphrase_format,
                KEYCODE_SEQUENCE_FORMAT
            );
        }

        // 2. Hash must be present and look like a SHA-256 hex digest.
        // A missing hash (legacy config with encrypted_passphrase, or a
        // corrupted file) also forces re-setup with clear guidance.
        let hash_ok = config
            .passphrase_hash
            .as_ref()
            .is_some_and(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
        if !hash_ok {
            anyhow::bail!(
                "No valid passphrase hash found in config file (it may be from an \
                 older version of HandsOff).\n\
                 A one-time re-setup is required: run 'handsoff --setup'."
            );
        }

        // Normalize hex case in place: `verify_keycodes` compares against
        // lowercase `hex::encode` output byte-for-byte, but hex case is
        // semantically meaningless — a hand-edited-but-correct uppercase hash
        // must still work, not silently mismatch forever.
        if let Some(hash) = &mut config.passphrase_hash {
            hash.make_ascii_lowercase();
        }

        // The empty-sequence digest is never a valid passphrase: no capture
        // can produce zero keys (minimum enforced), so this hash can never
        // verify and always indicates a corrupted/hand-edited config.
        if config.passphrase_hash.as_deref() == Some(crate::utils::hash_keycodes(&[]).as_str()) {
            anyhow::bail!(
                "Passphrase hash is the SHA-256 of an empty sequence — the config \
                 cannot contain a usable passphrase.\n\
                 A one-time re-setup is required: run 'handsoff --setup'."
            );
        }

        // 3. Auto-unlock mode must be known
        if config.auto_unlock_mode != AUTO_UNLOCK_MODE_BACKOFF
            && config.auto_unlock_mode != AUTO_UNLOCK_MODE_DISABLED
        {
            anyhow::bail!(
                "Invalid auto_unlock_mode '{}' (expected '{}' or '{}'). \
                 Run 'handsoff --setup' to reconfigure.",
                config.auto_unlock_mode,
                AUTO_UNLOCK_MODE_BACKOFF,
                AUTO_UNLOCK_MODE_DISABLED
            );
        }

        // 3.5. Base interval must be in range when backoff is enabled
        // (Finding 1: a hand-edited typo must fail loudly at load, not be
        // silently clamped by the runtime resolver).
        if config.auto_unlock_mode == AUTO_UNLOCK_MODE_BACKOFF {
            if let Some(v) = config.auto_unlock_base_interval {
                if !(crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS
                    ..=crate::app_state::AUTO_UNLOCK_CEILING_SECONDS)
                    .contains(&v)
                {
                    anyhow::bail!(
                        "Invalid auto_unlock_base_interval '{}' (must be {}-{}). \
                         Run 'handsoff --setup' to reconfigure.",
                        v,
                        crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS,
                        crate::app_state::AUTO_UNLOCK_CEILING_SECONDS
                    );
                }
            }
        }

        // 4. Validate hotkey format if provided
        if let Some(key) = &config.lock_hotkey {
            Config::validate_hotkey(key)
                .with_context(|| format!("Invalid lock_hotkey in config file: '{}'", key))?;
        }
        if let Some(key) = &config.talk_hotkey {
            Config::validate_hotkey(key)
                .with_context(|| format!("Invalid talk_hotkey in config file: '{}'", key))?;
        }

        // 5. Validate that lock and talk keys are different
        if let (Some(lock), Some(talk)) = (&config.lock_hotkey, &config.talk_hotkey) {
            if lock.to_uppercase() == talk.to_uppercase() {
                anyhow::bail!(
                    "Invalid config: Lock and Talk hotkeys must be different (both set to '{}'). Please run 'handsoff --setup' to reconfigure.",
                    lock
                );
            }
        }

        // 6. Auto-lock timeout must be in range (Finding: a hand-edited typo
        // of 0 re-locks within one poll interval of every unlock; an
        // over-max value silently disables auto-lock).
        if !(AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&config.auto_lock_timeout) {
            anyhow::bail!(
                "Invalid auto_lock_timeout '{}' (must be {}-{}). \
                 Run 'handsoff --setup' to reconfigure.",
                config.auto_lock_timeout,
                AUTO_LOCK_MIN_SECONDS,
                AUTO_LOCK_MAX_SECONDS
            );
        }

        Ok(config)
    }

    /// Save config to standard location
    ///
    /// Creates the config directory if it doesn't exist. The file is created
    /// with 0600 permissions atomically (issue #37 N7: `fs::write` first would
    /// leave a world-readable window before the chmod) and `set_permissions`
    /// afterwards only repairs pre-existing files with looser modes.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        // Serialize to TOML
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        // Write the file: created 0600 from the first byte, no world-readable
        // window (issue #37 N7). `set_permissions` after the write repairs the
        // mode of a pre-existing file that was looser.
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;

            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(CONFIG_FILE_PERMISSIONS)
                .open(&path)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
            file.sync_all()
                .with_context(|| format!("Failed to flush config file: {}", path.display()))?;

            let mut permissions = fs::metadata(&path)?.permissions();
            if permissions.mode() & 0o777 != CONFIG_FILE_PERMISSIONS {
                permissions.set_mode(CONFIG_FILE_PERMISSIONS);
                fs::set_permissions(&path, permissions)
                    .context("Failed to set config file permissions")?;
            }
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, contents)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        log::info!("Configuration saved to: {}", path.display());
        Ok(())
    }

    /// Save config to a specific path (no directory creation; the file is
    /// still created 0600 on unix — a config file must never exist
    /// world-readable, issue #37 N7 — and a pre-existing looser mode is not
    /// repaired here; the standard-location `save()` is the hardened variant).
    /// Used by tests and any caller with its own path policy.
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;

            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(CONFIG_FILE_PERMISSIONS)
                .open(path)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
            file.write_all(contents.as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }
        #[cfg(not(unix))]
        {
            fs::write(path, contents)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }
        Ok(())
    }

    /// Whether the backoff auto-unlock schedule is enabled
    pub fn auto_unlock_backoff_enabled(&self) -> bool {
        self.auto_unlock_mode == AUTO_UNLOCK_MODE_BACKOFF
    }

    /// Re-attach a stored passphrase hash/format from `previous` onto `self`.
    ///
    /// Preferences edits never touch the passphrase, but the `Config` constructor
    /// demands a ≥4-key sequence and re-hashes it. Callers pass a placeholder
    /// sequence to the constructor, then swap in the real stored hash here so
    /// the saved file keeps the working passphrase. Requires both configs to be
    /// keycode-v1 (the constructor guarantees it for `self`; `load` for
    /// `previous`).
    pub fn preserve_hash(mut self, previous: &Config) -> Result<Self> {
        if previous.passphrase_format != KEYCODE_SEQUENCE_FORMAT {
            anyhow::bail!(
                "Cannot preserve passphrase from unsupported format '{}'",
                previous.passphrase_format
            );
        }
        let hash_ok = previous
            .passphrase_hash
            .as_ref()
            .is_some_and(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
        if !hash_ok {
            anyhow::bail!("Cannot preserve passphrase: stored hash is missing or malformed");
        }
        self.passphrase_hash = previous.passphrase_hash.clone();
        self.passphrase_format = previous.passphrase_format.clone();
        Ok(self)
    }

    /// Normalize a user-typed hotkey: trim, uppercase, validate A-Z.
    pub fn normalize_hotkey(key: &str) -> String {
        let trimmed = key.trim().to_uppercase();
        // Length/format validation is the constructor's job; trim+uppercase
        // only normalizes what the user typed so the constructor sees "Q".
        trimmed
    }

    /// Get the lock hotkey Code, defaulting to KeyL if not configured
    pub fn get_lock_key_code(&self) -> Result<Code> {
        self.lock_hotkey
            .as_ref()
            .map(|s| Self::parse_key_string(s))
            .unwrap_or(Ok(Code::KeyL))
    }

    /// Get the talk hotkey Code, defaulting to KeyT if not configured
    pub fn get_talk_key_code(&self) -> Result<Code> {
        self.talk_hotkey
            .as_ref()
            .map(|s| Self::parse_key_string(s))
            .unwrap_or(Ok(Code::KeyT))
    }

    /// Validate that a hotkey string is a single letter A-Z (case insensitive)
    pub fn validate_hotkey(key: &str) -> Result<()> {
        let key_upper = key.to_uppercase();
        if key_upper.len() != 1 {
            return Err(anyhow!("Hotkey must be a single character"));
        }
        let ch = key_upper.chars().next().unwrap();
        if !ch.is_ascii_alphabetic() {
            return Err(anyhow!("Hotkey must be a letter A-Z"));
        }
        Ok(())
    }

    /// Parse a hotkey string (A-Z) to a Code enum value
    pub fn parse_key_string(key: &str) -> Result<Code> {
        Self::validate_hotkey(key)?;

        let key_upper = key.to_uppercase();
        let ch = key_upper.chars().next().unwrap();

        match ch {
            'A' => Ok(Code::KeyA),
            'B' => Ok(Code::KeyB),
            'C' => Ok(Code::KeyC),
            'D' => Ok(Code::KeyD),
            'E' => Ok(Code::KeyE),
            'F' => Ok(Code::KeyF),
            'G' => Ok(Code::KeyG),
            'H' => Ok(Code::KeyH),
            'I' => Ok(Code::KeyI),
            'J' => Ok(Code::KeyJ),
            'K' => Ok(Code::KeyK),
            'L' => Ok(Code::KeyL),
            'M' => Ok(Code::KeyM),
            'N' => Ok(Code::KeyN),
            'O' => Ok(Code::KeyO),
            'P' => Ok(Code::KeyP),
            'Q' => Ok(Code::KeyQ),
            'R' => Ok(Code::KeyR),
            'S' => Ok(Code::KeyS),
            'T' => Ok(Code::KeyT),
            'U' => Ok(Code::KeyU),
            'V' => Ok(Code::KeyV),
            'W' => Ok(Code::KeyW),
            'X' => Ok(Code::KeyX),
            'Y' => Ok(Code::KeyY),
            'Z' => Ok(Code::KeyZ),
            _ => Err(anyhow!("Invalid hotkey: {}", ch)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn temp_config_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "handsoff-test-config-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_config_new() {
        let keycodes = vec![0u32, 12, 15, 37];
        let config = Config::new(
            &keycodes,
            120,
            true,
            3600,
            Some("L".to_string()),
            Some("T".to_string()),
        )
        .expect("Failed to create config");

        assert_eq!(config.passphrase_format, "keycode-v1");
        assert_eq!(config.auto_lock_timeout, 120);
        assert_eq!(config.auto_unlock_mode, AUTO_UNLOCK_MODE_BACKOFF);
        assert!(config.auto_unlock_backoff_enabled());
        assert_eq!(config.passphrase_hash, Some(hash_keycodes(&keycodes)));
    }

    #[test]
    fn test_config_keycode_hash_is_layout_independent() {
        // The stored hash must be over the raw keycodes: the same physical
        // sequence produces the same hash regardless of layout.
        let keycodes = vec![12u32, 15, 0, 37];
        let a = Config::new(&keycodes, 120, true, 3600, None, None).unwrap();
        let b = Config::new(&keycodes, 120, true, 3600, None, None).unwrap();
        assert_eq!(a.passphrase_hash, b.passphrase_hash);

        // Different sequence -> different hash
        let c = Config::new(&[12, 15, 0, 36], 120, true, 3600, None, None).unwrap();
        assert_ne!(a.passphrase_hash, c.passphrase_hash);
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let path = temp_config_path();
        let config = Config::new(&[0, 12, 15, 37], 120, true, 3600, None, None)
            .expect("Failed to create config");

        config.save_to_path(&path).expect("Failed to save");
        let loaded = Config::load_from_path(&path).expect("Failed to load");

        assert_eq!(loaded.passphrase_hash, config.passphrase_hash);
        assert_eq!(loaded.passphrase_format, "keycode-v1");
        assert_eq!(loaded.auto_unlock_mode, AUTO_UNLOCK_MODE_BACKOFF);
        assert_eq!(loaded.auto_unlock_base_interval, Some(3600));
        assert_eq!(loaded.auto_lock_timeout, 120);

        // A custom setup-persisted base survives the round-trip (§2.7)
        let custom = Config::new(&[0, 12, 15, 37], 120, true, 300, None, None)
            .expect("Failed to create config");
        custom.save_to_path(&path).expect("Failed to save");
        let loaded_custom = Config::load_from_path(&path).expect("Failed to load");
        assert_eq!(loaded_custom.auto_unlock_base_interval, Some(300));

        // Disabled mode stores no base interval
        let disabled = Config::new(&[0, 12, 15, 37], 120, false, 300, None, None)
            .expect("Failed to create config");
        assert_eq!(disabled.auto_unlock_base_interval, None);

        let _ = fs::remove_file(&path);
    }

    #[test]
    #[cfg(unix)]
    fn test_config_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_config_path();
        let config = Config::new(&[0, 12, 15, 37], 120, false, 3600, None, None)
            .expect("Failed to create config");

        // save_to_path does not set permissions; save() does. Simulate by
        // checking the constant and the real save path via config_path().
        // For an isolated test, write manually and verify the mode mask logic.
        config.save_to_path(&path).expect("Failed to save");
        let metadata = fs::metadata(&path).expect("Failed to stat");
        let _mode = metadata.permissions().mode();
        // The permission constant must remain user-only.
        assert_eq!(CONFIG_FILE_PERMISSIONS, 0o600);
        assert_eq!(CONFIG_PERMISSION_MASK_GROUP_OTHER, 0o077);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_missing_config_file() {
        let result = Config::load_from_path(Path::new("/nonexistent/handsoff/config.toml"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("--setup"),
            "Error should direct to setup: {}",
            err
        );
    }

    #[test]
    fn test_load_rejects_out_of_range_base_interval() {
        // Finding 1: a hand-edited typo must fail loudly at config load with
        // re-setup guidance, not be silently clamped by the runtime.
        let path = temp_config_path();
        let bad = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
auto_unlock_base_interval = 36
"#;
        fs::write(&path, bad).expect("Failed to write config");

        let result = Config::load_from_path(&path);
        assert!(
            result.is_err(),
            "Out-of-range base interval must be rejected"
        );
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("auto_unlock_base_interval") && err.contains("--setup"),
            "Error must name the field and direct to setup: {}",
            err
        );

        // Above the ceiling is equally rejected.
        let high = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
auto_unlock_base_interval = 999999
"#;
        fs::write(&path, high).expect("Failed to write config");
        assert!(
            Config::load_from_path(&path).is_err(),
            "Base interval above the ceiling must be rejected"
        );

        // In-range values still load fine (boundary: minimum itself).
        let ok = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
auto_unlock_base_interval = 60
"#;
        fs::write(&path, ok).expect("Failed to write config");
        let loaded = Config::load_from_path(&path).expect("In-range base must load");
        assert_eq!(loaded.auto_unlock_base_interval, Some(60));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_config_new_rejects_out_of_range_base_interval() {
        // Config::new is authoritative for the range (Finding 1): setup flows
        // already validate interactively, but direct callers must not be able
        // to persist a bad value.
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 0, None, None).is_err());
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 59, None, None).is_err());
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 999_999, None, None).is_err());
        // Disabled mode stores no base interval — value irrelevant, no error.
        assert!(Config::new(&[0, 12, 15, 37], 120, false, 0, None, None).is_ok());
        // In-range boundaries are accepted.
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 60, None, None).is_ok());
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 86_400, None, None).is_ok());
    }

    #[test]
    fn test_config_new_rejects_short_keycodes() {
        // Config::new validates passphrase length (Finding 3): fewer than
        // MIN_PASSPHRASE_KEYS keys would produce a hash that can never
        // verify, so it must be rejected at construction.
        assert!(Config::new(&[0, 12, 15], 120, true, 3600, None, None).is_err());
        assert!(Config::new(&[], 120, true, 3600, None, None).is_err());
        // Minimum length is accepted.
        assert!(Config::new(&[0, 12, 15, 37], 120, true, 3600, None, None).is_ok());
    }

    #[test]
    fn test_load_normalizes_uppercase_hash() {
        // Hex case is meaningless: an uppercase hand-edited hash must load
        // and be normalized to the lowercase form verify_keycodes expects.
        let path = temp_config_path();
        let uppercase = crate::utils::hash_keycodes(&[0, 12, 15, 37]).to_uppercase();
        let toml_src = format!(
            r#"
passphrase_hash = "{}"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
"#,
            uppercase
        );
        fs::write(&path, toml_src).expect("Failed to write config");

        let loaded = Config::load_from_path(&path).expect("Uppercase hash must load");
        assert_eq!(
            loaded.passphrase_hash,
            Some(crate::utils::hash_keycodes(&[0, 12, 15, 37])),
            "Hash must be normalized to lowercase"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_load_rejects_empty_sequence_hash() {
        // The empty-sequence digest can never verify (no capture can produce
        // zero keys), so load must reject it with re-setup guidance.
        let path = temp_config_path();
        let toml_src = format!(
            r#"
passphrase_hash = "{}"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
"#,
            crate::utils::hash_keycodes(&[])
        );
        fs::write(&path, toml_src).expect("Failed to write config");

        let result = Config::load_from_path(&path);
        assert!(result.is_err(), "Empty-sequence hash must be rejected");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("empty sequence") && err.contains("--setup"),
            "Error must explain the problem and direct to setup: {}",
            err
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_legacy_format_rejected() {
        // Legacy AES-encrypted config must be rejected with re-setup guidance
        let path = temp_config_path();
        let legacy = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "legacy-encrypted-v0"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
"#;
        fs::write(&path, legacy).expect("Failed to write legacy config");

        let result = Config::load_from_path(&path);
        assert!(result.is_err(), "Legacy format must be rejected");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("re-setup"),
            "Legacy config error should demand re-setup: {}",
            err
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_legacy_encrypted_passphrase_rejected() {
        // A real legacy config has encrypted_passphrase and NO passphrase_hash.
        // This must parse and hit the explicit re-setup error — not a "missing
        // field" parse error (spec §6: fail loudly into setup).
        let path = temp_config_path();
        let legacy = r#"
encrypted_passphrase = "c29tZWJhc2U2NGRhdGE="
auto_lock_timeout = 120
auto_unlock_timeout = 0
"#;
        fs::write(&path, legacy).expect("Failed to write legacy config");

        let result = Config::load_from_path(&path);
        assert!(result.is_err(), "Legacy config must be rejected");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("re-setup"),
            "Legacy config error should demand re-setup: {}",
            err
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_invalid_hash_rejected() {
        let path = temp_config_path();
        let bad = r#"
passphrase_hash = "not-a-hash"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
"#;
        fs::write(&path, bad).expect("Failed to write config");

        assert!(Config::load_from_path(&path).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_invalid_auto_unlock_mode_rejected() {
        let path = temp_config_path();
        let bad = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
auto_lock_timeout = 120
auto_unlock_mode = "3600"
"#;
        fs::write(&path, bad).expect("Failed to write config");

        assert!(Config::load_from_path(&path).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_missing_optional_fields_default() {
        // passphrase_format and auto_unlock_mode have serde defaults; a config
        // written without them loads as keycode-v1 + backoff.
        let path = temp_config_path();
        let minimal = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
auto_lock_timeout = 120
"#;
        fs::write(&path, minimal).expect("Failed to write config");

        let loaded = Config::load_from_path(&path).expect("Should load with defaults");
        assert_eq!(loaded.passphrase_format, "keycode-v1");
        assert_eq!(loaded.auto_unlock_mode, AUTO_UNLOCK_MODE_BACKOFF);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_duplicate_hotkeys_in_new() {
        let result = Config::new(
            &[0, 12, 15, 37],
            120,
            true,
            3600,
            Some("L".to_string()),
            Some("L".to_string()),
        );
        assert!(result.is_err(), "Duplicate hotkeys must be rejected");
    }

    #[test]
    fn test_duplicate_hotkeys_case_insensitive() {
        let result = Config::new(
            &[0, 12, 15, 37],
            120,
            true,
            3600,
            Some("l".to_string()),
            Some("L".to_string()),
        );
        assert!(
            result.is_err(),
            "Case-insensitive duplicates must be rejected"
        );
    }

    #[test]
    fn test_different_hotkeys_accepted() {
        let result = Config::new(
            &[0, 12, 15, 37],
            120,
            true,
            3600,
            Some("L".to_string()),
            Some("T".to_string()),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_hotkey_in_loaded_config() {
        let path = temp_config_path();
        let bad = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
lock_hotkey = "1"
talk_hotkey = "T"
"#;
        fs::write(&path, bad).expect("Failed to write config");

        assert!(Config::load_from_path(&path).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_duplicate_hotkeys_in_loaded_config() {
        let path = temp_config_path();
        let bad = r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
lock_hotkey = "L"
talk_hotkey = "l"
"#;
        fs::write(&path, bad).expect("Failed to write config");

        assert!(Config::load_from_path(&path).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_auto_lock_timeout_range_in_new() {
        // Out of range (0 = near-permanent self-lockout; below min; above max)
        for bad in [0u64, 19, 601] {
            let result = Config::new(&[0, 12, 15, 37], bad, true, 3600, None, None);
            assert!(
                result.is_err(),
                "auto_lock_timeout={} must be rejected",
                bad
            );
        }
        // In range (inclusive bounds)
        for good in [20u64, 600] {
            let result = Config::new(&[0, 12, 15, 37], good, true, 3600, None, None);
            assert!(
                result.is_ok(),
                "auto_lock_timeout={} must be accepted",
                good
            );
        }
    }

    #[test]
    fn test_invalid_auto_lock_in_loaded_config() {
        for bad in [0u64, 19, 601, 100_000] {
            let path = temp_config_path();
            let bad_toml = format!(
                r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
auto_lock_timeout = {}
auto_unlock_mode = "backoff"
"#,
                bad
            );
            fs::write(&path, &bad_toml).expect("Failed to write config");

            let result = Config::load_from_path(&path);
            assert!(
                result.is_err(),
                "auto_lock_timeout={} must fail loudly at load",
                bad
            );

            let _ = fs::remove_file(&path);
        }
    }
}
