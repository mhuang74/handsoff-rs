//! Setup Wizard: single native window guiding first-run configuration.
//!
//! The tray launches this wizard whenever the config is absent or fails strict
//! validation (`setup::validate_config_strict`); a valid config never shows
//! it (Preferences is a separate #27 concern). Since ADR 0004 removed the
//! CLI and its terminal `--setup` flow, this wizard is the sole setup path.
//!
//! Steps, in order (spec #24):
//! 1. Permission explanation + Grant button → opens System Settings pane.
//! 2. Poll until TCC grants Accessibility (background check, status label).
//! 3. Passphrase capture: SILENT physical-key capture via the session
//!    event tap (`setup::capture_passphrase_headless`) — raw
//!    keycodes, ≥4 keys, Enter commit, Backspace delete, Escape restart,
//!    reserved keys rejected. Double entry, silent compare. No cleartext
//!    ever exists: the window shows progress dots only.
//! 4. Hotkeys (lock/talk last key) + auto-lock + auto-unlock on one form.
//! 5. SMAppService login-item checkbox (macOS 13+; no legacy fallback).
//!
//! Implementation: a single `tao` window; AppKit widgets through
//! `objc2-app-kit` (already in the dependency tree). Button actions route
//! through a custom `NSView` target (declare_class, same pattern as
//! tray-icon's TrayTarget) recording clicks in shared atomics; the tao
//! event loop polls them and drives step transitions. `run_wizard` runs
//! `run_return` on the calling main thread and returns the outcome.

use crate::config_file::Config;
use crate::setup::SetupOutcome;
use anyhow::Result;

/// Login-item registration result for step 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemResult {
    /// Registered (or already registered).
    Enabled,
    /// User unchecked / registration declined; app won't auto-launch.
    Disabled,
    /// Registration failed (e.g. running unbundled); wizard continues.
    Failed,
}

/// Everything the wizard collected, ready for `setup::assemble_and_save_config`.
pub struct WizardOutcome {
    pub setup: SetupOutcome,
    pub login_item: LoginItemResult,
}

/// One styled section of the Help window: bold heading + plain body, plus
/// an optional two-column table (item name + description) rendered as
/// aligned rows.
#[derive(Debug, Clone)]
pub struct HelpSection {
    pub heading: String,
    pub body: String,
    /// Optional table: each row is (left-column item, description).
    /// Rendered left-aligned with a fixed item column so descriptions line up.
    pub table: Vec<(String, String)>,
}

/// What the tray should do at startup (issue #29): run normally, re-grant
/// the Accessibility permission, or run the full Setup Wizard.
///
/// The distinction is issue #29's core routing rule: a config that passes
/// strict validation but finds `AXIsProcessTrusted() == false` has a STALE
/// grant (typical after an unsigned update replaces the binary and its
/// CDHash) — that user must NOT be sent through passphrase re-setup; only
/// the permission step. A config that fails strict validation needs the
/// full wizard (whose first step is the same permission screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupFlow {
    /// Config valid, permissions granted: run the tray normally.
    Run,
    /// Config valid but Accessibility grant stale/missing: permission-only
    /// re-grant (reuses the wizard's permission step; config untouched).
    ReGrant,
    /// Config absent or invalid: full Setup Wizard.
    Wizard,
}

/// Pure startup-routing decision (issue #29), unit-tested in
/// `tests/wizard_tests.rs`. `config_valid` is `validate_config_strict()`'s
/// result; `has_accessibility_permissions` the authoritative full check.
pub fn startup_flow(config_valid: bool, has_accessibility_permissions: bool) -> StartupFlow {
    if !config_valid {
        StartupFlow::Wizard
    } else if !has_accessibility_permissions {
        StartupFlow::ReGrant
    } else {
        StartupFlow::Run
    }
}

/// Launch the wizard modally and return the collected outcome.
///
/// Runs the wizard on the CALLER's tao event loop via `run_return` and
/// returns when the flow completes (or is cancelled by closing the window).
/// The tray owns the loop and keeps it alive for the whole process: tao's
/// macOS CFRunLoop observers are registered permanently in `EventLoop::new`
/// and hold a `Weak<PanicInfo>` that panics on `.upgrade()` once the loop
/// that created them is dropped, so a second loop (or dropping the wizard's)
/// would panic the tray on the next wake. One loop, created in `main`,
/// drives both the wizard and the tray.
///
/// On success a valid `config.toml` exists (round-trip: `Config::load`
/// succeeds and passes `setup::validate_config_strict`).
///
/// # Errors
/// Window creation failure, capture timeout, or window closed before the
/// flow completes.
#[cfg(target_os = "macos")]
pub fn run_wizard(
    event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<WizardOutcome> {
    self::macos::run_wizard_macos(event_loop)
}

#[cfg(not(target_os = "macos"))]
pub fn run_wizard(
    _event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<WizardOutcome> {
    anyhow::bail!("Setup wizard requires macOS")
}

/// Permission-only re-grant flow (issue #29): the wizard's permission step
/// WITHOUT passphrase capture or any other step. The tray routes here when
/// the config is valid but the Accessibility grant is stale — typically
/// because an unsigned update replaced the binary and its CDHash, so TCC's
/// per-code-signature grant no longer matches. The user re-ticks the box in
/// System Settings; the config is never read, shown, or written.
///
/// Runs on the CALLER's tao event loop via `run_return` (same
/// single-process-loop contract as `run_wizard`; see its doc comment).
///
/// # Errors
/// Window creation failure, or window closed before the grant completes.
#[cfg(target_os = "macos")]
pub fn run_permission_regrant(
    event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<()> {
    self::macos::run_permission_regrant_macos(event_loop)
}

#[cfg(not(target_os = "macos"))]
pub fn run_permission_regrant(
    _event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<()> {
    anyhow::bail!("Permission re-grant requires macOS")
}

/// User-event payload for the wizard's event loop (tray forwards these
/// untouched when no wizard is active).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, Default)]
pub struct WizardEvent;

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct WizardEvent;

/// Save a wizard outcome as the application config (seam 1).
pub fn wizard_outcome_to_config(outcome: &WizardOutcome) -> Result<Config> {
    crate::setup::assemble_and_save_config(&outcome.setup)
}

// ---------------------------------------------------------------------------
// Single-dialog invariant plumbing (issue #36).
//
// The tray's session loop does not drain the menu event channel while a
// dialog owns the nested run_return. The dialog drains it instead: clicks on
// window-flow items are consumed (the dialog re-fronts), clicks on
// immediate-action items are deferred here and returned to the tray at the
// next session start, so e.g. a Lock click during a dialog still lands.
// ---------------------------------------------------------------------------

/// Menu-item IDs that open window flows (Preferences, Change Passphrase,
/// Reset, Re-grant, Help). Registered once by the tray after building the menu;
/// dialogs consult it to distinguish duplicate-dialog clicks (consumed)
/// from immediate-action clicks (deferred).
pub static WINDOW_FLOW_MENU_IDS: parking_lot::Mutex<Vec<tray_icon::menu::MenuId>> =
    parking_lot::Mutex::new(Vec::new());

/// Immediate-action menu clicks absorbed while a dialog was open. The tray
/// drains this at each session start and dispatches the actions itself
/// (dialogs have no access to the core).
pub static DEFERRED_MENU_EVENTS: parking_lot::Mutex<Vec<tray_icon::menu::MenuId>> =
    parking_lot::Mutex::new(Vec::new());

/// Drain immediate-action menu clicks deferred during a dialog (tray calls
/// this at session start).
pub fn take_deferred_menu_events() -> Vec<tray_icon::menu::MenuId> {
    std::mem::take(&mut *DEFERRED_MENU_EVENTS.lock())
}

// ---------------------------------------------------------------------------
// Flow-runner engine hooks (issue #40). `window_flow.rs` owns the per-flow
// ceremony; these re-exports and the `macos` accessors below are the only
// surface it consumes. SIGNALS stays process-global — exposed, not duplicated.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub(crate) use self::macos::{absorb_menu_clicks, stop_run_loop};

/// Menu-item IDs that open window flows, as seen by the engine's per-tick
/// drain.
#[cfg(target_os = "macos")]
pub(crate) fn window_flow_menu_ids(
) -> parking_lot::MutexGuard<'static, Vec<tray_icon::menu::MenuId>> {
    WINDOW_FLOW_MENU_IDS.lock()
}

/// Engine entry (issue #40): clear every per-flow signal — issue #36
/// story 16 semantics verbatim, one authority.
#[cfg(target_os = "macos")]
pub(crate) fn begin_flow_signals() {
    self::macos::begin_flow_signals()
}

/// Engine close-poll (issue #40): did the window delegate request the
/// flow to end?
#[cfg(target_os = "macos")]
pub(crate) fn close_requested() -> bool {
    self::macos::close_requested()
}

// begin_flow invariant test hooks, re-exported for window_flow::tests.
#[cfg(all(target_os = "macos", test))]
pub(crate) use self::macos::{set_all_signals_dirty, signals_snapshot};

#[cfg(target_os = "macos")]
mod macos {
    use super::{LoginItemResult, WizardOutcome};
    use crate::config_file::Config;
    use crate::constants::{
        AUTO_LOCK_DEFAULT_SECONDS, AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS,
    };
    use crate::setup::{self, SetupOutcome};
    use anyhow::{anyhow, Result};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
    use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSAlert, NSApplication, NSApplicationActivationPolicy,
        NSBackingStoreType, NSButton, NSControlStateValueOn, NSEvent, NSEventModifierFlags,
        NSEventType, NSFont, NSAlertFirstButtonReturn, NSGridView, NSLayoutAttribute,
        NSScrollView, NSStackView, NSStackViewGravity, NSTextAlignment, NSTextField,
        NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;

    const WINDOW_W: f64 = 480.0;
    const WINDOW_H: f64 = 300.0;

    // Button action tags (NSInteger carried by the sender's tag property).
    const TAG_GRANT: isize = 1;
    const TAG_COMMIT: isize = 2;
    const TAG_FINISH: isize = 3;
    /// Issue #34 escape hatch: delete the stale TCC row and relaunch.
    const TAG_RESET: isize = 4;
    /// Explicit "Capture Passphrase" button: capture starts only on click.
    const TAG_CAPTURE: isize = 5;
    /// Dismiss button on the Change Passphrase success/failure state (#36).
    const TAG_OK: isize = 6;
    /// Cancel button on the Change Passphrase waiting state (#36).
    const TAG_CANCEL: isize = 7;

    // Step numbers.
    const STEP_PERMISSION: u8 = 0;
    const STEP_CAPTURE: u8 = 1;
    const STEP_FORM: u8 = 2;

    /// TCC service name tccd uses for the tray app's Accessibility grant.
    /// NOT the CFBundleIdentifier (`com.handsoff.inputlock`): for the
    /// ad-hoc-signed tray binary tccd keys the row as
    /// `<executable>.<app>` — verified live 2026-10-01 (`tccutil reset
    /// Accessibility com.handsoff.inputlock` fails with -10814, the
    /// `handsoff-tray.handsoff` spelling succeeds).
    const TCC_SERVICE: &str = "handsoff-tray.handsoff";

    /// How long the permission poll may keep failing after the user
    /// clicked Grant before the stale-grant escape hatch (issue #34)
    /// is surfaced. A normal first grant lands in seconds; a stale
    /// grant (checkbox ON, checks failing — TCC row pinned to an old
    /// CDHash) NEVER lands, so a sustained failure is the only
    /// detectable signature.
    const STALE_GRANT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    const STALE_GRANT_TEXT: &str = "Still stuck? If the HandsOff checkbox in System Settings\n\
         > Privacy & Security > Accessibility is already ticked but\n\
         this window keeps waiting, the permission is bound to an\n\
         old copy of the app (this happens after an update).\n\n\
         Click “Reset Permission & Restart…” to clear it —\n\
         HandsOff relaunches and asks for the permission fresh.\n\
         Your passphrase and settings are NOT affected.";

    /// Click signals shared between AppKit button targets and the wizard driver.
    struct WizardSignals {
        grant_clicked: AtomicBool,
        commit_clicked: AtomicBool,
        finish_clicked: AtomicBool,
        reset_clicked: AtomicBool,
        /// "Capture Passphrase" clicked: capture starts ONLY on this explicit
        /// user action — never automatically on permission grant (keyboard
        /// lockout incident 2026-10-01).
        capture_clicked: AtomicBool,
        /// OK clicked on a terminal state (Change Passphrase success/failure).
        ok_clicked: AtomicBool,
        /// Cancel clicked (Change Passphrase waiting phase).
        cancel_clicked: AtomicBool,
        /// Window close requested (any flow). The dialogs are raw NSWindows,
        /// so tao's CloseRequested never fires for them; the close arrives
        /// via the window delegate's `windowShouldClose:`, which sets this
        /// flag — polled in EVERY flow phase so closing a window always
        /// terminates its flow (issue #36 phase-0 close wedge).
        close_requested: AtomicBool,
        step: AtomicU8,
        /// Progress text for the status label (dots), set from the tap callback.
        status: parking_lot::Mutex<String>,
        /// When the user entered the waiting phase (Grant clicked); the
        /// poll thread starts the stale-grant clock from here, not from
        /// window open — reading the instructions may legitimately take
        /// longer than `STALE_GRANT_TIMEOUT`. `None` outside the wait.
        waiting_since: parking_lot::Mutex<Option<std::time::Instant>>,
    }

    impl WizardSignals {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                grant_clicked: AtomicBool::new(false),
                commit_clicked: AtomicBool::new(false),
                finish_clicked: AtomicBool::new(false),
                reset_clicked: AtomicBool::new(false),
                capture_clicked: AtomicBool::new(false),
                ok_clicked: AtomicBool::new(false),
                cancel_clicked: AtomicBool::new(false),
                close_requested: AtomicBool::new(false),
                step: AtomicU8::new(STEP_PERMISSION),
                status: parking_lot::Mutex::new(String::new()),
                waiting_since: parking_lot::Mutex::new(None),
            })
        }

        /// Clear every per-flow signal so a flow behaves identically whether
        /// launched fresh or right after another flow (issue #36 story 16).
        /// SIGNALS is process-global and shared across flows (wizard,
        /// re-grant, preferences, change-passphrase): a click landing between
        /// consumption and flow end would otherwise leak into the next flow
        /// (e.g. a stale capture click auto-starting capture).
        fn begin_flow(&self) {
            self.grant_clicked.store(false, Ordering::SeqCst);
            self.commit_clicked.store(false, Ordering::SeqCst);
            self.finish_clicked.store(false, Ordering::SeqCst);
            self.reset_clicked.store(false, Ordering::SeqCst);
            self.capture_clicked.store(false, Ordering::SeqCst);
            self.ok_clicked.store(false, Ordering::SeqCst);
            self.cancel_clicked.store(false, Ordering::SeqCst);
            self.close_requested.store(false, Ordering::SeqCst);
            self.step.store(STEP_PERMISSION, Ordering::SeqCst);
            *self.status.lock() = String::new();
            *self.waiting_since.lock() = None;
        }
    }

    static SIGNALS: std::sync::LazyLock<Arc<WizardSignals>> =
        std::sync::LazyLock::new(WizardSignals::new);

    // ---- window_flow.rs engine hooks (issue #40) ----
    // The flow-runner engine (`src/window_flow.rs`) owns the per-flow
    // ceremony and consumes exactly these pieces of the module state;
    // everything else stays private to `macos`. Process-global SIGNALS
    // stays process-global — these are accessors, not new instances.

    /// Engine entry: clear every per-flow signal (issue #36 story 16
    /// semantics verbatim; see `WizardSignals::begin_flow`).
    pub(crate) fn begin_flow_signals() {
        SIGNALS.begin_flow();
    }

    /// Engine close-poll: has the window delegate's `windowShouldClose:`
    /// requested the flow to end?
    pub(crate) fn close_requested() -> bool {
        SIGNALS.close_requested.load(Ordering::SeqCst)
    }

    // ---- begin_flow invariant test hooks (spec PR 2 testing decisions;
    // engine-side tests only — never called by production code) ----

    /// Set every per-flow signal dirty (the test's precondition).
    #[cfg(test)]
    pub(crate) fn set_all_signals_dirty() {
        SIGNALS.grant_clicked.store(true, Ordering::SeqCst);
        SIGNALS.commit_clicked.store(true, Ordering::SeqCst);
        SIGNALS.finish_clicked.store(true, Ordering::SeqCst);
        SIGNALS.reset_clicked.store(true, Ordering::SeqCst);
        SIGNALS.capture_clicked.store(true, Ordering::SeqCst);
        SIGNALS.ok_clicked.store(true, Ordering::SeqCst);
        SIGNALS.cancel_clicked.store(true, Ordering::SeqCst);
        SIGNALS.close_requested.store(true, Ordering::SeqCst);
        SIGNALS.step.store(255, Ordering::SeqCst);
        *SIGNALS.status.lock() = "stale".to_string();
        *SIGNALS.waiting_since.lock() = Some(std::time::Instant::now());
    }

    /// Snapshot of every per-flow signal as booleans, for the
    /// begin_flow invariant test.
    #[cfg(test)]
    pub(crate) struct SignalsSnapshot {
        pub grant_clicked: bool,
        pub commit_clicked: bool,
        pub finish_clicked: bool,
        pub reset_clicked: bool,
        pub capture_clicked: bool,
        pub ok_clicked: bool,
        pub cancel_clicked: bool,
        pub close_requested: bool,
        pub step_nondefault: bool,
        pub status_set: bool,
        pub waiting_since_set: bool,
    }

    #[cfg(test)]
    impl SignalsSnapshot {
        pub(crate) fn any_set(&self) -> bool {
            self.grant_clicked
                || self.commit_clicked
                || self.finish_clicked
                || self.reset_clicked
                || self.capture_clicked
                || self.ok_clicked
                || self.cancel_clicked
                || self.close_requested
                || self.step_nondefault
                || self.status_set
                || self.waiting_since_set
        }
    }

    /// Read every per-flow signal (the test's observation).
    #[cfg(test)]
    pub(crate) fn signals_snapshot() -> SignalsSnapshot {
        SignalsSnapshot {
            grant_clicked: SIGNALS.grant_clicked.load(Ordering::SeqCst),
            commit_clicked: SIGNALS.commit_clicked.load(Ordering::SeqCst),
            finish_clicked: SIGNALS.finish_clicked.load(Ordering::SeqCst),
            reset_clicked: SIGNALS.reset_clicked.load(Ordering::SeqCst),
            capture_clicked: SIGNALS.capture_clicked.load(Ordering::SeqCst),
            ok_clicked: SIGNALS.ok_clicked.load(Ordering::SeqCst),
            cancel_clicked: SIGNALS.cancel_clicked.load(Ordering::SeqCst),
            close_requested: SIGNALS.close_requested.load(Ordering::SeqCst),
            step_nondefault: SIGNALS.step.load(Ordering::SeqCst) != STEP_PERMISSION,
            status_set: !SIGNALS.status.lock().is_empty(),
            waiting_since_set: SIGNALS.waiting_since.lock().is_some(),
        }
    }

    declare_class!(
        struct WizardTarget;

        unsafe impl ClassType for WizardTarget {
            type Super = NSView;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "HandsOffWizardTarget";
        }

        impl DeclaredClass for WizardTarget {
            type Ivars = ();
        }

        // Supertrait of NSWindowDelegate; conformance is required before
        // NSWindowDelegate can be implemented.
        unsafe impl NSObjectProtocol for WizardTarget {}

        unsafe impl NSWindowDelegate for WizardTarget {
            // Only the optional `windowShouldClose:` is implemented (below);
            // remaining optional delegate methods are untouched.
        }

        unsafe impl WizardTarget {
            #[method(buttonClicked:)]
            fn button_clicked(&self, sender: &AnyObject) {
                let tag: isize = unsafe { msg_send![sender, tag] };
                match tag {
                    TAG_GRANT => SIGNALS.grant_clicked.store(true, Ordering::SeqCst),
                    TAG_COMMIT => SIGNALS.commit_clicked.store(true, Ordering::SeqCst),
                    TAG_FINISH => SIGNALS.finish_clicked.store(true, Ordering::SeqCst),
                    TAG_RESET => SIGNALS.reset_clicked.store(true, Ordering::SeqCst),
                    TAG_CAPTURE => SIGNALS.capture_clicked.store(true, Ordering::SeqCst),
                    TAG_OK => SIGNALS.ok_clicked.store(true, Ordering::SeqCst),
                    TAG_CANCEL => SIGNALS.cancel_clicked.store(true, Ordering::SeqCst),
                    _ => {}
                }
            }

            // NSWindowDelegate (optional method): the red close button ends
            // an in-flight passphrase capture immediately. The wizard's tao
            // event loop is blocked inside the capture's nested CFRunLoop
            // pump, so CloseRequested cannot be processed until capture
            // ends — but AppKit dispatch (this delegate callback) still runs
            // during the pump, so the abort flag is seen within one 100 ms
            // pump slice. `close_requested` additionally lets the flow loops
            // observe the close OUTSIDE capture (issue #36: closing the
            // Change Passphrase dialog while it waits for the capture button
            // previously wedged the app — the flag was only polled during
            // capture, and the dead tao CloseRequested branch never fired
            // for these raw NSWindows).
            #[method(windowShouldClose:)]
            fn window_should_close(&self, _sender: &NSWindow) -> bool {
                SIGNALS.close_requested.store(true, Ordering::SeqCst);
                setup::request_capture_abort();
                true
            }
        }
    );

    /// Open System Settings → Accessibility pane.
    fn open_accessibility_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status();
    }

    /// Stop the AppKit run loop NOW instead of on the next accidental wake.
    ///
    /// `-[NSApplication stop:]` only takes effect when the run loop next
    /// processes an event; with tao's WaitUntil cadence the stop could lag
    /// (the Change Passphrase dialog took seconds to appear after its menu
    /// click — issue #36). Posting a dummy app-defined event forces an
    /// immediate wake: the technique tao's own `stop_app_on_panic` uses
    /// (see https://stackoverflow.com/questions/48041279).
    pub(crate) fn stop_run_loop(app: &NSApplication) {
        app.stop(None);
        let dummy = unsafe {
            NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::ApplicationDefined,
                NSPoint::new(0.0, 0.0),
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                0,
            )
        };
        if let Some(event) = dummy {
            app.postEvent_atStart(&event, true);
        }
    }

    /// `-[NSApplication activate]` is macOS 14+; min deployment is 13.0
    /// (Info.plist.template), where the unrecognized selector would crash the
    /// first-run path. `activateIgnoringOtherApps` exists since 10.0; the
    /// binding is deprecated-but-safe in objc2-app-kit 0.2.
    pub(crate) fn activate_app(app: &NSApplication) {
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }

    /// Single-dialog invariant (issue #36): while a dialog owns the nested
    /// `run_return`, the tray's session loop is NOT draining the
    /// process-global menu event channel, so clicks would pile up and each
    /// window-flow click would spawn a duplicate dialog after this flow
    /// ends. Drain them here instead:
    ///
    /// - Window-flow clicks (Preferences / Change Passphrase / Reset /
    ///   Re-grant) are CONSUMED: the live dialog comes to front (the visible
    ///   "a dialog is open" response, story 4) and the click is never queued
    ///   into a later flow.
    /// - Immediate-action clicks (Lock, Disable, Reenable, Check Updates)
    ///   are DEFERRED to the tray's next session start (never swallowed —
    ///   issue requirement 2), so e.g. a Lock click during the dialog still
    ///   lands right after it closes.
    pub(crate) fn absorb_menu_clicks(
        window: &NSWindow,
        app: &NSApplication,
        window_flow_ids: &[tray_icon::menu::MenuId],
    ) {
        let mut absorbed = false;
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if window_flow_ids.contains(&event.id) {
                absorbed = true;
                log::info!(
                    "Window-flow menu click {:?} while a dialog is open — dialog brought to front",
                    event.id
                );
            } else {
                log::info!(
                    "Menu click {:?} while a dialog is open — deferred to after the dialog closes",
                    event.id
                );
                super::DEFERRED_MENU_EVENTS.lock().push(event.id);
            }
        }
        if absorbed {
            window.makeKeyAndOrderFront(None);
            activate_app(app);
        }
    }

    /// Click the reset escape hatch: confirm, run `tccutil reset
    /// Accessibility <TCC_SERVICE>`, then relaunch the app. The reset is
    /// the same action the issue verified live as the remediation; the
    /// app cannot clear its own row silently (tccd requires user
    /// consent), hence the confirmation dialog. On confirm this never
    /// returns (relaunch exits the process); on cancel it falls through
    /// so the wizard keeps waiting.
    fn reset_permission_and_relaunch() {
        let mtm = MainThreadMarker::new().expect("reset dialog must run on main thread");
        let alert = unsafe { NSAlert::new(mtm) };
        unsafe {
            alert.setMessageText(&NSString::from_str("Reset Accessibility permission?"));
            alert.setInformativeText(&NSString::from_str(
                "This removes HandsOff's (stale) Accessibility entry in System \
                 Settings, then relaunches HandsOff so it can request the \
                 permission fresh. Your passphrase and settings are NOT affected.",
            ));
            alert.addButtonWithTitle(&NSString::from_str("Reset & Restart"));
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        if unsafe { alert.runModal() } != NSAlertFirstButtonReturn {
            log::info!("Stale-grant reset cancelled by user");
            return;
        }

        log::info!(
            "Running tccutil reset Accessibility {} (issue #34 stale-grant remediation)",
            TCC_SERVICE
        );
        let output = std::process::Command::new("/usr/bin/tccutil")
            .arg("reset")
            .arg("Accessibility")
            .arg(TCC_SERVICE)
            .output();
        match &output {
            Ok(o) if o.status.success() => {
                log::info!(
                    "tccutil reset succeeded: {}",
                    String::from_utf8_lossy(&o.stdout).trim()
                );
            }
            other => {
                log::error!(
                    "tccutil reset failed ({:?}); not relaunching",
                    other.as_ref().map(|o| o.status)
                );
                let fail = unsafe { NSAlert::new(mtm) };
                unsafe {
                    fail.setMessageText(&NSString::from_str("Reset failed"));
                    fail.setInformativeText(&NSString::from_str(
                        "Could not reset the permission automatically. Run this \
                         in Terminal, then relaunch HandsOff:\n\n\
                         tccutil reset Accessibility handsoff-tray.handsoff",
                    ));
                    fail.runModal();
                }
                return;
            }
        }

        relaunch_after_reset();
    }

    /// Replace this process with a fresh instance: spawn the same binary,
    /// then exit. Same approach as the tray's `relaunch_self` (spawn+exit
    /// rather than exec, so the dying process never runs again and the
    /// fresh instance re-runs the normal startup path against the reset
    /// TCC row). Unbundled (cargo run): current_exe has no .app ancestor;
    /// still exit so the user restarts it themselves.
    fn relaunch_after_reset() {
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                log::error!("Cannot locate own binary after reset: {}", e);
                std::process::exit(1);
            }
        };
        log::info!("Relaunching {:?} after permission reset", exe);
        match std::process::Command::new(&exe)
            .args(
                std::env::args()
                    .skip(1)
                    // Designated successor: parent exits right after
                    // spawning but still holds the flock — the child skips
                    // the fatal duplicate alert and re-acquires the lock
                    // during a short grace window (issue #37 N1).
                    .chain(["--skip-instance-lock".to_string()]),
            )
            .spawn()
        {
            Ok(_) => {
                log::info!("Relaunched after reset; exiting");
                std::process::exit(0);
            }
            Err(e) => {
                log::error!(
                    "Relaunch failed ({}); exiting so the user can start HandsOff manually",
                    e
                );
                std::process::exit(1);
            }
        }
    }

    /// Register/unregister the SMAppService login item (macOS 13+).
    ///
    /// `SMAppService` is ObjC-only (no C API): use the ObjC runtime directly
    /// rather than adding `objc2-service-management` as a dependency.
    /// Selectors are `registerAndReturnError:` / `unregisterAndReturnError:`
    /// (Apple docs) — an NSError out-param, not a return-code method.
    /// Requires a bundled app (the class reads the bundle); unbundled
    /// `cargo run` returns an error and the wizard continues (login item is
    /// optional, not a setup gate).
    fn set_login_item(enable: bool) -> crate::wizard::LoginItemResult {
        use objc2::rc::Retained;
        use objc2::{class, msg_send, msg_send_id};

        let some_class = class!(SMAppService);
        let service: Retained<objc2::runtime::AnyObject> =
            unsafe { msg_send_id![some_class, mainAppService] };
        // NSError** out-param: declare as AnyObject to stay in the runtime's
        // type world; nil on success.
        let mut error: *mut objc2::runtime::AnyObject = std::ptr::null_mut();
        let ok: bool = unsafe {
            if enable {
                msg_send![&service, registerAndReturnError: &mut error]
            } else {
                msg_send![&service, unregisterAndReturnError: &mut error]
            }
        };
        if ok {
            if enable {
                crate::wizard::LoginItemResult::Enabled
            } else {
                crate::wizard::LoginItemResult::Disabled
            }
        } else {
            log::warn!(
                "SMAppService {} failed (unbundled run?)",
                if enable { "register" } else { "unregister" }
            );
            crate::wizard::LoginItemResult::Failed
        }
    }

    pub(super) fn run_wizard_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
    ) -> Result<WizardOutcome> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| anyhow!("wizard must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        let style =
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, WINDOW_H));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("HandsOff Setup"));
        unsafe { window.setReleasedWhenClosed(false) };

        let target: Retained<WizardTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };

        // --- Widgets ---
        let make_label = |text: &str, width: f64, height: f64| -> Retained<NSTextField> {
            let l = unsafe { NSTextField::labelWithString(&NSString::from_str(text), mtm) };
            unsafe { l.setFrameSize(NSSize::new(width, height)) };
            l
        };

        let grant_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Grant Accessibility Permission"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { grant_btn.setTag(TAG_GRANT) };

        let commit_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Commit & Continue"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { commit_btn.setTag(TAG_COMMIT) };

        let finish_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Finish"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { finish_btn.setTag(TAG_FINISH) };

        let status_label = make_label("", WINDOW_W - 60.0, 20.0);
        // Wrapping label: instruction text word-wraps at the content width
        // (plain labels bleed off-window) and Auto Layout grows the stack
        // vertically for multi-paragraph step-0 text.
        let instr_label = unsafe {
            NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm)
        };
        unsafe {
            instr_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 40.0));
            instr_label.setPreferredMaxLayoutWidth(WINDOW_W - 60.0);
            instr_label.setMaximumNumberOfLines(0);
        }

        // Issue #34 escape hatch: hidden until the poll concludes the grant
        // is stale (waited STALE_GRANT_TIMEOUT with zero progress).
        let reset_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Reset Permission & Restart…"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { reset_btn.setTag(TAG_RESET) };
        reset_btn.setHidden(true);

        // Explicit capture start (keyboard-lockout fix): the wizard must
        // never silently install the keyboard-capture event tap. Hidden
        // until the Accessibility permission is granted; hidden again while
        // a capture runs.
        let capture_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Capture Passphrase"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { capture_btn.setTag(TAG_CAPTURE) };
        capture_btn.setHidden(true);

        // Close button aborts an in-flight capture (see windowShouldClose:).
        {
            let delegate = ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        let make_text_field = |placeholder: &str, width: f64| -> Retained<NSTextField> {
            let f = unsafe {
                NSTextField::initWithFrame(
                    mtm.alloc(),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 24.0)),
                )
            };
            unsafe { f.setPlaceholderString(Some(&NSString::from_str(placeholder))) };
            f
        };
        let lock_key = make_text_field("L", 48.0);
        let talk_key = make_text_field("T", 48.0);
        let auto_lock = make_text_field("120", 80.0);
        let auto_unlock = make_text_field("3600", 80.0);
        let login_checkbox = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str("Launch HandsOff at login"),
                Some(&*target),
                None,
                mtm,
            )
        };
        unsafe { login_checkbox.setState(NSControlStateValueOn) };

        // --- Layout: one vertical stack; visibility toggled per step ---
        let content = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, WINDOW_H)),
            )
        };
        unsafe {
            content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            content.setSpacing(12.0);
            // Leading-gravity keeps widgets top-aligned in the vertical stack.
            const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
            for v in [
                &*instr_label as *const NSTextField as *const NSView,
                &*grant_btn as *const NSButton as *const NSView,
                &*reset_btn as *const NSButton as *const NSView,
                &*capture_btn as *const NSButton as *const NSView,
                &*commit_btn as *const NSButton as *const NSView,
                &*status_label as *const NSTextField as *const NSView,
                &*lock_key as *const NSTextField as *const NSView,
                &*talk_key as *const NSTextField as *const NSView,
                &*auto_lock as *const NSTextField as *const NSView,
                &*auto_unlock as *const NSTextField as *const NSView,
                &*login_checkbox as *const NSButton as *const NSView,
                &*finish_btn as *const NSButton as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        // Step 0 visibility.
        grant_btn.setHidden(false);
        commit_btn.setHidden(true);
        capture_btn.setHidden(true);
        status_label.setHidden(true);
        lock_key.setHidden(true);
        talk_key.setHidden(true);
        auto_lock.setHidden(true);
        auto_unlock.setHidden(true);
        login_checkbox.setHidden(true);
        finish_btn.setHidden(true);
        unsafe {
            instr_label.setStringValue(&NSString::from_str(
                "HandsOff blocks all keyboard and mouse input until you\n\
                 type your secret passphrase. For that it needs the macOS\n\
                 Accessibility permission — granted to HandsOff itself,\n\
                 not a terminal.\n\n\
                 Click the button, then tick the box for HandsOff in\n\
                 System Settings.\n\n\
                 Note: HandsOff is unsigned. If macOS blocks it from\n\
                 launching, right-click (or Control-click) HandsOff.app and\n\
                 choose Open, then confirm Open in the dialog. You only\n\
                 need to do this once.",
            ))
        };

        window.center();
        window.makeKeyAndOrderFront(None);
        activate_app(&app);

        // ---- Drive the flow through the CALLER's tao event loop ----
        //
        // Flow-runner engine (issue #40): the hand-copied run_return loop
        // (cadence, menu drain, close-poll, exit path — including PR 1's
        // absorb_menu_clicks insertion, b636bde) is deleted; the engine
        // owns that ceremony and the spec owns the three-step state
        // machine. SIGNALS.step stays the shared step store (the
        // begin_flow snapshot test reads it); poll_step keeps it in sync
        // with the engine's current step on every transition.
        //
        // Permission polling on a background thread: fast lightweight
        // AXIsProcessTrusted checks every 500 ms (safe for repeated calls;
        // the full test-tap check degrades WindowServer when hammered), then
        // ONE authoritative full check when the lightweight flips true.
        //
        // Issue #34: if the wait lasts STALE_GRANT_TIMEOUT with NO progress
        // (lightweight never flips true), the grant is stale — the TCC row
        // is pinned to an old CDHash, the checkbox reads ON, and waiting
        // longer can never succeed. Surface the reset escape hatch.
        let perm_granted = Arc::new(AtomicBool::new(false));
        let perm_stale = Arc::new(AtomicBool::new(false));
        let poller = {
            let perm_granted = perm_granted.clone();
            let perm_stale = perm_stale.clone();
            Box::new(move |handle: &crate::window_flow::PollerHandle| {
                while !handle.cancelled() {
                    if crate::input_blocking::check_accessibility_permissions_lightweight()
                        && crate::input_blocking::check_accessibility_permissions()
                    {
                        perm_granted.store(true, Ordering::SeqCst);
                        return;
                    }
                    {
                        // Clock starts when the user clicks Grant (set by
                        // poll_step), not at window open — reading the
                        // instructions can legitimately take longer than
                        // STALE_GRANT_TIMEOUT. No click yet → no timing.
                        let started = SIGNALS.waiting_since.lock();
                        if let Some(t0) = *started {
                            if t0.elapsed() >= STALE_GRANT_TIMEOUT {
                                drop(started);
                                perm_stale.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }) as Box<dyn FnOnce(&crate::window_flow::PollerHandle) + Send>
        };

        // Main-thread-only flow state (poll_step is FnMut — mutable
        // captures, same locals the migrated loop owned).
        let mut first_keys: Option<Vec<u32>> = None;
        let mut capture_offered = false;
        let mut stale_shown = false;

        // Status text: SIGNALS.status (the tap callback's dot channel) plus
        // the status label — the same dual write the migrated loop's
        // set_status closure performed.
        fn set_status(status_label: &NSTextField, text: &str) {
            *SIGNALS.status.lock() = text.to_string();
            let ns = NSString::from_str(text);
            unsafe { status_label.setStringValue(&ns) };
        }

        // The engine's build_window takes the window; poll_step re-titles
        // it on an outcome-construction failure, so keep a Retained clone.
        let window_for_title = window.clone();

        let spec = crate::window_flow::FlowSpec {
            first_step: crate::window_flow::StepId(STEP_PERMISSION),
            build_window: Box::new(move || Ok(crate::window_flow::FlowWindow { window, app })),
            // The step-0 visibility and instruction text were set above,
            // before the engine starts (same as the migrated loop's setup);
            // steps render in place from poll_step, so the render hook has
            // nothing to swap.
            render_step: Box::new(|_| {}),
            poll_step: Box::new(move |id| {
                let step = id.0;

                // Issue #34: reset clicked → confirm, tccutil reset, relaunch
                // (never returns on confirm).
                if SIGNALS.reset_clicked.swap(false, Ordering::SeqCst) {
                    reset_permission_and_relaunch();
                    return crate::window_flow::StepPoll::Stay;
                }

                // Issue #34: wait exceeded STALE_GRANT_TIMEOUT with no
                // progress → the grant is stale; surface the escape hatch
                // once. (No return: the migrated loop fell through to the
                // grant check in the same tick.)
                if perm_stale.load(Ordering::SeqCst) && !stale_shown {
                    stale_shown = true;
                    reset_btn.setHidden(false);
                    // Long multi-line explanation → the multi-line
                    // instruction label (status label is a 20 pt one-liner;
                    // it would clip).
                    unsafe { instr_label.setStringValue(&NSString::from_str(STALE_GRANT_TEXT)) };
                    set_status(&status_label, "Permission appears stale (bound to an old copy of the app).");
                }

                // Step 0 → 1: Grant clicked.
                if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                    SIGNALS.step.store(STEP_CAPTURE, Ordering::SeqCst);
                    // Issue #34: start the stale-detection clock (see poller).
                    *SIGNALS.waiting_since.lock() = Some(std::time::Instant::now());
                    open_accessibility_settings();
                    unsafe {
                        instr_label.setStringValue(&NSString::from_str(
                            "Waiting for Accessibility permission…\n\
                             Tick the box for HandsOff in System Settings > Privacy & Security > Accessibility.",
                        ))
                    };
                    grant_btn.setHidden(true);
                    status_label.setHidden(false);
                    set_status(&status_label, "Waiting for permission…");
                    return crate::window_flow::StepPoll::Advance(crate::window_flow::StepId(
                        STEP_CAPTURE,
                    ));
                }

                // Step 1: poll permission; when granted, show the explicit
                // capture button — the keyboard-capturing tap is installed
                // ONLY on a click (keyboard-lockout fix: silent automatic
                // capture locked the whole keyboard for up to 300 s per
                // launch). `step` stays STEP_CAPTURE until both captures
                // succeed, so a failed capture can be retried from the
                // button.
                if step == STEP_CAPTURE {
                    if perm_granted.load(Ordering::SeqCst)
                        && SIGNALS.capture_clicked.swap(false, Ordering::SeqCst)
                    {
                        // Capture UI: status label shows entry + dots +
                        // countdown; the instruction label doubles as the
                        // feedback line (named reserved keys, too-short
                        // warnings, entry accepted) — issue #36 capture
                        // feedback.
                        set_status(&status_label, "");
                        capture_btn.setHidden(true);

                        // Double capture on the main thread. The headless
                        // pump yields control in 100 ms slices, so the
                        // AppKit event loop keeps servicing while the tap is
                        // installed. This is the sanctioned nested-pump
                        // exception: the engine's close-poll does not fire
                        // while the pump owns the thread (one long tick);
                        // the close abort arrives via windowShouldClose →
                        // close_requested + request_capture_abort, observed
                        // by capture_with_status within one 100 ms slice.
                        // Cancel path: closing the window ends the capture
                        // and restores keyboard input.
                        let first = match capture_with_status(
                            "Entry 1 of 2",
                            &status_label,
                            &instr_label,
                        ) {
                            Ok(k) => k,
                            Err(e) => {
                                if SIGNALS.close_requested.load(Ordering::SeqCst) {
                                    return crate::window_flow::StepPoll::Finish(
                                        crate::window_flow::FlowOutcome(Err(anyhow!(
                                            "Setup wizard closed before completing"
                                        ))),
                                    );
                                }
                                unsafe {
                                    instr_label.setStringValue(&NSString::from_str(&format!(
                                        "Capture failed: {e} — click the button to retry."
                                    )))
                                };
                                capture_btn.setHidden(false);
                                return crate::window_flow::StepPoll::Stay;
                            }
                        };
                        unsafe {
                            instr_label.setStringValue(&NSString::from_str(
                                "First entry accepted — re-enter the same Passphrase to confirm.",
                            ))
                        };

                        let second = match capture_with_status(
                            "Entry 2 of 2",
                            &status_label,
                            &instr_label,
                        ) {
                            Ok(k) => k,
                            Err(e) => {
                                if SIGNALS.close_requested.load(Ordering::SeqCst) {
                                    return crate::window_flow::StepPoll::Finish(
                                        crate::window_flow::FlowOutcome(Err(anyhow!(
                                            "Setup wizard closed before completing"
                                        ))),
                                    );
                                }
                                unsafe {
                                    instr_label.setStringValue(&NSString::from_str(&format!(
                                        "Capture failed: {e} — click the button to retry."
                                    )))
                                };
                                capture_btn.setHidden(false);
                                return crate::window_flow::StepPoll::Stay;
                            }
                        };

                        if first != second {
                            return crate::window_flow::StepPoll::Finish(
                                crate::window_flow::FlowOutcome(Err(anyhow!(
                                    "Passphrases did not match — restart the wizard to try again"
                                ))),
                            );
                        }
                        SIGNALS.step.store(STEP_FORM, Ordering::SeqCst);
                        first_keys = Some(first);
                        // Reveal the form.
                        status_label.setHidden(true);
                        commit_btn.setHidden(true);
                        lock_key.setHidden(false);
                        talk_key.setHidden(false);
                        auto_lock.setHidden(false);
                        auto_unlock.setHidden(false);
                        login_checkbox.setHidden(false);
                        finish_btn.setHidden(false);
                        instr_label.setHidden(false);
                        unsafe {
                            instr_label.setStringValue(&NSString::from_str(
                                "Choose hotkeys and timeouts. Empty fields use the defaults.",
                            ))
                        };
                        return crate::window_flow::StepPoll::Advance(crate::window_flow::StepId(
                            STEP_FORM,
                        ));
                    }
                    // Permission granted, capture not yet requested: show
                    // the explicit start button once (a local flag, not the
                    // step — the step must stay STEP_CAPTURE so the click
                    // above is still reachable).
                    if perm_granted.load(Ordering::SeqCst) && !capture_offered {
                        capture_offered = true;
                        grant_btn.setHidden(true);
                        status_label.setHidden(false);
                        capture_btn.setHidden(false);
                        unsafe {
                            instr_label.setHidden(false);
                            instr_label.setStringValue(&NSString::from_str(&format!(
                                "Accessibility granted. Click “Capture Passphrase” when ready.\n\n\
                                 Your keyboard will be captured until you type a Passphrase and press \
                                 Enter (max {}s). Nothing you type reaches other apps during capture. \
                                 Close this window to cancel.\n\n{}",
                                setup::CAPTURE_TIMEOUT_SECS,
                                setup::capture_rules_text(
                                    crate::constants::DEFAULT_LOCK_KEYCODE,
                                    crate::constants::DEFAULT_TALK_KEYCODE,
                                ),
                            )))
                        };
                        set_status(&status_label, "");
                        return crate::window_flow::StepPoll::Stay;
                    }
                    // Keep the waiting text fresh while permission is
                    // pending.
                    if SIGNALS.status.lock().is_empty() {
                        set_status(&status_label, "Waiting for Accessibility permission…");
                    }
                    return crate::window_flow::StepPoll::Stay;
                }

                // Step 2 → done: Finish clicked.
                if SIGNALS.finish_clicked.swap(false, Ordering::SeqCst)
                    && SIGNALS.step.load(Ordering::SeqCst) == STEP_FORM
                {
                    let lock = unsafe { lock_key.stringValue() }.to_string();
                    let talk = unsafe { talk_key.stringValue() }.to_string();
                    let lock = if lock.is_empty() {
                        None
                    } else {
                        Some(lock.to_uppercase())
                    };
                    let talk = if talk.is_empty() {
                        None
                    } else {
                        Some(talk.to_uppercase())
                    };
                    let auto_lock_val: u64 = unsafe { auto_lock.stringValue() }
                        .to_string()
                        .parse()
                        .unwrap_or(AUTO_LOCK_DEFAULT_SECONDS);
                    let auto_unlock_val: u64 = unsafe { auto_unlock.stringValue() }
                        .to_string()
                        .parse()
                        .unwrap_or(crate::app_state::AUTO_UNLOCK_BASE_SECONDS);

                    let keys = match first_keys.take() {
                        Some(k) => k,
                        // Unreachable in practice (STEP_FORM is entered only
                        // after a successful double capture); preserved from
                        // the migrated loop's guard.
                        None => {
                            return crate::window_flow::StepPoll::Finish(
                                crate::window_flow::FlowOutcome(Err(anyhow!(
                                    "No passphrase captured"
                                ))),
                            );
                        }
                    };

                    let login_item = if unsafe { login_checkbox.state() } == NSControlStateValueOn {
                        set_login_item(true)
                    } else {
                        LoginItemResult::Disabled
                    };

                    match build_outcome(keys, auto_lock_val, auto_unlock_val, lock, talk, login_item)
                    {
                        Ok(o) => {
                            return crate::window_flow::StepPoll::Finish(
                                crate::window_flow::FlowOutcome(Ok(o)),
                            );
                        }
                        Err(e) => {
                            window_for_title.setTitle(&NSString::from_str(&format!(
                                "HandsOff Setup — {}",
                                e
                            )));
                            // Stay: the window stays open with the reason in
                            // the title (same as the migrated loop).
                        }
                    }
                }
                crate::window_flow::StepPoll::Stay
            }),
            poller: Some(poller),
            // Close ends the flow in EVERY phase (issue #36); the engine's
            // close-poll handles it. Preserve the migrated loop's close
            // message verbatim.
            close_outcome: Some(Box::new(|| {
                Err(anyhow!("Setup wizard closed before completing"))
            })),
            // Anchor the WizardTarget NSView (close delegate — which also
            // aborts an in-flight capture — and click routing).
            keep_alive: vec![unsafe { Retained::cast::<NSView>(target) }],
        };
        crate::window_flow::run_flow(spec, event_loop)
    }

    /// Permission-only re-grant window (issue #29): the wizard's step-0
    /// screen alone — explanation, Grant button, background poll — with no
    /// passphrase capture, form, or login-item step. Same primitives as the
    /// wizard: `open_accessibility_settings()` on click, the lightweight +
    /// full permission poll thread (500 ms cadence, one authoritative full
    /// check when the lightweight flips true), and the same shared tao loop
    /// (`run_return` from the caller). Resolves Ok the moment the grant is
    /// detected; window-closed resolves Err.
    ///
    /// Only valid to call on macOS; see also `super::startup_flow` for how
    /// the tray decides between this, the full wizard, and plain startup.
    pub(super) fn run_permission_regrant_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
    ) -> Result<()> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| anyhow!("re-grant must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, WINDOW_H));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("HandsOff — Permission Needed"));
        unsafe { window.setReleasedWhenClosed(false) };

        let target: Retained<WizardTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };

        // Same wizard widgets; only step 0 is ever shown. The Grant button
        // routes through the SAME WizardSignals (TAG_GRANT) the wizard uses.
        let grant_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Grant Accessibility Permission"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { grant_btn.setTag(TAG_GRANT) };

        // Issue #34 escape hatch: hidden until the poll concludes the grant
        // is stale (waited STALE_GRANT_TIMEOUT with zero progress).
        let reset_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Reset Permission & Restart…"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { reset_btn.setTag(TAG_RESET) };
        reset_btn.setHidden(true);

        // Close button must terminate the flow (issue #36): the delegate's
        // windowShouldClose: sets close_requested, which the loop polls.
        {
            let delegate = ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        let status_label: Retained<NSTextField> =
            unsafe { NSTextField::labelWithString(&NSString::from_str(""), mtm) };
        unsafe { status_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 20.0)) };
        status_label.setHidden(true);

        let instr_label: Retained<NSTextField> = unsafe {
            NSTextField::wrappingLabelWithString(
                &NSString::from_str(
                    "HandsOff was updated, so macOS needs the Accessibility \
                     permission granted again to HandsOff itself — your \
                     passphrase and settings are NOT affected.\n\nBecause \
                     each release is signed differently, the OLD permission \
                     entry no longer matches this build. In System Settings \
                     > Privacy & Security > Accessibility, first REMOVE the \
                     old HandsOff entry (select it, click the − button), \
                     then click the button below and ADD HandsOff back \
                     (click +, choose HandsOff).",
                ),
                mtm,
            )
        };
        unsafe {
            // Word-wrapping label: long lines wrap at the content width
            // instead of bleeding off-window (NSTextField labels never wrap).
            instr_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 180.0));
            instr_label.setPreferredMaxLayoutWidth(WINDOW_W - 60.0);
            instr_label.setMaximumNumberOfLines(0);
        };

        let content = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, WINDOW_H)),
            )
        };
        unsafe {
            content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            content.setSpacing(12.0);
            const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
            for v in [
                &*instr_label as *const NSTextField as *const NSView,
                &*grant_btn as *const NSButton as *const NSView,
                &*reset_btn as *const NSButton as *const NSView,
                &*status_label as *const NSTextField as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        window.center();
        window.makeKeyAndOrderFront(None);
        activate_app(&app);

        // Flow-runner engine (issue #40): the hand-copied run_return loop
        // is deleted; the engine owns the ceremony and the spec owns the
        // re-grant state machine. The permission poll (lightweight
        // AXIsProcessTrusted every 500 ms, then ONE authoritative full
        // test-tap check when the lightweight flips true) plus the
        // issue #34 stale-grant watchdog now run as the engine's poller
        // closure — the first poller-slot user. Sanctioned deviation (the
        // old thread never exited on flow close; the token stops it within
        // one 500 ms tick on every exit path).
        //
        // The stale-detection clock still starts when the user clicks
        // Grant (SIGNALS.waiting_since, set in poll_step below), not at
        // window open — reading the instructions can legitimately take
        // longer than STALE_GRANT_TIMEOUT.
        let perm_granted = Arc::new(AtomicBool::new(false));
        let perm_stale = Arc::new(AtomicBool::new(false));
        let poller = {
            let perm_granted = perm_granted.clone();
            let perm_stale = perm_stale.clone();
            Box::new(move |handle: &crate::window_flow::PollerHandle| {
                while !handle.cancelled() {
                    if crate::input_blocking::check_accessibility_permissions_lightweight()
                        && crate::input_blocking::check_accessibility_permissions()
                    {
                        perm_granted.store(true, Ordering::SeqCst);
                        return;
                    }
                    {
                        // Clock starts when the user clicks Grant (set by
                        // poll_step), not at window open — reading the
                        // instructions can legitimately take longer than
                        // STALE_GRANT_TIMEOUT. No click yet → no timing.
                        let started = SIGNALS.waiting_since.lock();
                        if let Some(t0) = *started {
                            if t0.elapsed() >= STALE_GRANT_TIMEOUT {
                                drop(started);
                                perm_stale.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }) as Box<dyn FnOnce(&crate::window_flow::PollerHandle) + Send>
        };

        // stale_shown: the escape hatch surfaces once (local flag, same as
        // the migrated loop).
        let stale_shown = std::cell::Cell::new(false);
        let spec = crate::window_flow::FlowSpec {
            first_step: crate::window_flow::StepId(0),
            build_window: Box::new(move || Ok(crate::window_flow::FlowWindow { window, app })),
            // Single step: the grant/wait/stale states all render in step 0
            // (widget visibility + label text updated in poll_step, same as
            // the migrated loop's in-place updates).
            render_step: Box::new(|_| {}),
            poll_step: Box::new(move |_| {
                // Grant clicked → open the System Settings pane, wait for
                // the poll.
                if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                    open_accessibility_settings();
                    // Issue #34: start the stale-detection clock (see poller).
                    *SIGNALS.waiting_since.lock() = Some(std::time::Instant::now());
                    let ns = NSString::from_str(
                        "Waiting for Accessibility permission…\n\
                         Tick the box for HandsOff in System Settings > Privacy & Security > Accessibility.",
                    );
                    unsafe {
                        instr_label.setStringValue(&ns);
                        status_label.setHidden(false);
                        status_label.setStringValue(&NSString::from_str("Waiting for permission…"));
                    }
                    return crate::window_flow::StepPoll::Stay;
                }

                // Issue #34: reset clicked → confirm, tccutil reset, relaunch
                // (never returns on confirm).
                if SIGNALS.reset_clicked.swap(false, Ordering::SeqCst) {
                    reset_permission_and_relaunch();
                    return crate::window_flow::StepPoll::Stay;
                }

                // Issue #34: wait exceeded STALE_GRANT_TIMEOUT with no
                // progress → the grant is stale; surface the escape hatch
                // once.
                if perm_stale.load(Ordering::SeqCst) && !stale_shown.get() {
                    stale_shown.set(true);
                    reset_btn.setHidden(false);
                    let ns = NSString::from_str(STALE_GRANT_TEXT);
                    unsafe {
                        instr_label.setStringValue(&ns);
                        status_label.setStringValue(&NSString::from_str(
                            "Permission appears granted but is stale (old copy of the app).",
                        ));
                    }
                    return crate::window_flow::StepPoll::Stay;
                }

                // Poll resolves → done. The tray re-runs its startup
                // permission check after this returns, so no further
                // bookkeeping is needed.
                if perm_granted.load(Ordering::SeqCst) {
                    return crate::window_flow::StepPoll::Finish(
                        crate::window_flow::FlowOutcome(Ok(())),
                    );
                }

                // Keep the waiting text fresh (same pattern as the wizard's
                // SIGNALS.status guard — only fill when not already set).
                {
                    let mut status = SIGNALS.status.lock();
                    if status.is_empty() {
                        *status = "Waiting for Accessibility permission…".to_string();
                        let ns = NSString::from_str(status.as_str());
                        unsafe { status_label.setStringValue(&ns) };
                    }
                }
                crate::window_flow::StepPoll::Stay
            }),
            poller: Some(poller),
            // Close ends the flow in every phase (issue #36); the engine's
            // close-poll handles it. Preserve the migrated loop's close
            // message (the engine's generic message would lose the
            // re-grant-specific wording).
            close_outcome: Some(Box::new(|| {
                Err(anyhow!(
                    "Permission window closed before the Accessibility grant completed"
                ))
            })),
            // Anchor the WizardTarget NSView (close delegate + click
            // routing); widgets are retained by the content stack.
            keep_alive: vec![unsafe { Retained::cast::<NSView>(target) }],
        };
        crate::window_flow::run_flow(spec, event_loop)
    }

    /// Help window: a plain read-only NSWindow showing static styled
    /// sections (bold headings + plain body). Same
    /// primitives as the re-grant window — MainThreadMarker, Accessory
    /// activation, shared `WizardTarget` delegate for close handling, and
    /// the caller's tao loop via `run_return` — minus every interactive
    /// element: no buttons, no poll thread, no per-flow signals beyond
    /// close_requested.
    ///
    /// Returns when the window closes (always `Ok(())` unless window
    /// creation fails).
    pub(super) fn run_help_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
        sections: &[super::HelpSection],
    ) -> Result<()> {
        let mtm =
            MainThreadMarker::new().ok_or_else(|| anyhow!("Help must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        const HELP_WINDOW_H: f64 = 620.0;
        // Vertical gap between the stack's own children (4pt) vs between
        // whole sections (16pt), and the stack's edge insets (24pt, both
        // top and bottom = 48pt total). The exact-height formula below
        // depends on all three staying in sync.
        const HELP_STACK_SPACING: f64 = 4.0;
        const SECTION_GAP: f64 = 16.0;
        const HELP_STACK_INSETS: f64 = 24.0;
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, HELP_WINDOW_H));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("HandsOff Help"));
        unsafe { window.setReleasedWhenClosed(false) };

        let target: Retained<WizardTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };

        // Close button must terminate the flow (issue #36): the delegate's
        // windowShouldClose: sets close_requested, which the loop polls.
        {
            let delegate = ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        // Styled sections: one bold heading + plain body per HelpSection,
        // stacked vertically inside a scroll view (NSTextField labels do NOT
        // word-wrap, so body strings carry their own manual line breaks).
        let stack_view = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, HELP_WINDOW_H)),
            )
        };
        unsafe {
            stack_view.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            stack_view.setSpacing(HELP_STACK_SPACING);
            // Default vertical-stack alignment is centerX, which centers each
            // fixed-width label in the 480pt stack — force leading so all
            // content hugs the left edge.
            stack_view.setAlignment(NSLayoutAttribute::Leading);
            // HELP_STACK_INSETS padding all around; CONTENT_W already
            // accounts for it.
            stack_view.setEdgeInsets(NSEdgeInsets {
                top: HELP_STACK_INSETS,
                left: HELP_STACK_INSETS,
                bottom: HELP_STACK_INSETS,
                right: HELP_STACK_INSETS,
            });
        }
        const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
        const CONTENT_W: f64 = WINDOW_W - 60.0; // 480 - 24*2 insets - 12 slack
        // Track measured layout math so the stack can be sized to its
        // content after the loop (see stack_h below); otherwise the stack
        // keeps its init frame and the height-flexible grid absorbs the
        // leftover, rendering as a large empty block mid-table. Heights are
        // MEASURED per child (fittingSize at CONTENT_W), not estimated from
        // line counts: real line height at 13pt is ~16pt, not 18pt, and
        // estimates a few pt over per child accumulate into exactly the
        // leftover the grid then absorbs.
        let mut content_h: f64 = 0.0; // sum of child view heights
        let mut child_count: usize = 0;
        let mut last_view: Option<Retained<NSView>> = None;

        // Measure a label's natural height at the content width: set the
        // width, then read fittingSize (single-line labels do not word-wrap,
        // so this is exact for the manual-break strings help uses).
        unsafe fn measured_height(label: &NSTextField, width: f64) -> f64 {
            label.setFrameSize(NSSize::new(width, label.fittingSize().height.max(1.0)));
            label.fittingSize().height.max(1.0)
        }

        for section in sections {
            // Extra gap between sections, applied to the previous section's
            // last view before this section's heading is added.
            if let Some(prev) = &last_view {
                unsafe { stack_view.setCustomSpacing_afterView(SECTION_GAP, prev) };
            }

            let heading = unsafe {
                NSTextField::labelWithString(&NSString::from_str(&section.heading), mtm)
            };
            unsafe {
                heading.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
                heading.setAlignment(NSTextAlignment::Left);
                let h = measured_height(&heading, CONTENT_W);
                heading.setFrameSize(NSSize::new(CONTENT_W, h));
                let heading_view: Retained<NSView> = Retained::cast(heading);
                stack_view.addView_inGravity(&heading_view, GRAVITY);
                content_h += h;
                child_count += 1;
                last_view = Some(heading_view);
            }

            if !section.body.is_empty() {
                let body =
                    unsafe { NSTextField::labelWithString(&NSString::from_str(&section.body), mtm) };
                unsafe {
                    body.setFont(Some(&NSFont::systemFontOfSize(13.0)));
                    body.setAlignment(NSTextAlignment::Left);
                    let h = measured_height(&body, CONTENT_W);
                    body.setFrameSize(NSSize::new(CONTENT_W, h));
                    let body_view: Retained<NSView> = Retained::cast(body);
                    stack_view.addView_inGravity(&body_view, GRAVITY);
                    content_h += h;
                    child_count += 1;
                    last_view = Some(body_view);
                }
            }

            // Optional table: NSGridView aligns the item column by
            // construction (each row = [item label, description label]).
            if !section.table.is_empty() {
                let grid = unsafe {
                    NSGridView::initWithFrame(
                        mtm.alloc(),
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_W, 1.0)),
                    )
                };
                unsafe {
                    grid.setRowSpacing(2.0);
                    grid.setColumnSpacing(8.0);
                }
                for (item, desc) in &section.table {
                    let item_label = unsafe {
                        NSTextField::labelWithString(&NSString::from_str(item), mtm)
                    };
                    let desc_label = unsafe {
                        NSTextField::labelWithString(&NSString::from_str(desc), mtm)
                    };
                    unsafe {
                        item_label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
                        desc_label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
                        // SAFETY: NSTextField is an NSView subclass; the cast
                        // consumes the Retained and keeps the +1 alive in the
                        // array the grid retains.
                        let views = NSArray::from_vec(vec![
                            Retained::cast::<NSView>(item_label),
                            Retained::cast::<NSView>(desc_label),
                        ]);
                        grid.addRowWithViews(&views);
                    }
                }
                // Size the grid to its own fittingSize BEFORE adding it to
                // the stack: once in the stack it is the only
                // height-flexible child and would absorb any leftover stack
                // height (the empty-block bug). layoutSubtreeIfNeeded first:
                // without a layout pass fittingSize under-measures the last
                // row (verified: 368 vs 388 rendered).
                let grid_view: Retained<NSView> =
                    unsafe { Retained::cast(grid.clone()) };
                let grid_h = unsafe {
                    grid.layoutSubtreeIfNeeded();
                    let fs = grid_view.fittingSize();
                    grid_view.setFrameSize(NSSize::new(CONTENT_W, fs.height));
                    fs.height
                };
                unsafe { stack_view.addView_inGravity(&grid_view, GRAVITY) };
                content_h += grid_h;
                child_count += 1;
                last_view = Some(grid_view);
            }
        }

        // Size the stack to its exact measured content: insets (top+bottom)
        // + children + spacing between children + extra section gaps. Every
        // child was pre-sized, so the sum leaves no leftover for the grid to
        // absorb. setCustomSpacing_afterView REPLACES the default spacing
        // after that view, so the section gap contributes only the DELTA
        // (SECTION_GAP - HELP_STACK_SPACING) per section boundary — adding
        // the full gap on top of the default double-counts and hands the
        // excess to the grid (verified: 20pt excess for 5 boundaries).
        // (The stack-level fittingSize is NOT used: it distributes extra
        // height into the flexible grid — verified via probe.)
        let stack_h = 2.0 * HELP_STACK_INSETS
            + content_h
            + HELP_STACK_SPACING * (child_count.saturating_sub(1)) as f64
            + (SECTION_GAP - HELP_STACK_SPACING)
                * (sections.len().saturating_sub(1)) as f64;
        unsafe { stack_view.setFrameSize(NSSize::new(WINDOW_W, stack_h)) };

        let scroll_view = unsafe {
            NSScrollView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, HELP_WINDOW_H)),
            )
        };
        unsafe {
            scroll_view.setHasVerticalScroller(true);
            scroll_view.setDocumentView(Some(&stack_view));
            window.contentView().unwrap().addSubview(&scroll_view);
        }

        window.center();
        window.makeKeyAndOrderFront(None);
        activate_app(&app);

        // Flow-runner engine (issue #40): Help is the simplest flow — one
        // step, no poller, and its only exit path is the window close. The
        // engine owns the ceremony (begin_flow on entry, 100 ms cadence,
        // menu-drain, close-poll with orderOut + outcome write); the spec
        // supplies the static window and Help's close semantic (`Ok(())` —
        // closing Help is normal dismissal, not an error).
        let spec = crate::window_flow::FlowSpec {
            first_step: crate::window_flow::StepId(0),
            build_window: Box::new(move || {
                Ok(crate::window_flow::FlowWindow {
                    window,
                    app,
                })
            }),
            // Single step: nothing to render beyond the initial build.
            render_step: Box::new(|_| {}),
            // One step, no input: it never advances and never finishes on
            // its own — only the engine's close-poll ends the flow.
            poll_step: Box::new(|_| crate::window_flow::StepPoll::Stay),
            poller: None,
            // Closing Help is its normal exit: `Ok(())` (the migrated loop
            // wrote `Ok(())` and took the slot's `unwrap_or_else(Ok)`).
            close_outcome: Some(Box::new(|| Ok(()))),
            // Anchor the window's target object (a WizardTarget NSView) so
            // the close delegate lives as long as the flow (the widgets are
            // retained transitively by the scroll view's document view).
            keep_alive: vec![unsafe { Retained::cast::<NSView>(target) }],
        };
        crate::window_flow::run_flow(spec, event_loop)
    }

    /// Run one headless capture pass on the main thread, mirroring progress
    /// into the status label (entry prefix, dots, countdown) and transient
    /// messages into the feedback label (named reserved keys, too-short
    /// warnings, restarts).
    ///
    /// Called from inside the tao loop callback: the nested CFRunLoop pump
    /// in `capture_passphrase_headless` yields in 100 ms slices so AppKit
    /// keeps servicing its events while the tap is live. The `on_event`
    /// closure therefore runs on the MAIN thread and may touch AppKit
    /// directly (the loop callback is blocked inside the nested pump and
    /// cannot repaint).
    ///
    /// Silent model (ADR 0002, issue #36): feedback is dot COUNTS and named
    /// states only — never characters. Per-entry state is local (not the
    /// process-global SIGNALS), so consecutive entries and consecutive flows
    /// never leak dots into each other.
    fn capture_with_status(
        entry_label: &str,
        status_label: &Retained<NSTextField>,
        feedback_label: &Retained<NSTextField>,
    ) -> Result<Vec<u32>> {
        // Retained clones are refcounted — no lifetime tie to the caller.
        let status: Retained<NSTextField> = status_label.clone();
        let feedback: Retained<NSTextField> = feedback_label.clone();
        let entry = entry_label.to_string();
        let dots = Rc::new(RefCell::new(String::new()));
        let remaining = Rc::new(std::cell::Cell::new(setup::CAPTURE_TIMEOUT_SECS));

        let set_label = |label: &NSTextField, text: &str| unsafe {
            label.setStringValue(&NSString::from_str(text));
        };
        let render_status = {
            let dots = dots.clone();
            let remaining = remaining.clone();
            let entry = entry.clone();
            move || {
                let d = dots.borrow();
                format!("{}: {}  ({}s left)", entry, d, remaining.get())
            }
        };

        set_label(&status, &render_status());
        // Feedback label intentionally NOT cleared here: the caller sets it
        // (e.g. "First entry accepted" stays visible into the second entry).

        setup::capture_passphrase_headless(
            crate::constants::DEFAULT_LOCK_KEYCODE,
            crate::constants::DEFAULT_TALK_KEYCODE,
            Some(Box::new(move |ev| {
                // Main-thread only (nested CFRunLoop slices): AppKit is safe.
                match ev {
                    setup::CaptureEvent::Key => {
                        dots.borrow_mut().push('•');
                        set_label(&status, &render_status());
                    }
                    setup::CaptureEvent::Reserved(keycode) => {
                        // Name the rejected key (issue #36): a bare flash
                        // forced users to discover the reserved set by
                        // trial and error.
                        let text = match setup::reserved_key(
                            keycode,
                            crate::constants::DEFAULT_LOCK_KEYCODE,
                            crate::constants::DEFAULT_TALK_KEYCODE,
                        ) {
                            Some(k) => format!("{} is reserved ({}).", k.name, k.why),
                            None => "That key is reserved.".to_string(),
                        };
                        set_label(&feedback, &text);
                    }
                    setup::CaptureEvent::Restart => {
                        dots.borrow_mut().clear();
                        set_label(&status, &render_status());
                        set_label(&feedback, "Entry restarted.");
                    }
                    setup::CaptureEvent::Backspace => {
                        dots.borrow_mut().pop();
                        set_label(&status, &render_status());
                    }
                    setup::CaptureEvent::Tick(r) => {
                        remaining.set(r);
                        set_label(&status, &render_status());
                    }
                    setup::CaptureEvent::TooShort(len) => {
                        set_label(
                            &feedback,
                            &format!(
                                "{} keys so far — need at least {}.",
                                len,
                                crate::utils::MIN_PASSPHRASE_KEYS
                            ),
                        );
                    }
                }
            })),
        )
        .map_err(|e| anyhow!("Passphrase capture failed: {}", e))
    }

    /// Build the wizard outcome. The config constructor remains authoritative
    /// for every range; this validates only what the form needs pre-flight.
    fn build_outcome(
        keycodes: Vec<u32>,
        auto_lock: u64,
        auto_unlock_base: u64,
        lock: Option<String>,
        talk: Option<String>,
        login_item: LoginItemResult,
    ) -> Result<WizardOutcome> {
        if let (Some(l), Some(t)) = (&lock, &talk) {
            if l == t {
                return Err(anyhow!("Lock and Talk hotkeys must be different"));
            }
        }
        if !(AUTO_LOCK_MIN_SECONDS..=AUTO_LOCK_MAX_SECONDS).contains(&auto_lock) {
            return Err(anyhow!(
                "Auto-lock timeout must be {}-{} seconds",
                AUTO_LOCK_MIN_SECONDS,
                AUTO_LOCK_MAX_SECONDS
            ));
        }

        let auto_unlock = if auto_unlock_base == 0 {
            crate::config::AutoUnlockConfig::Disabled
        } else {
            crate::config::AutoUnlockConfig::Backoff {
                base_interval_secs: std::num::NonZeroU64::new(auto_unlock_base)
                    .ok_or_else(|| anyhow!("Auto-unlock interval must be positive"))?,
            }
        };

        Ok(WizardOutcome {
            setup: SetupOutcome {
                keycodes,
                auto_lock,
                auto_unlock,
                lock_key: lock,
                talk_key: talk,
            },
            login_item,
        })
    }

    // ------------------------------------------------------------------
    // Preferences window (issue #27) — same window stack, no passphrase
    // re-entry. Empty fields mean "unchanged"; Save applies via
    // `preferences::apply_preferences` (constructor revalidates, nothing is
    // written on a range violation).
    // ------------------------------------------------------------------

    pub(super) fn run_preferences_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
    ) -> Result<super::PreferencesOutcome> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow!("preferences must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, 240.0));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("HandsOff Preferences"));
        unsafe { window.setReleasedWhenClosed(false) };

        // Distinct target so wizard atomics are not clobbered (both modules
        // share the process, and the wizard may run later in the same loop).
        declare_class!(
            struct PrefsTarget;

            unsafe impl ClassType for PrefsTarget {
                type Super = NSView;
                type Mutability = mutability::MainThreadOnly;
                const NAME: &'static str = "HandsOffPrefsTarget";
            }

            impl DeclaredClass for PrefsTarget {
                type Ivars = ();
            }

            // Supertrait of NSWindowDelegate; conformance is required before
            // NSWindowDelegate can be implemented.
            unsafe impl NSObjectProtocol for PrefsTarget {}

            unsafe impl NSWindowDelegate for PrefsTarget {
                // Only the optional `windowShouldClose:` is implemented (below).
            }

            unsafe impl PrefsTarget {
                #[method(buttonClicked:)]
                fn button_clicked(&self, sender: &AnyObject) {
                    let tag: isize = unsafe { msg_send![sender, tag] };
                    if tag == TAG_SAVE {
                        SIGNALS.grant_clicked.store(true, Ordering::SeqCst);
                    }
                }

                // Close ends the flow (issue #36): without this the loop
                // kept polling a window that no longer existed (same wedge
                // class as the Change Passphrase phase-0 close).
                #[method(windowShouldClose:)]
                fn window_should_close(&self, _sender: &NSWindow) -> bool {
                    SIGNALS.close_requested.store(true, Ordering::SeqCst);
                    true
                }
            }
        );

        const TAG_SAVE: isize = 10;

        let target: Retained<PrefsTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };
        // Close button must terminate the flow (issue #36): the delegate's
        // windowShouldClose: sets close_requested, which the engine's
        // close-poll polls.
        {
            let delegate = ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        let make_label = |text: &str, width: f64, height: f64| -> Retained<NSTextField> {
            let l = unsafe { NSTextField::labelWithString(&NSString::from_str(text), mtm) };
            unsafe { l.setFrameSize(NSSize::new(width, height)) };
            l
        };
        let make_field = |placeholder: &str, width: f64| -> Retained<NSTextField> {
            let f = unsafe {
                NSTextField::initWithFrame(
                    mtm.alloc(),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 24.0)),
                )
            };
            unsafe { f.setPlaceholderString(Some(&NSString::from_str(placeholder))) };
            f
        };

        // Prefill from the CURRENT saved config so the user sees what is in
        // effect; empty-on-save means unchanged.
        let current = crate::config_file::Config::load();

        let instr_label = make_label(
            "Leave a field empty to keep its current value. The passphrase is not required here.",
            WINDOW_W - 60.0,
            30.0,
        );
        let lock_label = make_label("Lock hotkey (A-Z):", 150.0, 20.0);
        let lock_key = make_field("L", 48.0);
        let talk_label = make_label("Talk hotkey (A-Z):", 150.0, 20.0);
        let talk_key = make_field("T", 48.0);
        let auto_lock_label = make_label("Auto-lock timeout (s):", 150.0, 20.0);
        let auto_lock = make_field("120", 80.0);
        let auto_unlock_label = make_label("Auto-unlock base (s, 0=off):", 200.0, 20.0);
        let auto_unlock = make_field("3600", 80.0);
        let save_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Save"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { save_btn.setTag(TAG_SAVE) };

        if let Ok(cfg) = &current {
            let prefill =
                |f: &NSTextField, v: String| unsafe { f.setStringValue(&NSString::from_str(&v)) };
            if let Some(k) = &cfg.lock_hotkey {
                prefill(&lock_key, k.clone());
            }
            if let Some(k) = &cfg.talk_hotkey {
                prefill(&talk_key, k.clone());
            }
            prefill(&auto_lock, cfg.auto_lock_timeout.to_string());
            if cfg.auto_unlock_backoff_enabled() {
                prefill(
                    &auto_unlock,
                    cfg.auto_unlock_base_interval.unwrap_or(0).to_string(),
                );
            } else {
                prefill(&auto_unlock, "0".to_string());
            }
        }

        let content = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, 240.0)),
            )
        };
        unsafe {
            content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            content.setSpacing(8.0);
            const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
            for v in [
                &*instr_label as *const NSTextField as *const NSView,
                &*lock_label as *const NSTextField as *const NSView,
                &*lock_key as *const NSTextField as *const NSView,
                &*talk_label as *const NSTextField as *const NSView,
                &*talk_key as *const NSTextField as *const NSView,
                &*auto_lock_label as *const NSTextField as *const NSView,
                &*auto_lock as *const NSTextField as *const NSView,
                &*auto_unlock_label as *const NSTextField as *const NSView,
                &*auto_unlock as *const NSTextField as *const NSView,
                &*save_btn as *const NSButton as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        window.center();
        window.makeKeyAndOrderFront(None);
        activate_app(&app);

        // Flow-runner engine (issue #40): the hand-copied run_return loop
        // (cadence, menu drain, close-poll, exit path) is deleted; the
        // engine owns the ceremony and the spec owns the form. One step:
        // Save routes through the TAG_SAVE signal the PrefsTarget records;
        // the poll reads the fields, validates pre-flight, and either
        // finishes with the outcome or re-titles the window with the
        // reason (window stays open — same as the migrated loop).
        // The engine's build_window takes the window; the poll re-titles
        // it on validation failure, so keep a Retained clone for the poll.
        let window_for_poll = window.clone();
        let spec = crate::window_flow::FlowSpec {
            first_step: crate::window_flow::StepId(0),
            build_window: Box::new(move || Ok(crate::window_flow::FlowWindow { window, app })),
            // Single step: nothing to swap between phases.
            render_step: Box::new(|_| {}),
            poll_step: Box::new(move |_| {
                if !SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                    return crate::window_flow::StepPoll::Stay;
                }
                let parse_field = |f: &NSTextField| -> Option<u64> {
                    let s: String = unsafe { f.stringValue() }.to_string();
                    let s = s.trim().to_string();
                    if s.is_empty() {
                        None
                    } else {
                        s.parse::<u64>().ok()
                    }
                };
                let hotkey_field = |f: &NSTextField| -> Option<String> {
                    let s: String = unsafe { f.stringValue() }.to_string();
                    let s = s.trim().to_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_uppercase())
                    }
                };

                let edit = crate::preferences::PreferencesEdit {
                    lock_key: hotkey_field(&lock_key),
                    talk_key: hotkey_field(&talk_key),
                    auto_lock: parse_field(&auto_lock),
                    auto_unlock_base: parse_field(&auto_unlock),
                };

                // Range/distinctness validation happens in apply_preferences;
                // a parse failure (non-numeric timeout) is treated as
                // "unchanged" rather than discarding the other edits — but a
                // hotkey that fails A-Z validation fails the save and the
                // window stays open (title shows the reason).
                match validate_preflight(&edit) {
                    Ok(()) => crate::window_flow::StepPoll::Finish(
                        crate::window_flow::FlowOutcome(Ok(super::PreferencesOutcome { edit })),
                    ),
                    Err(e) => {
                        window_for_poll.setTitle(&NSString::from_str(&format!(
                            "HandsOff Preferences — {}",
                            e
                        )));
                        crate::window_flow::StepPoll::Stay
                    }
                }
            }),
            poller: None,
            // Closing the window discards the form: "Preferences closed
            // without saving" (the migrated loop's close message).
            close_outcome: Some(Box::new(|| {
                Err(anyhow!("Preferences closed without saving"))
            })),
            // Anchor the PrefsTarget NSView so its delegate + click routing
            // live as long as the flow (widgets are retained by the stack).
            keep_alive: vec![unsafe { Retained::cast::<NSView>(target) }],
        };
        crate::window_flow::run_flow(spec, event_loop)
    }

    /// Pre-flight validation for the Preferences form: only what the form
    /// itself can check cheaply (parse failures on provided values, hotkey
    /// shape). Range validation stays in the `Config` constructor.
    fn validate_preflight(edit: &crate::preferences::PreferencesEdit) -> Result<()> {
        if let Some(k) = &edit.lock_key {
            crate::config_file::Config::validate_hotkey(k)?;
        }
        if let Some(k) = &edit.talk_key {
            crate::config_file::Config::validate_hotkey(k)?;
        }
        if let (Some(l), Some(t)) = (&edit.lock_key, &edit.talk_key) {
            if l == t {
                return Err(anyhow!("Lock and Talk hotkeys must be different"));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Change Passphrase (issue #27) — silent double capture, config saved
    // with only the hash changed.
    // ------------------------------------------------------------------

    /// Swap a dialog to its terminal state: message + OK button, capture
    /// widgets hidden (issue #36 — success and failure are shown IN the
    /// dialog; the user dismisses deliberately).
    #[allow(clippy::too_many_arguments)]
    fn show_terminal_state(
        instr_label: &NSTextField,
        capture_btn: &NSButton,
        cancel_btn: &NSButton,
        ok_btn: &NSButton,
        status_label: &NSTextField,
        feedback_label: &NSTextField,
        message: &str,
    ) {
        unsafe { instr_label.setStringValue(&NSString::from_str(message)) };
        capture_btn.setHidden(true);
        cancel_btn.setHidden(true);
        status_label.setHidden(true);
        feedback_label.setHidden(true);
        ok_btn.setHidden(false);
    }

    pub(super) fn run_change_passphrase_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
    ) -> Result<Config> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow!("change passphrase must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        // Wide enough that the instructions (including the reserved-key
        // list) fit without clipping — issue #36.
        const CP_WINDOW_W: f64 = 560.0;
        const CP_WINDOW_H: f64 = 340.0;

        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(CP_WINDOW_W, CP_WINDOW_H),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("HandsOff — Change Passphrase"));
        unsafe { window.setReleasedWhenClosed(false) };

        let target: Retained<WizardTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };
        // Close must terminate the flow in EVERY phase (issue #36 wedge
        // fix): the delegate sets close_requested (and aborts an in-flight
        // capture); the engine's close-poll polls the flag in the waiting
        // and terminal phases, capture_with_status observes the abort
        // mid-capture.
        {
            let delegate = ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        let make_label = |text: &str, height: f64| -> Retained<NSTextField> {
            let l = unsafe { NSTextField::labelWithString(&NSString::from_str(text), mtm) };
            unsafe { l.setFrameSize(NSSize::new(CP_WINDOW_W - 60.0, height)) };
            l
        };
        let make_button = |title: &str, tag: isize| -> Retained<NSButton> {
            let b = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(title),
                    Some(&*target),
                    Some(objc2::sel!(buttonClicked:)),
                    mtm,
                )
            };
            unsafe { b.setTag(tag) };
            b
        };

        // Wrapping label: instruction text word-wraps at the content width
        // (plain labels bleed off-window); Auto Layout grows the stack
        // vertically for multi-paragraph text.
        let instr_label = unsafe {
            NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm)
        };
        unsafe {
            instr_label.setFrameSize(NSSize::new(CP_WINDOW_W - 60.0, 210.0));
            instr_label.setPreferredMaxLayoutWidth(CP_WINDOW_W - 60.0);
            instr_label.setMaximumNumberOfLines(0);
        }
        // Explicit capture start (keyboard-lockout fix): capture runs only
        // after this click, never automatically on window open.
        // Issue #37 N6: phase 0 is now VERIFY — the user must first prove
        // knowledge of the CURRENT Passphrase; only a verified capture moves
        // to phase 1 (the new-Passphrase double capture).
        let capture_btn = make_button("Verify Current Passphrase", TAG_CAPTURE);
        let cancel_btn = make_button("Cancel", TAG_CANCEL);
        let ok_btn = make_button("OK", TAG_OK);
        ok_btn.setHidden(true);
        let status_label = make_label("", 20.0);
        status_label.setHidden(true);
        let feedback_label = make_label("", 20.0);
        feedback_label.setHidden(true);

        // Stored hash for the verify step (N6): loaded from the config file,
        // the same authority `change_passphrase` re-reads when saving.
        let stored_hash: Option<String> = crate::config_file::Config::load()
            .ok()
            .and_then(|c| c.passphrase_hash);

        // Permission gate as an in-dialog failure state (issue #36: every
        // failure shows its reason the same way). Previously this returned
        // Err before any window existed; with the leak fix (orderOut on
        // every exit) the window is safe to use as the failure surface.
        let permission_ok = crate::input_blocking::check_accessibility_permissions();
        // phase: 0 = waiting for the VERIFY click (current Passphrase);
        // 1 = waiting for the capture click (new Passphrase, double entry);
        // 2 = success shown, waiting for OK; 3 = failure shown, waiting for OK.
        // Shared with poll_step/close_outcome below via Cells/RefCells.
        let phase = std::cell::Cell::new(0u8);
        let failure_reason = std::cell::RefCell::new(String::new());
        if !permission_ok {
            let msg = "Accessibility permission is required to capture the new \
                 Passphrase.\nGrant it to HandsOff in System Settings > Privacy & Security > \
                 Accessibility, then try again.\nYour existing Passphrase is unchanged."
                .to_string();
            phase.set(3);
            failure_reason.replace(msg.clone());
            show_terminal_state(
                &instr_label,
                &capture_btn,
                &cancel_btn,
                &ok_btn,
                &status_label,
                &feedback_label,
                &msg,
            );
        } else if stored_hash.is_none() {
            let msg = "No stored Passphrase hash found in the configuration — \
                 cannot verify the current Passphrase.\nYour existing Passphrase is unchanged."
                .to_string();
            phase.set(3);
            failure_reason.replace(msg.clone());
            show_terminal_state(
                &instr_label,
                &capture_btn,
                &cancel_btn,
                &ok_btn,
                &status_label,
                &feedback_label,
                &msg,
            );
        } else {
            phase.set(0);
            unsafe {
                instr_label.setStringValue(&NSString::from_str(&format!(
                    "To change your Passphrase, first type your CURRENT Passphrase to \
                     prove it is you.\n\nNothing you type is ever shown — dots mark \
                     progress only. Your keyboard is captured until you press Enter \
                     (max {}s); nothing you type reaches other apps.\n\n\
                     Click “Verify Current Passphrase” when ready.",
                    setup::CAPTURE_TIMEOUT_SECS,
                )))
            };
        }

        let content = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(CP_WINDOW_W, CP_WINDOW_H),
                ),
            )
        };
        unsafe {
            content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            content.setSpacing(12.0);
            const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
            for v in [
                &*instr_label as *const NSTextField as *const NSView,
                &*capture_btn as *const NSButton as *const NSView,
                &*cancel_btn as *const NSButton as *const NSView,
                &*ok_btn as *const NSButton as *const NSView,
                &*status_label as *const NSTextField as *const NSView,
                &*feedback_label as *const NSTextField as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        window.center();
        window.makeKeyAndOrderFront(None);
        activate_app(&app);

        // Flow-runner engine (issue #40): the hand-copied run_return loop
        // is deleted; the engine owns the ceremony and the spec owns the
        // four-phase state machine declared above (shared Cell/RefCell
        // state, main-thread only).
        //
        // Capture in flight is "Stay": the captures run the nested
        // CFRunLoop pump INSIDE a poll_step call (one long tick — the
        // pump yields 100 ms slices so AppKit keeps servicing); the
        // engine's close-poll simply doesn't fire while the pump owns
        // the thread, and capture_with_status observes the abort via
        // windowShouldClose → close_requested + request_capture_abort.
        //
        // Single step (StepId(0)): the phase cell IS the state machine;
        // render swaps happen inline in poll_step exactly as the migrated
        // loop updated widgets in place.
        let failure_reason_setter = failure_reason.clone();
        let saved = std::cell::RefCell::new(None::<Config>);
        // Captured CURRENT Passphrase from the verify phase (issue #37
        // N6); reused by the phase-1 save so the re-key step proves
        // knowledge of the existing Passphrase.
        let current_keys: std::cell::RefCell<Vec<u32>> =
            std::cell::RefCell::new(Vec::new());

        macro_rules! show_terminal {
            ($msg:expr) => {{
                failure_reason_setter.replace($msg.to_string());
                show_terminal_state(
                    &instr_label,
                    &capture_btn,
                    &cancel_btn,
                    &ok_btn,
                    &status_label,
                    &feedback_label,
                    &$msg.to_string(),
                );
            }};
        }

        let phase_for_close = phase.clone();
        let failure_for_close = failure_reason.clone();
        let saved_for_close = saved.clone();
        let spec = crate::window_flow::FlowSpec {
            first_step: crate::window_flow::StepId(0),
            build_window: Box::new(move || Ok(crate::window_flow::FlowWindow { window, app })),
            render_step: Box::new(|_| {}),
            poll_step: Box::new(move |_| {
                let p = phase.get();
                if p == 0 || p == 1 {
                    // Phase-0/1 close/cancel: the 2026-10-02 wedge —
                    // previously only capture observed the abort, so
                    // closing here spun forever. Now every exit path
                    // terminates the flow (close via the engine's
                    // close-poll, cancel here).
                    if SIGNALS.cancel_clicked.swap(false, Ordering::SeqCst) {
                        phase.set(3);
                        failure_reason.replace(
                            "Change Passphrase cancelled — config unchanged".to_string(),
                        );
                        return crate::window_flow::StepPoll::Finish(
                            crate::window_flow::FlowOutcome(Err(anyhow!(
                                "Change Passphrase cancelled — config unchanged"
                            ))),
                        );
                    }

                    if SIGNALS.capture_clicked.swap(false, Ordering::SeqCst) {
                        capture_btn.setHidden(true);
                        cancel_btn.setHidden(true);
                        status_label.setHidden(false);
                        feedback_label.setHidden(false);

                        // N6 phase 0: capture the CURRENT Passphrase once
                        // and verify it against the stored hash. A failed
                        // verification aborts the flow with a dialog — no
                        // state change.
                        if p == 0 {
                            unsafe {
                                instr_label.setStringValue(&NSString::from_str(
                                    "Verifying — type your CURRENT Passphrase, then press Enter.",
                                ))
                            };
                            match capture_with_status(
                                "Current Passphrase",
                                &status_label,
                                &feedback_label,
                            ) {
                                Ok(keys_captured) => {
                                    let verified = stored_hash.as_ref().is_some_and(|h| {
                                        crate::auth::verify_keycodes(&keys_captured, h)
                                    });
                                    if !verified {
                                        let msg = "The Passphrase you typed does not \
                                             match the current Passphrase.\nThe Passphrase was \
                                             NOT changed — your existing Passphrase still works."
                                            .to_string();
                                        show_terminal!(msg);
                                        phase.set(3);
                                        return crate::window_flow::StepPoll::Stay;
                                    }
                                    // Verified: move to the new-Passphrase
                                    // phase.
                                    current_keys.replace(keys_captured);
                                    phase.set(1);
                                    capture_btn.setHidden(false);
                                    unsafe {
                                        capture_btn.setTitle(&NSString::from_str(
                                            "Capture New Passphrase",
                                        ));
                                    }
                                    cancel_btn.setHidden(false);
                                    status_label.setHidden(true);
                                    feedback_label.setHidden(true);
                                    unsafe {
                                        instr_label.setStringValue(&NSString::from_str(&format!(
                                            "Current Passphrase verified. Now choose a NEW \
                                             Passphrase: a sequence of physical keys. Nothing \
                                             you type is ever shown — dots mark progress only. \
                                             You will type it twice to confirm.\n\n{}\n\n\
                                             TIP: pick something SHORT — 4–6 keys you can \
                                             type with one hand — and WRITE IT DOWN somewhere \
                                             safe. If you ever get locked out and forget it, \
                                             rebooting your Mac is the only way back in.\n\n\
                                             Click “Capture New Passphrase” when ready.",
                                            setup::capture_rules_text(
                                                crate::constants::DEFAULT_LOCK_KEYCODE,
                                                crate::constants::DEFAULT_TALK_KEYCODE,
                                            ),
                                        )))
                                    };
                                    return crate::window_flow::StepPoll::Stay;
                                }
                                Err(e) => {
                                    if SIGNALS.close_requested.load(Ordering::SeqCst) {
                                        return crate::window_flow::StepPoll::Finish(
                                            crate::window_flow::FlowOutcome(Err(anyhow!(
                                                "Change Passphrase cancelled — config unchanged"
                                            ))),
                                        );
                                    }
                                    let msg = format!(
                                        "Capture failed: {e}\nThe Passphrase was NOT changed — \
                                         your existing Passphrase still works."
                                    );
                                    show_terminal!(msg);
                                    phase.set(3);
                                    return crate::window_flow::StepPoll::Stay;
                                }
                            }
                        }

                        // Phase 1: double capture of the NEW Passphrase.
                        unsafe {
                            instr_label.setStringValue(&NSString::from_str(
                                "Capturing — type your new Passphrase, then press Enter.",
                            ))
                        };

                        // Double capture on the main thread (same contract
                        // as the wizard): the nested CFRunLoop pump keeps
                        // AppKit servicing; closing the window mid-capture
                        // aborts the tap via windowShouldClose:. Every
                        // error path routes to the in-dialog terminal state
                        // (or clean cancel when the window was closed) —
                        // issue #36.
                        macro_rules! capture_entry {
                            ($label:expr) => {
                                match capture_with_status(
                                    $label,
                                    &status_label,
                                    &feedback_label,
                                ) {
                                    Ok(k) => Ok(k),
                                    Err(e) => {
                                        if SIGNALS.close_requested.load(Ordering::SeqCst) {
                                            return crate::window_flow::StepPoll::Finish(
                                                crate::window_flow::FlowOutcome(Err(anyhow!(
                                                    "Change Passphrase cancelled — config unchanged"
                                                ))),
                                            );
                                        }
                                        let msg = format!(
                                            "Capture failed: {e}\nThe Passphrase was NOT changed — \
                                             your existing Passphrase still works."
                                        );
                                        show_terminal!(msg);
                                        phase.set(3);
                                        Err(())
                                    }
                                }
                            };
                        }

                        let first = match capture_entry!("Entry 1 of 2") {
                            Ok(k) => k,
                            Err(()) => return crate::window_flow::StepPoll::Stay,
                        };
                        unsafe {
                            feedback_label.setStringValue(&NSString::from_str(
                                "First entry accepted — re-enter the same Passphrase to confirm.",
                            ))
                        };
                        let second = match capture_entry!("Entry 2 of 2") {
                            Ok(k) => k,
                            Err(()) => return crate::window_flow::StepPoll::Stay,
                        };

                        if first != second {
                            let msg = "Entries did not match — the Passphrase was NOT \
                                 changed. Your existing Passphrase still works."
                                .to_string();
                            show_terminal!(msg);
                            phase.set(3);
                            return crate::window_flow::StepPoll::Stay;
                        }

                        // Save through the verified seam (issue #37 N6):
                        // the current Passphrase was already captured and
                        // verified against the stored hash in phase 0 —
                        // the save path re-checks it, so no state change
                        // can bypass authentication even if the flow
                        // ordering changed.
                        match crate::preferences::change_passphrase_verified_to_path(
                            &crate::config_file::Config::config_path(),
                            &current_keys.borrow(),
                            &first,
                        ) {
                            Ok(cfg) => {
                                saved.replace(Some(cfg));
                                phase.set(2);
                                let msg =
                                    "Passphrase changed — use the new Passphrase to unlock."
                                        .to_string();
                                show_terminal!(msg);
                            }
                            Err(e) => {
                                let msg = format!(
                                    "Could not save the new Passphrase: {e}\nYour existing \
                                     Passphrase still works."
                                );
                                show_terminal!(msg);
                                phase.set(3);
                            }
                        }
                    }
                    crate::window_flow::StepPoll::Stay
                } else {
                    // Terminal phases: OK (or the window close button)
                    // dismisses. Close routes through the engine's
                    // close-poll + this spec's close_outcome (below); OK
                    // finishes here.
                    if SIGNALS.ok_clicked.swap(false, Ordering::SeqCst) {
                        if phase.get() == 2 {
                            let cfg = saved
                                .borrow_mut()
                                .take()
                                .expect("success state holds the saved config");
                            return crate::window_flow::StepPoll::Finish(
                                crate::window_flow::FlowOutcome(Ok(cfg)),
                            );
                        }
                        let reason = failure_reason.borrow().clone();
                        return crate::window_flow::StepPoll::Finish(
                            crate::window_flow::FlowOutcome(Err(anyhow!("{}", reason))),
                        );
                    }
                    crate::window_flow::StepPoll::Stay
                }
            }),
            poller: None,
            // Phase-dependent close semantics (the migrated loop's `closed`
            // check): a close during the waiting phases (0/1) cancels the
            // flow; a close in a terminal phase (2/3) dismisses with that
            // phase's outcome — exactly the migrated loop's `|| closed`
            // handling. Preserves "Change Passphrase cancelled — config
            // unchanged" verbatim.
            close_outcome: Some(Box::new(move || {
                if phase_for_close.get() == 2 {
                    let cfg = saved_for_close
                        .borrow_mut()
                        .take()
                        .expect("success state holds the saved config");
                    Ok(cfg)
                } else if phase_for_close.get() == 3 {
                    Err(anyhow!("{}", failure_for_close.borrow()))
                } else {
                    Err(anyhow!("Change Passphrase cancelled — config unchanged"))
                }
            })),
            // Anchor the WizardTarget NSView; the engine holds it until the
            // flow ends (replaces the migrated loop's post-loop
            // window_for_cleanup orderOut — the engine's finish path
            // orderOuts on every exit, then run_flow returns).
            keep_alive: vec![unsafe { Retained::cast::<NSView>(target) }],
        };
        crate::window_flow::run_flow(spec, event_loop)
    }
}

/// Outcome of the Preferences window (issue #27).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default)]
pub struct PreferencesOutcome {
    /// Raw field contents; `None`-valued fields mean "leave unchanged".
    /// Change-passphrase request is a separate flow (its own menu item).
    pub edit: crate::preferences::PreferencesEdit,
}

#[cfg(target_os = "macos")]
pub fn run_preferences(
    event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<PreferencesOutcome> {
    self::macos::run_preferences_macos(event_loop)
}

#[cfg(not(target_os = "macos"))]
pub fn run_preferences(
    _event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<PreferencesOutcome> {
    anyhow::bail!("Preferences require macOS")
}

/// Run the Change Passphrase flow on the caller's event loop: silent double
/// capture via the same path the wizard uses, then save with only the
/// passphrase hash changed (every other field preserved — spec #24 story 14).
///
/// Returns the saved config on success. Window-closed or mismatch aborts
/// without touching the config.
#[cfg(target_os = "macos")]
pub fn run_change_passphrase(
    event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<Config> {
    self::macos::run_change_passphrase_macos(event_loop)
}

#[cfg(not(target_os = "macos"))]
pub fn run_change_passphrase(
    _event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
) -> Result<Config> {
    anyhow::bail!("Change Passphrase requires macOS")
}

/// Run the Help window on the caller's event loop: static styled sections
/// (bold headings + plain body text).
///
/// Returns when the window closes (always `Ok(())` unless window creation
/// fails); there are no buttons or other interactive elements.
#[cfg(target_os = "macos")]
pub fn run_help(
    event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
    sections: &[HelpSection],
) -> Result<()> {
    self::macos::run_help_macos(event_loop, sections)
}

#[cfg(not(target_os = "macos"))]
pub fn run_help(
    _event_loop: &mut tao::event_loop::EventLoop<WizardEvent>,
    _sections: &[HelpSection],
) -> Result<()> {
    anyhow::bail!("Help requires macOS")
}

/// Bring the app to the front before showing a modal dialog (see
/// `self::macos::activate_app` for the deployment-target rationale).
/// Shared with the tray's native alert helpers (issue #39).
#[cfg(target_os = "macos")]
pub fn activate_app(app: &objc2_app_kit::NSApplication) {
    self::macos::activate_app(app);
}
