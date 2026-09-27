# HandsOff Deep Design Review & Remediation Plan

**Status:** Approved for planning (decisions confirmed with owner 2026-09-27)
**Author:** Senior Rust/macOS tech-lead review
**Scope:** Full source review of `src/` (lib, event tap, app state, config, crypto, hotkeys, both binaries), `README.md`, `constants.rs`, existing specs.
**Review focus:** power inefficiency, user-lockout risk, wasted CPU, UX, system-crash risk (+ security-adjacent findings, per owner request).

---

## 1. Review Decisions (confirmed with owner)

| # | Question | Decision |
|---|----------|----------|
| D1 | Passphrase matching strategy | **Migrate to keycode-sequence passphrases** (hash the physical key-code sequence, not characters). Layout-independent; one-time forced re-setup on upgrade is accepted. |
| D2 | Auto-unlock default | **Enabled by default with a long timeout: 3600 s (60 min)**. Requires raising `AUTO_UNLOCK_MAX_SECONDS` (currently 900). |
| D3 | Security-adjacent findings | **Include all** as a dedicated section and plan items. |
| D4 | Plan format | **Single document**: severity-ranked findings → phased remediation roadmap with effort estimates and verification. |

---

## 2. Current Architecture (as found)

- One `CGEventTap` at session level, created at startup and held for process lifetime, subscribing to key down/up, mouse moved, all button/drag/scroll events (`src/input_blocking/event_tap.rs:125-137`).
- Tap run-loop source is added to the **calling thread's** run loop (`CFRunLoop::get_current()` in `enable_event_tap`, `event_tap.rs:361-367`) — in practice the main thread in both binaries. A dedicated CFRunLoop background thread also runs (`lib.rs:213-251`) but **has no sources** (see P-4).
- Shared state: single `parking_lot::Mutex<AppStateInner>` god-object (`app_state.rs`), accessed via per-field helper methods, ~10 lock round-trips per locked keystroke.
- Six independent `thread::sleep` polling loops + one blocking hotkey receiver (see P-3).
- Unlock path: raw macOS keycode → hardcoded US-QWERTY char map (`utils/keycode.rs`) → append to buffer → SHA-256 compare per keystroke.
- Tray main loop: `ControlFlow::WaitUntil(500ms)` polling flags + rebuilding the tooltip string every tick (`bin/handsoff-tray.rs:327-468`).
- CLI main loop: `CFRunLoop::run_in_mode(500ms)`; checks only `should_exit` and `should_stop_event_tap` (`bin/handsoff.rs:301-344`).

### 2.1 Steady-state wakeup budget (unlocked, enabled, idle hands)

| Source | Interval | Wakeups/s |
|---|---|---|
| Buffer-reset thread | 250 ms | 4.0 |
| Dead CFRunLoop thread | 500 ms | 2.0 |
| Tray event loop `WaitUntil` | 500 ms | 2.0 |
| Auto-lock thread | 5 s | 0.2 |
| Permission monitor | 15 s | 0.07 |
| Auto-unlock thread | 10 s | 0.1 (if enabled) |
| **Total (idle, no input)** | | **≈ 8.4/s** |
| MouseMoved callbacks while moving | per event | 60–120/s |

`thread::sleep` timers get **no macOS timer coalescing**; each is a separate kernel timer. In "Disabled" mode the threads skip work but still wake (≈ 4.6/s) — contradicting the "minimal CPU mode" claim in `lib.rs:384` and the expectations in `specs/windowserver-stability-fix.md`.

---

## 3. Findings

Severity: **C**ritical (lockout/crash), **H**igh, **M**edium, **L**ow.

### 3.1 User-lockout risk

**L-1 [C] Passphrases settable at setup may be impossible to type while locked.**
Setup reads cooked, layout-aware stdin (`rpassword`); unlock decodes raw keycodes through a hardcoded US-QWERTY map (`utils/keycode.rs:86-178`). Consequences:
- Non-US layouts: on AZERTY the physical "A" key emits keycode 12, which the map decodes as `q` — even plain ASCII passphrases become untypeable.
- Option/dead-key characters (`é`, `ñ`, `ü`), emoji, CJK: accepted at setup, never producible while locked.
- CapsLock state ignored (only Shift flag checked, `input_blocking/mod.rs:91`).
With auto-unlock defaulting to 0 (release), recovery is Safe Mode or SSH (`README.md` Troubleshooting) — an unacceptable bar for a consumer utility.
→ Fixed structurally by D1 (keycode-sequence passphrases; physical keys are layout-independent). See §5 Phase 1.

**L-2 [C] CLI never recovers the tap after sleep/wake.**
`DISABLED_BY_TIMEOUT` sets `should_reenable_event_tap` (`event_tap.rs:230-244`), but only the tray loop consumes that flag (`bin/handsoff-tray.rs:368-380`). The CLI loop (`bin/handsoff.rs:301-344`) checks only `should_exit` / `should_stop_event_tap`. Result in CLI: after any sleep/wake the tap stays dead forever while the app reports locked — input flows, and the unlock passphrase is typed into whatever window has focus.

**L-3 [H] Keystroke-leak window during tap re-enable (both binaries).**
Between `DISABLED_BY_TIMEOUT` and re-enable there is a ≥ 500 ms poll delay (`POLL_INTERVAL_ENABLED_MS`), extendable to ≥ 10 s by the re-enable debounce (`REENABLE_DEBOUNCE_SECS`, `app_state.rs:306-331`). During that window `is_locked == true` but keystrokes — including passphrase attempts — land in the focused app. macOS documents that a tap may be re-enabled from within the callback via `CGEventTapEnable` with the real tap handle; the past PAC crash (`specs/fix_PAC_crash_*.md`) came from passing the *proxy*, not from in-callback re-enable itself.

**L-4 [H] Auto-unlock disabled by default.**
`AUTO_UNLOCK_DEFAULT_SECONDS = 0` in release (`constants.rs:36-38`). Combined with L-1, a single bad setup choice is a hard lockout. → D2: default 3600 s; raise `AUTO_UNLOCK_MAX_SECONDS` 900 → 7200; update range validation (`config.rs:31-47`), the bounds tests in `config.rs` (assert 900/901 boundaries at lines 236-243, 303-314, 371-388), setup prompts in both binaries, and README.

**L-5 [H] Silent default passphrase `qwet` on first run.**
Tray auto-creates config with `DEFAULT_PASSPHRASE = "qwet"` without user interaction (`bin/handsoff-tray.rs:24, 166-196`), and the tooltip publicly displays "default: qwet" (`bin/handsoff-tray.rs:668`) to anyone hovering the menu-bar icon. The four keys are near-adjacent on the top row — plausible toddler mash. Both a lockout vector (user never ran setup, doesn't know the passphrase… actually does via tooltip, which is worse) and a security hole.

**L-6 [M] CapsLock / modifier-state mismatch.** Subsumed by L-1 fix (keycode sequences don't depend on shift state producing the right char — they record the physical key plus a normalized modifier bit).

### 3.2 Power inefficiency

**P-1 [H] MouseMoved subscription.** The event mask includes `MouseMoved` and three drag types (`event_tap.rs:125-137`). Every pointer movement crosses WindowServer → process, takes the state mutex (`update_input_time`), does 2× `Instant::now()`, atomic telemetry, and a `CGEvent::from_ptr` + `mem::forget` — purely to refresh an auto-lock timestamp; the event is always passed through (`event_tap.rs:252-257`). This is the single largest steady-state energy cost. Idle time is available cheaply via `CGEventSourceSecondsSinceLastEventType(kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType)` polled from a slow timer. Note: only `MouseMoved` is removable — the three drag types MUST remain in the mask (they are blocked while locked; see the mid-hold leak rationale in Phase 2.1).

**P-2 [H] Always-on session tap.** The tap intercepts every keystroke/click system-wide for the process lifetime, yet blocking is only needed while locked (a small minority of uptime). Recommended redesign: **tap-on-lock** — create the tap when locking, destroy on unlock; rely on the Carbon/global hotkey (no accessibility permission needed, zero steady-state cost) to trigger locking, and on the idle-time API for auto-lock. Structurally eliminates P-1, most of the per-event CPU, the constant WindowServer connection, and shrinks the WindowServer-coupling surface noted in `specs/windowserver-stability-fix.md`.

**P-3 [H] Six uncoalesced polling loops** (see §2.1). Consolidate into one 1 s scheduler tick driving buffer-reset/auto-lock/auto-unlock/permission checks; make buffer reset lazy (validate `last_key_time` on the next keystroke; arm a one-shot timer only while locked with a non-empty buffer); park threads on a condvar while disabled instead of sleep-skip.

**P-4 [M] Dead CFRunLoop background thread.** Started "required for event tap" (`lib.rs:214`), but the tap source is added to the caller's (main) run loop, which both binaries already pump (CLI: `handsoff.rs:301-308`; tray: tao pumps the main run loop). The background run loop has no sources and burns 2 wakeups/s forever. Delete it.

**P-5 [M] `lsof` subprocess per tap create/destroy.** `log_mach_port_count` (`event_tap.rs:39-64`) spawns `lsof -p` — documented at 500 ms–8 s (`lib.rs:343` comment). Telemetry only. Gate behind `HANDS_OFF_TELEMETRY=1` or remove.

**P-6 [L] Per-callback telemetry overhead.** 2× `Instant::now()`, 3 atomic RMWs, one CAS loop per event (`event_tap.rs:176-345`). Cheap per call, multiplied by event rate. Sample (1/N) or gate the slow-path timing behind a debug build flag.

### 3.3 Wasted CPU

**C-1 [M] ~10 mutex round-trips + 2 String clones per locked keystroke.** `handle_keyboard_event` (`input_blocking/mod.rs:13-140`) calls `get_lock_keycode`, `get_talk_keycode`, `is_locked`, `update_input_time`, `append_to_buffer`, `update_key_time`, `get_buffer` (clone) ×2, `get_passphrase_hash` (clone), `set_locked`, `clear_buffer` — each a separate `parking_lot` lock. Take one guard for the whole handler; verify against the borrowed buffer in place.

**C-2 [M] Tooltip rebuilt every 500 ms.** `build_tooltip` (`handsoff-tray.rs:610-710`) allocates ~15 formatted strings every tick, then String-compares against the last one. Rebuild only on state transitions and once per second while a countdown is visible.

**C-3 [M] Duplicate lock-hotkey paths.** The lock hotkey is handled in the event tap (`mod.rs:22-39`) *and* via `global_hotkey` (`lib.rs:514-540`) — two registrations, two state transitions to keep consistent. After P-2, the global hotkey is the sole lock trigger (unlocked) and the tap only exists while locked; one path remains.

**C-4 [L] Full test-tap permission probe.** `check_accessibility_permissions` (`mod.rs:155-258`) creates and destroys a real `CGEventTap` at startup/restart — exactly the WindowServer churn the project previously fought. Use `AXIsProcessTrusted` everywhere except one startup confirmation.

### 3.4 UX

**U-1 [H] Blind unlock with zero feedback.** No indication of typing progress, of the 3 s buffer auto-clear, or of a failed attempt. Users retype into a void and can conclude the Mac is frozen (→ hard reboot, the worst outcome for this app's reputation). Add: menubar icon pulse per accepted key and on buffer clear, an optional locked-state HUD strip ("HandsOff locked — type passphrase"), and a sound on unlock/failure.

**U-2 [H] Silent mid-meeting auto-lock.** Auto-lock fires on pure input inactivity — watching a video or listening on a call locks the machine mid-presentation. Add a 10 s pre-lock notification ("Locking in 10 s — any input cancels") and optional meeting-aware suppression (camera/mic active via CMIO/CoreAudio property, or any active user-activity assertion).

**U-3 [M] Blocking osascript alerts.** `show_alert` (`handsoff-tray.rs:577-589`) spawns `osascript` and blocks on `.output()` until the user clicks OK — stalling the main run loop, which starves the tap → `DISABLED_BY_TIMEOUT`. Replace with non-blocking notifications.

**U-4 [M] Documentation contradiction.** README header: auto-lock "120 seconds"; Usage section: "automatically locks after 30 seconds". Tooltip leaks the default passphrase (L-5). Fix docs.

**U-5 [M] Talk hotkey transforms input while unlocked.** The Ctrl+Cmd+Shift+T → spacebar rewrite has no `is_locked` gate (`mod.rs:45-67`); pressing it unlocked injects a literal space into the focused app. Resolves naturally under P-2 (no tap while unlocked); otherwise gate it.

**U-6 [L] Locked state is nearly invisible.** Only a small red circle in the menu bar. Consider a status-item title ("LOCKED") while locked.

### 3.5 Crash / system-stability risk

**R-1 [H] Latent UAF via implicit run-loop ownership.** `enable_event_tap` / `remove_event_tap_from_runloop` both use `CFRunLoop::get_current()` (`event_tap.rs:361-367, 425-431`). Today add/remove happen to run on the same thread; if a future refactor (or `Drop` from an unexpected thread) removes from a different thread, the removal silently targets the wrong run loop while the real loop retains a source backed by a `CFRelease`d tap → crash on next wake. The 20 ms drain sleep (`EVENT_TAP_DRAIN_DELAY_MS`) is a racy band-aid, not synchronization. Fix: store the owning `CFRunLoopRef` at enable time, remove from it explicitly, `debug_assert` thread identity.

**R-2 [M] `user_info` lifetime vs in-flight callbacks.** `stop_event_tap` frees the boxed `Arc<AppState>` (`lib.rs:340-347`) immediately after disable+drain. Safe only because callbacks are serialized on the main run loop today — an implicit invariant. Codify ordering: disable → drain → remove source from owning run loop → *then* free; never free while the source is registered.

**R-3 [M] Panic across FFI = abort.** `event_tap_callback` has no `catch_unwind`; any panic in the handler aborts the process mid-callback. Fail-open (input is restored), but it crashes the app and any telemetry/state. Wrap the callback body; on panic, pass the event through and request a clean restart.

**R-4 [L] Duplicated hand-rolled FFI.** `CGEventTapCreate` and its constants are declared twice (`event_tap.rs:93-118`, `mod.rs:173-204`) with drift risk; `core-graphics` already exposes safe bindings for most of this surface. Consolidate into one module.

**R-5 [L] WindowServer coupling.** History of zombie Mach ports / desktop stutter (`specs/windowserver-stability-fix.md`). P-2 (tap-on-lock) bounds tap lifetime to locked periods — the structural fix.

### 3.6 Security-adjacent (included per D3)

**S-1 [H] Partial passphrase written to logs.** `debug!("Buffer updated: {}", state.get_buffer())` (`mod.rs:113`) logs the growing plaintext passphrase. Remove; log length only.

**S-2 [M] Plaintext passphrase held for process lifetime** for the Reset menu (`passphrase_for_reset`, `handsoff-tray.rs:310`). Under keycode-sequence passphrases, Reset can force-unlock via state without re-verifying plaintext; keep only the hash; zeroize any transient copies.

**S-3 [M] Predictable default passphrase.** See L-5.

**S-4 [L] Non-constant-time hash compare** (`utils/mod.rs:12`). Local threat model makes this low-risk, but with the D1 migration, switch to a constant-time compare (`subtle`) — one line.

---

## 4. Target Metrics (acceptance for the plan overall)

| Metric | Today | Target |
|---|---|---|
| Idle wakeups, unlocked | ≈ 8.4/s + per-event | ≤ 1/s (scheduler tick only) |
| Idle wakeups, disabled | ≈ 4.6/s | ≤ 0.2/s |
| CPU, unlocked, no input | per-event callbacks | ≈ 0% (no tap installed) |
| Mouse-move cost | callback per move | zero (no MouseMoved in mask) |
| Lockout recovery without Safe Mode/SSH | none | auto-unlock ≤ 60 min; setup rejects untypeable passphrases |
| Locked-input guarantee | gap ≥ 500 ms on wake | no gap (in-callback re-enable) |
| Lock/CPU measured via | — | `powermetrics --samplers tasks` before/after, 10 min idle |

---

## 5. Phased Remediation Plan

Effort: S < 1 day, M = 1–3 days, L = 3–5 days. Phases are independently shippable; Phase 0 first.

### Phase 0 — Safety hotfixes (S)

No behavior redesign; closes the worst holes.

1. **S-1**: delete the buffer-content debug log; log buffer length only.
2. **L-2**: handle `should_reenable_event_tap_and_clear` and `should_start_event_tap_and_clear` in the CLI main loop (mirror the tray loop blocks, `handsoff-tray.rs:368-403`).
3. **L-3**: in-callback tap re-enable — store the live `CGEventTapRef` in `AppState` (atomic usize) after creation; on `DISABLED_BY_TIMEOUT` in `event_tap_callback`, call `CGEventTapEnable(tap, true)` directly (real tap handle, *not* the proxy — preserves the PAC fix). Keep the flag-based path + debounce as fallback. Clear the stored handle before teardown.
4. **P-5**: gate `log_mach_port_count` behind `HANDS_OFF_TELEMETRY=1`.
5. **U-4**: fix the README 30 s vs 120 s contradiction.

**Verify:** new unit test: CLI flag-drain logic (extract a shared `drain_pending_flags()` in `lib.rs` used by both binaries — test that); manual sleep/wake cycle ×5 on CLI and tray, confirm tap stays live and `TAPS_CREATED == TAPS_DESTROYED + 1`; confirm no passphrase appears in logs at debug level.

### Phase 1 — Lockout-proofing (M)

Breaking change accepted per D1.

1. **Keycode-sequence passphrases (L-1, L-6):**
   - Setup records the sequence of `(keycode, shift?)` pairs (define canonical form: keycode + whether Shift is required to produce the char; simplest robust form: raw keycode sequence ignoring modifiers entirely, since Escape/Backspace are control keys — spec the canonical form in code comments).
   - `AppState.input_buffer: String` → `Vec<i64>` of keycodes; hash = SHA-256 over the keycode byte sequence; constant-time compare (S-4).
   - Reject Escape, Backspace, and the configured hotkey combos as passphrase members at setup.
   - Minimum length 4 keys.
   - `utils/keycode.rs` char map is deleted from the unlock path (may remain for hotkey display only).
2. **Config migration:** add `passphrase_format = "keycode-v1"` field; on load, a legacy config triggers a one-time forced re-setup (blocking alert + instructions, both binaries); never silently keep an unverifiable legacy passphrase.
3. **Auto-unlock default 3600 s (L-4):** `AUTO_UNLOCK_DEFAULT_SECONDS = 3600` (release), `AUTO_UNLOCK_MAX_SECONDS = 7200`; update `config.rs` range validation + bounds tests (900/901 → 7200/7201), setup prompt text, README.
4. **First-run setup enforcement (L-5, S-3):** delete `DEFAULT_PASSPHRASE` auto-creation; tray first run without config → modal alert directing to `--setup` (or an in-app setup flow), then exit cleanly. Remove the passphrase hint from the tooltip.
5. **S-2:** Reset path force-unlocks via state, not stored plaintext; drop `passphrase_for_reset`; zeroize setup-time plaintext copies.

**Verify:** layout matrix test — set passphrase on US layout, unlock with layout switched to AZERTY and Dvorak (must succeed: keycodes are layout-independent); legacy config → forced re-setup flow; auto-unlock fires at 3600 s (shorten via env for the test); `cargo test` with updated bounds tests.

### Phase 2 — Power/CPU architecture (L)

1. **Tap-on-lock (P-1, P-2, C-3, U-5, R-5):**
   - Remove **only** `MouseMoved` from the event mask. Locked mask = keys + button down/up + scroll + **all three drag types** (`LeftMouseDragged`/`RightMouseDragged`/`OtherMouseDragged`). Drags MUST stay in the mask: if lock engages while a button is already held (auto-lock mid-hold, or lock hotkey pressed with button down), the tap never saw the button-down and any subsequent drag events outside the mask would pass through — dragging/selecting in the focused app while "locked". Current code blocks these when locked (`event_tap.rs:289-318`); that behavior must be preserved. Under tap-on-lock, drag types in the mask carry no idle cost anyway (no tap exists while unlocked).
   - Create the tap on lock (menu, hotkey, auto-lock), destroy on unlock. Lock trigger while unlocked = `global_hotkey` only (event-driven, zero steady-state cost); the tap-based hotkey handling in `mod.rs` is removed.
   - Lock flow when unlocked: hotkey/menu/auto-lock thread sets a `request_lock` flag → wake the main run loop (`CFRunLoopWakeUp` / tray `EventLoopProxy`) → main thread creates tap, then sets `is_locked`. If tap creation fails: never enter locked state; notify.
   - Auto-lock idle source: `CGEventSourceSecondsSinceLastEventType` polled from the scheduler; delete `update_input_time` from the event path.
2. **Thread consolidation (P-3):** one scheduler thread, 1 s tick, dispatching due checks (auto-lock, auto-unlock, permission); buffer reset becomes lazy (checked on next keystroke) + one-shot timer only while locked with non-empty buffer; all worker loops park on a condvar while `is_disabled`.
3. **Delete the dead CFRunLoop thread (P-4)** and its start/stop machinery.
4. **Single-guard keystroke handler (C-1):** one `state.inner.lock()` per event; no buffer clones; hash computed from the borrowed buffer.
5. **Tray event-driven wakeups:** replace `WaitUntil(500ms)` flag polling with `EventLoopProxy` user events posted by state transitions; tooltip rebuilt on transitions + 1 s tick only while a countdown is shown (C-2). Disabled mode: park at 60 s tick.
6. **C-4:** `AXIsProcessTrusted` everywhere except one startup test-tap.
7. **P-6:** sample callback telemetry (1/16) or gate behind debug builds.

**Verify:** `powermetrics --samplers tasks -n 1` idle 10 min before/after — wakeups ≤ 1/s unlocked, ≤ 0.2/s disabled; locked blocking still complete (automated key/mouse injection via `CGEventPost` while locked → assert nothing reaches a test harness app); **mid-hold leak regression: press and hold a mouse button, then lock via hotkey and via auto-lock, then drag — no drag/select may reach the focused app**; lock→unlock→lock ×100 soak, `TAPS_CREATED == TAPS_DESTROYED`; talk hotkey only transforms while locked.

### Phase 3 — Crash hardening (M)

1. **R-1/R-2:** store owning `CFRunLoopRef` at enable; remove source from it explicitly; enforce free ordering (disable → drain → remove → free); `debug_assert` same-thread add/remove.
2. **R-3:** `catch_unwind` around the callback body; on panic, pass event through + set restart flag.
3. **R-4:** single FFI module; drop the duplicate `CGEventTapCreate` declaration in `mod.rs` (permission probe reuses `event_tap` bindings).
4. **U-3:** replace `show_alert` osascript with non-blocking notifications; where a modal is unavoidable, show it from a helper thread that never stalls the tap-owning run loop.

**Verify:** `cargo test`; forced panic injection in the handler behind a test flag → app survives, input flows, restart requested; 24 h soak with periodic lock/unlock and 3 sleep/wake cycles, watching `TAPS_CREATED/DESTROYED` and Mach port count (telemetry on).

### Phase 4 — UX (M–L)

1. **U-1:** locked HUD strip (borderless overlay window shown only while locked: "HandsOff locked — type passphrase, Esc to restart entry"); menubar icon pulse per accepted key + on buffer clear; unlock success/failure sound.
2. **U-2:** 10 s pre-lock notification with cancel-on-input; meeting-aware auto-lock suppression (spike: CMIO camera-active property vs CoreAudio input-in-use vs user-activity assertions — pick the cheapest reliable signal; feature-flag default off).
3. **U-6:** status-item title "LOCKED" while locked.

**Verify:** manual UX pass: lock → HUD visible → type wrong passphrase → feedback → Esc → correct passphrase → unlock sound; auto-lock warning appears 10 s early and cancels on input; Zoom call with camera on does not auto-lock when suppression enabled.

---

## 6. Plan Risks

- **Tap-on-lock activation latency** (hotkey → main-loop wake → tap create): tens of ms worst case; a keystroke in that window passes through. Acceptable (locking, not unlocking), but document it.
- **Keycode-sequence migration** forces re-setup for every existing user on upgrade; the installer/CHANGELOG must call this out prominently, and the app must fail loudly into setup, never into a locked state.
- **In-callback re-enable (L-3)** touches the code path involved in the past PAC crash; must reuse the stored tap handle — never the proxy — and be reviewed against `specs/fix_PAC_crash_*.md`.
- **Meeting-aware suppression** signals are undocumented/private-adjacent APIs; keep behind a feature flag, default off, and treat as Phase 4 spike before committing.

## 7. Out of Scope

- Sandboxing / notarization changes, installer changes beyond the migration notice.
- Replacing the `parking_lot` god-object state with a finer-grained design (unnecessary at this scale once C-1 lands).
- Network/telemetry features.
