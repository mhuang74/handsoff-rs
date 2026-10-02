# HandsOff-RS Worktree + Interception Hang Risk Assessment (origin/main @ 2bf03da)

Worktree: `/home/mhuang/Development/handsoff-main` (detached HEAD `2bf03da`, clean).
Main checkout untouched. Read-only analysis; no edits, tests, or commits anywhere.

Branch deltas: `origin/main..fix_installer_hang` = `3889213` (#34 stale-TCC escape hatch), `86dd3c1` (wizard lockout: explicit capture start, close-abort, 120 s timeout, single-instance flock), `ee5196a` (#36 Change Passphrase overhaul). Tap-core files (`event_tap.rs`, `lib.rs`, `mod.rs`) are effectively identical between branches; fixes land in `setup.rs` / `wizard.rs` / `handsoff-tray.rs`. So every un-fixed risk below is live on `fix_installer_hang` too unless noted.


---

## H1 — Filter-tap severity: a wedged callback stalls ALL host input [CRITICAL]

- `src/input_blocking/event_tap.rs:150-186` — `create_event_tap` uses `K_CGEVENT_TAP_OPTION_DEFAULT = 0` (`event_tap.rs:146`), i.e. `CGEventTapOptionDefault` = **filter tap**, not listen-only.

- Mask (`event_tap.rs:154-164`): KeyDown, KeyUp, Left/Right MouseDown/Up/Dragged, OtherMouseDragged, ScrollWheel — all filtered system-wide at head-insert (`K_CGHEAD_INSERT_EVENT_TAP = 0`, `event_tap.rs:145`).
- Callback returning NULL (`event_tap.rs:346-352`) swallows the event; the same machinery that blocks input during lock is the mechanism that would lock out the whole session if the callback stalls.
- macOS's own mitigation is the tap-timeout (`DISABLED_BY_TIMEOUT` 0xFFFFFFFE, `event_tap.rs:200`): a too-slow callback gets the tap disabled, which *restores* input but silently stops protection. So the failure mode is either host-wide input stall (callback blocked, e.g. on a mutex held elsewhere) or silent loss of blocking (macOS disables the tap). Both are bad; the stall is worse.
- Trigger scenario: any blocking call added to the hot path (I/O, subprocess, contended lock) → every keystroke/click system-wide queues behind it. Currently the hot path is clean (H5), so this is a *severity amplifier* for H2–H6 rather than an independent trigger.

## H2 — Teardown drain is heuristic, not a barrier [LOW; corrected after verification]

- `event_tap.rs:455-487` `remove_event_tap_from_runloop`: `CGEventTapEnable(tap,false)` → 20 ms sleep (`constants.rs:111` `EVENT_TAP_DRAIN_DELAY_MS`) → remove source → `CFRelease(tap)`.
- **Run-loop ownership (verified, corrects an earlier draft claim):** `enable_event_tap` adds the tap source via `CFRunLoop::get_current()` (`event_tap.rs:414`), and every start/stop path runs on the **main thread** — tray: `start_event_tap` at `handsoff-tray.rs:228`, `stop_event_tap` at `handsoff-tray.rs:549`; CLI: `handsoff.rs:208` / `handsoff.rs:252`, whose main loop comment states "Run the event loop on the main thread - this is required for event tap to work!". Add and remove therefore target the **same** main-thread run loop; callbacks are serialized with teardown on that thread. The CFRunLoop background thread (`lib.rs:267-303`) hosts no tap source — it is a vestigial 500 ms spin loop, not the tap's service thread. The earlier "wrong-runloop removal + CFRelease racing in-flight callbacks" UAF claim does not hold; the historical PAC crash (`docs/fix_PAC_crash_claude.md`, 8b9a66e) was about the `proxy` parameter type misuse in the callback, which is separately and correctly guarded (`event_tap.rs:198-199` comment).
- Residual (low) concerns: (a) the 20 ms drain is a heuristic — adequate given same-thread serialization, but uncommented as to *why* it's sufficient; (b) `CFRunLoop::get_current()` in both enable and remove is thread-identity-dependent: any future refactor that calls either from a non-main thread silently attaches/detaches the tap to the wrong loop with no assertion guarding the invariant. A secondary knock-on: since callbacks and teardown share the main thread, a wedged callback (H1) now also blocks the tray's stop/restart recovery paths — the stall and the recovery freeze arrive together.

## H3 — Capture tap without panic containment: stranded swallow-everything tap [HIGH]

- `src/setup.rs:253-499` `capture_passphrase_headless` installs a KeyDown filter tap (`setup.rs:432-446`) whose callback returns NULL for **every** key (`setup.rs:336-424`), then pumps nested CFRunLoop in 100 ms slices up to `CAPTURE_TIMEOUT_SECS` (`setup.rs:461` — **300 s on origin/main**; fix branch lowers it to 120 s with countdown).
- Teardown (`setup.rs:486-493`: disable → 20 ms sleep → CFRelease) is plain code after the loop — **no `catch_unwind` anywhere in the codebase** (verified: zero hits). A panic between tap install and teardown (e.g. in the `on_event` UI reporter closure invoked under the capture-state mutex at `setup.rs:349-352`, or in AppKit calls from the wizard's main-thread reporter, `wizard.rs:825-864`) unwinds through `extern "C"` (UB/abort) or, where unwinding is suppressed, leaves the tap installed and enabled: **system-wide keyboard blackhole until process death**.
- The 2026-10-01 incident (16 stacked wizard launches silently eating keys) is exactly this class; `86dd3c1` fixes the *trigger* (auto-install + no close-abort + no single-instance), not the invariant. On origin/main all three original triggers are still live: capture auto-starts when the wizard reaches the capture step, window close does not abort (`wizard.rs:1259-1266` CloseRequested only handles the pre-capture phase), and there is no flock guard — duplicate tray launches each install their own swallow tap for up to 300 s.
- Escape hatch: only Ctrl+C (recognized inside the callback, `setup.rs:368-380`) or the 300 s timeout. If install fails the tap is never created (fine); if the *process* dies mid-capture the tap dies with it — the strand window is "panic caught somewhere upstream but process lives" plus the multi-launch stacking case.

## H4 — `stop_cfrunloop_thread`: vestigial thread, bounded-but-pointless join [LOW; corrected after verification]

- `lib.rs:265-327`: `start_cfrunloop_thread` spawns a thread running `run_in_mode` in 500 ms slices (`lib.rs:283-296`, doc comment at `lib.rs:267-268` claims "Required for event tap to receive events"). Per the verified threading model (H2), the tap's run-loop source is added to the **main** thread's loop (`event_tap.rs:414` from main-thread call sites) — the background thread hosts no source and services nothing. The thread is a vestigial 500 ms spin loop burning a core slice every half-second while the tap is live.
- `stop_cfrunloop_thread` (`lib.rs:305-327`) sends shutdown via `mpsc` (checked between slices, `lib.rs:291-294`) then joins. Since no tap callback runs on this thread, the join cannot be blocked by a wedged callback — the earlier "callback blocks the slice → join never completes" chain was false. Worst case is bounded: one in-flight 500 ms slice plus slice-granularity detection, i.e. the join returns within ~1 s of shutdown. The missing join *timeout* is still an unguarded invariant (a future repurposing of this thread could reintroduce the stall), but today's risk is negligible.
- The real stop-path serialization risk moved to the main thread (see H2 knock-on and H6): the actual tap drain/quiescence and `stop_event_tap` run where the callbacks run, so a wedged callback now freezes the *main* thread's teardown — no join involved.

## H5 — Hot path holds the state mutex across SHA-256 verify [MEDIUM — do not grow]

- `mod.rs:22-122` `handle_keyboard_event`: hotkey checks lock-free; then one `parking_lot` guard held across buffer mutation + `auth::verify_keycodes` (SHA-256 over up to buffer-length keys, `utils/mod.rs:35-46`) and, on match, `complete_passphrase_unlock` re-locks (`app_state.rs:234-252`).
- Today the critical section is microseconds — fine. The structural risk: every future feature that touches state under this mutex (logging, config I/O, notifications) lands **inside the tap callback's critical path**, converting H1 from theoretical to immediate. The comment at `mod.rs:19-21` ("single state guard for the whole handler (C-1)") documents the tradeoff; nothing enforces keeping it small.
- `handle_mouse_event` (`mod.rs:126-133`) only takes `update_input_time()` (its own lock acquisition, `app_state.rs:315`) — mouse events contend with the same mutex from the same callback thread.

## H6 — `show_alert` / `confirm_reset` osascript subprocess on the tray main thread [MEDIUM]

- `handsoff-tray.rs:1032-1044` `show_alert` and `handsoff-tray.rs:1008-1023` `confirm_reset`: `Command::new("osascript").output()` — **blocking** wait for a child process, called from inside the tray session/event-loop closures (e.g. `handsoff-tray.rs:735, 748, 798, 820, 855, 891, 939, 969, 986`).
- While blocked: no tray event-loop ticks → the `should_stop_event_tap` / `should_reenable_event_tap` / `should_start_event_tap` polling blocks at `handsoff-tray.rs:573-619` don't run, menu clicks queue, tooltip freezes. An `osascript` hang (login-window transitions, TCC prompts, securityd stalls) freezes tray responsiveness for the duration — and since the tap's run-loop source is serviced on this same main thread (H2), a long osascript stall also delays tap event delivery: during that window macOS may hit the tap-timeout path and disable blocking outright.
- Same class: `log_mach_port_count` runs `lsof -p` on tap create/destroy (`event_tap.rs:36-62`, called at `:190` and `:486`) — a subprocess on the main thread inside `start/stop_event_tap`. Documented 500 ms–8 s latency; the re-enable path removed its call (`lib.rs:405-407` note) but create/destroy still pay it.

## H7 — Permission polling: full test-tap creation cycles [LOW on origin/main — largely fixed]

- Historical degradation (docs/fix_slow_event_callback_opus_46.md): periodic `CGEventTapCreate`+`CFRelease` cycles degraded WindowServer callback latency → tap timeouts.
- origin/main permission monitor (`lib.rs:718`) uses `check_accessibility_permissions_lightweight()` = `AXIsProcessTrusted()` only (`mod.rs:137-145`) every `PERMISSION_CHECK_INTERVAL_SECS = 15` (`constants.rs:79`). The full test-tap check (`mod.rs:148-258`) is confined to startup and `poll_accessibility_granted` (`setup.rs:529-547`, deadline-bounded loop, caller-chosen interval) and restart gating (`lib.rs:376`). Residual exposure: each restart/regrant pays one full tap create; acceptable.
- Not fixed-on-branch (already fine on both).

## H8 — Background-thread polling loops [LOW — bounded and off hot path]

All bounded sleeps on dedicated threads, none on the tap thread: buffer reset 250 ms (`lib.rs:496-511`, `constants.rs:64`), auto-lock 5 s (`lib.rs:514-545`), auto-unlock 10 s (`lib.rs:623-642`, `constants.rs:74`; backoff math in `app_state.rs:19-40, 279-300` — doubling capped, reset only on passphrase unlock `app_state.rs:234-252`), hotkey listener blocking `recv()` (`lib.rs:559-586`). These affect detection latency (e.g. auto-unlock fires up to 10 s late), never input latency. No hang risk found.

---

## Phase A question set — answers

1. **Does the event tap callback hold any lock while blocking?** It takes the state mutex (`mod.rs:68`) and holds it across the locked-keystroke handler including SHA-256 verify (H5). It releases before returning; the block decision itself is lock-free for mouse events except `update_input_time`. No lock is held across any I/O — nothing *currently* blocking, but the guard span is the structural exposure (H1 amplifier).
2. **Is every loop bounded?** Capture pump: 300 s deadline (`setup.rs:461`) — bounded but long; fix branch: 120 s. Permission poll: caller-bounded (`setup.rs:529-547`). `run_interactive_setup` capture retry loop (`setup.rs:700-706`): unbounded by design (user aborts via Ctrl+C). Background threads: infinite by design, 0.25–15 s sleeps. `stop_cfrunloop_thread` join: no timeout, but bounded in practice to ~1 s since the thread services no tap source (H4). Prompt re-prompt loops: 3-strike bounded (`setup.rs:605-630`).
3. **Blocking syscalls in the input hot path?** None in `event_tap_callback` (`event_tap.rs:196-352`): atomics, mutex, `Instant::now`, `mem::forget`; slow-callback telemetry threshold 500 µs (`constants.rs:99`). Blocking calls exist adjacent to the hot path: 20 ms sleeps in both teardowns, `lsof` subprocess on create/destroy (H6), osascript on tray actions (H6).
4. **Escape hatch if install/close fails?** Install failure → `create_event_tap` returns None → error path, no tap (safe). Close/teardown is the gap: the 20 ms drain (H2) is an unverified heuristic rather than a proven quiescence barrier, and no catch_unwind wraps the capture (H3); the CFRunLoop-thread join (H4) is timeout-less but bounded (~1 s). The sharper close-failure mode is main-thread wedging: since callbacks and teardown share the main thread, a stuck callback blocks `stop_event_tap` itself (H2 knock-on, H6). User-facing escape from a live capture tap: Ctrl+C chord (TUI) or timeout; wizard on origin/main has **no** close-abort (fixed by 86dd3c1) and no countdown (fix branch adds `CaptureEvent::Tick`).

## Priority order

| # | Risk | Severity | Fixed on fix_installer_hang? |
|---|------|----------|------------------------------|
| H1 | Filter-tap stall amplification | CRITICAL (class) | n/a — inherent to design |
| H3 | Capture tap stranded on panic / auto-install / stacking | HIGH | trigger fixed (86dd3c1); panic containment still absent |
| H2 | Teardown drain heuristic; thread-identity-dependent run-loop attach (no invariant assertion) | LOW | no |
| H4 | Vestigial CFRunLoop thread (500 ms spin); join timeout-less but bounded | LOW | no |
| H5 | Mutex-held SHA-256 in hot path | MEDIUM (latent) | no |
| H6 | osascript/lsof on main thread | MEDIUM | partially (lsof removed from re-enable only) |
| H7 | Test-tap polling degradation | LOW | already fixed on both |
| H8 | Polling-thread latency | LOW | n/a — not a hang |

Material scope: assessment only. Prototyped fixes (e.g. catch_unwind + abort-flag around capture, removing the vestigial CFRunLoop thread, asserting the main-thread run-loop invariant for tap add/remove) would live in the worktree as a separate follow-up.

**Revision note (2026-10-02):** the first draft scored the tap teardown as a HIGH wrong-runloop UAF (add/remove on different run loops). Verification of the call graph (`enable_event_tap` event_tap.rs:414 and `remove_event_tap_from_runloop` event_tap.rs:478 both use `CFRunLoop::get_current()`, and all lifecycle call sites — tray handsoff-tray.rs:228/549, CLI handsoff.rs:208/252 — run on the main thread) disproved it: add and remove share the main-thread run loop and callbacks are serialized with teardown. H2 is downgraded to LOW; H4's "callback blocks the join" chain was collateral of the same false premise (the background thread services no tap source, so the join is bounded ~1 s) and is downgraded accordingly, with the thread re-characterized as vestigial.
