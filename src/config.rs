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

use crate::app_state::{
    AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_UNLOCK_BASE_SECONDS,
    AUTO_UNLOCK_CEILING_SECONDS,
};
use log::{debug, info, warn};
use std::env;
use std::num::NonZeroU64;

/// Minimum allowed base interval for the auto-unlock backoff schedule.
/// Prevents an accidental instant unlock (windows open no sooner than this).
pub const AUTO_UNLOCK_MIN_BASE_SECONDS: u64 = 60;

/// Effective auto-unlock configuration (§2.7).
///
/// Replaces the former `Option<u64>` encoding (`None` = disabled, `Some(0)` =
/// force-disabled sentinel, `Some(n)` = base interval): the disabled state is
/// an explicit variant and the base interval cannot be zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoUnlockConfig {
    /// Auto-unlock disabled entirely.
    Disabled,
    /// Backoff schedule enabled with the given base interval (windows double
    /// up to `AUTO_UNLOCK_CEILING_SECONDS`).
    Backoff { base_interval_secs: NonZeroU64 },
}

/// Parse the HANDS_OFF_AUTO_UNLOCK environment variable.
///
/// The value overrides the auto-unlock *backoff base interval* (§2.7):
/// - `0` disables auto-unlock entirely → `AutoUnlockConfig::Disabled`
/// - `60..=86400` sets the base interval → `AutoUnlockConfig::Backoff`
/// - unset or invalid returns None (config file value is used)
pub fn parse_auto_unlock_config() -> Option<AutoUnlockConfig> {
    match env::var("HANDS_OFF_AUTO_UNLOCK") {
        Ok(val) => match val.parse::<u64>() {
            Ok(0) => {
                info!("Auto-unlock disabled via HANDS_OFF_AUTO_UNLOCK=0");
                Some(AutoUnlockConfig::Disabled)
            }
            Ok(seconds)
                if (AUTO_UNLOCK_MIN_BASE_SECONDS..=AUTO_UNLOCK_CEILING_SECONDS)
                    .contains(&seconds) =>
            {
                info!(
                    "Auto-unlock base interval set via environment variable: {} seconds",
                    seconds
                );
                Some(AutoUnlockConfig::Backoff {
                    base_interval_secs: NonZeroU64::new(seconds)
                        .expect("interval validated above minimum"),
                })
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

/// Resolve the auto-unlock backoff configuration (internal, testable version).
///
/// Precedence order:
/// 1. Environment variable (`Disabled` = force off; `Backoff` = base interval n)
/// 2. Config file: `backoff` mode → persisted base interval (falls back to
///    `AUTO_UNLOCK_BASE_SECONDS` for configs written before §2.7 stored it);
///    `disabled` → off (honored, §2.7 schema)
/// 3. Build-time default: **enabled** with `AUTO_UNLOCK_BASE_SECONDS` (V2) —
///    applies only when no config exists and no env var is set
fn resolve_auto_unlock_internal(
    env: Option<AutoUnlockConfig>,
    config_backoff_enabled: Option<bool>,
    config_base_interval: Option<u64>,
) -> AutoUnlockConfig {
    // 1. Environment variable wins: Disabled explicitly disables, Backoff
    //    overrides the base interval.
    if let Some(env_config) = env {
        return env_config;
    }
    // 2. Config file mode (None = no config file at all → build default)
    match config_backoff_enabled {
        Some(true) => {
            // Defense-in-depth backstop: config-file loading (§load_from_path)
            // and Config::new already reject out-of-range base intervals
            // loudly, so this clamp only guards direct callers bypassing
            // config load. Not the primary guard (Finding 1).
            let secs = config_base_interval
                .unwrap_or(AUTO_UNLOCK_BASE_SECONDS)
                .clamp(AUTO_UNLOCK_MIN_BASE_SECONDS, AUTO_UNLOCK_CEILING_SECONDS);
            AutoUnlockConfig::Backoff {
                base_interval_secs: NonZeroU64::new(secs).expect("clamped above minimum"),
            }
        }
        Some(false) => AutoUnlockConfig::Disabled,
        // 3. Build-time default: enabled-by-default (V2) for a fresh install
        // with no config file.
        None => AutoUnlockConfig::Backoff {
            base_interval_secs: NonZeroU64::new(AUTO_UNLOCK_BASE_SECONDS)
                .expect("AUTO_UNLOCK_BASE_SECONDS is nonzero"),
        },
    }
}

/// Resolve the auto-unlock backoff configuration.
///
/// Precedence: env var > config file (mode + persisted base interval) >
/// build-time default (enabled). `config_backoff_enabled`: `Some(mode)` from a
/// loaded config, `None` when no config exists. `config_base_interval`: the
/// persisted §2.7 base interval, if any.
pub fn resolve_auto_unlock(
    config_backoff_enabled: Option<bool>,
    config_base_interval: Option<u64>,
) -> AutoUnlockConfig {
    resolve_auto_unlock_internal(
        parse_auto_unlock_config(),
        config_backoff_enabled,
        config_base_interval,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    /// Serialize tests that mutate process environment variables: libtest
    /// runs tests on parallel threads, and env vars are process-global, so
    /// concurrent set_var/remove_var races can flake (e.g. an invalid-value
    /// test observing the value another test just set).
    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn env_lock() -> MutexGuard<'static, ()> {
        // A poisoned lock only means a previous env test panicked; the env
        // mutations themselves are harmless, so proceed past poisoning.
        ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn backoff(secs: u64) -> AutoUnlockConfig {
        AutoUnlockConfig::Backoff {
            base_interval_secs: NonZeroU64::new(secs).unwrap(),
        }
    }

    #[test]
    fn test_parse_auto_unlock_zero_disables() {
        let _env = env_lock();
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "0");
        assert_eq!(
            parse_auto_unlock_config(),
            Some(AutoUnlockConfig::Disabled),
            "0 means explicit disable"
        );
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_valid_base_intervals() {
        let _env = env_lock();
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "60");
        assert_eq!(parse_auto_unlock_config(), Some(backoff(60)));

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "3600");
        assert_eq!(parse_auto_unlock_config(), Some(backoff(3600)));

        env::set_var("HANDS_OFF_AUTO_UNLOCK", "86400");
        assert_eq!(parse_auto_unlock_config(), Some(backoff(86400)));

        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_invalid_values() {
        let _env = env_lock();
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");

        // Below minimum base
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "59");
        assert_eq!(parse_auto_unlock_config(), None, "Should reject below 60");

        // Above ceiling
        env::set_var("HANDS_OFF_AUTO_UNLOCK", "86401");
        assert_eq!(
            parse_auto_unlock_config(),
            None,
            "Should reject above 86400"
        );

        // Negative / non-numeric / units / empty
        for bad in ["-60", "invalid", "30s", ""] {
            env::set_var("HANDS_OFF_AUTO_UNLOCK", bad);
            assert_eq!(parse_auto_unlock_config(), None, "Should reject {:?}", bad);
        }

        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
    }

    #[test]
    fn test_parse_auto_unlock_not_set() {
        let _env = env_lock();
        env::remove_var("HANDS_OFF_AUTO_UNLOCK");
        assert_eq!(
            parse_auto_unlock_config(),
            None,
            "Should return None when not set, allowing config value"
        );
    }

    #[test]
    fn test_parse_auto_lock_valid_values() {
        let _env = env_lock();
        env::set_var("HANDS_OFF_AUTO_LOCK", "20");
        assert_eq!(parse_auto_lock_timeout(), Some(20));

        env::set_var("HANDS_OFF_AUTO_LOCK", "600");
        assert_eq!(parse_auto_lock_timeout(), Some(600));

        env::remove_var("HANDS_OFF_AUTO_LOCK");
    }

    #[test]
    fn test_parse_auto_lock_invalid_values() {
        let _env = env_lock();
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
        let _env = env_lock();
        // Env base interval overrides a disabled config
        assert_eq!(
            resolve_auto_unlock_internal(Some(backoff(300)), Some(false), Some(7200)),
            backoff(300)
        );
        // Env base interval overrides an enabled config
        assert_eq!(
            resolve_auto_unlock_internal(Some(backoff(300)), Some(true), Some(7200)),
            backoff(300)
        );
    }

    #[test]
    fn test_resolve_env_zero_disables_even_when_config_enabled() {
        let _env = env_lock();
        assert_eq!(
            resolve_auto_unlock_internal(Some(AutoUnlockConfig::Disabled), Some(true), Some(7200)),
            AutoUnlockConfig::Disabled
        );
        assert_eq!(
            resolve_auto_unlock_internal(Some(AutoUnlockConfig::Disabled), Some(false), None),
            AutoUnlockConfig::Disabled
        );
    }

    #[test]
    fn test_resolve_config_backoff_enabled() {
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), None),
            backoff(AUTO_UNLOCK_BASE_SECONDS)
        );
    }

    #[test]
    fn test_resolve_config_disabled_is_honored() {
        // §2.7: an explicit config `disabled` mode must stay disabled
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(false), Some(7200)),
            AutoUnlockConfig::Disabled
        );
    }

    #[test]
    fn test_resolve_defaults_to_enabled_when_no_config() {
        // V2: enabled-by-default. A missing/expired config falls back to ON.
        assert_eq!(
            resolve_auto_unlock_internal(None, None, None),
            backoff(AUTO_UNLOCK_BASE_SECONDS)
        );
    }

    #[test]
    fn test_resolve_env_overrides_all_config_values() {
        let _env = env_lock();
        for config_enabled in [Some(true), Some(false), None] {
            assert_eq!(
                resolve_auto_unlock_internal(Some(backoff(7200)), config_enabled, Some(300)),
                backoff(7200)
            );
        }
    }

    #[test]
    fn test_resolve_config_persisted_base_interval() {
        // §2.7: setup-persisted base interval is honored when mode is backoff
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), Some(300)),
            backoff(300)
        );
        // Legacy config without the field falls back to the build default
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), None),
            backoff(AUTO_UNLOCK_BASE_SECONDS)
        );
        // Persisted base is ignored when mode is disabled or env is set
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(false), Some(300)),
            AutoUnlockConfig::Disabled
        );
        assert_eq!(
            resolve_auto_unlock_internal(Some(backoff(600)), Some(true), Some(300)),
            backoff(600)
        );
    }

    #[test]
    fn test_resolve_config_clamps_out_of_range_base_interval() {
        // Backstop behavior only: config load and Config::new reject
        // out-of-range base intervals loudly; direct resolver callers that
        // bypass config load get clamped instead of panicking.
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), Some(0)),
            backoff(AUTO_UNLOCK_MIN_BASE_SECONDS)
        );
        assert_eq!(
            resolve_auto_unlock_internal(None, Some(true), Some(999_999)),
            backoff(AUTO_UNLOCK_CEILING_SECONDS)
        );
    }

}
