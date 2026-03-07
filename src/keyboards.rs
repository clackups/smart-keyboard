// src/keyboards.rs
//
// Physical keyboard structure and language layout definitions.
// All source bytes are ASCII; non-ASCII runtime values use \u{XXXX} escapes.

// =============================================================================
// Key-width kinds
// =============================================================================

/// Semantic width category for each physical key.
/// Pixel values are computed in main.rs from the screen width so every row
/// fills the available area exactly.
#[derive(Clone, Copy)]
pub enum KW {
    Std,    // 1x    -- regular alphanumeric / symbol key
    Tab,    // ~1.5x -- Tab key
    BSlash, // fills remainder of top-alpha row (\)
    Caps,   // ~1.75x -- Caps Lock
    Enter,  // fills remainder of home row
    Bksp,   // fills remainder of number row
    LShift, // ~2.25x -- left Shift
    RShift, // fills remainder of lower-alpha row
    Mod,    // ~1.5x -- Ctrl / Win / Alt / AltGr
    Space,  // fills remainder of bottom row
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
        Action::Space | Action::Regular(_) => "",
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
        Action::Regular(_) => "",
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
//   KEY_GRAVE=41  KEY_1..KEY_0=2..11  KEY_MINUS=12  KEY_EQUAL=13
//   KEY_BACKSPACE=14  KEY_TAB=15
//   KEY_Q..KEY_P=16..25  KEY_LEFTBRACE=26  KEY_RIGHTBRACE=27  KEY_BACKSLASH=43
//   KEY_CAPSLOCK=58
//   KEY_A..KEY_L=30..38  KEY_SEMICOLON=39  KEY_APOSTROPHE=40  KEY_ENTER=28
//   KEY_LEFTSHIFT=42  KEY_Z..KEY_SLASH=44..53  KEY_RIGHTSHIFT=54
//   KEY_LEFTCTRL=29  KEY_LEFTMETA=125  KEY_LEFTALT=56
//   KEY_SPACE=57  KEY_RIGHTALT=100  KEY_RIGHTCTRL=97

pub static KEYS: &[&[PhysKey]] = &[
    // --- Number row (13 Std + Bksp) ---
    &[
        PhysKey { kw: KW::Std,  action: Action::Regular(0),  scancode: 41  }, // `
        PhysKey { kw: KW::Std,  action: Action::Regular(1),  scancode: 2   }, // 1
        PhysKey { kw: KW::Std,  action: Action::Regular(2),  scancode: 3   }, // 2
        PhysKey { kw: KW::Std,  action: Action::Regular(3),  scancode: 4   }, // 3
        PhysKey { kw: KW::Std,  action: Action::Regular(4),  scancode: 5   }, // 4
        PhysKey { kw: KW::Std,  action: Action::Regular(5),  scancode: 6   }, // 5
        PhysKey { kw: KW::Std,  action: Action::Regular(6),  scancode: 7   }, // 6
        PhysKey { kw: KW::Std,  action: Action::Regular(7),  scancode: 8   }, // 7
        PhysKey { kw: KW::Std,  action: Action::Regular(8),  scancode: 9   }, // 8
        PhysKey { kw: KW::Std,  action: Action::Regular(9),  scancode: 10  }, // 9
        PhysKey { kw: KW::Std,  action: Action::Regular(10), scancode: 11  }, // 0
        PhysKey { kw: KW::Std,  action: Action::Regular(11), scancode: 12  }, // -
        PhysKey { kw: KW::Std,  action: Action::Regular(12), scancode: 13  }, // =
        PhysKey { kw: KW::Bksp, action: Action::Backspace,   scancode: 14  }, // Backspace
    ],
    // --- Top alpha row (Tab + 12 Std + BSlash) ---
    &[
        PhysKey { kw: KW::Tab,    action: Action::Tab,          scancode: 15  }, // Tab
        PhysKey { kw: KW::Std,    action: Action::Regular(13),  scancode: 16  }, // q
        PhysKey { kw: KW::Std,    action: Action::Regular(14),  scancode: 17  }, // w
        PhysKey { kw: KW::Std,    action: Action::Regular(15),  scancode: 18  }, // e
        PhysKey { kw: KW::Std,    action: Action::Regular(16),  scancode: 19  }, // r
        PhysKey { kw: KW::Std,    action: Action::Regular(17),  scancode: 20  }, // t
        PhysKey { kw: KW::Std,    action: Action::Regular(18),  scancode: 21  }, // y
        PhysKey { kw: KW::Std,    action: Action::Regular(19),  scancode: 22  }, // u
        PhysKey { kw: KW::Std,    action: Action::Regular(20),  scancode: 23  }, // i
        PhysKey { kw: KW::Std,    action: Action::Regular(21),  scancode: 24  }, // o
        PhysKey { kw: KW::Std,    action: Action::Regular(22),  scancode: 25  }, // p
        PhysKey { kw: KW::Std,    action: Action::Regular(23),  scancode: 26  }, // [
        PhysKey { kw: KW::Std,    action: Action::Regular(24),  scancode: 27  }, // ]
        PhysKey { kw: KW::BSlash, action: Action::Regular(25),  scancode: 43  }, // backslash
    ],
    // --- Home row (Caps + 11 Std + Enter) ---
    &[
        PhysKey { kw: KW::Caps,  action: Action::CapsLock,    scancode: 58  }, // Caps
        PhysKey { kw: KW::Std,   action: Action::Regular(26), scancode: 30  }, // a
        PhysKey { kw: KW::Std,   action: Action::Regular(27), scancode: 31  }, // s
        PhysKey { kw: KW::Std,   action: Action::Regular(28), scancode: 32  }, // d
        PhysKey { kw: KW::Std,   action: Action::Regular(29), scancode: 33  }, // f
        PhysKey { kw: KW::Std,   action: Action::Regular(30), scancode: 34  }, // g
        PhysKey { kw: KW::Std,   action: Action::Regular(31), scancode: 35  }, // h
        PhysKey { kw: KW::Std,   action: Action::Regular(32), scancode: 36  }, // j
        PhysKey { kw: KW::Std,   action: Action::Regular(33), scancode: 37  }, // k
        PhysKey { kw: KW::Std,   action: Action::Regular(34), scancode: 38  }, // l
        PhysKey { kw: KW::Std,   action: Action::Regular(35), scancode: 39  }, // ;
        PhysKey { kw: KW::Std,   action: Action::Regular(36), scancode: 40  }, // '
        PhysKey { kw: KW::Enter, action: Action::Enter,        scancode: 28  }, // Enter
    ],
    // --- Lower alpha row (LShift + 10 Std + RShift) ---
    &[
        PhysKey { kw: KW::LShift, action: Action::LShift,      scancode: 42  }, // LShift
        PhysKey { kw: KW::Std,    action: Action::Regular(37),  scancode: 44  }, // z
        PhysKey { kw: KW::Std,    action: Action::Regular(38),  scancode: 45  }, // x
        PhysKey { kw: KW::Std,    action: Action::Regular(39),  scancode: 46  }, // c
        PhysKey { kw: KW::Std,    action: Action::Regular(40),  scancode: 47  }, // v
        PhysKey { kw: KW::Std,    action: Action::Regular(41),  scancode: 48  }, // b
        PhysKey { kw: KW::Std,    action: Action::Regular(42),  scancode: 49  }, // n
        PhysKey { kw: KW::Std,    action: Action::Regular(43),  scancode: 50  }, // m
        PhysKey { kw: KW::Std,    action: Action::Regular(44),  scancode: 51  }, // ,
        PhysKey { kw: KW::Std,    action: Action::Regular(45),  scancode: 52  }, // .
        PhysKey { kw: KW::Std,    action: Action::Regular(46),  scancode: 53  }, // /
        PhysKey { kw: KW::RShift, action: Action::RShift,       scancode: 54  }, // RShift
    ],
    // --- Bottom row (Ctrl + Win + Alt + Space + AltGr + Ctrl, 5 gaps) ---
    // Menu key removed; Space fills the extra width automatically.
    &[
        PhysKey { kw: KW::Mod,   action: Action::Ctrl,  scancode: 29  }, // LCtrl
        PhysKey { kw: KW::Mod,   action: Action::Win,   scancode: 125 }, // LMeta/Win
        PhysKey { kw: KW::Mod,   action: Action::Alt,   scancode: 56  }, // LAlt
        PhysKey { kw: KW::Space, action: Action::Space, scancode: 57  }, // Space
        PhysKey { kw: KW::Mod,   action: Action::AltGr, scancode: 100 }, // RAlt/AltGr
        PhysKey { kw: KW::Mod,   action: Action::Ctrl,  scancode: 97  }, // RCtrl
    ],
];

// =============================================================================
// Language layouts
// =============================================================================

/// One substitutable key slot: label shown on the button and string inserted.
pub struct LayoutKey {
    /// Text shown on the button face.
    pub label:  &'static str,
    /// String appended to the text buffer when the key is activated.
    pub insert: &'static str,
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
        // slots 0-12: number row
        LayoutKey { label: "`",  insert: "`"  }, // 0
        LayoutKey { label: "1",  insert: "1"  }, // 1
        LayoutKey { label: "2",  insert: "2"  }, // 2
        LayoutKey { label: "3",  insert: "3"  }, // 3
        LayoutKey { label: "4",  insert: "4"  }, // 4
        LayoutKey { label: "5",  insert: "5"  }, // 5
        LayoutKey { label: "6",  insert: "6"  }, // 6
        LayoutKey { label: "7",  insert: "7"  }, // 7
        LayoutKey { label: "8",  insert: "8"  }, // 8
        LayoutKey { label: "9",  insert: "9"  }, // 9
        LayoutKey { label: "0",  insert: "0"  }, // 10
        LayoutKey { label: "-",  insert: "-"  }, // 11
        LayoutKey { label: "=",  insert: "="  }, // 12
        // slots 13-25: top alpha row
        LayoutKey { label: "q",  insert: "q"  }, // 13
        LayoutKey { label: "w",  insert: "w"  }, // 14
        LayoutKey { label: "e",  insert: "e"  }, // 15
        LayoutKey { label: "r",  insert: "r"  }, // 16
        LayoutKey { label: "t",  insert: "t"  }, // 17
        LayoutKey { label: "y",  insert: "y"  }, // 18
        LayoutKey { label: "u",  insert: "u"  }, // 19
        LayoutKey { label: "i",  insert: "i"  }, // 20
        LayoutKey { label: "o",  insert: "o"  }, // 21
        LayoutKey { label: "p",  insert: "p"  }, // 22
        LayoutKey { label: "[",  insert: "["  }, // 23
        LayoutKey { label: "]",  insert: "]"  }, // 24
        LayoutKey { label: "\\", insert: "\\" }, // 25
        // slots 26-36: home row
        LayoutKey { label: "a",  insert: "a"  }, // 26
        LayoutKey { label: "s",  insert: "s"  }, // 27
        LayoutKey { label: "d",  insert: "d"  }, // 28
        LayoutKey { label: "f",  insert: "f"  }, // 29
        LayoutKey { label: "g",  insert: "g"  }, // 30
        LayoutKey { label: "h",  insert: "h"  }, // 31
        LayoutKey { label: "j",  insert: "j"  }, // 32
        LayoutKey { label: "k",  insert: "k"  }, // 33
        LayoutKey { label: "l",  insert: "l"  }, // 34
        LayoutKey { label: ";",  insert: ";"  }, // 35
        LayoutKey { label: "'",  insert: "'"  }, // 36
        // slots 37-46: lower alpha row
        LayoutKey { label: "z",  insert: "z"  }, // 37
        LayoutKey { label: "x",  insert: "x"  }, // 38
        LayoutKey { label: "c",  insert: "c"  }, // 39
        LayoutKey { label: "v",  insert: "v"  }, // 40
        LayoutKey { label: "b",  insert: "b"  }, // 41
        LayoutKey { label: "n",  insert: "n"  }, // 42
        LayoutKey { label: "m",  insert: "m"  }, // 43
        LayoutKey { label: ",",  insert: ","  }, // 44
        LayoutKey { label: ".",  insert: "."  }, // 45
        LayoutKey { label: "/",  insert: "/"  }, // 46
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
        LayoutKey { label: "\u{02BC}", insert: "\u{02BC}" }, // 0  ` -> apostrophe
        LayoutKey { label: "1",        insert: "1"        }, // 1
        LayoutKey { label: "2",        insert: "2"        }, // 2
        LayoutKey { label: "3",        insert: "3"        }, // 3
        LayoutKey { label: "4",        insert: "4"        }, // 4
        LayoutKey { label: "5",        insert: "5"        }, // 5
        LayoutKey { label: "6",        insert: "6"        }, // 6
        LayoutKey { label: "7",        insert: "7"        }, // 7
        LayoutKey { label: "8",        insert: "8"        }, // 8
        LayoutKey { label: "9",        insert: "9"        }, // 9
        LayoutKey { label: "0",        insert: "0"        }, // 10
        LayoutKey { label: "-",        insert: "-"        }, // 11
        LayoutKey { label: "=",        insert: "="        }, // 12
        // slots 13-25: top alpha row
        LayoutKey { label: "\u{0439}", insert: "\u{0439}" }, // 13  q -> J
        LayoutKey { label: "\u{0446}", insert: "\u{0446}" }, // 14  w -> Ts
        LayoutKey { label: "\u{0443}", insert: "\u{0443}" }, // 15  e -> U
        LayoutKey { label: "\u{043A}", insert: "\u{043A}" }, // 16  r -> K
        LayoutKey { label: "\u{0435}", insert: "\u{0435}" }, // 17  t -> Ye
        LayoutKey { label: "\u{043D}", insert: "\u{043D}" }, // 18  y -> N
        LayoutKey { label: "\u{0433}", insert: "\u{0433}" }, // 19  u -> G
        LayoutKey { label: "\u{0448}", insert: "\u{0448}" }, // 20  i -> Sh
        LayoutKey { label: "\u{0449}", insert: "\u{0449}" }, // 21  o -> Shch
        LayoutKey { label: "\u{0437}", insert: "\u{0437}" }, // 22  p -> Z
        LayoutKey { label: "\u{0445}", insert: "\u{0445}" }, // 23  [ -> Kh
        LayoutKey { label: "\u{0457}", insert: "\u{0457}" }, // 24  ] -> Yi
        LayoutKey { label: "\\",       insert: "\\"       }, // 25  \ -> same
        // slots 26-36: home row
        LayoutKey { label: "\u{0444}", insert: "\u{0444}" }, // 26  a -> F
        LayoutKey { label: "\u{0456}", insert: "\u{0456}" }, // 27  s -> I
        LayoutKey { label: "\u{0432}", insert: "\u{0432}" }, // 28  d -> V
        LayoutKey { label: "\u{0430}", insert: "\u{0430}" }, // 29  f -> A
        LayoutKey { label: "\u{043F}", insert: "\u{043F}" }, // 30  g -> P
        LayoutKey { label: "\u{0440}", insert: "\u{0440}" }, // 31  h -> R
        LayoutKey { label: "\u{043E}", insert: "\u{043E}" }, // 32  j -> O
        LayoutKey { label: "\u{043B}", insert: "\u{043B}" }, // 33  k -> L
        LayoutKey { label: "\u{0434}", insert: "\u{0434}" }, // 34  l -> D
        LayoutKey { label: "\u{0436}", insert: "\u{0436}" }, // 35  ; -> Zh
        LayoutKey { label: "\u{0454}", insert: "\u{0454}" }, // 36  ' -> Ye
        // slots 37-46: lower alpha row
        LayoutKey { label: "\u{044F}", insert: "\u{044F}" }, // 37  z -> Ya
        LayoutKey { label: "\u{0447}", insert: "\u{0447}" }, // 38  x -> Ch
        LayoutKey { label: "\u{0441}", insert: "\u{0441}" }, // 39  c -> S
        LayoutKey { label: "\u{043C}", insert: "\u{043C}" }, // 40  v -> M
        LayoutKey { label: "\u{0438}", insert: "\u{0438}" }, // 41  b -> I
        LayoutKey { label: "\u{0442}", insert: "\u{0442}" }, // 42  n -> T
        LayoutKey { label: "\u{044C}", insert: "\u{044C}" }, // 43  m -> soft sign
        LayoutKey { label: "\u{0431}", insert: "\u{0431}" }, // 44  , -> B
        LayoutKey { label: "\u{044E}", insert: "\u{044E}" }, // 45  . -> Yu
        LayoutKey { label: "/",        insert: "/"        }, // 46  / -> same
    ],
};

/// All available layouts.
///
/// To add a new language: define a new LayoutDef constant (see UA above) and
/// append a reference to it here.  The toggle button appears automatically.
pub static LAYOUTS: &[&LayoutDef] = &[&US, &UA];
