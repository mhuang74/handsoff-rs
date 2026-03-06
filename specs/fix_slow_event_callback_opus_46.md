# Fix: Desktop Stutter from Event Tap Timeout

## Context

Despite the v0.6.7 fix for zombie Mach port accumulation during sleep/wake, the Desktop Stutter still occurs after 40-60 minutes of normal use. The event tap callback gets disabled by macOS ("callback was too slow") even without sleep/wake events. The stutter comes and goes in waves, self-resolving after a few minutes before recurring.

**Root cause**: The permission monitor thread calls `check_accessibility_permissions()` every 15 seconds, which creates a brand-new `CGEventTapCreate()` test tap and immediately `CFRelease()`s it. Over 40-60 minutes, that's 160-240 WindowServer interactions (Mach port create/destroy cycles). This gradually degrades WindowServer's ability to service the real event tap callback within its timeout window. The log timestamps confirm: permission checks appear ~10 seconds before each timeout.

## Plan

### Phase 1: Primary Fix + Telemetry

#### 1. Lightweight permission check (eliminates root cause)

**File: `src/input_blocking/mod.rs`**
- Add `check_accessibility_permissions_lightweight()` that only calls `AXIsProcessTrusted()` — no test tap creation, no WindowServer interaction
- Keep existing `check_accessibility_permissions()` for one-time startup validation

**File: `src/lib.rs`**
- Line 641: Change periodic monitor to call `check_accessibility_permissions_lightweight()` instead of `check_accessibility_permissions()`
- Line 605 (startup): Keep using full `check_accessibility_permissions()` — runs only once

**Why this is safe**: The real event tap callback already detects permission revocation via `DISABLED_BY_USER_INPUT` (0xFFFFFFFF) at `event_tap.rs:184-188`. The periodic check is a secondary safety net. `AXIsProcessTrusted()` reliably detects revocation (caching issues only affect the grant direction).

#### 2. Callback timing telemetry (validates hypothesis)

**File: `src/input_blocking/event_tap.rs`**
- Add atomic counters: `CALLBACK_COUNT`, `CALLBACK_SLOW_COUNT`, `CALLBACK_MAX_DURATION_US`
- Wrap callback body with `Instant::now()` timing (~40ns overhead, negligible)
- Log slow callbacks only when exceeding threshold (500us)

**File: `src/constants.rs`**
- Add `CALLBACK_SLOW_THRESHOLD_US: u64 = 500`

#### 3. Periodic telemetry summary

**File: `src/lib.rs`** (permission monitor thread, every 60s)
- Log callback count, slow count, and max duration from atomics
- Reset max duration after logging

### Phase 2: Follow-up optimization (if telemetry shows mutex contention)

#### 4. Reduce mutex acquisitions in callback hot path

**File: `src/input_blocking/mod.rs`** — `handle_keyboard_event()`
- Batch-read `lock_keycode`, `talk_keycode`, `is_locked` in one mutex acquisition (currently 4 separate locks per keyboard event)

## Files to Modify

| File | Change |
|------|--------|
| `src/input_blocking/mod.rs` | Add `check_accessibility_permissions_lightweight()` |
| `src/lib.rs:641` | Switch periodic monitor to lightweight check |
| `src/input_blocking/event_tap.rs` | Add callback timing telemetry (atomics) |
| `src/constants.rs` | Add `CALLBACK_SLOW_THRESHOLD_US` |

## Verification

1. `cargo build` — ensure compilation
2. `cargo test` — run existing tests
3. Run the tray app for 1+ hours of normal use
4. Confirm: no more "timeout (callback was too slow)" warnings in logs during normal operation
5. Check telemetry logs: callback durations should be well under 500us
6. Test permission revocation: remove accessibility permission while app is running, confirm it's detected via `AXIsProcessTrusted()` and/or the callback's `DISABLED_BY_USER_INPUT`
