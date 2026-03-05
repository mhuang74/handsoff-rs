# Fix Plan: Slow Event Tap Callback / Desktop Stutter

## Summary
Desktop stutter still occurs after the recent WindowServer leak fix, but now it is intermittent and self-recovers in a few minutes.  
Based on telemetry and code analysis, the likely remaining issue is callback-path pressure from non-essential work around timeout recovery:

1. Repeated `kCGEventTapDisabledByTimeout` events while fully awake (not only after sleep/wake).
2. Long re-enable gaps (up to ~8-9 seconds) during timeout recovery windows.
3. Ongoing periodic creation/release of test event taps every 15 seconds from permission monitoring.
4. Synchronous `lsof` execution in tap lifecycle telemetry, which can block and amplify recovery latency.

This spec documents findings, implemented mitigation, and next-phase telemetry/fix work.

## Observed Evidence
From production logs provided by user:

- `2026-03-03T06:57:35Z`: timeout disable event received.
- `2026-03-03T06:57:44Z`: re-enable success logged.
- Gap: ~9 seconds.

- `2026-03-05T09:50:14Z`: timeout disable event received.
- `2026-03-05T09:50:22Z`: re-enable success logged.
- Gap: ~8 seconds.

- `2026-03-05T09:50:23Z`: another timeout disable occurred one second after successful re-enable.

Interpretation:
- The event tap is not permanently broken.
- Recovery now happens, but recovery latency can be long enough to cause visible stutter/input instability.
- Timeout disable events are not exclusively tied to sleep/wake.

## Current Code Paths Relevant to Issue

### 1) Timeout callback signaling
File: `src/input_blocking/event_tap.rs`

- On `kCGEventTapDisabledByTimeout`, callback requests re-enable of existing tap:
  - `state.request_reenable_event_tap()`
- This avoids creating a new WindowServer connection and fixes previous Mach port accumulation behavior.

### 2) Recovery on tray main loop
File: `src/bin/handsoff-tray.rs`

- Tray loop polls state flags and calls `core.reenable_event_tap()` when requested.

### 3) Re-enable implementation
File: `src/lib.rs`

- `reenable_event_tap()` calls `CGEventTapEnable(existing_tap, true)` and logs telemetry before/after.

### 4) Permission monitor cadence
File: `src/lib.rs`, function `start_permission_monitor_thread`

- Every `PERMISSION_CHECK_INTERVAL_SECS` (15s), code historically ran:
  - `check_accessibility_permissions()`
- That function does:
  - `AXIsProcessTrusted()`
  - `CGEventTapCreate(...)` + `CFRelease(...)` test probe

This creates ongoing event-tap churn even when permissions are healthy.

### 5) Mach-port telemetry path
File: `src/input_blocking/event_tap.rs`

- `log_mach_port_count()` historically invoked synchronous:
  - `lsof -p <pid>`
- This can be expensive and variable in latency.

## Hypotheses (Ranked)

### H1 (High confidence): Periodic full permission probe causes avoidable WindowServer load
The permission monitor creates/releases a test event tap every 15 seconds. Over 1 hour this is ~240 create/release cycles.  
Even if each cycle is "correct", this can increase pressure and contribute to intermittent callback timeouts.

### H2 (High confidence): Synchronous `lsof` telemetry adds recovery latency
`lsof` is external-process + full FD scan work on a hot lifecycle path (create/destroy/reenable telemetry checkpoints).  
Under system load, `lsof` can be multi-second, matching observed 8-9 second delays between timeout warning and re-enable success.

### H3 (Medium confidence): Genuine callback work occasionally exceeds timeout budget
If callback execution itself spikes (lock contention, passphrase verification, logging, scheduler delay), macOS can disable the tap by timeout.  
Current telemetry does not yet measure callback execution duration distribution, so this remains unproven.

## Fix Strategy

## Phase 1 (Implemented in this branch)
Goal: reduce non-essential tap churn and make expensive telemetry opt-in.

### Change A: Add lightweight permission check
File: `src/input_blocking/mod.rs`

- Added:
  - `check_accessibility_permissions_lightweight()`
- Behavior:
  - Calls only `AXIsProcessTrusted()`
  - Does not create a test event tap

### Change B: Use lightweight check during healthy steady-state
File: `src/lib.rs` (`start_permission_monitor_thread`)

- Updated permission monitor loop:
  - If last known state is `true` (permissions healthy): use lightweight check.
  - If last known state is `false` (recovery mode): run full probe `check_accessibility_permissions()`.

Rationale:
- Keep strong validation only when needed (restoration detection after revocation).
- Avoid repeated test tap churn during normal operation.

### Change C: Add full-probe latency telemetry
File: `src/input_blocking/mod.rs`

- Added elapsed-time measurement around full `check_accessibility_permissions()`.
- Emits warning when probe exceeds 200ms:
  - `"[telemetry] Full accessibility permission probe took ... ms"`

Purpose:
- Quantify whether permission checks themselves are becoming slow.

### Change D: Make `lsof` Mach-port telemetry opt-in
File: `src/input_blocking/event_tap.rs`

- `log_mach_port_count()` now:
  - Returns early unless env var `HANDSOFF_ENABLE_LSOF_TELEMETRY` is set.
  - Logs one-time info that `lsof` telemetry is disabled by default.
- When enabled:
  - Measures and logs `lsof` latency.
  - Warns when `lsof` takes >500ms.

Purpose:
- Remove synchronous heavy command from default runtime path.
- Keep deep telemetry available for targeted diagnostic sessions.

## Phase 2 (Implemented in this branch)
Goal: directly validate callback slowness hypothesis and isolate remaining stall sources.

### Change E: Callback execution timing telemetry
File: `src/input_blocking/event_tap.rs`

Implemented instrumentation:
- Measure callback duration per event.
- Track counters/histogram buckets (e.g., `<1ms`, `1-5ms`, `5-10ms`, `>10ms`).
- Periodically emit summary (every N seconds or M events).
- Log detailed slow-event sample only above threshold (e.g., >5ms), including:
  - event type
  - locked/unlocked state
  - key handling branch / mouse branch
  - time spent in lock acquisition path

Expected result:
- Proves whether true callback slowness is occurring vs scheduler/system-side disablement.

### Change F: Timeout burst guard
Implemented in this branch:
- Added debounce/backoff for repeated timeout-driven re-enable requests.
- Duplicate re-enable requests inside configured window are ignored.

Files:
- `src/lib.rs`
- `src/constants.rs`

## Detailed Implementation Notes (Phase 1)

### `src/input_blocking/mod.rs`
- New import: `std::time::Instant`, `warn`.
- Full probe now measures elapsed and warns on slow checks.
- Added lightweight permission function:
  - `AXIsProcessTrusted()` only.
  - Informational log: `"AXIsProcessTrusted check (lightweight): ..."`

### `src/lib.rs`
- In permission monitor loop:
  - replaced unconditional full probe with conditional:
    - healthy -> lightweight
    - recovery -> full probe
- Added explicit log when recovery-mode full probe is used.

### `src/input_blocking/event_tap.rs`
- Added `LSOF_DISABLED_LOGGED: AtomicBool` to avoid repeated "disabled" logs.
- `log_mach_port_count()` now gated by env var and includes latency timing when enabled.

## Risk Assessment

### Risk 1: Missing edge cases with AX trust caching
`AXIsProcessTrusted()` can lag or be cached inconsistently.  
Mitigation:
- Full event-tap probe remains active in recovery mode (`last_permission_state == false`), where precise restoration detection is required.

### Risk 2: Reduced default visibility into Mach ports
Disabling `lsof` by default lowers always-on telemetry depth.  
Mitigation:
- Telemetry remains available via `HANDSOFF_ENABLE_LSOF_TELEMETRY=1`.
- This is a deliberate tradeoff to keep hot path lean.

### Risk 3: Stutter may still have another source
If stutter persists, root cause may include callback-lock contention or unrelated system load.  
Mitigation:
- Phase 2 callback timing telemetry designed to confirm or falsify callback slowness directly.

## Test Plan

## Automated
1. Run:
   - `cargo test --workspace`
2. Ensure all tests pass with no regressions.

## Manual Runtime Validation
1. Build and run tray app with default settings (no `HANDSOFF_ENABLE_LSOF_TELEMETRY`).
2. Use machine normally for 60-120 minutes.
3. Collect logs and compare:
   - Frequency of timeout disable warnings.
   - Duration between timeout warning and re-enable success.
   - Presence of slow full-probe warnings.
4. Repeat one diagnostic run with:
   - `HANDSOFF_ENABLE_LSOF_TELEMETRY=1`
   - Check `lsof latency` fields; verify correlation with stutter windows.

Success criteria:
- No sustained stutter periods.
- Fewer timeout bursts or shorter recovery windows.
- No growth in live tap count (`created - destroyed` remains bounded, typically 1 when active).

## Rollout Plan
1. Merge Phase 1 changes.
2. Collect runtime telemetry from affected machine(s) for at least 24-48 hours of usage.
3. If timeouts/stutter still observed, implement Phase 2 callback latency instrumentation and repeat.

## Open Questions
1. During stutter windows, is CPU pressure high in other processes (`WindowServer`, browsers, IDE)?
2. Are timeout bursts correlated with high-frequency input (trackpad gestures, scroll storms)?
3. Do timeout events occur more in locked or unlocked state?

These should be answered by the next telemetry pass before deeper architectural changes.
