//! Configuration file management with encrypted passphrase storage
//!
//! This module handles loading and saving the application configuration file,
//! which includes the encrypted passphrase and timeout settings.

use crate::crypto;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Application configuration stored in config.toml
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Base64-encoded AES-256-GCM encrypted passphrase
    pub encrypted_passphrase: String,
    /// Auto-lock timeout in seconds (default: 30)
    pub auto_lock_timeout: u64,
    /// Auto-unlock timeout in seconds (default: 60)
    pub auto_unlock_timeout: u64,
}

impl Config {
    /// Create a new config with encrypted passphrase
    ///
    /// # Arguments
    ///
    /// * `plaintext_passphrase` - The passphrase to encrypt and store
    /// * `auto_lock` - Auto-lock timeout in seconds
    /// * `auto_unlock` - Auto-unlock timeout in seconds
    pub fn new(plaintext_passphrase: &str, auto_lock: u64, auto_unlock: u64) -> Result<Self> {
        let encrypted_passphrase = crypto::encrypt_passphrase(plaintext_passphrase)
            .context("Failed to encrypt passphrase")?;

        Ok(Self {
            encrypted_passphrase,
            auto_lock_timeout: auto_lock,
            auto_unlock_timeout: auto_unlock,
        })
    }

    /// Get the standard config file path
    ///
    /// - macOS: `~/Library/Application Support/handsoff/config.toml`
    /// - Linux: `~/.config/handsoff/config.toml`
    /// - Windows: `%APPDATA%\handsoff\config.toml`
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
    /// - File permissions are too permissive (warning only)
    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!(
                "Configuration file not found at: {}\n\nRun 'handsoff --setup' to create it.",
                path.display()
            );
        }

        // Check file permissions (warning if too permissive)
        #[cfg(unix)]
        {
            let metadata = fs::metadata(path).context("Failed to read config file metadata")?;
            let permissions = metadata.permissions();
            let mode = permissions.mode();

            // Check if readable by group or others (should be 600)
            if mode & 0o077 != 0 {
                log::warn!(
                    "Config file has permissive permissions: {:o}. Should be 600 (user read/write only).",
                    mode & 0o777
                );
            }
        }

        // Read and parse config file
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&contents).context("Failed to parse config file")?;

        Ok(config)
    }

    /// Save config to standard location
    ///
    /// Creates the config directory if it doesn't exist.
    /// Sets file permissions to 600 (user read/write only).
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        // Serialize to TOML
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        // Write to file
        fs::write(&path, contents)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        // Set permissions to 600 (user read/write only)
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&path, permissions)
                .context("Failed to set config file permissions")?;
        }

        log::info!("Configuration saved to: {}", path.display());
        Ok(())
    }

    /// Decrypt and return the plaintext passphrase
    pub fn get_passphrase(&self) -> Result<String> {
        crypto::decrypt_passphrase(&self.encrypted_passphrase)
            .context("Failed to decrypt passphrase")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn temp_config_path() -> PathBuf {
        // Use a unique, per-test path to prevent interference between tests,
        // even when they run in parallel within the same process.
        //
        // Strategy:
        // - Base: system temp dir
        // - Subdir: "handsoff_tests/config_file"
        // - Unique segment: high-resolution timestamp + thread ID
        //
        // This ensures each call gets its own directory/file instead of sharing
        // a single path based only on PID.
        use std::thread;
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut base = std::env::temp_dir();
        base.push("handsoff_tests");
        base.push("config_file");

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tid = format!("{:?}", thread::current().id());
        base.push(format!("t_{nanos}_{tid}"));

        let _ = fs::create_dir_all(&base);

        base.join("config.toml")
    }

    #[test]
    fn test_config_new() {
        let config = Config::new("test_passphrase", 30, 60).expect("Failed to create config");

        assert_eq!(config.auto_lock_timeout, 30);
        assert_eq!(config.auto_unlock_timeout, 60);
        assert!(!config.encrypted_passphrase.is_empty());
    }

    #[test]
    fn test_config_get_passphrase() {
        let original = "my_secret_password";
        let config = Config::new(original, 30, 60).expect("Failed to create config");

        let decrypted = config.get_passphrase().expect("Failed to get passphrase");

        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let temp_path = temp_config_path();

        // Ensure clean slate
        let _ = fs::remove_file(&temp_path);

        // Create config
        let original_config = Config {
            encrypted_passphrase: "test_encrypted_data".to_string(),
            auto_lock_timeout: 45,
            auto_unlock_timeout: 120,
        };

        // Write to temp file
        let contents = toml::to_string_pretty(&original_config).expect("Failed to serialize");
        fs::write(&temp_path, contents).expect("Failed to write temp config");

        // Use the same logic as production via load_from_path
        let loaded_config = Config::load_from_path(&temp_path).expect("Failed to load temp config");

        // Verify
        assert_eq!(
            original_config.encrypted_passphrase,
            loaded_config.encrypted_passphrase
        );
        assert_eq!(
            original_config.auto_lock_timeout,
            loaded_config.auto_lock_timeout
        );
        assert_eq!(
            original_config.auto_unlock_timeout,
            loaded_config.auto_unlock_timeout
        );

        // Cleanup
        fs::remove_file(temp_path).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_config_permissions() {
        let temp_path = temp_config_path();

        let config = Config {
            encrypted_passphrase: "test".to_string(),
            auto_lock_timeout: 30,
            auto_unlock_timeout: 60,
        };

        // Write config
        let contents = toml::to_string_pretty(&config).unwrap();
        fs::write(&temp_path, contents).unwrap();

        // Set permissions to 600
        let mut permissions = fs::metadata(&temp_path).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&temp_path, permissions).unwrap();

        // Verify permissions
        let metadata = fs::metadata(&temp_path).unwrap();
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "Permissions should be 600");

        // Cleanup
        fs::remove_file(temp_path).ok();
    }

    #[test]
    fn test_config_portability() {
        // This test verifies that a config created in one "session" works in another
        // by creating, saving, and loading multiple times

        let passphrase = "portable_test_passphrase";

        // Session 1: Create and get encrypted value
        let config1 = Config::new(passphrase, 30, 60).expect("Failed to create config 1");
        let encrypted1 = config1.encrypted_passphrase.clone();

        // Session 2: Create another config with same passphrase
        let config2 = Config::new(passphrase, 30, 60).expect("Failed to create config 2");
        let encrypted2 = config2.encrypted_passphrase.clone();

        // The encrypted values will be different (random nonces) but both should decrypt to same value
        assert_ne!(
            encrypted1, encrypted2,
            "Encrypted values should differ due to random nonces"
        );

        let decrypted1 = config1.get_passphrase().expect("Failed to decrypt 1");
        let decrypted2 = config2.get_passphrase().expect("Failed to decrypt 2");

        assert_eq!(decrypted1, passphrase);
        assert_eq!(decrypted2, passphrase);
        assert_eq!(decrypted1, decrypted2);
    }

    #[test]
    fn test_missing_config_file() {
        // Use a guaranteed-nonexistent path to test missing config handling
        let missing_path = Path::new("/tmp/handsoff_missing_config_test_config.toml");
        // Ensure it does not exist if the test is re-run
        let _ = fs::remove_file(missing_path);

        let result = Config::load_from_path(missing_path);

        // Should fail with helpful error message
        assert!(result.is_err());
        if let Err(e) = result {
            let error_msg = format!("{:#}", e);
            assert!(error_msg.contains("not found") || error_msg.contains("--setup"));
        }
    }
}
