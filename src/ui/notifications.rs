use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};

/// Show a notification when the input is unlocked
pub fn show_unlock_notification() {
    unsafe {
        // Create a user notification
        let notification: id = msg_send![class!(NSUserNotification), alloc];
        let notification: id = msg_send![notification, init];

        let title = NSString::alloc(nil).init_str("Input Unlocked");
        let message = NSString::alloc(nil).init_str("Keyboard and mouse inputs are now active");

        let _: () = msg_send![notification, setTitle: title];
        let _: () = msg_send![notification, setInformativeText: message];

        // Deliver the notification
        let center: id = msg_send![class!(NSUserNotificationCenter), defaultUserNotificationCenter];
        let _: () = msg_send![center, deliverNotification: notification];
    }
}

/// Show a notification when the input is locked
pub fn show_lock_notification() {
    unsafe {
        let notification: id = msg_send![class!(NSUserNotification), alloc];
        let notification: id = msg_send![notification, init];

        let title = NSString::alloc(nil).init_str("Input Locked");
        let message = NSString::alloc(nil).init_str("Keyboard and mouse inputs are now blocked");

        let _: () = msg_send![notification, setTitle: title];
        let _: () = msg_send![notification, setInformativeText: message];

        let center: id = msg_send![class!(NSUserNotificationCenter), defaultUserNotificationCenter];
        let _: () = msg_send![center, deliverNotification: notification];
    }
}

/// Show a full-screen overlay notification (more prominent)
#[allow(dead_code)]
pub fn show_unlock_overlay() {
    // This would create a brief full-screen or large overlay window
    // showing that input is unlocked. For now, we'll use a regular notification.
    show_unlock_notification();
}
