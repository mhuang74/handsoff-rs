use cocoa::base::{id, nil};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};

// NSAlertStyle constants
const NSALERT_STYLE_INFORMATIONAL: usize = 1;
const NSALERT_STYLE_WARNING: usize = 0;

/// Show a dialog to set the passphrase
pub fn show_set_passphrase_dialog() -> Option<String> {
    unsafe {
        // Create alert
        let alert: id = msg_send![class!(NSAlert), alloc];
        let alert: id = msg_send![alert, init];

        let title = NSString::alloc(nil).init_str("Set Passphrase");
        let message = NSString::alloc(nil).init_str("Enter a passphrase to unlock the input:");

        let _: () = msg_send![alert, setMessageText: title];
        let _: () = msg_send![alert, setInformativeText: message];
        let _: () = msg_send![alert, setAlertStyle: NSALERT_STYLE_INFORMATIONAL];

        // Add buttons
        let ok_button = NSString::alloc(nil).init_str("OK");
        let cancel_button = NSString::alloc(nil).init_str("Cancel");
        let _: () = msg_send![alert, addButtonWithTitle: ok_button];
        let _: () = msg_send![alert, addButtonWithTitle: cancel_button];

        // Create secure text field
        let text_field: id = msg_send![class!(NSSecureTextField), alloc];
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 24.0));
        let text_field: id = msg_send![text_field, initWithFrame: frame];

        let _: () = msg_send![alert, setAccessoryView: text_field];

        // Show modal and get response
        let response: isize = msg_send![alert, runModal];

        // NSAlertFirstButtonReturn = 1000
        if response == 1000 {
            let string_value: id = msg_send![text_field, stringValue];
            let cstr: *const i8 = msg_send![string_value, UTF8String];
            if !cstr.is_null() {
                let rust_string = std::ffi::CStr::from_ptr(cstr)
                    .to_string_lossy()
                    .into_owned();
                if !rust_string.is_empty() {
                    return Some(rust_string);
                }
            }
        }

        None
    }
}

/// Show settings dialog
pub fn show_settings_dialog() {
    unsafe {
        let alert: id = msg_send![class!(NSAlert), alloc];
        let alert: id = msg_send![alert, init];

        let title = NSString::alloc(nil).init_str("Settings");
        let message = NSString::alloc(nil).init_str(
            "Settings:\n\
            \n\
            Lock Hotkey: Ctrl+Cmd+Shift+L\n\
            Talk Hotkey: Ctrl+Cmd+Shift+T\n\
            Touch ID Trigger: Ctrl+Cmd+Shift+U\n\
            Auto-lock Timeout: 3 minutes\n\
            \n\
            (Customization coming soon)"
        );

        let _: () = msg_send![alert, setMessageText: title];
        let _: () = msg_send![alert, setInformativeText: message];
        let _: () = msg_send![alert, runModal];
    }
}

/// Show permissions request dialog
pub fn show_permissions_dialog() {
    unsafe {
        let alert: id = msg_send![class!(NSAlert), alloc];
        let alert: id = msg_send![alert, init];

        let title = NSString::alloc(nil).init_str("Accessibility Permissions Required");
        let message = NSString::alloc(nil).init_str(
            "HandsOff requires Accessibility permissions to block input.\n\
            \n\
            Please grant permissions in:\n\
            System Settings > Privacy & Security > Accessibility\n\
            \n\
            After granting permissions, please restart the app."
        );

        let _: () = msg_send![alert, setMessageText: title];
        let _: () = msg_send![alert, setInformativeText: message];
        let _: () = msg_send![alert, setAlertStyle: NSALERT_STYLE_WARNING];
        let _: () = msg_send![alert, runModal];
    }
}
