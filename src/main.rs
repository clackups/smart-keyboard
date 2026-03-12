mod config;
mod gamepad;
mod keyboards;
mod narrator;
mod output;

use std::cell::RefCell;
use std::rc::Rc;

use fltk::{
    app,
    button::Button,
    enums::{Align, Color, Event, FrameType, Key},
    frame::Frame,
    group::Group,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

use keyboards::{
    is_modifier, is_sticky, special_hook_str, special_label,
    Action, KW, KEYS, LAYOUTS, REGULAR_KEY_COUNT,
};

use gamepad::{Gamepad, GamepadAction, GamepadEvent};
use narrator::Narrator;

// =============================================================================
// Key hooks
// =============================================================================

/// Receives key-press and key-release notifications from the on-screen keyboard.
///
/// `scancode` is the Linux evdev key code (linux/input-event-codes.h).
/// `key` is the inserted string (e.g. "a", "\n") or a descriptor token for
/// non-printing keys ("Backspace", "LShift", "CapsLock", etc.).
///
/// There are two kinds of notifications:
///
/// * **Raw events** – `on_key_press` / `on_key_release` fire for every GUI
///   push/release event (mouse button down, mouse button up).  They may fire
///   twice per logical key action when the keyboard is operated by mouse or
///   touch (once from the widget's raw event handler and once from the
///   action-execution callback).  These are provided for hooks that need
///   immediate, low-latency feedback (e.g. audio click).
///
/// * **Action events** – `on_key_action` fires exactly once per logical key
///   action, after modifier state has been resolved.  `modifier_bits` carries
///   the USB HID modifier byte that was active at the time of the action:
///     bit 0 (0x01) = LEFTCTRL
///     bit 1 (0x02) = LEFTSHIFT
///     bit 2 (0x04) = LEFTALT
///     bit 5 (0x20) = RIGHTSHIFT
///     bit 6 (0x40) = RIGHTALT (AltGr)
///   This is the correct callback to use for hardware output (uinput, BLE, …).
///
/// The default implementation of `on_key_action` calls `on_key_press` followed
/// by `on_key_release`, preserving backwards compatibility for hooks that only
/// implement those two methods.
pub trait KeyHook {
    fn on_key_press(&self, scancode: u16, key: &str);
    fn on_key_release(&self, scancode: u16, key: &str);

    /// Called exactly once per logical key action from `execute_action` (on
    /// physical key press).  `on_key_release` is called separately when the
    /// physical activation key or button is released.
    ///
    /// Default: delegates to `on_key_press` only.
    fn on_key_action(&self, scancode: u16, key: &str, modifier_bits: u8) {
        let _ = modifier_bits; // unused in default delegation
        self.on_key_press(scancode, key);
        // NOTE: on_key_release is driven by the physical key-up event, not here.
    }
}

/// No-op hook: logs every action to stderr.  Used when no output is configured.
pub struct DummyKeyHook;

impl KeyHook for DummyKeyHook {
    fn on_key_press(&self, scancode: u16, key: &str) {
        eprintln!("[key_press]   scancode=0x{:02x} key={:?}", scancode, key);
    }
    fn on_key_release(&self, scancode: u16, key: &str) {
        eprintln!("[key_release] scancode=0x{:02x} key={:?}", scancode, key);
    }
}

// =============================================================================
// Menu
// =============================================================================

/// A single entry in the application pop-up menu.
///
/// Each item has a human-readable `label`, an `is_enabled` closure that is
/// evaluated at menu-open time to decide whether the item can be selected, and
/// an `execute` closure that is called when the item is activated by the user.
///
/// Add new items to the `menu_item_defs` `Vec` inside `main` to extend the
/// menu in the future.
pub struct MenuItemDef {
    pub label:      &'static str,
    is_enabled: Box<dyn Fn() -> bool>,
    execute:    Box<dyn Fn()>,
}

/// Return the index of the first enabled item in `items`, or `None` if all
/// items are disabled (or the list is empty).
fn menu_first_enabled(items: &[MenuItemDef]) -> Option<usize> {
    items.iter().position(|it| (it.is_enabled)())
}

/// Starting from `current`, scan in direction `dir` (+1 = down, -1 = up) for
/// the next enabled item.  Returns `current` unchanged if no other enabled
/// item exists in that direction (the cursor stays put at the edge).
fn menu_move_sel(current: usize, dir: i32, items: &[MenuItemDef]) -> usize {
    let n = items.len() as i32;
    let mut i = current as i32 + dir;
    while i >= 0 && i < n {
        if (items[i as usize].is_enabled)() {
            return i as usize;
        }
        i += dir;
    }
    current
}

/// Refresh the background and label colours of every menu item button to
/// reflect the current selection and enabled state.
fn menu_set_item_colors(
    sel:    Option<usize>,
    items:  &[MenuItemDef],
    btns:   &mut [Button],
    colors: Colors,
) {
    for (i, btn) in btns.iter_mut().enumerate() {
        let enabled = (items[i].is_enabled)();
        if Some(i) == sel {
            btn.set_color(colors.nav_sel);
            btn.set_label_color(colors.key_label_normal);
        } else if enabled {
            btn.set_color(colors.key_mod);
            btn.set_label_color(colors.key_label_mod);
        } else {
            btn.set_color(colors.status_ind_bg);
            btn.set_label_color(colors.status_ind_text);
        }
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
// UI colour palette (resolved from config)
// =============================================================================

/// All UI colours resolved from [`config::ColorsConfig`] into FLTK [`Color`] values.
/// Implements `Copy` because [`Color`] is a newtype over `u32`.
#[derive(Clone, Copy)]
struct Colors {
    key_normal:              Color,
    key_mod:                 Color,
    mod_active:              Color,
    nav_sel:                 Color,
    status_bar_bg:           Color,
    status_ind_bg:           Color,
    status_ind_text:         Color,
    status_ind_active_text:  Color,
    conn_disconnected:       Color,
    conn_connecting:         Color,
    conn_connected:          Color,
    win_bg:                  Color,
    disp_bg:                 Color,
    disp_text:               Color,
    lang_btn_inactive:       Color,
    lang_btn_label:          Color,
    key_label_normal:        Color,
    key_label_mod:           Color,
}

impl Colors {
    fn from_config(cfg: &config::ColorsConfig) -> Self {
        let c = |rgb: &config::ColorRgb| Color::from_rgb(rgb.0, rgb.1, rgb.2);
        Colors {
            key_normal:              c(&cfg.key_normal),
            key_mod:                 c(&cfg.key_mod),
            mod_active:              c(&cfg.mod_active),
            nav_sel:                 c(&cfg.nav_sel),
            status_bar_bg:           c(&cfg.status_bar_bg),
            status_ind_bg:           c(&cfg.status_ind_bg),
            status_ind_text:         c(&cfg.status_ind_text),
            status_ind_active_text:  c(&cfg.status_ind_active_text),
            conn_disconnected:       c(&cfg.conn_disconnected),
            conn_connecting:         c(&cfg.conn_connecting),
            conn_connected:          c(&cfg.conn_connected),
            win_bg:                  c(&cfg.win_bg),
            disp_bg:                 c(&cfg.disp_bg),
            disp_text:               c(&cfg.disp_text),
            lang_btn_inactive:       c(&cfg.lang_btn_inactive),
            lang_btn_label:          c(&cfg.lang_btn_label),
            key_label_normal:        c(&cfg.key_label_normal),
            key_label_mod:           c(&cfg.key_label_mod),
        }
    }
}

// =============================================================================
// Modifier button descriptor
// =============================================================================

/// A modifier-key button together with its action and base (inactive) color.
/// Stored in a shared list so execute_action can update visual state.
struct ModBtn {
    btn:      Button,
    action:   Action,
    base_col: Color,
    /// Corresponding status-bar indicator frame (shared between LShift & RShift).
    status:   Option<Frame>,
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

/// Apply a specific navigation selection, updating highlight colours.
///
/// Does nothing if `new_sel` equals the current `*sel`.
/// Returns `true` if the selection changed, `false` if it was already at `new_sel`.
fn nav_set(
    all_btns:   &mut Vec<Vec<(Button, Action, u16, Color)>>,
    lang_btns:  &mut Vec<Button>,
    layout_idx: usize,
    sel:        &mut NavSel,
    mod_state:  &Rc<RefCell<ModState>>,
    new_sel:    NavSel,
    colors:     Colors,
) -> bool {
    // Nothing to do when already at the target.
    if new_sel == *sel {
        return false;
    }

    // Restore the old selection's colour.
    match *sel {
        NavSel::Lang(li) => {
            let c = if li == layout_idx { colors.mod_active }
                    else                { colors.lang_btn_inactive };
            lang_btns[li].set_color(c);
        }
        NavSel::Key(row, col) => {
            let old_action = all_btns[row][col].1;
            let old_base   = all_btns[row][col].3;
            let c = if is_modifier(old_action) && mod_state.borrow().is_active(old_action) {
                colors.mod_active
            } else {
                old_base
            };
            all_btns[row][col].0.set_color(c);
        }
    }

    // Highlight the new selection and give it keyboard focus.
    match new_sel {
        NavSel::Lang(li) => {
            lang_btns[li].set_color(colors.nav_sel);
            let _ = lang_btns[li].take_focus();
        }
        NavSel::Key(row, col) => {
            all_btns[row][col].0.set_color(colors.nav_sel);
            let _ = all_btns[row][col].0.take_focus();
        }
    }

    *sel = new_sel;
    app::redraw();
    true
}

/// Move the keyboard-navigation cursor and update highlight colours.
///
/// When `rollover` is `false`, navigation clamps at all edges (no wrap-around).
/// When `rollover` is `true`, moving past the edge of the keyboard wraps the
/// selection to the opposite edge.
/// The cursor can move between the language-button strip and the keyboard grid;
/// vertical transitions are pixel-centre aligned so wide keys map naturally.
///
/// Returns `true` if the selection actually changed, `false` if it was already
/// at the edge in the requested direction (only possible when rollover is false).
fn nav_move(
    all_btns:   &mut Vec<Vec<(Button, Action, u16, Color)>>,
    lang_btns:  &mut Vec<Button>,
    layout_idx: usize,
    sel:        &mut NavSel,
    mod_state:  &Rc<RefCell<ModState>>,
    dr:         i32,
    dc:         i32,
    colors:     Colors,
    rollover:   bool,
) -> bool {
    let new_sel: NavSel = match *sel {
        NavSel::Lang(li) => {
            if dr < 0 {
                if rollover {
                    // Up from the lang strip wraps to the last keyboard row.
                    let rows = all_btns.len();
                    if rows == 0 {
                        NavSel::Lang(li)
                    } else {
                        let cx = lang_btns[li].x() + lang_btns[li].w() / 2;
                        NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                    }
                } else {
                    // Already at the top edge.
                    NavSel::Lang(li)
                }
            } else if dr > 0 {
                // Down into the first keyboard row, pixel-aligned.
                let cx = lang_btns[li].x() + lang_btns[li].w() / 2;
                NavSel::Key(0, closest_col(&all_btns[0], cx))
            } else {
                // Left / right within the lang strip.
                let lc = lang_btns.len();
                if rollover {
                    NavSel::Lang((li as i32 + dc).rem_euclid(lc as i32) as usize)
                } else {
                    NavSel::Lang((li as i32 + dc).clamp(0, lc as i32 - 1) as usize)
                }
            }
        }
        NavSel::Key(row, col) => {
            if dr < 0 && row == 0 {
                if !lang_btns.is_empty() {
                    // Up from the top keyboard row → lang strip, pixel-aligned.
                    let cx = all_btns[0][col].0.x() + all_btns[0][col].0.w() / 2;
                    NavSel::Lang(closest_lang(lang_btns, cx))
                } else if rollover {
                    // No lang strip: wrap to the last keyboard row.
                    let rows = all_btns.len();
                    let cx   = all_btns[0][col].0.x() + all_btns[0][col].0.w() / 2;
                    NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                } else {
                    NavSel::Key(row, col)
                }
            } else if dr != 0 {
                let rows = all_btns.len();
                let cx   = all_btns[row][col].0.x() + all_btns[row][col].0.w() / 2;
                // Scan rows in direction dr, skipping any row where no button
                // is close to cx.  This makes navigation skip rows that have no
                // keys in the nav cluster (e.g. the home row has no nav-cluster
                // buttons, so Down from End should pass over it and land on ↑).
                // "Close enough" = the nearest edge of the best button in that
                // row is within one button-width of cx.
                let mut scan     = row;
                let mut dest_row = row; // will be updated when we find a close row
                loop {
                    let next_raw = scan as i32 + dr;
                    // Check if we've gone past the edge.
                    if next_raw < 0 || next_raw >= rows as i32 {
                        if rollover {
                            // Wrap: going down past last row → lang strip (if any) or stay.
                            // Going up past row 0 is already handled above (row == 0 case).
                            // This branch handles dr > 0 past last row.
                            if dr > 0 {
                                if !lang_btns.is_empty() {
                                    dest_row = rows; // sentinel: means "go to lang strip"
                                } else {
                                    // Wrap to first row.
                                    dest_row = rows + 1; // sentinel: means "wrap to row 0"
                                }
                            }
                        }
                        break;
                    }
                    scan = next_raw as usize;
                    let best_col = closest_col(&all_btns[scan], cx);
                    let btn      = &all_btns[scan][best_col].0;
                    let dist = if cx >= btn.x() && cx < btn.x() + btn.w() {
                        0
                    } else if cx < btn.x() {
                        btn.x() - cx
                    } else {
                        cx - (btn.x() + btn.w())
                    };
                    if dist <= btn.w() {
                        dest_row = scan;
                        break;
                    }
                    // Too far – keep scanning.
                }
                if dest_row == rows {
                    // Sentinel: wrap to lang strip.
                    NavSel::Lang(closest_lang(lang_btns, cx))
                } else if dest_row == rows + 1 {
                    // Sentinel: wrap to first row.
                    NavSel::Key(0, closest_col(&all_btns[0], cx))
                } else if dest_row == row {
                    NavSel::Key(row, col) // clamped at edge
                } else {
                    NavSel::Key(dest_row, closest_col(&all_btns[dest_row], cx))
                }
            } else {
                // Left / right within the current keyboard row.
                let rl = all_btns[row].len();
                if rollover {
                    NavSel::Key(row, (col as i32 + dc).rem_euclid(rl as i32) as usize)
                } else {
                    let new_col = (col as i32 + dc).clamp(0, rl as i32 - 1) as usize;
                    NavSel::Key(row, new_col)
                }
            }
        }
    };

    nav_set(all_btns, lang_btns, layout_idx, sel, mod_state, new_sel, colors)
}

/// Compute the center `NavSel` for the current keyboard layout.
///
/// Mirrors the `AbsolutePos { horiz: 0.5, vert: 0.5 }` logic: maps the
/// midpoint of the normalized 0.0–1.0 coordinate space to a row / column.
/// The language strip (if present) is included as band 0; the keyboard rows
/// occupy the remaining bands.
fn nav_center(
    all_btns:  &[Vec<(Button, Action, u16, Color)>],
    lang_btns: &[Button],
) -> Option<NavSel> {
    /// Normalized coordinate for the center of the selectable area.
    const CENTER: f32 = 0.5;
    let num_rows = all_btns.len();
    let num_lang = lang_btns.len();
    if num_rows == 0 {
        return None;
    }
    let has_lang    = num_lang > 0;
    let total_bands = if has_lang { 1 + num_rows } else { num_rows };
    let band  = (CENTER * total_bands as f32)
        .floor()
        .clamp(0.0, total_bands as f32 - 1.0) as usize;
    if has_lang && band == 0 {
        let li = (CENTER * num_lang as f32)
            .floor()
            .clamp(0.0, num_lang as f32 - 1.0) as usize;
        Some(NavSel::Lang(li))
    } else {
        let row      = if has_lang { band - 1 } else { band };
        let num_cols = all_btns[row].len();
        let col      = (CENTER * num_cols as f32)
            .floor()
            .clamp(0.0, num_cols as f32 - 1.0) as usize;
        Some(NavSel::Key(row, col))
    }
}

// =============================================================================
// Action execution
// =============================================================================

/// Perform the action of a key: notify hooks, insert text, update modifiers.
///
/// `mod_btns` is the list of modifier buttons so their visual state can be
/// updated when a modifier is toggled or a sticky modifier auto-releases.
///
/// Returns the `key_str` that was passed to `on_key_action`, so the caller
/// can invoke `on_key_release` when the physical activation key is later
/// released.
fn execute_action(
    action:    Action,
    scancode:  u16,
    layout_i:  usize,
    buf:       &mut TextBuffer,
    disp:      &mut TextDisplay,
    hook:      &Rc<dyn KeyHook>,
    mod_state: &Rc<RefCell<ModState>>,
    mod_btns:  &[ModBtn],
    colors:    Colors,
) -> String {
    // For Regular keys, compute the text to insert respecting Shift / CapsLock.
    // Symbol/number keys (label_shifted != "") use the shifted character on LShift/RShift;
    // CapsLock does NOT affect them (standard keyboard behaviour).
    // Letter keys (label_shifted == "") use to_uppercase() for any of Caps/LShift/RShift.
    let regular_text: Option<String> = if let Action::Regular(slot) = action {
        let key = &LAYOUTS[layout_i].keys[slot];
        let ms  = mod_state.borrow();
        Some(if !key.label_shifted.is_empty() && (ms.lshift || ms.rshift) {
            key.insert_shifted.to_string()
        } else if key.label_shifted.is_empty() && (ms.caps || ms.lshift || ms.rshift) {
            key.insert_unshifted.to_uppercase()
        } else {
            key.insert_unshifted.to_string()
        })
    } else {
        None
    };

    let key_str: &str = match action {
        Action::Regular(_) => regular_text.as_deref().unwrap_or(""),
        other              => special_hook_str(other),
    };

    // Capture modifier state BEFORE any toggles or auto-releases so that
    // on_key_action receives the bits that were active when the key was pressed.
    // Bit layout (matches USB HID modifier byte):
    //   0x01 = Ctrl (left), 0x02 = LShift, 0x04 = Alt (left),
    //   0x20 = RShift,       0x40 = AltGr (right alt)
    let modifier_bits: u8 = {
        let ms = mod_state.borrow();
        (if ms.ctrl   { 0x01 } else { 0 })
            | (if ms.lshift { 0x02 } else { 0 })
            | (if ms.alt    { 0x04 } else { 0 })
            | (if ms.rshift { 0x20 } else { 0 })
            | (if ms.altgr  { 0x40 } else { 0 })
    };

    if is_modifier(action) {
        // Toggle the modifier and refresh the color of its button(s).
        let now_active = mod_state.borrow_mut().toggle(action);
        for m in mod_btns {
            if m.action == action {
                m.btn.clone().set_color(if now_active { colors.mod_active } else { m.base_col });
                if let Some(mut sf) = m.status.clone() {
                    sf.set_color(if now_active { colors.mod_active } else { colors.status_ind_bg });
                    sf.set_label_color(if now_active { colors.status_ind_active_text } else { colors.status_ind_text });
                }
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
                    if let Some(mut sf) = m.status.clone() {
                        sf.set_color(colors.status_ind_bg);
                        sf.set_label_color(colors.status_ind_text);
                    }
                }
            }
            ms.release_sticky();
        }
        app::redraw();
    }

    hook.on_key_action(scancode, key_str, modifier_bits);
    key_str.to_string()
}

// =============================================================================
// Audio narration slugs and tone frequencies
// =============================================================================

/// Map a label string to a filesystem-safe slug used as the WAV filename stem.
///
/// * ASCII alphanumerics are kept as-is.
/// * Known ASCII punctuation is mapped to descriptive words.
/// * Any other character (e.g. Cyrillic) is encoded as `uXXXX`.
fn label_to_audio_slug(label: &str) -> String {
    label
        .chars()
        .map(|c| match c {
            '`'  => "backtick".to_string(),
            '-'  => "minus".to_string(),
            '='  => "equals".to_string(),
            '['  => "lbracket".to_string(),
            ']'  => "rbracket".to_string(),
            '\\' => "backslash".to_string(),
            ';'  => "semicolon".to_string(),
            '\'' => "apostrophe".to_string(),
            ','  => "comma".to_string(),
            '.'  => "period".to_string(),
            '/'  => "slash".to_string(),
            _ if c.is_ascii_alphanumeric() => c.to_string(),
            _    => format!("u{:04x}", c as u32),
        })
        .collect()
}

/// Return the audio-file slug for an [`Action`] in the given layout.
///
/// The slug is the stem of the corresponding `.wav` file inside the audio
/// directory.  Returns an empty string for actions that carry no narration
/// (e.g. [`Action::Noop`]).
fn action_audio_slug(action: Action, layout_idx: usize) -> String {
    match action {
        Action::Regular(slot) => {
            let layout_name = keyboards::LAYOUTS[layout_idx].name.to_lowercase();
            let label = keyboards::LAYOUTS[layout_idx].keys[slot].label_unshifted;
            format!("{}_{}", layout_name, label_to_audio_slug(label))
        }
        Action::Backspace  => "backspace".to_string(),
        Action::Tab        => "tab".to_string(),
        Action::CapsLock   => "capslock".to_string(),
        Action::Enter      => "enter".to_string(),
        Action::LShift | Action::RShift => "shift".to_string(),
        Action::Ctrl       => "ctrl".to_string(),
        Action::Win        => "win".to_string(),
        Action::Alt        => "alt".to_string(),
        Action::AltGr      => "altgr".to_string(),
        Action::Space      => "space".to_string(),
        Action::Esc        => "esc".to_string(),
        Action::F1         => "f1".to_string(),
        Action::F2         => "f2".to_string(),
        Action::F3         => "f3".to_string(),
        Action::F4         => "f4".to_string(),
        Action::F5         => "f5".to_string(),
        Action::F6         => "f6".to_string(),
        Action::F7         => "f7".to_string(),
        Action::F8         => "f8".to_string(),
        Action::F9         => "f9".to_string(),
        Action::F10        => "f10".to_string(),
        Action::F11        => "f11".to_string(),
        Action::F12        => "f12".to_string(),
        Action::Insert     => "insert".to_string(),
        Action::Delete     => "delete".to_string(),
        Action::Home       => "home".to_string(),
        Action::End        => "end".to_string(),
        Action::PageUp     => "pageup".to_string(),
        Action::PageDown   => "pagedown".to_string(),
        Action::ArrowUp    => "up".to_string(),
        Action::ArrowDown  => "down".to_string(),
        Action::ArrowLeft  => "left".to_string(),
        Action::ArrowRight => "right".to_string(),
        Action::Noop       => String::new(),
    }
}

/// Return the audio-file slug for the current navigation selection.
fn nav_audio_slug(
    sel: NavSel,
    layout_idx: usize,
    all_btns: &[Vec<(Button, Action, u16, Color)>],
) -> String {
    match sel {
        NavSel::Lang(li) => {
            format!("lang_{}", keyboards::LAYOUTS[li].name.to_lowercase())
        }
        NavSel::Key(row, col) => action_audio_slug(all_btns[row][col].1, layout_idx),
    }
}

/// Return the tone frequency (Hz) for an [`Action`] in tone audio mode.
///
/// Key categories and their pitches:
///
/// 1. **All letter / punctuation keys, except F and J** → A5 (880 Hz).
/// 2. **F and J** (the physical home-row bump keys, slots 29 & 32)
///    → B5 (988 Hz) — a distinctive major second above the other letters.
/// 3. **Digit keys 1–0** → ascending C-major scale C4–E5 (261–659 Hz),
///    with 1 the lowest pitch and 0 the highest.
/// 4. **Function keys F1–F12** → ascending A-minor pentatonic A1–B3
///    (55–247 Hz), clearly in the bass register below the digit range.
/// 5. **All other keys** (Space, Enter, modifiers, arrows, …) → each key
///    has its own unique pitch chosen for pleasant distinctiveness.
///
/// Returns `0.0` for [`Action::Noop`] (no tone should be played).
fn tone_freq_for_action(action: Action) -> f32 {
    match action {
        // --- Regular keys ---
        Action::Regular(slot) => match slot {
            // Digit row: slots 1-10 → 1,2,3,4,5,6,7,8,9,0
            // Ascending C-major scale C4..E5 (pitch rises from 1 to 0).
            1  => 261.63,  // C4
            2  => 293.66,  // D4
            3  => 329.63,  // E4
            4  => 349.23,  // F4
            5  => 392.00,  // G4
            6  => 440.00,  // A4
            7  => 493.88,  // B4
            8  => 523.25,  // C5
            9  => 587.33,  // D5
            10 => 659.26,  // E5  (key "0")
            // F and J – physical home-row bump keys
            29 | 32 => 987.77,  // B5
            // All other letter / punctuation keys
            _ => 880.00,        // A5
        },
        // --- Function keys: ascending A-minor pentatonic A1..B3 ---
        Action::F1  =>  55.00,   // A1
        Action::F2  =>  65.41,   // C2
        Action::F3  =>  73.42,   // D2
        Action::F4  =>  82.41,   // E2
        Action::F5  =>  98.00,   // G2
        Action::F6  => 110.00,   // A2
        Action::F7  => 130.81,   // C3
        Action::F8  => 146.83,   // D3
        Action::F9  => 164.81,   // E3
        Action::F10 => 196.00,   // G3
        Action::F11 => 220.00,   // A3
        Action::F12 => 246.94,   // B3
        // --- Special keys ---
        Action::Esc        => 1760.00,  // A6 – high/urgent
        Action::Backspace  =>  415.30,  // Ab4
        Action::Tab        =>  369.99,  // F#4
        Action::CapsLock   =>  932.33,  // Bb5
        Action::Enter      =>  554.37,  // C#5
        Action::LShift | Action::RShift => 311.13,  // Eb4
        Action::Ctrl       =>  277.18,  // Db4
        Action::Win        =>  174.61,  // F3
        Action::Alt        =>  233.08,  // Bb3
        Action::AltGr      =>  207.65,  // Ab3
        Action::Space      => 1046.50,  // C6
        Action::Insert     => 1318.51,  // E6
        Action::Delete     => 1244.51,  // Eb6
        Action::Home       => 1174.66,  // D6
        Action::End        => 1108.73,  // Db6
        Action::PageUp     => 1396.91,  // F6
        Action::PageDown   => 1567.98,  // G6
        Action::ArrowUp    =>  783.99,  // G5
        Action::ArrowDown  =>  739.99,  // F#5
        Action::ArrowLeft  =>  698.46,  // F5
        Action::ArrowRight =>  622.25,  // Eb5
        Action::Noop       =>    0.00,
    }
}

/// Return the tone frequency (Hz) for an [`Action`] in `tone_hint` audio mode.
///
/// Identical to [`tone_freq_for_action`] except that all letter and punctuation
/// keys are silent (0.0 Hz), with the exception of **F** (slot 29) and **J**
/// (slot 32) which retain their distinctive B5 (987.77 Hz) pitch as a home-row
/// position hint.  Digit keys (slots 1–10) and all non-`Regular` actions keep
/// their pitches unchanged.
fn tone_hint_freq_for_action(action: Action) -> f32 {
    match action {
        Action::Regular(slot) => match slot {
            // Digit keys: same ascending scale as in tone mode.
            1..=10 => tone_freq_for_action(action),
            // F and J – home-row bump keys: play a distinctive tone.
            29 | 32 => 987.77,  // B5
            // All other letter / punctuation keys: silent.
            _ => 0.0,
        },
        // Function keys and all special keys: unchanged from tone mode.
        _ => tone_freq_for_action(action),
    }
}

/// Return the tone frequency (Hz) for `action` under the given [`AudioMode`].
///
/// Delegates to [`tone_hint_freq_for_action`] for [`AudioMode::ToneHint`] and
/// to [`tone_freq_for_action`] for all other modes.
fn action_tone_hz(action: Action, mode: &config::AudioMode) -> f32 {
    match mode {
        config::AudioMode::ToneHint => tone_hint_freq_for_action(action),
        _ => tone_freq_for_action(action),
    }
}

/// Tone frequency (Hz) used for language-toggle buttons in tone mode.
/// E4 = 329.63 Hz – a neutral, distinctive pitch in the mid register.
const LANG_BTN_TONE_HZ: f32 = 329.63;

/// Return the tone frequency (Hz) for the current navigation selection.
///
/// Language-toggle buttons use [`LANG_BTN_TONE_HZ`] as a neutral, distinctive tone.
fn nav_tone_freq(
    sel: NavSel,
    all_btns: &[Vec<(Button, Action, u16, Color)>],
    mode: &config::AudioMode,
) -> f32 {
    match sel {
        NavSel::Lang(_) => LANG_BTN_TONE_HZ,
        NavSel::Key(row, col) => action_tone_hz(all_btns[row][col].1, mode),
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    debug_assert!(
        LAYOUTS.iter().all(|l| l.keys.len() == REGULAR_KEY_COUNT),
        "every LayoutDef must have exactly REGULAR_KEY_COUNT entries"
    );

    let cfg = config::Config::load();
    let nav_keys = config::NavKeys::from_config(&cfg.input.keyboard);
    let colors = Colors::from_config(&cfg.ui.colors);

    // Build the narrator early so it can be cloned into closures below.
    let narrator = Rc::new(RefCell::new(Narrator::new(cfg.output.audio.clone())));
    // Clone the audio mode so it can be captured independently by closures.
    let audio_mode = cfg.output.audio.clone();

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
    win.set_color(colors.win_bg);
    win.set_border(false); // remove title bar / window decorations
    win.fullscreen(true);

    let pad  = 3i32;
    let gap  =  1i32;

    let avail_w = sw - 2 * pad;

    let display_h  = ((sh as f32 / 12.0) as i32).max(10);
    let lang_btn_h = ((sh as f32 / 12.0) as i32).max(10);

    // Status bar occupies a thin strip at the very top of the window.
    let status_h = (sh / 24).max(18).min(32);

    let kbd_h = (sh - status_h - display_h - lang_btn_h - 2 * pad - gap).min(avail_w / 3);
    // 6 rows (F-keys + 5 QWERTY rows), 5 inter-row gaps
    let key_h = (kbd_h - 5 * gap) / 6;

    let pad_top = status_h + pad + (sh - status_h - display_h - lang_btn_h - 6 * key_h - 7 * gap) / 2;
    let kbd_y = pad_top + display_h + lang_btn_h + 2 * gap;

    // Ortholinear: every key is key_w wide.
    // The widest rows (number row and QWERTY row) are 18 slots wide:
    //   14 main keys + 1 Spacer + 3 nav keys → 17*(key_w+gap) - gap = avail_w
    //   key_w = (avail_w - 17*gap) / 17
    // Bottom row: Ctrl Win Alt [Space] AltGr Ctrl Spacer ← ↓ → = 9 non-Space slots
    //   Space spans exactly 6 grid columns: space_w = 6*key_w + 5*gap
    //   (Pinning to exact grid avoids integer-division remainder bleeding into the
    //   spacebar width; the row may be a few pixels narrower than avail_w.)
    let key_w   = ((avail_w - 16 * gap) / 17).max(10);
    let space_w = 6 * key_w + 5 * gap;
    let pad_left = pad + (avail_w - 17 * key_w - 16 * gap)/2;

    let px = |kw: KW| match kw {
        KW::Space            => space_w,
        KW::Std | KW::Spacer => key_w,
    };

    // --- Font sizes ---
    // Drive label size from key width so the longest labels ("AltGr", "Enter",
    // "Shift") stay within the button boundary.  key_w/4 gives ~25% horizontal
    // margin for a 5-character label in a proportional font.
    let lbl_size  = (key_w / 4).max(10);
    // Buttons that show only a single character get a larger font so they are
    // easier to read at a glance (single letters / digits / symbols).
    let big_lbl_size = lbl_size * 2;
    let disp_size = ((display_h * 2 / 5) as i32).max(12).min(28);
    // Lang buttons are one grid column wide (key_w); reuse lbl_size so their
    // text labels fit with the same margin as keyboard-key labels.
    let btn_size  = lbl_size;

    // --- Status bar (top strip) ---
    // Label size: 3/4 of key label size, at least 8 px.
    let status_lbl_size = (lbl_size * 3 / 4).max(8);
    // Each indicator is wide enough for a 5-character label ("ALTGR") plus margin.
    let ind_gap = 4i32;
    let ind_pad = 2i32;   // top/bottom padding inside the status bar strip
    let ind_h   = status_h - 2 * ind_pad;
    let ind_w   = status_lbl_size * 4;

    let mut _status_bar_bg = Frame::new(0, 0, sw, status_h, "");
    _status_bar_bg.set_color(colors.status_bar_bg);
    _status_bar_bg.set_frame(FrameType::FlatBox);

    // Helper: build one status-bar indicator frame.
    let make_ind = |x: i32, label: &'static str| {
        let mut f = Frame::new(x, ind_pad, ind_w, ind_h, label);
        f.set_color(colors.status_ind_bg);
        f.set_label_color(colors.status_ind_text);
        f.set_frame(FrameType::FlatBox);
        f.set_label_size(status_lbl_size);
        f
    };

    let mut ind_x = ind_gap;
    let caps_status  = make_ind(ind_x, "CAPS");  ind_x += ind_w + ind_gap;
    let shift_status = make_ind(ind_x, "SHIFT"); ind_x += ind_w + ind_gap;
    let ctrl_status  = make_ind(ind_x, "CTRL");  ind_x += ind_w + ind_gap;
    let alt_status   = make_ind(ind_x, "ALT");   ind_x += ind_w + ind_gap;
    let altgr_status = make_ind(ind_x, "ALTGR");

    // Right-side status icons, built right-to-left:
    //   [gamepad icon (if enabled)] [BLE icon (if ble mode)]
    //
    // BLE icon colours:
    //   Green  = BLE dongle found and port open.
    //   Yellow = BLE mode configured but dongle not found.
    // Gamepad icon colours:
    //   Green "G" = gamepad device connected.
    //   Red   "G" = gamepad device not found / disconnected.
    let ble_mode = matches!(cfg.output.mode, config::OutputMode::Ble);

    // BLE connectivity icon – rightmost, only shown when output mode is BLE.
    let conn_x = sw - ind_gap - ind_w;
    let mut conn_status = Frame::new(conn_x, ind_pad, ind_w, ind_h, "●");
    conn_status.set_color(colors.status_ind_bg);
    conn_status.set_label_color(colors.conn_disconnected); // initial: disconnected
    conn_status.set_frame(FrameType::FlatBox);
    conn_status.set_label_size(status_lbl_size);
    if !ble_mode {
        conn_status.hide();
    }

    // Gamepad icon – left of BLE icon when BLE is shown, otherwise rightmost.
    // Only created when gamepad input is enabled in config.
    let gp_icon_x = if ble_mode { conn_x - ind_gap - ind_w } else { conn_x };
    let mut gamepad_status: Option<Frame> = if cfg.input.gamepad.enabled {
        let mut f = Frame::new(gp_icon_x, ind_pad, ind_w, ind_h, "G");
        f.set_color(colors.status_ind_bg);
        f.set_label_color(colors.conn_disconnected); // initial: disconnected (red G)
        f.set_frame(FrameType::FlatBox);
        f.set_label_size(status_lbl_size);
        Some(f)
    } else {
        None
    };

    // --- Shared state ---
    let layout_idx: Rc<RefCell<usize>>    = Rc::new(RefCell::new(0));
    let mod_state:  Rc<RefCell<ModState>> = Rc::new(RefCell::new(ModState::default()));
    // mod_btns is populated during the key loop; closures borrow it at call time.
    let mod_btns: Rc<RefCell<Vec<ModBtn>>> = Rc::new(RefCell::new(Vec::new()));
    // Tracks the (scancode, key_str) of the key currently "held" by the keyboard
    // activation key or gamepad action button.  Set on press, cleared on release.
    let active_nav_key: Rc<RefCell<Option<(u16, String)>>> = Rc::new(RefCell::new(None));
    let buf  = TextBuffer::default();

    // Build the output hook from the loaded configuration and update the
    // connectivity indicator accordingly.
    // `ble_conn_opt` holds a shared reference to the BLE connection when BLE
    // mode is active; used by the "Disconnect BLE" menu item.
    let mut ble_conn_opt: Option<std::rc::Rc<std::cell::RefCell<output::BleConnection>>> = None;
    let hook: Rc<dyn KeyHook> = match cfg.output.mode {
        config::OutputMode::Print => {
            Rc::new(output::PrintKeyHook)
        }
        config::OutputMode::Ble => {
            let ble_cfg = &cfg.output.ble;
            let (ble_hook, ble_conn) = output::BleKeyHook::new(
                ble_cfg.vid,
                ble_cfg.pid,
                ble_cfg.serial.clone(),
            );

            // Intervals for the BLE connection-management timer.
            const BLE_RETRY_INTERVAL_S:  f64 = 1.0; // retry to connect when disconnected
            const BLE_STATUS_INTERVAL_S: f64 = 1.0; // status check when connected

            // Three-valued state used to detect transitions for stdout logging.
            #[derive(PartialEq, Clone, Copy)]
            enum BleState { Disconnected, Connecting, Connected }

            // Timer that manages the BLE connection life-cycle:
            //
            //  * When disconnected: try to connect once per second.
            //    – On success → amber icon (connected, status not yet checked);
            //      schedule status check in 1 s.
            //    – On failure → red icon; retry in 1 s.
            //
            //  * When connected: send the "S" command every 1 s.
            //    – STATUS:CONNECTED:xxxx  → green icon; re-check in 1 s.
            //    – STATUS:NOTCONNECTED    → amber icon; log disconnect if
            //      previously connected; re-check in 1 s.
            //    – Other response / no response → amber icon; re-check in 1 s.
            //    – Write failure (connection lost) → red icon; retry in 1 s.
            //
            // State changes between Disconnected and Connected are printed to
            // stdout.  When the host MAC address is available it is included in
            // the "BLE connected" message.
            let mut conn_status_t = conn_status.clone();
            ble_conn_opt = Some(ble_conn.clone());
            let ble_conn_t = ble_conn;
            let ble_state = Rc::new(RefCell::new(BleState::Disconnected));
            app::add_timeout3(0.0, move |handle| {
                if !ble_conn_t.borrow().is_connected() {
                    if ble_conn_t.borrow_mut().try_connect() {
                        *ble_state.borrow_mut() = BleState::Connecting;
                        conn_status_t.set_label_color(colors.conn_connecting);
                        app::redraw();
                        app::repeat_timeout3(BLE_STATUS_INTERVAL_S, handle);
                    } else {
                        if *ble_state.borrow() != BleState::Disconnected {
                            println!("BLE disconnected");
                        }
                        *ble_state.borrow_mut() = BleState::Disconnected;
                        conn_status_t.set_label_color(colors.conn_disconnected);
                        app::redraw();
                        app::repeat_timeout3(BLE_RETRY_INTERVAL_S, handle);
                    }
                } else {
                    match ble_conn_t.borrow_mut().check_status() {
                        Err(()) => {
                            // Write failed → connection lost.
                            if *ble_state.borrow() != BleState::Disconnected {
                                println!("BLE disconnected");
                            }
                            *ble_state.borrow_mut() = BleState::Disconnected;
                            conn_status_t.set_label_color(colors.conn_disconnected);
                            app::redraw();
                            app::repeat_timeout3(BLE_RETRY_INTERVAL_S, handle);
                        }
                        Ok(Some(ref s)) if s.starts_with("STATUS:CONNECTED:") => {
                            if *ble_state.borrow() != BleState::Connected {
                                let mac = s.trim_start_matches("STATUS:CONNECTED:").trim();
                                println!("BLE connected: {}", mac);
                            }
                            *ble_state.borrow_mut() = BleState::Connected;
                            conn_status_t.set_label_color(colors.conn_connected);
                            app::redraw();
                            app::repeat_timeout3(BLE_STATUS_INTERVAL_S, handle);
                        }
                        Ok(Some(ref s)) if s.starts_with("STATUS:NOTCONNECTED") => {
                            // The dongle is reachable but the BLE link to the
                            // remote host has been lost.
                            if *ble_state.borrow() == BleState::Connected {
                                println!("BLE disconnected");
                            }
                            *ble_state.borrow_mut() = BleState::Connecting;
                            conn_status_t.set_label_color(colors.conn_connecting);
                            app::redraw();
                            app::repeat_timeout3(BLE_STATUS_INTERVAL_S, handle);
                        }
                        Ok(_) => {
                            // Connected but remote host not paired / not ready.
                            *ble_state.borrow_mut() = BleState::Connecting;
                            conn_status_t.set_label_color(colors.conn_connecting);
                            app::redraw();
                            app::repeat_timeout3(BLE_STATUS_INTERVAL_S, handle);
                        }
                    }
                }
            });

            Rc::new(ble_hook)
        }
    };

    // --- Menu item definitions ---
    //
    // Each `MenuItemDef` has a label, an `is_enabled` closure (checked at
    // menu-open time) and an `execute` closure (called on activation).
    // Add new items to this Vec to extend the menu in the future.
    let mut menu_item_defs: Vec<MenuItemDef> = Vec::new();

    // "Disconnect BLE": only available when BLE mode is active and the dongle
    // is currently connected.
    if ble_mode {
        if let Some(ref conn) = ble_conn_opt {
            let conn_check = conn.clone();
            let conn_exec  = conn.clone();
            menu_item_defs.push(MenuItemDef {
                label:      "Disconnect BLE",
                is_enabled: Box::new(move || conn_check.borrow().is_connected()),
                execute:    Box::new(move || {
                    if !conn_exec.borrow_mut().send_disconnect() {
                        eprintln!("[menu] Disconnect BLE: failed to send disconnect command");
                    }
                }),
            });
        }
    }

    let menu_item_defs: Rc<Vec<MenuItemDef>> = Rc::new(menu_item_defs);

    // --- Text display (read-only) ---
    let mut disp = TextDisplay::new(pad_left, pad_top, sw - 2 * pad_left, display_h, "");
    disp.set_buffer(buf.clone());
    disp.set_color(colors.disp_bg);
    disp.set_text_color(colors.disp_text);
    disp.set_frame(FrameType::DownBox);
    disp.set_text_size(disp_size);

    // --- Language toggle buttons (one per entry in LAYOUTS) ---
    let active_col   = colors.mod_active;
    let inactive_col = colors.lang_btn_inactive;

    let lang_y = pad_top + display_h + gap;
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
        let btn_x = pad_left + li as i32 * (lang_w + gap);
        let mut btn = Button::new(btn_x, lang_y, lang_w, lang_btn_h, def.name);
        btn.set_color(if li == 0 { active_col } else { inactive_col });
        btn.set_label_color(colors.lang_btn_label);
        btn.set_label_size(btn_size);

        let layout_idx_c  = layout_idx.clone();
        let lang_btns_c   = lang_btns.clone();
        let switch_btns_c = switch_btns.clone();
        let all_btns_c    = all_btns.clone();
        let sel_c         = sel.clone();
        let mod_state_c   = mod_state.clone();
        let narrator_c    = narrator.clone();
        btn.set_callback(move |_| {
            // Execute the language switch.
            *layout_idx_c.borrow_mut() = li;
            for (j, lb) in lang_btns_c.borrow_mut().iter_mut().enumerate() {
                lb.set_color(if j == li { active_col } else { inactive_col });
            }
            let def = LAYOUTS[li];
            for (kb, slot) in switch_btns_c.borrow_mut().iter_mut() {
                let key = &def.keys[*slot];
                if key.label_shifted.is_empty() {
                    kb.set_label(key.label_unshifted);
                    let sz = if key.label_unshifted.chars().count() == 1 { big_lbl_size } else { lbl_size };
                    kb.set_label_size(sz);
                } else {
                    let lbl = format!("{}\n{}", key.label_shifted, key.label_unshifted);
                    kb.set_label(&lbl);
                    kb.set_label_size(lbl_size);
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
                    colors.mod_active
                } else {
                    old_base
                };
                ab[old_r][old_c].0.set_color(restore);
            }
            // (If old_sel was Lang(_), the colour loop above already restored it.)
            lang_btns_c.borrow_mut()[li].set_color(colors.nav_sel);
            let _ = lang_btns_c.borrow_mut()[li].take_focus();
            *sel_c.borrow_mut() = NavSel::Lang(li);
            narrator_c.borrow_mut().play(
                &format!("lang_{}", LAYOUTS[li].name.to_lowercase()),
                LANG_BTN_TONE_HZ,
            );
            app::redraw();
        });
        lang_btns.borrow_mut().push(btn);
    }

    // --- Keyboard key grid ---
    // (all_btns and sel were declared before the lang-button loop above)

    for (row_i, row) in KEYS.iter().enumerate() {
        let row_y = kbd_y + row_i as i32 * (key_h + gap);
        let mut x = pad_left;
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
                Action::Regular(_) | Action::Space => colors.key_normal,
                _                                  => colors.key_mod,
            };

            let init_label: String = match phys.action {
                Action::Regular(slot) => {
                    let key = &LAYOUTS[0].keys[slot];
                    if key.label_shifted.is_empty() {
                        key.label_unshifted.to_string()
                    } else {
                        format!("{}\n{}", key.label_shifted, key.label_unshifted)
                    }
                }
                other => special_label(other).to_string(),
            };

            let mut btn = Button::new(x, row_y, w, key_h, None);
            btn.set_label(&init_label);
            let this_lbl_size = if init_label.chars().count() == 1 { big_lbl_size } else { lbl_size };
            btn.set_label_size(this_lbl_size);
            btn.set_color(base_col);
            if matches!(phys.action, Action::Regular(_) | Action::Space) {
                btn.set_label_color(colors.key_label_normal);
            } else {
                btn.set_label_color(colors.key_label_mod);
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
                            LAYOUTS[*layout_idx_h.borrow()].keys[slot].insert_unshifted
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
                let narrator_c   = narrator.clone();
                let audio_mode_c = audio_mode.clone();
                let action       = phys.action;
                let scancode     = phys.scancode;
                btn.set_callback(move |_| {
                    let key_str = execute_action(
                        action, scancode,
                        *layout_idx_c.borrow(),
                        &mut buf_c, &mut disp_c, &hook_c,
                        &mod_state_c,
                        &mod_btns_c.borrow(),
                        colors,
                    );
                    // For mouse/touch clicks the Released event fires before this
                    // callback, so the key press command was sent in execute_action →
                    // on_key_action.  Send the release immediately so the BLE dongle
                    // receives K0000 right after the press.
                    hook_c.on_key_release(scancode, &key_str);
                    // Move the amber highlight to the clicked button.
                    {
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
                                    colors.mod_active
                                } else {
                                    old_base
                                };
                                ab[old_r][old_c].0.set_color(restore);
                            }
                            NavSel::Lang(li) => {
                                let restore = if li == *layout_idx_c.borrow() {
                                    colors.mod_active
                                } else {
                                    colors.lang_btn_inactive
                                };
                                lang_btns_c.borrow_mut()[li].set_color(restore);
                            }
                        }
                        ab[row_i][col_i].0.set_color(colors.nav_sel);
                        let _ = ab[row_i][col_i].0.take_focus();
                        *s = NavSel::Key(row_i, col_i);
                    }
                    narrator_c.borrow_mut().play(
                        &action_audio_slug(action, *layout_idx_c.borrow()),
                        action_tone_hz(action, &audio_mode_c),
                    );
                    app::redraw();
                });
            }

            // Track substitutable keys for layout switching.
            if let Action::Regular(slot) = phys.action {
                switch_btns.borrow_mut().push((btn.clone(), slot));
            }

            // Track modifier keys for toggle color updates.
            if is_mod {
                let status = match phys.action {
                    Action::CapsLock                => Some(caps_status.clone()),
                    Action::LShift | Action::RShift => Some(shift_status.clone()),
                    Action::Ctrl                    => Some(ctrl_status.clone()),
                    Action::Alt                     => Some(alt_status.clone()),
                    Action::AltGr                   => Some(altgr_status.clone()),
                    _                               => None,
                };
                mod_btns.borrow_mut().push(ModBtn {
                    btn:      btn.clone(),
                    action:   phys.action,
                    base_col: base_col,
                    status:   status,
                });
            }

            btn_row.push((btn, phys.action, phys.scancode, base_col));
            x += w + gap;
        }
        all_btns.borrow_mut().push(btn_row);
    }

    // --- Initial navigation highlight ---
    // When absolute_axes is enabled the joystick covers the full axis range and
    // starts at the physical centre, so initialise the selection at the centre
    // of the keyboard grid.  Otherwise start at the top-left key (row 0, col 0).
    let (init_row, init_col) = {
        let ab = all_btns.borrow();
        // Integer division rounds down, so for an even number of rows this
        // picks the upper-middle row (e.g. row 2 of 4), matching where the
        // joystick centred on the vertical axis would land.
        let mid_row = ab.len() / 2;
        if cfg.input.gamepad.absolute_axes
            && !ab.is_empty()
            && !ab[mid_row].is_empty()
        {
            (mid_row, ab[mid_row].len() / 2)
        } else {
            (0, 0)
        }
    };
    {
        let mut ab = all_btns.borrow_mut();
        ab[init_row][init_col].0.set_color(colors.nav_sel);
        let _ = ab[init_row][init_col].0.take_focus();
    }
    *sel.borrow_mut() = NavSel::Key(init_row, init_col);
    {
        let ab = all_btns.borrow();
        narrator.borrow_mut().play(
            &nav_audio_slug(NavSel::Key(init_row, init_col), *layout_idx.borrow(), &ab),
            nav_tone_freq(NavSel::Key(init_row, init_col), &ab, &audio_mode),
        );
    }

    // --- Shared gamepad cell (also used by the keyboard handler for rumble) ---
    // Created here (before the keyboard handler closure) so both the keyboard
    // handler and the gamepad polling timer can share the same instance.
    let gp_cell: Rc<RefCell<Option<Gamepad>>> = Rc::new(RefCell::new(None));

    // --- Menu state & UI ---
    // `menu_sel` tracks whether the pop-up menu is currently open (Some(i) =
    // open with item i selected; None = closed).
    let menu_sel: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    // Layout parameters for the menu overlay (centred on screen).
    let menu_item_h   = key_h;
    let menu_title_h  = key_h;
    let menu_inner_pad = pad * 2;
    let num_menu_items = menu_item_defs.len();
    let menu_w = (sw / 2).max(200).min(600);
    let menu_h = menu_inner_pad * 2 + menu_title_h
        + if num_menu_items > 0 {
            gap + num_menu_items as i32 * menu_item_h
                + (num_menu_items as i32 - 1) * gap
        } else {
            0
        };
    let menu_x = (sw - menu_w) / 2;
    let menu_y = (sh - menu_h) / 2;

    // The Group is the last widget added to the window so it renders on top.
    let mut menu_group = Group::new(menu_x, menu_y, menu_w, menu_h, "");

    let mut menu_bg = Frame::new(menu_x, menu_y, menu_w, menu_h, "");
    menu_bg.set_color(colors.status_bar_bg);
    menu_bg.set_frame(FrameType::FlatBox);

    // The leading underscore suppresses the "unused variable" warning; the
    // Frame must be kept alive so FLTK renders it as part of menu_group.
    let mut _menu_title = Frame::new(
        menu_x + menu_inner_pad,
        menu_y + menu_inner_pad,
        menu_w - 2 * menu_inner_pad,
        menu_title_h,
        "Menu",
    );
    _menu_title.set_color(colors.status_bar_bg);
    _menu_title.set_label_color(colors.status_ind_active_text);
    _menu_title.set_frame(FrameType::FlatBox);
    _menu_title.set_label_size(lbl_size);

    // Menu item widgets are Buttons so that:
    //   • mouse clicks fire the item's callback directly, and
    //   • keyboard focus can be moved here so Space/Enter reach the button
    //     callback rather than the keyboard buttons behind the overlay.
    let mut menu_item_btns: Vec<Button> = Vec::new();
    for (i, item) in menu_item_defs.iter().enumerate() {
        let fy = menu_y + menu_inner_pad + menu_title_h + gap
            + i as i32 * (menu_item_h + gap);
        let mut b = Button::new(
            menu_x + menu_inner_pad,
            fy,
            menu_w - 2 * menu_inner_pad,
            menu_item_h,
            None,
        );
        b.set_label(item.label);
        b.set_color(colors.key_mod);
        b.set_label_color(colors.key_label_mod);
        b.set_frame(FrameType::FlatBox);
        b.set_label_size(lbl_size);
        b.set_align(Align::Inside | Align::Left);
        menu_item_btns.push(b);
    }

    menu_group.end();

    // Give the background Frame an event handler so that clicks in the title /
    // padding area are absorbed and cannot fall through to keyboard buttons.
    menu_bg.handle(|_, ev| {
        matches!(ev, Event::Push | Event::Released)
    });

    // Set click callbacks on each menu item button.
    for (i, btn) in menu_item_btns.iter_mut().enumerate() {
        let items_c     = menu_item_defs.clone();
        let sel_btn     = menu_sel.clone();
        let mut grp_btn = menu_group.clone();
        btn.set_callback(move |_| {
            if (items_c[i].is_enabled)() {
                (items_c[i].execute)();
            }
            *sel_btn.borrow_mut() = None;
            grp_btn.hide();
            app::redraw();
        });
    }

    menu_group.hide();

    // --- Navigation: physical arrow keys + spacebar ---
    // super_handle_first(false) makes the Rust handler run BEFORE FLTK routes
    // the event to any child widget, so we can intercept arrow keys and spacebar
    // before any focused button consumes them.
    {
        let sel_c             = sel.clone();
        let all_btns_c        = all_btns.clone();
        let lang_btns_c       = lang_btns.clone();
        let layout_idx_c      = layout_idx.clone();
        let mod_state_c       = mod_state.clone();
        let mod_btns_c        = mod_btns.clone();
        let mut buf_c         = buf.clone();
        let mut disp_c        = disp.clone();
        let hook_c            = Rc::clone(&hook);
        let active_nav_key_c  = active_nav_key.clone();
        let gp_cell_c         = gp_cell.clone();
        let gp_rumble         = cfg.input.gamepad.rumble;
        let narrator_c        = narrator.clone();
        let audio_mode_c      = audio_mode.clone();
        let menu_sel_c        = menu_sel.clone();
        let menu_items_c      = menu_item_defs.clone();
        let mut menu_item_btns_c = menu_item_btns.clone();
        let mut menu_group_c  = menu_group.clone();
        let rollover          = cfg.navigate.rollover;

        // false = Rust handler runs BEFORE FLTK routes the event to any child
        // widget, so arrow keys and spacebar are intercepted here regardless of
        // which button (if any) currently holds FLTK keyboard focus.
        win.super_handle_first(false);
        win.handle(move |_w, ev| {
            let k = app::event_key();

            if ev == Event::KeyDown {
                // ── Menu open: route all key events to menu navigation ─────
                if menu_sel_c.borrow().is_some() {
                    if k == Key::Escape || k == nav_keys.menu {
                        // Close the menu without taking any action.
                        *menu_sel_c.borrow_mut() = None;
                        menu_group_c.hide();
                        app::redraw();
                    } else if k == nav_keys.up || k == nav_keys.down {
                        let dir = if k == nav_keys.up { -1i32 } else { 1i32 };
                        let cur = menu_sel_c.borrow().unwrap();
                        let next = menu_move_sel(cur, dir, &menu_items_c);
                        if next != cur {
                            *menu_sel_c.borrow_mut() = Some(next);
                            menu_set_item_colors(
                                Some(next), &menu_items_c,
                                &mut menu_item_btns_c, colors,
                            );
                            let _ = menu_item_btns_c[next].take_focus();
                            app::redraw();
                        }
                    } else if k == nav_keys.activate {
                        // Fallback: execute the selected item and close the menu.
                        // (Normally the focused menu button handles Space itself.)
                        let idx = menu_sel_c.borrow().unwrap();
                        if (menu_items_c[idx].is_enabled)() {
                            (menu_items_c[idx].execute)();
                        }
                        *menu_sel_c.borrow_mut() = None;
                        menu_group_c.hide();
                        app::redraw();
                    }
                    // Consume all key events while the menu is open.
                    return true;
                }

                // Suppress Escape so FLTK does not close the window.
                if k == Key::Escape {
                    return true;
                }

                // Arrow-key navigation (keys loaded from config).
                if k == nav_keys.up || k == nav_keys.down
                    || k == nav_keys.left || k == nav_keys.right
                {
                    let (dr, dc) = if k == nav_keys.up        { (-1,  0) }
                                   else if k == nav_keys.down  { ( 1,  0) }
                                   else if k == nav_keys.left  { ( 0, -1) }
                                   else                        { ( 0,  1) }; // right
                    let changed = {
                        let mut ab = all_btns_c.borrow_mut();
                        let mut lb = lang_btns_c.borrow_mut();
                        let mut s  = sel_c.borrow_mut();
                        nav_move(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, dr, dc, colors, rollover)
                    };
                    if changed {
                        if gp_rumble {
                            if let Some(ref mut gp) = *gp_cell_c.borrow_mut() {
                                gp.rumble();
                            }
                        }
                        let sel = *sel_c.borrow();
                        let ab  = all_btns_c.borrow();
                        narrator_c.borrow_mut().play(
                            &nav_audio_slug(sel, *layout_idx_c.borrow(), &ab),
                            nav_tone_freq(sel, &ab, &audio_mode_c),
                        );
                    }
                    return true;
                }

                // Activate key: fire the currently highlighted on-screen button.
                if k == nav_keys.activate {
                    // Copy NavSel (it is Copy) so the borrow is released before any
                    // callback that may itself borrow sel_c (e.g. the lang callback).
                    let cur_sel = *sel_c.borrow();
                    match cur_sel {
                        NavSel::Lang(li) => {
                            // Fire the language-switch button.  Its callback updates
                            // layout_idx, key labels, and the amber highlight.
                            lang_btns_c.borrow_mut()[li].do_callback();
                            // Language switches don't generate hardware key events,
                            // so there is nothing to release on activation-key up.
                            *active_nav_key_c.borrow_mut() = None;
                        }
                        NavSel::Key(row, col) => {
                            let (action, scancode) = {
                                let ab = all_btns_c.borrow();
                                (ab[row][col].1, ab[row][col].2)
                            };
                            let key_str = execute_action(
                                action, scancode,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c,
                                &mod_btns_c.borrow(),
                                colors,
                            );
                            // Store the activated key so on_key_release can be sent
                            // when the physical activation key is released.
                            *active_nav_key_c.borrow_mut() = Some((scancode, key_str));
                            // Re-apply amber in case execute_action changed the colour
                            // (e.g. when the selected button is a modifier key).
                            all_btns_c.borrow_mut()[row][col].0.set_color(colors.nav_sel);
                        }
                    }
                    return true;
                }

                // Menu key: open the pop-up menu (if any items are enabled).
                if k == nav_keys.menu {
                    if let Some(first) = menu_first_enabled(&menu_items_c) {
                        // Release any held activation key before entering menu mode
                        // so the BLE dongle receives a key-release report.
                        if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                            hook_c.on_key_release(sc, &ks);
                        }
                        *menu_sel_c.borrow_mut() = Some(first);
                        menu_set_item_colors(
                            Some(first), &menu_items_c,
                            &mut menu_item_btns_c, colors,
                        );
                        // Transfer keyboard focus to the first menu item button so
                        // that Space/Enter reach the button callback rather than
                        // the keyboard button that previously held focus.
                        let _ = menu_item_btns_c[first].take_focus();
                        menu_group_c.show();
                        app::redraw();
                    }
                    return true;
                }

                // navigate_center: move selection to the center of the keyboard.
                if nav_keys.navigate_center.map_or(false, |nk| k == nk) {
                    if let Some(center) = {
                        let ab = all_btns_c.borrow();
                        let lb = lang_btns_c.borrow();
                        nav_center(&ab, &lb)
                    } {
                        let changed = {
                            let mut ab = all_btns_c.borrow_mut();
                            let mut lb = lang_btns_c.borrow_mut();
                            let mut s  = sel_c.borrow_mut();
                            nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                        };
                        if changed {
                            if gp_rumble {
                                if let Some(ref mut gp) = *gp_cell_c.borrow_mut() {
                                    gp.rumble();
                                }
                            }
                            let sel = *sel_c.borrow();
                            let ab  = all_btns_c.borrow();
                            narrator_c.borrow_mut().play(
                                &nav_audio_slug(sel, *layout_idx_c.borrow(), &ab),
                                nav_tone_freq(sel, &ab, &audio_mode_c),
                            );
                        }
                    }
                    return true;
                }

                // activate_enter: directly produce the Enter output.
                if nav_keys.activate_enter.map_or(false, |ak| k == ak) {
                    let key_str = execute_action(
                        Action::Enter, 0x1c,
                        *layout_idx_c.borrow(),
                        &mut buf_c, &mut disp_c, &hook_c,
                        &mod_state_c, &mod_btns_c.borrow(), colors,
                    );
                    *active_nav_key_c.borrow_mut() = Some((0x1c, key_str));
                    return true;
                }

                // activate_space: directly produce the Space output.
                if nav_keys.activate_space.map_or(false, |ak| k == ak) {
                    let key_str = execute_action(
                        Action::Space, 0x39,
                        *layout_idx_c.borrow(),
                        &mut buf_c, &mut disp_c, &hook_c,
                        &mod_state_c, &mod_btns_c.borrow(), colors,
                    );
                    *active_nav_key_c.borrow_mut() = Some((0x39, key_str));
                    return true;
                }

                // activate_shift / ctrl / alt / altgr: force the modifier on,
                // then activate the currently selected key as if that modifier
                // were already held.  The modifier is auto-released by
                // execute_action after the key fires.
                let which_mod: Option<u8> =
                    if      nav_keys.activate_shift .map_or(false, |ak| k == ak) { Some(0) }
                    else if nav_keys.activate_ctrl  .map_or(false, |ak| k == ak) { Some(1) }
                    else if nav_keys.activate_alt   .map_or(false, |ak| k == ak) { Some(2) }
                    else if nav_keys.activate_altgr .map_or(false, |ak| k == ak) { Some(3) }
                    else { None };

                if let Some(m) = which_mod {
                    {
                        let mut ms = mod_state_c.borrow_mut();
                        match m {
                            0 => ms.lshift = true,
                            1 => ms.ctrl   = true,
                            2 => ms.alt    = true,
                            _ => ms.altgr  = true,
                        }
                    }
                    let cur_sel = *sel_c.borrow();
                    match cur_sel {
                        NavSel::Lang(li) => {
                            lang_btns_c.borrow_mut()[li].do_callback();
                            *active_nav_key_c.borrow_mut() = None;
                        }
                        NavSel::Key(row, col) => {
                            let (action, scancode) = {
                                let ab = all_btns_c.borrow();
                                (ab[row][col].1, ab[row][col].2)
                            };
                            let key_str = execute_action(
                                action, scancode,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                            );
                            *active_nav_key_c.borrow_mut() = Some((scancode, key_str));
                            all_btns_c.borrow_mut()[row][col].0.set_color(colors.nav_sel);
                        }
                    }
                    return true;
                }
            } else if ev == Event::KeyUp {
                // When the menu is open, consume key-up events silently.
                if menu_sel_c.borrow().is_some() {
                    return true;
                }
                // Activation key released: send the key-release event.
                if k == nav_keys.activate {
                    if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                        hook_c.on_key_release(sc, &ks);
                    }
                    return true;
                }
                // Release any activate_* variant key similarly.
                let is_activate_variant =
                    nav_keys.activate_shift .map_or(false, |ak| k == ak)
                    || nav_keys.activate_ctrl  .map_or(false, |ak| k == ak)
                    || nav_keys.activate_alt   .map_or(false, |ak| k == ak)
                    || nav_keys.activate_altgr .map_or(false, |ak| k == ak)
                    || nav_keys.activate_enter .map_or(false, |ak| k == ak)
                    || nav_keys.activate_space .map_or(false, |ak| k == ak);
                if is_activate_variant {
                    if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                        hook_c.on_key_release(sc, &ks);
                    }
                    return true;
                }
            } else if ev == Event::Push {
                // When the menu is open, block mouse clicks that land outside
                // the menu overlay so keyboard buttons in those areas cannot
                // fire.  Clicks inside the menu area fall through so that FLTK
                // routes them normally to the menu item buttons or menu_bg.
                if menu_sel_c.borrow().is_some() {
                    let ex = app::event_x();
                    let ey = app::event_y();
                    if ex < menu_x || ex >= menu_x + menu_w
                        || ey < menu_y || ey >= menu_y + menu_h
                    {
                        return true; // absorb the click
                    }
                }
            }

            false
        });
    }

    win.end();
    win.show();

    // --- Gamepad input (if enabled in config) ---
    if cfg.input.gamepad.enabled {
        // Clone config for use inside the reconnection closure.
        let gp_cfg = cfg.input.gamepad.clone();
        let gp_rumble = cfg.input.gamepad.rumble;

        // Open the gamepad now and store it in the shared gp_cell.
        *gp_cell.borrow_mut() = Gamepad::open(&cfg.input.gamepad);

        // Update the initial gamepad icon state based on whether the device
        // was found at startup.
        if let Some(ref mut icon) = gamepad_status {
            if gp_cell.borrow().is_some() {
                icon.set_label_color(colors.conn_connected);
            }
            // If not connected the icon already shows red (set at creation).
        }

        let all_btns_c        = all_btns.clone();
        let lang_btns_c       = lang_btns.clone();
        let layout_idx_c      = layout_idx.clone();
        let mod_state_c       = mod_state.clone();
        let mod_btns_c        = mod_btns.clone();
        let sel_c             = sel.clone();
        let mut buf_c         = buf.clone();
        let mut disp_c        = disp.clone();
        let hook_c            = Rc::clone(&hook);
        let active_nav_key_c  = active_nav_key.clone();
        let mut gamepad_status_t = gamepad_status.clone();
        let gp_cell_t         = gp_cell.clone();
        let narrator_t        = narrator.clone();
        let audio_mode_t      = audio_mode.clone();
        let menu_sel_gp       = menu_sel.clone();
        let menu_items_gp     = menu_item_defs.clone();
        let mut menu_item_btns_gp = menu_item_btns.clone();
        let mut menu_group_gp = menu_group.clone();
        let gp_rollover       = cfg.navigate.rollover;

        // Reuse a single Vec across poll calls to avoid repeated allocation.
        let mut gp_evt_buf: Vec<GamepadEvent> = Vec::new();

        // Poll at ~60 Hz; this keeps input latency low without burning CPU
        // the way an idle callback would.  When the gamepad is disconnected
        // the timer slows to 1 Hz and retries opening the device.
        app::add_timeout3(0.016, move |handle| {
            // Phase 1 – reconnect if currently disconnected.
            if gp_cell_t.borrow().is_none() {
                match Gamepad::open(&gp_cfg) {
                    Some(gp) => {
                        eprintln!("[gamepad] reconnected");
                        *gp_cell_t.borrow_mut() = Some(gp);
                        if let Some(ref mut icon) = gamepad_status_t {
                            icon.set_label_color(colors.conn_connected);
                            app::redraw();
                        }
                        // Fall through to poll the newly opened device.
                    }
                    None => {
                        // Still no device; retry in 1 s.
                        app::repeat_timeout3(1.0, handle);
                        return;
                    }
                }
            }

            // Phase 2 – poll for events; detect disconnection.
            let still_alive = {
                let mut opt = gp_cell_t.borrow_mut();
                opt.as_mut().unwrap().poll(&mut gp_evt_buf)
            };

            if !still_alive {
                eprintln!("[gamepad] disconnected");
                *gp_cell_t.borrow_mut() = None;
                if let Some(ref mut icon) = gamepad_status_t {
                    icon.set_label_color(colors.conn_disconnected);
                    app::redraw();
                }
                app::repeat_timeout3(1.0, handle);
                return;
            }

            // Phase 3 – process the events collected in Phase 2.
            for evt in gp_evt_buf.iter() {
                match evt.action {
                    GamepadAction::Menu => {
                        if !evt.pressed { continue; }
                        if menu_sel_gp.borrow().is_some() {
                            // Menu is open: close it.
                            *menu_sel_gp.borrow_mut() = None;
                            menu_group_gp.hide();
                            app::redraw();
                        } else {
                            // Menu is closed: open it if any items are enabled.
                            if let Some(first) = menu_first_enabled(&menu_items_gp) {
                                if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                    hook_c.on_key_release(sc, &ks);
                                }
                                *menu_sel_gp.borrow_mut() = Some(first);
                                menu_set_item_colors(
                                    Some(first), &menu_items_gp,
                                    &mut menu_item_btns_gp, colors,
                                );
                                // Transfer keyboard focus so Space/Enter reach the
                                // menu button, not the keyboard button behind it.
                                let _ = menu_item_btns_gp[first].take_focus();
                                menu_group_gp.show();
                                app::redraw();
                            }
                        }
                    }
                    GamepadAction::Up
                    | GamepadAction::Down
                    | GamepadAction::Left
                    | GamepadAction::Right => {
                        // Only navigate on button press, not release.
                        if !evt.pressed {
                            continue;
                        }
                        // When menu is open, route vertical nav to menu.
                        if menu_sel_gp.borrow().is_some() {
                            let dir = match evt.action {
                                GamepadAction::Up   => -1i32,
                                GamepadAction::Down =>  1i32,
                                _                   => continue, // ignore left/right
                            };
                            let cur = menu_sel_gp.borrow().unwrap();
                            let next = menu_move_sel(cur, dir, &menu_items_gp);
                            if next != cur {
                                *menu_sel_gp.borrow_mut() = Some(next);
                                menu_set_item_colors(
                                    Some(next), &menu_items_gp,
                                    &mut menu_item_btns_gp, colors,
                                );
                                let _ = menu_item_btns_gp[next].take_focus();
                                app::redraw();
                            }
                            continue;
                        }
                        let (dr, dc) = match evt.action {
                            GamepadAction::Up    => (-1,  0),
                            GamepadAction::Down  => ( 1,  0),
                            GamepadAction::Left  => ( 0, -1),
                            _                    => ( 0,  1), // Right
                        };
                        let changed = {
                            let mut ab = all_btns_c.borrow_mut();
                            let mut lb = lang_btns_c.borrow_mut();
                            let mut s  = sel_c.borrow_mut();
                            nav_move(
                                &mut ab, &mut lb,
                                *layout_idx_c.borrow(),
                                &mut s, &mod_state_c,
                                dr, dc,
                                colors,
                                gp_rollover,
                            )
                        };
                        if changed {
                            if gp_rumble {
                                if let Some(ref mut gp) = *gp_cell_t.borrow_mut() {
                                    gp.rumble();
                                }
                            }
                            let sel = *sel_c.borrow();
                            let ab  = all_btns_c.borrow();
                            narrator_t.borrow_mut().play(
                                &nav_audio_slug(sel, *layout_idx_c.borrow(), &ab),
                                nav_tone_freq(sel, &ab, &audio_mode_t),
                            );
                        }
                    }
                    GamepadAction::Activate => {
                        // When menu is open, Activate executes the selected item.
                        if menu_sel_gp.borrow().is_some() {
                            if evt.pressed {
                                let idx = menu_sel_gp.borrow().unwrap();
                                if (menu_items_gp[idx].is_enabled)() {
                                    (menu_items_gp[idx].execute)();
                                }
                                *menu_sel_gp.borrow_mut() = None;
                                menu_group_gp.hide();
                                app::redraw();
                            }
                            continue;
                        }
                        if evt.pressed {
                            let cur_sel = *sel_c.borrow();
                            match cur_sel {
                                NavSel::Lang(li) => {
                                    lang_btns_c.borrow_mut()[li].do_callback();
                                    // Language switches don't generate hardware key
                                    // events, so there is nothing to release.
                                    *active_nav_key_c.borrow_mut() = None;
                                }
                                NavSel::Key(row, col) => {
                                    let (action, scancode) = {
                                        let ab = all_btns_c.borrow();
                                        (ab[row][col].1, ab[row][col].2)
                                    };
                                    let key_str = execute_action(
                                        action, scancode,
                                        *layout_idx_c.borrow(),
                                        &mut buf_c, &mut disp_c, &hook_c,
                                        &mod_state_c,
                                        &mod_btns_c.borrow(),
                                        colors,
                                    );
                                    // Store the activated key so on_key_release can be
                                    // sent when the gamepad button is released.
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                }
                            }
                        } else {
                            // Button released: send the key-release event.
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateEnter => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if evt.pressed {
                            let key_str = execute_action(
                                Action::Enter, 0x1c,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x1c, key_str));
                        } else {
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateSpace => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if evt.pressed {
                            let key_str = execute_action(
                                Action::Space, 0x39,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x39, key_str));
                        } else {
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateShift
                    | GamepadAction::ActivateCtrl
                    | GamepadAction::ActivateAlt
                    | GamepadAction::ActivateAltGr => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Force-activate the relevant modifier, then run the
                            // same logic as the regular Activate button.
                            {
                                let mut ms = mod_state_c.borrow_mut();
                                match evt.action {
                                    GamepadAction::ActivateShift => ms.lshift = true,
                                    GamepadAction::ActivateCtrl  => ms.ctrl   = true,
                                    GamepadAction::ActivateAlt   => ms.alt    = true,
                                    _                            => ms.altgr  = true,
                                }
                            }
                            let cur_sel = *sel_c.borrow();
                            match cur_sel {
                                NavSel::Lang(li) => {
                                    lang_btns_c.borrow_mut()[li].do_callback();
                                    *active_nav_key_c.borrow_mut() = None;
                                }
                                NavSel::Key(row, col) => {
                                    let (action, scancode) = {
                                        let ab = all_btns_c.borrow();
                                        (ab[row][col].1, ab[row][col].2)
                                    };
                                    let key_str = execute_action(
                                        action, scancode,
                                        *layout_idx_c.borrow(),
                                        &mut buf_c, &mut disp_c, &hook_c,
                                        &mod_state_c,
                                        &mod_btns_c.borrow(),
                                        colors,
                                    );
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                }
                            }
                        } else {
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::NavigateCenter => {
                        // Only act on button press; ignore release.
                        if !evt.pressed { continue; }
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if let Some(center) = {
                            let ab = all_btns_c.borrow();
                            let lb = lang_btns_c.borrow();
                            nav_center(&ab, &lb)
                        } {
                            let changed = {
                                let mut ab = all_btns_c.borrow_mut();
                                let mut lb = lang_btns_c.borrow_mut();
                                let mut s  = sel_c.borrow_mut();
                                nav_set(
                                    &mut ab, &mut lb,
                                    *layout_idx_c.borrow(),
                                    &mut s, &mod_state_c,
                                    center,
                                    colors,
                                )
                            };
                            if changed {
                                if gp_rumble {
                                    if let Some(ref mut gp) = *gp_cell_t.borrow_mut() {
                                        gp.rumble();
                                    }
                                }
                                let sel = *sel_c.borrow();
                                let ab  = all_btns_c.borrow();
                                narrator_t.borrow_mut().play(
                                    &nav_audio_slug(sel, *layout_idx_c.borrow(), &ab),
                                    nav_tone_freq(sel, &ab, &audio_mode_t),
                                );
                            }
                        }
                    }
                    GamepadAction::AbsolutePos { horiz, vert } => {
                        // Ignore absolute-position events while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        // Map normalised 0.0…1.0 coordinates to the full
                        // selectable area, which consists of the language-toggle
                        // strip followed by the keyboard key rows.
                        //
                        // The vertical range is divided into equal bands:
                        //   band 0 (when lang buttons exist) → language strip
                        //   remaining bands → keyboard rows (0-indexed)
                        //
                        // If there are no language buttons the vertical range is
                        // divided solely across the keyboard rows.
                        let new_sel = {
                            let ab = all_btns_c.borrow();
                            let lb = lang_btns_c.borrow();
                            let num_rows = ab.len();
                            let num_lang = lb.len();
                            if num_rows == 0 { continue; }
                            // Total virtual rows: 1 for the lang strip (if any)
                            // plus one per keyboard row.
                            let has_lang = num_lang > 0;
                            let total_bands = if has_lang { 1 + num_rows } else { num_rows };
                            let band = (vert * total_bands as f32)
                                .floor()
                                .clamp(0.0, total_bands as f32 - 1.0) as usize;
                            if has_lang && band == 0 {
                                // Language strip: map horiz across lang buttons.
                                let li = (horiz * num_lang as f32)
                                    .floor()
                                    .clamp(0.0, num_lang as f32 - 1.0) as usize;
                                NavSel::Lang(li)
                            } else {
                                // Keyboard row: subtract 1 for the lang strip
                                // band when it exists.
                                let row = if has_lang { band - 1 } else { band };
                                let num_cols = ab[row].len();
                                let col = (horiz * num_cols as f32)
                                    .floor()
                                    .clamp(0.0, num_cols as f32 - 1.0) as usize;
                                NavSel::Key(row, col)
                            }
                        };
                        #[cfg(debug_assertions)]
                        if new_sel != *sel_c.borrow() {
                            match new_sel {
                                NavSel::Lang(li) =>
                                    eprintln!(
                                        "[gamepad] abs_pos horiz={:.3} vert={:.3} → lang={}",
                                        horiz, vert, li
                                    ),
                                NavSel::Key(row, col) =>
                                    eprintln!(
                                        "[gamepad] abs_pos horiz={:.3} vert={:.3} → row={} col={}",
                                        horiz, vert, row, col
                                    ),
                            }
                        }
                        let changed = {
                            let mut ab = all_btns_c.borrow_mut();
                            let mut lb = lang_btns_c.borrow_mut();
                            let mut s  = sel_c.borrow_mut();
                            nav_set(
                                &mut ab, &mut lb,
                                *layout_idx_c.borrow(),
                                &mut s, &mod_state_c,
                                new_sel,
                                colors,
                            )
                        };
                        if changed {
                            if gp_rumble {
                                if let Some(ref mut gp) = *gp_cell_t.borrow_mut() {
                                    gp.rumble();
                                }
                            }
                            let sel = *sel_c.borrow();
                            let ab  = all_btns_c.borrow();
                            narrator_t.borrow_mut().play(
                                &nav_audio_slug(sel, *layout_idx_c.borrow(), &ab),
                                nav_tone_freq(sel, &ab, &audio_mode_t),
                            );
                        }
                    }
                }
            }
            app::repeat_timeout3(0.016, handle);
        });
    }

    a.run().unwrap();
}
