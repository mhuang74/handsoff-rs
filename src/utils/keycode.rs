/// Convert macOS keycode to character
/// Based on HIToolbox/Events.h keycodes for US keyboard layout
pub fn keycode_to_char(keycode: i64, shift: bool) -> Option<char> {
    match keycode {
        // Letters
        0 => Some(if shift { 'A' } else { 'a' }),
        1 => Some(if shift { 'S' } else { 's' }),
        2 => Some(if shift { 'D' } else { 'd' }),
        3 => Some(if shift { 'F' } else { 'f' }),
        4 => Some(if shift { 'H' } else { 'h' }),
        5 => Some(if shift { 'G' } else { 'g' }),
        6 => Some(if shift { 'Z' } else { 'z' }),
        7 => Some(if shift { 'X' } else { 'x' }),
        8 => Some(if shift { 'C' } else { 'c' }),
        9 => Some(if shift { 'V' } else { 'v' }),
        11 => Some(if shift { 'B' } else { 'b' }),
        12 => Some(if shift { 'Q' } else { 'q' }),
        13 => Some(if shift { 'W' } else { 'w' }),
        14 => Some(if shift { 'E' } else { 'e' }),
        15 => Some(if shift { 'R' } else { 'r' }),
        16 => Some(if shift { 'Y' } else { 'y' }),
        17 => Some(if shift { 'T' } else { 't' }),
        31 => Some(if shift { 'O' } else { 'o' }),
        32 => Some(if shift { 'U' } else { 'u' }),
        34 => Some(if shift { 'I' } else { 'i' }),
        35 => Some(if shift { 'P' } else { 'p' }),
        37 => Some(if shift { 'L' } else { 'l' }),
        38 => Some(if shift { 'J' } else { 'j' }),
        40 => Some(if shift { 'K' } else { 'k' }),
        45 => Some(if shift { 'N' } else { 'n' }),
        46 => Some(if shift { 'M' } else { 'm' }),

        // Numbers
        18 => Some(if shift { '!' } else { '1' }),
        19 => Some(if shift { '@' } else { '2' }),
        20 => Some(if shift { '#' } else { '3' }),
        21 => Some(if shift { '$' } else { '4' }),
        23 => Some(if shift { '%' } else { '5' }),
        22 => Some(if shift { '^' } else { '6' }),
        26 => Some(if shift { '&' } else { '7' }),
        28 => Some(if shift { '*' } else { '8' }),
        25 => Some(if shift { '(' } else { '9' }),
        29 => Some(if shift { ')' } else { '0' }),

        // Symbols
        27 => Some(if shift { '_' } else { '-' }),
        24 => Some(if shift { '+' } else { '=' }),
        33 => Some(if shift { '{' } else { '[' }),
        30 => Some(if shift { '}' } else { ']' }),
        41 => Some(if shift { ':' } else { ';' }),
        39 => Some(if shift { '"' } else { '\'' }),
        42 => Some(if shift { '|' } else { '\\' }),
        43 => Some(if shift { '<' } else { ',' }),
        47 => Some(if shift { '>' } else { '.' }),
        44 => Some(if shift { '?' } else { '/' }),
        50 => Some(if shift { '~' } else { '`' }),

        // Space
        49 => Some(' '),

        // Return/Enter
        36 | 76 => Some('\n'),

        // Tab
        48 => Some('\t'),

        // Delete (treat as backspace for passphrase entry)
        51 => None, // Handle separately in event handler

        _ => None,
    }
}
