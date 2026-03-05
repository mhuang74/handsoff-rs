# Plan: Investigate and Fix Desktop Stutter (Event Tap Timeout Issue)

## Context

The user reports persistent desktop stutter that occurs intermittently after ~40-60 minutes of system use, which then self-resolves after a few minutes. Analysis of telemetry logs reveals the root cause: macOS disables the event tap with error code `0xFFFFFFFE` indicating "callback was too slow".

**Key Observation from Logs:**
```
[06:52:34] timeout → re-enabled in ~1 second
[06:57:35] timeout → re-enabling took 4 seconds (06:57:35 → 06:57:39)
[06:58:52] timeout → re-enabled immediately
[09:50:14] timeout → re-enabling took 8 seconds (09:50:14 → 09:50:22)
[09:50:23] timeout → re-enabled immediately
```

The variability in re-enable time (1s, 4s, immediate, 8s, immediate) suggests intermittent blocking - sometimes the system is responsive, sometimes it's slow.

## Problem Statement

The event tap callback acquires multiple `parking_lot::Mutex` locks per event, but currently has **NO timing telemetry** to measure how long the callback actually takes. iOS/macOS has an internal callback timeout threshold (typically ~30-60ms), and exceeding this causes the tap to be disabled.

**Critical Finding:** The callback path requires these lock acquisitions:
- Mouse move: 1 lock (`update_input_time`)
- Mouse click: 2 locks (`is_locked()`, `update_input_time()`)
- Keypress: 3-4 locks (keycode checks, `is_locked()`, buffer operations)

Four background threads also acquire this mutex at various intervals (250ms, 5s, 10s, 15s), creating potential lock contention.

## Hypothesis

The desktop stutter is caused by one or more of:

1. **Lock Contention** - A background thread holds the `AppState` mutex while the event callback tries to acquire it, causing the callback to exceed macOS's timeout threshold.

2. **Lock Starvation** - The buffer reset thread runs every 250ms and acquires the mutex. Frequent small acquisitions could starve the callback, especially if the scheduler prioritizes background threads.

3. **System Load/Priority** - Event tap callbacks run on the CFRunLoop thread. If the system is under load or thread priority is suboptimal, callbacks get delayed.

4. **Accumulating State** - Some state (buffer, timestamps, etc.) might grow over time, causing slowdown that triggers timeouts after prolonged use.

## Investigation Plan

### Phase 1: Add Comprehensive Callback Timing Telemetry

**File:** `src/input_blocking/event_tap.rs`

**Changes:**
1. Add callback duration tracking using `Instant::now()`
2. Log warnings when callback exceeds certain thresholds (1ms, 5ms, 10ms, 20ms)
3. Track specific slow paths (e.g., event type-specific timing)
4. Add counter for slow callbacks vs fast callbacks

**Implementation details:**
```rust
// Add static counters for telemetry
static SLOW_CALLBACKS_10MS: AtomicU32 = AtomicU32::new(0);
static SLOW_CALLBACKS_20MS: AtomicU32 = AtomicU32::new(0);
static TOTAL_CALLBACKS: AtomicU32 = AtomicU32::new(0);

// In callback:
let start = Instant::now();
// ... existing callback code ...
let duration = start.elapsed();
TOTAL_CALLBACKS.fetch_add(1, Ordering::Relaxed);
if duration > Duration::from_millis(20) {
    SLOW_CALLBACKS_20MS.fetch_add(1, Ordering::Relaxed);
    warn!("[callback-slow] Duration: {:?} us - event type: 0x{:X}", duration.as_micros(), event_type);
}
```

### Phase 2: Add Lock Acquisition Timing

**File:** `src/app_state.rs`

**Changes:**
1. Add instrumentation to `lock()` method to track lock acquisition times
2. Track how long the lock is held
3. Warn when lock hold time exceeds thresholds (1ms, 5ms, 10ms)
4. This helps identify if background threads are holding the lock too long

**Implementation details:**
```rust
// Add to AppState struct
static LONG_LOCK_HOLDS_5MS: AtomicU32 = AtomicU32::new(0);
// ...

// At lock scope exit:
let hold_time = guard_start.elapsed();
if hold_time > Duration::from_millis(5) {
    LONG_LOCK_HOLDS_5MS.fetch_add(1, Ordering::Relaxed);
}
```

### Phase 3: Add Slow-Path Detection for Background Threads

**File:** `src/lib.rs` - background thread functions

**Changes:**
1. Add timing to each background thread's critical sections
2. Track when a thread holds the lock for >10ms or >20ms
3. Log warnings with thread name when slow operations detected

### Phase 4: System Health Telemetry

**Add periodic diagnostics:**
1. CPU usage measurement (using `sysinfo` crate)
2. Thread count and state
3. Process memory usage
4. Run a "stress test" mode that artificially slows things down to validate telemetry works

## Decision Points Based on Findings

**If telemetry shows:**
- Callback always <5ms → Look into macOS system issues, thread priority, or WindowServer behavior
- Callback occasionally >10ms → Identify which event types are slow (mouse vs keyboard) and optimize that path
- Lock contention detected → Consider using `try_lock()` with fallback, or RwLock for read/write separation
- Background threads are slow → Optimize the slow thread, reduce check frequency, or defer work

## Implementation Approach

### Step 1: Instrument Callback Timing (event_tap.rs)

Add telemetry to measure callback duration without changing any logic:
```rust
unsafe extern "C" fn event_tap_callback(...) -> CGEventRef {
    let start = Instant::now();

    // ... existing callback logic ...

    let duration = start.elapsed();

    // Log slow callbacks
    if duration > Duration::from_millis(5) {
        log::warn!(
            "[callback-telemetry] Duration: {:?} us, event: 0x{:X}, type: {}",
            duration.as_micros(),
            event_type,
            event_type_to_name(event_type)
        );
    }

    // Return result
}
```

### Step 2: Instrument Lock Duration (app_state.rs)

Add automatic timing at lock scope boundaries:
```rust
// Create a helper struct
struct TimedLockGuard<'a, T> {
    inner: parking_lot::MutexGuard<'a, T>,
    start: Instant,
}

impl<'a, T> Drop for TimedLockGuard<'a, T> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        if duration > Duration::from_millis(5) {
            log::warn!("[lock-telemetry] Held for {:?} us", duration.as_micros());
        }
    }
}
```

### Step 3: Add Thread Identification

Make each background thread log with a recognizable prefix so we know which thread is slow:
- `[buffer-reset-thread]`
- `[auto-lock-thread]`
- `[permission-monitor-thread]`
- `[cfrunloop-thread]`

### Step 4: Run User Testing

After deploying the telemetry build:
1. User runs the app normally
2. Collect logs for 40-60 minutes until stutter occurs
3. Analyze which patterns correlate with timeout events
4. Identify the slow path

## Verification

After implementing telemetry changes:

1. **Test that telemetry works**: Run app and verify logs show callback timing data
2. **Verify overhead is minimal**: Ensure added timing doesn't itself slow the callback
3. **Wait for stutter**: User needs to run for 40+ minutes to reproduce issue
4. **Analyze logs**: Look for correlation between slow callbacks/locks and timeout events
5. **Implement fix**: Based on findings, implement targeted optimization

## Files to Modify

1. `src/input_blocking/event_tap.rs` - Add callback timing telemetry
2. `src/app_state.rs` - Add lock acquisition/hold timing
3. `src/lib.rs` - Add thread identification and slow-path logging
4. `src/constants.rs` - Add telemetry thresholds (if needed)

## Next Steps After Telemetry Analysis

Potential fixes based on findings:
- **Use try_lock()** with pass-through if lock unavailable
- **Separate hot/read-only state** into `Arc<RwLock>` for non-blocking reads
- **Reduce background thread frequency** if they cause contention
- **Adjust thread priorities** if scheduling issues detected
- **Prevent callback from doing work** when lock is contended
- **Use atomic fields** for commonly-read state (is_locked, last_input_time)

## Expected Outcome

With proper telemetry, we will be able to:
1. Identify exactly why the callback times out
2. Quantify the timeout threshold
3. Implement a targeted fix rather than guessing
4. Detect regressions in future updates
