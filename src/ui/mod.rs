pub mod menubar;
pub mod notifications;
pub mod dialogs;

use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};

/// Show a simple alert dialog
#[allow(dead_code)]
pub fn show_alert(title: &str, message: &str) {
    unsafe {
        let ns_title = NSString::alloc(nil).init_str(title);
        let ns_message = NSString::alloc(nil).init_str(message);

        let alert: id = msg_send![class!(NSAlert), alloc];
        let alert: id = msg_send![alert, init];

        let _: () = msg_send![alert, setMessageText: ns_title];
        let _: () = msg_send![alert, setInformativeText: ns_message];
        let _: () = msg_send![alert, runModal];
    }
}
