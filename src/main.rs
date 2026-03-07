use std::cell::RefCell;
use std::rc::Rc;

use fltk::{
    app,
    button::Button,
    enums::{Color, Event, FrameType},
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

// =============================================================================
// Key hooks
// =============================================================================

/// Receives key-press and key-release notifications from the on-screen keyboard.
///
/// Implement this trait and pass it to the application to react to every
/// virtual key event.  The `key` argument is the string that the key would
/// insert (e.g. "a", "\n") or a descriptive token for non-printing keys
/// ("Backspace", "Tab", "LShift", etc.).
pub trait KeyHook {
    fn on_key_press(&self, key: &str);
    fn on_key_release(&self, key: &str);
}

/// No-op hook used as a placeholder until a real implementation is installed.
///
/// Replace this with a concrete KeyHook implementation to handle events.
pub struct DummyKeyHook;

impl KeyHook for DummyKeyHook {
    fn on_key_press(&self, key: &str) {
        eprintln!("[key_press]   {:?}", key);
    }
    fn on_key_release(&self, key: &str) {
        eprintln!("[key_release] {:?}", key);
    }
}

// =============================================================================
// Key-width kinds
// =============================================================================

/// Semantic width category for each physical key.
///
/// Pixel values are computed at runtime from the screen width so that every
/// keyboard row fills the available area exactly.
#[derive(Clone, Copy)]
enum KW {
    Std,    // 1x    -- regular letter / number / symbol key
    Tab,    // ~1.5x -- left edge of the top-alpha row
    BSlash, // fills the right remainder of the top-alpha row
    Caps,   // ~1.75x -- left edge of the home row
    Enter,  // fills the right remainder of the home row
    Bksp,   // fills the right remainder of the number row
    LShift, // ~2.25x -- left edge of the lower-alpha row
    RShift, // fills the right remainder of the lower-alpha row
    Mod,    // ~1.5x -- Ctrl / Win / Alt / AltGr / Menu keys
    Space,  // fills the remainder of the bottom row
}

// =============================================================================
// Physical key actions
// =============================================================================

/// What a physical key does when activated.
///
/// `Regular(n)` slots are substitutable: each LayoutDef provides one entry per
/// slot, so switching layouts just re-labels and re-maps those keys.
/// All other variants have fixed behaviour independent of the language.
#[derive(Clone, Copy)]
enum Action {
    /// Index into the current LayoutDef::keys slice (0..REGULAR_KEY_COUNT).
    Regular(usize),
    // --- special keys with fixed behaviour regardless of layout ---
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
    Menu,
    Space,
}

/// Display label for a non-Regular key (shown on the button face).
fn special_label(action: Action) -> &'static str {
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
        Action::Menu       => "Menu",
        Action::Space      => "",
        Action::Regular(_) => "",
    }
}

/// Token passed to the key hook for a non-Regular key.
fn special_hook_str(action: Action) -> &'static str {
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
        Action::Menu       => "Menu",
        Action::Regular(_) => "",
    }
}

// =============================================================================
// Physical keyboard structure
// =============================================================================

struct PhysKey {
    kw:     KW,
    action: Action,
}

/// Total number of Regular(n) slots across all rows of KEYS.
/// Every LayoutDef must supply exactly this many LayoutKey entries.
const REGULAR_KEY_COUNT: usize = 47;

/// Physical keyboard rows.  Shape and special keys are fixed; Regular(n)
/// slots are filled by the active LayoutDef at runtime.
///
/// Regular slot index map:
///   Row 0 (number row)  : slots  0-12  (` 1 2 3 4 5 6 7 8 9 0 - =)
///   Row 1 (top alpha)   : slots 13-25  (q w e r t y u i o p [ ] \)
///   Row 2 (home row)    : slots 26-36  (a s d f g h j k l ; ')
///   Row 3 (lower alpha) : slots 37-46  (z x c v b n m , . /)
static KEYS: &[&[PhysKey]] = &[
    // --- Number row (13 Std + Bksp, 13 gaps) ---
    &[
        PhysKey { kw: KW::Std,  action: Action::Regular(0)  }, // `
        PhysKey { kw: KW::Std,  action: Action::Regular(1)  }, // 1
        PhysKey { kw: KW::Std,  action: Action::Regular(2)  }, // 2
        PhysKey { kw: KW::Std,  action: Action::Regular(3)  }, // 3
        PhysKey { kw: KW::Std,  action: Action::Regular(4)  }, // 4
        PhysKey { kw: KW::Std,  action: Action::Regular(5)  }, // 5
        PhysKey { kw: KW::Std,  action: Action::Regular(6)  }, // 6
        PhysKey { kw: KW::Std,  action: Action::Regular(7)  }, // 7
        PhysKey { kw: KW::Std,  action: Action::Regular(8)  }, // 8
        PhysKey { kw: KW::Std,  action: Action::Regular(9)  }, // 9
        PhysKey { kw: KW::Std,  action: Action::Regular(10) }, // 0
        PhysKey { kw: KW::Std,  action: Action::Regular(11) }, // -
        PhysKey { kw: KW::Std,  action: Action::Regular(12) }, // =
        PhysKey { kw: KW::Bksp, action: Action::Backspace   },
    ],
    // --- Top alpha row (Tab + 12 Std + BSlash, 13 gaps) ---
    &[
        PhysKey { kw: KW::Tab,    action: Action::Tab          },
        PhysKey { kw: KW::Std,    action: Action::Regular(13)  }, // q
        PhysKey { kw: KW::Std,    action: Action::Regular(14)  }, // w
        PhysKey { kw: KW::Std,    action: Action::Regular(15)  }, // e
        PhysKey { kw: KW::Std,    action: Action::Regular(16)  }, // r
        PhysKey { kw: KW::Std,    action: Action::Regular(17)  }, // t
        PhysKey { kw: KW::Std,    action: Action::Regular(18)  }, // y
        PhysKey { kw: KW::Std,    action: Action::Regular(19)  }, // u
        PhysKey { kw: KW::Std,    action: Action::Regular(20)  }, // i
        PhysKey { kw: KW::Std,    action: Action::Regular(21)  }, // o
        PhysKey { kw: KW::Std,    action: Action::Regular(22)  }, // p
        PhysKey { kw: KW::Std,    action: Action::Regular(23)  }, // [
        PhysKey { kw: KW::Std,    action: Action::Regular(24)  }, // ]
        PhysKey { kw: KW::BSlash, action: Action::Regular(25)  }, // backslash
    ],
    // --- Home row (Caps + 11 Std + Enter, 12 gaps) ---
    &[
        PhysKey { kw: KW::Caps,  action: Action::CapsLock     },
        PhysKey { kw: KW::Std,   action: Action::Regular(26)  }, // a
        PhysKey { kw: KW::Std,   action: Action::Regular(27)  }, // s
        PhysKey { kw: KW::Std,   action: Action::Regular(28)  }, // d
        PhysKey { kw: KW::Std,   action: Action::Regular(29)  }, // f
        PhysKey { kw: KW::Std,   action: Action::Regular(30)  }, // g
        PhysKey { kw: KW::Std,   action: Action::Regular(31)  }, // h
        PhysKey { kw: KW::Std,   action: Action::Regular(32)  }, // j
        PhysKey { kw: KW::Std,   action: Action::Regular(33)  }, // k
        PhysKey { kw: KW::Std,   action: Action::Regular(34)  }, // l
        PhysKey { kw: KW::Std,   action: Action::Regular(35)  }, // ;
        PhysKey { kw: KW::Std,   action: Action::Regular(36)  }, // '
        PhysKey { kw: KW::Enter, action: Action::Enter        },
    ],
    // --- Lower alpha row (LShift + 10 Std + RShift, 11 gaps) ---
    &[
        PhysKey { kw: KW::LShift, action: Action::LShift      },
        PhysKey { kw: KW::Std,    action: Action::Regular(37)  }, // z
        PhysKey { kw: KW::Std,    action: Action::Regular(38)  }, // x
        PhysKey { kw: KW::Std,    action: Action::Regular(39)  }, // c
        PhysKey { kw: KW::Std,    action: Action::Regular(40)  }, // v
        PhysKey { kw: KW::Std,    action: Action::Regular(41)  }, // b
        PhysKey { kw: KW::Std,    action: Action::Regular(42)  }, // n
        PhysKey { kw: KW::Std,    action: Action::Regular(43)  }, // m
        PhysKey { kw: KW::Std,    action: Action::Regular(44)  }, // ,
        PhysKey { kw: KW::Std,    action: Action::Regular(45)  }, // .
        PhysKey { kw: KW::Std,    action: Action::Regular(46)  }, // /
        PhysKey { kw: KW::RShift, action: Action::RShift       },
    ],
    // --- Bottom row (6 Mod + Space, 6 gaps) ---
    &[
        PhysKey { kw: KW::Mod,   action: Action::Ctrl  },
        PhysKey { kw: KW::Mod,   action: Action::Win   },
        PhysKey { kw: KW::Mod,   action: Action::Alt   },
        PhysKey { kw: KW::Space, action: Action::Space },
        PhysKey { kw: KW::Mod,   action: Action::AltGr },
        PhysKey { kw: KW::Mod,   action: Action::Menu  },
        PhysKey { kw: KW::Mod,   action: Action::Ctrl  },
    ],
];

// =============================================================================
// Language layouts
// =============================================================================

/// One substitutable key slot: the text shown on the button face and the
/// string appended to the buffer when the key is pressed.
struct LayoutKey {
    label:  &'static str,
    insert: &'static str,
}

/// A named keyboard layout.
///
/// To add a new language:
///   1. Define `static MY_LANG: LayoutDef = LayoutDef { name: "XX", keys: &[...] };`
///      with exactly REGULAR_KEY_COUNT (47) entries in the order shown in the
///      slot index map inside the KEYS doc-comment.
///   2. Append `&MY_LANG` to the LAYOUTS slice below.
///
/// That is the only change required -- the toggle button appears automatically.
struct LayoutDef {
    name: &'static str,
    keys: &'static [LayoutKey],
}

// ---------------------------------------------------------------------------
// US (QWERTY)
// ---------------------------------------------------------------------------
static US: LayoutDef = LayoutDef {
    name: "US",
    keys: &[
        // --- number row (slots 0-12) ---
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
        // --- top alpha (slots 13-25) ---
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
        // --- home row (slots 26-36) ---
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
        // --- lower alpha (slots 37-46) ---
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
// All non-ASCII runtime values are expressed as \u{XXXX} escape sequences so
// the source file remains strictly ASCII throughout.
//
// Slot-to-Ukrainian character mapping:
//   slot 0  ` -> \u{02BC}  MODIFIER LETTER APOSTROPHE (Ukrainian apostrophe)
//   slot 13 q -> \u{0439}  CYRILLIC SMALL LETTER SHORT I         (J)
//   slot 14 w -> \u{0446}  CYRILLIC SMALL LETTER TSE             (Ts)
//   slot 15 e -> \u{0443}  CYRILLIC SMALL LETTER U
//   slot 16 r -> \u{043A}  CYRILLIC SMALL LETTER KA              (K)
//   slot 17 t -> \u{0435}  CYRILLIC SMALL LETTER IE              (Ye)
//   slot 18 y -> \u{043D}  CYRILLIC SMALL LETTER EN              (N)
//   slot 19 u -> \u{0433}  CYRILLIC SMALL LETTER GHE             (G)
//   slot 20 i -> \u{0448}  CYRILLIC SMALL LETTER SHA             (Sh)
//   slot 21 o -> \u{0449}  CYRILLIC SMALL LETTER SHCHA           (Shch)
//   slot 22 p -> \u{0437}  CYRILLIC SMALL LETTER ZE              (Z)
//   slot 23 [ -> \u{0445}  CYRILLIC SMALL LETTER HA              (Kh)
//   slot 24 ] -> \u{0457}  CYRILLIC SMALL LETTER YI              (Yi)
//   slot 25 \ -> \  (unchanged)
//   slot 26 a -> \u{0444}  CYRILLIC SMALL LETTER EF              (F)
//   slot 27 s -> \u{0456}  CYRILLIC SMALL LETTER BYELORUSSIAN-UKRAINIAN I
//   slot 28 d -> \u{0432}  CYRILLIC SMALL LETTER VE              (V)
//   slot 29 f -> \u{0430}  CYRILLIC SMALL LETTER A
//   slot 30 g -> \u{043F}  CYRILLIC SMALL LETTER PE              (P)
//   slot 31 h -> \u{0440}  CYRILLIC SMALL LETTER ER              (R)
//   slot 32 j -> \u{043E}  CYRILLIC SMALL LETTER O
//   slot 33 k -> \u{043B}  CYRILLIC SMALL LETTER EL              (L)
//   slot 34 l -> \u{0434}  CYRILLIC SMALL LETTER DE              (D)
//   slot 35 ; -> \u{0436}  CYRILLIC SMALL LETTER ZHE             (Zh)
//   slot 36 ' -> \u{0454}  CYRILLIC SMALL LETTER UKRAINIAN IE    (Ye)
//   slot 37 z -> \u{044F}  CYRILLIC SMALL LETTER YA              (Ya)
//   slot 38 x -> \u{0447}  CYRILLIC SMALL LETTER CHE             (Ch)
//   slot 39 c -> \u{0441}  CYRILLIC SMALL LETTER ES              (S)
//   slot 40 v -> \u{043C}  CYRILLIC SMALL LETTER EM              (M)
//   slot 41 b -> \u{0438}  CYRILLIC SMALL LETTER I
//   slot 42 n -> \u{0442}  CYRILLIC SMALL LETTER TE              (T)
//   slot 43 m -> \u{044C}  CYRILLIC SMALL LETTER SOFT SIGN
//   slot 44 , -> \u{0431}  CYRILLIC SMALL LETTER BE              (B)
//   slot 45 . -> \u{044E}  CYRILLIC SMALL LETTER YU              (Yu)
//   slot 46 / -> /  (unchanged)
// ---------------------------------------------------------------------------
static UA: LayoutDef = LayoutDef {
    name: "UA",
    keys: &[
        // --- number row (slots 0-12) ---
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
        // --- top alpha (slots 13-25) ---
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
        LayoutKey { label: "\\",       insert: "\\"       }, // 25  \ -> \ (same)
        // --- home row (slots 26-36) ---
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
        // --- lower alpha (slots 37-46) ---
        LayoutKey { label: "\u{044F}", insert: "\u{044F}" }, // 37  z -> Ya
        LayoutKey { label: "\u{0447}", insert: "\u{0447}" }, // 38  x -> Ch
        LayoutKey { label: "\u{0441}", insert: "\u{0441}" }, // 39  c -> S
        LayoutKey { label: "\u{043C}", insert: "\u{043C}" }, // 40  v -> M
        LayoutKey { label: "\u{0438}", insert: "\u{0438}" }, // 41  b -> I
        LayoutKey { label: "\u{0442}", insert: "\u{0442}" }, // 42  n -> T
        LayoutKey { label: "\u{044C}", insert: "\u{044C}" }, // 43  m -> soft sign
        LayoutKey { label: "\u{0431}", insert: "\u{0431}" }, // 44  , -> B
        LayoutKey { label: "\u{044E}", insert: "\u{044E}" }, // 45  . -> Yu
        LayoutKey { label: "/",        insert: "/"        }, // 46  / -> / (same)
    ],
};

/// All available layouts.
///
/// To add a new language: define a new LayoutDef constant (see UA above for
/// an example) and append a reference to it here.  The toggle button appears
/// automatically; no other code change is required.
static LAYOUTS: &[&LayoutDef] = &[&US, &UA];

// =============================================================================
// Main
// =============================================================================

fn main() {
    let a = app::App::default().with_scheme(app::Scheme::Gleam);

    // --- Screen geometry (full-screen, all widget sizes derived from this) ---
    let (sw, sh) = app::screen_size();
    let sw = if sw > 1.0 { sw as i32 } else { 1920 };
    let sh = if sh > 1.0 { sh as i32 } else { 1080 };

    let pad       = 10i32;
    let gap       = 3i32; // pixels between keys and between UI sections

    let display_h  = ((sh as f32 * 0.10) as i32).max(50);
    let lang_btn_h = ((sh as f32 * 0.05) as i32).max(28);

    // Keyboard occupies everything below the language buttons.
    let kbd_y = pad + display_h + gap + lang_btn_h + gap;
    let kbd_h = sh - kbd_y - pad;
    let key_h = ((kbd_h - 4 * gap) / 5).max(10); // 5 rows, 4 inter-row gaps

    // --- Key-width calculations ---
    // Reference: number row = 13 Std + Bksp (fills remainder) + 13 gaps
    //   => key_w = (avail_w - 13*gap) / 15   (13 x 1-unit + 1 x 2-unit = 15)
    // Every other row uses the same key_w so all rows reach exactly avail_w.
    let avail_w  = sw - 2 * pad;
    let key_w    = ((avail_w - 13 * gap) / 15).max(10);

    let bksp_w   = avail_w - 13 * key_w - 13 * gap;           // row 0
    let tab_w    = (key_w as f32 * 1.5).round() as i32;
    let bslash_w = avail_w - tab_w - 12 * key_w - 13 * gap;   // row 1
    let caps_w   = (key_w as f32 * 1.75).round() as i32;
    let enter_w  = avail_w - caps_w - 11 * key_w - 12 * gap;  // row 2
    let lshift_w = (key_w as f32 * 2.25).round() as i32;
    let rshift_w = avail_w - lshift_w - 10 * key_w - 11 * gap; // row 3
    let mod_w    = (key_w as f32 * 1.5).round() as i32;
    let space_w  = avail_w - 6 * mod_w - 6 * gap;             // row 4

    let px = |kw: KW| match kw {
        KW::Std    => key_w,
        KW::Tab    => tab_w,
        KW::BSlash => bslash_w,
        KW::Caps   => caps_w,
        KW::Enter  => enter_w,
        KW::Bksp   => bksp_w,
        KW::LShift => lshift_w,
        KW::RShift => rshift_w,
        KW::Mod    => mod_w,
        KW::Space  => space_w,
    };

    // --- Font sizes (proportional to widget dimensions) ---
    let lbl_size  = (key_h / 3).max(10);
    let disp_size = ((display_h * 2 / 5) as i32).max(12).min(28);
    let btn_size  = (lang_btn_h * 2 / 5).max(10);

    // --- Shared state ---
    let layout_idx: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let buf = TextBuffer::default();

    // Install the dummy hook; replace DummyKeyHook with a real KeyHook impl.
    let hook: Rc<dyn KeyHook> = Rc::new(DummyKeyHook);

    // --- Window (full-screen) ---
    let mut win = Window::new(0, 0, sw, sh, "Smart Keyboard");
    win.set_color(Color::from_rgb(40, 40, 43));

    // --- Text display (read-only) ---
    let mut disp = TextDisplay::new(pad, pad, avail_w, display_h, "");
    disp.set_buffer(buf.clone());
    disp.set_color(Color::from_rgb(28, 28, 28));
    disp.set_text_color(Color::from_rgb(180, 255, 180));
    disp.set_frame(FrameType::DownBox);
    disp.set_text_size(disp_size);

    // --- Language toggle buttons (one per layout, generated from LAYOUTS) ---
    let active_col   = Color::from_rgb(70, 130, 180);
    let inactive_col = Color::from_rgb(80, 80, 80);

    let lang_y = pad + display_h + gap;
    let lang_w = (avail_w / 10).max(60).min(120);

    // Shared handle to all language buttons so callbacks can recolour them.
    let lang_btns: Rc<RefCell<Vec<Button>>> = Rc::new(RefCell::new(Vec::new()));

    // switch_btns: (button handle, Regular slot index).
    // Populated in the key loop below; used by language callbacks to relabel.
    let switch_btns: Rc<RefCell<Vec<(Button, usize)>>> =
        Rc::new(RefCell::new(Vec::new()));

    for (li, def) in LAYOUTS.iter().enumerate() {
        let btn_x = pad + li as i32 * (lang_w + gap);
        let mut btn = Button::new(btn_x, lang_y, lang_w, lang_btn_h, def.name);
        btn.set_color(if li == 0 { active_col } else { inactive_col });
        btn.set_label_color(Color::White);
        btn.set_label_size(btn_size);

        let layout_idx_c  = layout_idx.clone();
        let lang_btns_c   = lang_btns.clone();
        let switch_btns_c = switch_btns.clone();

        btn.set_callback(move |_| {
            *layout_idx_c.borrow_mut() = li;
            // Recolour language buttons.
            for (j, lb) in lang_btns_c.borrow_mut().iter_mut().enumerate() {
                lb.set_color(if j == li { active_col } else { inactive_col });
            }
            // Relabel every substitutable key with the new layout's text.
            let def = LAYOUTS[li];
            for (kb, slot) in switch_btns_c.borrow_mut().iter_mut() {
                kb.set_label(def.keys[*slot].label);
            }
            app::redraw();
        });

        lang_btns.borrow_mut().push(btn);
    }

    // --- Keyboard keys ---
    for (row_i, row) in KEYS.iter().enumerate() {
        let row_y = kbd_y + row_i as i32 * (key_h + gap);
        let mut x = pad;

        for phys in row.iter() {
            let w = px(phys.kw);

            // Modifier/meta keys get a darker background; they type nothing.
            let is_mod = matches!(
                phys.action,
                Action::LShift | Action::RShift | Action::CapsLock
                    | Action::Ctrl | Action::Win | Action::Alt
                    | Action::AltGr | Action::Menu
            );

            let init_label: &'static str = match phys.action {
                Action::Regular(slot) => LAYOUTS[0].keys[slot].label,
                other                 => special_label(other),
            };

            let mut btn = Button::new(x, row_y, w, key_h, init_label);
            btn.set_label_size(lbl_size);
            if is_mod {
                btn.set_color(Color::from_rgb(100, 100, 110));
                btn.set_label_color(Color::from_rgb(210, 210, 210));
            } else {
                btn.set_color(Color::from_rgb(218, 218, 222));
                btn.set_label_color(Color::from_rgb(20, 20, 20));
            }

            // --- Key-press / key-release hooks ---
            // Returning false from handle() delegates to FLTK's default button
            // behaviour (visual press feedback + firing the set_callback below).
            {
                let hook_c       = Rc::clone(&hook);
                let layout_idx_h = layout_idx.clone();
                let action       = phys.action;
                btn.handle(move |_b, ev| {
                    let key: &str = match action {
                        Action::Regular(slot) => {
                            LAYOUTS[*layout_idx_h.borrow()].keys[slot].insert
                        }
                        other => special_hook_str(other),
                    };
                    match ev {
                        Event::Push     => { hook_c.on_key_press(key);   false }
                        Event::Released => { hook_c.on_key_release(key); false }
                        _               => false,
                    }
                });
            }

            // --- Character-insertion callback ---
            {
                let layout_idx_c = layout_idx.clone();
                let mut buf_c    = buf.clone();
                let mut disp_c   = disp.clone();
                let action       = phys.action;
                btn.set_callback(move |_| {
                    match action {
                        Action::Regular(slot) => {
                            let ch = LAYOUTS[*layout_idx_c.borrow()].keys[slot].insert;
                            buf_c.append(ch);
                        }
                        Action::Backspace => {
                            let text = buf_c.text();
                            let n    = text.chars().count();
                            if n > 0 {
                                buf_c.set_text(
                                    &text.chars().take(n - 1).collect::<String>(),
                                );
                            }
                        }
                        Action::Tab   => buf_c.append("\t"),
                        Action::Enter => buf_c.append("\n"),
                        Action::Space => buf_c.append(" "),
                        // Modifier keys do not insert characters.
                        _ => {}
                    }
                    // Keep the display scrolled to the latest text.
                    let len   = buf_c.length();
                    let lines = disp_c.count_lines(0, len, false);
                    disp_c.scroll(lines, 0);
                });
            }

            // Track Regular keys for relabelling on layout switch.
            if let Action::Regular(slot) = phys.action {
                switch_btns.borrow_mut().push((btn.clone(), slot));
            }

            x += w + gap;
        }
    }

    win.end();
    win.show();
    win.fullscreen(true); // applied after show() to avoid decoration flash

    a.run().unwrap();
}
