# Telemetry Implementation Summary

## Completed Implementation

### Phase 1: Callback Timing Telemetry ✅

**File: `src/input_blocking/event_tap.rs`**

Implemented comprehensive callback duration tracking:

- **Static counters for tracking:**
  - `TOTAL_CALLBACKS` - Total event callbacks received
  - `SLOW_CALLBACKS_1MS` - Callbacks exceeding 1ms
  - `SLOW_CALLBACKS_5MS` - Callbacks exceeding 5ms
  - `SLOW_CALLBACKS_10MS` - Callbacks exceeding 10ms
  - `SLOW_CALLBACKS_20MS` - Callbacks exceeding 20ms
  - `SLOW_CALLBACKS_30MS` - Callbacks exceeding 30ms
  - `MAX_CALLBACK_DURATION_US` - Longest callback duration seen

- **Helper functions:**
  - `event_type_to_name()` - Converts event types to human-readable names
  - `log_callback_telemetry_summary()` - Periodically logs telemetry statistics

- **Callback instrumentation:**
  - Each callback is timed from start to finish
  - Warnings logged for callbacks exceeding 5ms, 10ms, 20ms, and 30ms thresholds
  - Event type information included in slow callback warnings

### Phase 2: Lock Timing Telemetry ✅

**File: `src/app_state.rs`**

Implemented lock acquisition and hold duration tracking:

- **Lock telemetry counters:**
  - `TOTAL_LOCK_ACQUISITIONS` - Total lock acquisitions
  - `LONG_LOCK_HOLDS_1MS` - Locks held >1ms
  - `LONG_LOCK_HOLDS_5MS` - Locks held >5ms
  - `LONG_LOCK_HOLDS_10MS` - Locks held >10ms
  - `MAX_LOCK_HOLD_US` - Longest lock hold duration seen

- **TimedLockGuard struct:**
  - Wraps `parking_lot::MutexGuard` to automatically time lock hold duration
  - Includes context string to identify where lock was acquired
  - Logs warnings when locks are held longer than thresholds

- **New AppState methods:**
  - `timed_lock(&self, context)` - Returns timed lock guard
  - `is_locked_timed()` - Uses timed lock for hot paths
  - `update_input_time_timed()` - Uses timed lock for hot paths

- **Helper function:**
  - `log_lock_telemetry_summary()` - Periodically logs lock statistics

### Phase 3: Thread Identification ✅

**File: `src/lib.rs`**

Added thread identification prefixes for all background threads:

- **Thread name prefixes:**
  - `[cfrunloop-thread]` - CFRunLoop event processing thread
  - `[buffer-reset-thread]` - Input buffer reset thread (250ms)
  - `[auto-lock-thread]` - Auto-lock checking thread (5s)
  - `[hotkey-listener-thread]` - Global hotkey listener thread
  - `[auto-unlock-thread]` - Auto-unlock checking thread (10s)
  - `[permission-monitor-thread]` - Accessibility permission monitor (15s)

- **Periodic telemetry:**
  - `increment_telemetry_seconds()` - Called periodically by background threads
  - `TELEMETRY_SECONDS` counter tracks time since program start
  - Logs telemetry summaries every 60 seconds

- **Telemetry summary function:**
  - `log_callback_telemetry_summary()` - Logs callback timing statistics
  - `log_lock_telemetry_summary()` - Logs lock timing statistics

### Phase 4: Constants ✅

**File: `src/constants.rs`**

Added telemetry configuration constants:

- `CALLBACK_SLOW_WARNING_THRESHOLD_MS` - Slow callback warning threshold
- `LOCK_SLOW_WARNING_THRESHOLD_MS` - Slow lock hold warning threshold
- `TELEMETRY_SUMMARY_INTERVAL_SECS` - Periodic telemetry summary interval

## How to Use the Telemetry

### Viewing Slow Callback Warnings

When running the app, slow callbacks will appear in logs like:

```
[callback-telemetry-slow] Duration: 5123us (>5ms) - event type: 0x10 (LeftMouseDown)
[callback-telemetry-slow] Duration: 15320us (>10ms) - event type: 0xA (KeyDown)
[callback-telemetry-slow] Duration: 32456us (>30ms) - event type: 0xB (KeyUp)
```

Each warning includes:
- Duration in microseconds
- Which threshold was exceeded
- Event type code and readable name

### Viewing Slow Lock Holds

When locks are held too long, warnings appear like:

```
[lock-telemetry-slow] Lock held for 8234us (>10ms) at: update_input_time
[lock-telemetry-slow] Lock held for 12453us (>10ms) at: is_locked
```

The context string identifies where the lock was acquired.

### Periodic Telemetry Summaries

Every 60 seconds, the system logs aggregate statistics:

```
[callback-telemetry] Total: 45231, >1ms: 234 (0.5%), >5ms: 12 (0.0%), >10ms: 3 (0.0%), >20ms: 1, >30ms: 0, max: 32456ms
[lock-telemetry] Total acqs: 8912, >1ms: 456, >5ms: 23, >10ms: 5, max: 12453ms
```

These summaries show:
- Total callbacks/lock acquisitions
- Count and percentage exceeding each threshold
- Maximum duration seen

## Next Steps for Users

1. **Build and run the telemetry-instrumented version**

2. **Use the app normally** - work, type, move the mouse, etc.

3. **Wait for the stutter to occur** - this typically happens after 40-60 minutes

4. **Collect the logs** and look for correlation between:
   - Slow callback warnings and event tap timeout events
   - Slow lock holds and stutter timing
   - Which event types trigger slow callbacks
   - Which threads are holding locks too long

5. **Provide the logs for analysis** - the team will use the telemetry data to identify the root cause and implement targeted fixes

## Expected Findings

Based on the telemetry, we may discover:

1. **Lock contention is the culprit** - A background thread holds the lock while callbacks are waiting

2. **Specific event types are slow** - Keyboard events take longer than mouse events

3. **Thread priority issues** - The CFRunLoop thread is being starved by background threads

4. **Timeout threshold is tight** - macOS disables the tap at 30ms but some callbacks legitimately take longer

## Future Optimizations

Based on telemetry findings, potential optimizations include:

- Use `try_lock()` with fallback for non-critical situations
- Separate hot read-only state into `Arc<RwLock>`
- Reduce background thread check frequency
- Adjust thread priorities
- Use atomic fields for commonly-read state
- Defer non-essential work from hot paths
