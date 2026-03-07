mod keyboards;

use std::cell::RefCell;
use std::rc::Rc;

use fltk::{
    app,
    button::Button,
    enums::{Color, Event, FrameType, Key},
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

use keyboards::{
    is_modifier, is_sticky, special_hook_str, special_label,
    Action, KW, KEYS, LAYOUTS, REGULAR_KEY_COUNT,
};

// =============================================================================
// Key hooks
// =============================================================================

/// Receives key-press and key-release notifications from the on-screen keyboard.
///
/// `scancode` is the Linux evdev key code (linux/input-event-codes.h).
/// `key` is the inserted string (e.g. "a", "\n") or a descriptor token for
/// non-printing keys ("Backspace", "LShift", "CapsLock", etc.).
///
/// Implement this trait and replace `DummyKeyHook` to react to virtual key events.
pub trait KeyHook {
    fn on_key_press(&self, scancode: u16, key: &str);
    fn on_key_release(&self, scancode: u16, key: &str);
}

/// No-op hook: logs every event to stderr.  Replace with a real implementation.
pub struct DummyKeyHook;

impl KeyHook for DummyKeyHook {
    fn on_key_press(&self, scancode: u16, key: &str) {
        eprintln!("[key_press]   scancode={} key={:?}", scancode, key);
    }
    fn on_key_release(&self, scancode: u16, key: &str) {
        eprintln!("[key_release] scancode={} key={:?}", scancode, key);
    }
}

// =============================================================================
// Modifier key state
// =============================================================================

/// Tracks the toggle / sticky state for every modifier key.
///
/// * CapsLock: pure toggle (press once to lock, press again to unlock).
/// * Ctrl, Shift (L/R), Alt, AltGr: sticky-toggle.
///   First press activates them; they auto-deactivate after the next regular
///   keypress.  A second press before any regular key deactivates them early.
#[derive(Default)]
struct ModState {
    pub caps:   bool,
    pub lshift: bool,
    pub rshift: bool,
    pub ctrl:   bool,
    pub alt:    bool,
    pub altgr:  bool,
}

impl ModState {
    /// Flip the modifier for `action`; returns the new active state.
    fn toggle(&mut self, action: Action) -> bool {
        let s = self.slot_mut(action);
        *s = !*s;
        *s
    }

    /// Deactivate all sticky modifiers (Ctrl/Shift/Alt/AltGr).
    fn release_sticky(&mut self) {
        self.lshift = false;
        self.rshift = false;
        self.ctrl   = false;
        self.alt    = false;
        self.altgr  = false;
    }

    fn is_active(&self, action: Action) -> bool { *self.slot(action) }

    fn slot(&self, action: Action) -> &bool {
        match action {
            Action::CapsLock => &self.caps,
            Action::LShift   => &self.lshift,
            Action::RShift   => &self.rshift,
            Action::Ctrl     => &self.ctrl,
            Action::Alt      => &self.alt,
            Action::AltGr    => &self.altgr,
            _                => unreachable!(),
        }
    }
    fn slot_mut(&mut self, action: Action) -> &mut bool {
        match action {
            Action::CapsLock => &mut self.caps,
            Action::LShift   => &mut self.lshift,
            Action::RShift   => &mut self.rshift,
            Action::Ctrl     => &mut self.ctrl,
            Action::Alt      => &mut self.alt,
            Action::AltGr    => &mut self.altgr,
            _                => unreachable!(),
        }
    }
}

// =============================================================================
// Color constants
// =============================================================================

fn col_key_normal()  -> Color { Color::from_rgb(218, 218, 222) }
fn col_key_mod()     -> Color { Color::from_rgb(100, 100, 110) }
fn col_mod_active()  -> Color { Color::from_rgb( 70, 130, 180) } // steel-blue
fn col_nav_sel()     -> Color { Color::from_rgb(255, 200,   0) } // amber

// =============================================================================
// Modifier button descriptor
// =============================================================================

/// A modifier-key button together with its action and base (inactive) color.
/// Stored in a shared list so execute_action can update visual state.
struct ModBtn {
    btn:      Button,
    action:   Action,
    base_col: Color,
}

// =============================================================================
// Navigation
// =============================================================================

/// Move the keyboard-navigation cursor and update highlight colours.
///
/// `dr` / `dc` are row / column deltas.
/// Navigation clamps at the edges (no wrap-around).
/// For vertical moves the target column is chosen by pixel-centre alignment
/// so wide keys (Space, Shifts) map naturally across rows.
fn nav_move(
    all_btns:  &mut Vec<Vec<(Button, Action, u16, Color)>>,
    sel:       &mut (usize, usize),
    mod_state: &Rc<RefCell<ModState>>,
    dr:        i32,
    dc:        i32,
) {
    let (row, col) = *sel;

    // Compute the new position first so we can bail out early at edges
    // without touching any button colours (avoids a visual flicker).
    let rows    = all_btns.len();
    let new_row = (row as i32 + dr).clamp(0, rows as i32 - 1) as usize;
    let row_len = all_btns[new_row].len();
    let new_col = if dr != 0 {
        if new_row == row {
            // Clamped at the vertical edge: stay in the same column.
            col
        } else {
            // Vertical move: find the button in the new row whose pixel x-range
            // contains the current button's centre-x.  This correctly aligns
            // wide keys (Space, Shifts) across rows with different key widths.
            // If no button contains it, pick the nearest one.
            let cur_cx = all_btns[row][col].0.x() + all_btns[row][col].0.w() / 2;
            all_btns[new_row]
                .iter()
                .enumerate()
                .min_by_key(|(_, b)| {
                    let bx = b.0.x();
                    let bw = b.0.w();
                    if cur_cx >= bx && cur_cx < bx + bw { 0i32 }
                    else if cur_cx < bx                 { bx - cur_cx }
                    else                                { cur_cx - (bx + bw) }
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        }
    } else {
        (col as i32 + dc).clamp(0, row_len as i32 - 1) as usize // clamp within row
    };

    // Nothing to do: already at the edge.
    if new_row == row && new_col == col {
        return;
    }

    // Restore the correct colour of the previously highlighted key,
    // accounting for active modifier state.
    let old_action  = all_btns[row][col].1;
    let old_base    = all_btns[row][col].3;
    let restore_col = if is_modifier(old_action)
        && mod_state.borrow().is_active(old_action)
    {
        col_mod_active()
    } else {
        old_base
    };
    all_btns[row][col].0.set_color(restore_col);

    all_btns[new_row][new_col].0.set_color(col_nav_sel());
    // Move FLTK keyboard focus to the highlighted button so the physical
    // spacebar (routed by FLTK to the focused widget) fires the correct key.
    let _ = all_btns[new_row][new_col].0.take_focus();
    *sel = (new_row, new_col);
    app::redraw();
}

// =============================================================================
// Action execution
// =============================================================================

/// Perform the action of a key: notify hooks, insert text, update modifiers.
///
/// `mod_btns` is the list of modifier buttons so their visual state can be
/// updated when a modifier is toggled or a sticky modifier auto-releases.
fn execute_action(
    action:    Action,
    scancode:  u16,
    layout_i:  usize,
    buf:       &mut TextBuffer,
    disp:      &mut TextDisplay,
    hook:      &Rc<dyn KeyHook>,
    mod_state: &Rc<RefCell<ModState>>,
    mod_btns:  &[ModBtn],
) {
    let key_str: &str = match action {
        Action::Regular(slot) => LAYOUTS[layout_i].keys[slot].insert,
        other                 => special_hook_str(other),
    };

    hook.on_key_press(scancode, key_str);

    if is_modifier(action) {
        // Toggle the modifier and refresh the color of its button(s).
        let now_active = mod_state.borrow_mut().toggle(action);
        for m in mod_btns {
            if m.action == action {
                m.btn.clone().set_color(if now_active { col_mod_active() } else { m.base_col });
            }
        }
        app::redraw();
    } else {
        // Regular key: insert text into the buffer.
        match action {
            Action::Regular(slot) => {
                buf.append(LAYOUTS[layout_i].keys[slot].insert);
            }
            Action::Backspace => {
                let text = buf.text();
                let n    = text.chars().count();
                if n > 0 {
                    buf.set_text(&text.chars().take(n - 1).collect::<String>());
                }
            }
            Action::Tab   => buf.append("\t"),
            Action::Enter => buf.append("\n"),
            Action::Space => buf.append(" "),
            _ => {}
        }
        // Scroll the display to keep the newest text visible.
        let len   = buf.length();
        let lines = disp.count_lines(0, len, false);
        disp.scroll(lines, 0);

        // Auto-release sticky modifiers and reset their button colours.
        {
            let mut ms = mod_state.borrow_mut();
            for m in mod_btns {
                if is_sticky(m.action) && ms.is_active(m.action) {
                    m.btn.clone().set_color(m.base_col);
                }
            }
            ms.release_sticky();
        }
        app::redraw();
    }

    hook.on_key_release(scancode, key_str);
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    debug_assert!(
        LAYOUTS.iter().all(|l| l.keys.len() == REGULAR_KEY_COUNT),
        "every LayoutDef must have exactly REGULAR_KEY_COUNT entries"
    );

    let a = app::App::default().with_scheme(app::Scheme::Gleam);

    // --- Screen geometry: all widget sizes are derived proportionally ---
    let (sw_f, sh_f) = app::screen_size();
    let sw = if sw_f > 1.0 { sw_f as i32 } else { 1920 };
    let sh = if sh_f > 1.0 { sh_f as i32 } else { 1080 };

    let pad  = 10i32;
    let gap  =  3i32;

    let display_h  = ((sh as f32 * 0.10) as i32).max(50);
    let lang_btn_h = ((sh as f32 * 0.05) as i32).max(28);

    let kbd_y = pad + display_h + gap + lang_btn_h + gap;
    let kbd_h = sh - kbd_y - 2 * pad; // bottom margin = 2*pad for a clearly visible gap
    let key_h = ((kbd_h - 4 * gap) / 5).max(10);

    // Reference row: 13 Std + Bksp (fills) + 13 gaps = avail_w
    //   key_w = (avail_w - 13*gap) / 15
    // Bottom row after Menu removal: LCtrl + LWin + LAlt + RAlt + RCtrl = 5 Mod keys.
    const BOTTOM_MOD_COUNT: i32 = 5;
    let avail_w  = sw - 2 * pad;
    let key_w    = ((avail_w - 13 * gap) / 15).max(10);

    let bksp_w   = avail_w - 13 * key_w - 13 * gap;
    let tab_w    = (key_w as f32 * 1.5).round() as i32;
    let bslash_w = avail_w - tab_w - 12 * key_w - 13 * gap;
    let caps_w   = (key_w as f32 * 1.75).round() as i32;
    let enter_w  = avail_w - caps_w - 11 * key_w - 12 * gap;
    let lshift_w = (key_w as f32 * 2.25).round() as i32;
    let rshift_w = avail_w - lshift_w - 10 * key_w - 11 * gap;
    let mod_w    = (key_w as f32 * 1.5).round() as i32;
    let space_w  = avail_w - BOTTOM_MOD_COUNT * mod_w - BOTTOM_MOD_COUNT * gap;

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

    // --- Font sizes ---
    let lbl_size  = (key_h / 3).max(10);
    let disp_size = ((display_h * 2 / 5) as i32).max(12).min(28);
    let btn_size  = (lang_btn_h * 2 / 5).max(10);

    // --- Shared state ---
    let layout_idx: Rc<RefCell<usize>>    = Rc::new(RefCell::new(0));
    let mod_state:  Rc<RefCell<ModState>> = Rc::new(RefCell::new(ModState::default()));
    // mod_btns is populated during the key loop; closures borrow it at call time.
    let mod_btns: Rc<RefCell<Vec<ModBtn>>> = Rc::new(RefCell::new(Vec::new()));
    let buf  = TextBuffer::default();
    let hook: Rc<dyn KeyHook> = Rc::new(DummyKeyHook);

    // --- Window (fullscreen; handler intercepts events before children) ---
    let mut win = Window::new(0, 0, sw, sh, "Smart Keyboard");
    win.set_color(Color::from_rgb(40, 40, 43));

    // --- Text display (read-only) ---
    let mut disp = TextDisplay::new(pad, pad, avail_w, display_h, "");
    disp.set_buffer(buf.clone());
    disp.set_color(Color::from_rgb(28, 28, 28));
    disp.set_text_color(Color::from_rgb(180, 255, 180));
    disp.set_frame(FrameType::DownBox);
    disp.set_text_size(disp_size);

    // --- Language toggle buttons (one per entry in LAYOUTS) ---
    let active_col   = Color::from_rgb(70, 130, 180);
    let inactive_col = Color::from_rgb(80, 80, 80);

    let lang_y = pad + display_h + gap;
    let lang_w = (avail_w / 10).max(60).min(120);

    let lang_btns:   Rc<RefCell<Vec<Button>>>          = Rc::new(RefCell::new(Vec::new()));
    let switch_btns: Rc<RefCell<Vec<(Button, usize)>>> = Rc::new(RefCell::new(Vec::new()));

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
            for (j, lb) in lang_btns_c.borrow_mut().iter_mut().enumerate() {
                lb.set_color(if j == li { active_col } else { inactive_col });
            }
            let def = LAYOUTS[li];
            for (kb, slot) in switch_btns_c.borrow_mut().iter_mut() {
                kb.set_label(def.keys[*slot].label);
            }
            app::redraw();
        });
        lang_btns.borrow_mut().push(btn);
    }

    // --- Keyboard key grid ---
    // all_btns[row][col] = (Button, Action, scancode, base_color)
    let all_btns: Rc<RefCell<Vec<Vec<(Button, Action, u16, Color)>>>> =
        Rc::new(RefCell::new(Vec::new()));

    // Navigation selection state: shared with button click callbacks and the
    // window key-event handler so all three can read and update it.
    let sel: Rc<RefCell<(usize, usize)>> = Rc::new(RefCell::new((0, 0)));

    for (row_i, row) in KEYS.iter().enumerate() {
        let row_y = kbd_y + row_i as i32 * (key_h + gap);
        let mut x = pad;
        let mut btn_row: Vec<(Button, Action, u16, Color)> = Vec::new();

        for (col_i, phys) in row.iter().enumerate() {
            let w        = px(phys.kw);
            let is_mod   = is_modifier(phys.action);
            let is_win   = matches!(phys.action, Action::Win);
            let base_col = if is_mod || is_win { col_key_mod() } else { col_key_normal() };

            let init_label: &'static str = match phys.action {
                Action::Regular(slot) => LAYOUTS[0].keys[slot].label,
                other                 => special_label(other),
            };

            let mut btn = Button::new(x, row_y, w, key_h, init_label);
            btn.set_label_size(lbl_size);
            btn.set_color(base_col);
            if is_mod || is_win {
                btn.set_label_color(Color::from_rgb(210, 210, 210));
            } else {
                btn.set_label_color(Color::from_rgb(20, 20, 20));
            }

            // --- Press / release hook (fires before default C++ button handling) ---
            {
                let hook_c       = Rc::clone(&hook);
                let layout_idx_h = layout_idx.clone();
                let action       = phys.action;
                let scancode     = phys.scancode;
                btn.handle(move |_b, ev| {
                    let key_str: &str = match action {
                        Action::Regular(slot) => {
                            LAYOUTS[*layout_idx_h.borrow()].keys[slot].insert
                        }
                        other => special_hook_str(other),
                    };
                    match ev {
                        Event::Push     => { hook_c.on_key_press(scancode, key_str);   false }
                        Event::Released => { hook_c.on_key_release(scancode, key_str); false }
                        _               => false,
                    }
                });
            }

            // --- Click callback: text insertion + modifier toggling ---
            {
                let layout_idx_c = layout_idx.clone();
                let mod_state_c  = mod_state.clone();
                let mod_btns_c   = mod_btns.clone();
                let all_btns_c   = all_btns.clone();
                let sel_c        = sel.clone();
                let mut buf_c    = buf.clone();
                let mut disp_c   = disp.clone();
                let hook_c       = Rc::clone(&hook);
                let action       = phys.action;
                let scancode     = phys.scancode;
                btn.set_callback(move |_| {
                    execute_action(
                        action, scancode,
                        *layout_idx_c.borrow(),
                        &mut buf_c, &mut disp_c, &hook_c,
                        &mod_state_c,
                        &mod_btns_c.borrow(),
                    );
                    // Move the amber highlight to the clicked button.
                    let mut ab = all_btns_c.borrow_mut();
                    let mut s  = sel_c.borrow_mut();
                    let (old_r, old_c) = *s;
                    let old_action  = ab[old_r][old_c].1;
                    let old_base    = ab[old_r][old_c].3;
                    let restore_col = if is_modifier(old_action)
                        && mod_state_c.borrow().is_active(old_action)
                    {
                        col_mod_active()
                    } else {
                        old_base
                    };
                    ab[old_r][old_c].0.set_color(restore_col);
                    ab[row_i][col_i].0.set_color(col_nav_sel());
                    let _ = ab[row_i][col_i].0.take_focus();
                    *s = (row_i, col_i);
                    app::redraw(); // repaint the new amber highlight
                });
            }

            // Track substitutable keys for layout switching.
            if let Action::Regular(slot) = phys.action {
                switch_btns.borrow_mut().push((btn.clone(), slot));
            }

            // Track modifier keys for toggle color updates.
            if is_mod {
                mod_btns.borrow_mut().push(ModBtn {
                    btn:      btn.clone(),
                    action:   phys.action,
                    base_col: base_col,
                });
            }

            btn_row.push((btn, phys.action, phys.scancode, base_col));
            x += w + gap;
        }
        all_btns.borrow_mut().push(btn_row);
    }

    // --- Initial navigation highlight at (row=0, col=0) ---
    {
        let mut ab = all_btns.borrow_mut();
        ab[0][0].0.set_color(col_nav_sel());
        let _ = ab[0][0].0.take_focus();
    }

    // --- Navigation: physical arrow keys + spacebar ---
    // super_handle_first(false) makes the Rust handler run BEFORE FLTK routes
    // the event to any child widget, so we can intercept arrow keys and spacebar
    // before any focused button consumes them.
    {
        let sel_c        = sel.clone();
        let all_btns_c   = all_btns.clone();
        let layout_idx_c = layout_idx.clone();
        let mod_state_c  = mod_state.clone();
        let mod_btns_c   = mod_btns.clone();
        let mut buf_c    = buf.clone();
        let mut disp_c   = disp.clone();
        let hook_c       = Rc::clone(&hook);

        // false = Rust handler runs BEFORE FLTK routes the event to any child
        // widget, so arrow keys and spacebar are intercepted here regardless of
        // which button (if any) currently holds FLTK keyboard focus.
        win.super_handle_first(false);
        win.handle(move |_w, ev| {
            if ev != Event::KeyDown {
                return false;
            }
            let k = app::event_key();

            // Arrow-key navigation.
            if k == Key::Up || k == Key::Down || k == Key::Left || k == Key::Right {
                let (dr, dc) = match k {
                    Key::Up    => (-1,  0),
                    Key::Down  => ( 1,  0),
                    Key::Left  => ( 0, -1),
                    _          => ( 0,  1), // Right
                };
                let mut ab = all_btns_c.borrow_mut();
                let mut s  = sel_c.borrow_mut();
                nav_move(&mut ab, &mut s, &mod_state_c, dr, dc);
                return true;
            }

            // Physical spacebar fires the currently highlighted on-screen key.
            // Use the key code (not event_text) for reliable detection on all backends.
            if k == Key::from_char(' ') {
                let (row, col) = *sel_c.borrow();
                let (action, scancode) = {
                    let ab = all_btns_c.borrow();
                    (ab[row][col].1, ab[row][col].2)
                };
                execute_action(
                    action, scancode,
                    *layout_idx_c.borrow(),
                    &mut buf_c, &mut disp_c, &hook_c,
                    &mod_state_c,
                    &mod_btns_c.borrow(),
                );
                return true;
            }

            false
        });
    }

    win.end();
    // fullscreen(true) before show() so the window is full-screen from the
    // very first frame; calling it after show() can cause a late WM resize
    // that makes the window taller than the visible screen area.
    win.fullscreen(true);
    win.show();

    a.run().unwrap();
}
