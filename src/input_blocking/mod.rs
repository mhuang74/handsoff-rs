pub mod event_tap;
pub mod hotkeys;

use crate::app_state::AppState;
use crate::auth;
use crate::utils::keycode::keycode_to_char;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField};
use log::{debug, info};

/// Handle a keyboard event during lock
///
/// Returns true if the event should be blocked, false if it should pass through
pub fn handle_keyboard_event(
    event: &CGEvent,
    event_type: CGEventType,
    state: &AppState,
) -> bool {
    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let flags = event.get_flags();

    // Check for Talk hotkey (Ctrl+Cmd+Shift+T) - keycode 17 is 'T'
    // Track press/release state for passthrough
    if keycode == 17 &&
        flags.contains(CGEventFlags::CGEventFlagControl) &&
        flags.contains(CGEventFlags::CGEventFlagCommand) &&
        flags.contains(CGEventFlags::CGEventFlagShift)
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

    // Only process KeyDown events for passphrase entry
    // CGEventType doesn't implement PartialEq, so we compare as u32
    if (event_type as u32) != (CGEventType::KeyDown as u32) {
        return true; // Block KeyUp events too
    }

    let shift = flags.contains(CGEventFlags::CGEventFlagShift);

    // Check for Touch ID trigger (Ctrl+Cmd+Shift+U)
    if is_touchid_trigger(keycode, flags) {
        info!("Touch ID trigger detected");
        std::thread::spawn({
            let state = state.clone();
            move || {
                if let Ok(true) = auth::touchid::authenticate() {
                    info!("Touch ID authentication successful");
                    state.set_locked(false);
                    state.clear_buffer();
                    crate::ui::menubar::update_menu_bar_icon(false);
                    crate::ui::notifications::show_unlock_notification();
                }
            }
        });
        return true; // Block the event
    }

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
                info!("Passphrase verified - unlocking");
                state.set_locked(false);
                state.clear_buffer();
                crate::ui::menubar::update_menu_bar_icon(false);
                crate::ui::notifications::show_unlock_notification();
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

/// Check if the current key combination is the Touch ID trigger (Ctrl+Cmd+Shift+U)
fn is_touchid_trigger(keycode: i64, flags: CGEventFlags) -> bool {
    keycode == 32 && // U key
        flags.contains(CGEventFlags::CGEventFlagControl) &&
        flags.contains(CGEventFlags::CGEventFlagCommand) &&
        flags.contains(CGEventFlags::CGEventFlagShift)
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
        let tap = CGEventTapCreate(
            K_CGSESSION_EVENT_TAP,
            K_CGHEAD_INSERT_EVENT_TAP,
            K_CGEVENT_TAP_OPTION_DEFAULT,
            1, // Just test with one event type
            test_callback,
            std::ptr::null_mut(),
        );

        !tap.is_null()
    }
}
