# Fix Plan: PAC Crash in Event Tap Callback

## Summary
The tray app periodically crashes on macOS 15.7.x with `EXC_BREAKPOINT (SIGTRAP)` inside `__CFCheckCFInfoPACSignature`, triggered by `SLEventTapEnable -> CFMachPortGetContext`. The crash occurs when the event tap callback receives a “tap disabled” event and calls `CGEventTapEnable` with the **wrong pointer** (`CGEventTapProxy` instead of `CGEventTapRef`). On ARM64e with PAC, this becomes a hard trap.

## Symptoms
- Periodic crash of `handsoff-tray` (e.g., 2026-02-26 04:04:21 +0800) on macOS 15.7.3 (24G419).
- Crash stack includes:
  - `__CFCheckCFInfoPACSignature`
  - `CFMachPortGetContext`
  - `SLEventTapEnable`
  - `handsoff::input_blocking::event_tap::event_tap_callback`
- Exception: `EXC_BREAKPOINT` with `pointer authentication trap IA`.

## Steps to Reproduce (Likely / Best-effort)
1. Run the tray app with accessibility permissions enabled.
2. Induce event tap disablement by either:
   - Temporarily removing Accessibility permission while the app is running; or
   - Forcing a timeout by starving the event tap (e.g., heavy system load or long-running callback).
3. macOS dispatches a “tap disabled” event (`kCGEventTapDisabledByTimeout` or `kCGEventTapDisabledByUserInput`).
4. Callback path invokes `CGEventTapEnable(proxy, true)` where `proxy` is a `CGEventTapProxy`, not a `CGEventTapRef`.
5. Crash occurs in `SLEventTapEnable` due to invalid Mach port / PAC signature check.

## Root Cause
In `src/input_blocking/event_tap.rs`, the event tap callback is defined as:
```
unsafe extern "C" fn event_tap_callback(proxy: CGEventTapRef, ...)
```
The parameter type is incorrect. For a CGEvent tap callback, the first parameter is **CGEventTapProxy**, not the CGEvent tap reference returned by `CGEventTapCreate`. The code then calls:
```
CGEventTapEnable(proxy, true);
```
This passes the wrong pointer type into `CGEventTapEnable`. On ARM64e, CoreFoundation validates PAC for the Mach port; the proxy fails this check, resulting in the pointer authentication trap.

## Fix Overview
- Stop calling `CGEventTapEnable` inside the callback using `proxy`.
- Instead, signal the main thread (which owns the real `CGEventTapRef`) to restart or stop the tap.
- Add a defensive `user_info` null check before dereferencing, to avoid a potential use-after-free if callbacks arrive after teardown.

## Detailed Changes Required
### 1) Correct callback handling for disabled tap events
**File:** `src/input_blocking/event_tap.rs`
- Remove the call to `CGEventTapEnable(proxy, true)` inside the “tap disabled” branch.
- Replace with state signaling:
  - For `kCGEventTapDisabledByTimeout`: request a restart, e.g. `state.request_start_event_tap()`.
  - For `kCGEventTapDisabledByUserInput`: keep current behavior (request stop + exit), and optionally also avoid restart.

### 2) Add a null guard for `user_info`
**File:** `src/input_blocking/event_tap.rs`
- Before dereferencing `user_info`, check for null and pass the event through if null.
- This reduces risk of a late callback after state teardown.

### 3) (Optional) Clarify types and intent
**File:** `src/input_blocking/event_tap.rs`
- Update the callback signature to reflect the correct type:
  - Define `type CGEventTapProxy = *mut c_void;` (or use the core-graphics type if available)
  - Use that type for the `proxy` parameter to prevent confusion.
- Update doc comments / logs to indicate that re-enabling is done by main thread restart, not from the callback.

## Acceptance Criteria
- No further crashes in `__CFCheckCFInfoPACSignature` when taps are disabled.
- When permissions are revoked or a timeout occurs:
  - Tap is stopped cleanly without crash.
  - On permission restoration or timeout recovery, tap restarts via main thread path.
- No regressions in input blocking behavior or hotkey handling.

## Test Plan
- Manual: run tray app on macOS 15.7.x, then revoke Accessibility permission while running.
- Confirm app does not crash, and logs indicate tap stop.
- Restore permission, confirm tap restarts and input blocking functions normally.
- Optional: simulate heavy load to induce timeout and ensure no crash.
