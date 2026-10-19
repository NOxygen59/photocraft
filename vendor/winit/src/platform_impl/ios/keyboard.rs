//! Hardware keyboards (iPad Magic Keyboard, Smart Keyboard, Bluetooth and USB keyboards) on
//! iPadOS 13.4+: `UIPress.key` gives the USB HID usage of the key and the characters it types.

use objc2_ui_kit::{UIKey, UIKeyModifierFlags};
use smol_str::SmolStr;

use crate::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKey, NativeKeyCode, PhysicalKey,
};

/// The physical key of a USB HID keyboard usage (page 7), with its named logical key, if any.
pub(crate) fn hid_key(usage: isize) -> (PhysicalKey, Option<NamedKey>, KeyLocation) {
    use KeyCode as C;
    use KeyLocation::{Left, Numpad, Right, Standard};
    use NamedKey as N;
    let code = |c: KeyCode| (PhysicalKey::Code(c), None, Standard);
    let named = |c: KeyCode, n: NamedKey| (PhysicalKey::Code(c), Some(n), Standard);
    let letters = [
        C::KeyA, C::KeyB, C::KeyC, C::KeyD, C::KeyE, C::KeyF, C::KeyG, C::KeyH, C::KeyI, C::KeyJ,
        C::KeyK, C::KeyL, C::KeyM, C::KeyN, C::KeyO, C::KeyP, C::KeyQ, C::KeyR, C::KeyS, C::KeyT,
        C::KeyU, C::KeyV, C::KeyW, C::KeyX, C::KeyY, C::KeyZ,
    ];
    let digits = [
        C::Digit1, C::Digit2, C::Digit3, C::Digit4, C::Digit5, C::Digit6, C::Digit7, C::Digit8,
        C::Digit9, C::Digit0,
    ];
    let functions = [
        (C::F1, N::F1), (C::F2, N::F2), (C::F3, N::F3), (C::F4, N::F4), (C::F5, N::F5),
        (C::F6, N::F6), (C::F7, N::F7), (C::F8, N::F8), (C::F9, N::F9), (C::F10, N::F10),
        (C::F11, N::F11), (C::F12, N::F12),
    ];
    let functions_high = [
        (C::F13, N::F13), (C::F14, N::F14), (C::F15, N::F15), (C::F16, N::F16), (C::F17, N::F17),
        (C::F18, N::F18), (C::F19, N::F19), (C::F20, N::F20), (C::F21, N::F21), (C::F22, N::F22),
        (C::F23, N::F23), (C::F24, N::F24),
    ];
    let numpad = [
        C::Numpad1, C::Numpad2, C::Numpad3, C::Numpad4, C::Numpad5, C::Numpad6, C::Numpad7,
        C::Numpad8, C::Numpad9, C::Numpad0,
    ];
    match usage {
        0x04..=0x1D => code(letters[(usage - 0x04) as usize]),
        0x1E..=0x27 => code(digits[(usage - 0x1E) as usize]),
        0x28 => named(C::Enter, N::Enter),
        0x29 => named(C::Escape, N::Escape),
        0x2A => named(C::Backspace, N::Backspace),
        0x2B => named(C::Tab, N::Tab),
        0x2C => named(C::Space, N::Space),
        0x2D => code(C::Minus),
        0x2E => code(C::Equal),
        0x2F => code(C::BracketLeft),
        0x30 => code(C::BracketRight),
        0x31 | 0x32 => code(C::Backslash),
        0x33 => code(C::Semicolon),
        0x34 => code(C::Quote),
        0x35 => code(C::Backquote),
        0x36 => code(C::Comma),
        0x37 => code(C::Period),
        0x38 => code(C::Slash),
        0x39 => named(C::CapsLock, N::CapsLock),
        0x3A..=0x45 => {
            let (c, n) = functions[(usage - 0x3A) as usize];
            named(c, n)
        },
        0x46 => named(C::PrintScreen, N::PrintScreen),
        0x47 => named(C::ScrollLock, N::ScrollLock),
        0x48 => named(C::Pause, N::Pause),
        0x49 => named(C::Insert, N::Insert),
        0x4A => named(C::Home, N::Home),
        0x4B => named(C::PageUp, N::PageUp),
        0x4C => named(C::Delete, N::Delete),
        0x4D => named(C::End, N::End),
        0x4E => named(C::PageDown, N::PageDown),
        0x4F => named(C::ArrowRight, N::ArrowRight),
        0x50 => named(C::ArrowLeft, N::ArrowLeft),
        0x51 => named(C::ArrowDown, N::ArrowDown),
        0x52 => named(C::ArrowUp, N::ArrowUp),
        0x53 => (PhysicalKey::Code(C::NumLock), Some(N::NumLock), Numpad),
        0x54 => (PhysicalKey::Code(C::NumpadDivide), None, Numpad),
        0x55 => (PhysicalKey::Code(C::NumpadMultiply), None, Numpad),
        0x56 => (PhysicalKey::Code(C::NumpadSubtract), None, Numpad),
        0x57 => (PhysicalKey::Code(C::NumpadAdd), None, Numpad),
        0x58 => (PhysicalKey::Code(C::NumpadEnter), Some(N::Enter), Numpad),
        0x59..=0x62 => (PhysicalKey::Code(numpad[(usage - 0x59) as usize]), None, Numpad),
        0x63 => (PhysicalKey::Code(C::NumpadDecimal), None, Numpad),
        0x64 => code(C::IntlBackslash),
        0x65 => named(C::ContextMenu, N::ContextMenu),
        0x67 => (PhysicalKey::Code(C::NumpadEqual), None, Numpad),
        0x68..=0x73 => {
            let (c, n) = functions_high[(usage - 0x68) as usize];
            named(c, n)
        },
        0x87 => code(C::IntlRo),
        0x89 => code(C::IntlYen),
        0x90 => code(C::Lang1),
        0x91 => code(C::Lang2),
        0xE0 => (PhysicalKey::Code(C::ControlLeft), Some(N::Control), Left),
        0xE1 => (PhysicalKey::Code(C::ShiftLeft), Some(N::Shift), Left),
        0xE2 => (PhysicalKey::Code(C::AltLeft), Some(N::Alt), Left),
        0xE3 => (PhysicalKey::Code(C::SuperLeft), Some(N::Super), Left),
        0xE4 => (PhysicalKey::Code(C::ControlRight), Some(N::Control), Right),
        0xE5 => (PhysicalKey::Code(C::ShiftRight), Some(N::Shift), Right),
        0xE6 => (PhysicalKey::Code(C::AltRight), Some(N::Alt), Right),
        0xE7 => (PhysicalKey::Code(C::SuperRight), Some(N::Super), Right),
        _ => (PhysicalKey::Unidentified(NativeKeyCode::Unidentified), None, Standard),
    }
}

/// The modifier a modifier key's HID usage holds down.
pub(crate) fn hid_modifier(usage: isize) -> Option<ModifiersState> {
    match usage {
        0xE0 | 0xE4 => Some(ModifiersState::CONTROL),
        0xE1 | 0xE5 => Some(ModifiersState::SHIFT),
        0xE2 | 0xE6 => Some(ModifiersState::ALT),
        0xE3 | 0xE7 => Some(ModifiersState::SUPER),
        _ => None,
    }
}

pub(crate) fn modifiers_from_flags(flags: UIKeyModifierFlags) -> ModifiersState {
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::SHIFT, flags.contains(UIKeyModifierFlags::UIKeyModifierShift));
    state.set(ModifiersState::CONTROL, flags.contains(UIKeyModifierFlags::UIKeyModifierControl));
    state.set(ModifiersState::ALT, flags.contains(UIKeyModifierFlags::UIKeyModifierAlternate));
    state.set(ModifiersState::SUPER, flags.contains(UIKeyModifierFlags::UIKeyModifierCommand));
    state
}

/// A key's press, as winit describes it.
pub(crate) struct PressedKey {
    pub usage: isize,
    pub physical_key: PhysicalKey,
    pub logical_key: Key,
    pub location: KeyLocation,
    /// What the key types (none with Command or Control held, and for keys that type nothing).
    pub text: Option<SmolStr>,
    /// The key types a character (or is Backspace) and no Command or Control is held: in a text
    /// field, iPadOS's own text input should take it (dead keys, input methods).
    pub types_text: bool,
}

fn printable(s: &str) -> Option<SmolStr> {
    (!s.is_empty() && !s.chars().any(char::is_control)).then(|| SmolStr::new(s))
}

pub(crate) fn pressed_key(key: &UIKey) -> PressedKey {
    let usage = unsafe { key.keyCode() }.0 as isize;
    let flags = unsafe { key.modifierFlags() };
    let shortcut = flags.intersects(
        UIKeyModifierFlags::UIKeyModifierCommand | UIKeyModifierFlags::UIKeyModifierControl,
    );
    let characters = unsafe { key.characters() }.to_string();
    let ignoring = unsafe { key.charactersIgnoringModifiers() }.to_string();
    let (physical_key, named, location) = hid_key(usage);
    let (logical_key, text, types_text) = match named {
        Some(NamedKey::Space) => {
            (Key::Named(NamedKey::Space), (!shortcut).then(|| SmolStr::new(" ")), !shortcut)
        },
        // In a text field, UIKeyInput's deleteBackward takes it (and repeats it), once.
        Some(NamedKey::Backspace) => (Key::Named(NamedKey::Backspace), None, !shortcut),
        Some(n) => (Key::Named(n), None, false),
        None => {
            // As macOS does: the character of the layout (AZERTY, QWERTZ…), shifted, but without
            // Option, Command or Control.
            let shifted = flags.contains(UIKeyModifierFlags::UIKeyModifierShift);
            let base = printable(&ignoring).map(|s| {
                if shifted { SmolStr::new(s.to_uppercase()) } else { s }
            });
            let logical = match base.or_else(|| printable(&characters)) {
                Some(s) => Key::Character(s),
                None if characters.is_empty() => Key::Dead(None),
                None => Key::Unidentified(NativeKey::Unidentified),
            };
            let text = if shortcut { None } else { printable(&characters) };
            (logical, text, !shortcut)
        },
    };
    PressedKey { usage, physical_key, logical_key, location, text, types_text }
}
