use anyhow::{Context, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};
use log::info;

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    pub lock_hotkey: Option<HotKey>,
    pub talk_hotkey: Option<HotKey>,
}

impl HotkeyManager {
    pub fn new() -> Result<Self> {
        let manager =
            GlobalHotKeyManager::new().context("Failed to create global hotkey manager")?;

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
