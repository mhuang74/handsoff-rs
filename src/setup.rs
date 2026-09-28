//! Passphrase setup capture via a throwaway event tap.
//!
//! specs/deep-design-review-v2-2026-09.md §3 / V6:
//! - `--setup` captures the physical key-code sequence with a CGEventTap active
//!   only during setup (no new TCC permission — reuses Accessibility).
//! - Interactive only: over SSH/headless there is no physical keyboard in the
//!   session the tap would capture, so setup is refused.
//! - Rejects Escape, Backspace, and the configured lock/talk hotkey combos as
//!   passphrase members. Minimum 4 keys.

use crate::constants::{BACKSPACE_KEYCODE, DEFAULT_LOCK_KEYCODE, DEFAULT_TALK_KEYCODE};
use crate::utils::MIN_PASSPHRASE_KEYS;
use anyhow::{anyhow, Result};

const ESCAPE_KEYCODE: i64 = 53;
const ENTER_KEYCODE: i64 = 36;
const ENTER_KEYCODE_KEYPAD: i64 = 76;

/// A key recorded during setup capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedKey {
    /// Raw macOS virtual keycode
    pub keycode: u32,
    /// Human-readable rendering for confirmation. US-QWERTY characters when
    /// the map can render the key; `<key N>` for keys it cannot (F-keys,
    /// keypad, arrows) so every recorded member displays meaningfully.
    pub display: String,
}

/// The §3 rejection set: keycodes that may never join a passphrase.
///
/// Members: Escape (53), Backspace, Enter (36), keypad Enter (76), and the
/// lock/talk hotkey last keys. Membership-only — Enter/Escape/Backspace
/// remain *control keys* in both capture and unlock paths (commit / restart /
/// delete); this set governs which keys are eligible to be recorded.
pub fn is_rejected_keycode(keycode: i64, lock_hotkey_keycode: i64, talk_hotkey_keycode: i64) -> bool {
    keycode == ESCAPE_KEYCODE
        || keycode == BACKSPACE_KEYCODE
        || keycode == ENTER_KEYCODE
        || keycode == ENTER_KEYCODE_KEYPAD
        || keycode == lock_hotkey_keycode
        || keycode == talk_hotkey_keycode
}

/// Validate a completed capture: minimum key count (§3).
pub fn validate_sequence(keys: &[CapturedKey]) -> Result<()> {
    if keys.len() < MIN_PASSPHRASE_KEYS {
        return Err(anyhow!(
            "Passphrase must be at least {} keys (got {})",
            MIN_PASSPHRASE_KEYS,
            keys.len()
        ));
    }
    Ok(())
}

/// Render a captured sequence for display (e.g. `[a] [b] [c] [d]`).
/// Characters come from the US-QWERTY map and are for confirmation only —
/// the stored value is the raw keycode sequence.
pub fn display_sequence(keys: &[CapturedKey]) -> String {
    keys.iter()
        .map(|k| format!("[{}]", k.display))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Display fallback for keys the US-QWERTY map cannot render.
fn display_label(keycode: i64) -> String {
    crate::utils::keycode::keycode_to_char(keycode, false)
        .map(|c| c.to_string())
        .unwrap_or_else(|| format!("<key {}>", keycode))
}

/// Detect whether we can capture physical key events at all.
///
/// Returns Err when running non-interactively (SSH/headless): a session tap in
/// an SSH login session has no WindowServer console access and no physical
/// keyboard events to capture, so setup cannot proceed (§3, interactive-only).
pub fn check_interactive_session() -> Result<()> {
    let is_ssh = std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_TTY").is_ok();
    if is_ssh {
        return Err(anyhow!(
            "Passphrase setup requires an interactive console session.\n\
             Keycode capture needs a physical keyboard attached to the GUI login session;\n\
             it cannot run over SSH or a headless connection.\n\
             Run setup locally on the Mac: handsoff --setup"
        ));
    }
    Ok(())
}

/// Capture a passphrase keycode sequence using a temporary event tap.
///
/// Runs a nested CFRunLoop on the calling (main) thread while the tap is
/// installed. Each captured key prints a masked progress dot. Enter commits
/// (when the minimum length is met), Backspace deletes, Escape restarts from
/// empty. All captured keys are blocked from reaching the focused app.
///
/// # Errors
/// - Non-interactive session
/// - Accessibility permission missing
/// - Tap creation failed
/// - Capture abandoned (timeout)
#[cfg(target_os = "macos")]
pub fn capture_passphrase(
    lock_hotkey_keycode: i64,
    talk_hotkey_keycode: i64,
) -> Result<Vec<CapturedKey>> {
    check_interactive_session()?;

    if !crate::input_blocking::check_accessibility_permissions() {
        return Err(anyhow!(
            "Accessibility permissions are required to capture keycodes.\n\
             Grant them in System Settings > Privacy & Security > Accessibility, then re-run setup."
        ));
    }

    use parking_lot::Mutex;
    use std::sync::Arc;

    struct CaptureState {
        keys: Vec<CapturedKey>,
        done: bool,
    }

    let state = Arc::new(Mutex::new(CaptureState {
        keys: Vec::new(),
        done: false,
    }));

    println!("\nPassphrase capture");
    println!("------------------");
    println!(
        "Type your passphrase using PHYSICAL keys (at least {} keys).",
        MIN_PASSPHRASE_KEYS
    );
    println!("Layout-independent: what matters is which keys you press, not the characters.");
    println!("  Enter        commit (at least {} keys)", MIN_PASSPHRASE_KEYS);
    println!("  Backspace    delete last key");
    println!("  Escape       restart capture from empty");
    println!(
        "Reserved keys (Escape, Backspace, Enter, lock/talk hotkey keys) cannot be passphrase members.\n"
    );
    print!("Passphrase: ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    // ---- throwaway tap (mirrors event_tap.rs FFI; see R-4 for consolidation) ----
    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    use core_graphics::event::{CGEventFlags, EventField};
    use foreign_types::ForeignType;
    use std::ffi::c_void;

    type TapRef = *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: unsafe extern "C" fn(
                proxy: *mut c_void,
                event_type: u32,
                event: core_graphics::sys::CGEventRef,
                user_info: *mut c_void,
            ) -> core_graphics::sys::CGEventRef,
            user_info: *mut c_void,
        ) -> TapRef;
        fn CGEventTapEnable(tap: TapRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: *mut c_void,
            port: *mut c_void,
            order: i64,
        ) -> *mut c_void;
        fn CFRelease(cf: *const c_void);
    }

    const K_CGSESSION_EVENT_TAP: u32 = 1;
    const K_CGHEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CGEVENT_TAP_OPTION_DEFAULT: u32 = 0;
    const K_CGEVENT_KEY_DOWN: u64 = 10;

    type Shared = Arc<Mutex<CaptureState>>;

    unsafe extern "C" fn capture_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: core_graphics::sys::CGEventRef,
        user_info: *mut c_void,
    ) -> core_graphics::sys::CGEventRef {
        if user_info.is_null() || event.is_null() || event_type != K_CGEVENT_KEY_DOWN as u32 {
            return event;
        }

        let shared = &*(user_info as *const Shared);
        let cg_event = core_graphics::event::CGEvent::from_ptr(event);
        let keycode = cg_event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
        let flags = cg_event.get_flags();
        std::mem::forget(cg_event); // event is owned by the system

        // Block Ctrl+Cmd+Shift combos entirely: hotkeys must not fire mid-capture.
        if flags.contains(CGEventFlags::CGEventFlagControl)
            && flags.contains(CGEventFlags::CGEventFlagCommand)
            && flags.contains(CGEventFlags::CGEventFlagShift)
        {
            return std::ptr::null_mut();
        }

        let mut st = shared.lock();

        if st.done {
            return std::ptr::null_mut(); // swallow everything after commit
        }

        if keycode == ENTER_KEYCODE || keycode == ENTER_KEYCODE_KEYPAD {
            if st.keys.len() >= MIN_PASSPHRASE_KEYS {
                st.done = true;
                println!();
            } else {
                println!(
                    "\nNeed at least {} keys — keep typing. ({} so far)",
                    MIN_PASSPHRASE_KEYS,
                    st.keys.len()
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            return std::ptr::null_mut();
        }

        if keycode == ESCAPE_KEYCODE {
            st.keys.clear();
            print!(" [restart] ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            return std::ptr::null_mut();
        }

        if keycode == BACKSPACE_KEYCODE {
            st.keys.pop();
            print!("\x08 \x08");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            return std::ptr::null_mut();
        }

        // Hotkey last-keys are reserved: with Ctrl+Cmd+Shift held they would
        // trigger the hotkey instead of entering the passphrase, and bare
        // they would be ambiguous (§3).
        if is_rejected_keycode(
            keycode,
            LOCK_HOTKEY_KEYCODE.load(std::sync::atomic::Ordering::Relaxed),
            TALK_HOTKEY_KEYCODE.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            drop(st);
            print!(" [reserved] ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            return std::ptr::null_mut();
        }

        // Every non-rejected keycode is recorded — including keys the
        // US-QWERTY map cannot render (F-keys, keypad, arrows). The map is
        // display-only (§3) and MUST NOT gate passphrase membership.
        let display = display_label(keycode);
        st.keys.push(CapturedKey {
            keycode: keycode as u32,
            display,
        });
        print!("•");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        std::ptr::null_mut() // block captured keystrokes from reaching apps
    }

    // Hotkey keycodes for the callback (plain statics: capture is single-shot,
    // single-threaded, and the tap lives only for this function's scope).
    static LOCK_HOTKEY_KEYCODE: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(DEFAULT_LOCK_KEYCODE);
    static TALK_HOTKEY_KEYCODE: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(DEFAULT_TALK_KEYCODE);
    LOCK_HOTKEY_KEYCODE.store(lock_hotkey_keycode, std::sync::atomic::Ordering::Relaxed);
    TALK_HOTKEY_KEYCODE.store(talk_hotkey_keycode, std::sync::atomic::Ordering::Relaxed);

    // Install the tap on the calling (main) run loop.
    let shared_box = Box::into_raw(Box::new(state.clone())) as *mut c_void;
    let tap: TapRef = unsafe {
        let t = CGEventTapCreate(
            K_CGSESSION_EVENT_TAP,
            K_CGHEAD_INSERT_EVENT_TAP,
            K_CGEVENT_TAP_OPTION_DEFAULT,
            1 << K_CGEVENT_KEY_DOWN,
            capture_callback,
            shared_box,
        );
        if t.is_null() {
            drop(Box::from_raw(shared_box as *mut Shared));
            return Err(anyhow!("Failed to create setup capture event tap"));
        }
        let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), t, 0);
        if source.is_null() {
            CFRelease(t);
            drop(Box::from_raw(shared_box as *mut Shared));
            return Err(anyhow!("Failed to create run loop source for capture tap"));
        }
        let src = core_foundation::runloop::CFRunLoopSource::wrap_under_create_rule(
            source as core_foundation::runloop::CFRunLoopSourceRef,
        );
        CFRunLoop::get_current().add_source(&src, kCFRunLoopDefaultMode);
        CGEventTapEnable(t, true);
        t
    };

    // Pump the run loop until Enter commits the capture.
    const CAPTURE_TIMEOUT_SECS: u64 = 300;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(CAPTURE_TIMEOUT_SECS);
    let result = loop {
        unsafe {
            CFRunLoop::run_in_mode(
                kCFRunLoopDefaultMode,
                std::time::Duration::from_millis(100),
                false,
            );
        }
        {
            let st = state.lock();
            if st.done {
                break Ok(st.keys.clone());
            }
        }
        if std::time::Instant::now() > deadline {
            break Err(anyhow!(
                "Passphrase capture timed out after {}s",
                CAPTURE_TIMEOUT_SECS
            ));
        }
    };

    // Tear down the tap before returning (always).
    unsafe {
        CGEventTapEnable(tap, false);
        // Drain in-flight callbacks before releasing (same rationale as
        // event_tap.rs EVENT_TAP_DRAIN_DELAY_MS).
        std::thread::sleep(std::time::Duration::from_millis(20));
        CFRelease(tap);
        drop(Box::from_raw(shared_box as *mut Shared));
    }

    let keys = result?;
    validate_sequence(&keys)?;
    Ok(keys)
}

#[cfg(not(target_os = "macos"))]
pub fn capture_passphrase(
    _lock_hotkey_keycode: i64,
    _talk_hotkey_keycode: i64,
) -> Result<Vec<CapturedKey>> {
    check_interactive_session()?;
    Err(anyhow!("Keycode capture is only supported on macOS"))
}

/// Everything the interactive setup flow collected.
pub struct SetupOutcome {
    /// Physical keycodes of the captured passphrase (validated: >= 4 keys)
    pub keycodes: Vec<u32>,
    /// Auto-lock timeout in seconds
    pub auto_lock: u64,
    /// Auto-unlock configuration (§2.7 enum; `0` input → `Disabled`)
    pub auto_unlock: crate::config::AutoUnlockConfig,
    /// Lock hotkey last key (None = default L)
    pub lock_key: Option<String>,
    /// Talk hotkey last key (None = default T)
    pub talk_key: Option<String>,
}

/// Prompt for a number with a default value (empty input → default).
fn prompt_number(print: &mut dyn FnMut(&str), prompt: &str, default: u64) -> Result<u64> {
    print(prompt);
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(default)
    } else {
        input
            .parse::<u64>()
            .map_err(|_| anyhow!("Invalid number: {}", input))
    }
}

/// Prompt for a hotkey (single letter A-Z); empty input → None (default).
fn prompt_hotkey(print: &mut dyn FnMut(&str), prompt: &str) -> Result<Option<String>> {
    print(prompt);
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(None) // Use default
    } else {
        crate::config_file::Config::validate_hotkey(input)?;
        Ok(Some(input.to_uppercase()))
    }
}

/// Run the full interactive setup flow (R8: shared by CLI and tray binaries).
///
/// Output goes through `print` (CLI/tray pass their own printers); input is
/// read from stdin (both binaries share the terminal). Flow: banner → hotkey
/// resolution → passphrase capture → confirm → hotkey prompts → auto-lock →
/// auto-unlock (0 = disabled, else bounded to
/// `AUTO_UNLOCK_MIN_BASE_SECONDS..=AUTO_UNLOCK_CEILING_SECONDS`; invalid
/// entries re-prompt, bailing after 3 consecutive invalid attempts).
pub fn run_interactive_setup(print: &mut dyn FnMut(&str)) -> Result<SetupOutcome> {
    print("HandsOff Setup");
    print("==============");
    print("");

    // Resolve the currently-effective hotkey keycodes for reserved-key
    // validation (env var > config file > defaults — R3).
    print("Resolving configured hotkeys for passphrase validation…");
    let (lock_keycode, talk_keycode) = crate::config::current_hotkey_keycodes()?;

    // Capture the passphrase as a physical keycode sequence via a temporary
    // event tap (interactive console sessions only — refused over SSH).
    let keys = capture_passphrase(lock_keycode, talk_keycode)
        .map_err(|e| anyhow!("Passphrase capture failed: {}", e))?;

    // Confirm the capture
    print(&format!("Captured passphrase: {}", display_sequence(&keys)));
    print!("Confirm this passphrase? [Y/n]: ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let mut confirm = String::new();
    std::io::stdin().read_line(&mut confirm)?;
    let confirm = confirm.trim().to_lowercase();
    if !confirm.is_empty() && confirm != "y" && confirm != "yes" {
        anyhow::bail!("Setup cancelled. Re-run setup to try again.");
    }

    // Prompt for hotkeys
    print("");
    print("Hotkey Configuration");
    print("--------------------");
    print("Configure the hotkeys (modifiers Cmd+Ctrl+Shift are mandatory, but choose the last key).");
    print("Enter a single letter A-Z, or press Enter to use the default.");
    print("");

    let lock_key = prompt_hotkey(print, "Lock hotkey (default: L): ")?;
    let talk_key = prompt_hotkey(print, "Talk hotkey (Hotkey to Unmute, default: T): ")?;

    // Validate that lock and talk keys are different
    if let (Some(lock), Some(talk)) = (&lock_key, &talk_key) {
        if lock == talk {
            anyhow::bail!("Error: Lock and Talk hotkeys must be different");
        }
    }

    // Prompt for timeouts
    print("");
    print("Timeout Configuration");
    print("---------------------");
    print("");
    let auto_lock = prompt_number(print, "Auto-lock timeout in seconds (default: 120): ", 120)?;

    print("Auto-unlock backoff is enabled by default:");
    print(&format!(
        "  first unlock window at {} min of awake time after lock,",
        crate::app_state::AUTO_UNLOCK_BASE_SECONDS / 60
    ));
    print("  then doubling (2 h, 4 h, 8 h…) capped at 24 h.");
    print("  Only a successful passphrase unlock resets the schedule.");

    let mut invalid_attempts = 0u32;
    let auto_unlock = loop {
        let input = prompt_number(
            print,
            &format!(
                "Auto-unlock base interval in seconds (0=disabled, default: {}): ",
                crate::app_state::AUTO_UNLOCK_BASE_SECONDS
            ),
            crate::app_state::AUTO_UNLOCK_BASE_SECONDS,
        )?;

        if input == 0 {
            print("Auto-unlock disabled.");
            break crate::config::AutoUnlockConfig::Disabled;
        }

        if (crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS
            ..=crate::app_state::AUTO_UNLOCK_CEILING_SECONDS)
            .contains(&input)
        {
            break crate::config::AutoUnlockConfig::Backoff {
                base_interval_secs: std::num::NonZeroU64::new(input)
                    .expect("interval validated above minimum"),
            };
        }

        print(&format!(
            "Error: Auto-unlock base interval must be {}-{} seconds (or 0 to disable)",
            crate::config::AUTO_UNLOCK_MIN_BASE_SECONDS,
            crate::app_state::AUTO_UNLOCK_CEILING_SECONDS
        ));
        invalid_attempts += 1;
        if invalid_attempts >= 3 {
            anyhow::bail!("Too many invalid entries. Re-run setup to try again.");
        }
    };

    Ok(SetupOutcome {
        keycodes: keys.iter().map(|k| k.keycode).collect(),
        auto_lock,
        auto_unlock,
        lock_key,
        talk_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const F5_KEYCODE: i64 = 122; // F5: not renderable, NOT rejected
    const KEYPAD_1_KEYCODE: i64 = 83; // keypad digit: valid member
    const ARROW_UP_KEYCODE: i64 = 126; // arrow: valid member

    #[test]
    fn test_is_rejected_keycode_control_keys() {
        // Escape(53), Backspace(constant), Enter(36), keypad Enter(76) are
        // all in the §3 rejection set regardless of hotkeys.
        for keycode in [
            ESCAPE_KEYCODE,
            BACKSPACE_KEYCODE,
            ENTER_KEYCODE,
            ENTER_KEYCODE_KEYPAD,
        ] {
            assert!(
                is_rejected_keycode(keycode, DEFAULT_LOCK_KEYCODE, DEFAULT_TALK_KEYCODE),
                "keycode {} must be rejected",
                keycode
            );
        }
    }

    #[test]
    fn test_is_rejected_keycode_hotkeys() {
        // The hotkey last-keys themselves are rejected; other keys are not.
        assert!(is_rejected_keycode(37, 37, 17)); // lock hotkey L
        assert!(is_rejected_keycode(17, 37, 17)); // talk hotkey T
        assert!(!is_rejected_keycode(37, 12, 17)); // L is fine when hotkey is Q
        assert!(!is_rejected_keycode(17, 37, 12));
    }

    #[test]
    fn test_is_rejected_keycode_allows_unrenderable_keys() {
        // F-keys, keypad digits, arrows are NOT members of the rejection set
        // even though keycode_to_char cannot render them (R2 core fix).
        for keycode in [F5_KEYCODE, KEYPAD_1_KEYCODE, ARROW_UP_KEYCODE] {
            assert!(
                !is_rejected_keycode(keycode, DEFAULT_LOCK_KEYCODE, DEFAULT_TALK_KEYCODE),
                "keycode {} must be allowed as a passphrase member",
                keycode
            );
        }
    }

    #[test]
    fn test_captured_key_display_fallback() {
        // Renderable keys show their US-QWERTY character…
        assert_eq!(display_label(0), "a");
        // …unrenderable keys fall back to <key N>.
        assert_eq!(display_label(F5_KEYCODE), "<key 122>");
    }

    #[test]
    fn test_display_sequence_with_unrenderable_keys() {
        let keys = vec![
            CapturedKey { keycode: 0, display: "a".to_string() },
            CapturedKey { keycode: 122, display: "<key 122>".to_string() },
        ];
        assert_eq!(display_sequence(&keys), "[a] [<key 122>]");
    }
}
