pub mod event_tap;
pub mod hotkeys;

use crate::app_state::AppState;
use crate::auth;
use crate::constants::BACKSPACE_KEYCODE;
use crate::utils::keycode::keycode_to_char;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField};
use log::{debug, error, info};

const ESCAPE_KEYCODE: i64 = 53;

/// Handle a keyboard event during lock
///
/// Returns true if the event should be blocked, false if it should pass through
///
/// Passphrase entry is keycode-based (specs/deep-design-review-v2-2026-09.md §3):
/// the buffer holds raw macOS virtual keycodes, matched against the SHA-256
/// hash captured at setup — layout-independent, modifier-independent.
///
/// Takes a single state guard for the whole handler (C-1) instead of the
/// former ~10 per-field lock round-trips.
pub fn handle_keyboard_event(event: &CGEvent, event_type: CGEventType, state: &AppState) -> bool {
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let flags = event.get_flags();
    let is_key_down = (event_type as u32) == (CGEventType::KeyDown as u32);

    // Lock hotkey (Ctrl+Cmd+Shift+<key>): locks only, never unlocks.
    if keycode == state.get_lock_keycode()
        && flags.contains(CGEventFlags::CGEventFlagControl)
        && flags.contains(CGEventFlags::CGEventFlagCommand)
        && flags.contains(CGEventFlags::CGEventFlagShift)
    {
        if is_key_down && !state.is_locked() {
            info!("Lock hotkey pressed - locking input");
            state.set_locked(true);
        } else if is_key_down {
            info!("Lock hotkey pressed but already locked (use passphrase to unlock)");
        }
        return true; // Block the hotkey itself
    }

    // Talk hotkey (Ctrl+Cmd+Shift+<key>): transform to spacebar while locked.
    // When unlocked there is no tap, so no transformation can leak (U-5).
    if keycode == state.get_talk_keycode()
        && flags.contains(CGEventFlags::CGEventFlagControl)
        && flags.contains(CGEventFlags::CGEventFlagCommand)
        && flags.contains(CGEventFlags::CGEventFlagShift)
    {
        const SPACEBAR_KEYCODE: i64 = 49;

        if is_key_down {
            info!("Talk hotkey pressed - transforming to spacebar");
            state.set_talk_key_pressed(true);
        } else {
            info!("Talk hotkey released - transforming to spacebar");
            state.set_talk_key_pressed(false);
        }

        // Transform the event: change keycode to spacebar and remove modifier flags
        event.set_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE, SPACEBAR_KEYCODE);
        event.set_flags(CGEventFlags::CGEventFlagNull);

        return false; // Allow the transformed event to pass through
    }

    // Take one guard for everything below (C-1)
    let mut state_guard = state.lock();

    if !state_guard.is_locked {
        state_guard.last_input_time = std::time::Instant::now();
        return false; // Pass through
    }

    // Locked: block everything and handle passphrase entry.
    // Only KeyDown events contribute to the buffer.
    if !is_key_down {
        return true; // Block KeyUp events too
    }

    // Any locked keyboard input counts as input for the auto-unlock window's
    // idle clock (§2.2: "any input resets the idle timer and extends it") —
    // not just mouse events.
    state_guard.last_input_time = std::time::Instant::now();

    // Escape immediately clears the buffer
    if keycode == ESCAPE_KEYCODE {
        state_guard.input_buffer.clear();
        debug!("Buffer cleared via Escape key");
        return true;
    }

    // Backspace pops the last keycode
    if keycode == BACKSPACE_KEYCODE {
        state_guard.input_buffer.pop();
        state_guard.last_key_time = Some(std::time::Instant::now());
        return true;
    }

    // Recordable passphrase keys: any keycode the US-QWERTY map can render.
    // Control/function/arrow keys return None and are ignored (not recorded,
    // not rejected) — they neither advance nor clear the passphrase.
    if keycode_to_char(keycode, false).is_none() {
        return true;
    }

    state_guard.input_buffer.push(keycode as u32);
    state_guard.last_key_time = Some(std::time::Instant::now());

    // S-1: never log buffer contents — length only.
    debug!("Buffer updated: {} keys", state_guard.input_buffer.len());

    // Check for a full match against the stored hash
    if let Some(hash) = state_guard.passphrase_hash.clone() {
        if auth::verify_keycodes(&state_guard.input_buffer, &hash) {
            info!("Passphrase verified - input unlocked");
            drop(state_guard);
            state.complete_passphrase_unlock();
            return true; // Block the final matching event
        }
    }

    // Block all keyboard events during lock
    true
}

/// Handle a mouse/trackpad event during lock
///
/// Returns true if the event should be blocked
pub fn handle_mouse_event(_event_type: CGEventType, state: &AppState) -> bool {
    // Update input time for auto-lock tracking
    state.update_input_time();

    // Block all mouse/trackpad events during lock
    true
}

/// Lightweight accessibility permission check using only AXIsProcessTrusted().
/// No WindowServer interaction — safe to call frequently from background threads.
///
/// This avoids the CGEventTapCreate/CFRelease cycle that, over hundreds of calls,
/// degrades WindowServer's ability to service the real event tap callback within
/// its timeout window.
///
/// Safe for revocation detection: AXIsProcessTrusted() reliably returns false
/// when permissions are removed (caching issues only affect the grant direction).
/// The real event tap callback also detects revocation via DISABLED_BY_USER_INPUT.
pub fn check_accessibility_permissions_lightweight() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    unsafe { AXIsProcessTrusted() }
}

/// Check accessibility permissions (full check with test tap creation).
/// Use only at startup or for one-time validation — NOT for periodic monitoring.
pub fn check_accessibility_permissions() -> bool {
    use core_graphics::sys::CGEventTapRef;
    use std::ffi::c_void;

    // CGEventTapProxy is the callback's first parameter - different type from CGEventTapRef
    type CGEventTapProxy = *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: unsafe extern "C" fn(
                proxy: CGEventTapProxy, // Note: CGEventTapProxy, NOT CGEventTapRef
                event_type: u32,
                event: core_graphics::sys::CGEventRef,
                user_info: *mut c_void,
            ) -> core_graphics::sys::CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventTapRef;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    unsafe extern "C" fn test_callback(
        _proxy: CGEventTapProxy,
        _event_type: u32,
        event: core_graphics::sys::CGEventRef,
        _user_info: *mut c_void,
    ) -> core_graphics::sys::CGEventRef {
        event
    }

    const K_CGSESSION_EVENT_TAP: u32 = 1;
    const K_CGHEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CGEVENT_TAP_OPTION_DEFAULT: u32 = 0;

    unsafe {
        // Check using AXIsProcessTrusted first (informational)
        let ax_trusted = AXIsProcessTrusted();
        info!("AXIsProcessTrusted check: {}", ax_trusted);

        // Test event tap creation - this is the PRIMARY and most reliable check
        // Event tap creation directly tests if we can actually intercept events
        let tap = CGEventTapCreate(
            K_CGSESSION_EVENT_TAP,
            K_CGHEAD_INSERT_EVENT_TAP,
            K_CGEVENT_TAP_OPTION_DEFAULT,
            1, // Just test with one event type
            test_callback,
            std::ptr::null_mut(),
        );

        let tap_created = !tap.is_null();
        info!("Event tap creation check: {}", tap_created);

        // Clean up test tap if it was created
        if tap_created {
            CFRelease(tap as *const c_void);
        }

        // IMPORTANT: Use event tap test as the authoritative check
        // AXIsProcessTrusted() is known to have caching issues on macOS and may
        // return false even after permissions are granted until app restart.
        // The event tap creation test is more reliable because it directly tests
        // what we need to work.
        if tap_created && !ax_trusted {
            info!("Event tap test passed but AXIsProcessTrusted returned false");
            info!("This is a known macOS caching issue - trusting event tap test");
            info!("The app should work correctly despite AXIsProcessTrusted returning false");
        }

        if !tap_created {
            error!("Accessibility permission check failed:");
            error!("  - AXIsProcessTrusted: {}", ax_trusted);
            error!("  - Event tap created: {}", tap_created);
            error!("  - Bundle ID should be: com.handsoff.inputlock");
            error!("  - Please check System Settings > Privacy & Security > Accessibility");
        }

        // Return true if event tap can be created (the actual test that matters)
        tap_created
    }
}
