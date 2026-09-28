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

/// A key recorded during setup capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedKey {
    /// Raw macOS virtual keycode
    pub keycode: u32,
    /// Human-readable rendering for confirmation (US-QWERTY display only)
    pub display: char,
}

/// Reasons a captured key may be rejected as a passphrase member (§3).
#[derive(Debug, PartialEq, Eq)]
pub enum RejectedKey {
    Escape,
    Backspace,
    Hotkey,
}

/// Check whether a keycode is allowed as a passphrase member.
///
/// Escape and Backspace are reserved for buffer control while locked. A
/// passphrase key equal to a hotkey's last key is rejected: with Ctrl+Cmd+Shift
/// held it would trigger the hotkey instead of entering the passphrase, and
/// bare it would be ambiguous.
pub fn reject_reason(
    keycode: i64,
    lock_hotkey_keycode: i64,
    talk_hotkey_keycode: i64,
) -> Option<RejectedKey> {
    if keycode == ESCAPE_KEYCODE {
        return Some(RejectedKey::Escape);
    }
    if keycode == BACKSPACE_KEYCODE {
        return Some(RejectedKey::Backspace);
    }
    if keycode == lock_hotkey_keycode || keycode == talk_hotkey_keycode {
        return Some(RejectedKey::Hotkey);
    }
    None
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
    println!("  Enter        commit");
    println!("  Backspace    delete last key");
    println!("  Escape       restart capture from empty");
    println!(
        "Reserved keys (Escape, Backspace, lock/talk hotkey keys) cannot be passphrase members.\n"
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
    const ENTER_KEYCODE: i64 = 36;
    const ENTER_KEYCODE_KEYPAD: i64 = 76;

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
            st.done = true;
            println!();
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

        // Reserved keys (hotkey members) are rejected with feedback; other
        // unrenderable keys (function/arrow/etc) are silently ignored.
        match reject_reason(
            keycode,
            LOCK_HOTKEY_KEYCODE.load(std::sync::atomic::Ordering::Relaxed),
            TALK_HOTKEY_KEYCODE.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            Some(RejectedKey::Hotkey) => {
                drop(st);
                print!(" [reserved] ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                return std::ptr::null_mut();
            }
            Some(_) => return std::ptr::null_mut(),
            None => {}
        }

        let Some(display) = crate::utils::keycode::keycode_to_char(keycode, false) else {
            return std::ptr::null_mut();
        };

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
