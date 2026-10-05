# CPU spin fix — implementation plan v1.2 (standalone)

Date: 2026-10-05 · Target release: **v0.9.1** (patch on `main`) · Base commit: `72260c7` · Status: ready to implement
Type: pure deletion of a dead busy-spin thread + version bump. No tap/lock/unlock/capture logic changes.

This is a self-contained, executable plan. It supersedes the fix/process guidance in
`cpu-spin-cfrunloop-thread-2026-10.md` (v1.0) and the review/amendments in
`cpu-spin-cfrunloop-thread-2026-10-v1.1-review.md` (v1.1). Those two are read-only context; nothing in them is edited.

Do not implement from this document alone without also running the verification protocol — the value of this
release is a measured on-device before/after, which CI cannot observe.

## Root cause (one paragraph)

`HandsOffCore::start_cfrunloop_thread` (`src/lib.rs:299`) runs a background thread that calls
`CFRunLoop::run_in_mode(DefaultMode, 500ms, false)` in a loop. That returns `kCFRunLoopRunFinished` **immediately**
when the run loop has no sources/timers/observers — it does *not* sleep for the duration. The thread's own run loop
has zero sources (the event-tap source is added to the **main** thread's loop at
`src/input_blocking/event_tap.rs:412` via `CFRunLoop::get_current()`). So every iteration returns instantly and the
loop re-spins as fast as CoreFoundation can do its mode bookkeeping → one full core burned whenever the tray is
enabled. The thread is dead weight: it services nothing. **Fix: delete it entirely.**

## Safety nets (why deletion is safe)

- **Tap is serviced by the main loop.** The tap source is owned by the main thread's run loop, which tao's
  `[NSApplication run]` pumps continuously. Input blocking already works with this background loop empty — proven by
  the field sample (main thread parked in `mach_msg` 99.9% while the spin thread burned the core).
- **No spin multiplication.** `mpsc::Sender` stays alive in the struct field for the thread's lifetime, so
  `try_recv()` always returns `Err(Empty)` (never `Err(Disconnected)`) — the shutdown check holds. The `is_some()`
  guard + every disable/reenable path routing through `stop_event_tap` (which always stops the thread) caps the
  count at one. Sleep/wake does not accumulate threads either: the timeout re-enable path
  (`event_tap::reenable_existing_tap`, `event_tap.rs:439`) only calls `CGEventTapEnable` and never touches this thread.
- **No code reads the thread.** The `cfrunloop_thread` field is write-only (spawn/stop); nothing branches on its
  presence as logic. `grep cfrunloop tests/` → no matches.
- **One behavioral net, one revert rule.** The only unprovable-by-reading risk is wake-on-event latency on the main
  loop once the CPU is no longer kept artificially hot. If post-fix smoke ever shows input genuinely *not* blocked on
  a fresh grant, **revert the PR — do not start re-adding run-loop pumps.**

## Files touched

| File | What |
|---|---|
| `src/lib.rs` | Delete the two thread fns, both call sites + comments, the field, the `None` init; trim 3 import lines |
| `src/constants.rs` | Delete the constant + doc; fix the dangling cross-ref at line 88 |
| `src/bin/handsoff-tray.rs` | Delete the now-false 3-line NOTE comment |
| `Cargo.toml` | Bump `[package].version` and `[package.metadata.bundle].version` to `0.9.1` |
| `Cargo.lock` | Regenerate (records `handsoff 0.9.1`) |

## Implementation steps

### 1. `src/lib.rs` — imports (3 edits)

**1a.** Constants use-block, drop `CFRUNLOOP_POLL_INTERVAL_MS` (line 18-22):

```rust
// before
use constants::{
    AUTO_LOCK_CHECK_INTERVAL_SECS, AUTO_UNLOCK_CEILING_SECONDS, AUTO_UNLOCK_CHECK_INTERVAL_SECS,
    BUFFER_RESET_CHECK_INTERVAL_MS, CALLBACK_TELEMETRY_INTERVAL_SECS, CFRUNLOOP_POLL_INTERVAL_MS,
    PERMISSION_CHECK_INTERVAL_SECS,
};
// after
use constants::{
    AUTO_LOCK_CHECK_INTERVAL_SECS, AUTO_UNLOCK_CEILING_SECONDS, AUTO_UNLOCK_CHECK_INTERVAL_SECS,
    BUFFER_RESET_CHECK_INTERVAL_MS, CALLBACK_TELEMETRY_INTERVAL_SECS, PERMISSION_CHECK_INTERVAL_SECS,
};
```

**1b.** Deleting the channel machinery and both thread fns removes the only `mpsc`/`Sender` user. If clippy flags it,
delete `lib.rs:28` `use std::sync::mpsc::{self, Sender};`.

**1c.** Deleting the `cfrunloop_thread` field removes the only `JoinHandle` user. If clippy flags it, edit `lib.rs:30`
`use std::thread::{self, JoinHandle};` → `use std::thread::{self};`. Keep `self` — `thread::spawn` is still used by
the buffer-reset / auto-lock / hotkey / auto-unlock / permission threads in this file.

> Build after 1a first; let clippy decide 1b/1c. Do not pre-delete imports speculatively.

### 2. `src/lib.rs` — struct field and initializer

Remove the field and its doc comment (`lib.rs:74-75` area):

```rust
// before
    /// CFRunLoop thread handle and shutdown channel
    cfrunloop_thread: Option<(JoinHandle<()>, Sender<()>)>,
    /// State pointer passed to event tap (for cleanup)
    event_tap_state_ptr: Option<*mut std::ffi::c_void>,
// after
    /// State pointer passed to event tap (for cleanup)
    event_tap_state_ptr: Option<*mut std::ffi::c_void>,
```

Remove the initializer (`lib.rs:93`):

```rust
// before
            cfrunloop_thread: None,
            event_tap_state_ptr: None,
// after
            event_tap_state_ptr: None,
```

### 3. `src/lib.rs` — delete the call sites + stale comments

In `start_event_tap` remove the first two statements (`lib.rs:362-364`):

```rust
// before
        // Start CFRunLoop thread first (required for event tap)
        self.start_cfrunloop_thread();

        // Create event tap
// after
        // Create event tap
```

In `stop_event_tap` remove the trailing statements (`lib.rs:401-402`):

```rust
// before
        // Stop CFRunLoop thread
        self.stop_cfrunloop_thread();
    }
// after
    }
```

### 4. `src/lib.rs` — delete both thread functions

Delete `start_cfrunloop_thread` including its doc comment (`lib.rs:297-338`) and `stop_cfrunloop_thread`
(`lib.rs:340-358`). The function bodies (verbatim, so the delete is unambiguous):

```rust
    /// Start CFRunLoop in a background thread
    /// Required for event tap to receive events
    fn start_cfrunloop_thread(&mut self) {
        if self.cfrunloop_thread.is_some() {
            warn!("CFRunLoop thread already running");
            return;
        }

        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            info!("CFRunLoop thread started");
            use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};

            loop {
                // Run the loop for 0.5 seconds, then check for shutdown
                let result = unsafe {
                    CFRunLoop::run_in_mode(
                        kCFRunLoopDefaultMode,
                        Duration::from_millis(CFRUNLOOP_POLL_INTERVAL_MS),
                        false,
                    )
                };

                // Check if shutdown requested
                if shutdown_rx.try_recv().is_ok() {
                    info!("CFRunLoop thread received shutdown signal");
                    break;
                }

                // Log result for debugging (will be removed later if too verbose)
                if result != CFRunLoopRunResult::TimedOut {
                    log::trace!("CFRunLoop run_in_mode returned: {:?}", result);
                }
            }

            info!("CFRunLoop thread stopped");
        });

        self.cfrunloop_thread = Some((handle, shutdown_tx));
        info!("CFRunLoop thread spawned successfully");
    }

    /// Stop CFRunLoop background thread
    fn stop_cfrunloop_thread(&mut self) {
        if let Some((handle, shutdown_tx)) = self.cfrunloop_thread.take() {
            info!("Stopping CFRunLoop thread");

            // Send shutdown signal
            if let Err(e) = shutdown_tx.send(()) {
                warn!("Failed to send shutdown signal to CFRunLoop thread: {}", e);
            }

            // Wait for thread to finish (with timeout)
            match handle.join() {
                Ok(()) => info!("CFRunLoop thread stopped successfully"),
                Err(e) => warn!("CFRunLoop thread panicked: {:?}", e),
            }
        } else {
            warn!("CFRunLoop thread not running, nothing to stop");
        }
    }
```

> The `use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};` on the third line is
> scoped *inside* the closure and dies with the deletion — no separate global-import cleanup for it.

### 5. `src/constants.rs` — delete constant + fix dangling cross-ref

**5a.** Remove the constant and its doc comment:

```rust
// before (delete these 6 lines)
/// CFRunLoop polling interval for event processing.
/// Unit: milliseconds
/// Recommended range: 100-1000 (lower = more responsive, higher = less CPU)
pub const CFRUNLOOP_POLL_INTERVAL_MS: u64 = 500;
```

**5b.** Line 88 references the deleted constant; make it standalone:

```rust
// before
/// Recommended range: 100-1000 (same as CFRUNLOOP_POLL_INTERVAL_MS)
// after
/// Recommended range: 100-1000 (lower = more responsive, higher = less CPU)
```

### 6. `src/bin/handsoff-tray.rs` — delete now-false NOTE comment

```rust
// before (delete these 3 lines)
    // NOTE: CFRunLoop thread is now managed by HandsOffCore
    // It starts when event tap is created and stops when event tap is destroyed
    // This eliminates the zombie CFRunLoop connection that caused WindowServer issues
```

### 7. `Cargo.toml` + `Cargo.lock` — version bump 0.9.0 → 0.9.1

- `Cargo.toml:3`  `[package].version` → `"0.9.1"`
- `Cargo.toml:51` `[package.metadata.bundle].version` → `"0.9.1"` (populates `CFBundleShortVersionString`/
  `CFBundleVersion` via `cargo bundle`; no workflow override exists, so this is what the built app self-reports)
- Run a build so `Cargo.lock` regenerates to `handsoff 0.9.1`.

### 8. Build / lint / test

- `cargo zigbuild --release` (Linux dev host; SDKROOT setup per repo memory) — cross-compile for macOS.
- `cargo clippy` — **must be clean**; this is the check that confirms no leftover unused import from steps 1b/1c.
- `cargo test` — runs on **macos-latest CI**, not locally on Linux.

## Git / release process

1. Branch: work on the existing `minimize_cpu_1005` (already at `main`'s tip, no divergence) or a short-lived fix
   branch off `main` — either is fine; do not fork unrelated history.
2. Issue first: open a GitHub issue with the v1.0 sample summary as evidence. Labels **`ready-for-agent` + `bug`**
   (the `bug` label makes the generated release notes categorize it under `### Fixed`; `ready-for-agent` alone lands
   in no category).
3. PR closing the issue. Keep the diff scoped to this plan (thread deletion + comment/constant/import cleanup +
   version bump). **No** wakeup-consolidation, **no** behavior changes.
4. CI green on the PR (build + clippy + `cargo test` on macos-latest).
5. **Run the verification protocol (below) on a locally-built bundle. Do not merge on CI green alone.**
6. Merge to `main` → CI green on the merge SHA → tag `v0.9.1` at that SHA → `release.yml` builds + publishes the DMG.
7. **CHANGELOG is release-owned:** the workflow *prepends* a generated `## [0.9.1] - <date>` block from PR titles.
   Do **not** hand-write a `## [0.9.1]` header pre-tag (it would duplicate). Carry the before/after numbers and the
   re-grant note in the **PR body**; append prose to CHANGELOG **after** release if wanted.

## Verification protocol (user-run, on-device — agent GUI is blocked per AGENTS.md)

> The baseline is unrecoverable once the fix-build replaces the running binary. Capture before first.

1. **Build release** via `cargo zigbuild` (SDKROOT per repo memory); copy the binary into a locally-built
   `/Applications/HandsOff.app` bundle, bundle id `handsoff-tray.handsoff` unchanged, run from that TCC-approved path.
2. **Fresh grant** (the fix build's CDHash invalidates the old grant, per ADR 0001):
   `tccutil reset Accessibility handsoff-tray.handsoff`, relaunch from the bundle, re-grant. Skipping this makes the
   functional smoke false-fail "input not blocked."
3. **Baseline (pre-fix), before step 1's build overwrites the running app:**
   - `sample handsoff-tray 5 -file ~/tmp/hs-sample-before.txt` while enabled-idle.
   - Activity Monitor %CPU enabled-idle (expect ~100% / one core) and the Energy tab — record both.
4. **Functional smoke (post-fix build):** Lock → input genuinely blocked → unlock via passphrase → Disable/Reenable
   still work. This is the pass/fail for behavioral correctness.
5. **After measurement, same protocol as baseline:**
   - `sample handsoff-tray 5 -file ~/tmp/hs-sample-after.txt` enabled-idle — expect **no** `start_cfrunloop_thread`
     hot thread; main thread still parked in `mach_msg`.
   - Activity Monitor %CPU enabled-idle — target **<1%, ~0**; record the actual number.
   - Energy tab — record (paired with baseline; feeds the deferred wakeup decision).
6. **Record the before/after triples (sample headline, %CPU, Energy) in the PR body.**

## Rollback (document-only)

If on-device smoke shows input blocking genuinely regressed on a fresh grant:

1. `git revert <merge-sha>` on `main` — the diff is a pure deletion, so the revert is a pure restore.
2. Delete the `v0.9.1` tag locally + remote; do not re-tag a new SHA as `v0.9.1` (release assets already attach).
3. If the release already published, cut the revert as `v0.9.2` rather than mutating `v0.9.1`.

## Acceptance criteria

- [ ] Release build green with thread, field, channel machinery, constant, both call sites, stale comments
      (`handsoff-tray.rs:244-246`, `constants.rs:88` cross-ref) removed; clippy clean.
- [ ] `Cargo.toml` both version fields `0.9.1` + `Cargo.lock` regenerated (app self-reports 0.9.1).
- [ ] `cargo test` green on macos-latest CI.
- [ ] Issue/PR labeled `bug` (+ `ready-for-agent`).
- [ ] Functional smoke passes on a **fresh** Accessibility grant.
- [ ] After %CPU enabled-idle <1%; `hs-sample-after.txt` shows no `start_cfrunloop_thread` hot thread.
- [ ] Before/after triples + re-grant note in the PR body; prose appended to CHANGELOG only post-release.
- [ ] PR merged to `main`, CI green on merge SHA, `v0.9.1` tagged at that SHA.
- [ ] Release/PR body tells users to re-grant Accessibility after install (ADR 0001 CDHash change).

## Explicitly out of scope (deferred)

Wakeup consolidation — buffer-reset parking, tray `WaitUntil` → event-driven, ~8 wakeups/s remaining. Decide **after**
the Energy number lands. Do not bundle into v0.9.1: the point release's value is a clean before/after CPU attribution.
