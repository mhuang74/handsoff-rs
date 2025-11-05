# WindowServer Stability Issue - Investigation and Fix Plan

**Status:** Draft
**Created:** 2025-11-04
**Priority:** High
**Impact:** System-wide stability degradation after several hours

## Problem Statement

Even in "Disabled" mode (where Tabs and Run loop are removed and UI thread mainly sleeps), the macOS Desktop/UI stability decreases after several hours of operation.

### Symptoms

- **High WindowServer CPU usage** - WindowServer process consumes excessive CPU
- **System-wide sluggishness** - Affects all applications (Safari, VSCode, Finder, etc.)
- **UI responsiveness issues** - Difficulty switching tabs, entering data
- **Temporary relief** - Killing WindowServer helps for a few hours before symptoms return

### Expected Behavior

In Disabled mode, HandsOff should have minimal system impact (~0.1% CPU) with no WindowServer interaction, allowing indefinite operation without degradation.

## Investigation Findings

### What HandsOff Does

HandsOff is a macOS input blocking utility that:
- Uses `CGEventTap` API to intercept all keyboard, mouse, and trackpad events
- Allows screen to remain visible while blocking all input
- Enables unlocking via secret passphrase
- Designed for video conferencing, presentations, or leaving laptop unattended

### Disabled Mode Current Implementation

**Location:** `/src/lib.rs:185-205`

The `disable()` function currently:
1. Sets `is_disabled` flag to `true`
2. Stops the event tap and removes it from the run loop
3. Unregisters hotkeys
4. Clears input buffer

**Critical Issue:** Background threads only check the `is_disabled()` flag to skip work, but **they are never actually stopped or terminated**.

### System Interactions That Continue in Disabled Mode

#### 1. CFRunLoop Thread (PRIMARY SUSPECT)

**Location:** `/src/bin/handsoff-tray.rs:82-86`

```rust
std::thread::spawn(|| {
    info!("Starting CFRunLoop in background thread (required for event tap)");
    use core_foundation::runloop::CFRunLoop;
    CFRunLoop::run_current();
});
```

**Problem:**
- This thread runs forever and is **never stopped** when disabling
- Creates a persistent Core Foundation run loop
- Even though the event tap is removed, the run loop continues running
- Maintains a "zombie connection" with WindowServer
- WindowServer continues servicing this orphaned run loop
- Over hours, accumulates state in WindowServer leading to CPU spikes

**Impact:** HIGH - This is likely the primary cause of the WindowServer degradation

#### 2. Tao Event Loop (SECONDARY SUSPECT)

**Location:** `/src/bin/handsoff-tray.rs:133-226`

```rust
event_loop.run(move |_event, _, control_flow| {
    *control_flow = ControlFlow::WaitUntil(
        std::time::Instant::now() + std::time::Duration::from_millis(500)
    );
    // ... periodic updates every 500ms
});
```

**Problem:**
- Wakes up **every 500 milliseconds** (twice per second)
- Updates tray icon, tooltip text, and menu state
- Each wakeup requires WindowServer coordination
- No check for disabled state - runs at full frequency regardless
- 7,200 WindowServer interactions per hour
- Over hours, micro-interactions accumulate in WindowServer's state

**Impact:** MEDIUM - Contributes to WindowServer load accumulation

#### 3. Permission Monitor Thread (TERTIARY SUSPECT)

**Location:** `/src/lib.rs:396-485`

**Problem:**
- Runs every 15 seconds checking accessibility permissions
- Calls `AXIsProcessTrusted()` (Accessibility API → WindowServer communication)
- Creates and immediately destroys a test `CGEventTap` on each check
- **NO disabled state check** - runs even when HandsOff is disabled
- 240 CGEventTap create/destroy cycles per hour
- Potential resource leak in WindowServer from repeated CGEventTap lifecycle

**Impact:** MEDIUM - Repeated Core Graphics operations may leak resources

#### 4. Five Background Threads

**Location:** `/src/lib.rs:259-396`

All threads continue running in sleep/wake cycles:
- **Buffer reset thread:** Sleeps 1 sec, checks disabled flag ✓
- **Auto-lock thread:** Sleeps 5 sec, checks disabled flag ✓
- **Hotkey listener thread:** Blocks on `receiver.recv()`, checks disabled flag ✓
- **Auto-unlock thread:** Sleeps 10 sec, checks disabled flag ✓
- **Permission monitor thread:** Sleeps 15 sec, **NO disabled check** ✗

**Problem:**
- Threads are never terminated, only skip work when disabled
- Continue consuming scheduler resources
- Thread wake/sleep cycles may trigger minor system interactions

**Impact:** LOW - Minor overhead but architecturally incorrect

### Root Cause Analysis

The fundamental architectural issue is that **"Disabled mode" only pauses work but doesn't stop the threads and run loops that interact with WindowServer.**

#### Primary Root Cause: CFRunLoop Zombie Connection

1. CFRunLoop thread is created to service the CGEventTap
2. When `disable()` is called, the event tap is removed from the run loop
3. The CFRunLoop thread continues running with no sources attached
4. Core Foundation maintains connections with WindowServer for the orphaned run loop
5. WindowServer continues servicing these connections
6. Over hours, this creates accumulating state/load in WindowServer
7. Eventually manifests as high CPU usage and system-wide sluggishness

#### Contributing Factors

- **Tao event loop polling:** 500ms wakeups create periodic WindowServer rendering operations
- **Permission monitor CGEventTap cycling:** Creating/destroying event taps every 15s may leak WindowServer resources
- **Potential memory leak:** Event tap state is boxed into raw pointer (`Box::into_raw()`) but may not be properly freed
- **Accumulated micro-interactions:** Combination of all interactions over hours degrades WindowServer performance

## Proposed Solution

### Architecture Changes

Implement proper **thread lifecycle management** where Disabled mode:
1. **Terminates** all background threads (not just pauses them)
2. **Stops** the CFRunLoop thread completely
3. **Reduces** Tao event loop frequency to minimize WindowServer interaction
4. **Joins** all threads for clean resource cleanup

### Implementation Plan

#### Phase 1: CFRunLoop Thread Lifecycle (CRITICAL)

**Files:** `src/bin/handsoff-tray.rs`, `src/lib.rs`

**Changes:**
1. Store CFRunLoop thread handle in `AppState`
2. Implement shutdown signaling using `std::sync::mpsc::channel`
3. Modify CFRunLoop thread to listen for shutdown signal:
   ```rust
   let (tx, rx) = mpsc::channel();
   let handle = std::thread::spawn(move || {
       CFRunLoop::run_in_mode(
           CFRunLoopMode::Default,
           0.5, // Check every 500ms for shutdown
           false
       );
       if rx.try_recv().is_ok() {
           return; // Shutdown requested
       }
   });
   ```
4. On `enable()`: Create CFRunLoop thread
5. On `disable()`: Send shutdown signal and join thread

**Expected Impact:** Eliminates zombie WindowServer connection - should prevent long-term degradation

#### Phase 2: Optimize Tao Event Loop Polling

**File:** `src/bin/handsoff-tray.rs:133-226`

**Changes:**
1. Check `is_disabled()` state in event loop
2. Adjust polling frequency based on state:
   ```rust
   let poll_interval = if state.lock().unwrap().is_disabled() {
       Duration::from_secs(5)  // Disabled: check every 5 seconds
   } else {
       Duration::from_millis(500)  // Enabled: responsive updates
   };
   *control_flow = ControlFlow::WaitUntil(Instant::now() + poll_interval);
   ```

**Expected Impact:** Reduces WindowServer interaction by 90% when disabled (from 7,200 to 720 wakeups/hour)

#### Phase 3: Stop Permission Monitor When Disabled

**File:** `src/lib.rs:396-485`

**Changes:**
1. Add `is_disabled()` check at start of permission monitor loop:
   ```rust
   loop {
       if state.lock().unwrap().is_disabled() {
           std::thread::sleep(Duration::from_secs(15));
           continue; // Skip permission checking when disabled
       }
       // ... existing permission check logic
   }
   ```

**Expected Impact:** Eliminates 240 CGEventTap create/destroy cycles per hour when disabled

#### Phase 4: Proper Background Thread Shutdown

**File:** `src/lib.rs` (start_background_threads function)

**Changes:**
1. Create shutdown channels for all 5 background threads
2. Store thread handles in `AppState`
3. Replace infinite loops with shutdown-aware loops:
   ```rust
   loop {
       match rx.recv_timeout(Duration::from_secs(interval)) {
           Err(RecvTimeoutError::Timeout) => {
               // Do periodic work
           }
           Ok(_) | Err(RecvTimeoutError::Disconnected) => {
               break; // Shutdown requested
           }
       }
   }
   ```
4. On `disable()`: Send shutdown signals and join all threads
5. On `enable()`: Restart background threads

**Expected Impact:** Clean resource cleanup, proper thread termination, no lingering system interactions

#### Phase 5: Review Event Tap Memory Management

**File:** `src/input_blocking/event_tap.rs`

**Changes:**
1. Review `stop_event_tap()` function (lines 245-260)
2. Verify boxed state pointer created at line 69 is properly freed:
   ```rust
   // In start_event_tap (line 69):
   let state_ptr = Box::into_raw(Box::new(state));

   // In stop_event_tap - ensure we properly drop:
   if !state_ptr.is_null() {
       unsafe { Box::from_raw(state_ptr) }; // Explicitly drop
   }
   ```
3. Add explicit cleanup if needed

**Expected Impact:** Prevents potential memory leaks in event tap lifecycle

### Testing Plan

#### Unit Tests
- Test enable/disable cycles don't leak threads
- Verify all threads terminate on disable
- Confirm CFRunLoop properly stops

#### Integration Tests
1. **Short-term test (1 hour):**
   - Enable HandsOff → Disable → Monitor WindowServer CPU
   - Verify CPU remains low (<1%)
   - Check no orphaned threads exist

2. **Long-term stability test (8+ hours):**
   - Leave HandsOff in Disabled mode overnight
   - Monitor WindowServer CPU usage every 30 minutes
   - Verify no degradation over time
   - Test system responsiveness remains normal

3. **Cycle stress test:**
   - Rapidly enable/disable HandsOff 100 times
   - Monitor for thread leaks, memory leaks
   - Verify WindowServer remains stable

#### Success Metrics
- WindowServer CPU usage <1% after 8+ hours in Disabled mode
- No system-wide sluggishness or responsiveness issues
- No orphaned threads after disable
- System remains stable indefinitely

## Implementation Complexity

**Estimated Effort:** Medium (2-3 days)

### Challenges
1. **CFRunLoop lifecycle management** - Core Foundation APIs require careful handling
2. **Thread synchronization** - Ensuring clean shutdown without race conditions
3. **State management** - Storing thread handles and shutdown channels in AppState
4. **Testing difficulty** - Long-term stability issues only manifest after hours

### Risks
- **Regression risk:** Changes to core threading model could introduce new bugs
- **Platform-specific behavior:** Core Foundation APIs may behave differently across macOS versions
- **Testing coverage:** Difficult to reproduce exact conditions that cause WindowServer issues

### Mitigation
- Thorough code review of thread lifecycle changes
- Extensive testing on multiple macOS versions
- Gradual rollout with monitoring
- Fallback mechanism to previous behavior if issues detected

## Alternative Workarounds

### Quick Fixes (If Full Solution Delayed)

1. **Periodic restart prompt:**
   - Detect how long HandsOff has been running in Disabled mode
   - Prompt user to restart app after 4-6 hours
   - Workaround, not a real fix

2. **Aggressive sleep in CFRunLoop:**
   - Modify CFRunLoop to check disabled state
   - Use very long sleep intervals (30+ seconds) when disabled
   - Reduces but doesn't eliminate WindowServer interaction

3. **Disable permission monitoring:**
   - Simple flag to skip permission checks when disabled
   - Quick win to reduce CGEventTap cycling
   - Implement first while working on proper solution

## References

### Related Code Locations
- Main app state: `/src/lib.rs`
- Tray application: `/src/bin/handsoff-tray.rs`
- Event tap implementation: `/src/input_blocking/event_tap.rs`
- Disable implementation: `/src/lib.rs:185-205`
- Background threads: `/src/lib.rs:259-396`

### Git History
- Disable feature added: Commit 7fa55f2 (Nov 1, 2025)
- Recent fix: Commit 400bf6e "Fix reset after disable" (Nov 3, 2025)
  - Only fixed hotkey manager instantiation
  - Did not address underlying thread lifecycle issues

### macOS APIs Referenced
- `CGEventTap` - Core Graphics event tap for input interception
- `CFRunLoop` - Core Foundation run loop for event processing
- `AXIsProcessTrusted()` - Accessibility API for permission checking
- Tao - Cross-platform windowing library (Rust)

## Next Steps

1. Review and approve this specification
2. Implement Phase 1 (CFRunLoop lifecycle) as highest priority
3. Implement Phase 3 (stop permission monitor) as quick win
4. Implement Phase 2 (Tao event loop optimization)
5. Implement Phase 4 (background thread shutdown)
6. Review Phase 5 (memory management)
7. Execute comprehensive testing plan
8. Monitor production usage for stability improvements

## Questions for Review

- Are there any use cases where background monitoring should continue in Disabled mode?
- Should we add telemetry to track WindowServer CPU correlation with HandsOff uptime?
- Is 5-second UI polling acceptable in Disabled mode, or should it be configurable?
- Should we add a "Deep Sleep" mode that's even more aggressive than Disabled?
