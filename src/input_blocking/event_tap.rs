use crate::app_state::AppState;
use crate::constants::{
    EVENT_TAP_METRICS_LOG_EVERY_EVENTS, EVENT_TAP_SLOW_CALLBACK_THRESHOLD_MS,
    EVENT_TAP_SLOW_LOG_COOLDOWN_MS,
};
use crate::input_blocking::{handle_keyboard_event, handle_mouse_event};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::CGEventType;
use core_graphics::sys::{CGEventRef, CGEventTapRef};
use foreign_types::ForeignType;
use log::{error, info, warn};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Counts total CGEventTap handles created since process start.
/// Compared with TAPS_DESTROYED to detect accumulation across sleep/wake cycles.
pub static TAPS_CREATED: AtomicU32 = AtomicU32::new(0);
/// Counts total CGEventTap handles released since process start.
pub static TAPS_DESTROYED: AtomicU32 = AtomicU32::new(0);
/// Ensures we only log once when lsof telemetry is disabled.
static LSOF_DISABLED_LOGGED: AtomicBool = AtomicBool::new(false);
/// Callback telemetry counters.
static CALLBACK_EVENTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static CALLBACK_TOTAL_DURATION_NS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_MAX_DURATION_NS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_SLOW_EVENTS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_BUCKET_LT1MS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_BUCKET_1_TO_5MS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_BUCKET_5_TO_10MS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_BUCKET_GE10MS: AtomicU64 = AtomicU64::new(0);
static LAST_SLOW_CALLBACK_LOG_EPOCH_MS: AtomicU64 = AtomicU64::new(0);

/// Log the current process Mach port count via lsof (telemetry only — not in hot path).
/// Returns None if lsof is unavailable or parsing fails.
pub fn log_mach_port_count(context: &str) {
    if std::env::var_os("HANDSOFF_ENABLE_LSOF_TELEMETRY").is_none() {
        if !LSOF_DISABLED_LOGGED.swap(true, Ordering::Relaxed) {
            info!(
                "[telemetry] lsof Mach-port telemetry disabled by default; set HANDSOFF_ENABLE_LSOF_TELEMETRY=1 to enable"
            );
        }
        return;
    }

    let pid = std::process::id();
    let started = Instant::now();
    match std::process::Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mach_count = stdout.lines().filter(|l| l.contains("MACH")).count();
            let created = TAPS_CREATED.load(Ordering::Relaxed);
            let destroyed = TAPS_DESTROYED.load(Ordering::Relaxed);
            let elapsed_ms = started.elapsed().as_millis();
            info!(
                "[telemetry] {} — Mach ports (lsof): {}, taps created: {}, taps destroyed: {}, live taps: {}, lsof latency: {} ms",
                context,
                mach_count,
                created,
                destroyed,
                created.saturating_sub(destroyed),
                elapsed_ms
            );
            if elapsed_ms > 500 {
                warn!(
                    "[telemetry] lsof call was slow ({} ms) during {}",
                    elapsed_ms, context
                );
            }
        }
        Err(e) => {
            warn!(
                "[telemetry] {} — could not run lsof for Mach port count: {}",
                context, e
            );
        }
    }
}

fn event_type_name(event_type: u32) -> &'static str {
    match event_type {
        t if t == CGEventType::KeyDown as u32 => "KeyDown",
        t if t == CGEventType::KeyUp as u32 => "KeyUp",
        t if t == CGEventType::MouseMoved as u32 => "MouseMoved",
        t if t == CGEventType::LeftMouseDown as u32 => "LeftMouseDown",
        t if t == CGEventType::LeftMouseUp as u32 => "LeftMouseUp",
        t if t == CGEventType::LeftMouseDragged as u32 => "LeftMouseDragged",
        t if t == CGEventType::RightMouseDown as u32 => "RightMouseDown",
        t if t == CGEventType::RightMouseUp as u32 => "RightMouseUp",
        t if t == CGEventType::RightMouseDragged as u32 => "RightMouseDragged",
        t if t == CGEventType::OtherMouseDragged as u32 => "OtherMouseDragged",
        t if t == CGEventType::ScrollWheel as u32 => "ScrollWheel",
        0xFFFFFFFE => "TapDisabledByTimeout",
        0xFFFFFFFF => "TapDisabledByUserInput",
        _ => "Other",
    }
}

fn maybe_update_max_duration(duration_ns: u64) {
    let mut current = CALLBACK_MAX_DURATION_NS.load(Ordering::Relaxed);
    while duration_ns > current {
        match CALLBACK_MAX_DURATION_NS.compare_exchange_weak(
            current,
            duration_ns,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn maybe_log_slow_callback(
    event_type: u32,
    branch: &str,
    elapsed_ms: u128,
    should_block: bool,
    is_locked: bool,
) {
    if elapsed_ms < EVENT_TAP_SLOW_CALLBACK_THRESHOLD_MS as u128 {
        return;
    }

    CALLBACK_SLOW_EVENTS.fetch_add(1, Ordering::Relaxed);

    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_SLOW_CALLBACK_LOG_EPOCH_MS.load(Ordering::Relaxed);
    if now_epoch_ms.saturating_sub(last) < EVENT_TAP_SLOW_LOG_COOLDOWN_MS {
        return;
    }
    LAST_SLOW_CALLBACK_LOG_EPOCH_MS.store(now_epoch_ms, Ordering::Relaxed);

    warn!(
        "[telemetry] Slow event tap callback: {} ms, event={}, branch={}, locked={}, should_block={}",
        elapsed_ms,
        event_type_name(event_type),
        branch,
        is_locked,
        should_block
    );
}

fn record_callback_metrics(
    event_type: u32,
    branch: &str,
    elapsed: std::time::Duration,
    should_block: bool,
    is_locked: bool,
) {
    let elapsed_ns = elapsed.as_nanos() as u64;
    let elapsed_ms = elapsed.as_millis();
    CALLBACK_EVENTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    CALLBACK_TOTAL_DURATION_NS.fetch_add(elapsed_ns, Ordering::Relaxed);
    maybe_update_max_duration(elapsed_ns);

    if elapsed_ms < 1 {
        CALLBACK_BUCKET_LT1MS.fetch_add(1, Ordering::Relaxed);
    } else if elapsed_ms < 5 {
        CALLBACK_BUCKET_1_TO_5MS.fetch_add(1, Ordering::Relaxed);
    } else if elapsed_ms < 10 {
        CALLBACK_BUCKET_5_TO_10MS.fetch_add(1, Ordering::Relaxed);
    } else {
        CALLBACK_BUCKET_GE10MS.fetch_add(1, Ordering::Relaxed);
    }

    maybe_log_slow_callback(event_type, branch, elapsed_ms, should_block, is_locked);

    let total = CALLBACK_EVENTS_TOTAL.load(Ordering::Relaxed);
    if total % EVENT_TAP_METRICS_LOG_EVERY_EVENTS == 0 {
        let total_ns = CALLBACK_TOTAL_DURATION_NS.load(Ordering::Relaxed);
        let avg_us = if total > 0 {
            total_ns / total / 1000
        } else {
            0
        };
        let max_ms = CALLBACK_MAX_DURATION_NS.load(Ordering::Relaxed) / 1_000_000;
        let slow = CALLBACK_SLOW_EVENTS.load(Ordering::Relaxed);
        let b0 = CALLBACK_BUCKET_LT1MS.load(Ordering::Relaxed);
        let b1 = CALLBACK_BUCKET_1_TO_5MS.load(Ordering::Relaxed);
        let b2 = CALLBACK_BUCKET_5_TO_10MS.load(Ordering::Relaxed);
        let b3 = CALLBACK_BUCKET_GE10MS.load(Ordering::Relaxed);
        info!(
            "[telemetry] Event tap callback metrics: total={}, avg={} us, max={} ms, slow(>={}ms)={}, buckets(<1ms/1-5ms/5-10ms/>=10ms)={}/{}/{}/{}",
            total,
            avg_us,
            max_ms,
            EVENT_TAP_SLOW_CALLBACK_THRESHOLD_MS,
            slow,
            b0,
            b1,
            b2,
            b3
        );
    }
}

// Type alias for CFRunLoopSourceRef
type CFRunLoopSourceRef = *mut c_void;
type CFAllocatorRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFIndex = i64;

// CGEventTapProxy is the first parameter to the callback - it's a different type from CGEventTapRef!
// CGEventTapProxy is `struct __CGEventTapProxy*`, while CGEventTapRef is `struct __CFMachPort*`
// Passing CGEventTapProxy to CGEventTapEnable() causes PAC failures on ARM64e
type CGEventTapProxy = *mut c_void;

// Raw FFI bindings for private functions
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,     // CGEventTapLocation
        place: u32,   // CGEventTapPlacement
        options: u32, // CGEventTapOptions
        events_of_interest: u64,
        callback: unsafe extern "C" fn(
            proxy: CGEventTapProxy, // Note: CGEventTapProxy, NOT CGEventTapRef
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

    fn CFRelease(cf: *const c_void);
}

const K_CGSESSION_EVENT_TAP: u32 = 1;
const K_CGHEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CGEVENT_TAP_OPTION_DEFAULT: u32 = 0;

/// Create and enable the event tap for input blocking
/// Returns (tap, state_ptr) tuple - caller must free state_ptr when done
pub fn create_event_tap(state: Arc<AppState>) -> Option<(CGEventTapRef, *mut c_void)> {
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

        let count = TAPS_CREATED.fetch_add(1, Ordering::Relaxed) + 1;
        info!(
            "Event tap created successfully (tap: {:?}, lifetime tap #{} created)",
            tap, count
        );
        log_mach_port_count("after create_event_tap");
        Some((tap, state_ptr))
    }
}

/// Callback function for the event tap
unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy, // Note: CGEventTapProxy, NOT CGEventTapRef - cannot use for CGEventTapEnable
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let callback_start = Instant::now();
    // Constants for special event types that indicate the tap has been disabled
    const K_CGEVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
    const K_CGEVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

    // Early null check - if user_info is null, pass through all events
    // This can happen if callback fires during/after teardown
    if user_info.is_null() {
        record_callback_metrics(
            event_type,
            "null-user-info",
            callback_start.elapsed(),
            false,
            false,
        );
        return event;
    }

    // Handle event tap disabled events
    // These events are sent by macOS when the tap is disabled
    if event_type == K_CGEVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT
    {
        let reason = if event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT {
            "user removed accessibility permissions"
        } else {
            "timeout (callback was too slow)"
        };

        log::warn!(
            "Event tap disabled by macOS (0x{:X}): {}",
            event_type,
            reason
        );

        // IMPORTANT: Do NOT call CGEventTapEnable(proxy, ...) here!
        // The proxy parameter is CGEventTapProxy, NOT CGEventTapRef.
        // These are different types: CGEventTapProxy is `struct __CGEventTapProxy*`
        // while CGEventTapRef is `struct __CFMachPort*`.
        // On ARM64e (Apple Silicon), PAC validates pointer type context,
        // so passing the wrong type causes a pointer authentication failure (crash).
        // Additionally, the tap may have been freed if teardown is in progress.
        //
        // Instead, signal the main thread to handle tap restart/stop.

        let state = &*(user_info as *const Arc<AppState>);
        let is_locked = state.is_locked();

        if event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT {
            // Permissions revoked - request full stop (tap must be recreated after permissions restored)
            state.request_stop_event_tap();
            state.request_exit(); // Request CLI to exit (ignored by tray app)
            log::warn!("Requested event tap stop and CLI exit due to permission loss");
        } else {
            // Timeout — most commonly triggered by sleep/wake. The tap is still valid;
            // re-enabling it reuses the existing WindowServer connection rather than
            // creating a new one. This avoids zombie Mach port accumulation.
            let created = TAPS_CREATED.load(Ordering::Relaxed);
            let destroyed = TAPS_DESTROYED.load(Ordering::Relaxed);
            log::warn!(
                "[wake-proxy] Event tap disabled by timeout — likely sleep/wake. \
                Requesting re-enable of existing tap (no new WindowServer connection). \
                Lifetime taps: created={}, destroyed={}, live={}",
                created,
                destroyed,
                created.saturating_sub(destroyed)
            );
            state.request_reenable_event_tap();
        }

        // Return event unmodified (these are system events)
        let branch = if event_type == K_CGEVENT_TAP_DISABLED_BY_USER_INPUT {
            "tap-disabled-user"
        } else {
            "tap-disabled-timeout"
        };
        record_callback_metrics(
            event_type,
            branch,
            callback_start.elapsed(),
            false,
            is_locked,
        );
        return event;
    }

    // Reconstruct the state from user_info without taking ownership
    let state = &*(user_info as *const Arc<AppState>);

    let cg_event = core_graphics::event::CGEvent::from_ptr(event);

    // Handle different event types - use safe pattern matching instead of transmute
    let (should_block, branch): (bool, &'static str) = match event_type {
        t if t == CGEventType::KeyDown as u32 => {
            // Always handle keyboard events (for hotkeys even when unlocked)
            (
                handle_keyboard_event(&cg_event, CGEventType::KeyDown, state),
                "keyboard-keydown",
            )
        }
        t if t == CGEventType::KeyUp as u32 => {
            // Always handle keyboard events (for hotkeys even when unlocked)
            (
                handle_keyboard_event(&cg_event, CGEventType::KeyUp, state),
                "keyboard-keyup",
            )
        }
        t if t == CGEventType::MouseMoved as u32 => {
            // Always allow mouse movement (needed for tooltips and cursor position)
            // This is a passive event and doesn't trigger any actions
            state.update_input_time();
            (false, "mouse-move") // Always pass through
        }
        t if t == CGEventType::LeftMouseDown as u32 => {
            if state.is_locked() {
                (
                    handle_mouse_event(CGEventType::LeftMouseDown, state),
                    "mouse-left-down-locked",
                )
            } else {
                state.update_input_time();
                (false, "mouse-left-down-unlocked")
            }
        }
        t if t == CGEventType::LeftMouseUp as u32 => {
            if state.is_locked() {
                (
                    handle_mouse_event(CGEventType::LeftMouseUp, state),
                    "mouse-left-up-locked",
                )
            } else {
                state.update_input_time();
                (false, "mouse-left-up-unlocked")
            }
        }
        t if t == CGEventType::RightMouseDown as u32 => {
            if state.is_locked() {
                (
                    handle_mouse_event(CGEventType::RightMouseDown, state),
                    "mouse-right-down-locked",
                )
            } else {
                state.update_input_time();
                (false, "mouse-right-down-unlocked")
            }
        }
        t if t == CGEventType::RightMouseUp as u32 => {
            if state.is_locked() {
                (
                    handle_mouse_event(CGEventType::RightMouseUp, state),
                    "mouse-right-up-locked",
                )
            } else {
                state.update_input_time();
                (false, "mouse-right-up-unlocked")
            }
        }
        t if t == CGEventType::ScrollWheel as u32 => {
            if state.is_locked() {
                (
                    handle_mouse_event(CGEventType::ScrollWheel, state),
                    "scroll-locked",
                )
            } else {
                state.update_input_time();
                (false, "scroll-unlocked")
            }
        }
        t if t == CGEventType::LeftMouseDragged as u32 => {
            // Mouse drag with left button - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                (true, "mouse-left-drag-locked") // Block during lock
            } else {
                (false, "mouse-left-drag-unlocked") // Pass through when unlocked
            }
        }
        t if t == CGEventType::RightMouseDragged as u32 => {
            // Mouse drag with right button - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                (true, "mouse-right-drag-locked") // Block during lock
            } else {
                (false, "mouse-right-drag-unlocked") // Pass through when unlocked
            }
        }
        t if t == CGEventType::OtherMouseDragged as u32 => {
            // Mouse drag with other button (middle/wheel) - reset auto-lock timer
            state.update_input_time();
            if state.is_locked() {
                (true, "mouse-other-drag-locked") // Block during lock
            } else {
                (false, "mouse-other-drag-unlocked") // Pass through when unlocked
            }
        }
        _ => (false, "other"), // Pass through other events
    };

    // CRITICAL: Prevent cg_event from being dropped/freed since we're returning the same pointer!
    // The event is owned by the system, not by us.
    std::mem::forget(cg_event);

    let result = if should_block {
        std::ptr::null_mut() // Block the event
    } else {
        event // Pass through
    };

    record_callback_metrics(
        event_type,
        branch,
        callback_start.elapsed(),
        should_block,
        state.is_locked(),
    );
    result
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

/// Re-enable an event tap that was disabled by macOS (e.g. after sleep/wake timeout).
/// Reuses the existing WindowServer connection — no new Mach port is created.
///
/// # Safety
/// The `tap` parameter must be a valid CGEventTapRef that was previously created and
/// not yet released via `remove_event_tap_from_runloop`.
pub unsafe fn reenable_existing_tap(tap: CGEventTapRef) {
    CGEventTapEnable(tap, true);
    info!("Event tap re-enabled (existing handle reused)");
}

/// Remove event tap source from run loop and disable it
///
/// # Safety
/// The `tap` and `source` parameters must be valid pointers
pub unsafe fn remove_event_tap_from_runloop(tap: CGEventTapRef, source: CFRunLoopSourceRef) {
    use core_foundation::base::TCFType;

    info!("Removing event tap from run loop (tap: {:?})", tap);

    // Disable the tap first so no new events are delivered
    CGEventTapEnable(tap, false);

    // Brief drain delay: give the kernel time to flush any in-flight event callbacks
    // that were already queued before the disable. Without this, WindowServer may hold
    // a send right to the Mach port while we release our receive right, leaving a zombie
    // port until WindowServer drains its queue and releases its send rights.
    std::thread::sleep(std::time::Duration::from_millis(
        crate::constants::EVENT_TAP_DRAIN_DELAY_MS,
    ));

    // Convert the source ref back to CFRunLoopSource and remove it from the run loop
    let source = core_foundation::runloop::CFRunLoopSource::wrap_under_get_rule(
        source as core_foundation::runloop::CFRunLoopSourceRef,
    );
    CFRunLoop::get_current().remove_source(&source, kCFRunLoopCommonModes);

    // CRITICAL: Release the CGEventTapRef (CFMachPortRef) to prevent WindowServer resource leak
    // Without this, each sleep/wake cycle accumulates zombie tap handles causing desktop stuttering
    CFRelease(tap as *const c_void);

    let count = TAPS_DESTROYED.fetch_add(1, Ordering::Relaxed) + 1;
    info!(
        "Event tap released and removed from run loop (lifetime tap #{} destroyed)",
        count
    );
    log_mach_port_count("after remove_event_tap_from_runloop");
}
