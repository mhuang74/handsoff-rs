# Plan: Fix Event Tap Use-After-Free Crash

## Context

The handsoff-tray app crashes periodically with a **Pointer Authentication Code (PAC) failure** on Apple Silicon. The crash occurs in `event_tap_callback` when calling `CGEventTapEnable(proxy, true)` at line 126 after macOS sends a "tap disabled" event.

### Root Cause Analysis

**Crash sequence:**
1. macOS sends a "tap disabled by timeout" message to the event tap's mach port
2. Main thread calls `stop_event_tap()` (e.g., during disable/restart/permission loss)
3. `remove_event_tap_from_runloop()` runs: disables tap, removes source, **releases CGEventTapRef**
4. `stop_event_tap()` frees the state_ptr (user_info)
5. Control returns to run loop
6. Run loop dispatches the pending "tap disabled" callback
7. Callback calls `CGEventTapEnable(proxy, true)` on the **freed tap pointer**
8. PAC validation fails → crash

The commit 8b9a66e added `CFRelease(tap)` to fix WindowServer resource leaks, but introduced this use-after-free when a callback races with tap teardown.

## Files to Modify

- `/Users/mhuang/Projects/Development/handsoff-rs/src/input_blocking/event_tap.rs` (lines 97-144, 255-276, 292-310)

## Implementation Plan

### Approach: Use a static AtomicBool flag

Since the tap and callbacks run on the same thread (main thread), we'll use a static atomic flag in `event_tap.rs` to coordinate teardown.

### Step 1: Add static flag to event_tap.rs

```rust
use std::sync::atomic::{AtomicBool, Ordering};

/// Flag indicating the event tap is active and safe to use
/// Set to false BEFORE releasing the tap to prevent callbacks from using freed pointer
static EVENT_TAP_ACTIVE: AtomicBool = AtomicBool::new(false);
```

### Step 2: Set flag in enable_event_tap()

In `enable_event_tap()` (around line 270), set the flag to true:

```rust
CGEventTapEnable(tap, true);
EVENT_TAP_ACTIVE.store(true, Ordering::Release);  // Mark tap as active
```

### Step 3: Clear flag in remove_event_tap_from_runloop()

In `remove_event_tap_from_runloop()`, clear the flag FIRST before any cleanup:

```rust
pub unsafe fn remove_event_tap_from_runloop(tap: CGEventTapRef, source: CFRunLoopSourceRef) {
    // Mark tap as inactive FIRST to prevent callbacks from using it
    EVENT_TAP_ACTIVE.store(false, Ordering::Release);

    info!("Removing event tap from run loop (tap: {:?})", tap);
    // ... rest of function unchanged
}
```

### Step 4: Check flag in callback before re-enabling

In `event_tap_callback()`, check the flag before calling `CGEventTapEnable`:

```rust
// Around line 124-126, change:
// Try to re-enable the tap (may fail if permissions gone)
// This is a no-op if permissions were removed, but helps with timeout case
CGEventTapEnable(proxy, true);

// To:
// Only try to re-enable if the tap is still active (not being torn down)
if EVENT_TAP_ACTIVE.load(Ordering::Acquire) {
    CGEventTapEnable(proxy, true);
} else {
    log::debug!("Skipping tap re-enable - tap is being torn down");
}
```

## Why This Works

1. The flag is checked **before** calling `CGEventTapEnable`
2. The flag is cleared **before** the tap is released
3. Since both operations happen on the main thread:
   - If callback runs BEFORE stop_event_tap(): flag is true, re-enable works normally
   - If callback runs AFTER stop_event_tap() starts: flag is false, skip re-enable, no crash

## Verification

1. Build: `cargo build --release`
2. Test scenarios:
   - Rapid lock/unlock cycles
   - Sleep/wake cycles (the original crash trigger)
   - Permission revocation via System Settings
   - Disable/enable via tray menu
3. Run overnight to confirm no crashes
4. Check that timeout recovery still works (if tap is disabled by timeout while active, it should re-enable)

## Risk Assessment

- **Low risk**: The change is minimal and defensive
- **Backward compatible**: No API changes
- **Failure mode**: If flag check fails, worst case is tap doesn't re-enable after timeout (user would notice input blocking stopped working, can restart app)
