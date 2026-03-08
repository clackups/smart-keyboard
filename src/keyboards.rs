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
//
//                 0    1    2    3    4    5    6    7    8    9   10   11   12   13  sep  15    16    17
// Row 0 (F):    Esc   F1   F2   F3   F4   F5   F6   F7   F8   F9  F10  F11  F12  (13 keys, left-aligned)
// Row 1 (num):   `    1    2    3    4    5    6    7    8    9    0    -    =   Bksp  ░  Ins  Home  PgUp
// Row 2 (top): Tab    q    w    e    r    t    y    u    i    o    p    [    ]    \    ░  Del  End   PgDn
// Row 3 (home):Caps   a    s    d    f    g    h    j    k    l    ;    '  Enter  (13 keys, left-aligned)
// Row 4 (low): Sft    z    x    c    v    b    n    m    ,    .    /   Sft (sp)(sp)(sp)(sp) [↑]
// Row 5 (bot): Ctrl  Win  Alt  [      Space      ] AltGr Ctrl  ░   ←    ↓    →
//
// ░ = Spacer (no button rendered).
//
// Alignment proof (↑ in row 4 aligns with ↓ in row 5):
//   key_w = (avail_w - 17*gap) / 18  →  avail_w ≈ 18*key_w + 17*gap
//   space_w = avail_w - 9*key_w - 9*gap ≈ 9*key_w + 8*gap
//   Row 5: 3 left mods + Space + 2 right mods + Spacer + ← + ↓ + →
//     x(↓) = pad + 3*(kw+g) + space_w+g + 2*(kw+g) + (kw+g) + (kw+g)
//           = pad + 3*(kw+g) + (9*kw+8*g)+g + 5*(kw+g)
//           = pad + 8*(kw+g) + 9*kw+9*g
//           = pad + 8*(kw+g) + 9*(kw+g)  -- not +g? Let me re-derive cleanly:
//     Substituting avail_w = 18*kw+17*g:
//       space_w = 18*kw+17*g - 9*kw - 9*g = 9*kw+8*g
//     After Alt: x = pad+3*(kw+g).  After Space: pad+3*(kw+g)+space_w+g = pad+12*(kw+g).
//     After AltGr: pad+13*(kw+g).  After RCtrl: pad+14*(kw+g).
//     After Spacer: pad+15*(kw+g).  ← at pad+15*(kw+g). ↓ at pad+16*(kw+g).
//   Row 4: LShift + 10 letters + RShift = 12 keys → x = pad+12*(kw+g).
//     4 Spacers → x(↑) = pad+12*(kw+g)+4*(kw+g) = pad+16*(kw+g)  ✓

pub static KEYS: &[&[PhysKey]] = &[
    // --- Row 0: Function key row (13 keys) ---
    &[
        PhysKey { kw: KW::Std, action: Action::Esc, scancode:  1 },
        PhysKey { kw: KW::Std, action: Action::F1,  scancode: 59 },
        PhysKey { kw: KW::Std, action: Action::F2,  scancode: 60 },
        PhysKey { kw: KW::Std, action: Action::F3,  scancode: 61 },
        PhysKey { kw: KW::Std, action: Action::F4,  scancode: 62 },
        PhysKey { kw: KW::Std, action: Action::F5,  scancode: 63 },
        PhysKey { kw: KW::Std, action: Action::F6,  scancode: 64 },
        PhysKey { kw: KW::Std, action: Action::F7,  scancode: 65 },
        PhysKey { kw: KW::Std, action: Action::F8,  scancode: 66 },
        PhysKey { kw: KW::Std, action: Action::F9,  scancode: 67 },
        PhysKey { kw: KW::Std, action: Action::F10, scancode: 68 },
        PhysKey { kw: KW::Std, action: Action::F11, scancode: 87 },
        PhysKey { kw: KW::Std, action: Action::F12, scancode: 88 },
    ],
    // --- Row 1: Number row + separator + nav cluster (18 slots) ---
    &[
        PhysKey { kw: KW::Std,    action: Action::Regular(0),  scancode:  41 }, // `
        PhysKey { kw: KW::Std,    action: Action::Regular(1),  scancode:   2 }, // 1
        PhysKey { kw: KW::Std,    action: Action::Regular(2),  scancode:   3 }, // 2
        PhysKey { kw: KW::Std,    action: Action::Regular(3),  scancode:   4 }, // 3
        PhysKey { kw: KW::Std,    action: Action::Regular(4),  scancode:   5 }, // 4
        PhysKey { kw: KW::Std,    action: Action::Regular(5),  scancode:   6 }, // 5
        PhysKey { kw: KW::Std,    action: Action::Regular(6),  scancode:   7 }, // 6
        PhysKey { kw: KW::Std,    action: Action::Regular(7),  scancode:   8 }, // 7
        PhysKey { kw: KW::Std,    action: Action::Regular(8),  scancode:   9 }, // 8
        PhysKey { kw: KW::Std,    action: Action::Regular(9),  scancode:  10 }, // 9
        PhysKey { kw: KW::Std,    action: Action::Regular(10), scancode:  11 }, // 0
        PhysKey { kw: KW::Std,    action: Action::Regular(11), scancode:  12 }, // -
        PhysKey { kw: KW::Std,    action: Action::Regular(12), scancode:  13 }, // =
        PhysKey { kw: KW::Std,    action: Action::Backspace,   scancode:  14 }, // Bksp
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode:   0 }, // separator
        PhysKey { kw: KW::Std,    action: Action::Insert,      scancode: 110 }, // Ins
        PhysKey { kw: KW::Std,    action: Action::Home,        scancode: 102 }, // Home
        PhysKey { kw: KW::Std,    action: Action::PageUp,      scancode: 104 }, // PgUp
    ],
    // --- Row 2: Top alpha row + separator + nav cluster (18 slots) ---
    &[
        PhysKey { kw: KW::Std,    action: Action::Tab,          scancode:  15 }, // Tab
        PhysKey { kw: KW::Std,    action: Action::Regular(13),  scancode:  16 }, // q
        PhysKey { kw: KW::Std,    action: Action::Regular(14),  scancode:  17 }, // w
        PhysKey { kw: KW::Std,    action: Action::Regular(15),  scancode:  18 }, // e
        PhysKey { kw: KW::Std,    action: Action::Regular(16),  scancode:  19 }, // r
        PhysKey { kw: KW::Std,    action: Action::Regular(17),  scancode:  20 }, // t
        PhysKey { kw: KW::Std,    action: Action::Regular(18),  scancode:  21 }, // y
        PhysKey { kw: KW::Std,    action: Action::Regular(19),  scancode:  22 }, // u
        PhysKey { kw: KW::Std,    action: Action::Regular(20),  scancode:  23 }, // i
        PhysKey { kw: KW::Std,    action: Action::Regular(21),  scancode:  24 }, // o
        PhysKey { kw: KW::Std,    action: Action::Regular(22),  scancode:  25 }, // p
        PhysKey { kw: KW::Std,    action: Action::Regular(23),  scancode:  26 }, // [
        PhysKey { kw: KW::Std,    action: Action::Regular(24),  scancode:  27 }, // ]
        PhysKey { kw: KW::Std,    action: Action::Regular(25),  scancode:  43 }, // backslash
        PhysKey { kw: KW::Spacer, action: Action::Noop,         scancode:   0 }, // separator
        PhysKey { kw: KW::Std,    action: Action::Delete,       scancode: 111 }, // Del
        PhysKey { kw: KW::Std,    action: Action::End,          scancode: 107 }, // End
        PhysKey { kw: KW::Std,    action: Action::PageDown,     scancode: 109 }, // PgDn
    ],
    // --- Row 3: Home row (13 keys, left-aligned) ---
    &[
        PhysKey { kw: KW::Std,  action: Action::CapsLock,    scancode:  58 }, // Caps
        PhysKey { kw: KW::Std,  action: Action::Regular(26), scancode:  30 }, // a
        PhysKey { kw: KW::Std,  action: Action::Regular(27), scancode:  31 }, // s
        PhysKey { kw: KW::Std,  action: Action::Regular(28), scancode:  32 }, // d
        PhysKey { kw: KW::Std,  action: Action::Regular(29), scancode:  33 }, // f
        PhysKey { kw: KW::Std,  action: Action::Regular(30), scancode:  34 }, // g
        PhysKey { kw: KW::Std,  action: Action::Regular(31), scancode:  35 }, // h
        PhysKey { kw: KW::Std,  action: Action::Regular(32), scancode:  36 }, // j
        PhysKey { kw: KW::Std,  action: Action::Regular(33), scancode:  37 }, // k
        PhysKey { kw: KW::Std,  action: Action::Regular(34), scancode:  38 }, // l
        PhysKey { kw: KW::Std,  action: Action::Regular(35), scancode:  39 }, // ;
        PhysKey { kw: KW::Std,  action: Action::Regular(36), scancode:  40 }, // '
        PhysKey { kw: KW::Std,  action: Action::Enter,       scancode:  28 }, // Enter
    ],
    // --- Row 4: Lower alpha row + 4 Spacers + arrow-up ---
    // 4 Spacer slots push ArrowUp to x = pad+16*(key_w+gap), aligning with ↓ in row 5.
    &[
        PhysKey { kw: KW::Std,    action: Action::LShift,      scancode:  42 }, // LShift
        PhysKey { kw: KW::Std,    action: Action::Regular(37), scancode:  44 }, // z
        PhysKey { kw: KW::Std,    action: Action::Regular(38), scancode:  45 }, // x
        PhysKey { kw: KW::Std,    action: Action::Regular(39), scancode:  46 }, // c
        PhysKey { kw: KW::Std,    action: Action::Regular(40), scancode:  47 }, // v
        PhysKey { kw: KW::Std,    action: Action::Regular(41), scancode:  48 }, // b
        PhysKey { kw: KW::Std,    action: Action::Regular(42), scancode:  49 }, // n
        PhysKey { kw: KW::Std,    action: Action::Regular(43), scancode:  50 }, // m
        PhysKey { kw: KW::Std,    action: Action::Regular(44), scancode:  51 }, // ,
        PhysKey { kw: KW::Std,    action: Action::Regular(45), scancode:  52 }, // .
        PhysKey { kw: KW::Std,    action: Action::Regular(46), scancode:  53 }, // /
        PhysKey { kw: KW::Std,    action: Action::RShift,      scancode:  54 }, // RShift
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode:   0 }, // gap
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode:   0 }, // gap
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode:   0 }, // gap
        PhysKey { kw: KW::Spacer, action: Action::Noop,        scancode:   0 }, // separator
        PhysKey { kw: KW::Std,    action: Action::ArrowUp,     scancode: 103 }, // ↑
    ],
    // --- Row 5: Bottom row + separator + arrow cluster ---
    // Space fills: space_w = avail_w - 9*key_w - 9*gap ≈ 9*key_w + 8*gap
    &[
        PhysKey { kw: KW::Std,    action: Action::Ctrl,       scancode:  29 }, // LCtrl
        PhysKey { kw: KW::Std,    action: Action::Win,        scancode: 125 }, // LMeta/Win
        PhysKey { kw: KW::Std,    action: Action::Alt,        scancode:  56 }, // LAlt
        PhysKey { kw: KW::Space,  action: Action::Space,      scancode:  57 }, // Space
        PhysKey { kw: KW::Std,    action: Action::AltGr,      scancode: 100 }, // RAlt/AltGr
        PhysKey { kw: KW::Std,    action: Action::Ctrl,       scancode:  97 }, // RCtrl
        PhysKey { kw: KW::Spacer, action: Action::Noop,       scancode:   0 }, // separator
        PhysKey { kw: KW::Std,    action: Action::ArrowLeft,  scancode: 105 }, // ←
        PhysKey { kw: KW::Std,    action: Action::ArrowDown,  scancode: 108 }, // ↓
        PhysKey { kw: KW::Std,    action: Action::ArrowRight, scancode: 106 }, // →
    ],
];

// =============================================================================
// Language layouts
// =============================================================================

/// One substitutable key slot: label shown on the button and string inserted.
pub struct LayoutKey {
    /// Text shown on the button face (unshifted value).
    pub label:   &'static str,
    /// String inserted when the key is activated without Shift / CapsLock.
    pub insert:  &'static str,
    /// Shifted character shown in the top half of the button face and inserted
    /// when Shift is held.  Empty for letter keys (they use `insert.to_uppercase()`
    /// instead, and CapsLock also applies to them).
    pub shifted: &'static str,
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
        LayoutKey { label: "`",  insert: "`",  shifted: "~"  }, // 0
        LayoutKey { label: "1",  insert: "1",  shifted: "!"  }, // 1
        LayoutKey { label: "2",  insert: "2",  shifted: "@"  }, // 2
        LayoutKey { label: "3",  insert: "3",  shifted: "#"  }, // 3
        LayoutKey { label: "4",  insert: "4",  shifted: "$"  }, // 4
        LayoutKey { label: "5",  insert: "5",  shifted: "%"  }, // 5
        LayoutKey { label: "6",  insert: "6",  shifted: "^"  }, // 6
        LayoutKey { label: "7",  insert: "7",  shifted: "&"  }, // 7
        LayoutKey { label: "8",  insert: "8",  shifted: "*"  }, // 8
        LayoutKey { label: "9",  insert: "9",  shifted: "("  }, // 9
        LayoutKey { label: "0",  insert: "0",  shifted: ")"  }, // 10
        LayoutKey { label: "-",  insert: "-",  shifted: "_"  }, // 11
        LayoutKey { label: "=",  insert: "=",  shifted: "+"  }, // 12
        // slots 13-22: top alpha row (q-p) – letter keys, no shifted display
        LayoutKey { label: "q",  insert: "q",  shifted: ""   }, // 13
        LayoutKey { label: "w",  insert: "w",  shifted: ""   }, // 14
        LayoutKey { label: "e",  insert: "e",  shifted: ""   }, // 15
        LayoutKey { label: "r",  insert: "r",  shifted: ""   }, // 16
        LayoutKey { label: "t",  insert: "t",  shifted: ""   }, // 17
        LayoutKey { label: "y",  insert: "y",  shifted: ""   }, // 18
        LayoutKey { label: "u",  insert: "u",  shifted: ""   }, // 19
        LayoutKey { label: "i",  insert: "i",  shifted: ""   }, // 20
        LayoutKey { label: "o",  insert: "o",  shifted: ""   }, // 21
        LayoutKey { label: "p",  insert: "p",  shifted: ""   }, // 22
        // slots 23-25: top-row punctuation
        LayoutKey { label: "[",  insert: "[",  shifted: "{"  }, // 23
        LayoutKey { label: "]",  insert: "]",  shifted: "}"  }, // 24
        LayoutKey { label: "\\", insert: "\\", shifted: "|"  }, // 25
        // slots 26-34: home alpha row (a-l) – letter keys
        LayoutKey { label: "a",  insert: "a",  shifted: ""   }, // 26
        LayoutKey { label: "s",  insert: "s",  shifted: ""   }, // 27
        LayoutKey { label: "d",  insert: "d",  shifted: ""   }, // 28
        LayoutKey { label: "f",  insert: "f",  shifted: ""   }, // 29
        LayoutKey { label: "g",  insert: "g",  shifted: ""   }, // 30
        LayoutKey { label: "h",  insert: "h",  shifted: ""   }, // 31
        LayoutKey { label: "j",  insert: "j",  shifted: ""   }, // 32
        LayoutKey { label: "k",  insert: "k",  shifted: ""   }, // 33
        LayoutKey { label: "l",  insert: "l",  shifted: ""   }, // 34
        // slots 35-36: home-row punctuation
        LayoutKey { label: ";",  insert: ";",  shifted: ":"  }, // 35
        LayoutKey { label: "'",  insert: "'",  shifted: "\"" }, // 36
        // slots 37-43: lower alpha row (z-m) – letter keys
        LayoutKey { label: "z",  insert: "z",  shifted: ""   }, // 37
        LayoutKey { label: "x",  insert: "x",  shifted: ""   }, // 38
        LayoutKey { label: "c",  insert: "c",  shifted: ""   }, // 39
        LayoutKey { label: "v",  insert: "v",  shifted: ""   }, // 40
        LayoutKey { label: "b",  insert: "b",  shifted: ""   }, // 41
        LayoutKey { label: "n",  insert: "n",  shifted: ""   }, // 42
        LayoutKey { label: "m",  insert: "m",  shifted: ""   }, // 43
        // slots 44-46: lower-row punctuation
        LayoutKey { label: ",",  insert: ",",  shifted: "<"  }, // 44
        LayoutKey { label: ".",  insert: ".",  shifted: ">"  }, // 45
        LayoutKey { label: "/",  insert: "/",  shifted: "?"  }, // 46
    ],
};

// ---------------------------------------------------------------------------
// Ukrainian (QWERTY-UA)
//
// All non-ASCII runtime values use \u{XXXX} Rust escape sequences; the source
// file contains only ASCII bytes.
//
// Slot -> Ukrainian character:
//   0  ` -> \u{02BC} MODIFIER LETTER APOSTROPHE (Ukrainian apostrophe)
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
//  46  / -> / (unchanged)
// ---------------------------------------------------------------------------
pub static UA: LayoutDef = LayoutDef {
    name: "UA",
    keys: &[
        // slots 0-12: number row
        // The grave key carries the Ukrainian apostrophe; its physical Shift value is ~.
        LayoutKey { label: "\u{02BC}", insert: "\u{02BC}", shifted: "~"  }, // 0  ` -> apostrophe
        LayoutKey { label: "1",        insert: "1",        shifted: "!"  }, // 1
        LayoutKey { label: "2",        insert: "2",        shifted: "@"  }, // 2
        LayoutKey { label: "3",        insert: "3",        shifted: "#"  }, // 3
        LayoutKey { label: "4",        insert: "4",        shifted: "$"  }, // 4
        LayoutKey { label: "5",        insert: "5",        shifted: "%"  }, // 5
        LayoutKey { label: "6",        insert: "6",        shifted: "^"  }, // 6
        LayoutKey { label: "7",        insert: "7",        shifted: "&"  }, // 7
        LayoutKey { label: "8",        insert: "8",        shifted: "*"  }, // 8
        LayoutKey { label: "9",        insert: "9",        shifted: "("  }, // 9
        LayoutKey { label: "0",        insert: "0",        shifted: ")"  }, // 10
        LayoutKey { label: "-",        insert: "-",        shifted: "_"  }, // 11
        LayoutKey { label: "=",        insert: "=",        shifted: "+"  }, // 12
        // slots 13-22: top alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label: "\u{0439}", insert: "\u{0439}", shifted: ""   }, // 13  q -> J
        LayoutKey { label: "\u{0446}", insert: "\u{0446}", shifted: ""   }, // 14  w -> Ts
        LayoutKey { label: "\u{0443}", insert: "\u{0443}", shifted: ""   }, // 15  e -> U
        LayoutKey { label: "\u{043A}", insert: "\u{043A}", shifted: ""   }, // 16  r -> K
        LayoutKey { label: "\u{0435}", insert: "\u{0435}", shifted: ""   }, // 17  t -> Ye
        LayoutKey { label: "\u{043D}", insert: "\u{043D}", shifted: ""   }, // 18  y -> N
        LayoutKey { label: "\u{0433}", insert: "\u{0433}", shifted: ""   }, // 19  u -> G
        LayoutKey { label: "\u{0448}", insert: "\u{0448}", shifted: ""   }, // 20  i -> Sh
        LayoutKey { label: "\u{0449}", insert: "\u{0449}", shifted: ""   }, // 21  o -> Shch
        LayoutKey { label: "\u{0437}", insert: "\u{0437}", shifted: ""   }, // 22  p -> Z
        // slots 23-25: top-row punctuation (same physical Shift symbols as US)
        LayoutKey { label: "\u{0445}", insert: "\u{0445}", shifted: "{"  }, // 23  [ -> Kh
        LayoutKey { label: "\u{0457}", insert: "\u{0457}", shifted: "}"  }, // 24  ] -> Yi
        LayoutKey { label: "\\",       insert: "\\",       shifted: "|"  }, // 25  \ -> same
        // slots 26-34: home alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label: "\u{0444}", insert: "\u{0444}", shifted: ""   }, // 26  a -> F
        LayoutKey { label: "\u{0456}", insert: "\u{0456}", shifted: ""   }, // 27  s -> I
        LayoutKey { label: "\u{0432}", insert: "\u{0432}", shifted: ""   }, // 28  d -> V
        LayoutKey { label: "\u{0430}", insert: "\u{0430}", shifted: ""   }, // 29  f -> A
        LayoutKey { label: "\u{043F}", insert: "\u{043F}", shifted: ""   }, // 30  g -> P
        LayoutKey { label: "\u{0440}", insert: "\u{0440}", shifted: ""   }, // 31  h -> R
        LayoutKey { label: "\u{043E}", insert: "\u{043E}", shifted: ""   }, // 32  j -> O
        LayoutKey { label: "\u{043B}", insert: "\u{043B}", shifted: ""   }, // 33  k -> L
        LayoutKey { label: "\u{0434}", insert: "\u{0434}", shifted: ""   }, // 34  l -> D
        // slots 35-36: home-row punctuation
        LayoutKey { label: "\u{0436}", insert: "\u{0436}", shifted: ":"  }, // 35  ; -> Zh
        LayoutKey { label: "\u{0454}", insert: "\u{0454}", shifted: "\"" }, // 36  ' -> Ye
        // slots 37-43: lower alpha row (Cyrillic letters) – no shifted display
        LayoutKey { label: "\u{044F}", insert: "\u{044F}", shifted: ""   }, // 37  z -> Ya
        LayoutKey { label: "\u{0447}", insert: "\u{0447}", shifted: ""   }, // 38  x -> Ch
        LayoutKey { label: "\u{0441}", insert: "\u{0441}", shifted: ""   }, // 39  c -> S
        LayoutKey { label: "\u{043C}", insert: "\u{043C}", shifted: ""   }, // 40  v -> M
        LayoutKey { label: "\u{0438}", insert: "\u{0438}", shifted: ""   }, // 41  b -> I
        LayoutKey { label: "\u{0442}", insert: "\u{0442}", shifted: ""   }, // 42  n -> T
        LayoutKey { label: "\u{044C}", insert: "\u{044C}", shifted: ""   }, // 43  m -> soft sign
        // slots 44-46: lower-row punctuation
        LayoutKey { label: "\u{0431}", insert: "\u{0431}", shifted: "<"  }, // 44  , -> B
        LayoutKey { label: "\u{044E}", insert: "\u{044E}", shifted: ">"  }, // 45  . -> Yu
        LayoutKey { label: "/",        insert: "/",        shifted: "?"  }, // 46  / -> same
    ],
};

/// All available layouts.
///
/// To add a new language: define a new LayoutDef constant (see UA above) and
/// append a reference to it here.  The toggle button appears automatically.
pub static LAYOUTS: &[&LayoutDef] = &[&US, &UA];
