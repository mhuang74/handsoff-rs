# UI Stack Review: No Full macOS UI Library — Cost/Benefit Assessment

**Date:** 2026-10-03
**Scope:** The architectural decision to avoid a full macOS UI library (original design proposed `cocoa-rs`; alternatives considered: egui/iced/cacao, Tauri) and the effort spent building the Setup Wizard, Preferences, Change Passphrase, and Help windows.
**Sources:** `specs/cli-and-tray-app-design.md` §3.2, `specs/in-app-setup-wizard.md`, ADRs 0001–0004, git history of `src/wizard.rs` / `src/preferences.rs`, PRs #31–#35, issues #26/#29/#34/#36.

---

## 1. The decision, as made

The original design (`specs/handsoff-design.md`) planned `cocoa-rs` (v0.25) for the menu bar and any passphrase/hotkey UI. The tray redesign (`specs/cli-and-tray-app-design.md` §3.2) explicitly rejected the heavier options:

| Rejected | Reason given |
|---|---|
| egui / iced / cacao | "Too heavy (>10MB), unnecessary for simple menu bar UI" |
| Tauri | "Requires web stack (WebView), overkill for menu-only UI" |

What shipped instead is a three-layer stack with no UI framework at all:

1. **tao (0.28)** — event loop; chosen because `tray-icon` already depends on it, not as a UI layer.
2. **objc2-app-kit (0.2)** — raw AppKit via generated Rust bindings: `NSWindow`, `NSAlert`, `NSButton`, `NSTextField`, `NSSecureTextField`, `NSStackView`, `NSGridView`, layout constraints. 148 AppKit usage sites in `wizard.rs` alone; 130 `unsafe` blocks; ~121 `msg_send!`/`extern "C"` sites.
3. **osascript shelling** — `show_alert`/`confirm_reset` in `handsoff-tray.rs` run `Command::new("osascript").output()` for modal dialogs instead of native `NSAlert` (which the wizard does use — two coexisting alert mechanisms).

Note: `cocoa`/`cocoa-foundation` appear in `Cargo.lock` only as transitive dependencies of tao/tray-icon — the `cocoa-rs` plan was never adopted.

## 2. Effort spent, quantified

The wizard/preference surface grew far beyond "menu bar app with minimal UI":

**Code size (current):**
- `src/wizard.rs` — 2,625 lines, 36 functions. Hosts five full window flows: Setup Wizard, permission re-grant, Preferences, Change Passphrase (two-phase verify-then-recapture), Help.
- `src/preferences.rs` — 411 lines (pure gating/save logic, the testable seam).
- `src/bin/handsoff-tray.rs` — 1,629 lines, mostly menu/dialog wiring.

**Churn (git, 2026-10-01 → 2026-10-03, `wizard.rs` + `preferences.rs` only):** ~7,900 insertions / ~2,100 deletions across 14 commits in **three days**. PR #35 alone touched `wizard.rs` +1,459/−177 and `handsoff-tray.rs` +604/−161.

**Bug-fix tax specific to the hand-rolled UI:**

| Incident | Cost | Root cause |
|---|---|---|
| #34 stale TCC grant | +253 lines (escape hatch: 30 s watchdog, `tccutil reset` flow, relaunch) | Wizard wait loop had no exit for a TCC row pinned to an old CDHash |
| 2026-10-01 silent keyboard lockout | +144 lines (explicit capture start, close-abort, single-instance flock) | Wizard auto-installed the swallow-everything capture tap on permission grant; 16 stacked wizard launches observed |
| #36 phase-0 close wedge | +628/−194 (Change Passphrase overhaul) | Raw `NSWindow` never fires tao `CloseRequested`; closing during the wait phase wedged the app, recovery required `killall` |
| #36 feedback UX | same commit | Dots-only progress, no match/mismatch verdict, clipped instruction text (label without word-wrap) |
| Dialog text clipping | 0.8.4 follow-up | `NSTextField::labelWithString` never wraps; replaced with wrapping label + taller window |

**Structural workarounds the no-framework choice forced** (all live in `wizard.rs` today):

- Process-global `WizardSignals`: 8 `AtomicBool` click flags + `close_requested` + status/waiting state, because button events arrive via a hand-declared `WizardTarget` ObjC class (`declare_class!`, `buttonClicked:` tag dispatch) and must cross into tao's loop by polling, not callbacks.
- `close_requested` must be polled **in every flow phase** in every flow loop (issue #36 phase-0 lesson); a missed poll site is a wedge.
- `absorb_menu_clicks` drains tray-menu clicks during dialogs (4 call sites); the Setup Wizard flow (`run_wizard_macos`, `wizard.rs:624-1137`) is the one flow that does **not** call it — a known open hole.
- `stop_run_loop` fires a dummy AppKit event because `-[NSApplication stop:]` only takes effect on the next accidental wake (issue #36 "dialog takes seconds to appear").
- Five near-duplicate `run_return` pump loops (~100 ms `WaitUntil` cadence) with hand-managed step state machines.
- Two exit mechanics coexist: `stop_run_loop` for flows, `std::process::exit(0)` for Quit (skipping destructors because dropping tao's CFRunLoop observers panics).

**Testing tax:** the macOS TCC GUI-testing limits (`docs/agents/gui-testing-limits.md`) mean none of this UI can be agent-verified — every flow needs manual smoke tests on a real Mac at the TCC-approved bundle path, and the dev box is Linux (zigbuild cross-compile; runtime verification only on macOS CI or a local Mac). The design mitigated this deliberately: `preferences.rs` and config round-trips are the tested seams; the GUI is "a thin shell over tested logic" (`specs/in-app-setup-wizard.md`, Testing Decisions). In practice the shell was not thin — lifecycle bugs (#36) lived precisely in the untestable layer.

## 3. Pros of the decision

1. **Zero new toolchain.** No Node/webview (Tauri), no retained-mode GUI framework runtime, no >10 MB binary bloat. The stack is tao + objc2 bindings — everything already needed for the tray, event tap, and notifications.
2. **Full access to AppKit behaviors a wrapper would gate.** The wizard depends on subtle native mechanics: `windowShouldClose:` delegate firing during a nested CFRunLoop pump (the close-abort path), `NSApplication` activation policy, `SMAppService` login items, deep links into System Settings (`x-apple.systempreferences:`), `NSAlert::runModal`. A wrapper (egui/iced/cacao) would have had to be bypassed for all of these anyway.
3. **Correct threat-model fit.** The UI's hardest problems were never widgets — they were TCC permission lifecycle (stale CDHash grants, misattribution, re-grant-on-update), silent capture semantics, and single-instance/lifecycle safety. Those live in `setup.rs`/`lib.rs` logic, testable without any UI framework. A fancier UI library would not have prevented #34 or #36; they were lifecycle-design bugs, not rendering bugs.
4. **Security-consistent.** The silent passphrase capture (no cleartext ever in a text field — capture is via event tap into keycode sequences) is orthogonal to the widget toolkit and was preserved; secure text fields (`NSSecureTextField`) exist only for non-secret inputs.
5. **The user-facing outcome is good.** Non-technical users get a real first-run wizard (Gatekeeper instructions, permission step, double-entry capture with per-entry feedback, login-item checkbox, re-grant screen doubling for updates), Preferences without re-setup, Change Passphrase with verify-current-first, a Help guide, Reset/Reenable separation — the full story list of `specs/in-app-setup-wizard.md` is implemented.

## 4. Cons of the decision

1. **The "thin shell" is ~2,600 lines of hand-rolled event-loop plumbing** with five structurally similar pump loops and a shared mutable global signal bus. Every new window flow re-derives the same close/poll/orderOut ceremony; #36 showed a single missed poll branch wedges the whole app (`killall` was the only recovery).
2. **Dual window models are a recurring bug class.** Raw `NSWindow`s (dialogs) vs tao windows have different close-event semantics; the codebase now carries permanent comments explaining that tao `CloseRequested` is dead code for these windows. This split produced the phase-0 wedge and the window-leak bugs.
3. **Two alert mechanisms.** Wizard flows use native `NSAlert::runModal`; the tray shells out to `osascript` — which is also a documented hang risk (H6 in `docs/assessment-interception-hang-risk-2026-10.md`: a blocking child process on the main thread stalls tap servicing and tray responsiveness). The framework-free choice is what made the cheap-but-wrong osascript path attractive.
4. **Effort was spent on framework problems, not product problems.** Of the ~10k lines of churn in three days, a substantial fraction (signals bus, run-loop waking, close plumbing, window leaks, text wrapping) is exactly what a retained UI toolkit handles. The counter-argument — that toolkits bring their own event-loop integration issues with CGEventTap — is real but unproven here.
5. **The GUI layer is untestable by construction.** With `preferences::menu_state` as pure logic the gating is testable (ADR 0003), but lifecycle correctness — the thing that actually broke — lives only in the manual-test layer, which the agent workflow cannot exercise (`AGENTS.md` GUI-testing limits).
6. **Maintainability risk compounds.** Each future window (e.g., a first-run What's New, a hotkey-capture widget) must re-implement the signals/poll/orderOut pattern correctly; nothing in the type system enforces the "poll `close_requested` in every phase" invariant — it is enforced by review and by incidents.

## 5. Assessment

**The framework rejection was right for the wrong reason.** The §3.2 rationale ("too heavy for a menu bar UI") was written when the UI *was* just a menu. What actually happened is that ADR 0002 turned the app into a wizard-driven product, and the team then built a small hand-rolled UI framework inside `wizard.rs` — signals bus, delegates, run-loop pumps, close semantics — which is precisely the layer a UI library exists to provide. The decision saved ~10 MB and a webview/toolchain, and it kept AppKit-level control that the TCC/re-grant/capture flows genuinely needed; those are strong, defensible reasons. But the ledger shows the cost was not zero: three days of near-continuous fixes (#34, #36, lockout incident, clipping follow-up) were framework-shaped problems — event-loop integration, window lifecycle, event delivery — paid in the least testable part of the codebase.

**Verdict:** keep the decision, but treat its cost as structural, not incidental:

1. **Consolidate the five pump loops into one parameterized flow runner** (steps, signal set, window config) so the close/poll/`orderOut` ceremony exists once. This converts the #36 invariant from per-flow review burden into library code.
2. **Retire the `osascript` alert path in `handsoff-tray.rs`** in favor of the existing `NSAlert` pattern (also removes H6); one alert mechanism, not two.
3. **Close the wizard-flow menu-drain hole** — `run_wizard_macos` is the only flow not calling `absorb_menu_clicks`.
4. If another interactive surface is ever needed, revisit the decision *then* with the real data from this review — the "menu-only UI" premise no longer describes the app.

---

*Companion documents: `docs/assessment-interception-hang-risk-2026-10.md` (tap/main-thread risks, incl. H6 osascript), `docs/assessment-critical-bugs-2026-10-02.md` (state-consistency findings), ADR 0002 (wizard adoption), ADR 0004 (sole-path cutover).*
