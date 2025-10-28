use cocoa::base::{id, nil};
use cocoa::foundation::{NSString, NSAutoreleasePool};
use objc::{class, msg_send, sel, sel_impl};
use super::dispatch::dispatch_to_main_thread;

/// Show a notification when the input is unlocked
pub fn show_unlock_notification() {
    unsafe {
        dispatch_to_main_thread(|| {
            let _pool = NSAutoreleasePool::new(nil);

            // Get the notification center first to ensure it's initialized
            let center_class = class!(NSUserNotificationCenter);
            let center: id = msg_send![center_class, defaultUserNotificationCenter];

            // Check if notification center is available
            if center == nil {
                return;
            }

            // Create a user notification
            let notification: id = msg_send![class!(NSUserNotification), new];

            let title = NSString::alloc(nil).init_str("Input Unlocked");
            let message = NSString::alloc(nil).init_str("Keyboard and mouse inputs are now active");

            let _: () = msg_send![notification, setTitle: title];
            let _: () = msg_send![notification, setInformativeText: message];

            // Deliver the notification
            let _: () = msg_send![center, deliverNotification: notification];
        });
    }
}

/// Show a notification when the input is locked
pub fn show_lock_notification() {
    unsafe {
        dispatch_to_main_thread(|| {
            let _pool = NSAutoreleasePool::new(nil);

            // Get the notification center first to ensure it's initialized
            let center_class = class!(NSUserNotificationCenter);
            let center: id = msg_send![center_class, defaultUserNotificationCenter];

            // Check if notification center is available
            if center == nil {
                return;
            }

            let notification: id = msg_send![class!(NSUserNotification), new];

            let title = NSString::alloc(nil).init_str("Input Locked");
            let message = NSString::alloc(nil).init_str("Keyboard and mouse inputs are now blocked");

            let _: () = msg_send![notification, setTitle: title];
            let _: () = msg_send![notification, setInformativeText: message];

            let _: () = msg_send![center, deliverNotification: notification];
        });
    }
}

/// Show a full-screen overlay notification (more prominent)
#[allow(dead_code)]
pub fn show_unlock_overlay() {
    // This would create a brief full-screen or large overlay window
    // showing that input is unlocked. For now, we'll use a regular notification.
    show_unlock_notification();
}

/// Show a notification when auto-unlock safety feature triggers
pub fn show_auto_unlock_notification() {
    unsafe {
        dispatch_to_main_thread(|| {
            let _pool = NSAutoreleasePool::new(nil);

            // Get the notification center first to ensure it's initialized
            let center_class = class!(NSUserNotificationCenter);
            let center: id = msg_send![center_class, defaultUserNotificationCenter];

            // Check if notification center is available
            if center == nil {
                log::error!("Failed to get notification center for auto-unlock");
                return;
            }

            let notification: id = msg_send![class!(NSUserNotification), new];

            let title = NSString::alloc(nil).init_str("HandsOff Auto-Unlock Activated");
            let message = NSString::alloc(nil).init_str(
                "Input interception disabled by safety timeout. You can use your computer normally."
            );

            let _: () = msg_send![notification, setTitle: title];
            let _: () = msg_send![notification, setInformativeText: message];

            // Set sound to default notification sound
            let sound_name = NSString::alloc(nil).init_str("_NSUserNotificationDefaultSoundName");
            let _: () = msg_send![notification, setSoundName: sound_name];

            // Deliver the notification
            let _: () = msg_send![center, deliverNotification: notification];

            log::info!("Auto-unlock notification delivered");
        });
    }
}
