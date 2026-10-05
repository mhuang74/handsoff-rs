# CPU spin investigation: dead CFRunLoop thread saturates a core

Date: 2026-10-05 · Version sampled: 0.9.0 (20261003.041430) · Machine: MacBook Air M2, macOS 15.8.1

## Symptom

Activity Monitor shows `handsoff-tray` at ~100% CPU (exactly one core, steady) **when the tray is enabled** (tray not Disabled). Constant regardless of input activity. When Disabled, CPU drops — because Disable (`stop_event_tap`, `src/lib.rs:469-482`) tears down the tap *and* stops the thread that is the actual culprit.

## Evidence: 5 s `sample` of the live release binary

Full report provided by the user (`~/tmp/hs-sample.txt`, pid 12718, 3737 samples @ 1 ms). Per-thread summary:

| Thread | Samples busy | State |
|---|---|---|
| Thread_163357 (main) | ~2/3737 | idle in `mach_msg` inside `[NSApplication run]` — healthy, services the tap source |
| Thread_163406 NSEventThread | 0 | idle |
| **Thread_168014** (`HandsOffCore::start_cfrunloop_thread` closure) | **2999/3737 ≈ 80%** | **hot userland spin** |
| Thread_168019 buffer-reset | 0 | `nanosleep` |
| Thread_168020 auto-lock | 0 | `nanosleep` |
| Thread_168021 hotkey listener | 0 | parked on crossbeam `recv` |
| Thread_168022 auto-unlock | 0 | `nanosleep` |
| Thread_168023 permission-monitor | 0 | `nanosleep` |

The busy thread's stacks are **not** in `mach_msg` (blocked) — they churn through `CFRunLoopRunSpecific → __CFRunLoopCopyMode → CFSetGetValue/CFEqual/CFHash` (586+ of the 588 `CF_IS_OBJC` top-of-stack hits are in this thread), interleaved with `try_recv` on the shutdown channel, a suppressed `trace!` log-level check, and `Duration::from_millis`. That is the loop body of `src/lib.rs:311-331` executing at full speed.

## Root cause

`src/lib.rs:299-338`, `start_cfrunloop_thread`:

```rust
loop {
    let result = unsafe {
        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode,
                               Duration::from_millis(CFRUNLOOP_POLL_INTERVAL_MS), // 500 ms
                               false)
    };
    if shutdown_rx.try_recv().is_ok() { break; }
    ...
}
```

**`CFRunLoopRunInMode` returns `kCFRunLoopRunFinished` immediately when the run loop has no sources/timers/observers in the mode — it does not honor the duration as a sleep.** The thread's run loop owns zero sources: the event tap's run-loop source is added to the *main* thread's loop (`enable_event_tap` uses `CFRunLoop::get_current()`, `src/input_blocking/event_tap.rs:412`, called from the main thread in the tray). So every iteration returns `Finished` instantly and the loop re-calls it as fast as it can → one core saturated by pure CoreFoundation mode-bookkeeping churn.

Contributing noise per iteration: the `result != CFRunLoopRunResult::TimedOut` check at `lib.rs:328-330` expects `TimedOut` but gets `Finished` every iteration (visible in the sample as `log::Level` `PartialOrd::le` frames) — the code comment "Run the loop for 0.5 seconds" was an assumption, never verified.

### Why the spec missed it

`specs/deep-design-review-2026-09.md` P-4 correctly flags the thread as dead ("tap source is added to the caller's (main) run loop… Delete it") but prices it at "2 wakeups/s" — assuming `run_in_mode` blocks for the duration. Reality is ~4 orders of magnitude worse: a full busy spin. P-4 (deletion) is still the correct fix; this document supplies the missing measurement.

### Hypotheses examined and ruled out

- **tao 0.28.1 waker spin** (`observer.rs` `EventLoopWaker`, 0.1 µs repeat-interval timer): main thread is 99.9% blocked in `mach_msg` — disproved. (`ControlFlow::Poll` is never used by the tray; `WaitUntil` re-arms at ~500 ms.)
- **Per-event tap callback cost**: symptom is constant and input-independent — ruled out.
- **Other pollers** (buffer-reset 250 ms, auto-lock 5 s, auto-unlock 10 s, permission-monitor 15 s, hotkey `recv`): all sleep-based, all idle in the sample — ruled out. They cannot physically saturate a core.

## Fix (agreed, not yet implemented)

Delete the thread entirely — spec P-4. The tap is serviced by the main run loop, which tao's `NSApplication run` pumps continuously (proven: input blocking works today with the background loop empty, and the sample shows the main loop parked waiting for events).

Scope of change (all in `src/lib.rs` unless noted):

1. Delete `start_cfrunloop_thread` (`lib.rs:299-338`) and `stop_cfrunloop_thread` (`lib.rs:340-358`).
2. Remove the `cfrunloop_thread` field (`lib.rs:75`), its `None` initializer (`lib.rs:93`), and the `mpsc` channel machinery.
3. Remove both call sites: `start_event_tap` (`lib.rs:363`) and `stop_event_tap` (`lib.rs:400`), and the stale "Start CFRunLoop thread first (required for event tap)" / "Stop CFRunLoop thread" comments.
4. Remove `CFRUNLOOP_POLL_INTERVAL_MS` from `src/constants.rs:59` (and the now-unreachable "Recommended range" doc comment that references it).
5. Clean imports (`CFRunLoopRunResult`, `kCFRunLoopDefaultMode`, `mpsc` if otherwise unused, `JoinHandle` if unused).

Risk: low. No code reads the thread's existence; no test references it (`grep cfrunloop tests/` = no matches); tap lifecycle, lock/unlock, and capture paths are untouched. The only behavior change: the process no longer burns a core. Note the thread was already started/stopped alongside the tap, so the enabled/disabled CPU delta also collapses to the intended (small) wakeup cost.

## Verification protocol (user, on-device)

Agent-driven GUI verification is blocked on this Mac (repo `AGENTS.md`), so:

1. Build release here via `cargo zigbuild` (SDKROOT setup per repo memory); copy the binary into a locally built `/Applications/HandsOff.app` bundle (bundle id `handsoff-tray.handsoff` unchanged, TCC-approved path).
2. If the stale-CDHash wall bites (grant pinned to old build, per issue #34): `tccutil reset Accessibility handsoff-tray.handsoff`, relaunch from the bundle, re-grant.
3. Functional smoke: Lock → input blocked → unlock via passphrase → Reenable/Disable still work.
4. Measurement: Activity Monitor idle %CPU while enabled (target: <1%, ideally ~0); re-run `sample handsoff-tray 5` (expect no `start_cfrunloop_thread` thread on top); spot-check Energy tab.
5. Record before/after numbers in the PR + CHANGELOG entry.

Process: GitHub issue first (with this document's sample summary as evidence, labeled `ready-for-agent`), branch + PR closing it. Wakeup consolidation (buffer-reset parking P-3, tray `WaitUntil` → event-driven C-2, ~8 wakeups/s remaining) is deliberately **deferred**: decide after measuring Energy impact with the spin gone.