use cocoa::appkit::{NSMenu, NSMenuItem, NSStatusBar};
use cocoa::base::{id, nil, selector, NO, YES};
use cocoa::foundation::{NSString, NSAutoreleasePool};
use objc::runtime::Sel;
use objc::{class, msg_send, sel, sel_impl};
use std::sync::Arc;
use parking_lot::Mutex;

use crate::app_state::AppState;
use super::dispatch::dispatch_to_main_thread;

const LOCK_ICON: &str = "🔒";
const UNLOCK_ICON: &str = "🔓";

// Global reference to status item as usize (unsafe, but needed for Objective-C interop)
// We store as usize to make it Send/Sync safe
static STATUS_ITEM: Mutex<Option<usize>> = Mutex::new(None);

/// Update the menu bar icon based on lock state
/// This can be called from any thread - it will dispatch to the main thread
pub fn update_menu_bar_icon(is_locked: bool) {
    let status_item_ptr = STATUS_ITEM.lock().clone();
    if let Some(ptr) = status_item_ptr {
        unsafe {
            dispatch_to_main_thread(move || {
                let _pool = NSAutoreleasePool::new(nil);
                let item = ptr as id;
                let icon = if is_locked {
                    LOCK_ICON
                } else {
                    UNLOCK_ICON
                };
                let ns_icon = NSString::alloc(nil).init_str(icon);
                let _: () = msg_send![item, setTitle: ns_icon];
            });
        }
    }
}

pub struct MenuBar {
    #[allow(dead_code)]
    status_item: id,
    #[allow(dead_code)]
    state: Arc<AppState>,
}

impl MenuBar {
    pub fn new(state: Arc<AppState>) -> Self {
        unsafe {
            let status_bar = NSStatusBar::systemStatusBar(nil);
            let status_item = status_bar.statusItemWithLength_(-1.0);

            // Register status_item globally for updates (store as usize for Send/Sync)
            *STATUS_ITEM.lock() = Some(status_item as usize);

            // Set initial icon
            let icon = NSString::alloc(nil).init_str(UNLOCK_ICON);
            let _: () = msg_send![status_item, setTitle: icon];

            // Create menu
            let menu = NSMenu::new(nil);

            // Enable Lock menu item
            let enable_lock_item = create_menu_item("Enable Lock", selector("enableLock:"));
            menu.addItem_(enable_lock_item);

            // Disable Lock menu item
            let disable_lock_item = create_menu_item("Disable Lock", selector("disableLock:"));
            menu.addItem_(disable_lock_item);

            // Separator
            let separator: id = msg_send![class!(NSMenuItem), separatorItem];
            menu.addItem_(separator);

            // Set Passphrase menu item
            let set_passphrase_item =
                create_menu_item("Set Passphrase...", selector("setPassphrase:"));
            menu.addItem_(set_passphrase_item);

            // Settings menu item
            let settings_item = create_menu_item("Settings...", selector("showSettings:"));
            menu.addItem_(settings_item);

            // Separator
            let separator: id = msg_send![class!(NSMenuItem), separatorItem];
            menu.addItem_(separator);

            // Quit menu item
            let quit_item = create_menu_item("Quit", selector("terminate:"));
            menu.addItem_(quit_item);

            let _: () = msg_send![status_item, setMenu: menu];

            Self { status_item, state }
        }
    }

    /// Update the menu bar icon based on lock state
    #[allow(dead_code)]
    pub fn update_icon(&self) {
        unsafe {
            let icon = if self.state.is_locked() {
                LOCK_ICON
            } else {
                UNLOCK_ICON
            };
            let ns_icon = NSString::alloc(nil).init_str(icon);
            let _: () = msg_send![self.status_item, setTitle: ns_icon];
        }
    }

    /// Enable menu items based on lock state
    #[allow(dead_code)]
    pub fn update_menu_items(&self) {
        unsafe {
            let menu: id = msg_send![self.status_item, menu];
            let items: id = msg_send![menu, itemArray];
            let count: usize = msg_send![items, count];

            for i in 0..count {
                let item: id = msg_send![items, objectAtIndex: i];
                let action: Sel = msg_send![item, action];

                // Disable certain menu items when locked
                if self.state.is_locked() {
                    if action == selector("setPassphrase:")
                        || action == selector("showSettings:")
                        || action == selector("terminate:")
                    {
                        let _: () = msg_send![item, setEnabled: NO];
                    }
                } else {
                    let _: () = msg_send![item, setEnabled: YES];
                }
            }
        }
    }
}

/// Create a menu item with a title and action
unsafe fn create_menu_item(title: &str, action: Sel) -> id {
    let title_str = NSString::alloc(nil).init_str(title);
    let item = NSMenuItem::alloc(nil);
    item.initWithTitle_action_keyEquivalent_(title_str, action, NSString::alloc(nil))
}

/// Register menu item action handlers
#[allow(dead_code)]
pub fn register_menu_handlers() {
    // Note: In a full implementation, you would create an NSObject subclass
    // to handle menu item actions. For now, this is a placeholder.
    // The actual implementation would use objc::declare to create a custom class.
}
