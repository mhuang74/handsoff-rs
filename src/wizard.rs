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
    use objc2::runtime::AnyObject;
    use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton,
        NSControlStateValueOn, NSStackView, NSStackViewGravity, NSTextField,
        NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;

    const WINDOW_W: f64 = 480.0;
    const WINDOW_H: f64 = 300.0;

    // Button action tags (NSInteger carried by the sender's tag property).
    const TAG_GRANT: isize = 1;
    const TAG_COMMIT: isize = 2;
    const TAG_FINISH: isize = 3;

    // Step numbers.
    const STEP_PERMISSION: u8 = 0;
    const STEP_CAPTURE: u8 = 1;
    const STEP_FORM: u8 = 2;

    /// Click signals shared between AppKit button targets and the wizard driver.
    struct WizardSignals {
        grant_clicked: AtomicBool,
        commit_clicked: AtomicBool,
        finish_clicked: AtomicBool,
        step: AtomicU8,
        /// Progress text for the status label (dots), set from the tap callback.
        status: parking_lot::Mutex<String>,
    }

    impl WizardSignals {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                grant_clicked: AtomicBool::new(false),
                commit_clicked: AtomicBool::new(false),
                finish_clicked: AtomicBool::new(false),
                step: AtomicU8::new(STEP_PERMISSION),
                status: parking_lot::Mutex::new(String::new()),
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

        unsafe impl WizardTarget {
            #[method(buttonClicked:)]
            fn button_clicked(&self, sender: &AnyObject) {
                let tag: isize = unsafe { msg_send![sender, tag] };
                match tag {
                    TAG_GRANT => SIGNALS.grant_clicked.store(true, Ordering::SeqCst),
                    TAG_COMMIT => SIGNALS.commit_clicked.store(true, Ordering::SeqCst),
                    TAG_FINISH => SIGNALS.finish_clicked.store(true, Ordering::SeqCst),
                    _ => {}
                }
            }
        }
    );

    /// Open System Settings → Accessibility pane.
    fn open_accessibility_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status();
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
                 not a terminal.\n\nClick the button, then tick the box for HandsOff in System Settings.",
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
        let perm_granted = Arc::new(AtomicBool::new(false));
        std::thread::spawn({
            let perm_granted = perm_granted.clone();
            move || loop {
                if crate::input_blocking::check_accessibility_permissions_lightweight()
                    && crate::input_blocking::check_accessibility_permissions()
                {
                    perm_granted.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });

        let outcome_slot = std::rc::Rc::new(std::cell::RefCell::new(
            None::<Result<WizardOutcome>>,
        ));
        let outcome = outcome_slot.clone();
        let mut first_keys: Option<Vec<u32>> = None;

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

            // Step 0 → 1: Grant clicked.
            if SIGNALS.grant_clicked.swap(false, Ordering::SeqCst) {
                SIGNALS.step.store(STEP_CAPTURE, Ordering::SeqCst);
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

            // Step 1: poll permission; when granted, run the double capture.
            if step == STEP_CAPTURE {
                if perm_granted.load(Ordering::SeqCst) {
                    SIGNALS.step.store(STEP_FORM, Ordering::SeqCst);
                    // UI out of the way for capture; dots go to the status label.
                    instr_label.setHidden(true);
                    set_status("");
                    commit_btn.setHidden(false);

                    // Double capture on the main thread. The headless pump
                    // yields control in 100 ms slices, so the AppKit event
                    // loop keeps servicing while the tap is installed.
                    let first = match capture_with_status(&status_label) {
                        Ok(k) => k,
                        Err(e) => {
                            *outcome.borrow_mut() = Some(Err(anyhow!("Passphrase capture failed: {}", e)));
                            app.stop(None);
                            return;
                        }
                    };
                    set_status("Re-enter the same passphrase to confirm…");

                    let second = match capture_with_status(&status_label) {
                        Ok(k) => k,
                        Err(e) => {
                            *outcome.borrow_mut() = Some(Err(anyhow!("Passphrase capture failed: {}", e)));
                            app.stop(None);
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

    /// Run one headless capture pass on the main thread, mirroring progress
    /// into the status label.
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
                &*status_label as *const NSTextField as *const NSView,
            ] {
                content.addView_inGravity(&*v, GRAVITY);
            }
        }
        unsafe { window.contentView().unwrap().addSubview(&content) };

        window.center();
        window.makeKeyAndOrderFront(None);
        unsafe { app.activateIgnoringOtherApps(true) };

        // Permission gate BEFORE capture: the tap needs Accessibility granted
        // to HandsOff itself. The Preferences path can hit this when TCC
        // invalidated the grant (post-update CDHash change).
        if !crate::input_blocking::check_accessibility_permissions() {
            return Err(anyhow!(
                "Accessibility permission is required to capture the new passphrase. \
                 Grant it to HandsOff in System Settings > Privacy & Security > Accessibility, \
                 then try again."
            ));
        }

        use tao::event::Event;
        use tao::platform::run_return::EventLoopExtRunReturn;

        let outcome_slot = std::rc::Rc::new(std::cell::RefCell::new(None::<Result<Config>>));
        let outcome = outcome_slot.clone();
        let mut phase = 0u8; // 0 = idle, 1 = first capture, 2 = confirm capture

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

            if phase == 0 {
                phase = 1;
                status_label.setHidden(false);
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
