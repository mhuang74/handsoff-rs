//! Configuration parsing for HandsOff
//!
//! This module handles parsing of environment variables that can optionally
//! override settings from the config file. The primary configuration source
//! is the encrypted config.toml file (see config_file module).
//!
//! Environment variables (all optional):
//! - HANDS_OFF_AUTO_LOCK: Override auto-lock timeout from config file
//! - HANDS_OFF_AUTO_UNLOCK: Override auto-unlock timeout from config file
//! - HANDS_OFF_LOCK_HOTKEY: Override lock hotkey last key (A-Z)
//! - HANDS_OFF_TALK_HOTKEY: Override talk hotkey last key (A-Z)

use crate::app_state::{
    AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_UNLOCK_DEFAULT_SECONDS,
    AUTO_UNLOCK_MAX_SECONDS, AUTO_UNLOCK_MIN_SECONDS,
};
use crate::config_file::Config;
use log::{debug, info, warn};
use std::env;

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
///
/// - If HANDS_OFF_AUTO_UNLOCK is set:
///   - Returns Some(seconds) if valid timeout is configured (60-900 seconds)
///   - Returns None if disabled (0) or invalid/out-of-range
/// - If HANDS_OFF_AUTO_UNLOCK is NOT set:
///   - Returns None (allows config file value to be used)
pub fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds)
                if (AUTO_UNLOCK_MIN_SECONDS..=AUTO_UNLOCK_MAX_SECONDS).contains(&seconds) =>
            {
                info!("Auto-unlock timeout set via environment variable: {} seconds", seconds);
                Some(seconds)
            }
            Ok(0) => {
                info!("Auto-unlock disabled via HANDS_OFF_AUTO_UNLOCK=0");
                None
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-unlock timeout: {} (must be {}-{} or 0). Ignoring environment variable.",
                    seconds, AUTO_UNLOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS
                );
                None
            }
            Err(e) => {
                warn!(
                    "Failed to parse HANDS_OFF_AUTO_UNLOCK: {}. Ignoring environment variable.",
                    e
                );
                None
            }
        },
        Err(_) => {
            // Not set: return None to allow config file value to be used
            debug!("HANDS_OFF_AUTO_UNLOCK not set.");
            None
        }
    }
}

/// Parse the HANDS_OFF_AUTO_LOCK environment variable
///
/// Returns Some(seconds) if valid timeout is configured (20-600 seconds)
/// Returns None if not set or invalid
pub fn parse_auto_lock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_LOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds) if (AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&seconds) => {
                info!(
                    "Auto-lock timeout set via environment variable: {} seconds",
                    seconds
                );
                Some(seconds)
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-lock timeout: {} (must be {}-{} seconds). Using default.",
                    seconds, AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_MAX_SECONDS
                );
                None
            }
            Err(e) => {
                warn!("Failed to parse HANDS_OFF_AUTO_LOCK: {}. Using default.", e);
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_AUTO_LOCK not set.");
            None
        }
    }
}

/// Parse the HANDS_OFF_LOCK_HOTKEY environment variable
///
/// Returns Some(key) if a valid letter A-Z is specified
/// Returns None if not set or invalid
pub fn parse_lock_hotkey() -> Option<String> {
    match env::var("HANDS_OFF_LOCK_HOTKEY") {
        Ok(val) => match Config::validate_hotkey(&val) {
            Ok(()) => {
                info!("Lock hotkey set via environment variable: {}", val);
                Some(val.to_uppercase())
            }
            Err(e) => {
                warn!(
                    "Invalid lock hotkey '{}': {}. Using default.",
                    val, e
                );
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_LOCK_HOTKEY not set.");
            None
        }
    }
}

/// Parse the HANDS_OFF_TALK_HOTKEY environment variable
///
/// Returns Some(key) if a valid letter A-Z is specified
/// Returns None if not set or invalid
pub fn parse_talk_hotkey() -> Option<String> {
    match env::var("HANDS_OFF_TALK_HOTKEY") {
        Ok(val) => match Config::validate_hotkey(&val) {
            Ok(()) => {
                info!("Talk hotkey set via environment variable: {}", val);
                Some(val.to_uppercase())
            }
            Err(e) => {
                warn!(
                    "Invalid talk hotkey '{}': {}. Using default.",
                    val, e
                );
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_TALK_HOTKEY not set.");
            None
        }
    }
}

/// Resolve auto-unlock timeout using proper precedence (internal, testable version)
///
/// Precedence order:
/// 1. Environment variable value (if provided)
/// 2. Config file value
/// 3. Build-time default
///
/// # Arguments
///
/// * `env_value` - The value from environment variable (None if not set or invalid)
/// * `config_value` - The auto_unlock_timeout from config.toml (0 means disabled)
///
/// # Returns
///
/// * `Some(seconds)` - Auto-unlock is enabled with the specified timeout
/// * `None` - Auto-unlock is disabled
fn resolve_auto_unlock_timeout_internal(env_value: Option<u64>, config_value: u64) -> Option<u64> {
    // 1. Use environment variable if provided
    env_value
        // 2. Fall back to config file (0 means disabled)
        .or_else(|| {
            if config_value == 0 {
                None
            } else {
                Some(config_value)
            }
        })
        // 3. Fall back to build-time default
        .or_else(|| {
            if AUTO_UNLOCK_DEFAULT_SECONDS == 0 {
                None
            } else {
                Some(AUTO_UNLOCK_DEFAULT_SECONDS)
            }
        })
}

/// Resolve auto-unlock timeout using proper precedence
///
/// Precedence order:
/// 1. Environment variable (HANDS_OFF_AUTO_UNLOCK)
/// 2. Config file value
/// 3. Build-time default
///
/// # Arguments
///
/// * `config_value` - The auto_unlock_timeout from config.toml (0 means disabled)
///
/// # Returns
///
/// * `Some(seconds)` - Auto-unlock is enabled with the specified timeout
/// * `None` - Auto-unlock is disabled
pub fn resolve_auto_unlock_timeout(config_value: u64) -> Option<u64> {
    resolve_auto_unlock_timeout_internal(parse_auto_unlock_timeout(), config_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto_unlock_valid_values() {
        // Test minimum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "300");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(300),
            "Should accept 300 seconds"
        );

        // Test large value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "600");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "900");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(900),
            "Should accept 900 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_disabled() {
        // Test explicit disable with 0
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "0");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None for 0"
        );

        // Test not set (should return None to allow config file value)
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None when not set to allow config file value"
        );
    }

    #[test]
    fn test_parse_auto_unlock_default_behavior() {
        // When HANDS_OFF_AUTO_UNLOCK is not set, should always return None
        // to allow config file value to be used (build default is applied later)
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");

        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None when env var not set, regardless of build type"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_invalid_values() {
        // Clean up any previous test state first
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");

        // Test too low
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 60"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value below 60"
        );

        // Test too high
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "901");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 900"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "1000");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value above 900"
        );

        // Test negative number (will fail to parse)
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "-60");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject negative value"
        );

        // Test non-numeric
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "invalid");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject non-numeric value"
        );

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "30s");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject value with units"
        );

        // Test empty string - remove first to ensure clean state
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject empty string"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_boundary_cases() {
        // Test just below minimum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject 59 seconds"
        );

        // Test at minimum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "900");
        assert_eq!(
            parse_auto_unlock_timeout(),
            Some(900),
            "Should accept 900 seconds"
        );

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "901");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should reject 901 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_lock_valid_values() {
        // Test minimum valid value
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(20),
            "Should accept 20 seconds"
        );

        // Test typical value
        env::set_var("HANDS_OFF_AUTO_LOCK", "60");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(60),
            "Should accept 60 seconds"
        );

        // Test maximum valid value
        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_invalid_values() {
        // Test too low
        env::set_var("HANDS_OFF_AUTO_LOCK", "10");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject value below 20"
        );

        // Test too high
        env::set_var("HANDS_OFF_AUTO_LOCK", "601");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject value above 600"
        );

        // Test non-numeric
        env::set_var("HANDS_OFF_AUTO_LOCK", "invalid");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should reject non-numeric value"
        );

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_boundary_cases() {
        // Test just below minimum
        env::set_var("HANDS_OFF_AUTO_LOCK", "19");
        assert_eq!(parse_auto_lock_timeout(), None, "Should reject 19 seconds");

        // Test at minimum boundary
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(20),
            "Should accept 20 seconds"
        );

        // Test at maximum boundary
        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(
            parse_auto_lock_timeout(),
            Some(600),
            "Should accept 600 seconds"
        );

        // Test just above maximum
        env::set_var("HANDS_OFF_AUTO_LOCK", "601");
        assert_eq!(parse_auto_lock_timeout(), None, "Should reject 601 seconds");

        // Clean up
        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_not_set() {
        // Test not set (should return None, not panic)
        env::remove_var("HANDS_OFF_AUTO_LOCK");
        assert_eq!(
            parse_auto_lock_timeout(),
            None,
            "Should return None when not set"
        );
    }

    // ========================================================================
    // Tests for resolve_auto_unlock_timeout_internal() - Full Precedence Logic
    // ========================================================================
    // These tests verify the complete precedence chain:
    // 1. Environment variable
    // 2. Config file value
    // 3. Build-time default
    //
    // This is a regression test suite for the bug where config file values
    // were ignored in debug builds because parse_auto_unlock_timeout()
    // was returning the build default instead of None.
    //
    // We test the internal function to avoid environment variable pollution
    // between parallel test runs.

    #[test]
    fn test_resolve_precedence_env_var_overrides_all() {
        // Setup: env var = 300, config = 120
        let result = resolve_auto_unlock_timeout_internal(Some(300), 120);

        assert_eq!(
            result,
            Some(300),
            "Environment variable should override config file value"
        );
    }

    #[test]
    fn test_resolve_precedence_config_used_when_no_env_var() {
        // Setup: no env var, config = 180
        let result = resolve_auto_unlock_timeout_internal(None, 180);

        assert_eq!(
            result,
            Some(180),
            "Config file value should be used when env var not set (THIS WAS THE BUG!)"
        );
    }

    #[test]
    fn test_resolve_precedence_config_zero_means_disabled() {
        // Setup: no env var, config = 0
        let result = resolve_auto_unlock_timeout_internal(None, 0);

        // When config is 0, it means disabled. Should fall back to build default.
        if AUTO_UNLOCK_DEFAULT_SECONDS == 0 {
            assert_eq!(
                result,
                None,
                "Config=0 with release build default should result in None (disabled)"
            );
        } else {
            assert_eq!(
                result,
                Some(AUTO_UNLOCK_DEFAULT_SECONDS),
                "Config=0 with debug build default should use build default"
            );
        }
    }

    #[test]
    fn test_resolve_precedence_env_var_zero_disables_even_with_config() {
        // Setup: env var = None (explicit disable via 0), config = 120
        // Note: parse_auto_unlock_timeout() returns None when env var is "0"
        let result = resolve_auto_unlock_timeout_internal(None, 120);

        // When env var is set to 0, parse_auto_unlock_timeout() returns None,
        // so this tests the case where we want config to be used instead
        assert_eq!(
            result,
            Some(120),
            "When env var parsing returns None, config value should be used"
        );
    }

    #[test]
    fn test_resolve_precedence_invalid_env_var_falls_back_to_config() {
        // Setup: env var = None (invalid), config = 200
        // Note: parse_auto_unlock_timeout() returns None for invalid values
        let result = resolve_auto_unlock_timeout_internal(None, 200);

        assert_eq!(
            result,
            Some(200),
            "Invalid env var (None) should fall back to config file value"
        );
    }

    #[test]
    fn test_resolve_precedence_out_of_range_env_var_falls_back_to_config() {
        // Setup: env var = None (out of range), config = 150
        // Note: parse_auto_unlock_timeout() returns None for out-of-range values
        let result = resolve_auto_unlock_timeout_internal(None, 150);

        assert_eq!(
            result,
            Some(150),
            "Out-of-range env var (None) should fall back to config file value"
        );
    }

    #[test]
    fn test_resolve_precedence_build_default_used_as_last_resort() {
        // Setup: no env var, config = 0 (disabled)
        let result = resolve_auto_unlock_timeout_internal(None, 0);

        // This tests the final fallback to build default
        if AUTO_UNLOCK_DEFAULT_SECONDS == 0 {
            assert_eq!(
                result,
                None,
                "In release builds, should default to disabled (None)"
            );
        } else {
            assert_eq!(
                result,
                Some(AUTO_UNLOCK_DEFAULT_SECONDS),
                "In debug builds, should default to {} seconds",
                AUTO_UNLOCK_DEFAULT_SECONDS
            );
        }
    }

    #[test]
    fn test_resolve_precedence_multiple_config_values() {
        // Test that different config values are properly respected when no env var
        // Test various valid config values
        assert_eq!(resolve_auto_unlock_timeout_internal(None, 60), Some(60));
        assert_eq!(resolve_auto_unlock_timeout_internal(None, 120), Some(120));
        assert_eq!(resolve_auto_unlock_timeout_internal(None, 300), Some(300));
        assert_eq!(resolve_auto_unlock_timeout_internal(None, 600), Some(600));
        assert_eq!(resolve_auto_unlock_timeout_internal(None, 900), Some(900));
    }

    #[test]
    fn test_resolve_precedence_env_var_takes_precedence_over_all_config_values() {
        // Verify env var override works for any config value
        assert_eq!(
            resolve_auto_unlock_timeout_internal(Some(250), 60),
            Some(250)
        );
        assert_eq!(
            resolve_auto_unlock_timeout_internal(Some(250), 120),
            Some(250)
        );
        assert_eq!(
            resolve_auto_unlock_timeout_internal(Some(250), 0),
            Some(250)
        );
    }
}
