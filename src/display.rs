// src/display.rs
//
// Display-related types and UI for the on-screen keyboard.

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

use crate::{config, KeyHook};
use crate::keyboards::{self, is_modifier, is_sticky, special_hook_str, special_label,
    Action, KW, KEYS};
use crate::gamepad::Gamepad;
use crate::narrator::Narrator;
use crate::output;



// =============================================================================
// UI colour palette (resolved from config)
// =============================================================================

/// All UI colours resolved from [`config::ColorsConfig`] into FLTK [`Color`] values.
/// Implements `Copy` because [`Color`] is a newtype over `u32`.
#[derive(Clone, Copy)]
pub struct Colors {
    pub key_normal:              Color,
    pub key_mod:                 Color,
    pub mod_active:              Color,
    pub nav_sel:                 Color,
    pub status_bar_bg:           Color,
    pub status_ind_bg:           Color,
    pub status_ind_text:         Color,
    pub status_ind_active_text:  Color,
    pub conn_disconnected:       Color,
    pub conn_connecting:         Color,
    pub conn_connected:          Color,
    pub win_bg:                  Color,
    pub disp_bg:                 Color,
    pub disp_text:               Color,
    pub lang_btn_inactive:       Color,
    pub lang_btn_label:          Color,
    pub key_label_normal:        Color,
    pub key_label_mod:           Color,
}

impl Colors {
    pub fn from_config(cfg: &config::ColorsConfig) -> Self {
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
pub struct ModBtn {
    pub btn:      Button,
    pub action:   Action,
    pub base_col: Color,
    /// Corresponding status-bar indicator frame (shared between LShift & RShift).
    pub status:   Option<Frame>,
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
    pub is_enabled: Box<dyn Fn() -> bool>,
    pub execute:    Box<dyn Fn()>,
}

/// Return the index of the first enabled item in `items`, or `None` if all
/// items are disabled (or the list is empty).
pub fn menu_first_enabled(items: &[MenuItemDef]) -> Option<usize> {
    items.iter().position(|it| (it.is_enabled)())
}

/// Starting from `current`, scan in direction `dir` (+1 = down, -1 = up) for
/// the next enabled item.  Returns `current` unchanged if no other enabled
/// item exists in that direction (the cursor stays put at the edge).
pub fn menu_move_sel(current: usize, dir: i32, items: &[MenuItemDef]) -> usize {
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
pub fn menu_set_item_colors(
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
/// * Ctrl, Shift (L/R), Alt, AltGr, Win: sticky-toggle.
///   First press activates them; they auto-deactivate after the next regular
///   keypress.  A second press before any regular key deactivates them early.
#[derive(Default)]
pub struct ModState {
    pub caps:   bool,
    pub lshift: bool,
    pub rshift: bool,
    pub ctrl:   bool,
    pub win:    bool,
    pub alt:    bool,
    pub altgr:  bool,
}

impl ModState {
    /// Flip the modifier for `action`; returns the new active state.
    pub fn toggle(&mut self, action: Action) -> bool {
        let s = self.slot_mut(action);
        *s = !*s;
        *s
    }

    /// Deactivate all sticky modifiers (Ctrl/Shift/Alt/AltGr/Win).
    pub fn release_sticky(&mut self) {
        self.lshift = false;
        self.rshift = false;
        self.ctrl   = false;
        self.win    = false;
        self.alt    = false;
        self.altgr  = false;
    }

    /// If `action` is LShift or RShift, deactivate the other Shift key and
    /// return its `Action`; otherwise return `None`.
    pub fn release_shift_peer(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::LShift if self.rshift => { self.rshift = false; Some(Action::RShift) }
            Action::RShift if self.lshift => { self.lshift = false; Some(Action::LShift) }
            _ => None,
        }
    }

    /// If `action` is Alt or AltGr, deactivate the other Alt key and return
    /// its `Action`; otherwise return `None`.
    pub fn release_alt_peer(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Alt   if self.altgr => { self.altgr = false; Some(Action::AltGr) }
            Action::AltGr if self.alt   => { self.alt   = false; Some(Action::Alt)   }
            _ => None,
        }
    }

    pub fn is_active(&self, action: Action) -> bool { *self.slot(action) }

    /// Returns `true` when either Shift key is held (Left or Right Shift).
    ///
    /// CapsLock is intentionally excluded: it only affects letter keys (which
    /// always use the unshifted label for narration), not punctuation keys.
    pub fn is_shifted(&self) -> bool { self.lshift || self.rshift }

    pub fn slot(&self, action: Action) -> &bool {
        match action {
            Action::CapsLock => &self.caps,
            Action::LShift   => &self.lshift,
            Action::RShift   => &self.rshift,
            Action::Ctrl     => &self.ctrl,
            Action::Win      => &self.win,
            Action::Alt      => &self.alt,
            Action::AltGr    => &self.altgr,
            _                => unreachable!(),
        }
    }
    pub fn slot_mut(&mut self, action: Action) -> &mut bool {
        match action {
            Action::CapsLock => &mut self.caps,
            Action::LShift   => &mut self.lshift,
            Action::RShift   => &mut self.rshift,
            Action::Ctrl     => &mut self.ctrl,
            Action::Win      => &mut self.win,
            Action::Alt      => &mut self.alt,
            Action::AltGr    => &mut self.altgr,
            _                => unreachable!(),
        }
    }
}



// =============================================================================
// Navigation selection
// =============================================================================

/// Identifies which button currently holds the amber navigation highlight.
#[derive(Clone, Copy, PartialEq)]
pub enum NavSel {
    /// A language-toggle button; index into `lang_btns`.
    Lang(usize),
    /// A keyboard key: `all_btns[row][col]`.
    Key(usize, usize),
}

// =============================================================================
// Navigation
// =============================================================================

/// Find the index in `items` (iterator of `(x, width)`) whose range best covers `cx`.
pub fn closest_to_cx(items: impl Iterator<Item = (i32, i32)>, cx: i32) -> usize {
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
pub fn closest_col(row: &[(Button, Action, u16, Color)], cx: i32) -> usize {
    closest_to_cx(row.iter().map(|b| (b.0.x(), b.0.w())), cx)
}

/// Find the index in the lang-button strip whose x-range best covers pixel centre `cx`.
pub fn closest_lang(lang_btns: &[Button], cx: i32) -> usize {
    closest_to_cx(lang_btns.iter().map(|b| (b.x(), b.w())), cx)
}

/// Apply a specific navigation selection, updating highlight colours.
///
/// Does nothing if `new_sel` equals the current `*sel`.
/// Returns `true` if the selection changed, `false` if it was already at `new_sel`.
pub fn nav_set(
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
pub fn nav_move(
    all_btns:     &mut Vec<Vec<(Button, Action, u16, Color)>>,
    lang_btns:    &mut Vec<Button>,
    layout_idx:   usize,
    sel:          &mut NavSel,
    mod_state:    &Rc<RefCell<ModState>>,
    dr:           i32,
    dc:           i32,
    colors:       Colors,
    rollover:     bool,
    preferred_cx: &mut i32,
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
                        *preferred_cx = cx;
                        NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                    }
                } else {
                    // Already at the top edge.
                    NavSel::Lang(li)
                }
            } else if dr > 0 {
                // Down into the first keyboard row, pixel-aligned.
                let cx = lang_btns[li].x() + lang_btns[li].w() / 2;
                *preferred_cx = cx;
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
                // Compute cx from the current key (row 0 is the F-key row, never a
                // wide Space, so no special case needed here).
                let cx = all_btns[row][col].0.x() + all_btns[row][col].0.w() / 2;
                *preferred_cx = cx;
                if !lang_btns.is_empty() {
                    // Up from the top keyboard row -> lang strip, but only if
                    // cx is within a lang button's pixel range.  Keys like F2-F12
                    // extend beyond the two lang buttons; without this guard they
                    // would all jump to the UA button (the closest one), even
                    // though no lang button sits directly above them.
                    let li = closest_lang(lang_btns, cx);
                    let lb = &lang_btns[li];
                    if cx >= lb.x() && cx < lb.x() + lb.w() {
                        NavSel::Lang(li)
                    } else if rollover {
                        // cx is not within any lang button (e.g. F2-F12).
                        // Wrap: scan from the last keyboard row upward and land
                        // on the first row whose button pixel range covers cx.
                        let rows = all_btns.len();
                        let mut found = NavSel::Key(row, col); // fallback: stay
                        for scan in (0..rows).rev() {
                            let best = closest_col(&all_btns[scan], cx);
                            let btn  = &all_btns[scan][best].0;
                            if cx >= btn.x() && cx < btn.x() + btn.w() {
                                found = NavSel::Key(scan, best);
                                break;
                            }
                        }
                        found
                    } else {
                        NavSel::Key(row, col) // clamp - nothing directly above
                    }
                } else if rollover {
                    // No lang strip: wrap to the last keyboard row.
                    let rows = all_btns.len();
                    NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                } else {
                    NavSel::Key(row, col)
                }
            } else if dr != 0 {
                let rows = all_btns.len();
                // When the current key is the Spacebar (a wide key), use the stored
                // preferred column position so the cursor returns to the same column
                // the user navigated from, rather than drifting to the Spacebar's
                // wide centre.  For all other keys, update preferred_cx so that
                // subsequent Spacebar navigation remembers this column.
                let cx = if all_btns[row][col].1 == Action::Space {
                    *preferred_cx
                } else {
                    let c = all_btns[row][col].0.x() + all_btns[row][col].0.w() / 2;
                    *preferred_cx = c;
                    c
                };
                // Scan rows in direction dr, stopping when cx falls within a
                // button's pixel range (dist == 0).  Rows where no button
                // covers cx are skipped, so e.g. End-down skips the home row
                // and lands directly on ArrowUp.  Using strict containment
                // (not a distance tolerance) prevents cross-column jumps such
                // as Bksp-up->F12 or Enter-down->RShift.
                let mut scan     = row;
                let mut dest_row = row; // will be updated when we find a close row
                loop {
                    let next_raw = scan as i32 + dr;
                    // Check if we've gone past the edge.
                    if next_raw < 0 || next_raw >= rows as i32 {
                        if rollover {
                            if dr > 0 {
                                // Wrap: going down past last row -> lang strip (if any).
                                if !lang_btns.is_empty() {
                                    dest_row = rows; // sentinel: means "go to lang strip"
                                } else {
                                    // Wrap to first row.
                                    dest_row = rows + 1; // sentinel: means "wrap to row 0"
                                }
                            } else {
                                // dr < 0: went up past row 0 without cx landing in any
                                // row-0 button (e.g. Bksp, Ins, Home, PgUp whose pixel
                                // range lies beyond the F-key row).  Apply the same wrap
                                // as the dr < 0 && row == 0 case.
                                dest_row = rows + 2; // sentinel: means "roll over upward"
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
                    if dist == 0 {
                        dest_row = scan;
                        break;
                    }
                    // cx not within any button in this row - keep scanning.
                }
                if dest_row == rows {
                    // Sentinel: wrap to lang strip (if cx falls within a lang
                    // button) or further to row 0 scanning downward.
                    // When rolling over from the Spacebar, skip the lang strip
                    // entirely and land on the F-key row (row 0) using the
                    // remembered preferred column.
                    if all_btns[row][col].1 == Action::Space {
                        NavSel::Key(0, closest_col(&all_btns[0], cx))
                    } else {
                        let li = closest_lang(lang_btns, cx);
                        let lb = &lang_btns[li];
                        if cx >= lb.x() && cx < lb.x() + lb.w() {
                            NavSel::Lang(li)
                        } else {
                            // cx is not within any lang button (e.g. Enter,
                            // RShift).  Wrap: scan from row 0 downward and land
                            // on the first row whose button pixel range covers cx.
                            let mut found = NavSel::Key(row, col); // fallback: stay
                            for scan in 0..rows {
                                let best = closest_col(&all_btns[scan], cx);
                                let btn  = &all_btns[scan][best].0;
                                if cx >= btn.x() && cx < btn.x() + btn.w() {
                                    found = NavSel::Key(scan, best);
                                    break;
                                }
                            }
                            found
                        }
                    }
                } else if dest_row == rows + 1 {
                    // Sentinel: wrap to first row.
                    NavSel::Key(0, closest_col(&all_btns[0], cx))
                } else if dest_row == rows + 2 {
                    // Sentinel: went up past row 0 with cx not covered by any
                    // row-0 button (e.g. Bksp, Ins, Home, PgUp).  Mirror the
                    // dr < 0 && row == 0 rollover logic: check lang strip first,
                    // then scan from the last keyboard row upward.
                    if !lang_btns.is_empty() {
                        let li = closest_lang(lang_btns, cx);
                        let lb = &lang_btns[li];
                        if cx >= lb.x() && cx < lb.x() + lb.w() {
                            NavSel::Lang(li)
                        } else {
                            let mut found = NavSel::Key(row, col); // fallback: stay
                            for scan in (0..rows).rev() {
                                let best = closest_col(&all_btns[scan], cx);
                                let btn  = &all_btns[scan][best].0;
                                if cx >= btn.x() && cx < btn.x() + btn.w() {
                                    found = NavSel::Key(scan, best);
                                    break;
                                }
                            }
                            found
                        }
                    } else {
                        // No lang strip: wrap to last row.
                        NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                    }
                } else if dest_row == row {
                    NavSel::Key(row, col) // clamped at edge
                } else {
                    NavSel::Key(dest_row, closest_col(&all_btns[dest_row], cx))
                }
            } else {
                // Left / right within the current keyboard row.
                let rl      = all_btns[row].len();
                let new_col = if rollover {
                    (col as i32 + dc).rem_euclid(rl as i32) as usize
                } else {
                    (col as i32 + dc).clamp(0, rl as i32 - 1) as usize
                };
                // Update preferred_cx so that a subsequent vertical navigation from
                // the new key's column is used correctly even if the user later moves
                // onto the Spacebar.
                *preferred_cx = all_btns[row][new_col].0.x() + all_btns[row][new_col].0.w() / 2;
                NavSel::Key(row, new_col)
            }
        }
    };

    nav_set(all_btns, lang_btns, layout_idx, sel, mod_state, new_sel, colors)
}

/// Find the key matching `center_key` label in the current layout.
///
/// Searches `all_btns` for the first key whose unshifted label (or
/// [`special_label`]) equals `center_key` (case-sensitive).  Returns the
/// `NavSel` for that key, or `None` if no match is found.
pub fn find_center_key(
    all_btns:   &[Vec<(Button, Action, u16, Color)>],
    layout_idx: usize,
    center_key: &str,
) -> Option<NavSel> {
    for (r, row) in all_btns.iter().enumerate() {
        for (c, (_btn, action, _sc, _col)) in row.iter().enumerate() {
            let label = match *action {
                Action::Regular(n) => keyboards::get_layouts()[layout_idx].keys[n].label_unshifted.as_str(),
                other              => special_label(other),
            };
            if label == center_key {
                return Some(NavSel::Key(r, c));
            }
        }
    }
    None
}

/// Find the `(row, col)` position of the first button whose action matches
/// `target_action`, or `None` if no button has that action.
///
/// Used to locate buttons for non-selection activate actions (Enter, Space,
/// Arrow keys) so their background can be temporarily highlighted when the
/// corresponding gamepad / GPIO button is pressed.
pub fn find_btn_by_action(
    all_btns:      &[Vec<(Button, Action, u16, Color)>],
    target_action: Action,
) -> Option<(usize, usize)> {
    for (r, row) in all_btns.iter().enumerate() {
        for (c, entry) in row.iter().enumerate() {
            if entry.1 == target_action {
                return Some((r, c));
            }
        }
    }
    None
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
pub fn execute_action(
    action:         Action,
    scancode:       u16,
    layout_i:       usize,
    buf:            &mut TextBuffer,
    disp:           &mut TextDisplay,
    hook:           &Rc<dyn KeyHook>,
    mod_state:      &Rc<RefCell<ModState>>,
    mod_btns:       &[ModBtn],
    colors:         Colors,
    update_display: bool,
) -> String {
    // For Regular keys, compute the text to insert respecting Shift / CapsLock.
    // Symbol/number keys (label_shifted != "") use the shifted character on LShift/RShift;
    // CapsLock does NOT affect them (standard keyboard behaviour).
    // Letter keys (label_shifted == "") use to_uppercase() for any of Caps/LShift/RShift.
    let regular_text: Option<String> = if let Action::Regular(slot) = action {
        let key = &keyboards::get_layouts()[layout_i].keys[slot];
        let ms  = mod_state.borrow();
        Some(if !key.label_shifted.is_empty() && (ms.lshift || ms.rshift) {
            key.insert_shifted.clone()
        } else if key.label_shifted.is_empty() && (ms.caps || ms.lshift || ms.rshift) {
            key.insert_unshifted.to_uppercase()
        } else {
            key.insert_unshifted.clone()
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
    //   0x08 = Win (left GUI), 0x20 = RShift, 0x40 = AltGr (right alt)
    let modifier_bits: u8 = {
        let ms = mod_state.borrow();
        (if ms.ctrl   { 0x01 } else { 0 })
            | (if ms.lshift { 0x02 } else { 0 })
            | (if ms.alt    { 0x04 } else { 0 })
            | (if ms.win    { 0x08 } else { 0 })
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
        // Synchronize paired modifier keys: pressing/releasing either Shift
        // releases the other Shift; pressing/releasing either Alt releases
        // the other Alt (AltGr).
        let peer = {
            let mut ms = mod_state.borrow_mut();
            let p = ms.release_shift_peer(action);
            if p.is_some() { p } else { ms.release_alt_peer(action) }
        };
        if let Some(peer_action) = peer {
            for m in mod_btns {
                if m.action == peer_action {
                    m.btn.clone().set_color(m.base_col);
                    if let Some(mut sf) = m.status.clone() {
                        sf.set_color(colors.status_ind_bg);
                        sf.set_label_color(colors.status_ind_text);
                    }
                }
            }
        }
        app::redraw();
    } else {
        // Regular key: insert text into the buffer.
        if update_display {
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
                Action::Enter => buf.set_text(""),
                Action::Space => buf.append(" "),
                _ => {}
            }
            // Scroll the display to keep the newest text visible.
            let len   = buf.length();
            let lines = disp.count_lines(0, len, false);
            disp.scroll(lines, 0);
        }

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
pub fn label_to_audio_slug(label: &str) -> String {
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
///
/// When `shifted` is `true` and the key has a non-empty `insert_shifted`,
/// the slug is computed from `insert_shifted` instead of `label_unshifted`.
/// `insert_shifted` is used in preference to `label_shifted` because some
/// keymaps use FLTK escape sequences in `label_shifted` (e.g. `"@@"` for `@`
/// or `"&&"` for `&`), while `insert_shifted` always holds the plain character.
/// Letter keys (whose `insert_shifted` is empty) always use the unshifted slug
/// regardless of `shifted`.
pub fn action_audio_slug(action: Action, layout_idx: usize, shifted: bool) -> String {
    match action {
        Action::Regular(slot) => {
            let layout = &keyboards::get_layouts()[layout_idx];
            let layout_name = layout.name.to_lowercase();
            let key = &layout.keys[slot];
            let label = if shifted && !key.insert_shifted.is_empty() {
                key.insert_shifted.as_str()
            } else {
                key.label_unshifted.as_str()
            };
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
///
/// When `shifted` is `true`, delegates to [`action_audio_slug`] with
/// `shifted = true` so that keys with a non-empty `insert_shifted` produce the
/// shifted slug.  Language-toggle buttons are never affected by shift.
pub fn nav_audio_slug(
    sel: NavSel,
    layout_idx: usize,
    all_btns: &[Vec<(Button, Action, u16, Color)>],
    shifted: bool,
) -> String {
    match sel {
        NavSel::Lang(li) => {
            format!("lang_{}", keyboards::get_layouts()[li].name.to_lowercase())
        }
        NavSel::Key(row, col) => action_audio_slug(all_btns[row][col].1, layout_idx, shifted),
    }
}

/// Return the tone frequency (Hz) for an [`Action`] in tone audio mode.
///
/// Key categories and their pitches:
///
/// 1. **All letter / punctuation keys, except F and J** -> A5 (880 Hz).
/// 2. **F and J** (the physical home-row bump keys, slots 29 & 32)
///    -> B5 (988 Hz) - a distinctive major second above the other letters.
/// 3. **Digit keys 1-0** -> ascending C-major scale C4-E5 (261-659 Hz),
///    with 1 the lowest pitch and 0 the highest.
/// 4. **Function keys F1-F12** -> ascending A-minor pentatonic A1-B3
///    (55-247 Hz), clearly in the bass register below the digit range.
/// 5. **All other keys** (Space, Enter, modifiers, arrows, ...) -> each key
///    has its own unique pitch chosen for pleasant distinctiveness.
///
/// Returns `0.0` for [`Action::Noop`] (no tone should be played).
pub fn tone_freq_for_action(action: Action) -> f32 {
    match action {
        // --- Regular keys ---
        Action::Regular(slot) => match slot {
            // Digit row: slots 1-10 -> 1,2,3,4,5,6,7,8,9,0
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
            // F and J - physical home-row bump keys
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
        Action::Esc        => 1760.00,  // A6 - high/urgent
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
/// position hint.  Digit keys (slots 1-10) and all non-`Regular` actions keep
/// their pitches unchanged.
pub fn tone_hint_freq_for_action(action: Action) -> f32 {
    match action {
        Action::Regular(slot) => match slot {
            // Digit keys: same ascending scale as in tone mode.
            1..=10 => tone_freq_for_action(action),
            // F and J - home-row bump keys: play a distinctive tone.
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
pub fn action_tone_hz(action: Action, mode: &config::AudioMode) -> f32 {
    match mode {
        config::AudioMode::ToneHint => tone_hint_freq_for_action(action),
        _ => tone_freq_for_action(action),
    }
}

/// Tone frequency (Hz) used for language-toggle buttons in tone mode.
/// E4 = 329.63 Hz - a neutral, distinctive pitch in the mid register.
pub const LANG_BTN_TONE_HZ: f32 = 329.63;

/// Return the tone frequency (Hz) for the current navigation selection.
///
/// Language-toggle buttons use [`LANG_BTN_TONE_HZ`] as a neutral, distinctive tone.
pub fn nav_tone_freq(
    sel: NavSel,
    all_btns: &[Vec<(Button, Action, u16, Color)>],
    mode: &config::AudioMode,
) -> f32 {
    match sel {
        NavSel::Lang(_) => LANG_BTN_TONE_HZ,
        NavSel::Key(row, col) => action_tone_hz(all_btns[row][col].1, mode),
    }
}

/// Called after a navigation selection change.
///
/// When `changed` is `true`:
/// * If `do_rumble` is set and a gamepad is connected, triggers a short rumble.
/// * Plays the audio cue (narration clip or tone) for the new selection.
///   When `shifted` is `true` and the focused key has a non-empty
///   `insert_shifted`, the shifted audio clip is attempted first; if the file
///   does not exist on disk, the unshifted clip is played as a fallback.
///
/// Does nothing when `changed` is `false`.
pub fn on_nav_changed(
    changed:    bool,
    do_rumble:  bool,
    gp_cell:    &Rc<RefCell<Option<Gamepad>>>,
    sel:        &Rc<RefCell<NavSel>>,
    all_btns:   &Rc<RefCell<Vec<Vec<(Button, Action, u16, Color)>>>>,
    layout_idx: usize,
    narrator:   &Rc<RefCell<Narrator>>,
    audio_mode: &config::AudioMode,
    shifted:    bool,
) {
    if !changed { return; }
    if do_rumble {
        if let Some(ref mut gp) = *gp_cell.borrow_mut() {
            gp.rumble();
        }
    }
    let cur_sel = *sel.borrow();
    let ab = all_btns.borrow();
    let slug     = nav_audio_slug(cur_sel, layout_idx, &ab, shifted);
    let fallback = if shifted { nav_audio_slug(cur_sel, layout_idx, &ab, false) } else { String::new() };
    narrator.borrow_mut().play_with_fallback(
        &slug,
        &fallback,
        nav_tone_freq(cur_sel, &ab, audio_mode),
    );
}
// =============================================================================
// Build UI
// =============================================================================

/// Parameters passed to [`build_ui`].
pub struct BuildUiParams<'a> {
    pub cfg:              &'a config::Config,
    pub hook:             Rc<dyn KeyHook>,
    pub narrator:         Rc<RefCell<Narrator>>,
    pub audio_mode:       config::AudioMode,
    pub switch_scancodes: Rc<Vec<Vec<u8>>>,
    pub menu_item_defs:   Vec<MenuItemDef>,
    pub ble_conn_opt:     Option<Rc<RefCell<output::BleConnection>>>,
}

/// Handles to shared UI state returned by [`build_ui`].
pub struct UiHandles {
    pub all_btns:          Rc<RefCell<Vec<Vec<(Button, Action, u16, Color)>>>>,
    pub lang_btns:         Rc<RefCell<Vec<Button>>>,
    pub sel:               Rc<RefCell<NavSel>>,
    pub layout_idx:        Rc<RefCell<usize>>,
    pub mod_state:         Rc<RefCell<ModState>>,
    pub mod_btns:          Rc<RefCell<Vec<ModBtn>>>,
    pub active_nav_key:    Rc<RefCell<Option<(u16, String)>>>,
    /// Position `(row, col)` of a button that has been temporarily highlighted
    /// because a gamepad / GPIO activate-action button is currently pressed.
    /// Set on press, cleared on release.  `None` for the regular `Activate`
    /// action (the nav-selection button is already highlighted by navigation).
    pub active_btn_pressed: Rc<RefCell<Option<(usize, usize)>>>,
    pub preferred_cx:      Rc<RefCell<i32>>,
    pub menu_sel:          Rc<RefCell<Option<usize>>>,
    pub menu_item_defs:    Rc<Vec<MenuItemDef>>,
    pub menu_item_btns:    Vec<Button>,
    pub menu_group:        Group,
    pub buf:               TextBuffer,
    pub disp:              TextDisplay,
    pub gamepad_status:    Option<Frame>,
    pub gpio_status:       Option<Frame>,
    pub gp_cell:           Rc<RefCell<Option<Gamepad>>>,
    pub colors:            Colors,
    pub show_text_display: bool,
    /// Whether mouse mode is currently active.
    pub mouse_mode:        Rc<RefCell<bool>>,
    /// Status-bar indicator frame for mouse mode ("MOUSE" pill).
    pub mouse_mode_ind:    Frame,
    /// The main application window, returned so callers can register
    /// additional event handlers (e.g., the physical keyboard handler).
    pub win: Window,
}

/// Build the full keyboard UI: window, widgets, event handlers.
/// Calls `win.end()` and `win.show()` before returning.
pub fn build_ui(p: BuildUiParams) -> UiHandles {
    let cfg              = p.cfg;
    let hook             = p.hook;
    let narrator         = p.narrator;
    let audio_mode       = p.audio_mode;
    let switch_scancodes = p.switch_scancodes;
    let menu_item_defs: Rc<Vec<MenuItemDef>> = Rc::new(p.menu_item_defs);
    let ble_conn_opt     = p.ble_conn_opt;

    let layouts = keyboards::get_layouts();

    let colors = Colors::from_config(&cfg.ui.colors);
    let show_text_display = cfg.ui.show_text_display;

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

    let display_h  = if cfg.ui.show_text_display { ((sh as f32 / 12.0) as i32).max(10) } else { 0 };
    let lang_btn_h = if layouts.len() <= 1 { 0 } else { ((sh as f32 / 12.0) as i32).max(10) };

    // Status bar occupies a thin strip at the very top of the window.
    let status_h = (sh / 24).max(18).min(32);

    let kbd_h = (sh - status_h - display_h - lang_btn_h - 2 * pad - gap).min(avail_w / 3);
    // 6 rows (F-keys + 5 QWERTY rows), 5 inter-row gaps
    let key_h = (kbd_h - 5 * gap) / 6;

    let pad_top = status_h + pad + (sh - status_h - display_h - lang_btn_h - 6 * key_h - 7 * gap) / 2;
    let kbd_y = pad_top + display_h + lang_btn_h + 2 * gap;

    // Ortholinear: every key is key_w wide.
    // The widest rows (number row and QWERTY row) are 18 slots wide:
    //   14 main keys + 1 Spacer + 3 nav keys -> 17*(key_w+gap) - gap = avail_w
    //   key_w = (avail_w - 17*gap) / 17
    // Bottom row: Ctrl Win Alt [Space] AltGr Ctrl Spacer <- v -> = 9 non-Space slots
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
    let altgr_status = make_ind(ind_x, "ALTGR"); ind_x += ind_w + ind_gap;
    let mouse_mode_ind = make_ind(ind_x, "MOUSE");

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

    // BLE connectivity icon - rightmost, only shown when output mode is BLE.
    let conn_x = sw - ind_gap - ind_w;
    let mut conn_status = Frame::new(conn_x, ind_pad, ind_w, ind_h, "\u{25CF}");
    conn_status.set_color(colors.status_ind_bg);
    conn_status.set_label_color(colors.conn_disconnected); // initial: disconnected
    conn_status.set_frame(FrameType::FlatBox);
    conn_status.set_label_size(status_lbl_size);
    if !ble_mode {
        conn_status.hide();
    }

    // Gamepad icon - left of BLE icon when BLE is shown, otherwise rightmost.
    // Only created when gamepad input is enabled in config.
    let gp_icon_x = if ble_mode { conn_x - ind_gap - ind_w } else { conn_x };
    let gamepad_status: Option<Frame> = if cfg.input.gamepad.enabled {
        let mut f = Frame::new(gp_icon_x, ind_pad, ind_w, ind_h, "G");
        f.set_color(colors.status_ind_bg);
        f.set_label_color(colors.conn_disconnected); // initial: disconnected (red G)
        f.set_frame(FrameType::FlatBox);
        f.set_label_size(status_lbl_size);
        Some(f)
    } else {
        None
    };

    // GPIO icon - left of gamepad icon (if enabled) or left of BLE icon,
    // otherwise at the rightmost right-side position.
    // Only created when GPIO input is enabled in config.
    let gpio_icon_x = if cfg.input.gamepad.enabled {
        gp_icon_x - ind_gap - ind_w
    } else {
        gp_icon_x
    };
    let gpio_status: Option<Frame> = if cfg.input.gpio.enabled {
        let mut f = Frame::new(gpio_icon_x, ind_pad, ind_w, ind_h, "P");
        f.set_color(colors.status_ind_bg);
        f.set_label_color(colors.conn_disconnected); // initial: not yet opened (red P)
        f.set_frame(FrameType::FlatBox);
        f.set_label_size(status_lbl_size);
        Some(f)
    } else {
        None
    };

    // --- BLE connection-management timer (moved from hook creation) ---
    if let Some(ref ble_conn) = ble_conn_opt {
        #[derive(PartialEq, Clone, Copy)]
        enum BleState { Disconnected, Connecting, Connected }

        const BLE_RETRY_INTERVAL_S:  f64 = 1.0;
        const BLE_STATUS_INTERVAL_S: f64 = 1.0;

        let mut conn_status_t = conn_status.clone();
        let ble_conn_t = ble_conn.clone();
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
                            // Write failed -> connection lost.
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
    }

    // --- Shared state ---
    let layout_idx: Rc<RefCell<usize>>    = Rc::new(RefCell::new(0));
    let mod_state:  Rc<RefCell<ModState>> = Rc::new(RefCell::new(ModState::default()));
    // mod_btns is populated during the key loop; closures borrow it at call time.
    let mod_btns: Rc<RefCell<Vec<ModBtn>>> = Rc::new(RefCell::new(Vec::new()));
    // Tracks the (scancode, key_str) of the key currently "held" by the keyboard
    // activation key or gamepad action button.  Set on press, cleared on release.
    let active_nav_key: Rc<RefCell<Option<(u16, String)>>> = Rc::new(RefCell::new(None));
    // Tracks the (row, col) of a button that has been temporarily highlighted
    // because a gamepad / GPIO non-selection activate action is pressed (e.g.
    // ActivateEnter highlights the Enter button).  Cleared and restored on release.
    let active_btn_pressed: Rc<RefCell<Option<(usize, usize)>>> = Rc::new(RefCell::new(None));
    let buf  = TextBuffer::default();
    // --- Text display (read-only) ---
    let mut disp = TextDisplay::new(pad_left, pad_top, sw - 2 * pad_left, display_h, "");
    disp.set_buffer(buf.clone());
    disp.set_color(colors.disp_bg);
    disp.set_text_color(colors.disp_text);
    disp.set_frame(FrameType::DownBox);
    disp.set_text_size(disp_size);
    if !show_text_display {
        disp.hide();
    }

    // --- Language toggle buttons (one per entry in LAYOUTS) ---
    let active_col   = colors.mod_active;
    let inactive_col = colors.lang_btn_inactive;

    let lang_y = pad_top + display_h + gap;
    // Language buttons snap to the keyboard grid: each button is exactly one
    // grid column wide (key_w) and placed at pad + li*(key_w+gap), aligning
    // with grid columns 0, 1, 2 ...
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
    // Preferred horizontal pixel position for vertical navigation.
    // Updated whenever the cursor moves to a regular (non-Spacebar) key, or
    // horizontally to any key.  Used as the column reference when navigating
    // away from the Spacebar, whose wide centre would otherwise cause the
    // selection to drift from the column the user came from.
    let preferred_cx: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));

    if layouts.len() > 1 {
        for (li, def) in layouts.iter().enumerate() {
            let btn_x = pad_left + li as i32 * (lang_w + gap);
            let mut btn = Button::new(btn_x, lang_y, lang_w, lang_btn_h, def.name.as_str());
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
            let switch_scancodes_c = switch_scancodes.clone();
            let hook_lang_c   = Rc::clone(&hook);
            btn.set_callback(move |_| {
                // Execute the language switch.
                *layout_idx_c.borrow_mut() = li;
                for (j, lb) in lang_btns_c.borrow_mut().iter_mut().enumerate() {
                    lb.set_color(if j == li { active_col } else { inactive_col });
                }
                let def = &keyboards::get_layouts()[li];
                for (kb, slot) in switch_btns_c.borrow_mut().iter_mut() {
                    let key = &def.keys[*slot];
                    if key.label_shifted.is_empty() {
                        kb.set_label(key.label_unshifted.as_str());
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
                    &format!("lang_{}", keyboards::get_layouts()[li].name.to_lowercase()),
                    LANG_BTN_TONE_HZ,
                );
                debug_assert!(
                    li < switch_scancodes_c.len(),
                    "switch_scancodes and layouts are out of sync at index {}",
                    li,
                );
                if li < switch_scancodes_c.len() {
                    hook_lang_c.on_lang_switch(&switch_scancodes_c[li]);
                }
                app::redraw();
            });
            lang_btns.borrow_mut().push(btn);
        }
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
                    let key = &keyboards::get_layouts()[0].keys[slot];
                    if key.label_shifted.is_empty() {
                        key.label_unshifted.clone()
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
                            keyboards::get_layouts()[*layout_idx_h.borrow()].keys[slot].insert_unshifted.as_str()
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
                let layout_idx_c          = layout_idx.clone();
                let mod_state_c           = mod_state.clone();
                let mod_btns_c            = mod_btns.clone();
                let all_btns_c            = all_btns.clone();
                let lang_btns_c           = lang_btns.clone();
                let sel_c                 = sel.clone();
                let mut buf_c             = buf.clone();
                let mut disp_c            = disp.clone();
                let hook_c                = Rc::clone(&hook);
                let narrator_c            = narrator.clone();
                let audio_mode_c          = audio_mode.clone();
                let center_after_activate = cfg.navigate.center_after_activate;
                let center_key_c          = cfg.navigate.center_key.clone();
                let action                = phys.action;
                let scancode              = phys.scancode;
                btn.set_callback(move |_| {
                    // Capture shift state BEFORE execute_action, which releases
                    // sticky modifiers (including Shift) for non-modifier keys.
                    let shifted_pre = mod_state_c.borrow().is_shifted();
                    let key_str = execute_action(
                        action, scancode,
                        *layout_idx_c.borrow(),
                        &mut buf_c, &mut disp_c, &hook_c,
                        &mod_state_c,
                        &mod_btns_c.borrow(),
                        colors,
                        show_text_display,
                    );
                    // For mouse/touch clicks the Released event fires before this
                    // callback, so the key press command was sent in execute_action ->
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
                    // If center_after_activate is set, jump selection to center key
                    // and narrate the new position.  Otherwise narrate the activated
                    // button itself (existing behaviour).
                    let jumped = if center_after_activate {
                        if let Some(center) = {
                            let ab = all_btns_c.borrow();
                            find_center_key(&ab, *layout_idx_c.borrow(), &center_key_c)
                        } {
                            let changed = {
                                let mut ab = all_btns_c.borrow_mut();
                                let mut lb = lang_btns_c.borrow_mut();
                                let mut s  = sel_c.borrow_mut();
                                nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                            };
                            if changed {
                                let sel = *sel_c.borrow();
                                let ab  = all_btns_c.borrow();
                                let shifted_c = mod_state_c.borrow().is_shifted();
                                let slug = nav_audio_slug(sel, *layout_idx_c.borrow(), &ab, shifted_c);
                                let fallback = if shifted_c { nav_audio_slug(sel, *layout_idx_c.borrow(), &ab, false) } else { String::new() };
                                narrator_c.borrow_mut().play_with_fallback(
                                    &slug,
                                    &fallback,
                                    nav_tone_freq(sel, &ab, &audio_mode_c),
                                );
                            }
                            changed
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if !jumped {
                        let slug = action_audio_slug(action, *layout_idx_c.borrow(), shifted_pre);
                        let fallback = if shifted_pre { action_audio_slug(action, *layout_idx_c.borrow(), false) } else { String::new() };
                        narrator_c.borrow_mut().play_with_fallback(
                            &slug,
                            &fallback,
                            action_tone_hz(action, &audio_mode_c),
                        );
                    }
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
                    base_col,
                    status,
                });
            }

            btn_row.push((btn, phys.action, phys.scancode, base_col));
            x += w + gap;
        }
        all_btns.borrow_mut().push(btn_row);
    }

    // --- Initial navigation highlight ---
    // Start at the configured center key when:
    //   * absolute_axes is enabled (joystick covers the full axis range and
    //     must start at the physical centre of the grid), or
    //   * center_after_activate is enabled (every activation returns to center,
    //     so it is natural to also begin there).
    // Otherwise start at the top-left key (row 0, col 0).
    let (init_row, init_col) = {
        let ab = all_btns.borrow();
        if (cfg.input.gamepad.absolute_axes || cfg.navigate.center_after_activate)
            && !ab.is_empty()
        {
            if let Some(NavSel::Key(r, c)) =
                find_center_key(&ab, *layout_idx.borrow(), &cfg.navigate.center_key)
            {
                (r, c)
            } else {
                // fallback: midpoint of the grid
                let mid_row = ab.len() / 2;
                (mid_row, ab[mid_row].len() / 2)
            }
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
            &nav_audio_slug(NavSel::Key(init_row, init_col), *layout_idx.borrow(), &ab, false),
            nav_tone_freq(NavSel::Key(init_row, init_col), &ab, &audio_mode),
        );
    }

    // --- Shared gamepad cell (also used by the keyboard handler for rumble) ---
    // Created here (before the keyboard handler closure) so both the keyboard
    // handler and the gamepad polling timer can share the same instance.
    let gp_cell: Rc<RefCell<Option<Gamepad>>> = Rc::new(RefCell::new(None));

    // Mouse mode: when true, directional inputs move the mouse pointer rather
    // than navigating the on-screen keyboard.
    let mouse_mode: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

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
    //   * mouse clicks fire the item's callback directly, and
    //   * keyboard focus can be moved here so Space/Enter reach the button
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

    win.end();
    win.show();

    UiHandles {
        all_btns,
        lang_btns,
        sel,
        layout_idx,
        mod_state,
        mod_btns,
        active_nav_key,
        active_btn_pressed,
        preferred_cx,
        menu_sel,
        menu_item_defs,
        menu_item_btns,
        menu_group,
        buf,
        disp,
        gamepad_status,
        gpio_status,
        gp_cell,
        colors,
        show_text_display,
        mouse_mode,
        mouse_mode_ind,
        win,
    }
}
