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

/// Inverse of `code_to_keycode`: macOS virtual keycode → uppercase letter.
///
/// Display-only (issue #36): the Change Passphrase dialog names a rejected
/// hotkey key in its status line ("L is reserved — it is part of the Lock
/// hotkey"). Returns `None` for non-letter keycodes; callers fall back to
/// the raw keycode number. Never used to gate Passphrase membership.
pub fn keycode_to_letter(keycode: i64) -> Option<char> {
    match keycode {
        0 => Some('A'),
        11 => Some('B'),
        8 => Some('C'),
        2 => Some('D'),
        14 => Some('E'),
        3 => Some('F'),
        5 => Some('G'),
        4 => Some('H'),
        34 => Some('I'),
        38 => Some('J'),
        40 => Some('K'),
        37 => Some('L'),
        46 => Some('M'),
        45 => Some('N'),
        31 => Some('O'),
        35 => Some('P'),
        12 => Some('Q'),
        15 => Some('R'),
        1 => Some('S'),
        17 => Some('T'),
        32 => Some('U'),
        9 => Some('V'),
        13 => Some('W'),
        7 => Some('X'),
        16 => Some('Y'),
        6 => Some('Z'),
        _ => None,
    }
}
