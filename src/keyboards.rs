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

/// Total column count for the ortholinear grid (widest row = 17).
pub const GRID_COLS: u16 = 17;
/// Column span for the space bar within the 17-column grid.
pub const SPACE_COLS: u16 = 6;

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
            | Action::Win
            | Action::Alt
            | Action::AltGr
    )
}

/// Returns true if this modifier is sticky (auto-releases after next regular key).
#[allow(dead_code)]
pub fn is_sticky(action: Action) -> bool {
    matches!(
        action,
        Action::LShift | Action::RShift | Action::Ctrl | Action::Win | Action::Alt | Action::AltGr
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
// Ortholinear grid - 18 uniform columns, 6 rows.
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
        PhysKey { kw: KW::Std,    action: Action::ArrowUp,     scancode: 0x67 }, // up arrow
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
        PhysKey { kw: KW::Std,    action: Action::ArrowLeft,  scancode: 0x69 }, // left arrow
        PhysKey { kw: KW::Std,    action: Action::ArrowDown,  scancode: 0x6c }, // down arrow
        PhysKey { kw: KW::Std,    action: Action::ArrowRight, scancode: 0x6a }, // right arrow
    ],
];

// =============================================================================
// Language layouts
// =============================================================================

use serde::Deserialize;
use std::sync::OnceLock;

/// One substitutable key slot: display labels and strings to insert.
#[derive(Deserialize, Clone)]
pub struct LayoutKey {
    /// Text shown on the button face (unshifted state).
    pub label_unshifted:  String,
    /// String inserted when the key is activated without Shift / CapsLock.
    pub insert_unshifted: String,
    /// Text shown in the top half of the button face (shifted state).
    /// Empty for letter keys (they use `insert_unshifted.to_uppercase()` instead,
    /// and CapsLock also applies to them).
    pub label_shifted:    String,
    /// String inserted when Shift is held.
    /// Empty for letter keys (uppercase is computed from `insert_unshifted`).
    pub insert_shifted:   String,
}

/// A named keyboard layout.
pub struct LayoutDef {
    pub name: String,
    pub keys: Vec<LayoutKey>,
}

/// TOML file format for keymap files.
#[derive(Deserialize)]
struct KeymapFileToml {
    name: String,
    keys: Vec<LayoutKey>,
}

static ACTIVE_LAYOUTS: OnceLock<Vec<LayoutDef>> = OnceLock::new();

/// Store the active layouts (call once from main before showing the UI).
pub fn set_layouts(layouts: Vec<LayoutDef>) {
    let _ = ACTIVE_LAYOUTS.set(layouts);
}

/// Return the active layouts slice.  Returns `&[]` if not yet initialised.
pub fn get_layouts() -> &'static [LayoutDef] {
    ACTIVE_LAYOUTS.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// Returns the built-in default switch scancode for a known keymap name.
/// [modifier_byte, hid_keycode]: Ctrl+Shift+1 for "us", Ctrl+Shift+4 for "ua".
pub fn default_switch_scancode_for(name: &str) -> Vec<u8> {
    match name {
        "us" => vec![0x03, 0x1e],  // Ctrl+Shift+1
        "ua" => vec![0x03, 0x21],  // Ctrl+Shift+4
        _    => vec![],
    }
}

// ---------------------------------------------------------------------------
// Built-in fallback layout definitions
// ---------------------------------------------------------------------------

fn make_layout_key(lu: &str, iu: &str, ls: &str, is: &str) -> LayoutKey {
    LayoutKey {
        label_unshifted:  lu.to_string(),
        insert_unshifted: iu.to_string(),
        label_shifted:    ls.to_string(),
        insert_shifted:   is.to_string(),
    }
}

fn builtin_us_layout() -> LayoutDef {
    LayoutDef {
        name: "US".to_string(),
        keys: vec![
            // slots 0-12: number row
            make_layout_key("`",  "`",  "~",  "~"),   // 0
            make_layout_key("1",  "1",  "!",  "!"),   // 1
            make_layout_key("2",  "2",  "@@", "@"),   // 2
            make_layout_key("3",  "3",  "#",  "#"),   // 3
            make_layout_key("4",  "4",  "$",  "$"),   // 4
            make_layout_key("5",  "5",  "%",  "%"),   // 5
            make_layout_key("6",  "6",  "^",  "^"),   // 6
            make_layout_key("7",  "7",  "&&", "&"),   // 7
            make_layout_key("8",  "8",  "*",  "*"),   // 8
            make_layout_key("9",  "9",  "(",  "("),   // 9
            make_layout_key("0",  "0",  ")",  ")"),   // 10
            make_layout_key("-",  "-",  "_",  "_"),   // 11
            make_layout_key("=",  "=",  "+",  "+"),   // 12
            // slots 13-22: top alpha row (q-p)
            make_layout_key("q",  "q",  "",   ""),    // 13
            make_layout_key("w",  "w",  "",   ""),    // 14
            make_layout_key("e",  "e",  "",   ""),    // 15
            make_layout_key("r",  "r",  "",   ""),    // 16
            make_layout_key("t",  "t",  "",   ""),    // 17
            make_layout_key("y",  "y",  "",   ""),    // 18
            make_layout_key("u",  "u",  "",   ""),    // 19
            make_layout_key("i",  "i",  "",   ""),    // 20
            make_layout_key("o",  "o",  "",   ""),    // 21
            make_layout_key("p",  "p",  "",   ""),    // 22
            // slots 23-25: top-row punctuation
            make_layout_key("[",  "[",  "{",  "{"),   // 23
            make_layout_key("]",  "]",  "}",  "}"),   // 24
            make_layout_key("\\", "\\", "|",  "|"),   // 25
            // slots 26-34: home alpha row (a-l)
            make_layout_key("a",  "a",  "",   ""),    // 26
            make_layout_key("s",  "s",  "",   ""),    // 27
            make_layout_key("d",  "d",  "",   ""),    // 28
            make_layout_key("f",  "f",  "",   ""),    // 29
            make_layout_key("g",  "g",  "",   ""),    // 30
            make_layout_key("h",  "h",  "",   ""),    // 31
            make_layout_key("j",  "j",  "",   ""),    // 32
            make_layout_key("k",  "k",  "",   ""),    // 33
            make_layout_key("l",  "l",  "",   ""),    // 34
            // slots 35-36: home-row punctuation
            make_layout_key(";",  ";",  ":",  ":"),   // 35
            make_layout_key("'",  "'",  "\"", "\""),  // 36
            // slots 37-43: lower alpha row (z-m)
            make_layout_key("z",  "z",  "",   ""),    // 37
            make_layout_key("x",  "x",  "",   ""),    // 38
            make_layout_key("c",  "c",  "",   ""),    // 39
            make_layout_key("v",  "v",  "",   ""),    // 40
            make_layout_key("b",  "b",  "",   ""),    // 41
            make_layout_key("n",  "n",  "",   ""),    // 42
            make_layout_key("m",  "m",  "",   ""),    // 43
            // slots 44-46: lower-row punctuation
            make_layout_key(",",  ",",  "<",  "<"),   // 44
            make_layout_key(".",  ".",  ">",  ">"),   // 45
            make_layout_key("/",  "/",  "?",  "?"),   // 46
        ],
    }
}

fn builtin_ua_layout() -> LayoutDef {
    LayoutDef {
        name: "UA".to_string(),
        keys: vec![
            // slots 0-12: number row
            make_layout_key("\u{0027}", "\u{0027}", "\u{20b4}", "\u{20b4}"),  // 0  ` -> apostrophe
            make_layout_key("1",        "1",        "!",        "!"),         // 1
            make_layout_key("2",        "2",        "\"",       "\""),        // 2
            make_layout_key("3",        "3",        "\u{2116}", "\u{2116}"),  // 3  numero sign
            make_layout_key("4",        "4",        ";",        ";"),         // 4
            make_layout_key("5",        "5",        "%",        "%"),         // 5
            make_layout_key("6",        "6",        ":",        ":"),         // 6
            make_layout_key("7",        "7",        "?",        "?"),         // 7
            make_layout_key("8",        "8",        "*",        "*"),         // 8
            make_layout_key("9",        "9",        "(",        "("),         // 9
            make_layout_key("0",        "0",        ")",        ")"),         // 10
            make_layout_key("-",        "-",        "_",        "_"),         // 11
            make_layout_key("=",        "=",        "+",        "+"),         // 12
            // slots 13-22: top alpha row (Cyrillic)
            make_layout_key("\u{0439}", "\u{0439}", "", ""),  // 13  q -> J
            make_layout_key("\u{0446}", "\u{0446}", "", ""),  // 14  w -> Ts
            make_layout_key("\u{0443}", "\u{0443}", "", ""),  // 15  e -> U
            make_layout_key("\u{043A}", "\u{043A}", "", ""),  // 16  r -> K
            make_layout_key("\u{0435}", "\u{0435}", "", ""),  // 17  t -> Ye
            make_layout_key("\u{043D}", "\u{043D}", "", ""),  // 18  y -> N
            make_layout_key("\u{0433}", "\u{0433}", "", ""),  // 19  u -> G
            make_layout_key("\u{0448}", "\u{0448}", "", ""),  // 20  i -> Sh
            make_layout_key("\u{0449}", "\u{0449}", "", ""),  // 21  o -> Shch
            make_layout_key("\u{0437}", "\u{0437}", "", ""),  // 22  p -> Z
            // slots 23-25
            make_layout_key("\u{0445}", "\u{0445}", "", ""),  // 23  [ -> Kh
            make_layout_key("\u{0457}", "\u{0457}", "", ""),  // 24  ] -> Yi
            make_layout_key("\\",       "\\",       "|", "|"), // 25  \
            // slots 26-34: home alpha row (Cyrillic)
            make_layout_key("\u{0444}", "\u{0444}", "", ""),  // 26  a -> F
            make_layout_key("\u{0456}", "\u{0456}", "", ""),  // 27  s -> I
            make_layout_key("\u{0432}", "\u{0432}", "", ""),  // 28  d -> V
            make_layout_key("\u{0430}", "\u{0430}", "", ""),  // 29  f -> A
            make_layout_key("\u{043F}", "\u{043F}", "", ""),  // 30  g -> P
            make_layout_key("\u{0440}", "\u{0440}", "", ""),  // 31  h -> R
            make_layout_key("\u{043E}", "\u{043E}", "", ""),  // 32  j -> O
            make_layout_key("\u{043B}", "\u{043B}", "", ""),  // 33  k -> L
            make_layout_key("\u{0434}", "\u{0434}", "", ""),  // 34  l -> D
            // slots 35-36
            make_layout_key("\u{0436}", "\u{0436}", "", ""),  // 35  ; -> Zh
            make_layout_key("\u{0454}", "\u{0454}", "", ""),  // 36  ' -> Ye
            // slots 37-43: lower alpha row (Cyrillic)
            make_layout_key("\u{044F}", "\u{044F}", "", ""),  // 37  z -> Ya
            make_layout_key("\u{0447}", "\u{0447}", "", ""),  // 38  x -> Ch
            make_layout_key("\u{0441}", "\u{0441}", "", ""),  // 39  c -> S
            make_layout_key("\u{043C}", "\u{043C}", "", ""),  // 40  v -> M
            make_layout_key("\u{0438}", "\u{0438}", "", ""),  // 41  b -> I
            make_layout_key("\u{0442}", "\u{0442}", "", ""),  // 42  n -> T
            make_layout_key("\u{044C}", "\u{044C}", "", ""),  // 43  m -> soft sign
            // slots 44-46
            make_layout_key("\u{0431}", "\u{0431}", "", ""),  // 44  , -> B
            make_layout_key("\u{044E}", "\u{044E}", "", ""),  // 45  . -> Yu
            make_layout_key(".",        ".",        ",", ","), // 46  / -> FULL STOP
        ],
    }
}

pub fn builtin_layout(name: &str) -> Option<LayoutDef> {
    match name {
        "us" => Some(builtin_us_layout()),
        "ua" => Some(builtin_ua_layout()),
        _    => None,
    }
}

// ---------------------------------------------------------------------------
// TOML file loading
// ---------------------------------------------------------------------------

/// Load a keymap TOML file from `config_dir`.
/// Looks for `keymap_{name}.toml`.
pub fn load_layout_from_toml(config_dir: &str, name: &str) -> Option<LayoutDef> {
    let dir = std::path::Path::new(config_dir);
    let filename = format!("keymap_{}.toml", name);
    let path = dir.join(&filename);
    let content = std::fs::read_to_string(&path).ok()?;
    let toml_data: KeymapFileToml = match toml::from_str(&content) {
        Ok(d)  => d,
        Err(e) => {
            eprintln!("[keymap] failed to parse {}: {}", path.display(), e);
            return None;
        }
    };
    Some(LayoutDef { name: toml_data.name, keys: toml_data.keys })
}

/// Load active layouts from TOML files (falling back to built-ins).
pub fn load_active_layouts(active_keymaps: &[String], config_dir: &str) -> Vec<LayoutDef> {
    let mut layouts = Vec::new();
    for name in active_keymaps {
        if let Some(layout) = load_layout_from_toml(config_dir, name) {
            layouts.push(layout);
        } else if let Some(layout) = builtin_layout(name) {
            layouts.push(layout);
        } else {
            eprintln!("[keymap] no definition found for keymap {:?}, skipping", name);
        }
    }
    layouts
}

