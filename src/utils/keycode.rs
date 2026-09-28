//! Hotkey `Code` → macOS virtual keycode mapping.
//!
//! Used to translate the configured hotkey last key (A-Z) into the keycode
//! the CGEventTap/`global_hotkey` runtime registers. The former
//! character-decode map (`keycode_to_char`) was removed: setup no longer
//! displays captured passphrases (double-capture silent confirm), and the
//! unlock path always compared raw keycodes (spec §3).

/// Convert global_hotkey Code enum to macOS keycode
/// Returns None if the Code is not a letter key
pub fn code_to_keycode(code: global_hotkey::hotkey::Code) -> Option<i64> {
    use global_hotkey::hotkey::Code;
    match code {
        Code::KeyA => Some(0),
        Code::KeyB => Some(11),
        Code::KeyC => Some(8),
        Code::KeyD => Some(2),
        Code::KeyE => Some(14),
        Code::KeyF => Some(3),
        Code::KeyG => Some(5),
        Code::KeyH => Some(4),
        Code::KeyI => Some(34),
        Code::KeyJ => Some(38),
        Code::KeyK => Some(40),
        Code::KeyL => Some(37),
        Code::KeyM => Some(46),
        Code::KeyN => Some(45),
        Code::KeyO => Some(31),
        Code::KeyP => Some(35),
        Code::KeyQ => Some(12),
        Code::KeyR => Some(15),
        Code::KeyS => Some(1),
        Code::KeyT => Some(17),
        Code::KeyU => Some(32),
        Code::KeyV => Some(9),
        Code::KeyW => Some(13),
        Code::KeyX => Some(7),
        Code::KeyY => Some(16),
        Code::KeyZ => Some(6),
        _ => None, // Not a letter key
    }
}
