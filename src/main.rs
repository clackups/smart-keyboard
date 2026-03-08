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
// Navigation selection
// =============================================================================

/// Identifies which button currently holds the amber navigation highlight.
#[derive(Clone, Copy, PartialEq)]
enum NavSel {
    /// A language-toggle button; index into `lang_btns`.
    Lang(usize),
    /// A keyboard key: `all_btns[row][col]`.
    Key(usize, usize),
}

// =============================================================================
// Navigation
// =============================================================================

/// Find the index in `items` (iterator of `(x, width)`) whose range best covers `cx`.
fn closest_to_cx(items: impl Iterator<Item = (i32, i32)>, cx: i32) -> usize {
    items
        .enumerate()
        .min_by_key(|(_, (bx, bw))| {
            if cx >= *bx && cx < bx + bw { 0i32 }
            else if cx < *bx             { bx - cx }
            else                         { cx - (bx + bw) }
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Find the index in a keyboard row whose x-range best covers pixel centre `cx`.
fn closest_col(row: &[(Button, Action, u16, Color)], cx: i32) -> usize {
    closest_to_cx(row.iter().map(|b| (b.0.x(), b.0.w())), cx)
}

/// Find the index in the lang-button strip whose x-range best covers pixel centre `cx`.
fn closest_lang(lang_btns: &[Button], cx: i32) -> usize {
    closest_to_cx(lang_btns.iter().map(|b| (b.x(), b.w())), cx)
}

/// Move the keyboard-navigation cursor and update highlight colours.
///
/// Navigation clamps at all edges (no wrap-around).
/// The cursor can move between the language-button strip and the keyboard grid;
/// vertical transitions are pixel-centre aligned so wide keys map naturally.
fn nav_move(
    all_btns:   &mut Vec<Vec<(Button, Action, u16, Color)>>,
    lang_btns:  &mut Vec<Button>,
    layout_idx: usize,
    sel:        &mut NavSel,
    mod_state:  &Rc<RefCell<ModState>>,
    dr:         i32,
    dc:         i32,
) {
    let new_sel: NavSel = match *sel {
        NavSel::Lang(li) => {
            if dr < 0 {
                // Already at the top edge.
                NavSel::Lang(li)
            } else if dr > 0 {
                // Down into the first keyboard row, pixel-aligned.
                let cx = lang_btns[li].x() + lang_btns[li].w() / 2;
                NavSel::Key(0, closest_col(&all_btns[0], cx))
            } else {
                // Left / right within the lang strip, clamped.
                let lc = lang_btns.len();
                NavSel::Lang((li as i32 + dc).clamp(0, lc as i32 - 1) as usize)
            }
        }
        NavSel::Key(row, col) => {
            if dr < 0 && row == 0 {
                // Up from the top keyboard row → lang strip, pixel-aligned.
                let cx = all_btns[0][col].0.x() + all_btns[0][col].0.w() / 2;
                NavSel::Lang(closest_lang(lang_btns, cx))
            } else if dr != 0 {
                let rows    = all_btns.len();
                let new_row = (row as i32 + dr).clamp(0, rows as i32 - 1) as usize;
                if new_row == row {
                    // Clamped at the bottom edge.
                    NavSel::Key(row, col)
                } else {
                    let cx = all_btns[row][col].0.x() + all_btns[row][col].0.w() / 2;
                    NavSel::Key(new_row, closest_col(&all_btns[new_row], cx))
                }
            } else {
                // Left / right within the current keyboard row, clamped.
                let rl      = all_btns[row].len();
                let new_col = (col as i32 + dc).clamp(0, rl as i32 - 1) as usize;
                NavSel::Key(row, new_col)
            }
        }
    };

    // Nothing to do when already at the edge.
    if new_sel == *sel {
        return;
    }

    // Restore the old selection's colour.
    match *sel {
        NavSel::Lang(li) => {
            let c = if li == layout_idx { Color::from_rgb(70, 130, 180) }
                    else                { Color::from_rgb(80, 80, 80) };
            lang_btns[li].set_color(c);
        }
        NavSel::Key(row, col) => {
            let old_action = all_btns[row][col].1;
            let old_base   = all_btns[row][col].3;
            let c = if is_modifier(old_action) && mod_state.borrow().is_active(old_action) {
                col_mod_active()
            } else {
                old_base
            };
            all_btns[row][col].0.set_color(c);
        }
    }

    // Highlight the new selection in amber and give it keyboard focus.
    match new_sel {
        NavSel::Lang(li) => {
            lang_btns[li].set_color(col_nav_sel());
            let _ = lang_btns[li].take_focus();
        }
        NavSel::Key(row, col) => {
            all_btns[row][col].0.set_color(col_nav_sel());
            let _ = all_btns[row][col].0.take_focus();
        }
    }

    *sel = new_sel;
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
    // For Regular keys, compute the text to insert respecting Shift / CapsLock.
    // Symbol/number keys (shifted != "") use the shifted character on LShift/RShift;
    // CapsLock does NOT affect them (standard keyboard behaviour).
    // Letter keys (shifted == "") use to_uppercase() for any of Caps/LShift/RShift.
    let regular_text: Option<String> = if let Action::Regular(slot) = action {
        let key = &LAYOUTS[layout_i].keys[slot];
        let ms  = mod_state.borrow();
        Some(if !key.shifted.is_empty() && (ms.lshift || ms.rshift) {
            key.shifted.to_string()
        } else if key.shifted.is_empty() && (ms.caps || ms.lshift || ms.rshift) {
            key.insert.to_uppercase()
        } else {
            key.insert.to_string()
        })
    } else {
        None
    };

    let key_str: &str = match action {
        Action::Regular(_) => regular_text.as_deref().unwrap_or(""),
        other              => special_hook_str(other),
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
            Action::Regular(_) => {
                buf.append(regular_text.as_deref().unwrap_or(""));
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

    // app::screen_size() is reliable immediately after App::default() because
    // the display/compositor connection is open at that point.  We size the
    // window explicitly to those dimensions so the child-widget layout has real
    // pixel values before show().  set_border(false) removes the title bar /
    // decorations entirely; fullscreen(true) then covers the whole screen.
    let (sw_f, sh_f) = app::screen_size();
    let sw = sw_f as i32;
    let sh = sh_f as i32;

    let mut win = Window::new(0, 0, sw, sh, "Smart Keyboard");
    win.set_color(Color::from_rgb(40, 40, 43));
    win.set_border(false); // remove title bar / window decorations
    win.fullscreen(true);

    let pad  = 10i32;
    let gap  =  3i32;

    let display_h  = ((sh as f32 * 0.10) as i32).max(50);
    let lang_btn_h = ((sh as f32 * 0.05) as i32).max(28);

    let kbd_y = pad + display_h + gap + lang_btn_h + gap;
    let kbd_h = sh - kbd_y - 2 * pad; // bottom margin = 2*pad
    // 6 rows (F-keys + 5 QWERTY rows), 5 inter-row gaps
    let key_h = ((kbd_h - 5 * gap) / 6).max(10);

    // Ortholinear: every key is key_w wide.
    // The widest rows (number row and QWERTY row) are 18 slots wide:
    //   14 main keys + 1 Spacer + 3 nav keys → 18*(key_w+gap) - gap = avail_w
    //   key_w = (avail_w - 17*gap) / 18
    // Bottom row: Ctrl Win Alt [Space] AltGr Ctrl Spacer ← ↓ → = 9 non-Space slots
    //   Space spans exactly 9 grid columns: space_w = 9*key_w + 8*gap
    //   (Pinning to exact grid avoids integer-division remainder bleeding into the
    //   spacebar width; the row may be a few pixels narrower than avail_w.)
    let avail_w = sw - 2 * pad;
    let key_w   = ((avail_w - 17 * gap) / 18).max(10);
    let space_w = 9 * key_w + 8 * gap;

    let px = |kw: KW| match kw {
        KW::Space            => space_w,
        KW::Std | KW::Spacer => key_w,
    };

    // --- Font sizes ---
    // Drive label size from key width so the longest labels ("AltGr", "Enter",
    // "Shift") stay within the button boundary.  key_w/4 gives ~25% horizontal
    // margin for a 5-character label in a proportional font.
    let lbl_size  = (key_w / 4).max(10);
    let disp_size = ((display_h * 2 / 5) as i32).max(12).min(28);
    // Lang buttons are one grid column wide (key_w); reuse lbl_size so their
    // text labels fit with the same margin as keyboard-key labels.
    let btn_size  = lbl_size;

    // --- Shared state ---
    let layout_idx: Rc<RefCell<usize>>    = Rc::new(RefCell::new(0));
    let mod_state:  Rc<RefCell<ModState>> = Rc::new(RefCell::new(ModState::default()));
    // mod_btns is populated during the key loop; closures borrow it at call time.
    let mod_btns: Rc<RefCell<Vec<ModBtn>>> = Rc::new(RefCell::new(Vec::new()));
    let buf  = TextBuffer::default();
    let hook: Rc<dyn KeyHook> = Rc::new(DummyKeyHook);

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
    // Language buttons snap to the keyboard grid: each button is exactly one
    // grid column wide (key_w) and placed at pad + li*(key_w+gap), aligning
    // with grid columns 0, 1, 2 …
    let lang_w = key_w;

    let lang_btns:   Rc<RefCell<Vec<Button>>>          = Rc::new(RefCell::new(Vec::new()));
    let switch_btns: Rc<RefCell<Vec<(Button, usize)>>> = Rc::new(RefCell::new(Vec::new()));

    // Declared here (before the lang-button loop) so that lang-button click
    // callbacks can share them with keyboard-key callbacks.
    // all_btns[row][col] = (Button, Action, scancode, base_color)
    let all_btns: Rc<RefCell<Vec<Vec<(Button, Action, u16, Color)>>>> =
        Rc::new(RefCell::new(Vec::new()));
    // Navigation cursor: starts on the first keyboard key.
    let sel: Rc<RefCell<NavSel>> = Rc::new(RefCell::new(NavSel::Key(0, 0)));

    for (li, def) in LAYOUTS.iter().enumerate() {
        let btn_x = pad + li as i32 * (lang_w + gap);
        let mut btn = Button::new(btn_x, lang_y, lang_w, lang_btn_h, def.name);
        btn.set_color(if li == 0 { active_col } else { inactive_col });
        btn.set_label_color(Color::White);
        btn.set_label_size(btn_size);

        let layout_idx_c  = layout_idx.clone();
        let lang_btns_c   = lang_btns.clone();
        let switch_btns_c = switch_btns.clone();
        let all_btns_c    = all_btns.clone();
        let sel_c         = sel.clone();
        let mod_state_c   = mod_state.clone();
        btn.set_callback(move |_| {
            // Execute the language switch.
            *layout_idx_c.borrow_mut() = li;
            for (j, lb) in lang_btns_c.borrow_mut().iter_mut().enumerate() {
                lb.set_color(if j == li { active_col } else { inactive_col });
            }
            let def = LAYOUTS[li];
            for (kb, slot) in switch_btns_c.borrow_mut().iter_mut() {
                let key = &def.keys[*slot];
                if key.shifted.is_empty() {
                    kb.set_label(key.label);
                } else {
                    let lbl = format!("{}\n{}", key.shifted, key.label);
                    kb.set_label(&lbl);
                }
            }
            // Move the amber highlight to this lang button.
            // Copy sel (it is Copy) so the borrow is released before we mutate below.
            let old_sel = *sel_c.borrow();
            if let NavSel::Key(old_r, old_c) = old_sel {
                let mut ab = all_btns_c.borrow_mut();
                let old_action = ab[old_r][old_c].1;
                let old_base   = ab[old_r][old_c].3;
                let restore = if is_modifier(old_action)
                    && mod_state_c.borrow().is_active(old_action)
                {
                    col_mod_active()
                } else {
                    old_base
                };
                ab[old_r][old_c].0.set_color(restore);
            }
            // (If old_sel was Lang(_), the colour loop above already restored it.)
            lang_btns_c.borrow_mut()[li].set_color(col_nav_sel());
            let _ = lang_btns_c.borrow_mut()[li].take_focus();
            *sel_c.borrow_mut() = NavSel::Lang(li);
            app::redraw();
        });
        lang_btns.borrow_mut().push(btn);
    }

    // --- Keyboard key grid ---
    // (all_btns and sel were declared before the lang-button loop above)

    for (row_i, row) in KEYS.iter().enumerate() {
        let row_y = kbd_y + row_i as i32 * (key_h + gap);
        let mut x = pad;
        let mut btn_row: Vec<(Button, Action, u16, Color)> = Vec::new();

        // btn_col tracks the index within btn_row (skips Spacer slots).
        let mut btn_col = 0usize;

        for phys in row.iter() {
            let w = px(phys.kw);

            // Spacer: advance x but create no button.
            if matches!(phys.kw, KW::Spacer) {
                x += w + gap;
                continue;
            }

            let col_i    = btn_col;
            btn_col     += 1;
            let is_mod   = is_modifier(phys.action);
            // Regular letter/symbol keys and the Space bar are light;
            // every other key (modifiers, F-keys, nav, arrows) is dark.
            let base_col = match phys.action {
                Action::Regular(_) | Action::Space => col_key_normal(),
                _                                  => col_key_mod(),
            };

            let init_label: String = match phys.action {
                Action::Regular(slot) => {
                    let key = &LAYOUTS[0].keys[slot];
                    if key.shifted.is_empty() {
                        key.label.to_string()
                    } else {
                        format!("{}\n{}", key.shifted, key.label)
                    }
                }
                other => special_label(other).to_string(),
            };

            let mut btn = Button::new(x, row_y, w, key_h, None);
            btn.set_label(&init_label);
            btn.set_label_size(lbl_size);
            btn.set_color(base_col);
            if matches!(phys.action, Action::Regular(_) | Action::Space) {
                btn.set_label_color(Color::from_rgb(20, 20, 20));
            } else {
                btn.set_label_color(Color::from_rgb(210, 210, 210));
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
                let lang_btns_c  = lang_btns.clone();
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
                    // Restore the previously highlighted button's colour.
                    match *s {
                        NavSel::Key(old_r, old_c) => {
                            let old_action = ab[old_r][old_c].1;
                            let old_base   = ab[old_r][old_c].3;
                            let restore = if is_modifier(old_action)
                                && mod_state_c.borrow().is_active(old_action)
                            {
                                col_mod_active()
                            } else {
                                old_base
                            };
                            ab[old_r][old_c].0.set_color(restore);
                        }
                        NavSel::Lang(li) => {
                            let restore = if li == *layout_idx_c.borrow() {
                                Color::from_rgb(70, 130, 180)
                            } else {
                                Color::from_rgb(80, 80, 80)
                            };
                            lang_btns_c.borrow_mut()[li].set_color(restore);
                        }
                    }
                    ab[row_i][col_i].0.set_color(col_nav_sel());
                    let _ = ab[row_i][col_i].0.take_focus();
                    *s = NavSel::Key(row_i, col_i);
                    app::redraw();
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
        let lang_btns_c  = lang_btns.clone();
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

            // Suppress Escape so FLTK does not close the window.
            if k == Key::Escape {
                return true;
            }

            // Arrow-key navigation.
            if k == Key::Up || k == Key::Down || k == Key::Left || k == Key::Right {
                let (dr, dc) = match k {
                    Key::Up    => (-1,  0),
                    Key::Down  => ( 1,  0),
                    Key::Left  => ( 0, -1),
                    _          => ( 0,  1), // Right
                };
                let mut ab = all_btns_c.borrow_mut();
                let mut lb = lang_btns_c.borrow_mut();
                let mut s  = sel_c.borrow_mut();
                nav_move(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, dr, dc);
                return true;
            }

            // Physical spacebar fires the currently highlighted on-screen button.
            if k == Key::from_char(' ') {
                // Copy NavSel (it is Copy) so the borrow is released before any
                // callback that may itself borrow sel_c (e.g. the lang callback).
                let cur_sel = *sel_c.borrow();
                match cur_sel {
                    NavSel::Lang(li) => {
                        // Fire the language-switch button.  Its callback updates
                        // layout_idx, key labels, and the amber highlight.
                        lang_btns_c.borrow_mut()[li].do_callback();
                    }
                    NavSel::Key(row, col) => {
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
                        // Re-apply amber in case execute_action changed the colour
                        // (e.g. when the selected button is a modifier key).
                        all_btns_c.borrow_mut()[row][col].0.set_color(col_nav_sel());
                    }
                }
                return true;
            }

            false
        });
    }

    win.end();
    win.show();

    a.run().unwrap();
}
