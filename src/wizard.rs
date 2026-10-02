//! Setup Wizard: single native window guiding first-run configuration.
//!
//! ADR 0002: replaces the terminal `--setup` gauntlet for average users. The
//! tray launches this wizard whenever the config is absent or fails strict
//! validation (`setup::validate_config_strict`); a valid config never shows
//! it (Preferences is a separate #27 concern).
//!
//! Steps, in order (spec #24):
//! 1. Permission explanation + Grant button → opens System Settings pane.
//! 2. Poll until TCC grants Accessibility (background check, status label).
//! 3. Passphrase capture: SILENT physical-key capture via the same session
//!    event tap the TUI uses (`setup::capture_passphrase_headless`) — raw
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
pub fn startup_flow(
    config_valid: bool,
    has_accessibility_permissions: bool,
) -> StartupFlow {
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

#[cfg(target_os = "macos")]
mod macos {
    use super::{LoginItemResult, WizardOutcome};
    use crate::constants::{
        AUTO_LOCK_MAX_SECONDS, AUTO_LOCK_MIN_SECONDS, AUTO_LOCK_DEFAULT_SECONDS,
    };
    use crate::config_file::Config;
    use crate::setup::{self, SetupOutcome};
    use anyhow::{anyhow, Result};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
    use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSAlert, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton,
        NSControlStateValueOn, NSModalResponseOK, NSStackView, NSStackViewGravity, NSTextField,
        NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::cell::RefCell;
    use std::rc::Rc;
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
    const STALE_GRANT_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(30);

    const STALE_GRANT_TEXT: &str = "Still stuck? If the HandsOff checkbox in System Settings > \
         Privacy & Security > Accessibility is already ticked but this window keeps waiting, \
         the permission is bound to an old copy of the app (this happens after an update).\n\n\
         Click “Reset Permission & Restart…” to clear it — HandsOff relaunches and asks for \
         the permission fresh. Your passphrase and settings are NOT affected.";

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
                step: AtomicU8::new(STEP_PERMISSION),
                status: parking_lot::Mutex::new(String::new()),
                waiting_since: parking_lot::Mutex::new(None),
            })
        }
    }

    static SIGNALS: std::sync::LazyLock<Arc<WizardSignals>> =
        std::sync::LazyLock::new(WizardSignals::new);

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
                    _ => {}
                }
            }

            // NSWindowDelegate (optional method): the red close button ends
            // an in-flight passphrase capture immediately. The wizard's tao
            // event loop is blocked inside the capture's nested CFRunLoop
            // pump, so CloseRequested cannot be processed until capture
            // ends — but AppKit dispatch (this delegate callback) still runs
            // during the pump, so the abort flag is seen within one 100 ms
            // pump slice.
            #[method(windowShouldClose:)]
            fn window_should_close(&self, _sender: &NSWindow) -> bool {
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

    /// Click the reset escape hatch: confirm, run `tccutil reset
    /// Accessibility <TCC_SERVICE>`, then relaunch the app. The reset is
    /// the same action the issue verified live as the remediation; the
    /// app cannot clear its own row silently (tccd requires user
    /// consent), hence the confirmation dialog. On confirm this never
    /// returns (relaunch exits the process); on cancel it falls through
    /// so the wizard keeps waiting.
    fn reset_permission_and_relaunch() {
        let mtm = MainThreadMarker::new()
            .expect("reset dialog must run on main thread");
        let alert = unsafe { NSAlert::new(mtm) };
        unsafe {
            alert.setMessageText(&NSString::from_str(
                "Reset Accessibility permission?",
            ));
            alert.setInformativeText(&NSString::from_str(
                "This removes HandsOff's (stale) Accessibility entry in System \
                 Settings, then relaunches HandsOff so it can request the \
                 permission fresh. Your passphrase and settings are NOT affected.",
            ));
            alert.addButtonWithTitle(&NSString::from_str("Reset & Restart"));
            alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        }
        if unsafe { alert.runModal() } != NSModalResponseOK {
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
                    // spawning but still holds the flock — child must
                    // skip the single-instance guard.
                    .chain(["--skip-instance-lock".to_string()]),
            )
            .spawn()
        {
            Ok(_) => {
                log::info!("Relaunched after reset; exiting");
                std::process::exit(0);
            }
            Err(e) => {
                log::error!("Relaunch failed ({}); exiting so the user can start HandsOff manually", e);
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
        let service: Retained<objc2::runtime::AnyObject> = unsafe {
            msg_send_id![some_class, mainAppService]
        };
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

        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Resizable;
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
        let instr_label = make_label("", WINDOW_W - 60.0, 40.0);

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
            let delegate =
                ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
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
                "HandsOff blocks all keyboard and mouse input until you type your secret passphrase. \
                 For that it needs the macOS Accessibility permission — granted to HandsOff itself, \
                 not a terminal.\n\nClick the button, then tick the box for HandsOff in System Settings.\
                 \n\nNote: HandsOff is unsigned. If macOS blocks it from launching, right-click \
                 (or Control-click) HandsOff.app and choose Open, then confirm Open in the dialog. \
                 You only need to do this once.",
            ))
        };

        window.center();
        window.makeKeyAndOrderFront(None);
        // `-[NSApplication activate]` is macOS 14+; min version is 13.0
        // (Info.plist.template), where the unrecognized selector would crash
        // the first-run path. activateIgnoringOtherApps exists since 10.0.
        unsafe { app.activateIgnoringOtherApps(true) };

        // ---- Drive the flow through the CALLER's tao event loop ----
        use tao::event::Event;
        use tao::platform::run_return::EventLoopExtRunReturn;

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
        // SIGNALS is process-global and shared across flows (wizard,
        // re-grant, preferences): clear the stale-detection clock so a
        // previous flow's instant can't make this flow's reset button
        // appear before the user even clicks Grant.
        *SIGNALS.waiting_since.lock() = None;
        std::thread::spawn({
            let perm_granted = perm_granted.clone();
            let perm_stale = perm_stale.clone();
            move || loop {
                if crate::input_blocking::check_accessibility_permissions_lightweight()
                    && crate::input_blocking::check_accessibility_permissions()
                {
                    perm_granted.store(true, Ordering::SeqCst);
                    return;
                }
                {
                    // Clock starts when the user clicks Grant (set by the
                    // event loop), not at window open — reading the
                    // instructions can legitimately take longer than
                    // STALE_GRANT_TIMEOUT. No click yet → no timing.
                    let mut started = SIGNALS.waiting_since.lock();
                    if let Some(t0) = *started {
                        if t0.elapsed() >= STALE_GRANT_TIMEOUT {
                            drop(started);
                            perm_stale.store(true, Ordering::SeqCst);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });

        let outcome_slot = std::rc::Rc::new(std::cell::RefCell::new(
            None::<Result<WizardOutcome>>,
        ));
        let outcome = outcome_slot.clone();
        let mut first_keys: Option<Vec<u32>> = None;
        let mut capture_offered = false;
        let mut stale_shown = false;

        let status_label_retained: Retained<NSTextField> = status_label.clone();
        let set_status = move |text: &str| {
            *SIGNALS.status.lock() = text.to_string();
            let ns = NSString::from_str(text);
            unsafe { status_label_retained.setStringValue(&ns) };
        };

        event_loop.run_return(move |event, _, control_flow| {
            // Drive step transitions on every wake; cadence via WaitUntil.
            *control_flow = tao::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            );

            if let Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } = &event
            {
                *outcome.borrow_mut() = Some(Err(anyhow!("Setup wizard closed before completing")));
                app.stop(None);
                return;
            }

            let step = SIGNALS.step.load(Ordering::SeqCst);

            // Issue #34: reset clicked → confirm, tccutil reset, relaunch
            // (never returns on confirm).
            if SIGNALS.reset_clicked.swap(false, Ordering::SeqCst) {
                reset_permission_and_relaunch();
                return;
            }

            // Issue #34: wait exceeded STALE_GRANT_TIMEOUT with no progress
            // → the grant is stale; surface the escape hatch once.
            if perm_stale.load(Ordering::SeqCst) && !stale_shown {
                stale_shown = true;
                reset_btn.setHidden(false);
                // Long multi-line explanation → the multi-line instruction
                // label (status label is a 20 pt one-liner; it would clip).
                unsafe { instr_label.setStringValue(&NSString::from_str(STALE_GRANT_TEXT)) };
                set_status("Permission appears stale (bound to an old copy of the app).");
            }

            // Step 0 → 1: Grant clicked.
            if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                SIGNALS.step.store(STEP_CAPTURE, Ordering::SeqCst);
                // Issue #34: start the stale-detection clock (see poll thread).
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
                set_status("Waiting for permission…");
                return;
            }

            // Step 1: poll permission; when granted, show the explicit
            // capture button — the keyboard-capturing tap is installed ONLY
            // on a click (keyboard-lockout fix: silent automatic capture
            // locked the whole keyboard for up to 300 s per launch).
            // `step` stays STEP_CAPTURE until both captures succeed, so a
            // failed capture can be retried from the button.
            if step == STEP_CAPTURE {
                if perm_granted.load(Ordering::SeqCst) && SIGNALS.capture_clicked.swap(false, Ordering::SeqCst) {
                    // UI out of the way for capture; dots go to the status label.
                    instr_label.setHidden(true);
                    set_status("");
                    capture_btn.setHidden(true);

                    // Double capture on the main thread. The headless pump
                    // yields control in 100 ms slices, so the AppKit event
                    // loop keeps servicing while the tap is installed.
                    // Cancel path: closing the window sets the abort flag
                    // via windowShouldClose:, ending the capture and
                    // restoring keyboard input.
                    let first = match capture_with_status(&status_label) {
                        Ok(k) => k,
                        Err(e) => {
                            set_status(&format!(
                                "Capture failed: {e} — click the button to retry."
                            ));
                            capture_btn.setHidden(false);
                            return;
                        }
                    };
                    set_status("Re-enter the same passphrase to confirm…");

                    let second = match capture_with_status(&status_label) {
                        Ok(k) => k,
                        Err(e) => {
                            set_status(&format!(
                                "Capture failed: {e} — click the button to retry."
                            ));
                            capture_btn.setHidden(false);
                            return;
                        }
                    };

                    if first != second {
                        *outcome.borrow_mut() = Some(Err(anyhow!(
                            "Passphrases did not match — restart the wizard to try again"
                        )));
                        app.stop(None);
                        return;
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
                    return;
                }
                // Permission granted, capture not yet requested: show the
                // explicit start button once (a local flag, not the step —
                // the step must stay STEP_CAPTURE so the click above is
                // still reachable).
                if perm_granted.load(Ordering::SeqCst) && !capture_offered {
                    capture_offered = true;
                    grant_btn.setHidden(true);
                    status_label.setHidden(false);
                    capture_btn.setHidden(false);
                    unsafe {
                        instr_label.setHidden(false);
                        instr_label.setStringValue(&NSString::from_str(
                            "Accessibility granted. Click “Capture Passphrase” when ready.\n\n\
                             Your keyboard will be captured until you type a passphrase and press \
                             Enter (max 2 min). Nothing you type reaches other apps during capture. \
                             Close this window to cancel.",
                        ))
                    };
                    set_status("");
                    return;
                }
                // Keep the waiting text fresh while permission is pending.
                {
                    if SIGNALS.status.lock().is_empty() {
                        set_status("Waiting for Accessibility permission…");
                    }
                }
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

                let keys = match &first_keys {
                    Some(k) => k.clone(),
                    None => {
                        *outcome.borrow_mut() = Some(Err(anyhow!("No passphrase captured")));
                        app.stop(None);
                        return;
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
                        *outcome.borrow_mut() = Some(Ok(o));
                        app.stop(None);
                    }
                    Err(e) => {
                        window.setTitle(&NSString::from_str(&format!("HandsOff Setup — {}", e)));
                    }
                }
            }
        });

        let result = outcome_slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(anyhow!("Setup wizard event loop ended unexpectedly")));
        result
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

        let status_label: Retained<NSTextField> =
            unsafe { NSTextField::labelWithString(&NSString::from_str(""), mtm) };
        unsafe { status_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 20.0)) };
        status_label.setHidden(true);

        let instr_label: Retained<NSTextField> = unsafe {
            NSTextField::labelWithString(
                &NSString::from_str(
                    "HandsOff was updated, so macOS needs the Accessibility permission \
                     granted again to HandsOff itself — your passphrase and settings are \
                     NOT affected.\n\nClick the button, then tick the box for HandsOff in \
                     System Settings > Privacy & Security > Accessibility.",
                ),
                mtm,
            )
        };
        unsafe { instr_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 80.0)) };

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
        // Same activation path as the wizard (see run_wizard_macos):
        // `-[NSApplication activate]` is macOS 14+ and the min version is
        // 13.0 (Info.plist.template), where the unrecognized selector would
        // crash this path. activateIgnoringOtherApps exists since 10.0.
        unsafe { app.activateIgnoringOtherApps(true) };

        use tao::event::Event;
        use tao::platform::run_return::EventLoopExtRunReturn;

        // Same poll primitive the wizard uses (see run_wizard_macos): fast
        // lightweight AXIsProcessTrusted checks every 500 ms, then ONE
        // authoritative full test-tap check when the lightweight flips true.
        // Issue #34: no progress for STALE_GRANT_TIMEOUT → stale grant,
        // surface the reset escape hatch (checkbox reads ON, TCC row pinned
        // to an old CDHash; waiting longer can never succeed).
        let perm_granted = Arc::new(AtomicBool::new(false));
        let perm_stale = Arc::new(AtomicBool::new(false));
        // SIGNALS is process-global and shared across flows: clear the
        // stale-detection clock so a previous flow's instant can't make
        // the reset button appear before Grant is clicked.
        *SIGNALS.waiting_since.lock() = None;
        std::thread::spawn({
            let perm_granted = perm_granted.clone();
            let perm_stale = perm_stale.clone();
            move || loop {
                if crate::input_blocking::check_accessibility_permissions_lightweight()
                    && crate::input_blocking::check_accessibility_permissions()
                {
                    perm_granted.store(true, Ordering::SeqCst);
                    return;
                }
                {
                    // Clock starts when the user clicks Grant (set by the
                    // event loop), not at window open — reading the
                    // instructions can legitimately take longer than
                    // STALE_GRANT_TIMEOUT. No click yet → no timing.
                    let mut started = SIGNALS.waiting_since.lock();
                    if let Some(t0) = *started {
                        if t0.elapsed() >= STALE_GRANT_TIMEOUT {
                            drop(started);
                            perm_stale.store(true, Ordering::SeqCst);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });

        let outcome_slot: Rc<RefCell<Option<Result<()>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let outcome = outcome_slot.clone();
        let mut stale_shown = false;

        event_loop.run_return(move |event, _, control_flow| {
            *control_flow = tao::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            );

            if let Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } = &event
            {
                *outcome.borrow_mut() = Some(Err(anyhow!(
                    "Permission window closed before the Accessibility grant completed"
                )));
                app.stop(None);
                return;
            }

            // Grant clicked → open the System Settings pane, wait for the poll.
            if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                open_accessibility_settings();
                // Issue #34: start the stale-detection clock (see poll thread).
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
                return;
            }

            // Issue #34: reset clicked → confirm, tccutil reset, relaunch
            // (never returns on confirm).
            if SIGNALS.reset_clicked.swap(false, Ordering::SeqCst) {
                reset_permission_and_relaunch();
                return;
            }

            // Issue #34: wait exceeded STALE_GRANT_TIMEOUT with no progress
            // → the grant is stale; surface the escape hatch once.
            if perm_stale.load(Ordering::SeqCst) && !stale_shown {
                stale_shown = true;
                reset_btn.setHidden(false);
                let ns = NSString::from_str(STALE_GRANT_TEXT);
                unsafe {
                    instr_label.setStringValue(&ns);
                    status_label.setStringValue(&NSString::from_str(
                        "Permission appears granted but is stale (bound to an old copy of the app).",
                    ));
                }
                return;
            }

            // Poll resolves → done. The tray re-runs its startup permission
            // check after this returns, so no further bookkeeping is needed.
            if perm_granted.load(Ordering::SeqCst) {
                *outcome.borrow_mut() = Some(Ok(()));
                app.stop(None);
                return;
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
        });

        let result = outcome_slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(anyhow!("Permission re-grant event loop ended unexpectedly")));
        result
    }

    /// Run one headless capture pass on the main thread, mirroring progress
    /// into the status label (dots, countdown ticks).
    ///
    /// Called from inside the tao loop callback: the nested CFRunLoop pump
    /// in `capture_passphrase_headless` yields in 100 ms slices so AppKit
    /// keeps servicing its events while the tap is live. The `on_event`
    /// closure therefore runs on the MAIN thread and may touch AppKit
    /// directly — the label is the only progress surface (the loop callback
    /// is blocked inside the nested pump and cannot repaint).
    fn capture_with_status(status_label: &Retained<NSTextField>) -> Result<Vec<u32>> {
        // Retained clone is refcounted — no lifetime tie to the caller.
        let label: Retained<NSTextField> = status_label.clone();
        setup::capture_passphrase_headless(
            crate::constants::DEFAULT_LOCK_KEYCODE,
            crate::constants::DEFAULT_TALK_KEYCODE,
            Some(Box::new(move |ev| {
                // Main-thread only (nested CFRunLoop slices): AppKit is safe.
                let text = match &ev {
                    setup::CaptureEvent::Key => {
                        let mut s = SIGNALS.status.lock();
                        s.push('•');
                        s.clone()
                    }
                    setup::CaptureEvent::Reserved => {
                        let mut s = SIGNALS.status.lock();
                        s.push_str(" [reserved] ");
                        s.clone()
                    }
                    setup::CaptureEvent::Restart => {
                        SIGNALS.status.lock().clear();
                        String::new()
                    }
                    setup::CaptureEvent::Backspace => {
                        let mut s = SIGNALS.status.lock();
                        s.pop();
                        s.clone()
                    }
                    setup::CaptureEvent::Tick(remaining) => {
                        // Countdown: prefix the in-progress dots so the user
                        // sees both progress and the time bound.
                        let dots = SIGNALS.status.lock().clone();
                        let dots = match dots.find(" (") {
                            Some(i) => dots[..i].to_string(),
                            None => dots,
                        };
                        format!("{dots} ({remaining}s left)")
                    }
                    setup::CaptureEvent::TooShort(len) => {
                        let mut s = SIGNALS.status.lock();
                        *s = format!(
                            "{} so far — need at least {} keys",
                            len,
                            crate::utils::MIN_PASSPHRASE_KEYS
                        );
                        s.clone()
                    }
                };
                let ns = NSString::from_str(&text);
                unsafe {
                    label.setStringValue(&ns);
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
        let mtm =
            MainThreadMarker::new().ok_or_else(|| anyhow!("preferences must run on main thread"))?;

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

            unsafe impl PrefsTarget {
                #[method(buttonClicked:)]
                fn button_clicked(&self, sender: &AnyObject) {
                    let tag: isize = unsafe { msg_send![sender, tag] };
                    if tag == TAG_SAVE {
                        SIGNALS.grant_clicked.store(true, Ordering::SeqCst);
                    }
                }
            }
        );

        const TAG_SAVE: isize = 10;

        let target: Retained<PrefsTarget> = unsafe {
            let t = mtm.alloc().set_ivars(());
            msg_send_id![super(t), initWithFrame: frame]
        };

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
            let prefill = |f: &NSTextField, v: String| unsafe {
                f.setStringValue(&NSString::from_str(&v))
            };
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
        unsafe { app.activateIgnoringOtherApps(true) };

        use tao::event::Event;
        use tao::platform::run_return::EventLoopExtRunReturn;

        let outcome_slot = std::rc::Rc::new(std::cell::RefCell::new(
            None::<Result<super::PreferencesOutcome>>,
        ));
        let outcome = outcome_slot.clone();

        event_loop.run_return(move |event, _, control_flow| {
            *control_flow = tao::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            );

            if let Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } = &event
            {
                *outcome.borrow_mut() =
                    Some(Err(anyhow!("Preferences closed without saving")));
                app.stop(None);
                return;
            }

            if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
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
                    Ok(()) => {
                        *outcome.borrow_mut() = Some(Ok(super::PreferencesOutcome { edit }));
                        app.stop(None);
                    }
                    Err(e) => {
                        window
                            .setTitle(&NSString::from_str(&format!("HandsOff Preferences — {}", e)));
                    }
                }
            }
        });

        let result = outcome_slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(anyhow!("Preferences event loop ended unexpectedly")));
        result
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

    pub(super) fn run_change_passphrase_macos(
        event_loop: &mut tao::event_loop::EventLoop<super::WizardEvent>,
    ) -> Result<Config> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow!("change passphrase must run on main thread"))?;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        // Permission gate BEFORE any window exists: the capture tap needs
        // Accessibility granted to HandsOff itself. This path is reachable
        // exactly when TCC revoked the grant (post-update CDHash change);
        // erroring before window creation avoids orphaning a window the
        // caller's error path never closes (setReleasedWhenClosed(false)).
        if !crate::input_blocking::check_accessibility_permissions() {
            return Err(anyhow!(
                "Accessibility permission is required to capture the new passphrase. \
                 Grant it to HandsOff in System Settings > Privacy & Security > Accessibility, \
                 then try again."
            ));
        }

        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, 180.0));
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
        // Close button aborts an in-flight capture (see windowShouldClose:).
        {
            let delegate =
                ProtocolObject::<dyn NSWindowDelegate>::from_retained(target.clone());
            window.setDelegate(Some(&delegate));
        }

        let instr_label: Retained<NSTextField> = unsafe {
            NSTextField::labelWithString(
                &NSString::from_str(
                    "Type your new passphrase (silent — nothing appears). \
                     Press Enter to commit, Backspace to delete, Escape to start over. \
                     You will type it twice to confirm.",
                ),
                mtm,
            )
        };
        unsafe { instr_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 60.0)) };
        let status_label: Retained<NSTextField> =
            unsafe { NSTextField::labelWithString(&NSString::from_str(""), mtm) };
        unsafe { status_label.setFrameSize(NSSize::new(WINDOW_W - 60.0, 20.0)) };

        // Explicit capture start (keyboard-lockout fix): capture runs only
        // after this click, never automatically on window open.
        let capture_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Capture New Passphrase"),
                Some(&*target),
                Some(objc2::sel!(buttonClicked:)),
                mtm,
            )
        };
        unsafe { capture_btn.setTag(TAG_CAPTURE) };

        let content = unsafe {
            NSStackView::initWithFrame(
                mtm.alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_W, 180.0)),
            )
        };
        unsafe {
            content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
            content.setSpacing(12.0);
            const GRAVITY: NSStackViewGravity = NSStackViewGravity::Leading;
            for v in [
                &*instr_label as *const NSTextField as *const NSView,
                &*capture_btn as *const NSButton as *const NSView,
                &*status_label as *const NSTextField as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        window.center();
        window.makeKeyAndOrderFront(None);
        unsafe { app.activateIgnoringOtherApps(true) };

        use tao::event::Event;
        use tao::platform::run_return::EventLoopExtRunReturn;

        let outcome_slot = std::rc::Rc::new(std::cell::RefCell::new(None::<Result<Config>>));
        let outcome = outcome_slot.clone();
        let mut phase = 0u8; // 0 = waiting for capture click, 1 = capturing

        event_loop.run_return(move |event, _, control_flow| {
            *control_flow = tao::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            );

            if let Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } = &event
            {
                *outcome.borrow_mut() =
                    Some(Err(anyhow!("Change Passphrase cancelled — config unchanged")));
                app.stop(None);
                return;
            }

            if phase == 0 && SIGNALS.capture_clicked.swap(false, Ordering::SeqCst) {
                phase = 1;
                status_label.setHidden(false);
                capture_btn.setHidden(true);
                let saved: Result<Config> = (|| {
                    let first = capture_with_status(&status_label)?;
                    unsafe {
                        status_label.setStringValue(&NSString::from_str(
                            "Re-enter the same passphrase to confirm…",
                        ))
                    };
                    let second = capture_with_status(&status_label)?;
                    if first != second {
                        return Err(anyhow!(
                            "Passphrases did not match — nothing was changed. \
                             Try Change Passphrase again."
                        ));
                    }
                    crate::preferences::change_passphrase(&first)
                })();
                *outcome.borrow_mut() = Some(saved);
                app.stop(None);
            }
        });

        let result = outcome_slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Err(anyhow!("Change Passphrase event loop ended unexpectedly")));
        result
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
