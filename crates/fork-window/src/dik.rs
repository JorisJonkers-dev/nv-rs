//! Physical keys as DirectInput key codes (the scan code, 0x80 added for extended keys), the keys
//! the Windows window layer reports (`d3d9real::dik`). Keys of the same place give the same code
//! on every layout, as a scan code does.

/// `(name, code)` for every key the table knows; `name` is winit's `KeyCode` variant.
pub const KEYS: &[(&str, u8)] = &[
    ("Escape", 0x01),
    ("Digit1", 0x02),
    ("Digit2", 0x03),
    ("Digit3", 0x04),
    ("Digit4", 0x05),
    ("Digit5", 0x06),
    ("Digit6", 0x07),
    ("Digit7", 0x08),
    ("Digit8", 0x09),
    ("Digit9", 0x0a),
    ("Digit0", 0x0b),
    ("Minus", 0x0c),
    ("Equal", 0x0d),
    ("Backspace", 0x0e),
    ("Tab", 0x0f),
    ("KeyQ", 0x10),
    ("KeyW", 0x11),
    ("KeyE", 0x12),
    ("KeyR", 0x13),
    ("KeyT", 0x14),
    ("KeyY", 0x15),
    ("KeyU", 0x16),
    ("KeyI", 0x17),
    ("KeyO", 0x18),
    ("KeyP", 0x19),
    ("BracketLeft", 0x1a),
    ("BracketRight", 0x1b),
    ("Enter", 0x1c),
    ("ControlLeft", 0x1d),
    ("KeyA", 0x1e),
    ("KeyS", 0x1f),
    ("KeyD", 0x20),
    ("KeyF", 0x21),
    ("KeyG", 0x22),
    ("KeyH", 0x23),
    ("KeyJ", 0x24),
    ("KeyK", 0x25),
    ("KeyL", 0x26),
    ("Semicolon", 0x27),
    ("Quote", 0x28),
    ("Backquote", 0x29),
    ("ShiftLeft", 0x2a),
    ("Backslash", 0x2b),
    ("KeyZ", 0x2c),
    ("KeyX", 0x2d),
    ("KeyC", 0x2e),
    ("KeyV", 0x2f),
    ("KeyB", 0x30),
    ("KeyN", 0x31),
    ("KeyM", 0x32),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
    ("ShiftRight", 0x36),
    ("NumpadMultiply", 0x37),
    ("AltLeft", 0x38),
    ("Space", 0x39),
    ("CapsLock", 0x3a),
    ("F1", 0x3b),
    ("F2", 0x3c),
    ("F3", 0x3d),
    ("F4", 0x3e),
    ("F5", 0x3f),
    ("F6", 0x40),
    ("F7", 0x41),
    ("F8", 0x42),
    ("F9", 0x43),
    ("F10", 0x44),
    ("NumLock", 0x45),
    ("ScrollLock", 0x46),
    ("Numpad7", 0x47),
    ("Numpad8", 0x48),
    ("Numpad9", 0x49),
    ("NumpadSubtract", 0x4a),
    ("Numpad4", 0x4b),
    ("Numpad5", 0x4c),
    ("Numpad6", 0x4d),
    ("NumpadAdd", 0x4e),
    ("Numpad1", 0x4f),
    ("Numpad2", 0x50),
    ("Numpad3", 0x51),
    ("Numpad0", 0x52),
    ("NumpadDecimal", 0x53),
    ("IntlBackslash", 0x56),
    ("F11", 0x57),
    ("F12", 0x58),
    ("NumpadEnter", 0x9c),
    ("ControlRight", 0x9d),
    ("NumpadDivide", 0xb5),
    ("PrintScreen", 0xb7),
    ("AltRight", 0xb8),
    ("Pause", 0xc5),
    ("Home", 0xc7),
    ("ArrowUp", 0xc8),
    ("PageUp", 0xc9),
    ("ArrowLeft", 0xcb),
    ("ArrowRight", 0xcd),
    ("End", 0xcf),
    ("ArrowDown", 0xd0),
    ("PageDown", 0xd1),
    ("Insert", 0xd2),
    ("Delete", 0xd3),
    ("SuperLeft", 0xdb),
    ("SuperRight", 0xdc),
    ("ContextMenu", 0xdd),
];

/// The code of the key named `name` (a winit `KeyCode` variant).
pub fn by_name(name: &str) -> Option<u8> {
    KEYS.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

/// DirectInput's mouse buttons in its order: left, right, middle, X1 (back), X2 (forward).
pub const BUTTONS: [&str; 5] = ["Left", "Right", "Middle", "Back", "Forward"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_code_or_name_twice() {
        let codes: HashSet<u8> = KEYS.iter().map(|k| k.1).collect();
        let names: HashSet<&str> = KEYS.iter().map(|k| k.0).collect();
        assert_eq!(codes.len(), KEYS.len());
        assert_eq!(names.len(), KEYS.len());
    }

    #[test]
    fn the_games_keys_have_their_codes() {
        // The WASD of movement, Escape, Tab (the Pip-Boy), the arrows, the extended Control.
        assert_eq!(by_name("KeyW"), Some(0x11));
        assert_eq!(by_name("KeyA"), Some(0x1e));
        assert_eq!(by_name("KeyS"), Some(0x1f));
        assert_eq!(by_name("KeyD"), Some(0x20));
        assert_eq!(by_name("Escape"), Some(0x01));
        assert_eq!(by_name("Tab"), Some(0x0f));
        assert_eq!(by_name("ArrowUp"), Some(0xc8));
        assert_eq!(by_name("ControlRight"), Some(0x9d));
        assert_eq!(by_name("Nothing"), None);
        // Extended keys are the scan code with 0x80 added.
        assert!(KEYS.iter().filter(|k| k.1 >= 0x80).all(|k| k.1 & 0x7f != 0));
    }
}
