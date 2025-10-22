use crate::app_state::AppState;
use crate::input_blocking::{handle_keyboard_event, handle_mouse_event};
use core_graphics::event::CGEventType;
use core_graphics::sys::{CGEventRef, CGEventTapRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
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
        tap: u32, // CGEventTapLocation
        place: u32, // CGEventTapPlacement
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

const kCGSessionEventTap: u32 = 1;
const kCGHeadInsertEventTap: u32 = 0;
const kCGEventTapOptionDefault: u32 = 0;

/// Create and enable the event tap for input blocking
pub fn create_event_tap(state: Arc<AppState>) -> Option<CGEventTapRef> {
    info!("Creating event tap for input blocking");

    // Event types to monitor - create event mask
    let event_mask: u64 = (1 << CGEventType::KeyDown as u64)
        | (1 << CGEventType::KeyUp as u64)
        | (1 << CGEventType::MouseMoved as u64)
        | (1 << CGEventType::LeftMouseDown as u64)
        | (1 << CGEventType::LeftMouseUp as u64)
        | (1 << CGEventType::RightMouseDown as u64)
        | (1 << CGEventType::RightMouseUp as u64)
        | (1 << CGEventType::ScrollWheel as u64);

    // Box the state so we can pass it as user_info
    let state_ptr = Box::into_raw(Box::new(state)) as *mut c_void;

    unsafe {
        let tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
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

    // Only process events when locked
    if !state.is_locked() {
        // Update input time for auto-lock even when unlocked
        state.update_input_time();
        return event; // Pass through
    }

    let cg_event = core_graphics::event::CGEvent::from_ptr(event);
    let event_type_enum = std::mem::transmute::<u32, CGEventType>(event_type);

    // Handle different event types
    let should_block = match event_type {
        t if t == CGEventType::KeyDown as u32 || t == CGEventType::KeyUp as u32 => {
            handle_keyboard_event(&cg_event, event_type_enum, state)
        }
        t if t == CGEventType::MouseMoved as u32
            || t == CGEventType::LeftMouseDown as u32
            || t == CGEventType::LeftMouseUp as u32
            || t == CGEventType::RightMouseDown as u32
            || t == CGEventType::RightMouseUp as u32
            || t == CGEventType::ScrollWheel as u32 =>
        {
            handle_mouse_event(event_type_enum, state)
        }
        _ => false, // Pass through other events
    };

    if should_block {
        std::ptr::null_mut() // Block the event
    } else {
        event // Pass through
    }
}

/// Enable the event tap
pub fn enable_event_tap(tap: CGEventTapRef) {
    use core_foundation::base::TCFType;

    unsafe {
        // CGEventTap is a CFMachPort, so we can use CFMachPortCreateRunLoopSource
        let source_ref = CFMachPortCreateRunLoopSource(
            std::ptr::null_mut(),  // use default allocator
            tap as CFMachPortRef,  // cast event tap to mach port
            0                      // order
        );

        // Convert raw pointer to CFRunLoopSource
        let source = core_foundation::runloop::CFRunLoopSource::wrap_under_create_rule(
            source_ref as core_foundation::runloop::CFRunLoopSourceRef
        );
        CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    info!("Event tap enabled");
}

/// Disable the event tap
pub fn disable_event_tap(tap: CGEventTapRef) {
    unsafe {
        CGEventTapEnable(tap, false);
    }
    info!("Event tap disabled");
}
