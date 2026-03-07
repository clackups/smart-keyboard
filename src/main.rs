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

// ── Key hooks ─────────────────────────────────────────────────────────────────

/// Receives key-press and key-release notifications from the on-screen keyboard.
pub trait KeyHook {
    fn on_key_press(&self, key: &str);
    fn on_key_release(&self, key: &str);
}

/// No-op hook used as a placeholder until a real implementation is installed.
pub struct DummyKeyHook;

impl KeyHook for DummyKeyHook {
    fn on_key_press(&self, key: &str) {
        eprintln!("[key_press]   {key:?}");
    }
    fn on_key_release(&self, key: &str) {
        eprintln!("[key_release] {key:?}");
    }
}

// ── Layout ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Layout {
    US,
    UA,
}

// ── Key-width kinds ───────────────────────────────────────────────────────────

/// Semantic width category; pixel values are computed at runtime from the
/// screen width so every row fills the available area exactly.
#[derive(Clone, Copy)]
enum KW {
    Std,    // 1× – regular letter / number / symbol key
    Tab,    // ≈1.5× – left edge of the top-alpha row
    BSlash, // fills the right remainder of the top-alpha row
    Caps,   // ≈1.75× – left edge of the home row
    Enter,  // fills the right remainder of the home row
    Bksp,   // fills the right remainder of the number row
    LShift, // ≈2.25× – left edge of the lower-alpha row
    RShift, // fills the right remainder of the lower-alpha row
    Mod,    // ≈1.5× – Ctrl / Win / Alt / AltGr / Menu
    Space,  // fills the remainder of the bottom row
}

// ── Keyboard layout data ──────────────────────────────────────────────────────

/// Each entry: `(us_label, ua_label, us_insert, ua_insert, key_width_kind)`.
///
/// * `us_insert == ""`     → modifier key; nothing is inserted.
/// * `us_insert == "\x08"` → Backspace action (remove last character).
static KEYBOARD: &[&[(&str, &str, &str, &str, KW)]] = &[
    // ── Number row ─────────────────────────────────────────────────────────
    &[
        ("`",    "ʼ",    "`",    "ʼ",    KW::Std),
        ("1",    "1",    "1",    "1",    KW::Std),
        ("2",    "2",    "2",    "2",    KW::Std),
        ("3",    "3",    "3",    "3",    KW::Std),
        ("4",    "4",    "4",    "4",    KW::Std),
        ("5",    "5",    "5",    "5",    KW::Std),
        ("6",    "6",    "6",    "6",    KW::Std),
        ("7",    "7",    "7",    "7",    KW::Std),
        ("8",    "8",    "8",    "8",    KW::Std),
        ("9",    "9",    "9",    "9",    KW::Std),
        ("0",    "0",    "0",    "0",    KW::Std),
        ("-",    "-",    "-",    "-",    KW::Std),
        ("=",    "=",    "=",    "=",    KW::Std),
        ("Bksp", "Bksp", "\x08", "\x08", KW::Bksp),
    ],
    // ── Top alpha row ───────────────────────────────────────────────────────
    &[
        ("Tab",  "Tab",  "\t",  "\t",  KW::Tab),
        ("q",    "й",    "q",   "й",   KW::Std),
        ("w",    "ц",    "w",   "ц",   KW::Std),
        ("e",    "у",    "e",   "у",   KW::Std),
        ("r",    "к",    "r",   "к",   KW::Std),
        ("t",    "е",    "t",   "е",   KW::Std),
        ("y",    "н",    "y",   "н",   KW::Std),
        ("u",    "г",    "u",   "г",   KW::Std),
        ("i",    "ш",    "i",   "ш",   KW::Std),
        ("o",    "щ",    "o",   "щ",   KW::Std),
        ("p",    "з",    "p",   "з",   KW::Std),
        ("[",    "х",    "[",   "х",   KW::Std),
        ("]",    "ї",    "]",   "ї",   KW::Std),
        ("\\",   "\\",   "\\",  "\\",  KW::BSlash),
    ],
    // ── Home row ────────────────────────────────────────────────────────────
    &[
        ("Caps",  "Caps",  "",    "",    KW::Caps),
        ("a",     "ф",     "a",   "ф",   KW::Std),
        ("s",     "і",     "s",   "і",   KW::Std),
        ("d",     "в",     "d",   "в",   KW::Std),
        ("f",     "а",     "f",   "а",   KW::Std),
        ("g",     "п",     "g",   "п",   KW::Std),
        ("h",     "р",     "h",   "р",   KW::Std),
        ("j",     "о",     "j",   "о",   KW::Std),
        ("k",     "л",     "k",   "л",   KW::Std),
        ("l",     "д",     "l",   "д",   KW::Std),
        (";",     "ж",     ";",   "ж",   KW::Std),
        ("'",     "є",     "'",   "є",   KW::Std),
        ("Enter", "Enter", "\n",  "\n",  KW::Enter),
    ],
    // ── Lower alpha row ─────────────────────────────────────────────────────
    &[
        ("Shift", "Shift", "",    "",    KW::LShift),
        ("z",     "я",     "z",   "я",   KW::Std),
        ("x",     "ч",     "x",   "ч",   KW::Std),
        ("c",     "с",     "c",   "с",   KW::Std),
        ("v",     "м",     "v",   "м",   KW::Std),
        ("b",     "и",     "b",   "и",   KW::Std),
        ("n",     "т",     "n",   "т",   KW::Std),
        ("m",     "ь",     "m",   "ь",   KW::Std),
        (",",     "б",     ",",   "б",   KW::Std),
        (".",     "ю",     ".",   "ю",   KW::Std),
        ("/",     "/",     "/",   "/",   KW::Std),
        ("Shift", "Shift", "",    "",    KW::RShift),
    ],
    // ── Bottom row ──────────────────────────────────────────────────────────
    &[
        ("Ctrl",  "Ctrl",  "",   "",   KW::Mod),
        ("Win",   "Win",   "",   "",   KW::Mod),
        ("Alt",   "Alt",   "",   "",   KW::Mod),
        ("",      "",      " ",  " ",  KW::Space), // space bar
        ("AltGr", "AltGr", "",   "",   KW::Mod),
        ("Menu",  "Menu",  "",   "",   KW::Mod),
        ("Ctrl",  "Ctrl",  "",   "",   KW::Mod),
    ],
];

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let a = app::App::default().with_scheme(app::Scheme::Gleam);

    // ── Screen geometry ──────────────────────────────────────────────────────
    let (sw, sh) = app::screen_size();
    let sw = if sw > 1.0 { sw as i32 } else { 1920 };
    let sh = if sh > 1.0 { sh as i32 } else { 1080 };

    let pad       = 10i32;
    let gap       = 3i32; // gap between keys and between UI sections

    let display_h  = ((sh as f32 * 0.10) as i32).max(50);
    let lang_btn_h = ((sh as f32 * 0.05) as i32).max(28);

    // Keyboard occupies everything below the language buttons.
    let kbd_y = pad + display_h + gap + lang_btn_h + gap;
    let kbd_h = sh - kbd_y - pad;
    let key_h = ((kbd_h - 4 * gap) / 5).max(10); // 5 rows, 4 inter-row gaps

    // ── Key-width calculations ────────────────────────────────────────────────
    // Reference: number row = 13 Std keys + 1 Bksp (fills remainder) + 13 gaps.
    // Solve for the base key width so the row is exactly avail_w pixels wide.
    let avail_w  = sw - 2 * pad;
    let key_w    = ((avail_w - 13 * gap) / 15).max(10);

    // Each "fill" key expands to consume whatever is left in its row.
    let bksp_w   = avail_w - 13 * key_w - 13 * gap; // number row
    let tab_w    = (key_w as f32 * 1.5).round() as i32;
    let bslash_w = avail_w - tab_w - 12 * key_w - 13 * gap; // top-alpha row
    let caps_w   = (key_w as f32 * 1.75).round() as i32;
    let enter_w  = avail_w - caps_w - 11 * key_w - 12 * gap; // home row
    let lshift_w = (key_w as f32 * 2.25).round() as i32;
    let rshift_w = avail_w - lshift_w - 10 * key_w - 11 * gap; // lower-alpha
    let mod_w    = (key_w as f32 * 1.5).round() as i32;
    let space_w  = avail_w - 6 * mod_w - 6 * gap; // bottom row

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

    // ── Font sizes (proportional to widget dimensions) ────────────────────────
    let lbl_size  = (key_h / 3).max(10);
    let disp_size = ((display_h * 2 / 5) as i32).max(12).min(28);
    let btn_size  = (lang_btn_h * 2 / 5).max(10);

    // ── Shared state ─────────────────────────────────────────────────────────
    let layout: Rc<RefCell<Layout>> = Rc::new(RefCell::new(Layout::US));
    let buf = TextBuffer::default();

    // Install the dummy hook; swap in a real KeyHook impl to add custom behaviour.
    let hook: Rc<dyn KeyHook> = Rc::new(DummyKeyHook);

    // ── Window (full-screen) ──────────────────────────────────────────────────
    let mut win = Window::new(0, 0, sw, sh, "Smart Keyboard");
    win.set_color(Color::from_rgb(40, 40, 43));

    // ── Text display (read-only) ──────────────────────────────────────────────
    let mut disp = TextDisplay::new(pad, pad, avail_w, display_h, "");
    disp.set_buffer(buf.clone());
    disp.set_color(Color::from_rgb(28, 28, 28));
    disp.set_text_color(Color::from_rgb(180, 255, 180));
    disp.set_frame(FrameType::DownBox);
    disp.set_text_size(disp_size);

    // ── Language toggle buttons ───────────────────────────────────────────────
    let active_col   = Color::from_rgb(70, 130, 180); // steel-blue = active layout
    let inactive_col = Color::from_rgb(80, 80, 80);

    let lang_y = pad + display_h + gap;
    let lang_w = (avail_w / 6).max(60);

    let mut us_btn = Button::new(pad, lang_y, lang_w, lang_btn_h, "US");
    us_btn.set_color(active_col);
    us_btn.set_label_color(Color::White);
    us_btn.set_label_size(btn_size);

    let mut ua_btn = Button::new(pad + lang_w + gap, lang_y, lang_w, lang_btn_h, "UA");
    ua_btn.set_color(inactive_col);
    ua_btn.set_label_color(Color::White);
    ua_btn.set_label_size(btn_size);

    // ── Keyboard keys ─────────────────────────────────────────────────────────
    // Collect buttons whose labels must change when the layout is switched.
    let switch_btns: Rc<RefCell<Vec<(Button, &'static str, &'static str)>>> =
        Rc::new(RefCell::new(Vec::new()));

    for (row_i, row) in KEYBOARD.iter().enumerate() {
        let row_y = kbd_y + row_i as i32 * (key_h + gap);
        let mut x = pad;

        for &(us_lbl, ua_lbl, us_ch, ua_ch, kw) in row.iter() {
            let w      = px(kw);
            let is_mod = us_ch.is_empty();

            let mut btn = Button::new(x, row_y, w, key_h, us_lbl);
            btn.set_label_size(lbl_size);
            if is_mod {
                btn.set_color(Color::from_rgb(100, 100, 110));
                btn.set_label_color(Color::from_rgb(210, 210, 210));
            } else {
                btn.set_color(Color::from_rgb(218, 218, 222));
                btn.set_label_color(Color::from_rgb(20, 20, 20));
            }

            // ── Key-press / key-release hooks ─────────────────────────────────
            // Returning `false` from the handler delegates to FLTK's default
            // button behaviour (visual press feedback + callback firing).
            {
                let hook_c   = Rc::clone(&hook);
                let layout_h = layout.clone();
                let us_s     = us_ch.to_string();
                let ua_s     = ua_ch.to_string();
                btn.handle(move |_b, ev| {
                    let key = if *layout_h.borrow() == Layout::US { &us_s } else { &ua_s };
                    match ev {
                        Event::Push     => { hook_c.on_key_press(key);   false }
                        Event::Released => { hook_c.on_key_release(key); false }
                        _               => false,
                    }
                });
            }

            // ── Character-insertion callback ──────────────────────────────────
            {
                let layout_c = layout.clone();
                let us_s     = us_ch.to_string();
                let ua_s     = ua_ch.to_string();
                let mut buf_c  = buf.clone();
                let mut disp_c = disp.clone();
                btn.set_callback(move |_| {
                    let ch = if *layout_c.borrow() == Layout::US { &us_s } else { &ua_s };
                    if ch == "\x08" {
                        // Backspace: remove the last Unicode scalar value.
                        let text = buf_c.text();
                        let n    = text.chars().count();
                        if n > 0 {
                            buf_c.set_text(&text.chars().take(n - 1).collect::<String>());
                        }
                    } else if !ch.is_empty() {
                        buf_c.append(ch);
                    }
                    // Scroll the display to keep the latest text visible.
                    let len   = buf_c.length();
                    let lines = disp_c.count_lines(0, len, false);
                    disp_c.scroll(lines, 0);
                });
            }

            if us_lbl != ua_lbl {
                switch_btns.borrow_mut().push((btn.clone(), us_lbl, ua_lbl));
            }
            x += w + gap;
        }
    }

    // ── Layout-switch callbacks ───────────────────────────────────────────────
    {
        let layout_c = layout.clone();
        let sb       = switch_btns.clone();
        let mut ua_c = ua_btn.clone();
        us_btn.set_callback(move |b| {
            *layout_c.borrow_mut() = Layout::US;
            b.set_color(active_col);
            ua_c.set_color(inactive_col);
            for (btn, us_lbl, _) in sb.borrow_mut().iter_mut() {
                btn.set_label(us_lbl);
            }
            app::redraw();
        });
    }
    {
        let layout_c = layout.clone();
        let sb       = switch_btns.clone();
        let mut us_c = us_btn.clone();
        ua_btn.set_callback(move |b| {
            *layout_c.borrow_mut() = Layout::UA;
            b.set_color(active_col);
            us_c.set_color(inactive_col);
            for (btn, _, ua_lbl) in sb.borrow_mut().iter_mut() {
                btn.set_label(ua_lbl);
            }
            app::redraw();
        });
    }

    win.end();
    win.show();
    win.fullscreen(true); // activate after show() to avoid decoration flash

    a.run().unwrap();
}
