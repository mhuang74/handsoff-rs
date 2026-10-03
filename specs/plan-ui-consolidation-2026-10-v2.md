# Plan: UI Consolidation v2 — Flow Runner, Menu-Drain Fix, Alert Retirement, Escape-Hatch Bug Fix

**Date:** 2026-10-03 (reviewed and revised same day)
**Status:** Planned (v2 — supersedes `plan-ui-consolidation-2026-10.md`; decisions re-settled in review session 2026-10-03; tracking issues #39 (fixes), #40 (refactor))
**Origin:** `docs/assessment-ui-stack-2026-10.md` verdict — keep the no-framework UI stack, fix its structural cost.
**Base:** `main` (`51513f3`, 0.9.0). **Correction vs v1:** `main` is no longer 0.8.5 — it is tree-identical to `update_readme@94803cc` (both 0.9.0; `git diff update_readme main` is empty). Both PRs target `main` directly; the release-time `update_readme → main` merge is deleted as redundant.

---

## What changed from v1 and why (review findings)

| # | v1 claim | Finding (evidence) | v2 resolution |
|---|---|---|---|
| 1 | "`NSModalResponseOK` = second button" for `confirm_reset` | **Factually wrong.** `NSAlert::runModal()` returns `NSAlertFirstButtonReturn=1000` / `NSAlertSecondButtonReturn=1001` (objc2-app-kit 0.2.2, `generated/NSAlert.rs:30-36`); `NSModalResponseOK=1` (`generated/NSWindow.rs:59`) never matches | Pin exact implementation: Cancel first (default), Reset second, true iff `runModal() == NSAlertSecondButtonReturn` |
| 2 | (not in v1) | **Live shipped bug, same class:** `wizard.rs:499` — `runModal() != NSModalResponseOK` is always true, so the #34 stale-grant escape hatch ("Reset & Restart") always cancels; the escape hatch is dead code | New PR 1 Commit 3: minimal constant fix (`NSAlertFirstButtonReturn`) |
| 3 | Base `update_readme`, "NOT main (0.8.5)" | `main` = `51513f3`, tree-identical to `update_readme@94803cc` | Both PRs target `main`; release merge step deleted |
| 4 | "Deferred clicks fire after the wizard closes" (drain-fix note + smoke step 4) | `handle_reset` exits the process on both wizard outcomes (`exit(1)` on cancel, `relaunch_self → exit(0)` on success, `handsoff-tray.rs:1189-1198`); the `take_deferred_menu_events` dispatch (`:568`) never runs on that path | Accepted (settled): deferred immediate-action clicks are **dropped** on Reset-exit paths — identical to today's silent drop. Fix all wording + smoke expectations |
| 5 | "The four session-loop sites (:139, :189, :399, :422) gain event servicing (H6 fix)" | Misclassified. `:139/:189` are in `main()` **before any tap exists**; `:399/:422/:1057/:1093/:1162/:1182/:1148` run in the outer session-dispatch loop **between** `run_return` invocations (loop not spinning). The only genuinely in-closure sites are `:924, :937, :987, :1009` (menu dispatch at `:674/:677/:680`) | H6 framing corrected; classification pinned per site |
| 6 | "runModal spins a nested modal loop → tap serviced" stated as fact | The tap source is registered `kCFRunLoopCommonModes` (`event_tap.rs:412`); whether the modal-panel run loop services it is **expected but unverified** (Apple docs don't enumerate the common-mode set) | Downgraded to smoke-verify item; the swap is justified on single-mechanism grounds alone |
| 7 | "Linux zigbuild + 142 existing tests after every commit" | Tests **cannot run on the Linux dev box** (`E0455` framework link error, reproduced); suite is 141 `#[test]` functions; tests run on macOS CI only | Per-commit: zigbuild compile locally, full suite on macOS CI |
| 8 | "poller … same as today: late writes … ≤500 ms" | Today's poll threads (`wizard.rs:841`, `:1266`) **never exit on flow close** — they poll every 500 ms for the rest of the process. The token is a real lifecycle change (improvement) | Explicitly sanctioned deviation under the no-behavior-change policy (user-visible behavior unchanged) |
| 9 | (not in v1) | In-process `NSAlert` from an Accessory tray app does not front itself the way the separate osascript process did | `show_alert`/`confirm_reset` activate the app (`activateIgnoringOtherApps(true)`, pattern at `wizard.rs:433`) before `runModal` |

## Goals

1. **Fix the wizard-flow menu-drain hole** — `run_wizard_macos` is the only window flow whose pump loop never calls `absorb_menu_clicks` (loop at `wizard.rs:881`; other four flows: re-grant `:1307`, help `:1634`, preferences `:1994`, CP `:2308`); a tray-menu click during the post-Reset wizard queues invisible duplicate dialogs (issue #36 class).
2. **Retire the `osascript` alert path** — `show_alert`/`confirm_reset` shell out to `osascript` with blocking `.output()` on the main thread (H6); replace with native `NSAlert`, semantics preserved, one alert mechanism app-wide.
3. **Fix the dead stale-grant escape hatch** — `wizard.rs:499` compares `runModal()` against `NSModalResponseOK`; the #34 "Reset & Restart" button always cancels (review finding #2 above).
4. **Consolidate the five hand-rolled pump loops** into one flow-runner engine so the lifecycle invariants that produced #36 (close wedge, window leaks) are structural instead of per-flow review burden.

## Non-goals

- No user-visible behavior change (policy below names the sanctioned deviations).
- No UI framework adoption; the objc2-app-kit + tao stack stays (per the assessment verdict).
- No non-blocking alert redesign (H6's deeper half is a separate decision).
- No migration of `SIGNALS` from process-global to per-flow instances (possible follow-up).
- No deferred-click dispatch on Reset-exit paths (settled: dropped, as today).

## Decisions (v2 review session, 2026-10-03)

| Decision | Choice |
|---|---|
| Consolidation depth | Full flow runner (`src/window_flow.rs`); wizard migrated last |
| Behavior policy | No user-visible behavior change; two sanctioned internal deviations (poller token stops poll threads at flow exit; native alerts replace osascript rendering) |
| Alert scope | All 14 sites (13 `show_alert` + `confirm_reset`); straight swap, semantics preserved |
| Alert response constants | `runModal()` compared only against `NSAlertFirstButtonReturn`/`NSAlertSecondButtonReturn`; **`NSModalResponseOK` must not appear near `runModal`** (review gate) |
| PR shape | Two PRs: fixes first (PR 1, three commits), refactor after (PR 2) |
| PR base | `main` (`51513f3`, 0.9.0) — tree-identical to `update_readme@94803cc`; no release-time branch merge |
| Smoke testing | None for PR 1 pre-merge; one manual pass for PR 2 before merge (user-executed) |
| Signals scope | Keep process-global `SIGNALS`; runner consumes it |
| Poller shutdown | Cancellation token (`AtomicBool`, set on every exit path) — sanctioned improvement over today's never-exiting poll threads |
| Drain fix fate | PR 1's insertion is subsumed (deleted) by PR 2 — intentional |
| Deferred clicks on Reset exits | Dropped (process exits); matches today; wording corrected everywhere |
| Escape-hatch bug (#34 dead path) | Fixed as PR 1 Commit 3 (minimal constant swap) |
| Release timing | Tag v0.9.0 on `main`'s post-merge SHA after both PRs + PR 2 manual pass |
| PR 2 commits | Engine commit + one commit per migrated flow, each compiling green |
| Changelog | All PR 1 commits + PR 2 append to the existing 0.9.0/Unreleased section; no new bumps |
| Migration order | Risk-ascending: Help → Preferences → re-grant → Change Passphrase → wizard |
| Verification split | Dev box (Linux): zigbuild compile only. Tests: macOS CI only (dev box cannot link `core-graphics-types`) |
| Artifacts | This spec + tracking issues #39 (fixes), #40 (refactor), both updated to v2 facts |

## PR 1 — Fixes (tracking issue #39; three commits off `main`)

### Commit 1: wizard-flow menu drain

- Insert `absorb_menu_clicks(&window, &app, &super::WINDOW_FLOW_MENU_IDS.lock())` into `run_wizard_macos`'s pump loop (the `run_return` closure at `wizard.rs:881`; insert alongside the existing close-poll at `:892`), matching the four existing call sites.
- Value on the post-Reset path: window-flow clicks (Preferences / Change Passphrase / Reset / Help) are **consumed and the wizard re-fronted** — no duplicate dialogs queued for after the flow.
- Immediate-action clicks (Lock, Disable, Reenable, Check Updates) are deferred into `DEFERRED_MENU_EVENTS` and — on the post-Reset wizard — **dropped**: `handle_reset` exits the process on both outcomes (`exit(1)` cancel, `relaunch_self` success), so the `take_deferred_menu_events` dispatch at session start (`handsoff-tray.rs:568`) never runs for them. This matches today's silent drop (clicks previously rotted in the never-drained channel). The dispatch path remains live for every other flow and for the first-run wizard's return to session start.
- First-run wizard (before the tray icon exists) has no clicks to absorb; the fix matters only for the post-Reset wizard where the tray is live.
- **This insertion is expected to be deleted by PR 2** (drain moves into the runner engine). Stated here so the deletion reads as intentional. Draining twice is harmless; missing it is the bug.

### Commit 2: osascript → NSAlert retirement

Rewrite both helpers in `handsoff-tray.rs`; all 14 call sites untouched; exact title/message strings preserved (13 `show_alert` sites: `:139, :189, :399, :422, :924, :937, :987, :1009, :1057, :1093, :1162, :1182, :1293`).

**`show_alert` (`:1314`)** — `NSAlert::new(mtm)`; `setMessageText(title)`; `setInformativeText(message)`; single OK button; `runModal()`; default alert style (osascript showed no caution icon here — cosmetic equivalence accepted).

**`confirm_reset` (`:1204`)** — pinned implementation:
- `addButtonWithTitle("Cancel")` **first** → Cancel is the default button: Return triggers Cancel, and AppKit auto-assigns Escape to the "Cancel" button — matching osascript's `default button "Cancel"` + Escape-to-cancel.
- `addButtonWithTitle("Reset")` second.
- `setAlertStyle(NSAlertStyle::Critical)` — matches `with icon caution`: the caution badge appears only in `NSAlertStyle::Critical` ("will cause the icon to be badged with a caution icon", NSAlert.h; `Warning` is the plain app-icon default). **Not `Warning`** — that would silently drop the caution badge.
- Return `true` **iff** `runModal() == NSAlertSecondButtonReturn` (1001). Explicit Reset click only — a mis-typed Enter/Escape cancels, preserving the destructive-wipe gate.
- **Never compare `runModal()` against `NSModalResponseOK`** (= 1; `NSAlert` returns 1000/1001, so the comparison is always unequal — this exact mistake is finding #2 and Commit 3's subject).
 
**Both helpers:** obtain `mtm = MainThreadMarker::new().expect(...)` (all 14 sites are main-thread — verified: `main()` startup sites, session outer-loop arms at `:385-:433`, and `handle_*` helpers), then `NSApplication::sharedApplication(mtm)` + `activateIgnoringOtherApps(true)` (reuse/expose `activate_app`, `wizard.rs:433`) before `runModal` — an in-process alert from the Accessory tray app does not front itself the way the separate osascript process did. This also covers site `:1293`, which runs **before** `EventLoop::new` (NSApp is created on demand; sanity-check at release smoke).
 
**Site classification (H6 framing, corrected):**
- `:139`, `:189` — `main()` startup, **before any tap exists**: nothing to service; flat swap.
- `:399`, `:422` — outer session loop between `run_return` invocations: the run loop is not spinning there today either; flat swap, no H6 exposure.
- `:924, :937, :987, :1009` — called from **inside** the session `run_return` closure (`handle_lock_toggle`/`handle_disable`/`handle_reenable` via the menu dispatch at `:674/:677/:680`): the only sites where osascript's flat block was H6-exposed. `runModal` spins a nested modal loop; whether it services the tap (source registered `kCFRunLoopCommonModes`, `event_tap.rs:412`) is **expected but unverified — release-smoke item**, not an asserted benefit. Either way it is no worse than the flat block.
- `:1057, :1093` — inside `handle_preferences`/`apply_config_to_core`, which run from the **outer session-dispatch loop** (`:385`, after `run_return` exits): same bucket as `:399/:422` — the run loop is not spinning there; flat swap, no H6 exposure.
- `:1162, :1182`, `confirm_reset` at `:1148` — `handle_reset` from the outer dispatch loop; between sessions; flat swap.

### Commit 3: stale-grant escape-hatch fix (found in review)

- `wizard.rs:499`: `if unsafe { alert.runModal() } != NSModalResponseOK` → always true ("Reset & Restart" returns 1000 ≠ 1), so the #34 escape hatch **always cancels** — dead code since it shipped.
- Minimal fix: compare against `NSAlertFirstButtonReturn` (the first-added button is "Reset & Restart", `wizard.rs:496`). One-line constant swap; button order, defaults, and Escape-to-Cancel (second button is titled "Cancel") unchanged.
- Known pre-existing quirk, out of scope, flagged for a possible follow-up: the first button is the default, so **Return triggers "Reset & Restart"** — aggressive default for a destructive action. Reordering buttons would change behavior; not done under the no-behavior-change policy.
- This fix must land **before** PR 2, whose engine treats `wizard.rs` as the pattern to copy.

**Verification (PR 1):** Linux zigbuild compiles both targets per commit (`cargo zigbuild` + `cargo check --tests` compile-only); the 141-test suite runs on macOS CI per commit (dev box cannot run it — `E0455`). No pre-merge manual pass (accepted). Review gates: exact string preservation; `confirm_reset` Cancel-first/default + `NSAlertStyle::Critical` + `NSAlertSecondButtonReturn`; grep the diff for `NSModalResponseOK` near `runModal` (must be absent).

## PR 2 — Flow Runner Refactor (tracking issue #40; branch stacked on PR 1's merged `main`)

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
3. **Close-poll in every phase:** check `SIGNALS.close_requested` on every tick of every step — the #36 invariant, now structural.
4. `window.orderOut(None)` + outcome write on every exit path (close, finish, error).
5. `absorb_menu_clicks` drain every tick (window-flow IDs consumed + re-front; immediate-action IDs deferred).
6. Poller lifecycle: spawn on entry, `cancel.store(true)` on every exit path. **Sanctioned deviation:** today's poll threads (`wizard.rs:841`, `:1266`) never exit on flow close — they tick every 500 ms until process exit. The token stops them within one tick. No user-visible behavior change; the deviation is named here so the no-behavior-change policy isn't silently violated.
7. Terminal-state rendering (`show_terminal_state` pattern) where the flow uses it.
8. Alert interactions (if a flow shows one mid-run) use `NSAlertFirstButtonReturn`/`NSAlertSecondButtonReturn` — the engine must not re-import the `NSModalResponseOK` mistake fixed in PR 1 Commit 3.

What stays flow-specific (in `wizard.rs` as spec builders): step state machines, capture integration via `setup::capture_passphrase_headless` (nested CFRunLoop pump — the runner's 100 ms cadence must not disturb the pump-slice contract the close-abort relies on), stale-grant watchdog (30 s from Grant click), hotkey/timeouts form assembly, Help's static sections.

### Migration commits (each compiles green, no user-visible behavior change)

1. **Engine:** `src/window_flow.rs` with `run_flow` + poller slot; no flow migrated yet. Includes the invariant checklist as module docs (below).
2. **Help** (simplest: static window, one exit path) — proves the engine end-to-end.
3. **Preferences** — form + save outcome; first flow with interactive steps.
4. **Re-grant** — permission poll thread becomes the first poller-slot user; stale-grant watchdog moves into the poller closure.
5. **Change Passphrase** — trickiest pre-wizard migration: two capture phases run the nested CFRunLoop pump *inside* the runner's ticks; the runner must treat "capture in flight" as a step-poll state, not fight the nested loop.
6. **Wizard** — four steps + watchdog + capture; deletes PR 1's `absorb_menu_clicks` insertion (drain now in the engine) and the remaining four loop bodies' duplicated ceremony.

Postconditions: the five `run_return` loops in `wizard.rs` collapse into spec builders; `wizard.rs` shrinks by roughly the ceremony share (~400–600 lines) while `window_flow.rs` owns the invariants once.

### Runner invariant checklist (review gate; also module docs)

For every flow and every phase of every step:
- [ ] `close_requested` checked → flow exits, window `orderOut`, outcome written.
- [ ] Menu clicks drained every tick; window-flow clicks re-front the dialog; immediate clicks deferred.
- [ ] Poller token set on every exit path (no poll thread outlives its flow by more than one tick).
- [ ] `SIGNALS.begin_flow()` clears all per-flow signals on entry (stale-click leak impossible).
- [ ] Pump cadence stays ~100 ms; no blocking call inside the pump (capture's nested pump is the sanctioned exception and owns the thread while running).
- [ ] Exit path identical for close/cancel/success/failure (no window left visible — #36 leak class).
- [ ] No `NSModalResponseOK` comparison near `runModal` anywhere in the engine or specs.

### Verification (PR 2)

- Linux zigbuild compile after every commit; 141-test suite on macOS CI after every commit (dev box cannot run tests).
- New unit tests where pure: step state machine transitions driven by synthetic signals; poller cancellation (token set → closure exits within one tick); `begin_flow` clear-everything invariant. No GUI wiring tests (per spec #24 Testing Decisions).
- **One manual macOS pass before merge, executed by the user** at the TCC-approved `/Applications/HandsOff.app` path (per `AGENTS.md` re-grant path requirement):

**Smoke checklist (user executes; ~15 min):**
1. Launch app (no config or after Reset) → wizard opens automatically; close button exits cleanly at each phase (permission, capture-wait, capture, form, login-item).
2. Click Grant → status advances when permission granted; stale-grant path: wait 30 s+ without granting → escape hatch appears (needs stale TCC row; skip if not reproducible, note it). **If reproduced: click "Reset & Restart" and verify it now actually proceeds** (PR 1 Commit 3 — v0.8.x shipped it dead: it always cancelled).
3. Complete a capture (click Capture button first — capture must NOT auto-start), type ≥4 keys, Enter, retype, mismatch and match paths; reserved key shows named-key feedback.
4. Post-Reset wizard with tray live: window-flow tray clicks (Preferences / Change Passphrase / Help) during the wizard **re-front the wizard** — no duplicate dialogs appear during or after; immediate-action clicks (Lock/Disable) are **dropped by design** when the wizard exits (cancel exits the process; success relaunches) — verify no crash, no deferred dialog appears post-exit.
5. Preferences: open, change timeout, save, verify config.toml changed and core applied (notification shown).
6. Change Passphrase: verify-current phase rejects wrong passphrase; correct → new capture; close during wait phase exits cleanly (the #36 wedge); close during capture aborts within one tick; success shows in-dialog confirmation; window gone after every exit path.
7. Help window opens, static, closes cleanly.
8. Re-grant flow: open from tray menu, closes cleanly, tap resumes after grant.
9. (Release smoke, not pre-merge) Alert rendering: `show_alert` dialogs appear **fronted/focused** (activation fix); `confirm_reset` shows Cancel default (Enter/Escape cancels, Reset proceeds only on explicit click); pre-event-loop site (duplicate-instance alert) renders; if a lock-failure alert occurs while a tap is live, note whether input blocking stayed responsive (common-modes observation, informational).

## Rollback

- PR 1: each of the three commits is an independent revert; the alert swap reverts to osascript cleanly (call sites untouched by design); Commit 3's revert restores the dead escape hatch (acceptable rollback state = pre-PR1).
- PR 2: per-flow commits mean a misbehaving flow can revert to its hand-rolled loop without stranding the engine; worst case, revert the whole PR and PR 1's drain fix survives (it lives in the pre-refactor shape).

## Release

- After both PRs merge + PR 2 manual pass: `main` already carries the 0.9.0 content (`51513f3` ≡ `94803cc`); **no `update_readme` merge** — tag `main`'s post-merge SHA `v0.9.0` (tag exact SHA; release run appears ~10 s late per workflow memory). All PR entries live in the 0.9.0 changelog section appended by the PRs themselves.
- Release smoke then covers PR 1's deferred manual verification: alert dialogs render and front correctly, `confirm_reset` two-button semantics, escape-hatch confirm proceeds, pre-event-loop alert site, plus smoke item 9's informational common-modes observation.

## Risks

| Risk | Mitigation |
|---|---|
| Refactor regresses lifecycle behavior invisible to tests (#36 class) | Per-flow commits + review checklist + mandatory manual pass before merge |
| Capture nested-pump contract broken by runner cadence | CP migration commit is where this is proven; wizard depends on the same pattern after |
| `confirm_reset` button semantics drift (Reset made default → Enter wipes config) | Pinned impl: Cancel first/default, `NSAlertSecondButtonReturn` check; review gate greps for `NSModalResponseOK` near `runModal` |
| Stale-grant path untestable locally | Checklist step 2 marked skippable-if-not-reproducible; release smoke retries |
| In-process alerts not fronted (Accessory policy) | Activation call in both helpers (existing `activate_app` pattern); release smoke verifies fronting |
| Common-modes tap-servicing assumption wrong | Downgraded to informational smoke item; swap justified independently (single mechanism, no flat block) |
| Dev box cannot run tests | Verification split: zigbuild locally, 141-test suite on macOS CI per commit |