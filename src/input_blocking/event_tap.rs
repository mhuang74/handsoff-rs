use crate::app_state::AppState;
use crate::input_blocking::{handle_keyboard_event, handle_mouse_event};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::CGEventType;
use core_graphics::sys::{CGEventRef, CGEventTapRef};
use foreign_types::ForeignType;
use log::{error, info};
use std::ffi::c_void;
use std::sync::Arc;

// Type alias for CFRunLoopSourceRef
type CFRunLoopSourceRef = *mut c_void;
type CFAllocatorRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFIndex = i64;

// Raw FFI bindings for private functions
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,     // CGEventTapLocation
        place: u32,   // CGEventTapPlacement
        options: u32, // CGEventTapOptions
        events_of_interest: u64,
        callback: unsafe extern "C" fn(
            proxy: CGEventTapRef,
            event_type: u32,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventTapRef;

    fn CGEventTapEnable(tap: CGEventTapRef, enable: bool);
}

// CFMachPort functions from CoreFoundation
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
}

const K_CGSESSION_EVENT_TAP: u32 = 1;
const K_CGHEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CGEVENT_TAP_OPTION_DEFAULT: u32 = 0;

/// Create and enable the event tap for input blocking
pub fn create_event_tap(state: Arc<AppState>) -> Option<CGEventTapRef> {
    info!("Creating event tap for input blocking");

    // Event types to monitor - create event mask
    let event_mask: u64 = (1 << CGEventType::KeyDown as u64)
        | (1 << CGEventType::KeyUp as u64)
        | (1 << CGEventType::MouseMoved as u64)
        | (1 << CGEventType::LeftMouseDown as u64)
        | (1 << CGEventType::LeftMouseUp as u64)
        | (1 << CGEventType::LeftMouseDragged as u64)
        | (1 << CGEventType::RightMouseDown as u64)
        | (1 << CGEventType::RightMouseUp as u64)
        | (1 << CGEventType::RightMouseDragged as u64)
        | (1 << CGEventType::OtherMouseDragged as u64)
        | (1 << CGEventType::ScrollWheel as u64);

    // Box the state so we can pass it as user_info
    let state_ptr = Box::into_raw(Box::new(state)) as *mut c_void;

    unsafe {
        let tap = CGEventTapCreate(
            K_CGSESSION_EVENT_TAP,
            K_CGHEAD_INSERT_EVENT_TAP,
            K_CGEVENT_TAP_OPTION_DEFAULT,
            event_mask,
            event_tap_callback,
            state_ptr,
        );

        if tap.is_null() {
            error!("Failed to create event tap - accessibility permissions may not be granted");
            // Clean up the boxed state
            let _ = Box::from_raw(state_ptr as *mut Arc<AppState>);
            return None;
        }

        Some(tap)
    }
}

/// Callback function for the event tap
unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapRef,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    // Reconstruct the state from user_info without taking ownership
    let state = &*(user_info as *const Arc<AppState>);

    let cg_event = core_graphics::event::CGEvent::from_ptr(event);

    // Handle different event types - use safe pattern matching instead of transmute
    let should_block = match event_type {
        t if t == CGEventType::KeyDown as u32 => {
            // Always handle keyboard events (for hotkeys even when unlocked)
            handle_keyboard_event(&cg_event, CGEventType::KeyDown, state)
        }
        t if t == CGEventType::KeyUp as u32 => {
            // Always handle keyboard events (for hotkeys even when unlocked)
            handle_keyboard_event(&cg_event, CGEventType::KeyUp, state)
        }
        t if t == CGEventType::MouseMoved as u32 => {
            // Always allow mouse movement (needed for tooltips and cursor position)
            // This is a passive event and doesn't trigger any actions
            state.update_input_time();
            false // Always pass through
        }
        t if t == CGEventType::LeftMouseDown as u32 => {
            if state.is_locked() {
                handle_mouse_event(CGEventType::LeftMouseDown, state)
            } else {
                state.update_input_time();
                false
            }
        }
        t if t == CGEventType::LeftMouseUp as u32 => {
            if state.is_locked() {
                handle_mouse_event(CGEventType::LeftMouseUp, state)
            } else {
                state.update_input_time();
                false
            }
        }
        t if t == CGEventType::RightMouseDown as u32 => {
            if state.is_locked() {
                handle_mouse_event(CGEventType::RightMouseDown, state)
            } else {
                state.update_input_time();
                false
            }
        }
        t if t == CGEventType::RightMouseUp as u32 => {
            if state.is_locked() {
                handle_mouse_event(CGEventType::RightMouseUp, state)
            } else {
                state.update_input_time();
                false
            }
        }
        t if t == CGEventType::ScrollWheel as u32 => {
            if state.is_locked() {
                handle_mouse_event(CGEventType::ScrollWheel, state)
            } else {
                state.update_input_time();
                false
            }
        }
        t if t == CGEventType::LeftMouseDragged as u32 => {
            // Mouse drag with left button - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                true // Block during lock
            } else {
                false // Pass through when unlocked
            }
        }
        t if t == CGEventType::RightMouseDragged as u32 => {
            // Mouse drag with right button - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                true // Block during lock
            } else {
                false // Pass through when unlocked
            }
        }
        t if t == CGEventType::OtherMouseDragged as u32 => {
            // Mouse drag with other button (middle/wheel) - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                true // Block during lock
            } else {
                false // Pass through when unlocked
            }
        }
        _ => false, // Pass through other events
    };

    // CRITICAL: Prevent cg_event from being dropped/freed since we're returning the same pointer!
    // The event is owned by the system, not by us.
    std::mem::forget(cg_event);

    if should_block {
        std::ptr::null_mut() // Block the event
    } else {
        event // Pass through
    }
}

/// Enable the event tap and return the run loop source
///
/// # Safety
/// The `tap` parameter must be a valid CGEventTapRef pointer returned from `CGEventTapCreate`.
///
/// # Returns
/// Returns the CFRunLoopSourceRef that was added to the run loop, so it can be removed later if needed
pub unsafe fn enable_event_tap(tap: CGEventTapRef) -> CFRunLoopSourceRef {
    use core_foundation::base::TCFType;

    // CGEventTap is a CFMachPort, so we can use CFMachPortCreateRunLoopSource
    let source_ref = CFMachPortCreateRunLoopSource(
        std::ptr::null_mut(), // use default allocator
        tap as CFMachPortRef, // cast event tap to mach port
        0,                    // order
    );

    // Convert raw pointer to CFRunLoopSource
    let source = core_foundation::runloop::CFRunLoopSource::wrap_under_create_rule(
        source_ref as core_foundation::runloop::CFRunLoopSourceRef,
    );
    CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
    CGEventTapEnable(tap, true);

    info!("Event tap enabled");

    // Return the source ref so caller can store it for later removal
    source_ref
}

/// Disable the event tap
///
/// # Safety
/// The `tap` parameter must be a valid CGEventTapRef pointer returned from `CGEventTapCreate`.
#[allow(dead_code)]
pub unsafe fn disable_event_tap(tap: CGEventTapRef) {
    CGEventTapEnable(tap, false);
    info!("Event tap disabled");
}

/// Remove event tap source from run loop and disable it
///
/// # Safety
/// The `tap` and `source` parameters must be valid pointers
pub unsafe fn remove_event_tap_from_runloop(tap: CGEventTapRef, source: CFRunLoopSourceRef) {
    use core_foundation::base::TCFType;

    info!("Removing event tap from run loop");

    // Disable the tap first
    CGEventTapEnable(tap, false);

    // Convert the source ref back to CFRunLoopSource and remove it from the run loop
    let source = core_foundation::runloop::CFRunLoopSource::wrap_under_get_rule(
        source as core_foundation::runloop::CFRunLoopSourceRef,
    );
    CFRunLoop::get_current().remove_source(&source, kCFRunLoopCommonModes);

    info!("Event tap removed from run loop");
}
