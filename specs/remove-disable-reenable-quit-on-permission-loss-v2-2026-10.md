# Remove Disable/Reenable tray items; quit immediately on permission loss

Source plan: `specs/remove_disable_reeanble_completely.md` (kept unmodified for history).
This plan supersedes it. Delta vs source: (a) the 30 s cancelable grace-quit is REPLACED
by an immediate quit with a final explanatory notification (user decision 2026-10-05);
(b) two defects found in review are fixed by design here rather than deferred to
contingencies — the startup-missing path must NOT quit, and RestartFailed must quit
even though permissions are present in that case (a bare "re-check permissions before
quitting" would wrongly keep the app alive tapless forever); (c) docs scope broadened
to README.md and the Help window's menu-items table and stuck-lock prose (the source
plan's "Use Reenable" grep misses them); (d) CHANGELOG.md is NOT hand-edited (the
release workflow's changelog bot prepends `## [${VERSION}] - ${DATE}` itself —
release.yml "Update CHANGELOG.md" step re-heads the file with `tail -n +3`; a
hand-written 0.10.0 block would end up duplicated, the same mistake already rescinded
for 0.9.1).

## Context

With the dead CFRunLoop spin thread gone (0.9.1), the "minimal CPU mode" rationale for
the tray's Disable/Reenable pair is gone; user wants the two items removed for menu
simplicity. Verified consequences accepted with the user:

- Tap is always-on when permissions exist.
- Auto-unlock backoff (60 min → 24 h) + reboot are the only stuck-Lock recoveries.
- On runtime permission REVOCATION the app unlocks and QUITS IMMEDIATELY (no grace),
  after one final notification explaining why and how to fix it.
- Tap restart FAILURE (RestartFailed) quits immediately too (same single notification
  pattern, failure wording).
- Startup with permissions already missing keeps the app running tapless
  (wizard/re-grant flow needs a live process) and auto-restarts the tap when granted —
  that path already exists and MUST NOT quit.

Why immediate instead of the 30 s grace: the grace version's cancelability had a real
race (grant lands while the monitor's 15 s poll and the tray's 500 ms poll are both
mid-flight → quit despite a successful re-grant), and a correct fix needed an
expiry-time restart attempt plus permission recheck. The user chose to drop the
complexity: quit immediately, tell the user why in the notification, let them re-grant
and relaunch. Launch Agents / login items can re-launch automatically if the user sets
one up; the app does not self-relaunch.

Scope boundary: menu-item + disabled-state eradication plus quit-on-revocation. The
sleep/wake tap-timeout recovery APIs (`reenable_event_tap`, `request_reenable_event_tap`,
`should_reenable_event_tap_and_clear`, `mark_reenable_completed`,
`event_tap::reenable_existing_tap`) are a separate lifecycle and MUST survive untouched.

Build/test constraint (repo AGENTS.md): build on Linux via cargo zigbuild (~/.local/zig,
SDK ~/.local/macos-sdk, `SDKROOT`); tests only run on macos-latest CI. Manual smoke
requires a release build copied into a locally-built `/Applications/HandsOff.app` (TCC
pins the grant to the resolved binary path).

## Approach

Order: (1) core state + lib cleanup, (2) tray menu/handlers/UX, (3) immediate-quit
wiring, (4) docs, (5) version. Steps 1–2 keep the build green independently; 3 depends
on 1; tests run after each of 1–3.

### 1. Core: delete the disabled state (src/lib.rs, src/app_state.rs, src/preferences.rs, src/constants.rs)

- `src/app_state.rs`: delete field `is_disabled` (~74-75) and its initializer (~101),
  methods `is_disabled()` (~528-531) and `set_disabled()` (~533-536). Verified: the only
  `set_disabled` writers are `disable()`/`enable()` in lib.rs.
- `src/app_state.rs`: `clear_lock_state()` (~275-281) has exactly two callers once the
  tray handlers go: `disable()` (lib.rs:407) and tests (app_state.rs:786-810,
  lifecycle_tests.rs:497-544). Delete the method AND its tests.
- `src/app_state.rs`: `reset_all()` + its caller chain is orphaned by the same deletion:
  `HandsOffCore::reset()` (lib.rs:289-291) is called only from `handle_reenable`
  (tray.rs:964), and `reset_all` is called only from `reset()`. Delete both methods and
  `reset_all`'s "User-initiated Reenable" doc comments. (Grep first to confirm no other
  callers appeared: `grep -rn "reset_all\|\.reset()" src/ tests/`.)
- `src/lib.rs`: delete `disable()` (396-424) and `enable()` (425-446). Their only
  callers are the deleted tray handlers (verified: `handle_disable` → lib.rs:933;
  `handle_reenable` → lib.rs:971).
- `src/lib.rs`: in the four background worker threads, delete the
  `if state.is_disabled() { ... }` skip blocks at ~553, ~574, ~615, ~657. In the
  permission monitor loop delete the `if state.is_disabled() { ... }` skip at ~723-725.
  Do NOT touch the revoke/restore/missing branches (701-790) except notification copy
  (step 3).
- `src/lib.rs`: `Drop::drop` calls `stop_event_tap()` (~803) — keep; hotkey unregister
  in Drop stays.
- `src/preferences.rs`: `MenuState` — delete fields `disable_enabled` and
  `reenable_enabled` (48-50) and doc comments; `menu_state` signature becomes
  `pub fn menu_state(is_locked: bool, has_permissions: bool) -> MenuState` with
  `lock_enabled: has_permissions && !is_locked`; keep `preferences_enabled: true`,
  `change_passphrase_enabled: !is_locked`, `reset_enabled: !is_locked`.
- `src/constants.rs`: delete `POLL_INTERVAL_DISABLED_SECS` (line 79) and its doc
  comment (lines 75-77). `POLL_INTERVAL_ENABLED_MS` stays.

### 2. Tray: remove items, handlers, disabled UX, Help text (src/bin/handsoff-tray.rs)

- Delete `disable_item`/`reenable_item` creation (253-255), appends (267-270), id
  clones (306-307), and all tuple threading of
  `disable_id`/`reenable_id`/`disable_item`/`reenable_item` (~323-372, 530-553, 549).
  The ids/items tuples shrink from 10 to 8 elements; update the tuple types in
  `run_session`'s signature accordingly (502-525).
- Deferred-dispatch (~566-613): remove `disable_id`/`reenable_id` from the gate match;
  the gate whitelist becomes lock_id, check_updates_id, quit_id.
- Live `MenuEvent` dispatch (~643-754): delete the `disable_id`/`reenable_id` branches;
  the combined gate condition `event_id == lock_id || event_id == disable_id ||
  event_id == reenable_id` (~646) becomes `event_id == lock_id`, gated on
  `flags.lock_enabled`.
- Delete `handle_disable` (~930-947) and `handle_reenable` (~949-1016) entirely.
- Poll cadence (~624-633): replace the `is_disabled` branch with a single
  `Duration::from_millis(POLL_INTERVAL_ENABLED_MS)`.
- `menu_state(...)` call sites (~392, ~415, ~570, ~651, ~796): drop the `is_disabled`
  argument.
- Icon (~820-850): delete `was_disabled` tracking (~330, ~344, ~554, ~582) and the
  `create_icon_disabled()` branch — icon becomes locked/unlocked only.
- Delete `create_icon_disabled()` (~1612-1616) and `assets/tray_disabled.png`.
  Check `assets/README.md` and any bundler resource lists for `tray_disabled`
  references.
- `build_tooltip`/`push_status` (~1369-1425): drop the `is_disabled` parameter and the
  `STATUS: DISABLED` branch (~1407-1410). NO PERMISSIONS branch last line becomes
  `"Input blocking resumes automatically once granted\n\n"` (drop the "Then use
  Reenable menu to restart" line).
- `TrackedState` tuple (439): remove `was_disabled` — becomes a 4-tuple
  `(bool, String, Instant, bool)`.
- `build_help_text` Menu-items table (~1517-1527): delete the "Disable" and "Reenable"
  rows. Stuck-lock prose that names Reenable must be reworded (the source plan's
  `"Use Reenable"` grep NEVER hits these — they say "Reenable" without "Use"):
  - ~1480-1482 (passphrase section): `"(The Reenable menu item can't help — it only
    turns HandsOff back on after Disable, and the menu can't be clicked while
    locked.)"` → drop the parenthetical entirely (the sentence before it already says
    "two ways back in: your passphrase, or rebooting your Mac").
  - ~1561-1563 (troubleshooting section): `"Reenable can't help — it only turns
    HandsOff back on after Disable, and the menu is unreachable while locked."` →
    delete the sentence ("Your only ways back in: type your passphrase, or reboot your
    Mac." already covers it).

### 3. Quit immediately on permission loss / restart failure

Design: the tray main thread ALONE decides to quit (background threads never call
`process::exit` for this — a monitor-thread exit would skip the final notification and
race the event loop). The quit is a plain `std::process::exit(0)` from the tray poll
callback, which is the established Quit mechanism (quit handler, tray.rs:747-753 —
deliberately skips destructors because dropping the tao event loop's CFRunLoop observers
panics). A notification fires BEFORE the exit call, followed by a short fixed delivery
gap (see "Notification delivery" below) — `notify-rust` posts to `usernoted`
asynchronously and an immediate exit can lose the notification, which would silently
defeat the whole "user knows why it quit" requirement.

Distinguishing runtime revocation from startup-missing (the source plan's critical
bug): the flag `should_stop_event_tap` is set BOTH by the monitor's runtime-revoke
branch (lib.rs:758) AND by its startup-missing branch (lib.rs:702) AND by the tap
callback's `DISABLED_BY_USER_INPUT` arm (event_tap.rs:243). Arming the quit in
`service_tap_lifecycle`'s `TapStopped` arm unconditionally would quit 30 s into every
permission-less launch — including fresh installs, the re-grant-degraded startup
(tray.rs:168-174), and TCC-propagation lag right after a completed wizard. So:

- `src/lib.rs` `service_tap_lifecycle` stop branch: capture
  `let tap_was_running = self.event_tap.is_some()` BEFORE `stop_event_tap()` consumes
  the field (lib.rs:66 already holds `event_tap: Option<CGEventTapRef>` — no new
  AppState flag; parallel state that can desync is exactly what this avoids). Before
  stopping, `Some` means the tap was actually enforcing; `None` means it was never
  started (startup-missing) — do not quit in that case. The monitor thread keeps
  requesting the stop unconditionally (lib.rs:702/758, event_tap.rs:243); the ONE
  decision point is in the core.
- `src/lib.rs`: add a pure decision helper so the guard is unit-testable without a
  live tap: `fn stopped_event(tap_was_running: bool) -> TapLifecycleEvent`; the
  service loop calls it with `self.event_tap.is_some()` captured before the stop.
  (CI cannot construct `event_tap = Some` — `CGEventTapCreate` returns `None`
  without the accessibility grant — so the linchpin logic must live on the bare
  bool, not behind a tap-holding precondition.)
- `TapLifecycleEvent`: change the `TapStopped` variant to
  `TapStopped { tap_was_running: bool }`. `TapStopped { false }` = permission missing
  with no tap currently held → tray shows status, keeps running (startup-missing path —
  unchanged behavior). `TapStopped { true }` = runtime revocation with a live tap →
  tray notifies + quits.
  - Residual window, accepted by design: after the first stop, `event_tap` is `None`,
    so a revoke that fires while no tap is held (granted-then-revoked within the ~15 s
    monitor + 500 ms poll window between a grant-triggered restart and the next
    revocation) would report `TapStopped { false }` → no quit. The exposure is one
    bounded monitor interval in an already-degraded permission-flapping scenario; the
    app stays running tapless with the NO PERMISSIONS tooltip, which is the
    startup-missing behavior, not a silent state. A `tap_ever_started` latch in
    AppState would close it but adds parallel state that can desync; rejected.
- `src/bin/handsoff-tray.rs` poll, in the `service_tap_lifecycle` match (~760-791):
  - `TapStopped { tap_was_running: true }` → notification:
    summary `"HandsOff - Permissions Revoked"`, body
    `"Accessibility permissions were revoked.\nInput blocking was stopped - your
    keyboard and mouse work normally now.\n\nHandsOff is quitting. To restore input
    blocking: re-grant Accessibility permission (System Settings > Privacy & Security
    > Accessibility) and relaunch HandsOff."`, then the delivery gap (below), then
    `std::process::exit(0)`.
  - `TapStopped { tap_was_running: false }` → log-only (tray keeps running, tapless —
    the startup-missing path; NO PERMISSIONS tooltip already covers UX).
  - `RestartFailed` → notification (summary `"HandsOff - Restart Failed"`, body
    `"Failed to restart input blocking: {e}\n\nHandsOff is quitting. Please relaunch
    to restore input blocking."`), then the delivery gap (below), then
    `std::process::exit(0)`.
    IMPORTANT: do NOT gate this exit on a permission re-check — in the RestartFailed
    case permissions ARE present (the failure is tap creation), so an arm-agnostic
    "quit only if still untrusted" check would cancel the quit and leave the app
    running tapless forever. The RestartFailed arm quits unconditionally.
  - `Restarted` → existing notification, no exit. (No deadline to clear — there is no
    grace.)

**Notification delivery before exit**: `notify-rust` on macOS posts to `usernoted`
asynchronously — `.show()` returning does NOT mean the notification is on screen, and
`std::process::exit(0)` immediately after can drop it. The pre-exit notification is the
entire answer to "the user knows why it quit", so both quit arms insert a fixed
delivery gap:

```rust
let _ = notify_rust::Notification::new() /* ... */ .show();
std::thread::sleep(std::time::Duration::from_millis(NOTIFICATION_EXIT_DELAY_MS));
std::process::exit(0);
```

- `src/constants.rs`: add `pub const NOTIFICATION_EXIT_DELAY_MS: u64 = 1000;`
  (1 s: comfortably covers the usernoted round-trip; short enough to read as
  "immediate quit" next to the ~15 s detection latency that dominates).
- Placement: the sleep runs INSIDE the tray poll callback after the notification,
  before exit. It blocks the run loop for 1 s on a path that ends the process — no
  other work is starved.
- Verification is macOS-side only (smoke 6): the notification must be observed on
  screen before the process dies. If 1 s proves insufficient on slow machines, bump
  the constant; do not remove the gap.
- `src/lib.rs` monitor-loop notification copy:
  - "Permissions Missing" (startup, ~708): body → `"Accessibility permissions are
    missing.\nInput blocking is not active.\n\nInput blocking resumes automatically
    once permissions are granted."` (this branch fires at startup and is the
    keep-running path — no quit wording).
  - "Permissions Revoked" (~765): the monitor's own notification becomes redundant
    with the tray's quit notification — DELETE the monitor notification in the revoke
    branch (lib.rs:760-768) so the user sees exactly one notification, the tray's,
    which carries the why-and-how-to-fix copy. (The unlock-if-locked logic and
    `request_stop_event_tap()` stay.)
- Startup-missing must not quit — verified by construction: `main` skips
  `start_event_tap()` without permissions (tray.rs:231-238), so `event_tap` is `None`
  when the monitor's startup branch requests the stop → `TapStopped { false }` → no
  exit. Same for the ReGrant-degraded startup (tray.rs:168-174) and for
  TCC-propagation lag after a completed wizard.
- `EXIT_GRACE_SECS` / exit-deadline AppState trio from the source plan: NOT built —
  no grace, no deadline, nothing to cancel.

### 4. Docs

- New `docs/adr/0005-remove-disable-reenable-menu-items.md`: records —
  Disable/Reenable menu items and the `is_disabled` state removed (menu simplicity
  post-CPU-fix); tap is always-on when permissions exist; the "unguarded anti-lockout
  escape hatch" framing in ADR-0003 §3 is corrected with the qualification the review
  established: Reenable was NEVER reachable during a healthy Locked state (live tap
  blocks every mouse event type — event_tap.rs:276-353, handle_mouse_event returns
  true unconditionally, input_blocking/mod.rs:136-142), and in a dead-tap-while-locked
  window the passphrase is equally unusable, so Reenable was only ever the FASTEST
  recovery there, not a bypass of a working lock; stuck-Lock recovery is now
  auto-unlock backoff (60 min→24 h) or reboot (user-accepted). Runtime permission
  revocation and tap-restart failure now quit IMMEDIATELY with a final notification
  explaining how to fix (user decision 2026-10-05, replacing the 30 s cancelable grace
  from the source plan — the grace had a grant-lands-mid-poll race and was judged not
  worth the machinery). Startup with missing permissions unchanged: app stays running
  for the wizard/re-grant flow, tap auto-restarts when granted. Restart failure is
  one-shot with no retry — accepted. Mark ADR-0003 superseded-by-0005 in its header,
  keeping its N2/N5 gating content that still applies (single gating authority,
  Change Passphrase/Reset refused while locked).
- `CONTEXT.md` glossary: delete the `**Disable**` entry (19-21) and `**Reenable**`
  entry (23-25) plus the `_Avoid_: reenable` line (29); delete "Reenable" mentions in
  the `**Reset**` entry's `_Avoid_` note; add a short "Permission loss" glossary term:
  revoked permissions (or a failed tap restart) quit the app immediately after a
  notification; re-grant and relaunch.
- `README.md` (missed by the source plan): line 126 drop "Disable"/"Reenable" from the
  menu-items list; delete the `**Disable**` (127) and `**Reenable**` (128) bullet
  definitions; line 122 "unlocked/disabled: white" → "unlocked: white"; add a short
  "Permission loss" bullet mirroring the new behavior.

### 5. Version

- Bump `0.9.1` → `0.10.0` in `Cargo.toml` (two occurrences: lines 3 and 51).
  `Cargo.lock` via `cargo update -w`.
- CHANGELOG.md: do NOT hand-edit (the release workflow's "Update CHANGELOG.md" step
  prepends `## [${VERSION}] - ${DATE}` from the generated release notes — release.yml
  re-heads the file with `tail -n +3`; a hand-written 0.10.0 block would be duplicated
  under the generated one; this exact instruction was already rescinded for 0.9.1,
  specs/cpu-spin-cfrunloop-thread-impl-2026-10-v1.2.md §7). Carry the removal +
  quit-on-revocation prose in the PR body; the changelog bot picks up the PR labels.

## Critical files & anchors

- `src/lib.rs` — `disable()`/`enable()`/`reset()` deletion (275-446); worker-skip
  blocks (553/574/615/657/723); `service_tap_lifecycle` (461-502) + `TapLifecycleEvent`
  (50-61) redesign; monitor notification copy (695-790).
- `src/bin/handsoff-tray.rs` — menu construction/dispatch (253-310, 530-613,
  643-754); handlers (930-1016); icon/tooltip (820-850, 1369-1425); Help text
  (1475-1563, table 1517-1527); poll cadence (624-633); quit wiring in the
  `service_tap_lifecycle` match (~760-791).
- `src/preferences.rs` — `MenuState`/`menu_state` (46-68).
- `src/app_state.rs` — `is_disabled` removal; `reset_all`/`clear_lock_state` orphan
  deletion + their tests (786-810). No new fields.
- `src/constants.rs` — `POLL_INTERVAL_DISABLED_SECS` deletion (75-79);
  `NOTIFICATION_EXIT_DELAY_MS` addition.
- `tests/lifecycle_tests.rs` — gating matrix rewrite (57-151, 497-544).

## Verification

Linux-side (can run here):
1. `SDKROOT=~/.local/macos-sdk cargo zigbuild --target x86_64-apple-darwin` (mirror the
   release matrix's triple — read `.github/workflows/release.yml` first) — must compile.
2. `grep -rn "is_disabled\|handle_disable\|handle_reenable\|disable_enabled\|reenable_enabled\|POLL_INTERVAL_DISABLED\|tray_disabled\|Use Reenable" src/ tests/ assets/ README.md`
   returns ONLY the tap-timeout recovery APIs (`reenable_event_tap`,
   `request_reenable_event_tap`, `reenable_existing_tap`, `should_reenable_event_tap*`,
   `mark_reenable_completed`). Note: this grep intentionally also covers README.md and
   bare "Reenable"/"Disable" strings in help text via a follow-up manual read of
   `build_help_text` output — the source plan's `"Use Reenable"`-only grep passed green
   while the Help window still documented the removed items.
3. New unit tests in `src/lib.rs` test module (macOS-gated, run on CI):
   - CI cannot construct a live tap (`event_tap = Some` needs `create_event_tap` →
     `CGEventTapCreate`, which returns `None` without an accessibility grant), so the
     stop-arm decision MUST be extracted into a pure, CI-testable helper:
     `fn stopped_event(tap_was_running: bool) -> TapLifecycleEvent { TapStopped { tap_was_running } }`
     — the service loop calls `stopped_event(self.event_tap.is_some())` before
     `stop_event_tap()`. Unit-test BOTH payloads off the bare bool:
     `stopped_event(false) == TapStopped { tap_was_running: false }` (startup-missing →
     no quit) and `stopped_event(true) == TapStopped { tap_was_running: true }`
     (runtime revocation → quit) — the linchpin guard, testable without a grant.
   - End-to-end flag-consumption test (CI-runnable, no grant needed):
     `service_tap_lifecycle` with `should_stop_event_tap` set and `event_tap == None`
     → returns `TapStopped { tap_was_running: false }` and the flag is cleared.
   - `request_start_event_tap` with tap creation failing (CI: no permissions →
     `restart_event_tap` bails) → `RestartFailed(_)` (quit arm).
   - `request_start_event_tap` with a live tap succeeding cannot run on CI (needs the
     grant) — covered by macOS smoke 5/7 instead; keep any existing CI-tolerant
     assertion pattern (`Restarted|RestartFailed`) if one exists.
4. `tests/lifecycle_tests.rs` rewritten for the 2-arg `menu_state`: locked →
   lock/change_passphrase/reset false, preferences true; unlocked+perms → lock true;
   no-perms → lock false. Delete the `clear_lock_state` re-arm tests (497-544) with
   the method. Push and watch macos-latest CI (watch by headSha, not --commit).

macOS-side (user, per AGENTS.md GUI limits — cannot be agent-verified):
5. Build release, copy into locally-built `/Applications/HandsOff.app` (TCC path
   requirement), run with `RUST_LOG=debug`: Lock → input blocked → passphrase unlock
   works (regression).
6. Revoke Accessibility in System Settings while UNLOCKED: expect ONE notification
   ("revoked" + quit wording + how to fix) that is VISIBLE ON SCREEN BEFORE the
   process dies (the `NOTIFICATION_EXIT_DELAY_MS` 1 s gap exists exactly for this —
   usernoted delivery is asynchronous and an instant exit can drop it), then the tray
   quits (detection is dominated by the monitor's 15 s
   `PERMISSION_CHECK_INTERVAL_SECS`; once detected, exit follows the 1 s gap).
7. Startup without grant (fresh `tccutil reset Accessibility
   handsoff-tray.handsoff` + relaunch): expect app STAYS RUNNING, NO PERMISSIONS
   tooltip, NO notification-driven quit; granting restarts the tap automatically.
   This is the regression the source plan would have shipped broken: its
   `TapStopped` arm armed the quit unconditionally, and the monitor's startup-missing
   branch (lib.rs:702) sets the same stop flag.
8. Restart-failure path is hard to smoke on demand (needs a live-tap creation failure
   with permissions present); rely on the unit test in step 3 and code review of the
   `RestartFailed` arm. Do NOT soften this arm with a permission re-check — that would
   break the accepted quit behavior.

## Assumptions & contingencies

- If grep finds callers of `reset_all`/`HandsOffCore::reset()` beyond
  `handle_reenable` and the tests, keep the method and delete only the call site.
- If reading `self.event_tap.is_some()` inside the stop branch hits a borrow conflict
  (the field is consumed by `stop_event_tap` on `&mut self`), capture the bool first
  into a local — no state change is required, only statement order.
- `cargo zigbuild` target triple: read `.github/workflows/release.yml` and mirror its
  macOS target exactly.
- The source plan's "Assumptions & contingencies" `initial_permission_check_done`
  idea is subsumed by the `tap_was_running` payload design — the guard lives where
  the stop is serviced, not in the monitor, so the monitor's startup branch needs no
  change beyond notification copy.
