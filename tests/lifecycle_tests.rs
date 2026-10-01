//! Tray lifecycle tests (issue #27).
//!
//! Two seams per spec #24 testing decisions:
//! 1. `ConfigFile` round-trips — Change Passphrase / Preferences edit / Reset
//!    outcomes are verified as `config.toml` round-trips through the
//!    path-taking API (`save_to_path` / `load_from_path`) so tests never
//!    touch the real user config.
//! 2. Menu-state gating — which actions are available when
//!    locked/disabled/no-permissions (`preferences::menu_state`).

use handsoff::config::AutoUnlockConfig;
use handsoff::config_file::Config;
use handsoff::preferences::{change_passphrase_available_seam, menu_state, wipe_config_at, PreferencesEdit};
use handsoff::setup;
use handsoff::utils::hash_keycodes;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::LazyLock;

static PATH_SEQ: LazyLock<parking_lot::Mutex<u32>> = LazyLock::new(|| parking_lot::Mutex::new(0));

fn temp_config_path() -> PathBuf {
    let mut seq = PATH_SEQ.lock();
    *seq += 1;
    std::env::temp_dir().join(format!(
        "handsoff-lifecycle-test-{}-{}.toml",
        std::process::id(),
        *seq
    ))
}

fn valid_keys() -> Vec<u32> {
    vec![0, 12, 15, 37] // a s d f physical codes
}

fn seeded_config() -> Config {
    setup::assemble_config(&setup::SetupOutcome {
        keycodes: valid_keys(),
        auto_lock: 120,
        auto_unlock: AutoUnlockConfig::Backoff {
            base_interval_secs: NonZeroU64::new(3600).unwrap(),
        },
        lock_key: Some("L".to_string()),
        talk_key: Some("T".to_string()),
    })
    .expect("seed config must assemble")
}

// ---------------------------------------------------------------------------
// Menu-state gating (acceptance: "menu-state tests where logic is testable")
// ---------------------------------------------------------------------------

#[test]
fn test_menu_state_normal_unlocked() {
    let m = menu_state(false, false, true);
    assert!(m.lock_enabled);
    assert!(m.disable_enabled);
    assert!(m.reenable_enabled, "Reenable must be unguarded");
    assert!(m.preferences_enabled);
    assert!(m.change_passphrase_enabled);
    assert!(m.reset_enabled);
}

#[test]
fn test_menu_state_locked() {
    let m = menu_state(true, false, true);
    // Menu is unreachable while locked (mouse blocked) — gating covers races.
    assert!(!m.lock_enabled);
    assert!(!m.disable_enabled);
    assert!(m.reenable_enabled, "Reenable must be unguarded while locked");
    assert!(m.reset_enabled, "Reset must stay reachable (recovery path)");
}

#[test]
fn test_menu_state_disabled() {
    let m = menu_state(false, true, true);
    assert!(!m.lock_enabled);
    assert!(!m.disable_enabled);
    assert!(m.reenable_enabled, "Reenable must be unguarded while disabled");
}

#[test]
fn test_menu_state_no_permissions() {
    let m = menu_state(false, false, false);
    assert!(!m.lock_enabled, "Lock needs permissions");
    assert!(!m.disable_enabled, "Disable needs permissions");
    // Config-level actions and the escape hatch never need permissions.
    assert!(m.reenable_enabled);
    assert!(m.preferences_enabled);
    assert!(m.change_passphrase_enabled);
    assert!(m.reset_enabled);
}

// ---------------------------------------------------------------------------
// Change Passphrase round-trip (acceptance: new hash, other fields preserved)
// ---------------------------------------------------------------------------

#[test]
fn test_change_passphrase_round_trip_new_hash_others_preserved() {
    let path = temp_config_path();
    let original = seeded_config();
    original.save_to_path(&path).unwrap();
    let original_hash = original.passphrase_hash.clone().unwrap();
    let original_bytes = std::fs::read_to_string(&path).unwrap();

    let new_keys = vec![11u32, 7, 31, 45, 2]; // different sequence, 5 keys
    let updated = change_passphrase_available_seam(&path, &new_keys)
        .expect("change_passphrase must succeed for a ≥4-key sequence");

    // New hash, verifiable against the new keys and NOT the old ones.
    let new_hash = updated.passphrase_hash.clone().unwrap();
    assert_ne!(new_hash, original_hash, "hash must change");
    assert_eq!(new_hash, hash_keycodes(&new_keys), "hash must match new sequence");
    assert_ne!(
        hash_keycodes(&valid_keys()),
        new_hash,
        "old sequence must not verify against new hash"
    );

    // Every other field preserved.
    assert_eq!(updated.auto_lock_timeout, original.auto_lock_timeout);
    assert_eq!(updated.auto_unlock_mode, original.auto_unlock_mode);
    assert_eq!(
        updated.auto_unlock_base_interval,
        original.auto_unlock_base_interval
    );
    assert_eq!(updated.lock_hotkey, original.lock_hotkey);
    assert_eq!(updated.talk_hotkey, original.talk_hotkey);

    // Disk round-trip: reload and confirm identical.
    let reloaded = Config::load_from_path(&path).unwrap();
    assert_eq!(reloaded.passphrase_hash.as_deref(), Some(new_hash.as_str()));

    // Sanity: the file actually changed.
    let updated_bytes = std::fs::read_to_string(&path).unwrap();
    assert_ne!(original_bytes, updated_bytes);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_change_passphrase_rejects_short_sequence_without_touching_file() {
    let path = temp_config_path();
    let original = seeded_config();
    original.save_to_path(&path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    let result = change_passphrase_available_seam(&path, &[0, 12, 15]); // 3 < 4
    assert!(result.is_err(), "short sequence must be rejected");

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(before, after, "failed change must not touch the config");

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Preferences round-trip (acceptance: only intended fields changed)
// ---------------------------------------------------------------------------

#[test]
fn test_preferences_edit_changes_only_intended_fields() {
    let path = temp_config_path();
    let original = seeded_config();
    original.save_to_path(&path).unwrap();

    // Path-based pure edit: merge + validate without touching the standard
    // location (the standard-location `apply_preferences` is the tray's thin
    // wrapper over this with Config::load/save).
    let updated = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: None,                          // unchanged
            talk_key: None,                          // unchanged
            auto_lock: Some(300),                    // changed
            auto_unlock_base: Some(7200),            // changed
        },
    )
    .expect("valid edit must apply");

    assert_eq!(updated.auto_lock_timeout, 300, "auto_lock must change");
    assert_eq!(updated.auto_unlock_base_interval, Some(7200), "base must change");
    assert!(
        updated.auto_unlock_backoff_enabled(),
        "nonzero base keeps backoff enabled"
    );
    assert_eq!(updated.lock_hotkey, Some("L".to_string()), "lock hotkey preserved");
    assert_eq!(updated.talk_hotkey, Some("T".to_string()), "talk hotkey preserved");
    assert_eq!(
        updated.passphrase_hash, original.passphrase_hash,
        "passphrase hash must be preserved by Preferences"
    );

    // Disk round-trip.
    let reloaded = Config::load_from_path(&path).unwrap();
    assert_eq!(reloaded.auto_lock_timeout, 300);
    assert_eq!(reloaded.auto_unlock_base_interval, Some(7200));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_preferences_edit_hotkey_only() {
    let path = temp_config_path();
    let original = seeded_config();
    original.save_to_path(&path).unwrap();

    let updated = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: Some("Q".to_string()),
            talk_key: None,
            auto_lock: None,
            auto_unlock_base: None,
        },
    )
    .unwrap();

    assert_eq!(updated.lock_hotkey, Some("Q".to_string()));
    assert_eq!(updated.talk_hotkey, Some("T".to_string()));
    assert_eq!(updated.auto_lock_timeout, 120);
    assert_eq!(updated.passphrase_hash, original.passphrase_hash);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_preferences_disable_backoff() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();

    let updated = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: None,
            talk_key: None,
            auto_lock: None,
            auto_unlock_base: Some(0), // 0 = disable backoff
        },
    )
    .unwrap();

    assert_eq!(updated.auto_unlock_mode, "disabled");
    assert_eq!(updated.auto_unlock_base_interval, None);
    assert_eq!(updated.auto_lock_timeout, 120, "untouched field preserved");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_preferences_invalid_edit_leaves_file_intact() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    // Identical hotkeys violate the constructor invariant.
    let result = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: Some("Q".to_string()),
            talk_key: Some("Q".to_string()),
            auto_lock: None,
            auto_unlock_base: None,
        },
    );
    assert!(result.is_err(), "identical hotkeys must fail");

    // Out-of-range auto-lock.
    let result = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: None,
            talk_key: None,
            auto_lock: Some(10_000), // > AUTO_LOCK_MAX (600)
            auto_unlock_base: None,
        },
    );
    assert!(result.is_err(), "out-of-range timeout must fail");

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(before, after, "invalid edits must not touch the config");

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Reset (acceptance: config gone)
// ---------------------------------------------------------------------------

#[test]
fn test_wipe_config_removes_file() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    assert!(path.exists());

    let wiped = wipe_config_at(&path).expect("wipe must succeed");
    assert!(wiped, "must report an actual wipe");
    assert!(!path.exists(), "config must be gone");
}

#[test]
fn test_wipe_config_idempotent() {
    let path = temp_config_path();
    assert!(!path.exists());

    let wiped = wipe_config_at(&path).expect("wipe of absent config must be Ok");
    assert!(!wiped, "no file = no wipe");
}

// ---------------------------------------------------------------------------
// After Reset, the wizard's fresh outcome satisfies strict validation again
// (the tray's relaunch path depends on it).
// ---------------------------------------------------------------------------

#[test]
fn test_post_reset_wizard_outcome_revalidates() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    wipe_config_at(&path).unwrap();
    assert!(!path.exists());

    // Fresh wizard run (same shape tests/wizard_tests.rs validates).
    let fresh = setup::assemble_config(&setup::SetupOutcome {
        keycodes: vec![12, 15, 0, 37],
        auto_lock: 120,
        auto_unlock: AutoUnlockConfig::Backoff {
            base_interval_secs: NonZeroU64::new(3600).unwrap(),
        },
        lock_key: None,
        talk_key: None,
    })
    .unwrap();
    fresh.save_to_path(&path).unwrap();
    assert!(
        Config::load_from_path(&path).is_ok(),
        "post-Reset wizard config must load (tray relaunch gate)"
    );

    let _ = std::fs::remove_file(&path);
}
