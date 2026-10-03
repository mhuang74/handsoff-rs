# Plan: UI Consolidation — Flow Runner, Menu-Drain Fix, Alert Retirement

> **SUPERSEDED by `specs/plan-ui-consolidation-2026-10-v2.md`** (same-day review).
> v1 contains factual errors corrected in v2: wrong PR base (main is tree-identical
> to update_readme, not 0.8.5), wrong `confirm_reset` response constant
> (`NSModalResponseOK` never equals `runModal()`'s 1000/1001 — and shipped
> `wizard.rs:499` carries this bug, making the #34 escape hatch dead), wrong
> deferred-click flush reasoning for the post-Reset wizard, and a verification
> plan impossible on the Linux dev box (tests cannot link there; 141 tests, not
> 142). Do not implement from this document.

**Date:** 2026-10-03
**Status:** Planned (decisions settled via grilling session 2026-10-03; tracking issues #39 (fixes), #40 (refactor))
**Origin:** `docs/assessment-ui-stack-2026-10.md` verdict — keep the no-framework UI stack, fix its structural cost.
**Base:** `update_readme` (0.9.0 state, `94803cc`) — NOT `main` (0.8.5). Both PRs target this branch.

---

## Goals

1. **Fix the wizard-flow menu-drain hole** — `run_wizard_macos` is the only window flow whose pump loop never calls `absorb_menu_clicks`; a tray-menu click during the post-Reset wizard queues invisible duplicate dialogs (issue #36 class).
2. **Retire the `osascript` alert path** — `show_alert`/`confirm_reset` shell out to `osascript` with blocking `.output()` on the main thread (H6 in `docs/assessment-interception-hang-risk-2026-10.md`); replace with native `NSAlert`, semantics preserved.
3. **Consolidate the five hand-rolled pump loops** into one flow-runner engine so the lifecycle invariants that produced #36 (close wedge, window leaks) are structural instead of per-flow review burden.

## Non-goals

- No behavior change of any kind (strict no-behavior-change policy; known quirks get separate tiny commits if ever).
- No UI framework adoption; the objc2-app-kit + tao stack stays (per the assessment verdict).
- No non-blocking alert redesign (H6's deeper half is a separate decision).
- No migration of `SIGNALS` from process-global to per-flow instances (possible follow-up; ObjC delegate surgery deferred).

## Decisions (grilling session, 2026-10-03)

| Decision | Choice |
|---|---|
| Consolidation depth | Full flow runner (`src/window_flow.rs`); wizard migrated last |
| Behavior policy | Strict no-behavior-change |
| Alert scope | All 14 sites; straight swap, semantics preserved |
| PR shape | Two PRs: fixes first (PR 1), refactor after (PR 2) |
| PR 1 base | `update_readme` (0.9.0), not `main` |
| Smoke testing | None for PR 1; one manual pass for PR 2 before merge |
| Alert de-risk | Ship as-is; contained diff, call sites untouched |
| Signals scope | Keep process-global `SIGNALS`; runner consumes it |
| Poller shutdown | Cancellation token (`AtomicBool`, set on every exit path) |
| Drain fix fate | PR 1's insertion is subsumed (deleted) by PR 2 — intentional |
| Release timing | Tag v0.9.0 after both PRs merge + PR 2 manual pass |
| PR 2 commits | Engine commit + one commit per migrated flow, each compiling green |
| Changelog | Both PRs append to the existing 0.9.0/Unreleased section; no new bumps |
| Migration order | Risk-ascending: Help → Preferences → re-grant → Change Passphrase → wizard |
| Smoke runner | User executes the checklist from the spec (also embedded in PR 2 body) |
| Artifacts | This spec + two GitHub tracking issues (#39 fixes, #40 refactor) |

## PR 1 — Fixes (tracking issue #39)

Single branch off `update_readme`, two commits:

### Commit 1: wizard-flow menu drain

- Insert `absorb_menu_clicks(&window, &app, &super::WINDOW_FLOW_MENU_IDS.lock())` into `run_wizard_macos`'s pump loop (the `run_return` closure at `wizard.rs:~839`), matching the four existing call sites (re-grant :1307, help :1634, preferences :1994, CP :2308).
- Verify the wizard's exit path flushes deferred clicks the same way the other flows do implicitly via the next tray session (`take_deferred_menu_events` dispatch at `handsoff-tray.rs:~567` runs at every session start — no explicit flush needed at flow exit; confirm by reading `run_session`'s deferred-dispatch block).
- Note: first-run wizard (before the tray icon exists) has no clicks to absorb; the fix matters only for the post-Reset wizard where the tray is live.
- **This insertion is expected to be deleted by PR 2** (drain moves into the runner engine). Stated here so the deletion reads as intentional. No double-drain bug is possible: draining twice is harmless, missing it is the bug.

### Commit 2: osascript → NSAlert retirement

- Rewrite `show_alert` (`handsoff-tray.rs:1314`) as `NSAlert::new(mtm)` + `setMessageText`/`setInformativeText` + single OK button + `runModal()`. Preserve the exact title/message strings of all 13 call sites — no wording changes.
- Rewrite `confirm_reset` (`handsoff-tray.rs:1204`) as two-button `NSAlert` ("Cancel" default / "Reset", `NSAlertStyle::Warning` to match `with icon caution`), returning `true` only on the Reset button (`NSModalResponseOK` = second button or explicit `runModal` response check — match osascript's "explicit confirm only" semantics).
- All 14 call sites untouched. The four session-loop sites (`:139`, `:189`, `:399`, `:422`) gain event servicing during the alert (H6 fix): `runModal` spins a nested modal loop, unlike `osascript .output()` which blocks flat.
- Threading: all sites are already main-thread (verified: session loop and handlers run on the main thread with `MainThreadMarker` available); `NSAlert` construction needs `MainThreadMarker` — use the existing pattern in `reset_permission_and_relaunch` (`wizard.rs:488`).
- Risk accepted (per grilling Q10): no unit test for the confirm gate; dialog behavior verified at release smoke.

**Verification (PR 1):** Linux zigbuild compiles both targets; existing 142 tests pass. No manual pass (accepted). Review focus: exact string preservation, button-default semantics in `confirm_reset`.

## PR 2 — Flow Runner Refactor (tracking issue #40)

### Engine design (`src/window_flow.rs`, new module)

```rust
// Sketch — exact signatures settled during implementation.

/// Per-flow background poller: closure runs on its own thread,
/// ticks until cancelled. The runner sets the token on EVERY exit
/// path (close, outcome, error) before returning.
pub struct PollerHandle { cancel: Arc<AtomicBool> }

/// What a step's poll does each pump tick (~100 ms cadence, as today).
pub enum StepPoll { Stay, Advance(StepId), Finish(FlowOutcome) }

/// Static description of one flow: window config, widgets, steps.
pub struct FlowSpec { /* window config, per-step render + poll closures, poller closure */ }

/// Runs one flow on the caller's tao event loop (same run_return model as today).
pub fn run_flow(spec: FlowSpec, event_loop: &mut EventLoop<WizardEvent>) -> Result<FlowOutcome>;
```

Engine responsibilities (each currently hand-copied in five loops):
1. `SIGNALS.begin_flow()` on entry (issue #36 story 16 semantics preserved verbatim).
2. Pump cadence: `ControlFlow::WaitUntil(now + 100 ms)` every tick.
3. **Close-poll in every phase:** check `SIGNALS.close_requested` on every tick of every step — the #36 invariant, now structural (one implementation, five users).
4. `window.orderOut(None)` + outcome write on every exit path (close, finish, error).
5. `absorb_menu_clicks` drain every tick (window-flow IDs consumed + re-front; immediate-action IDs deferred).
6. Poller lifecycle: spawn on entry, `cancel.store(true)` on every exit path, join not required (same as today: late writes land in cleared-by-`begin_flow` signals — but the token makes the window ≤1 tick instead of ≤500 ms).
7. Terminal-state rendering (`show_terminal_state` pattern) where the flow uses it.

What stays flow-specific (in `wizard.rs` as spec builders): step state machines, capture integration via `setup::capture_passphrase_headless` (nested CFRunLoop pump — the runner's 100 ms cadence must not disturb the pump-slice contract the close-abort relies on), stale-grant watchdog (30 s from Grant click), hotkey/timeouts form assembly, Help's static sections.

### Migration commits (each compiles green, strict no-behavior-change)

1. **Engine:** `src/window_flow.rs` with `run_flow` + poller slot; no flow migrated yet. Includes the invariant checklist as module docs (below).
2. **Help** (simplest: static window, one exit path) — proves the engine end-to-end.
3. **Preferences** — form + save outcome; first flow with interactive steps.
4. **Re-grant** — permission poll thread becomes the first poller-slot user; stale-grant watchdog moves into the poller closure.
5. **Change Passphrase** — trickiest pre-wizard migration: two capture phases run the nested CFRunLoop pump *inside* the runner's ticks; the runner must treat "capture in flight" as a step-poll state, not fight the nested loop.
6. **Wizard** — four steps + watchdog + capture; deletes PR 1's `absorb_menu_clicks` insertion (drain now in the engine) and the remaining four loop bodies' duplicated ceremony.

Postconditions: the five `run_return` loops in `wizard.rs` collapse into spec builders; `wizard.rs` shrinks by roughly the ceremony share (~400-600 lines) while `window_flow.rs` owns the invariants once.

### Runner invariant checklist (review gate; also module docs)

For every flow and every phase of every step:
- [ ] `close_requested` checked → flow exits, window `orderOut`, outcome written.
- [ ] Menu clicks drained every tick; window-flow clicks re-front the dialog; immediate clicks deferred.
- [ ] Poller token set on every exit path (no orphaned poll thread beyond one tick).
- [ ] `SIGNALS.begin_flow()` clears all per-flow signals on entry (stale-click leak impossible).
- [ ] Pump cadence stays ~100 ms; no blocking call inside the pump (capture's nested pump is the sanctioned exception and owns the thread while running).
- [ ] Exit path identical for close/cancel/success/failure (no window left visible — #36 leak class).

### Verification (PR 2)

- Linux zigbuild compile + 142 existing tests after every commit.
- New unit tests where pure: step state machine transitions driven by synthetic signals; poller cancellation (token set → closure exits within one tick); `begin_flow` clear-everything invariant. No GUI wiring tests (per spec #24 Testing Decisions).
- **One manual macOS pass before merge, executed by the user** at the TCC-approved `/Applications/HandsOff.app` path (per `AGENTS.md` re-grant path requirement):

**Smoke checklist (user executes; ~15 min):**
1. Launch app (no config or after Reset) → wizard opens automatically; close button exits cleanly at each phase (permission, capture-wait, capture, form, login-item).
2. Click Grant → status advances when permission granted; stale-grant path: wait 30 s+ without granting → escape hatch appears (needs stale TCC row; skip if not reproducible, note it).
3. Complete a capture (click Capture button first — capture must NOT auto-start), type ≥4 keys, Enter, retype, mismatch and match paths; reserved key shows named-key feedback.
4. Post-Reset wizard with tray live: click tray items during the wizard → no queued duplicate dialogs; immediate actions (Lock/Disable) deferred and fire after the wizard closes.
5. Preferences: open, change timeout, save, verify config.toml changed and core applied (notification shown).
6. Change Passphrase: verify-current phase rejects wrong passphrase; correct → new capture; close during wait phase exits cleanly (the #36 wedge); close during capture aborts within one tick; success shows in-dialog confirmation; window gone after every exit path.
7. Help window opens, static, closes cleanly.
8. Re-grant flow: open from tray menu, closes cleanly, tap resumes after grant.

## Rollback

- PR 1: each commit is an independent revert; the alert swap reverts to osascript cleanly (call sites untouched by design).
- PR 2: per-flow commits mean a misbehaving flow can revert to its hand-rolled loop without stranding the engine; worst case, revert the whole PR and PR 1's drain fix survives (it lives in the pre-refactor shape).

## Release

- After both PRs merge + PR 2 manual pass: merge `update_readme` → `main`, tag the merge SHA `v0.9.0` (tag exact SHA; release run appears ~10 s late per workflow memory). Both PRs' entries live in the 0.9.0 changelog section already appended by the PRs themselves.
- Release smoke then covers the deferred PR 1 manual verification (alert dialogs render, confirm_reset two-button semantics).

## Risks

| Risk | Mitigation |
|---|---|
| Refactor regresses lifecycle behavior invisible to tests (#36 class) | Per-flow commits + review checklist + mandatory manual pass before merge |
| Capture nested-pump contract broken by runner cadence | CP migration commit is where this is proven; wizard depends on the same pattern after |
| `confirm_reset` button semantics drift (Cancel default preserved?) | Review gate: explicit assert-default-button reading of the diff |
| Stale-grant path untestable locally | Checklist step 2 marked skippable-if-not-reproducible; release smoke retries |
