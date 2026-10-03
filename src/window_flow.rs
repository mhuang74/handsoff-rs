//! Window-flow runner engine (issue #40): the single home for the
//! per-flow ceremony the five hand-copied `run_return` loops in
//! `wizard.rs` currently duplicate.
//!
//! One flow = one window shown by the tray (wizard, Help, Preferences,
//! re-grant, Change Passphrase). The engine runs it on the CALLER's tao
//! event loop (`run_return`, same single-process-loop contract as the
//! existing loops — see `wizard::run_wizard` for why the loop is never
//! created or dropped here) and owns the invariants below. Flow-specific
//! logic (widgets, step state machines, capture integration, watchdogs,
//! form assembly) stays with the spec builders in `wizard.rs` via the
//! closures of a `FlowSpec`.
//!
//! Generic over the flow's outcome type `T` (`()` for re-grant/Help,
//! `WizardOutcome` for the wizard, `Config` for Change Passphrase,
//! `PreferencesOutcome` for Preferences).
//!
//! # Module docs: runner invariant checklist (review gate)
//!
//! For every flow and every phase of every step:
//!
//! - [ ] `close_requested` checked → flow exits, window `orderOut`,
//!   outcome written. The outcome is the spec's [`FlowSpec::close_outcome`]
//!   resolved for the step the close landed in (each flow owns its close
//!   semantic — Help closes `Ok(())`, the others carry their own
//!   cancellation message, Change Passphrase's depends on the phase);
//!   the generic "Window closed before the flow completed" applies only
//!   when a spec supplies no resolver.
//! - [ ] Menu clicks drained every tick; window-flow clicks re-front the
//!   dialog; immediate clicks deferred.
//! - [ ] Poller token set on every exit path (no poll thread outlives its
//!   flow by more than one tick).
//! - [ ] `SIGNALS.begin_flow()` clears all per-flow signals on entry
//!   (stale-click leak impossible).
//! - [ ] Pump cadence stays ~100 ms; no blocking call inside the pump
//!   (capture's nested pump is the sanctioned exception and owns the
//!   thread while running).
//! - [ ] Exit path identical for close/cancel/success/failure (no window
//!   left visible — #36 leak class).
//! - [ ] No `NSModalResponseOK` comparison near `runModal` anywhere in
//!   the engine or specs.
//!
//! # Alert responses
//!
//! If a flow shows an `NSAlert` mid-run (e.g. the reset escape hatch),
//! `runModal()` returns `NSAlertFirstButtonReturn` (1000) or
//! `NSAlertSecondButtonReturn` (1001). NEVER compare against
//! `NSModalResponseOK` (1) — it never matches (the shipped dead-code bug
//! fixed in PR 1, issue #39). This engine never shows alerts itself.

use anyhow::{anyhow, Result};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSView, NSWindow};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Pump cadence, same as today's five loops (engine responsibility 2).
pub(crate) const PUMP_TICK: Duration = Duration::from_millis(100);

/// Identifies a step within a flow. Flows define their own numbering; the
/// engine only tracks which one is current and drives transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepId(pub u8);

/// What one poll of a step decided (engine responsibility 4's decision
/// shape; the engine applies it via [`advance_step`]). Not `Eq`/`Debug`:
/// `FlowOutcome` wraps an `anyhow::Result`, which carries neither.
#[derive(Debug)]
pub enum StepPoll<T> {
    /// Step stays active; the engine keeps ticking. This includes
    /// "capture in flight": the poll fires only between pump slices of
    /// the nested CFRunLoop pump in `setup::capture_passphrase_headless`,
    /// so staying put while the capture owns the thread is exactly right
    /// (the engine must not fight the nested loop).
    Stay,
    /// Step finished; move to this step (the engine re-renders).
    Advance(StepId),
    /// Flow completed with its outcome (success or failure — the
    /// outcome carries which; close-before-completion is an `Err`).
    Finish(FlowOutcome<T>),
}

/// How a flow ended, mirroring the existing loops' `Result` outcomes.
///
/// `Debug`/`PartialEq` are hand-written: `anyhow::Error` carries neither
/// `Eq` nor a payload worth comparing, so equality is coarse — `Ok`
/// differs from `Err`, and all `Err`s compare equal. Enough for the
/// step-machine tests; flows never compare outcomes in production.
pub struct FlowOutcome<T>(pub Result<T>);

impl<T> std::fmt::Debug for FlowOutcome<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Ok(_) => f.write_str("FlowOutcome(Ok(_))"),
            Err(e) => write!(f, "FlowOutcome(Err({e:?}))"),
        }
    }
}

impl<T> PartialEq for FlowOutcome<T> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Ok(_), Ok(_)) => true,
            (Err(_), Err(_)) => true,
            _ => false,
        }
    }
}

impl<T> Eq for FlowOutcome<T> {}

/// Per-flow background poller handle. The poller closure runs on its own
/// thread and ticks until the token is set; the runner sets the token on
/// EVERY exit path (close, outcome, error) before returning.
///
/// Sanctioned deviation vs today: the permission poll threads
/// (`wizard.rs:841`, `:1266`) never exit on flow close — they tick every
/// 500 ms until process exit. The token stops them within one tick (spec
/// PR 2, engine responsibility 6). No user-visible behavior change; the
/// deviation is named so the no-behavior-change policy isn't silently
/// violated.
#[derive(Clone)]
pub struct PollerHandle {
    cancel: Arc<AtomicBool>,
}

impl PollerHandle {
    /// Is this poller cancelled? The closure checks this every tick and
    /// must exit within one tick of it flipping true.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Signal the poller to stop; it exits within one tick. crate-private:
    /// only the runner cancels (never the flow's own closures).
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// The window a flow built, plus the AppKit state the engine's ceremony
/// needs. The engine never touches widgets directly — those belong to the
/// spec builders in `wizard.rs`.
pub struct FlowWindow {
    pub window: Retained<NSWindow>,
    pub app: Retained<NSApplication>,
}

/// Static description of one flow: how to build the window, how to render
/// each step, how to poll it, and the background poller closure.
///
/// All AppKit-touching closures run on the MAIN thread inside the engine's
/// pump tick (same threading model as today's loop bodies). The poller
/// closure runs on its own background thread and must only touch
/// process-global signals and its own captured state — never AppKit.
pub struct FlowSpec<T> {
    /// First step of the flow (step machines start here; later commits'
    /// spec builders re-derive the step numbering of their loops).
    pub first_step: StepId,
    /// Window configuration, called on the main thread before the pump
    /// starts: build widgets, set the close delegate, `makeKeyAndOrderFront`.
    /// Returns the window + app the engine runs the ceremony against.
    pub build_window: Box<dyn FnOnce() -> Result<FlowWindow>>,
    /// Per-step render hook, called on the main thread when the engine
    /// switches to a step (and once for the first step). Steps own their
    /// widgets; this is the `show_terminal_state` seam — terminal phases
    /// are just steps whose poll finishes (engine responsibility 7).
    pub render_step: Box<dyn Fn(StepId)>,
    /// Per-step poll hook, called on the main thread every pump tick for
    /// the current step. Consumes per-flow click signals (e.g.
    /// `SIGNALS.grant_clicked.swap(false, ..)`) and decides the next
    /// state. The engine runs this AFTER the close/menu ceremony so a
    /// step can never preempt the invariants.
    pub poll_step: Box<dyn FnMut(StepId) -> StepPoll<T>>,
    /// Background poller body — the poller slot's first users are the
    /// permission polls (wizard/re-grant) with the stale-grant watchdog
    /// folded in. `None` for flows without one (Help). Runs on its own
    /// thread; must stop within one tick of [`PollerHandle::cancelled`].
    pub poller: Option<Box<dyn FnOnce(&PollerHandle) + Send>>,
    /// Outcome used when `close_requested` fires (engine close-poll).
    /// Each flow owns its close semantics — the loops being migrated write
    /// their own close messages ("Permission window closed before the
    /// Accessibility grant completed", "Change Passphrase cancelled —
    /// config unchanged"), and Help closes `Ok(())`. `None` falls back to
    /// the generic `Window closed before the flow completed` error.
    ///
    /// The closure captures whatever shared flow state the decision needs
    /// (e.g. the Change Passphrase phase cell, shared with `poll_step`
    /// via `Rc<RefCell<..>>`), exactly like the loop bodies being migrated.
    pub close_outcome: Option<Box<dyn FnOnce() -> Result<T> + 'static>>,
    /// Extra state the flow's AppKit objects anchor to beyond the spec
    /// itself (widgets, the window delegate target, …). The engine holds
    /// it until `run_flow` returns, mirroring today's loops where the
    /// `run_return` closure keeps every widget alive until the flow ends.
    ///
    /// `Vec<Retained<NSView>>` is the concrete shape every migrated flow
    /// needs (the window's target object is an NSView subclass; widget
    /// liveness is anchored transitively by the content view retaining
    /// them — the widgets ARE retained by their superviews, but keeping a
    /// handle here documents the contract and lets `close_outcome` reach
    /// the content view). Cheap: a few `Retained` clones per flow.
    pub keep_alive: Vec<Retained<NSView>>,
}

/// Outcome slot the engine writes exactly once before stopping the loop.
type OutcomeSlot<T> = Rc<RefCell<Option<Result<T>>>>;

/// Apply one step-poll decision to the current step. Pure so the
/// transition rules are testable without AppKit (spec PR 2 testing
/// decisions); `Err` carries the terminal outcome.
fn advance_step<T>(current: StepId, poll: StepPoll<T>) -> Result<StepId, FlowOutcome<T>> {
    match poll {
        StepPoll::Stay => Ok(current),
        StepPoll::Advance(next) => Ok(next),
        StepPoll::Finish(outcome) => Err(outcome),
    }
}

/// The one exit path, shared by close/finish/error so no path can forget
/// the window, the poller token, or the loop stop (invariant checklist:
/// "exit path identical for close/cancel/success/failure").
fn finish_flow<T>(
    window: &NSWindow,
    app: &NSApplication,
    slot: &OutcomeSlot<T>,
    poller: Option<&PollerHandle>,
    outcome: Result<T>,
) {
    window.orderOut(None);
    if let Some(p) = poller {
        p.cancel();
    }
    *slot.borrow_mut() = Some(outcome);
    crate::wizard::stop_run_loop(app);
}

/// Runs one flow on the caller's tao event loop (same `run_return` model
/// as today's five loops) and returns its outcome.
///
/// Engine ceremony per tick, in this order (mirrors the existing loops):
/// 1. `ControlFlow::WaitUntil(now + 100 ms)` — pump cadence.
/// 2. Drain menu clicks (window-flow IDs consumed + re-front; immediate
///    IDs deferred) — single-dialog invariant, issue #36.
/// 3. Close-poll: `close_requested` → exit via the shared finish path.
///    Checked EVERY tick, in EVERY phase of every step — the #36
///    invariant, now structural (engine responsibility 3).
/// 4. Step poll for the current step (`Stay` / `Advance` / `Finish`),
///    transitions applied by [`advance_step`] with a re-render on
///    advance.
///
/// On entry the engine calls `SIGNALS.begin_flow()` (issue #36 story 16
/// semantics verbatim, engine responsibility 1) and spawns the poller
/// AFTER the clear (same order as today's loops); on EVERY exit path it
/// cancels the poller before returning (responsibility 6).
///
/// # Errors
/// Window build failure, the flow's own error outcome (e.g. the window
/// closed before completion), or the loop ending without an outcome.
pub fn run_flow<T>(
    spec: FlowSpec<T>,
    event_loop: &mut tao::event_loop::EventLoop<crate::wizard::WizardEvent>,
) -> Result<T> {
    // Story 16: clear every per-flow signal so a previous flow's state
    // can't leak into this one. Process-global, one authority.
    crate::wizard::begin_flow_signals();

    let FlowWindow { window, app } =
        (spec.build_window)().map_err(|e| anyhow!("window build failed: {e:#}"))?;

    // Poller slot: spawned after begin_flow (same order as today's
    // loops), cancelled on every exit path below. A clone of the handle
    // stays outside the loop for the unconditional post-loop cancel.
    let poller = spec.poller.map(|body| spawn_poller(body));
    let post_loop_poller = poller.clone();

    // Step machine: engine-side current-step tracking; the spec's
    // closures own the transitions' meaning.
    let first_step = spec.first_step;
    let current_step = Rc::new(RefCell::new(Some(first_step)));
    (spec.render_step)(first_step);

    let mut poll_step = spec.poll_step;
    let mut close_outcome = spec.close_outcome;
    let outcome_slot: OutcomeSlot<T> = Rc::new(RefCell::new(None));
    {
        let outcome_slot = outcome_slot.clone();
        let current_step = current_step.clone();
        // Anchor for the flow's AppKit objects: the loop closure holds it
        // until the flow ends (same lifetime as today's run_return closures,
        // which keep every widget alive until the flow finishes).
        let _keep_alive = spec.keep_alive;

        use tao::platform::run_return::EventLoopExtRunReturn;
        event_loop.run_return(move |event, _, control_flow| {
            // 1. Pump cadence.
            *control_flow = tao::event_loop::ControlFlow::WaitUntil(Instant::now() + PUMP_TICK);

            let _ = &event; // raw NSWindow: tao events carry no useful signal

            // 2. Menu drain every tick (single-dialog invariant).
            crate::wizard::absorb_menu_clicks(
                &window,
                &app,
                &crate::wizard::window_flow_menu_ids(),
            );

            // 3. Close-poll in EVERY phase of every step. The spec's
            // close_outcome resolves the outcome (flows own their close
            // semantics); the generic message is the no-resolver fallback.
            if crate::wizard::close_requested() {
                let outcome = match close_outcome.take() {
                    Some(f) => f(),
                    None => Err(anyhow!("Window closed before the flow completed")),
                };
                finish_flow(
                    &window,
                    &app,
                    &outcome_slot,
                    poller.as_ref(),
                    outcome,
                );
                return;
            }

            // 4. Step poll for the current step.
            let step = current_step
                .borrow()
                .expect("current step is always Some between finish and loop end");
            match advance_step(step, poll_step(step)) {
                Err(FlowOutcome(outcome)) => {
                    finish_flow(&window, &app, &outcome_slot, poller.as_ref(), outcome);
                }
                Ok(next) if next != step => {
                    *current_step.borrow_mut() = Some(next);
                    (spec.render_step)(next);
                }
                Ok(_) => {}
            }
        });
    }

    if let Some(p) = post_loop_poller {
        p.cancel();
    }

    let mut final_outcome = outcome_slot.borrow_mut();
    let outcome = final_outcome
        .take()
        .unwrap_or_else(|| Err(anyhow!("Flow event loop ended unexpectedly")));
    drop(final_outcome);
    outcome
}

/// Spawn the poller closure on its own thread. It ticks its own cadence
/// and exits within one tick of the cancel token (engine responsibility
/// 6; sanctioned deviation from the never-exiting threads at
/// `wizard.rs:841`/`:1266` — see [`PollerHandle`]).
fn spawn_poller(body: Box<dyn FnOnce(&PollerHandle) + Send>) -> PollerHandle {
    let handle = PollerHandle {
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let thread_handle = handle.clone();
    std::thread::spawn(move || body(&thread_handle));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    const TEST_TICK: Duration = Duration::from_millis(20);

    /// Wait until `flag` flips or the deadline passes; returns the flag.
    fn wait_for(flag: &AtomicBool, deadline: Instant) -> bool {
        while Instant::now() < deadline {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            thread::sleep(Duration::from_millis(2));
        }
        flag.load(Ordering::SeqCst)
    }

    /// Poller cancellation (spec PR 2 testing decisions): token set →
    /// closure exits within one tick. Pure threading, runs everywhere.
    #[test]
    fn poller_exits_within_one_tick_after_cancel() {
        let exited = Arc::new(AtomicBool::new(false));
        let exited_cell = exited.clone();
        let handle = spawn_poller(Box::new(move |h: &PollerHandle| {
            // Poller contract: tick until cancelled.
            while !h.cancelled() {
                thread::sleep(TEST_TICK);
            }
            exited_cell.store(true, Ordering::SeqCst);
        }));
        // Let it tick at least once, then cancel and bound the wait.
        thread::sleep(TEST_TICK * 2);
        assert!(!exited.load(Ordering::SeqCst));
        handle.cancel();
        assert!(
            wait_for(&exited, Instant::now() + TEST_TICK * 3),
            "poller must exit within one tick of cancel"
        );
    }

    /// Cancel before the poller's first tick: still exits (the token is
    /// checked before each sleep — no missed-wakeup wedge).
    #[test]
    fn poller_cancelled_before_first_tick_exits() {
        let exited = Arc::new(AtomicBool::new(false));
        let exited_cell = exited.clone();
        let handle = spawn_poller(Box::new(move |h: &PollerHandle| {
            while !h.cancelled() {
                thread::sleep(TEST_TICK);
            }
            exited_cell.store(true, Ordering::SeqCst);
        }));
        handle.cancel();
        assert!(
            wait_for(&exited, Instant::now() + TEST_TICK * 3),
            "pre-first-tick cancel must still stop the poller"
        );
    }

    /// Engine-side step machine (spec PR 2 testing decisions): transitions
    /// driven by synthetic step polls, no AppKit.
    #[test]
    fn step_machine_transitions_driven_by_synthetic_signals() {
        let s0 = StepId(0);
        let s1 = StepId(1);
        let s2 = StepId(2);

        // Stay keeps the current step.
        assert_eq!(advance_step(s0, StepPoll::<()>::Stay), Ok(s0));
        // Advance switches to the named step.
        assert_eq!(advance_step(s0, StepPoll::<()>::Advance(s1)), Ok(s1));
        // A chain of advances walks the machine in order.
        assert_eq!(advance_step(s1, StepPoll::<()>::Advance(s2)), Ok(s2));
        // Finish is terminal and carries the outcome either way.
        assert_eq!(
            advance_step(s0, StepPoll::Finish(FlowOutcome::<()>(Ok(())))),
            Err(FlowOutcome::<()>(Ok(())))
        );
        assert_eq!(
            advance_step(
                s0,
                StepPoll::Finish(FlowOutcome::<()>(Err(anyhow!("closed"))))
            ),
            Err(FlowOutcome::<()>(Err(anyhow!("closed"))))
        );
        // Success and failure outcomes must differ.
        assert_ne!(
            advance_step(s0, StepPoll::Finish(FlowOutcome::<()>(Ok(())))),
            advance_step(
                s0,
                StepPoll::Finish(FlowOutcome::<()>(Err(anyhow!("closed"))))
            )
        );
    }

    /// begin_flow clear-everything invariant (spec PR 2 testing
    /// decisions), macOS-only: every per-flow signal clears — including
    /// status and waiting_since — so stale clicks from a previous flow
    /// cannot leak into the next (issue #36 story 16).
    #[cfg(target_os = "macos")]
    #[test]
    fn begin_flow_clears_every_signal() {
        use crate::wizard;
        wizard::set_all_signals_dirty();
        let dirty = wizard::signals_snapshot();
        assert!(dirty.any_set(), "precondition: signals start dirty");

        // The engine's entry call — the exact function run_flow invokes.
        wizard::begin_flow_signals();

        assert!(!wizard::signals_snapshot().any_set());
    }
}
