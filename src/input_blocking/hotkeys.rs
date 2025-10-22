use crate::app_state::AppState;
use anyhow::{Context, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use log::info;

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    lock_hotkey: Option<HotKey>,
    talk_hotkey: Option<HotKey>,
}

impl HotkeyManager {
    pub fn new() -> Result<Self> {
        let manager = GlobalHotKeyManager::new()
            .context("Failed to create global hotkey manager")?;

        Ok(Self {
            manager,
            lock_hotkey: None,
            talk_hotkey: None,
        })
    }

    /// Register the lock hotkey (default: Ctrl+Cmd+Shift+L)
    pub fn register_lock_hotkey(&mut self) -> Result<()> {
        let hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyL,
        );

        self.manager
            .register(hotkey)
            .context("Failed to register lock hotkey")?;

        self.lock_hotkey = Some(hotkey);
        info!("Lock hotkey registered: Ctrl+Cmd+Shift+L");
        Ok(())
    }

    /// Register the talk hotkey (default: Ctrl+Cmd+Shift+T)
    pub fn register_talk_hotkey(&mut self) -> Result<()> {
        let hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SUPER | Modifiers::SHIFT),
            Code::KeyT,
        );

        self.manager
            .register(hotkey)
            .context("Failed to register talk hotkey")?;

        self.talk_hotkey = Some(hotkey);
        info!("Talk hotkey registered: Ctrl+Cmd+Shift+T");
        Ok(())
    }

    /// Check if a hotkey event is the lock hotkey
    pub fn is_lock_hotkey(&self, event_id: u32) -> bool {
        self.lock_hotkey.is_some_and(|hk| hk.id() == event_id)
    }

    /// Check if a hotkey event is the talk hotkey
    pub fn is_talk_hotkey(&self, event_id: u32) -> bool {
        self.talk_hotkey.is_some_and(|hk| hk.id() == event_id)
    }

    /// Unregister all hotkeys
    #[allow(dead_code)]
    pub fn unregister_all(&mut self) -> Result<()> {
        if let Some(hotkey) = self.lock_hotkey.take() {
            self.manager.unregister(hotkey)?;
        }
        if let Some(hotkey) = self.talk_hotkey.take() {
            self.manager.unregister(hotkey)?;
        }
        Ok(())
    }
}

/// Handle hotkey events
pub fn handle_hotkey_event(
    event: GlobalHotKeyEvent,
    state: &AppState,
    manager: &HotkeyManager,
) {
    let event_id = event.id;

    if manager.is_lock_hotkey(event_id) {
        info!("Lock hotkey triggered");
        if !state.is_locked() {
            state.set_locked(true);
            info!("Input locked via hotkey");
            crate::ui::menubar::update_menu_bar_icon(true);
        }
    } else if manager.is_talk_hotkey(event_id) {
        info!("Talk hotkey triggered");
        // Note: Spacebar passthrough is handled in the event tap (event_tap.rs)
        // which detects the key combination and tracks press/release states
    }
}
