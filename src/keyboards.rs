// src/keyboards.rs
//
// Physical keyboard structure and language layout definitions.
// All source bytes are ASCII; non-ASCII runtime values use \u{XXXX} escapes.

// =============================================================================
// Key-width kinds
// =============================================================================

/// Semantic width category for each physical key.
/// The layout is ortholinear: every key is the same width (Std).
/// Pixel values are computed in main.rs from the screen width.
#[derive(Clone, Copy)]
pub enum KW {
    Std,    // uniform key width -- every non-space key
    Spacer, // invisible gap, same pixel width as Std, no button rendered
    Space,  // space bar: fills the remaining width of the bottom row
}

// =============================================================================
// Physical key actions
// =============================================================================

/// What a physical key does when activated.
///
/// `Regular(n)` slots are substitutable: each LayoutDef supplies one entry per
/// slot, enabling layout switching without changing the key structure.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    /// Index into LayoutDef::keys (0..REGULAR_KEY_COUNT).
    Regular(usize),
    Backspace,
    Tab,
    CapsLock,
    Enter,
    LShift,
    RShift,
    Ctrl,
    Win,
    Alt,
    AltGr,
    Space,
    // --- Function keys ---
    Esc,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // --- Navigation cluster ---
    Insert, Delete, Home, End, PageUp, PageDown,
    // --- Arrow keys ---
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    /// Spacer slot: advances x by key_w but creates no button.
    Noop,
}

/// Button face label for a non-Regular key.
pub fn special_label(action: Action) -> &'static str {
    match action {
        Action::Backspace  => "Bksp",
        Action::Tab        => "Tab",
        Action::CapsLock   => "Caps",
        Action::Enter      => "Enter",
        Action::LShift     => "Shift",
        Action::RShift     => "Shift",
        Action::Ctrl       => "Ctrl",
        Action::Win        => "Win",
        Action::Alt        => "Alt",
        Action::AltGr      => "AltGr",
        Action::Esc        => "Esc",
        Action::F1         => "F1",
        Action::F2         => "F2",
        Action::F3         => "F3",
        Action::F4         => "F4",
        Action::F5         => "F5",
        Action::F6         => "F6",
        Action::F7         => "F7",
        Action::F8         => "F8",
        Action::F9         => "F9",
        Action::F10        => "F10",
        Action::F11        => "F11",
        Action::F12        => "F12",
        Action::Insert     => "Ins",
        Action::Delete     => "Del",
        Action::Home       => "Home",
        Action::End        => "End",
        Action::PageUp     => "PgUp",
        Action::PageDown   => "PgDn",
        Action::ArrowUp    => "\u{2191}",
        Action::ArrowDown  => "\u{2193}",
        Action::ArrowLeft  => "\u{2190}",
        Action::ArrowRight => "\u{2192}",
        Action::Space | Action::Regular(_) | Action::Noop => "",
    }
}

/// Hook token for a non-Regular key (passed to KeyHook callbacks).
pub fn special_hook_str(action: Action) -> &'static str {
    match action {
        Action::Backspace  => "Backspace",
        Action::Tab        => "Tab",
        Action::Enter      => "Enter",
        Action::Space      => "Space",
        Action::CapsLock   => "CapsLock",
        Action::LShift     => "LShift",
        Action::RShift     => "RShift",
        Action::Ctrl       => "Ctrl",
        Action::Win        => "Win",
        Action::Alt        => "Alt",
        Action::AltGr      => "AltGr",
        Action::Esc        => "Esc",
        Action::F1         => "F1",
        Action::F2         => "F2",
        Action::F3         => "F3",
        Action::F4         => "F4",
        Action::F5         => "F5",
        Action::F6         => "F6",
        Action::F7         => "F7",
        Action::F8         => "F8",
        Action::F9         => "F9",
        Action::F10        => "F10",
        Action::F11        => "F11",
        Action::F12        => "F12",
        Action::Insert     => "Insert",
        Action::Delete     => "Delete",
        Action::Home       => "Home",
        Action::End        => "End",
        Action::PageUp     => "PageUp",
        Action::PageDown   => "PageDown",
        Action::ArrowUp    => "ArrowUp",
        Action::ArrowDown  => "ArrowDown",
        Action::ArrowLeft  => "ArrowLeft",
        Action::ArrowRight => "ArrowRight",
        Action::Regular(_) | Action::Noop => "",
    }
}

/// Returns true if this action is a toggling modifier key.
/// CapsLock always toggles; Ctrl/Shift/Alt/AltGr are sticky-toggle.
pub fn is_modifier(action: Action) -> bool {
    matches!(
        action,
        Action::CapsLock
            | Action::LShift
            | Action::RShift
            | Action::Ctrl
            | Action::Alt
            | Action::AltGr
    )
}

/// Returns true if this modifier is sticky (auto-releases after next regular key).
pub fn is_sticky(action: Action) -> bool {
    matches!(
        action,
        Action::LShift | Action::RShift | Action::Ctrl | Action::Alt | Action::AltGr
    )
}

// =============================================================================
// Physical keyboard structure
// =============================================================================

/// A single physical key: its visual width, logical action, and Linux evdev
/// scancode (linux/input-event-codes.h).
pub struct PhysKey {
    pub kw:       KW,
    pub action:   Action,
    /// Linux evdev scancode (linux/input-event-codes.h).
    pub scancode: u16,
}

/// Number of Regular(n) slots in KEYS.
/// Every LayoutDef::keys slice must have exactly this many entries.
pub const REGULAR_KEY_COUNT: usize = 47;

// Linux evdev key codes (linux/input-event-codes.h):
//   KEY_ESC=1
//   KEY_F1..F10=59..68  KEY_F11=87  KEY_F12=88
//   KEY_GRAVE=41  KEY_1..KEY_0=2..11  KEY_MINUS=12  KEY_EQUAL=13
//   KEY_BACKSPACE=14  KEY_TAB=15
//   KEY_Q..KEY_P=16..25  KEY_LEFTBRACE=26  KEY_RIGHTBRACE=27  KEY_BACKSLASH=43
//   KEY_CAPSLOCK=58
//   KEY_A..KEY_L=30..38  KEY_SEMICOLON=39  KEY_APOSTROPHE=40  KEY_ENTER=28
//   KEY_LEFTSHIFT=42  KEY_Z..KEY_SLASH=44..53  KEY_RIGHTSHIFT=54
//   KEY_LEFTCTRL=29  KEY_LEFTMETA=125  KEY_LEFTALT=56
//   KEY_SPACE=57  KEY_RIGHTALT=100  KEY_RIGHTCTRL=97
//   KEY_INSERT=110  KEY_DELETE=111  KEY_HOME=102  KEY_END=107
//   KEY_PAGEUP=104  KEY_PAGEDOWN=109
//   KEY_UP=103  KEY_DOWN=108  KEY_LEFT=105  KEY_RIGHT=106
//
// Ortholinear grid – 18 uniform columns, 6 rows.
// An extra Spacer column separates the main key block (cols 0-13) from the
// navigation / arrow cluster (cols 15-17).  The Spacer is invisible (no button)
// but occupies the same pixel width as a regular key, creating a clear gap.
//
// Column layout (0-indexed):
//   Cols  0-13: main keyboard block
//   Col  14:    Spacer (visual separator, no button)
//   Cols 15-17: navigation / arrow cluster

pub static KEYS: &[&[PhysKey]] = &[
    // --- Row 0: Function key row (13 keys) ---
    &[
        PhysKey { kw: KW::Std, action: Action::Esc, scancode: 0x01 },
        PhysKey { kw: KW::Std, action: Action::F1,  scancode: 0x3b },
        PhysKey { kw: KW::Std, action: Action::F2,  scancode: 0x3c },
        PhysKey { kw: KW::Std, action: Action::F3,  scancode: 0x3d },
        PhysKey { kw: KW::Std, action: Action::F4,  scancode: 0x3e },
        PhysKey { kw: KW::Std, action: Action::F5,  scancode: 0x3f },
        PhysKey { kw: KW::Std, action: Action::F6,  scancode: 0x40 },
        PhysKey { kw: KW::Std, action: Action::F7,  scancode: 0x41 },
        PhysKey { kw: KW::Std, action: Action::F8,  scancode: 0x42 },
        PhysKey { kw: KW::Std, action: Action::F9,  scancode: 0x43 },
        PhysKey { kw: KW::Std, action: Action::F10, scancode: 0x44 },
        PhysKey { kw: KW::Std, action: Action::F11, scancode: 0x57 },
        PhysKey { kw: KW::Std, action: Action::F12, scancode: 0x58 },
    ],
    // --- Row 1: Number row + separator + nav cluster (18 slots) ---
    &[
        PhysKey { kw: KW::Std,    action: Action::Regular(0),  scancode: 0x29 }, // `
        PhysKey { kw: KW::Std,    action: Action::Regular(1),  scancode: 0x02 }, // 1
        PhysKey { kw: KW::Std,    action: Action::Regular(2),  scancode: 0x03 }, // 2
        PhysKey { kw: KW::Std,    action: Action::Regular(3),  scancode: 0x04 }, // 3
        PhysKey { kw: KW::Std,    action: Action::Regular(4),  scancode: 0x05 }, // 4
        PhysKey { kw: KW::Std,    action: Action::Regular(5),  scancode: 0x06 }, // 5
        PhysKey { kw: KW::Std,    action: Action::Regular(6),  scancode: 0x07 }, // 6
        PhysKey { kw: KW::Std,    action: Action::Regular(7),  scancode: 0x08 }, // 7
        PhysKey { kw: KW::Std,    action: Action::Regular(8),  scancode: 0x09 }, // 8
        PhysKey { kw: KW::Std,    action: Action::Regular(9),  scancode: 0x0a }, // 9
        PhysKey { kw: KW::Std,    action: Action::Regular(10), scancode: 0x0b }, // 0
        PhysKey { kw: KW::Std,    action: Action::Regular(11), scancode: 0x0c }, // -
        PhysKey { kw: KW::Std,    action: Action::Regular(12), scancode: 0x0d }, // =
        PhysKey { kw: KW::Std,    action: Action::Backspace,   scancode: 0x0e }, // Bksp
        PhysKey { kw: KW::Std,    action: Action::Insert,      scancode: 0x6e }, // Ins
        PhysKey { kw: KW::Std,    action: Action::Home,        scancode: 0x66 }, // Home
        PhysKey { kw: KW::Std,    action: Action::PageUp,      scancode: 0x68 }, // PgUp
    ],
    // --- Row 2: Top alpha row + separator + nav cluster (18 slots) ---
    &[
        PhysKey { kw: KW::Std,    action: Action::Tab,          scancode: 0x0f }, // Tab
        PhysKey { kw: KW::Std,    action: Action::Regular(13),  scancode: 0x10 }, // q
        PhysKey { kw: KW::Std,    action: Action::Regular(14),  scancode: 0x11 }, // w
        PhysKey { kw: KW::Std,    action: Action::Regular(15),  scancode: 0x12 }, // e
        PhysKey { kw: KW::Std,    action: Action::Regular(16),  scancode: 0x13 }, // r
        PhysKey { kw: KW::Std,    action: Action::Regular(17),  scancode: 0x14 }, // t
        PhysKey { kw: KW::Std,    action: Action::Regular(18),  scancode: 0x15 }, // y
        PhysKey { kw: KW::Std,    action: Action::Regular(19),  scancode: 0x16 }, // u
        PhysKey { kw: KW::Std,    action: Action::Regular(20),  scancode: 0x17 }, // i
        PhysKey { kw: KW::Std,    action: Action::Regular(21),  scancode: 0x18 }, // o
        PhysKey { kw: KW::Std,    action: Action::Regular(22),  scancode: 0x19 }, // p
        PhysKey { kw: KW::Std,    action: Action::Regular(23),  scancode: 0x1a }, // [
        PhysKey { kw: KW::Std,    action: Action::Regular(24),  scancode: 0x1b }, // ]
        PhysKey { kw: KW::Std,    action: Action::Regular(25),  scancode: 0x2b }, // backslash
        PhysKey { kw: KW::Std,    action: Action::Delete,       scancode: 0x6f }, // Del
        PhysKey { kw: KW::Std,    action: Action::End,          scancode: 0x6b }, // End
        PhysKey { kw: KW::Std,    action: Action::PageDown,     scancode: 0x6d }, // PgDn
    ],
    // --- Row 3: Home row (13 keys, left-aligned) ---
    &[
        PhysKey { kw: KW::Std,  action: Action::CapsLock,    scancode: 0x3a }, // Caps
        PhysKey { kw: KW::Std,  action: Action::Regular(26), scancode: 0x1e }, // a
        PhysKey { kw: KW::Std,  action: Action::Regular(27), scancode: 0x1f }, // s
        PhysKey { kw: KW::Std,  action: Action::Regular(28), scancode: 0x20 }, // d
        PhysKey { kw: KW::Std,  action: Action::Regular(29), scancode: 0x21 }, // f
        PhysKey { kw: KW::Std,  action: Action::Regular(30), scancode: 0x22 }, // g
        PhysKey { kw: KW::Std,  action: Action::Regular(31), scancode: 0x23 }, // h
        PhysKey { kw: KW::Std,  action: Action::Regular(32), scancode: 0x24 }, // j
        PhysKey { kw: KW::Std,  action: Action::Regular(33), scancode: 0x25 }, // k
        PhysKey { kw: KW::Std,  action: Action::Regular(34), scancode: 0x26 }, // l
        PhysKey { kw: KW::Std,  action: Action::Regular(35), scancode: 0x27 }, // ;
        PhysKey { kw: KW::Std,  action: Action::Regular(36), scancode: 0x28 }, // '
        PhysKey { kw: KW::Std,  action: Action::Enter,       scancode: 0x1c }, // Enter
    ],
    // --- Row 4: Lower alpha row + Spacers + arrow-up ---
    &[
        PhysKey { kw: KW::Std,    action: Action::LShift,      scancode: 0x2a }, // LShift
        PhysKey { kw: KW::Std,    action: Action::Regular(37), scancode: 0x2c }, // z
        PhysKey { kw: KW::Std,    action: Action::Regular(38), scancode: 0x2d }, // x
        PhysKey { kw: KW::Std,    action: Action::Regular(39), scancode: 0x2e }, // c
        PhysKey { kw: KW::Std,    action: Action::Regular(40), scancode: 0x2f }, // v
        PhysKey { kw: KW::Std,    action: Action::Regular(41), scancode: 0x30 }, // b
        PhysKey { kw: KW::Std,    action: Action::Regular(42), scancode: 0x31 }, // n
        PhysKey { kw: KW::Std,    action: Action::Regular(43), scancode: 0x32 }, // m
        PhysKey { kw: KW::Std,    action: Action::Regular(44), scancode: 0x33 }, // ,
        PhysKey { kw: KW::Std,    action: Action::Regular(45), scancode: 0x34 }, // .
        PhysKey { kw: KW::Std,    action: Action::Regular(46), scancode: 0x35 }, // /
        PhysKey { kw: KW::Std,    action: Action::RShift,      scancode: 0x36 }, // RShift
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode: 0x00 }, // gap
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode: 0x00 }, // gap
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode: 0x00 }, // gap
        PhysKey { kw: KW::Std,    action: Action::ArrowUp,     scancode: 0x67 }, // ↑
    ],
    // --- Row 5: Bottom row + separator + arrow cluster ---
    &[
        PhysKey { kw: KW::Std,    action: Action::Ctrl,       scancode: 0x1d }, // LCtrl
        PhysKey { kw: KW::Std,    action: Action::Win,        scancode: 0x7d }, // LMeta/Win
        PhysKey { kw: KW::Std,    action: Action::Alt,        scancode: 0x38 }, // LAlt
        PhysKey { kw: KW::Space,  action: Action::Space,      scancode: 0x39 }, // Space
        PhysKey { kw: KW::Std,    action: Action::AltGr,      scancode: 0x64 }, // RAlt/AltGr
        PhysKey { kw: KW::Std,    action: Action::Ctrl,       scancode: 0x61 }, // RCtrl
        PhysKey { kw: KW::Spacer, action: Action::Noop,       scancode: 0x00 }, // separator
        PhysKey { kw: KW::Spacer, action: Action::Noop,       scancode: 0x00 }, // separator
        PhysKey { kw: KW::Spacer, action: Action::Noop,       scancode: 0x00 }, // separator
        PhysKey { kw: KW::Std,    action: Action::ArrowLeft,  scancode: 0x69 }, // ←
        PhysKey { kw: KW::Std,    action: Action::ArrowDown,  scancode: 0x6c }, // ↓
        PhysKey { kw: KW::Std,    action: Action::ArrowRight, scancode: 0x6a }, // →
    ],
];

// =============================================================================
// Language layouts
// =============================================================================

/// One substitutable key slot: display labels and strings to insert.
pub struct LayoutKey {
    /// Text shown on the button face (unshifted state).
    pub label_unshifted:  &'static str,
    /// String inserted when the key is activated without Shift / CapsLock.
    pub insert_unshifted: &'static str,
    /// Text shown in the top half of the button face (shifted state).
    /// Empty for letter keys (they use `insert_unshifted.to_uppercase()` instead,
    /// and CapsLock also applies to them).
    pub label_shifted:    &'static str,
    /// String inserted when Shift is held.
    /// Empty for letter keys (uppercase is computed from `insert_unshifted`).
    pub insert_shifted:   &'static str,
}

/// A named keyboard layout.
///
/// To add a new language:
///   1. Define `pub static MY_LANG: LayoutDef = LayoutDef { name: "XX", keys: &[...] };`
///      with exactly REGULAR_KEY_COUNT entries in slot order (see slot index map
///      in the KEYS comment above).
///   2. Append `&MY_LANG` to the LAYOUTS slice below.
///
/// No changes to main.rs are required; the toggle button appears automatically.
pub struct LayoutDef {
    pub name: &'static str,
    pub keys: &'static [LayoutKey],
}

// ---------------------------------------------------------------------------
// US (QWERTY)
// ---------------------------------------------------------------------------
pub static US: LayoutDef = LayoutDef {
    name: "US",
    keys: &[
        // slots 0-12: number row  (shifted = symbol above the digit/key)
        LayoutKey { label_unshifted: "`",  insert_unshifted: "`",  label_shifted: "~",  insert_shifted: "~"  }, // 0
        LayoutKey { label_unshifted: "1",  insert_unshifted: "1",  label_shifted: "!",  insert_shifted: "!"  }, // 1
        LayoutKey { label_unshifted: "2",  insert_unshifted: "2",  label_shifted: "@@", insert_shifted: "@"  }, // 2
        LayoutKey { label_unshifted: "3",  insert_unshifted: "3",  label_shifted: "#",  insert_shifted: "#"  }, // 3
        LayoutKey { label_unshifted: "4",  insert_unshifted: "4",  label_shifted: "$",  insert_shifted: "$"  }, // 4
        LayoutKey { label_unshifted: "5",  insert_unshifted: "5",  label_shifted: "%",  insert_shifted: "%"  }, // 5
        LayoutKey { label_unshifted: "6",  insert_unshifted: "6",  label_shifted: "^",  insert_shifted: "^"  }, // 6
        LayoutKey { label_unshifted: "7",  insert_unshifted: "7",  label_shifted: "&&", insert_shifted: "&"  }, // 7
        LayoutKey { label_unshifted: "8",  insert_unshifted: "8",  label_shifted: "*",  insert_shifted: "*"  }, // 8
        LayoutKey { label_unshifted: "9",  insert_unshifted: "9",  label_shifted: "(",  insert_shifted: "("  }, // 9
        LayoutKey { label_unshifted: "0",  insert_unshifted: "0",  label_shifted: ")",  insert_shifted: ")"  }, // 10
        LayoutKey { label_unshifted: "-",  insert_unshifted: "-",  label_shifted: "_",  insert_shifted: "_"  }, // 11
        LayoutKey { label_unshifted: "=",  insert_unshifted: "=",  label_shifted: "+",  insert_shifted: "+"  }, // 12
        // slots 13-22: top alpha row (q-p) – letter keys, no shifted display
        LayoutKey { label_unshifted: "q",  insert_unshifted: "q",  label_shifted: "",   insert_shifted: ""   }, // 13
        LayoutKey { label_unshifted: "w",  insert_unshifted: "w",  label_shifted: "",   insert_shifted: ""   }, // 14
        LayoutKey { label_unshifted: "e",  insert_unshifted: "e",  label_shifted: "",   insert_shifted: ""   }, // 15
        LayoutKey { label_unshifted: "r",  insert_unshifted: "r",  label_shifted: "",   insert_shifted: ""   }, // 16
        LayoutKey { label_unshifted: "t",  insert_unshifted: "t",  label_shifted: "",   insert_shifted: ""   }, // 17
        LayoutKey { label_unshifted: "y",  insert_unshifted: "y",  label_shifted: "",   insert_shifted: ""   }, // 18
        LayoutKey { label_unshifted: "u",  insert_unshifted: "u",  label_shifted: "",   insert_shifted: ""   }, // 19
        LayoutKey { label_unshifted: "i",  insert_unshifted: "i",  label_shifted: "",   insert_shifted: ""   }, // 20
        LayoutKey { label_unshifted: "o",  insert_unshifted: "o",  label_shifted: "",   insert_shifted: ""   }, // 21
        LayoutKey { label_unshifted: "p",  insert_unshifted: "p",  label_shifted: "",   insert_shifted: ""   }, // 22
        // slots 23-25: top-row punctuation
        LayoutKey { label_unshifted: "[",  insert_unshifted: "[",  label_shifted: "{",  insert_shifted: "{"  }, // 23
        LayoutKey { label_unshifted: "]",  insert_unshifted: "]",  label_shifted: "}",  insert_shifted: "}"  }, // 24
        LayoutKey { label_unshifted: "\\", insert_unshifted: "\\", label_shifted: "|",  insert_shifted: "|"  }, // 25
        // slots 26-34: home alpha row (a-l) – letter keys
        LayoutKey { label_unshifted: "a",  insert_unshifted: "a",  label_shifted: "",   insert_shifted: ""   }, // 26
        LayoutKey { label_unshifted: "s",  insert_unshifted: "s",  label_shifted: "",   insert_shifted: ""   }, // 27
        LayoutKey { label_unshifted: "d",  insert_unshifted: "d",  label_shifted: "",   insert_shifted: ""   }, // 28
        LayoutKey { label_unshifted: "f",  insert_unshifted: "f",  label_shifted: "",   insert_shifted: ""   }, // 29
        LayoutKey { label_unshifted: "g",  insert_unshifted: "g",  label_shifted: "",   insert_shifted: ""   }, // 30
        LayoutKey { label_unshifted: "h",  insert_unshifted: "h",  label_shifted: "",   insert_shifted: ""   }, // 31
        LayoutKey { label_unshifted: "j",  insert_unshifted: "j",  label_shifted: "",   insert_shifted: ""   }, // 32
        LayoutKey { label_unshifted: "k",  insert_unshifted: "k",  label_shifted: "",   insert_shifted: ""   }, // 33
        LayoutKey { label_unshifted: "l",  insert_unshifted: "l",  label_shifted: "",   insert_shifted: ""   }, // 34
        // slots 35-36: home-row punctuation
        LayoutKey { label_unshifted: ";",  insert_unshifted: ";",  label_shifted: ":",  insert_shifted: ":"  }, // 35
        LayoutKey { label_unshifted: "'",  insert_unshifted: "'",  label_shifted: "\"", insert_shifted: "\"" }, // 36
        // slots 37-43: lower alpha row (z-m) – letter keys
        LayoutKey { label_unshifted: "z",  insert_unshifted: "z",  label_shifted: "",   insert_shifted: ""   }, // 37
        LayoutKey { label_unshifted: "x",  insert_unshifted: "x",  label_shifted: "",   insert_shifted: ""   }, // 38
        LayoutKey { label_unshifted: "c",  insert_unshifted: "c",  label_shifted: "",   insert_shifted: ""   }, // 39
        LayoutKey { label_unshifted: "v",  insert_unshifted: "v",  label_shifted: "",   insert_shifted: ""   }, // 40
        LayoutKey { label_unshifted: "b",  insert_unshifted: "b",  label_shifted: "",   insert_shifted: ""   }, // 41
        LayoutKey { label_unshifted: "n",  insert_unshifted: "n",  label_shifted: "",   insert_shifted: ""   }, // 42
        LayoutKey { label_unshifted: "m",  insert_unshifted: "m",  label_shifted: "",   insert_shifted: ""   }, // 43
        // slots 44-46: lower-row punctuation
        LayoutKey { label_unshifted: ",",  insert_unshifted: ",",  label_shifted: "<",  insert_shifted: "<"  }, // 44
        LayoutKey { label_unshifted: ".",  insert_unshifted: ".",  label_shifted: ">",  insert_shifted: ">"  }, // 45
        LayoutKey { label_unshifted: "/",  insert_unshifted: "/",  label_shifted: "?",  insert_shifted: "?"  }, // 46
    ],
};

// ---------------------------------------------------------------------------
// Ukrainian (QWERTY-UA)
//
// All non-ASCII runtime values use \u{XXXX} Rust escape sequences; the source
// file contains only ASCII bytes.
//
// Slot -> Ukrainian character:
//   0  ` -> \u{0027} APOSTROPHE / \u{20b4} HRYVNA SYMBOL
//  13  q -> \u{0439} CYRILLIC SMALL LETTER SHORT I       (J)
//  14  w -> \u{0446} CYRILLIC SMALL LETTER TSE           (Ts)
//  15  e -> \u{0443} CYRILLIC SMALL LETTER U
//  16  r -> \u{043A} CYRILLIC SMALL LETTER KA            (K)
//  17  t -> \u{0435} CYRILLIC SMALL LETTER IE            (Ye)
//  18  y -> \u{043D} CYRILLIC SMALL LETTER EN            (N)
//  19  u -> \u{0433} CYRILLIC SMALL LETTER GHE           (G)
//  20  i -> \u{0448} CYRILLIC SMALL LETTER SHA           (Sh)
//  21  o -> \u{0449} CYRILLIC SMALL LETTER SHCHA         (Shch)
//  22  p -> \u{0437} CYRILLIC SMALL LETTER ZE            (Z)
//  23  [ -> \u{0445} CYRILLIC SMALL LETTER HA            (Kh)
//  24  ] -> \u{0457} CYRILLIC SMALL LETTER YI            (Yi)
//  25  \ -> \ (unchanged)
//  26  a -> \u{0444} CYRILLIC SMALL LETTER EF            (F)
//  27  s -> \u{0456} CYRILLIC SMALL LETTER BYELORUSSIAN-UKRAINIAN I
//  28  d -> \u{0432} CYRILLIC SMALL LETTER VE            (V)
//  29  f -> \u{0430} CYRILLIC SMALL LETTER A
//  30  g -> \u{043F} CYRILLIC SMALL LETTER PE            (P)
//  31  h -> \u{0440} CYRILLIC SMALL LETTER ER            (R)
//  32  j -> \u{043E} CYRILLIC SMALL LETTER O
//  33  k -> \u{043B} CYRILLIC SMALL LETTER EL            (L)
//  34  l -> \u{0434} CYRILLIC SMALL LETTER DE            (D)
//  35  ; -> \u{0436} CYRILLIC SMALL LETTER ZHE           (Zh)
//  36  ' -> \u{0454} CYRILLIC SMALL LETTER UKRAINIAN IE  (Ye)
//  37  z -> \u{044F} CYRILLIC SMALL LETTER YA            (Ya)
//  38  x -> \u{0447} CYRILLIC SMALL LETTER CHE           (Ch)
//  39  c -> \u{0441} CYRILLIC SMALL LETTER ES            (S)
//  40  v -> \u{043C} CYRILLIC SMALL LETTER EM            (M)
//  41  b -> \u{0438} CYRILLIC SMALL LETTER I
//  42  n -> \u{0442} CYRILLIC SMALL LETTER TE            (T)
//  43  m -> \u{044C} CYRILLIC SMALL LETTER SOFT SIGN
//  44  , -> \u{0431} CYRILLIC SMALL LETTER BE            (B)
//  45  . -> \u{044E} CYRILLIC SMALL LETTER YU            (Yu)
//  46  / -> FULL STOP, COMMA
// ---------------------------------------------------------------------------
pub static UA: LayoutDef = LayoutDef {
    name: "UA",
    keys: &[
        // slots 0-12: number row
        // The grave key carries the Ukrainian apostrophe; its physical Shift value is ~.
        // Number-row shifted symbols follow the KBDUR standard (differ from US layout).
        LayoutKey { label_unshifted: "\u{0027}", insert_unshifted: "\u{0027}", label_shifted: "\u{20b4}",  insert_shifted: "\u{20b4}"  }, // 0  ` -> apostrophe, hryvna
        LayoutKey { label_unshifted: "1",        insert_unshifted: "1",        label_shifted: "!",         insert_shifted: "!"         }, // 1
        LayoutKey { label_unshifted: "2",        insert_unshifted: "2",        label_shifted: "\"",        insert_shifted: "\""        }, // 2  Shift+2 -> "
        LayoutKey { label_unshifted: "3",        insert_unshifted: "3",        label_shifted: "\u{2116}",  insert_shifted: "\u{2116}"  }, // 3  Shift+3 -> №
        LayoutKey { label_unshifted: "4",        insert_unshifted: "4",        label_shifted: ";",         insert_shifted: ";"         }, // 4  Shift+4 -> ;
        LayoutKey { label_unshifted: "5",        insert_unshifted: "5",        label_shifted: "%",         insert_shifted: "%"         }, // 5
        LayoutKey { label_unshifted: "6",        insert_unshifted: "6",        label_shifted: ":",         insert_shifted: ":"         }, // 6  Shift+6 -> :
        LayoutKey { label_unshifted: "7",        insert_unshifted: "7",        label_shifted: "?",         insert_shifted: "?"         }, // 7  Shift+7 -> ?
        LayoutKey { label_unshifted: "8",        insert_unshifted: "8",        label_shifted: "*",         insert_shifted: "*"         }, // 8
        LayoutKey { label_unshifted: "9",        insert_unshifted: "9",        label_shifted: "(",         insert_shifted: "("         }, // 9
        LayoutKey { label_unshifted: "0",        insert_unshifted: "0",        label_shifted: ")",         insert_shifted: ")"         }, // 10
        LayoutKey { label_unshifted: "-",        insert_unshifted: "-",        label_shifted: "_",         insert_shifted: "_"         }, // 11
        LayoutKey { label_unshifted: "=",        insert_unshifted: "=",        label_shifted: "+",         insert_shifted: "+"         }, // 12
        // slots 13-22: top alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label_unshifted: "\u{0439}", insert_unshifted: "\u{0439}", label_shifted: "", insert_shifted: "" }, // 13  q -> J
        LayoutKey { label_unshifted: "\u{0446}", insert_unshifted: "\u{0446}", label_shifted: "", insert_shifted: "" }, // 14  w -> Ts
        LayoutKey { label_unshifted: "\u{0443}", insert_unshifted: "\u{0443}", label_shifted: "", insert_shifted: "" }, // 15  e -> U
        LayoutKey { label_unshifted: "\u{043A}", insert_unshifted: "\u{043A}", label_shifted: "", insert_shifted: "" }, // 16  r -> K
        LayoutKey { label_unshifted: "\u{0435}", insert_unshifted: "\u{0435}", label_shifted: "", insert_shifted: "" }, // 17  t -> Ye
        LayoutKey { label_unshifted: "\u{043D}", insert_unshifted: "\u{043D}", label_shifted: "", insert_shifted: "" }, // 18  y -> N
        LayoutKey { label_unshifted: "\u{0433}", insert_unshifted: "\u{0433}", label_shifted: "", insert_shifted: "" }, // 19  u -> G
        LayoutKey { label_unshifted: "\u{0448}", insert_unshifted: "\u{0448}", label_shifted: "", insert_shifted: "" }, // 20  i -> Sh
        LayoutKey { label_unshifted: "\u{0449}", insert_unshifted: "\u{0449}", label_shifted: "", insert_shifted: "" }, // 21  o -> Shch
        LayoutKey { label_unshifted: "\u{0437}", insert_unshifted: "\u{0437}", label_shifted: "", insert_shifted: "" }, // 22  p -> Z
        // slots 23-25: top-row bracket/backslash keys.
        // [ and ] now hold Cyrillic letters; Shift produces their uppercase, no secondary symbol.
        // \ is unchanged from US (Shift+\ = |).
        LayoutKey { label_unshifted: "\u{0445}", insert_unshifted: "\u{0445}", label_shifted: "", insert_shifted: "" }, // 23  [ -> Kh
        LayoutKey { label_unshifted: "\u{0457}", insert_unshifted: "\u{0457}", label_shifted: "", insert_shifted: "" }, // 24  ] -> Yi
        LayoutKey { label_unshifted: "\\",       insert_unshifted: "\\",       label_shifted: "|", insert_shifted: "|" }, // 25  \ -> same
        // slots 26-34: home alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label_unshifted: "\u{0444}", insert_unshifted: "\u{0444}", label_shifted: "", insert_shifted: "" }, // 26  a -> F
        LayoutKey { label_unshifted: "\u{0456}", insert_unshifted: "\u{0456}", label_shifted: "", insert_shifted: "" }, // 27  s -> I
        LayoutKey { label_unshifted: "\u{0432}", insert_unshifted: "\u{0432}", label_shifted: "", insert_shifted: "" }, // 28  d -> V
        LayoutKey { label_unshifted: "\u{0430}", insert_unshifted: "\u{0430}", label_shifted: "", insert_shifted: "" }, // 29  f -> A
        LayoutKey { label_unshifted: "\u{043F}", insert_unshifted: "\u{043F}", label_shifted: "", insert_shifted: "" }, // 30  g -> P
        LayoutKey { label_unshifted: "\u{0440}", insert_unshifted: "\u{0440}", label_shifted: "", insert_shifted: "" }, // 31  h -> R
        LayoutKey { label_unshifted: "\u{043E}", insert_unshifted: "\u{043E}", label_shifted: "", insert_shifted: "" }, // 32  j -> O
        LayoutKey { label_unshifted: "\u{043B}", insert_unshifted: "\u{043B}", label_shifted: "", insert_shifted: "" }, // 33  k -> L
        LayoutKey { label_unshifted: "\u{0434}", insert_unshifted: "\u{0434}", label_shifted: "", insert_shifted: "" }, // 34  l -> D
        // slots 35-36: home-row keys that now hold Cyrillic letters.
        // Shift produces their uppercase; there is no secondary punctuation symbol.
        LayoutKey { label_unshifted: "\u{0436}", insert_unshifted: "\u{0436}", label_shifted: "", insert_shifted: "" }, // 35  ; -> Zh
        LayoutKey { label_unshifted: "\u{0454}", insert_unshifted: "\u{0454}", label_shifted: "", insert_shifted: "" }, // 36  ' -> Ye
        // slots 37-43: lower alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label_unshifted: "\u{044F}", insert_unshifted: "\u{044F}", label_shifted: "", insert_shifted: "" }, // 37  z -> Ya
        LayoutKey { label_unshifted: "\u{0447}", insert_unshifted: "\u{0447}", label_shifted: "", insert_shifted: "" }, // 38  x -> Ch
        LayoutKey { label_unshifted: "\u{0441}", insert_unshifted: "\u{0441}", label_shifted: "", insert_shifted: "" }, // 39  c -> S
        LayoutKey { label_unshifted: "\u{043C}", insert_unshifted: "\u{043C}", label_shifted: "", insert_shifted: "" }, // 40  v -> M
        LayoutKey { label_unshifted: "\u{0438}", insert_unshifted: "\u{0438}", label_shifted: "", insert_shifted: "" }, // 41  b -> I
        LayoutKey { label_unshifted: "\u{0442}", insert_unshifted: "\u{0442}", label_shifted: "", insert_shifted: "" }, // 42  n -> T
        LayoutKey { label_unshifted: "\u{044C}", insert_unshifted: "\u{044C}", label_shifted: "", insert_shifted: "" }, // 43  m -> soft sign
        // slots 44-46: lower-row punctuation
        LayoutKey { label_unshifted: "\u{0431}", insert_unshifted: "\u{0431}", label_shifted: "", insert_shifted: "" }, // 44  , -> B
        LayoutKey { label_unshifted: "\u{044E}", insert_unshifted: "\u{044E}", label_shifted: "", insert_shifted: "" }, // 45  . -> Yu
        LayoutKey { label_unshifted: ".",        insert_unshifted: ".",        label_shifted: ",", insert_shifted: "," }, // 46  / -> FULL STOP, COMMA
    ],
};

/// All available layouts.
///
/// To add a new language: define a new LayoutDef constant (see UA above) and
/// append a reference to it here.  The toggle button appears automatically.
pub static LAYOUTS: &[&LayoutDef] = &[&US, &UA];
