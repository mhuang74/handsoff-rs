pub mod event_tap;
pub mod hotkeys;

use crate::app_state::AppState;
use crate::auth;
use crate::utils::keycode::keycode_to_char;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField};
use log::{debug, error, info};

/// Handle a keyboard event during lock
///
/// Returns true if the event should be blocked, false if it should pass through
pub fn handle_keyboard_event(event: &CGEvent, event_type: CGEventType, state: &AppState) -> bool {
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let flags = event.get_flags();

    // Check for Lock hotkey (Ctrl+Cmd+Shift+L) - keycode 37 is 'L'
    // This only LOCKS, never unlocks (unlock requires passphrase)
    if keycode == 37
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

    // Check for Talk hotkey (Ctrl+Cmd+Shift+T) - keycode 17 is 'T'
    // Track press/release state for passthrough
    if keycode == 17
        && flags.contains(CGEventFlags::CGEventFlagControl)
        && flags.contains(CGEventFlags::CGEventFlagCommand)
        && flags.contains(CGEventFlags::CGEventFlagShift)
    {
        if (event_type as u32) == (CGEventType::KeyDown as u32) {
            info!("Talk key pressed - enabling spacebar passthrough");
            state.set_talk_key_pressed(true);
        } else if (event_type as u32) == (CGEventType::KeyUp as u32) {
            info!("Talk key released - disabling spacebar passthrough");
            state.set_talk_key_pressed(false);
        }
        return true; // Block the talk hotkey itself
    }

    // Allow spacebar passthrough when talk key is pressed
    if state.is_talk_key_pressed() && keycode == 49 {
        // Keycode 49 is spacebar
        info!("Spacebar passthrough active (Talk key held)");
        return false; // Allow spacebar through
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

    // Handle backspace
    if keycode == 51 {
        // Delete key
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

/// Check accessibility permissions
pub fn check_accessibility_permissions() -> bool {
    use core_graphics::sys::CGEventTapRef;
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: unsafe extern "C" fn(
                proxy: CGEventTapRef,
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

    unsafe extern "C" fn test_callback(
        _proxy: CGEventTapRef,
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
        // First check using AXIsProcessTrusted - more reliable for permission status
        let ax_trusted = AXIsProcessTrusted();
        info!("AXIsProcessTrusted check: {}", ax_trusted);

        // Also test event tap creation as a secondary check
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

        if !ax_trusted || !tap_created {
            error!("Accessibility permission check failed:");
            error!("  - AXIsProcessTrusted: {}", ax_trusted);
            error!("  - Event tap created: {}", tap_created);
            error!("  - Bundle ID should be: com.handsoff.inputlock");
            error!("  - Please check System Settings > Privacy & Security > Accessibility");
        }

        ax_trusted && tap_created
    }
}
