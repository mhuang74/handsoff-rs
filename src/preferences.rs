//! Tray lifecycle logic for Preferences, Change Passphrase, and Reset
//! (issue #27).
//!
//! The GUI windows and menu items are a thin shell over the functions here;
//! everything that touches the config file goes through the `ConfigFile`
//! seam (`Config::load` / `save_to_path` / `save`) so each user action is
//! verifiable as a `config.toml` round-trip (spec #24 testing decisions).
//!
//! Terminology (CONTEXT.md): Reset wipes the config and restarts the Setup
//! Wizard; Reenable (the old "Reset") restarts input capture without changing
//! any configuration; Preferences edits settings without re-entering the
//! Passphrase; Change Passphrase rotates the passphrase while preserving all
//! other fields.

use crate::config_file::Config;
use crate::setup::SetupOutcome;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Which tray actions are available in a given app state (issue #27).
///
/// Pure function so the gating rules are unit-testable; the tray's event loop
/// just applies the flags to the menu items each poll.
///
/// Rules:
/// - **Lock**: needs permissions; never while locked (menu is unreachable
///   when locked anyway — mouse clicks are blocked — this covers races) or
///   disabled.
/// - **Disable**: needs permissions; not while locked or already disabled.
/// - **Reenable** (old Reset): ALWAYS enabled — the deliberate anti-lockout
///   escape hatch (CONTEXT.md: unguarded, by design).
/// - **Preferences / Change Passphrase / Reset**: config-level actions; they
///   open windows or wipe config, none of which needs input blocking or
///   permissions, so they are ALWAYS available. Reset must stay reachable in
///   principle from any state (it is the recovery path for a forgotten
///   passphrase); in practice the mouse is blocked while locked, so the
///   realistic path is Disable → Reset — but the item is never disabled on
///   our side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    pub lock_enabled: bool,
    pub disable_enabled: bool,
    /// Always true: unguarded escape hatch.
    pub reenable_enabled: bool,
    /// Always true: opens the Preferences window.
    pub preferences_enabled: bool,
    /// Always true: opens the Change Passphrase flow.
    pub change_passphrase_enabled: bool,
    /// Always true: double-confirmed wipe + wizard relaunch.
    pub reset_enabled: bool,
}

pub fn menu_state(is_locked: bool, is_disabled: bool, has_permissions: bool) -> MenuState {
    MenuState {
        lock_enabled: has_permissions && !is_locked && !is_disabled,
        disable_enabled: has_permissions && !is_locked && !is_disabled,
        reenable_enabled: true,
        preferences_enabled: true,
        change_passphrase_enabled: true,
        reset_enabled: true,
    }
}

/// Everything the Preferences window collects for a settings-only save.
///
/// `None` hotkey fields keep the stored value (the wizard form treats empty
/// as "use default"; Preferences treats empty as "unchanged" — a user who
/// cleared a field did not ask to reset it to L/T, and silently doing so
/// would break `validate_config_strict`'s distinctness invariant only after
/// the fact).
#[derive(Debug, Clone, Default)]
pub struct PreferencesEdit {
    /// Lock hotkey last key (A-Z); `None` keeps the current value.
    pub lock_key: Option<String>,
    /// Talk hotkey last key (A-Z); `None` keeps the current value.
    pub talk_key: Option<String>,
    /// Auto-lock timeout in seconds; `None` keeps the current value.
    pub auto_lock: Option<u64>,
    /// Auto-unlock base interval in seconds; `0` disables backoff, `None`
    /// keeps the current mode and interval.
    pub auto_unlock_base: Option<u64>,
}

impl PreferencesEdit {
    /// Whether any field would change anything (used to skip a save when the
    /// user opened Preferences and clicked Save without editing).
    pub fn is_noop(&self, cfg: &Config) -> bool {
        let lock_changed = self
            .lock_key
            .as_deref()
            .is_some_and(|k| !k.eq_ignore_ascii_case(cfg.lock_hotkey.as_deref().unwrap_or("L")));
        let talk_changed = self
            .talk_key
            .as_deref()
            .is_some_and(|k| !k.eq_ignore_ascii_case(cfg.talk_hotkey.as_deref().unwrap_or("T")));
        let auto_lock_changed = self.auto_lock.is_some_and(|v| v != cfg.auto_lock_timeout);
        let unlock_changed = match self.auto_unlock_base {
            None => false,
            Some(0) => cfg.auto_unlock_backoff_enabled(),
            Some(v) => {
                !cfg.auto_unlock_backoff_enabled()
                    || cfg.auto_unlock_base_interval.unwrap_or(0) != v
            }
        };
        !lock_changed && !talk_changed && !auto_lock_changed && !unlock_changed
    }
}

/// Apply a preferences edit to the config at the standard location.
///
/// Only the supplied fields change; the passphrase hash, format, and every
/// unmentioned field are preserved. The constructor remains authoritative for
/// all ranges (distinct hotkeys, timeout bounds, base interval range), so an
/// invalid edit fails without touching the file — the old config stays
/// intact and the caller surfaces the error.
///
/// # Errors
/// Config load failure, or a constructor range violation (nothing is saved).
pub fn apply_preferences(edit: &PreferencesEdit) -> Result<Config> {
    let current = Config::load().context("Failed to load current configuration")?;
    let path = Config::config_path();
    let updated =
        apply_preferences_to(&current, path, edit).context("Failed to save configuration")?;
    Ok(updated)
}

/// Path-taking variant of `apply_preferences` (the testable core).
///
/// Loads from `path`, merges `edit`, validates via the `Config` constructor,
/// and saves back to `path`. Nothing is written when validation fails.
pub fn apply_preferences_to_path(path: &std::path::Path, edit: &PreferencesEdit) -> Result<Config> {
    let current = Config::load_from_path(path)
        .context("Failed to load current configuration")?;
    apply_preferences_to(&current, path.to_path_buf(), edit)
        .context("Failed to save configuration")
}

/// Merge core shared by both wrappers: validate the merged config via the
/// constructor, preserve the stored passphrase, persist to `path`.
fn apply_preferences_to(
    current: &Config,
    path: PathBuf,
    edit: &PreferencesEdit,
) -> Result<Config> {
    let lock_key = match &edit.lock_key {
        Some(k) => Some(Config::normalize_hotkey(k)),
        None => current.lock_hotkey.clone(),
    };
    let talk_key = match &edit.talk_key {
        Some(k) => Some(Config::normalize_hotkey(k)),
        None => current.talk_hotkey.clone(),
    };
    let auto_lock = edit.auto_lock.unwrap_or(current.auto_lock_timeout);

    let (backoff_enabled, base) = match edit.auto_unlock_base {
        None => (
            current.auto_unlock_backoff_enabled(),
            current.auto_unlock_base_interval.unwrap_or(0),
        ),
        Some(0) => (false, 0),
        Some(v) => (true, v),
    };

    // Round-trip through the constructor: it validates the merged result
    // (ranges, distinctness) before anything is written. The passphrase
    // sequence is a placeholder — `preserve_hash` re-attaches the stored
    // hash so the saved file keeps the working passphrase.
    let placeholder_keys = [0u32; 4];
    let updated = Config::new(
        &placeholder_keys,
        auto_lock,
        backoff_enabled,
        base,
        lock_key,
        talk_key,
    )
    .and_then(|c| c.preserve_hash(current))
    .context("Failed to apply preferences")?;

    persist_to(&updated, &path)?;
    Ok(updated)
}

/// Change the passphrase, preserving every other config field.
///
/// The caller captures the new sequence via the silent capture path
/// (`setup::capture_passphrase_headless`) with double entry; this function
/// receives the confirmed sequence and validates it the same way the wizard
/// does (≥4 keys via the `Config` constructor), then saves the merged config.
/// Not a wipe: hotkeys, timeouts, and mode are untouched.
///
/// `keys` come from the caller's double-entry confirm — this function cannot
/// verify the user retyped identically (that comparison happens in the silent
/// capture path, where cleartext never exists).
///
/// # Errors
/// Config load failure, or a too-short sequence (nothing is saved).
pub fn change_passphrase(keys: &[u32]) -> Result<Config> {
    let current = Config::load().context("Failed to load current configuration")?;
    let updated = change_passphrase_to(&current, keys)
        .context("Failed to apply new passphrase")?;
    persist_to(&updated, &Config::config_path())
        .context("Failed to save configuration")?;
    Ok(updated)
}

/// Path-taking variant (the testable core): loads from `path`, replaces only
/// the passphrase hash, saves back to `path`. Nothing written on failure.
pub fn change_passphrase_to_path(path: &std::path::Path, keys: &[u32]) -> Result<Config> {
    let current = Config::load_from_path(path)
        .context("Failed to load current configuration")?;
    let updated =
        change_passphrase_to(&current, keys).context("Failed to apply new passphrase")?;
    persist_to(&updated, path).context("Failed to save configuration")?;
    Ok(updated)
}

/// Alias kept for test readability: same as `change_passphrase_to_path`.
pub fn change_passphrase_available_seam(
    path: &std::path::Path,
    keys: &[u32],
) -> Result<Config> {
    change_passphrase_to_path(path, keys)
}

/// Merge core shared by both wrappers: validate the new sequence via the
/// constructor and persist with every other field carried over.
fn change_passphrase_to(current: &Config, keys: &[u32]) -> Result<Config> {
    Config::new(
        keys,
        current.auto_lock_timeout,
        current.auto_unlock_backoff_enabled(),
        current.auto_unlock_base_interval.unwrap_or(0),
        current.lock_hotkey.clone(),
        current.talk_hotkey.clone(),
    )
}

/// Wipe the configuration (the Reset recovery path).
///
/// Removes the config file at the standard location and returns whether a
/// file actually existed. Idempotent: removing a nonexistent config is `Ok`.
/// The tray follows this with a relaunch into the Setup Wizard, which the
/// strict-validation gate then triggers on the next startup.
///
/// # Errors
/// The file exists but could not be removed.
pub fn wipe_config() -> Result<bool> {
    let path = Config::config_path();
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("Failed to remove config file: {}", path.display()))?;
    log::info!("Configuration wiped: {}", path.display());
    Ok(true)
}

/// Path-taking variant (the testable core).
pub fn wipe_config_at(path: &std::path::Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)
        .with_context(|| format!("Failed to remove config file: {}", path.display()))?;
    Ok(true)
}

/// Persist a config to the STANDARD-LOCATION path with the same hardening as
/// `Config::save`: creates the parent directory and enforces 0600 on unix
/// (`save_to_path` alone does neither — its doc comment in config_file.rs).
/// The path-taking test variants must NOT use this: tests write to temp dirs
/// with plain `save_to_path` semantics.
#[cfg(unix)]
fn persist_to(cfg: &Config, path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }
    cfg.save_to_path(path)?;
    let mut permissions = std::fs::metadata(path)
        .context("Failed to read config file metadata")?
        .permissions();
    permissions.set_mode(crate::constants::CONFIG_FILE_PERMISSIONS);
    std::fs::set_permissions(path, permissions)
        .context("Failed to set config file permissions")?;
    Ok(())
}

/// Non-unix variant: directory creation only (nothing to harden without
/// POSIX modes; mirrors `Config::save`'s cfg(unix) scoping).
#[cfg(not(unix))]
fn persist_to(cfg: &Config, path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }
    cfg.save_to_path(path).context("Failed to write config file")
}

/// Build a full `SetupOutcome`-shaped value from an existing config plus a
/// fresh passphrase sequence — the shape the wizard's config assembly expects
/// when re-creating a config after Reset with the same settings prefilled.
///
/// Preferences never uses this (it edits the loaded config in place); the
/// wizard after a Reset starts from scratch. Provided for the tray's
/// "relaunch wizard after Reset" path so the strict-validation gate sees a
/// config identical to a fresh wizard run.
pub fn outcome_for_fresh_setup(
    keys: Vec<u32>,
    auto_lock: u64,
    auto_unlock: crate::config::AutoUnlockConfig,
) -> SetupOutcome {
    SetupOutcome {
        keycodes: keys,
        auto_lock,
        auto_unlock,
        lock_key: None,
        talk_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AutoUnlockConfig;
    use crate::setup;
    use std::num::NonZeroU64;

    fn seeded_config() -> Config {
        setup::assemble_config(&setup::SetupOutcome {
            keycodes: vec![0, 12, 15, 37],
            auto_lock: 120,
            auto_unlock: AutoUnlockConfig::Backoff {
                base_interval_secs: NonZeroU64::new(3600).unwrap(),
            },
            lock_key: Some("L".to_string()),
            talk_key: Some("T".to_string()),
        })
        .expect("seed config must assemble")
    }

    #[test]
    fn test_preferences_edit_noop_detection() {
        let cfg = seeded_config();

        assert!(PreferencesEdit::default().is_noop(&cfg));
        assert!(PreferencesEdit {
            lock_key: Some("l".to_string()), // case-insensitive compare
            talk_key: None,
            auto_lock: None,
            auto_unlock_base: None,
        }
        .is_noop(&cfg));

        assert!(!PreferencesEdit {
            lock_key: None,
            talk_key: None,
            auto_lock: Some(240),
            auto_unlock_base: None,
        }
        .is_noop(&cfg));
    }
}
