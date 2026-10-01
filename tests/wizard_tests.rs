//! Setup-wizard core tests (issue #26).
//!
//! Two seams per spec #24 testing decisions:
//! 1. `ConfigFile` round-trips — a fresh wizard-shaped outcome must produce a
//!    `config.toml` that loads and passes strict validation.
//! 2. Strict validation table over malformed configs (the tray's
//!    "run or wizard" decision).
//!
//! These are cross-platform (no macOS FFI); the GUI itself is manual-smoke.

use handsoff::config_file::Config;
use handsoff::setup;
use handsoff::wizard::{LoginItemResult, WizardOutcome};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::LazyLock;
/// Process-global temp dir is fine: each test uses a unique file name.
static PATH_SEQ: LazyLock<parking_lot::Mutex<u32>> = LazyLock::new(|| parking_lot::Mutex::new(0));

fn temp_config_path() -> PathBuf {
    let mut seq = PATH_SEQ.lock();
    *seq += 1;
    std::env::temp_dir().join(format!(
        "handsoff-wizard-test-{}-{}.toml",
        std::process::id(),
        *seq
    ))
}

/// Build a valid wizard outcome (defaults matching the wizard form).
fn valid_outcome() -> WizardOutcome {
    WizardOutcome {
        setup: setup::SetupOutcome {
            keycodes: vec![0, 12, 15, 37], // a s d f physical codes
            auto_lock: 120,
            auto_unlock: handsoff::config::AutoUnlockConfig::Backoff {
                base_interval_secs: NonZeroU64::new(3600).unwrap(),
            },
            lock_key: Some("L".to_string()),
            talk_key: Some("T".to_string()),
        },
        login_item: LoginItemResult::Disabled,
    }
}

// ---------------------------------------------------------------------------
// Seam 1: wizard outcome → config.toml round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_wizard_outcome_round_trips_to_valid_config() {
    let path = temp_config_path();

    // Pure assembly (no I/O), then save/load on a temp path: tests must
    // never touch the real user config.
    let assembled = setup::assemble_config(&valid_outcome().setup)
        .expect("assemble_config must succeed for a valid outcome");

    // Round-trip: serialize the assembled config to the temp path and load.
    assembled
        .save_to_path(&path)
        .expect("save_to_path must succeed");
    let loaded = Config::load_from_path(&path).expect("load_from_path must succeed");

    assert_eq!(
        loaded.passphrase_hash,
        assembled.passphrase_hash,
        "hash must survive the round-trip unchanged"
    );
    assert_eq!(loaded.auto_lock_timeout, 120);
    assert_eq!(loaded.auto_unlock_mode, "backoff");
    assert_eq!(loaded.auto_unlock_base_interval, Some(3600));
    assert_eq!(loaded.lock_hotkey.as_deref(), Some("L"));
    assert_eq!(loaded.talk_hotkey.as_deref(), Some("T"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_wizard_outcome_passes_strict_validation_shape() {
    // Strict validation is defined as: load ok + distinct hotkeys. The
    // loaded-from-disk config here satisfies the same invariants the tray
    // checks (validate_config_strict itself hits the user config dir, so the
    // invariant check runs on the temp-loaded config instead).
    let path = temp_config_path();
    let assembled = setup::assemble_config(&valid_outcome().setup).unwrap();
    assembled.save_to_path(&path).unwrap();
    let loaded = Config::load_from_path(&path).unwrap();

    let lock = loaded.get_lock_key_code().expect("lock hotkey parses");
    let talk = loaded.get_talk_key_code().expect("talk hotkey parses");
    assert_ne!(lock, talk, "wizard must never emit identical hotkeys");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_wizard_rejects_short_passphrase() {
    let mut o = valid_outcome();
    o.setup.keycodes = vec![0, 12, 15]; // 3 keys < MIN 4
    assert!(
        setup::assemble_config(&o.setup).is_err(),
        "a wizard outcome with a too-short sequence must fail at the constructor"
    );
}

#[test]
fn test_wizard_rejects_identical_hotkeys() {
    let mut o = valid_outcome();
    o.setup.lock_key = Some("Q".to_string());
    o.setup.talk_key = Some("Q".to_string());
    assert!(
        setup::assemble_config(&o.setup).is_err(),
        "identical hotkeys must fail at the constructor"
    );
}

#[test]
fn test_wizard_rejects_out_of_range_auto_lock() {
    let mut o = valid_outcome();
    o.setup.auto_lock = 5; // < AUTO_LOCK_MIN_SECONDS (20)
    assert!(setup::assemble_config(&o.setup).is_err());

    let mut o = valid_outcome();
    o.setup.auto_lock = 10_000; // > AUTO_LOCK_MAX_SECONDS (600)
    assert!(setup::assemble_config(&o.setup).is_err());
}

// ---------------------------------------------------------------------------
// Seam 2: strict-validation table over malformed configs
// ---------------------------------------------------------------------------

/// Table: malformed TOML bodies that must all fail `load_from_path` (the
/// core of `validate_config_strict`). Each row is a config that a broken
/// wizard, a hand edit, or a legacy install could produce.
#[test]
fn test_strict_validation_rejects_malformed_configs() {
    let cases: Vec<(&str, String)> = vec![
        (
            "missing passphrase hash",
            r#"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
"#
            .to_string(),
        ),
        (
            "legacy encrypted_passphrase format",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "encrypted-passphrase"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
"#
            .to_string(),
        ),
        (
            "truncated hash (not 64 hex chars)",
            r#"
passphrase_hash = "abc123"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
"#
            .to_string(),
        ),
        (
            "empty-sequence hash",
            format!(
                r#"
passphrase_hash = "{}"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
"#,
                handsoff::utils::hash_keycodes(&[])
            ),
        ),
        (
            "duplicate hotkeys",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
lock_hotkey = "Q"
talk_hotkey = "Q"
"#
            .to_string(),
        ),
        (
            "auto-lock timeout below minimum",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 5
auto_unlock_mode = "disabled"
"#
            .to_string(),
        ),
        (
            "auto-lock timeout above maximum",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 100000
auto_unlock_mode = "disabled"
"#
            .to_string(),
        ),
        (
            "backoff base interval below minimum",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "backoff"
auto_unlock_base_interval = 10
"#
            .to_string(),
        ),
        (
            "unknown auto_unlock_mode",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "sometimes"
"#
            .to_string(),
        ),
        (
            "invalid hotkey (not A-Z)",
            r#"
passphrase_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passphrase_format = "keycode-v1"
auto_lock_timeout = 120
auto_unlock_mode = "disabled"
lock_hotkey = "1"
"#
            .to_string(),
        ),
    ];

    for (name, body) in cases {
        let path = temp_config_path();
        std::fs::write(&path, &body).expect("write temp config");
        let result = Config::load_from_path(&path);
        assert!(result.is_err(), "case `{}` must be rejected", name);
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn test_strict_validation_accepts_well_formed_config() {
    let path = temp_config_path();
    let assembled = setup::assemble_config(&valid_outcome().setup).unwrap();
    assembled.save_to_path(&path).unwrap();
    assert!(
        Config::load_from_path(&path).is_ok(),
        "a fresh wizard outcome must produce a loadable config"
    );
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Pure helpers: capture edge cases (seam 2 prior art: tests/auth_tests.rs)
// Pure helpers live in src/setup.rs cfg(test) (is_rejected_keycode /
// is_unlock_blocked_keycode / validate_sequence); they are unit-tested there.
// Here we test the wizard-level edge cases through the config seam.
// ---------------------------------------------------------------------------
