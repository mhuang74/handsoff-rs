//! Tray lifecycle tests (issue #27).
//!
//! Two seams per spec #24 testing decisions:
//! 1. `ConfigFile` round-trips — Change Passphrase / Preferences edit / Reset
//!    outcomes are verified as `config.toml` round-trips through the
//!    path-taking API (`save_to_path` / `load_from_path`) so tests never
//!    touch the real user config.
//! 2. Menu-state gating — which actions are available when
//!    locked/disabled/no-permissions (`preferences::menu_state`).

use handsoff::app_state::AppState;
use handsoff::config::AutoUnlockConfig;
use handsoff::config_file::Config;
use handsoff::preferences::{
    change_passphrase_available_seam, change_passphrase_verified_to_path, menu_state,
    wipe_config_at, PreferencesEdit,
};
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
    assert!(
        m.reenable_enabled,
        "Reenable must be unguarded while locked"
    );
    // N5: a dead-tap-while-locked window must not allow re-keying or wiping.
    assert!(!m.change_passphrase_enabled);
    assert!(!m.reset_enabled);
}

#[test]
fn test_menu_state_disabled() {
    let m = menu_state(false, true, true);
    assert!(!m.lock_enabled);
    assert!(!m.disable_enabled);
    assert!(
        m.reenable_enabled,
        "Reenable must be unguarded while disabled"
    );
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

/// Table-driven matrix over (locked × disabled × permissions) × action
/// (issue #37 N2/N5: the gating authority must be exhaustive and consistent).
#[test]
fn test_menu_state_gating_matrix() {
    // (is_locked, is_disabled, has_permissions)
    for &(locked, disabled, perms) in &[
        (false, false, true),
        (false, false, false),
        (true, false, true),
        (true, false, false),
        (false, true, true),
        (false, true, false),
    ] {
        let m = menu_state(locked, disabled, perms);
        let label = format!("locked={locked} disabled={disabled} perms={perms}");

        // Lock: requires permissions; never while locked or disabled.
        assert_eq!(
            m.lock_enabled,
            perms && !locked && !disabled,
            "lock: {label}"
        );
        // Disable: requires permissions; never while locked or disabled.
        assert_eq!(
            m.disable_enabled,
            perms && !locked && !disabled,
            "disable: {label}"
        );
        // Reenable: unguarded escape hatch, ALWAYS available.
        assert!(m.reenable_enabled, "reenable: {label}");
        // Preferences: always available.
        assert!(m.preferences_enabled, "preferences: {label}");
        // Change Passphrase / Reset: refused while locked (N5); otherwise
        // always available.
        assert_eq!(
            m.change_passphrase_enabled, !locked,
            "change_passphrase: {label}"
        );
        assert_eq!(m.reset_enabled, !locked, "reset: {label}");
    }
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
    assert_eq!(
        new_hash,
        hash_keycodes(&new_keys),
        "hash must match new sequence"
    );
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
            lock_key: None,               // unchanged
            talk_key: None,               // unchanged
            auto_lock: Some(300),         // changed
            auto_unlock_base: Some(7200), // changed
        },
    )
    .expect("valid edit must apply");

    assert_eq!(updated.auto_lock_timeout, 300, "auto_lock must change");
    assert_eq!(
        updated.auto_unlock_base_interval,
        Some(7200),
        "base must change"
    );
    assert!(
        updated.auto_unlock_backoff_enabled(),
        "nonzero base keeps backoff enabled"
    );
    assert_eq!(
        updated.lock_hotkey,
        Some("L".to_string()),
        "lock hotkey preserved"
    );
    assert_eq!(
        updated.talk_hotkey,
        Some("T".to_string()),
        "talk hotkey preserved"
    );
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

// ---------------------------------------------------------------------------
// Legacy config tolerance (issue #37 N8): backoff mode with no stored base
// interval must not break Preferences or Change Passphrase.
// ---------------------------------------------------------------------------

/// Backoff-mode config with NO `auto_unlock_base_interval` (legacy shape).
fn write_legacy_backoff_config(path: &std::path::Path) {
    let toml_src = format!(
        r#"
passphrase_hash = "{}"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
"#,
        hash_keycodes(&valid_keys())
    );
    std::fs::write(path, toml_src).expect("Failed to write legacy config");
}

#[test]
fn test_legacy_backoff_config_change_passphrase_falls_back_to_default_base() {
    let path = temp_config_path();
    write_legacy_backoff_config(&path);

    // Load succeeds (the stored hash is preserved through the merge).
    let new_keys = vec![11u32, 7, 31, 45];
    let updated = change_passphrase_available_seam(&path, &new_keys)
        .expect("Change Passphrase must tolerate a legacy backoff config");

    assert_eq!(updated.auto_unlock_mode, "backoff");
    assert_eq!(
        updated.auto_unlock_base_interval,
        Some(handsoff::constants::AUTO_UNLOCK_BASE_SECONDS),
        "missing base must fall back to the runtime default (3600 s)"
    );
    assert_eq!(
        updated.passphrase_hash.as_deref(),
        Some(hash_keycodes(&new_keys).as_str()),
        "new hash must be saved"
    );

    // The reloaded config validates (no base=0 constructor failure).
    let reloaded = Config::load_from_path(&path).expect("reloaded legacy-updated config");
    assert_eq!(
        reloaded.auto_unlock_base_interval,
        Some(handsoff::constants::AUTO_UNLOCK_BASE_SECONDS)
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_legacy_backoff_config_preferences_falls_back_to_default_base() {
    let path = temp_config_path();
    write_legacy_backoff_config(&path);

    let updated = handsoff::preferences::apply_preferences_to_path(
        &path,
        &PreferencesEdit {
            lock_key: None,
            talk_key: None,
            auto_lock: Some(240),
            auto_unlock_base: None,
        },
    )
    .expect("Preferences must tolerate a legacy backoff config");

    assert_eq!(updated.auto_lock_timeout, 240);
    assert_eq!(updated.auto_unlock_mode, "backoff");
    assert_eq!(
        updated.auto_unlock_base_interval,
        Some(handsoff::constants::AUTO_UNLOCK_BASE_SECONDS),
        "missing base must fall back to the runtime default (3600 s)"
    );
    assert_eq!(
        updated.passphrase_hash.as_deref(),
        Some(hash_keycodes(&valid_keys()).as_str()),
        "stored passphrase must be preserved"
    );

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// N2: a deferred Disable-then-Lock sequence cannot end in locked-without-tap.
// The gate refuses Disable while locked (menu_state), and clear_lock_state
// guarantees the Lock flag never outlives a stopped tap.
// ---------------------------------------------------------------------------

#[test]
fn test_disable_clears_lock_state_without_tap() {
    let state = AppState::new();
    state.set_locked(true);
    assert!(state.is_locked());

    // What disable() does to STATE before stopping the tap.
    state.clear_lock_state();

    assert!(
        !state.is_locked(),
        "Disable must never leave is_locked=true — a stopped tap enforces nothing"
    );
}

#[test]
fn test_deferred_disable_then_lock_sequence_gate() {
    // Deferred dispatch (issue #36) re-validates each click against
    // menu_state. Sequence from N2: Disable click queued, then Lock click
    // queued, then state resolves. After the Disable executes,
    // clear_lock_state runs — the subsequent Lock click is then evaluated
    // against the CURRENT state, where the gate must now deny a second
    // Disable (already disabled) and allow Lock only if unlocked.
    //
    // The dangerous combination — locked WITHOUT tap — is impossible if
    // both properties hold:
    //   (a) the gate denies Disable while locked, so Lock can't run after a
    //       refused Disable;
    //   (b) when Disable DOES run, clear_lock_state clears is_locked, so
    //       Lock-after-Disable re-locks only with the tap restarting later
    //       under explicit permission checks (lock() refuses without perms).
    let locked_state = AppState::new();
    locked_state.set_locked(true);

    // (a): gate refuses Disable while locked.
    assert!(
        !menu_state(true, false, true).disable_enabled,
        "Disable must be gated off while locked (no locked-without-tap via deferred dispatch)"
    );

    // (b): when Disable runs (unlocked), the Lock flag cannot survive it.
    let unlocked_state = AppState::new();
    unlocked_state.set_locked(true);
    unlocked_state.clear_lock_state();
    assert!(
        !unlocked_state.is_locked(),
        "disable path must clear the Lock flag"
    );
}

// ---------------------------------------------------------------------------
// Change Passphrase verification flow (issue #37 N6): verify-then-recapture.
// The captured sequences stand in for the live capture tap (stubbed capture
// closure at the dialog level); the assertions here check that authentication
// of the CURRENT Passphrase is enforced and that failure never touches disk.
// ---------------------------------------------------------------------------

#[test]
fn test_change_passphrase_verified_correct_current_succeeds() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    let original_hash = hash_keycodes(&valid_keys());

    let new_keys = vec![11u32, 7, 31, 45];
    let updated = change_passphrase_verified_to_path(&path, &valid_keys(), &new_keys)
        .expect("correct current Passphrase must allow the change");

    assert_eq!(
        updated.passphrase_hash.as_deref(),
        Some(hash_keycodes(&new_keys).as_str()),
        "new hash must be saved"
    );
    // Other fields preserved (backoff contract untouched — story 15).
    assert_eq!(updated.auto_lock_timeout, 120);
    assert_eq!(updated.auto_unlock_mode, original_mode_seeded());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_change_passphrase_verified_wrong_current_refuses() {
    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    // Wrong "current" sequence: authentication must fail, file untouched.
    let wrong_current = vec![11u32, 2, 8, 46];
    let result = change_passphrase_verified_to_path(&path, &wrong_current, &valid_keys());
    assert!(result.is_err(), "wrong current Passphrase must be refused");
    assert!(
        result.unwrap_err().to_string().contains("NOT changed"),
        "error must state nothing was changed"
    );

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        before, after,
        "failed verification must not touch the config"
    );

    let _ = std::fs::remove_file(&path);
}

fn original_mode_seeded() -> String {
    "backoff".to_string()
}

// ---------------------------------------------------------------------------
// Config permissions (issue #37 N7): atomic 0600 creation, permissive-mode
// auto-repair, hard failure only when the repair itself fails.
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn test_config_new_file_is_0600_from_creation() {
    use std::os::unix::fs::PermissionsExt;

    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "new config file must be 0600 from creation (no window)"
    );

    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn test_config_permissive_mode_repaired_on_load() {
    use std::os::unix::fs::PermissionsExt;

    let path = temp_config_path();
    seeded_config().save_to_path(&path).unwrap();
    // Simulate a pre-existing world-readable config.
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&path, perms).unwrap();

    let loaded = Config::load_from_path(&path)
        .expect("permissive-mode config must LOAD (repair, not wizard)");
    assert_eq!(
        loaded.passphrase_hash.as_deref(),
        Some(hash_keycodes(&valid_keys()).as_str()),
        "content must be untouched by the repair"
    );

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "load must chmod the config to 0600");

    let _ = std::fs::remove_file(&path);
}
