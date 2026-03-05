pub mod event_tap;
pub mod hotkeys;

use crate::app_state::AppState;
use crate::auth;
use crate::constants::BACKSPACE_KEYCODE;
use crate::utils::keycode::keycode_to_char;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField};
use log::{debug, error, info};

/// Handle a keyboard event during lock
///
/// Returns true if the event should be blocked, false if it should pass through
pub fn handle_keyboard_event(event: &CGEvent, event_type: CGEventType, state: &AppState) -> bool {
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let flags = event.get_flags();

    // Get configured hotkey keycodes from AppState
    let lock_keycode = state.get_lock_keycode();
    let talk_keycode = state.get_talk_keycode();

    // Check for Lock hotkey (Ctrl+Cmd+Shift+<configured key>)
    // This only LOCKS, never unlocks (unlock requires passphrase)
    if keycode == lock_keycode
        && flags.contains(CGEventFlags::CGEventFlagControl)
        && flags.contains(CGEventFlags::CGEventFlagCommand)
        && flags.contains(CGEventFlags::CGEventFlagShift)
    {
        if (event_type as u32) == (CGEventType::KeyDown as u32) {
            if !state.is_locked() {
                info!("Lock hotkey pressed - locking input");
                state.set_locked(true);
            } else {
                info!("Lock hotkey pressed but already locked (use passphrase to unlock)");
            }
        }
        return true; // Block the hotkey itself
    }

    // Check for Talk hotkey (Ctrl+Cmd+Shift+<configured key>)
    // Transform it into a spacebar event by modifying the keycode and removing modifiers
    if keycode == talk_keycode
        && flags.contains(CGEventFlags::CGEventFlagControl)
        && flags.contains(CGEventFlags::CGEventFlagCommand)
        && flags.contains(CGEventFlags::CGEventFlagShift)
    {
        const SPACEBAR_KEYCODE: i64 = 49;

        if (event_type as u32) == (CGEventType::KeyDown as u32) {
            info!("Talk hotkey pressed - transforming to spacebar");
            state.set_talk_key_pressed(true);
        } else if (event_type as u32) == (CGEventType::KeyUp as u32) {
            info!("Talk hotkey released - transforming to spacebar");
            state.set_talk_key_pressed(false);
        }

        // Transform the event: change keycode to spacebar and remove modifier flags
        event.set_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE, SPACEBAR_KEYCODE);
        event.set_flags(CGEventFlags::CGEventFlagNull);

        return false; // Allow the transformed event to pass through
    }

    // If not locked, pass through all non-hotkey events
    if !state.is_locked() {
        state.update_input_time();
        return false; // Pass through
    }

    // From here on, we're locked - block events and handle passphrase entry

    // Only process KeyDown events for passphrase entry
    // CGEventType doesn't implement PartialEq, so we compare as u32
    if (event_type as u32) != (CGEventType::KeyDown as u32) {
        return true; // Block KeyUp events too
    }

    let shift = flags.contains(CGEventFlags::CGEventFlagShift);

    // Handle Escape key to immediately clear buffer
    const ESCAPE_KEYCODE: i64 = 53;
    if keycode == ESCAPE_KEYCODE {
        state.clear_buffer();
        debug!("Buffer cleared via Escape key");
        return true; // Block the escape key event
    }

    // Handle backspace
    if keycode == BACKSPACE_KEYCODE {
        let mut buffer = state.get_buffer();
        if !buffer.is_empty() {
            buffer.pop();
            state.lock().input_buffer = buffer;
        }
        state.update_key_time();
        return true; // Block the event
    }

    // Convert keycode to character
    if let Some(ch) = keycode_to_char(keycode, shift) {
        state.append_to_buffer(ch);
        state.update_key_time();

        debug!("Buffer updated: {}", state.get_buffer());

        // Check if passphrase matches
        if let Some(hash) = state.get_passphrase_hash() {
            let buffer = state.get_buffer();
            if auth::verify_passphrase(&buffer, &hash) {
                info!("Passphrase verified - input unlocked");
                state.set_locked(false);
                state.clear_buffer();
                return true; // Block the final matching event
            }
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

/// Fast accessibility permission check that avoids CGEventTap creation churn.
///
/// Uses AXIsProcessTrusted() as the primary check (no WindowServer interaction).
/// Only creates a test CGEventTap when permission transitions from false→true,
/// to validate that the restored permission actually works.
///
/// This dramatically reduces WindowServer load compared to the original
/// `check_accessibility_permissions()` which created/destroyed a test tap every call.
///
/// # Arguments
/// * `last_ax_state` - Mutable reference to track the previous AXIsProcessTrusted state.
///   Caller should initialize this to `false` and pass the same variable on each call.
///
/// # Returns
/// `true` if accessibility permissions are granted and functional, `false` otherwise.
pub fn check_accessibility_permissions_fast(last_ax_state: &mut bool) -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    let ax_trusted = unsafe { AXIsProcessTrusted() };

    // Fast path: AX still trusted, skip test tap
    if ax_trusted && *last_ax_state {
        debug!("Permission check (fast): AXIsProcessTrusted=true, skipping test tap");
        return true;
    }

    // Permission was revoked
    if !ax_trusted {
        if *last_ax_state {
            info!("Permission check: AXIsProcessTrusted changed true→false");
        }
        *last_ax_state = false;
        return false;
    }

    // Permission restored (false→true) - validate with test tap once
    info!("Permission check: AXIsProcessTrusted changed false→true, validating with test tap");
    let tap_created = check_accessibility_permissions();
    *last_ax_state = tap_created;
    if tap_created {
        info!("Permission restoration validated successfully via test tap");
    } else {
        error!("AXIsProcessTrusted=true but test tap creation failed - permission may be incomplete");
    }
    tap_created
}

/// Check accessibility permissions by creating a test CGEventTap.
///
/// WARNING: This creates and destroys a CGEventTap on every call, which causes
/// WindowServer churn. After 40-60 minutes of periodic calls (~240 cycles/hour),
/// this can cause desktop stutter and event tap timeouts.
///
/// Prefer `check_accessibility_permissions_fast()` for periodic monitoring.
/// Use this function only for:
/// - Initial permission check at startup
/// - Validating permission restoration after revocation
/// - One-time permission checks (not in a loop)
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
