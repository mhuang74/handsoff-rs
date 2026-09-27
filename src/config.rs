//! Configuration parsing for HandsOff
//!
//! This module handles parsing of environment variables that can optionally
//! override settings from the config file. The primary configuration source
//! is the config.toml file (see config_file module).
//!
//! Environment variables (all optional):
//! - HANDS_OFF_AUTO_LOCK: Override auto-lock timeout from config file
//! - HANDS_OFF_AUTO_UNLOCK: Override the auto-unlock backoff base interval
//!   (60-86400 seconds; 0 disables auto-unlock entirely)
//! - HANDS_OFF_LOCK_HOTKEY: Override lock hotkey last key (A-Z)
//! - HANDS_OFF_TALK_HOTKEY: Override talk hotkey last key (A-Z)

use crate::app_state::{
    AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_UNLOCK_BASE_SECONDS,
    AUTO_UNLOCK_CEILING_SECONDS,
};
use crate::config_file::Config;
use log::{debug, info, warn};
use std::env;

/// Minimum allowed base interval for the auto-unlock backoff schedule.
/// Prevents an accidental instant unlock (windows open no sooner than this).
pub const AUTO_UNLOCK_MIN_BASE_SECONDS: u64 = 60;

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable.
///
/// The value overrides the auto-unlock *backoff base interval* (§2.7):
/// - `0` disables auto-unlock entirely
/// - `60..=86400` sets the base interval (windows then double up to the
///   86400 s ceiling)
/// - unset or invalid returns None (config file value is used)
pub fn parse_auto_unlock_timeout() -> Option<u64> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(0) => {
                info!("Auto-unlock disabled via HANDS_OFF_AUTO_UNLOCK=0");
                Some(0)
            }
            Ok(seconds)
                if (AUTO_UNLOCK_MIN_BASE_SECONDS..=AUTO_UNLOCK_CEILING_SECONDS)
                    .contains(&seconds) =>
            {
                info!(
                    "Auto-unlock base interval set via environment variable: {} seconds",
                    seconds
                );
                Some(seconds)
            }
            Ok(seconds) => {
                warn!(
                    "Invalid auto-unlock base interval: {} (must be {}-{} or 0). Ignoring environment variable.",
                    seconds, AUTO_UNLOCK_MIN_BASE_SECONDS, AUTO_UNLOCK_CEILING_SECONDS
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
            Err(err) => {
                warn!(
                    "Invalid lock hotkey '{}': {}. Using default.",
                    val, err
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
            Err(err) => {
                warn!(
                    "Invalid talk hotkey '{}': {}. Using default.",
                    val, err
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

/// Resolve the auto-unlock backoff configuration (internal, testable version).
///
/// Precedence order:
/// 1. Environment variable (Some(0) = force disabled; Some(n) = base interval n)
/// 2. Config file: `backoff` mode → persisted base interval (falls back to
///    `AUTO_UNLOCK_BASE_SECONDS` for configs written before §2.7 stored it);
///    `disabled` → off (honored, §2.7 schema)
/// 3. Build-time default: **enabled** with `AUTO_UNLOCK_BASE_SECONDS` (V2) —
///    applies only when no config exists and no env var is set
///
/// Returns the effective base interval in seconds, or `None` when auto-unlock
/// is disabled.
fn resolve_auto_unlock_internal(
    env_value: Option<u64>,
    config_backoff_enabled: Option<bool>,
    config_base_interval: Option<u64>,
) -> Option<u64> {
    // 1. Environment variable wins: 0 explicitly disables, n overrides base.
    if let Some(env_secs) = env_value {
        return if env_secs == 0 { None } else { Some(env_secs) };
    }
    // 2. Config file mode (None = no config file at all → build default)
    match config_backoff_enabled {
        Some(true) => Some(config_base_interval.unwrap_or(AUTO_UNLOCK_BASE_SECONDS)),
        Some(false) => None,
        // 3. Build-time default: enabled-by-default (V2) for a fresh install
        // with no config file.
        None => Some(AUTO_UNLOCK_BASE_SECONDS),
    }
}

/// Resolve the auto-unlock backoff configuration.
///
/// Precedence: env var > config file (mode + persisted base interval) >
/// build-time default (enabled). `config_backoff_enabled`: `Some(mode)` from a
/// loaded config, `None` when no config exists. `config_base_interval`: the
/// persisted §2.7 base interval, if any. Returns the effective base interval
/// in seconds, or `None` when disabled.
pub fn resolve_auto_unlock(
    config_backoff_enabled: Option<bool>,
    config_base_interval: Option<u64>,
) -> Option<u64> {
    resolve_auto_unlock_internal(parse_auto_unlock_timeout(), config_backoff_enabled, config_base_interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto_unlock_zero_disables() {
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "0");
        assert_eq!(parse_auto_unlock_timeout(), Some(0), "0 means explicit disable");
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_valid_base_intervals() {
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(parse_auto_unlock_timeout(), Some(60));

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(parse_auto_unlock_timeout(), Some(3600));

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "86400");
        assert_eq!(parse_auto_unlock_timeout(), Some(86400));

        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_invalid_values() {
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");

        // Below minimum base
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject below 60");

        // Above ceiling
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "86401");
        assert_eq!(parse_auto_unlock_timeout(), None, "Should reject above 86400");

        // Negative / non-numeric / units / empty
        for bad in ["-60", "invalid", "30s", ""] {
            env::set_var("HANDS_OFF_AUTO_UNLOCK", bad);
            assert_eq!(parse_auto_unlock_timeout(), None, "Should reject {:?}", bad);
        }

        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_not_set() {
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(
            parse_auto_unlock_timeout(),
            None,
            "Should return None when not set, allowing config value"
        );
    }

    #[test]
    fn test_parse_auto_lock_valid_values() {
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(parse_auto_lock_timeout(), Some(20));

        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(parse_auto_lock_timeout(), Some(600));

        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_invalid_values() {
        env::remove_var("HANDS_OFF_AUTO_LOCK");

        env::set_var("HANDS_OFF_AUTO_LOCK", "10");
        assert_eq!(parse_auto_lock_timeout(), None);

        env::set_var("HANDS_OFF_AUTO_LOCK", "601");
        assert_eq!(parse_auto_lock_timeout(), None);

        env::set_var("HANDS_OFF_AUTO_LOCK", "invalid");
        assert_eq!(parse_auto_lock_timeout(), None);

        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_resolve_env_var_overrides_config() {
        // Env base interval overrides a disabled config
        assert_eq!(resolve_auto_unlock_internal(Some(300), Some(false), Some(7200)), Some(300));
        // Env base interval overrides an enabled config
        assert_eq!(resolve_auto_unlock_internal(Some(300), Some(true), Some(7200)), Some(300));
    }

    #[test]
    fn test_resolve_env_zero_disables_even_when_config_enabled() {
        assert_eq!(resolve_auto_unlock_internal(Some(0), Some(true), Some(7200)), None);
        assert_eq!(resolve_auto_unlock_internal(Some(0), Some(false), None), None);
    }

    #[test]
    fn test_resolve_config_backoff_enabled() {
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), None),
            Some(AUTO_UNLOCK_BASE_SECONDS)
        );
    }

    #[test]
    fn test_resolve_config_disabled_is_honored() {
        // §2.7: an explicit config `disabled` mode must stay disabled
        assert_eq!(resolve_auto_unlock_internal(None, Some(false), Some(7200)), None);
    }

    #[test]
    fn test_resolve_defaults_to_enabled_when_no_config() {
        // V2: enabled-by-default. A missing/expired config falls back to ON.
        assert_eq!(
            resolve_auto_unlock_internal(None, None, None),
            Some(AUTO_UNLOCK_BASE_SECONDS)
        );
    }

    #[test]
    fn test_resolve_env_overrides_all_config_values() {
        for config_enabled in [Some(true), Some(false), None] {
            assert_eq!(resolve_auto_unlock_internal(Some(7200), config_enabled, Some(300)), Some(7200));
        }
    }

    #[test]
    fn test_resolve_config_persisted_base_interval() {
        // §2.7: setup-persisted base interval is honored when mode is backoff
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), Some(300)),
            Some(300)
        );
        // Legacy config without the field falls back to the build default
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), None),
            Some(AUTO_UNLOCK_BASE_SECONDS)
        );
        // Persisted base is ignored when mode is disabled or env is set
        assert_eq!(resolve_auto_unlock_internal(None, Some(false), Some(300)), None);
        assert_eq!(resolve_auto_unlock_internal(Some(600), Some(true), Some(300)), Some(600));
    }
}
