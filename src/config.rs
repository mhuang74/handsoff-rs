//! Configuration parsing for HandsOff
//!
//! This module handles parsing of environment variables that can optionally
//! override settings from the config file. The primary configuration source
//! is the encrypted config.toml file (see config_file module).
//!
//! Environment variables (all optional):
//! - HANDS_OFF_AUTO_LOCK: Override auto-lock timeout from config file
//! - HANDS_OFF_AUTO_UNLOCK: Override auto-unlock timeout from config file

use crate::app_state::{
    AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS, AUTO_UNLOCK_MIN_SECONDS,
};
use log::{debug, info, warn};
use std::env;

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable
///
/// Returns Some(seconds) if valid timeout is configured (60-900 seconds)
/// Returns None if disabled (0) or not set
pub fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(seconds)
                if (AUTO_UNLOCK_MIN_SECONDS..=AUTO_UNLOCK_MAX_SECONDS).contains(&seconds) =>
            {
                info!("Auto-unlock safety feature enabled: {} seconds", seconds);
                Some(seconds)
            }
            Ok(0) => {
                info!("Auto-unlock disabled (value: 0)");
                None
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-unlock timeout: {} (must be {}-{} or 0). Feature disabled.",
                    seconds, AUTO_UNLOCK_MIN_SECONDS, AUTO_UNLOCK_MAX_SECONDS
                );
                None
            }
            Err(e) => {
                warn!(
                    "Failed to parse HANDS_OFF_AUTO_UNLOCK: {}. Feature disabled.",
                    e
                );
                None
            }
        },
        Err(_) => {
            debug!("HANDS_OFF_AUTO_UNLOCK not set. Auto-unlock disabled.");
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

        // Test not set (should return None, not panic)
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None when not set"
        );
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
}
