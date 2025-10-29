use handsoff::utils::keycode::keycode_to_char;

#[test]
fn test_letter_keys_no_shift() {
    assert_eq!(keycode_to_char(0, false), Some('a'));
    assert_eq!(keycode_to_char(1, false), Some('s'));
    assert_eq!(keycode_to_char(2, false), Some('d'));
    assert_eq!(keycode_to_char(3, false), Some('f'));
    assert_eq!(keycode_to_char(4, false), Some('h'));
    assert_eq!(keycode_to_char(5, false), Some('g'));
    assert_eq!(keycode_to_char(6, false), Some('z'));
    assert_eq!(keycode_to_char(7, false), Some('x'));
    assert_eq!(keycode_to_char(8, false), Some('c'));
    assert_eq!(keycode_to_char(9, false), Some('v'));
}

#[test]
fn test_letter_keys_with_shift() {
    assert_eq!(keycode_to_char(0, true), Some('A'));
    assert_eq!(keycode_to_char(1, true), Some('S'));
    assert_eq!(keycode_to_char(2, true), Some('D'));
    assert_eq!(keycode_to_char(3, true), Some('F'));
    assert_eq!(keycode_to_char(4, true), Some('H'));
    assert_eq!(keycode_to_char(5, true), Some('G'));
    assert_eq!(keycode_to_char(6, true), Some('Z'));
    assert_eq!(keycode_to_char(7, true), Some('X'));
    assert_eq!(keycode_to_char(8, true), Some('C'));
    assert_eq!(keycode_to_char(9, true), Some('V'));
}

#[test]
fn test_number_keys_no_shift() {
    assert_eq!(keycode_to_char(18, false), Some('1'));
    assert_eq!(keycode_to_char(19, false), Some('2'));
    assert_eq!(keycode_to_char(20, false), Some('3'));
    assert_eq!(keycode_to_char(21, false), Some('4'));
    assert_eq!(keycode_to_char(23, false), Some('5'));
    assert_eq!(keycode_to_char(22, false), Some('6'));
    assert_eq!(keycode_to_char(26, false), Some('7'));
    assert_eq!(keycode_to_char(28, false), Some('8'));
    assert_eq!(keycode_to_char(25, false), Some('9'));
    assert_eq!(keycode_to_char(29, false), Some('0'));
}

#[test]
fn test_number_keys_with_shift() {
    assert_eq!(keycode_to_char(18, true), Some('!'));
    assert_eq!(keycode_to_char(19, true), Some('@'));
    assert_eq!(keycode_to_char(20, true), Some('#'));
    assert_eq!(keycode_to_char(21, true), Some('$'));
    assert_eq!(keycode_to_char(23, true), Some('%'));
    assert_eq!(keycode_to_char(22, true), Some('^'));
    assert_eq!(keycode_to_char(26, true), Some('&'));
    assert_eq!(keycode_to_char(28, true), Some('*'));
    assert_eq!(keycode_to_char(25, true), Some('('));
    assert_eq!(keycode_to_char(29, true), Some(')'));
}

#[test]
fn test_special_keys() {
    assert_eq!(keycode_to_char(49, false), Some(' ')); // Space
    assert_eq!(keycode_to_char(36, false), Some('\n')); // Return
    assert_eq!(keycode_to_char(48, false), Some('\t')); // Tab
}

#[test]
fn test_punctuation_no_shift() {
    assert_eq!(keycode_to_char(27, false), Some('-')); // Minus
    assert_eq!(keycode_to_char(24, false), Some('=')); // Equal
    assert_eq!(keycode_to_char(33, false), Some('[')); // Left bracket
    assert_eq!(keycode_to_char(30, false), Some(']')); // Right bracket
    assert_eq!(keycode_to_char(41, false), Some(';')); // Semicolon
    assert_eq!(keycode_to_char(39, false), Some('\'')); // Quote
    assert_eq!(keycode_to_char(43, false), Some(',')); // Comma
    assert_eq!(keycode_to_char(47, false), Some('.')); // Period
    assert_eq!(keycode_to_char(44, false), Some('/')); // Slash
    assert_eq!(keycode_to_char(42, false), Some('\\')); // Backslash
    assert_eq!(keycode_to_char(50, false), Some('`')); // Grave
}

#[test]
fn test_punctuation_with_shift() {
    assert_eq!(keycode_to_char(27, true), Some('_')); // Underscore
    assert_eq!(keycode_to_char(24, true), Some('+')); // Plus
    assert_eq!(keycode_to_char(33, true), Some('{')); // Left brace
    assert_eq!(keycode_to_char(30, true), Some('}')); // Right brace
    assert_eq!(keycode_to_char(41, true), Some(':')); // Colon
    assert_eq!(keycode_to_char(39, true), Some('"')); // Double quote
    assert_eq!(keycode_to_char(43, true), Some('<')); // Less than
    assert_eq!(keycode_to_char(47, true), Some('>')); // Greater than
    assert_eq!(keycode_to_char(44, true), Some('?')); // Question mark
    assert_eq!(keycode_to_char(42, true), Some('|')); // Pipe
    assert_eq!(keycode_to_char(50, true), Some('~')); // Tilde
}

#[test]
fn test_special_keys_return_none() {
    assert_eq!(keycode_to_char(51, false), None); // Delete
    assert_eq!(keycode_to_char(53, false), None); // Escape
    assert_eq!(keycode_to_char(123, false), None); // Left arrow
    assert_eq!(keycode_to_char(124, false), None); // Right arrow
    assert_eq!(keycode_to_char(125, false), None); // Down arrow
    assert_eq!(keycode_to_char(126, false), None); // Up arrow
}

#[test]
fn test_invalid_keycode() {
    assert_eq!(keycode_to_char(999, false), None);
    assert_eq!(keycode_to_char(-1, false), None);
    assert_eq!(keycode_to_char(200, false), None);
}

#[test]
fn test_all_letters_complete() {
    // Verify complete alphabet mapping
    let expected = "abcdefghijklmnopqrstuvwxyz";
    let keycodes = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];

    for (i, keycode) in keycodes.iter().enumerate() {
        let expected_char = expected.chars().nth(i).unwrap();
        assert_eq!(
            keycode_to_char(*keycode, false),
            Some(expected_char),
            "Keycode {} should map to '{}'",
            keycode,
            expected_char
        );
    }
}

#[test]
fn test_all_letters_uppercase() {
    let expected = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let keycodes = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];

    for (i, keycode) in keycodes.iter().enumerate() {
        let expected_char = expected.chars().nth(i).unwrap();
        assert_eq!(
            keycode_to_char(*keycode, true),
            Some(expected_char),
            "Keycode {} with shift should map to '{}'",
            keycode,
            expected_char
        );
    }
}
