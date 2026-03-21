// src/display.rs
//
// Display-related types, navigation logic, action execution, and audio helpers
// for the on-screen keyboard.  UI rendering is delegated to the iced framework;
// this module contains only data types and pure logic.

use iced::Color;

use crate::{config, KeyHook};
use crate::keyboards::{self, is_modifier, special_hook_str, special_label,
    Action, KW, KEYS};
use crate::gamepad::Gamepad;
use crate::narrator::Narrator;



// =============================================================================
// UI colour palette (resolved from config)
// =============================================================================

/// All UI colours resolved from [`config::ColorsConfig`] into [`iced::Color`] values.
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
        let c = |rgb: &config::ColorRgb| Color::from_rgb8(rgb.0, rgb.1, rgb.2);
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
// Button data (replaces FLTK widget tuples)
// =============================================================================

/// Data for a single keyboard key, without any widget reference.
pub struct BtnData {
    pub action:     Action,
    pub scancode:   u16,
    pub base_color: Color,
    pub x:          i32,
    pub w:          i32,
}

/// Data for a language-toggle button, without any widget reference.
pub struct LangBtnData {
    pub name: String,
    pub x:    i32,
    pub w:    i32,
}

// =============================================================================
// Modifier button descriptor
// =============================================================================

/// A modifier-key entry together with its grid position and base colour.
pub struct ModBtn {
    pub row:        usize,
    pub col:        usize,
    pub action:     Action,
    pub base_color: Color,
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

/// Identifies which button currently holds the navigation highlight.
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
pub fn closest_col(row: &[BtnData], cx: i32) -> usize {
    closest_to_cx(row.iter().map(|b| (b.x, b.w)), cx)
}

/// Find the index in the lang-button strip whose x-range best covers pixel centre `cx`.
pub fn closest_lang(lang_btns: &[LangBtnData], cx: i32) -> usize {
    closest_to_cx(lang_btns.iter().map(|b| (b.x, b.w)), cx)
}

/// Apply a specific navigation selection.
///
/// Does nothing if `new_sel` equals the current `*sel`.
/// Returns `true` if the selection changed, `false` if it was already at `new_sel`.
pub fn nav_set(
    sel:     &mut NavSel,
    new_sel: NavSel,
) -> bool {
    if new_sel == *sel {
        return false;
    }
    *sel = new_sel;
    true
}

/// Move the keyboard-navigation cursor.
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
    all_btns:     &[Vec<BtnData>],
    lang_btns:    &[LangBtnData],
    sel:          &mut NavSel,
    dr:           i32,
    dc:           i32,
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
                        let cx = lang_btns[li].x + lang_btns[li].w / 2;
                        *preferred_cx = cx;
                        NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                    }
                } else {
                    // Already at the top edge.
                    NavSel::Lang(li)
                }
            } else if dr > 0 {
                // Down into the first keyboard row, pixel-aligned.
                let cx = lang_btns[li].x + lang_btns[li].w / 2;
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
                let cx = all_btns[row][col].x + all_btns[row][col].w / 2;
                *preferred_cx = cx;
                if !lang_btns.is_empty() {
                    let li = closest_lang(lang_btns, cx);
                    let lb = &lang_btns[li];
                    if cx >= lb.x && cx < lb.x + lb.w {
                        NavSel::Lang(li)
                    } else if rollover {
                        let rows = all_btns.len();
                        let mut found = NavSel::Key(row, col);
                        for scan in (0..rows).rev() {
                            let best = closest_col(&all_btns[scan], cx);
                            let b = &all_btns[scan][best];
                            if cx >= b.x && cx < b.x + b.w {
                                found = NavSel::Key(scan, best);
                                break;
                            }
                        }
                        found
                    } else {
                        NavSel::Key(row, col)
                    }
                } else if rollover {
                    let rows = all_btns.len();
                    NavSel::Key(rows - 1, closest_col(&all_btns[rows - 1], cx))
                } else {
                    NavSel::Key(row, col)
                }
            } else if dr != 0 {
                let rows = all_btns.len();
                let cx = if all_btns[row][col].action == Action::Space {
                    *preferred_cx
                } else {
                    let c = all_btns[row][col].x + all_btns[row][col].w / 2;
                    *preferred_cx = c;
                    c
                };
                let mut scan     = row;
                let mut dest_row = row;
                loop {
                    let next_raw = scan as i32 + dr;
                    if next_raw < 0 || next_raw >= rows as i32 {
                        if rollover {
                            if dr > 0 {
                                if !lang_btns.is_empty() {
                                    dest_row = rows; // sentinel: go to lang strip
                                } else {
                                    dest_row = rows + 1; // sentinel: wrap to row 0
                                }
                            } else {
                                dest_row = rows + 2; // sentinel: roll over upward
                            }
                        }
                        break;
                    }
                    scan = next_raw as usize;
                    let best_col = closest_col(&all_btns[scan], cx);
                    let b = &all_btns[scan][best_col];
                    let dist = if cx >= b.x && cx < b.x + b.w {
                        0
                    } else if cx < b.x {
                        b.x - cx
                    } else {
                        cx - (b.x + b.w)
                    };
                    if dist == 0 {
                        dest_row = scan;
                        break;
                    }
                }
                if dest_row == rows {
                    // Sentinel: wrap to lang strip or row 0.
                    if all_btns[row][col].action == Action::Space {
                        NavSel::Key(0, closest_col(&all_btns[0], cx))
                    } else {
                        let li = closest_lang(lang_btns, cx);
                        let lb = &lang_btns[li];
                        if cx >= lb.x && cx < lb.x + lb.w {
                            NavSel::Lang(li)
                        } else {
                            let mut found = NavSel::Key(row, col);
                            for scan in 0..rows {
                                let best = closest_col(&all_btns[scan], cx);
                                let b = &all_btns[scan][best];
                                if cx >= b.x && cx < b.x + b.w {
                                    found = NavSel::Key(scan, best);
                                    break;
                                }
                            }
                            found
                        }
                    }
                } else if dest_row == rows + 1 {
                    NavSel::Key(0, closest_col(&all_btns[0], cx))
                } else if dest_row == rows + 2 {
                    // Went up past row 0 with cx not covered by any row-0 button.
                    if !lang_btns.is_empty() {
                        let li = closest_lang(lang_btns, cx);
                        let lb = &lang_btns[li];
                        if cx >= lb.x && cx < lb.x + lb.w {
                            NavSel::Lang(li)
                        } else {
                            let mut found = NavSel::Key(row, col);
                            for scan in (0..rows).rev() {
                                let best = closest_col(&all_btns[scan], cx);
                                let b = &all_btns[scan][best];
                                if cx >= b.x && cx < b.x + b.w {
                                    found = NavSel::Key(scan, best);
                                    break;
                                }
                            }
                            found
                        }
                    } else {
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
                *preferred_cx = all_btns[row][new_col].x + all_btns[row][new_col].w / 2;
                NavSel::Key(row, new_col)
            }
        }
    };

    nav_set(sel, new_sel)
}

/// Find the key matching `center_key` label in the current layout.
///
/// Searches `all_btns` for the first key whose unshifted label (or
/// [`special_label`]) equals `center_key` (case-sensitive).  Returns the
/// `NavSel` for that key, or `None` if no match is found.
pub fn find_center_key(
    all_btns:   &[Vec<BtnData>],
    layout_idx: usize,
    center_key: &str,
) -> Option<NavSel> {
    for (r, row) in all_btns.iter().enumerate() {
        for (c, btn) in row.iter().enumerate() {
            let label = match btn.action {
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
pub fn find_btn_by_action(
    all_btns:      &[Vec<BtnData>],
    target_action: Action,
) -> Option<(usize, usize)> {
    for (r, row) in all_btns.iter().enumerate() {
        for (c, btn) in row.iter().enumerate() {
            if btn.action == target_action {
                return Some((r, c));
            }
        }
    }
    None
}

// =============================================================================
// Action execution
// =============================================================================

/// Describes a text-buffer edit returned by [`execute_action`].
pub enum TextEdit {
    Append(String),
    Clear,
    DeleteLast,
}

/// Result of [`execute_action`], carrying all information needed for the caller
/// to update UI state without requiring any widget references.
pub struct ActionResult {
    pub key_str:                String,
    /// Which modifier was toggled and its new on/off state.
    pub modifier_toggled:       Option<(Action, bool)>,
    /// Peer modifier that was auto-released (e.g. RShift when LShift toggled).
    pub modifier_peer_released: Option<Action>,
    /// Text-buffer operation to perform (only set for non-modifier keys when
    /// `update_display` is `true`).
    pub text_edit:              Option<TextEdit>,
}

/// Perform the action of a key: notify hooks, update text buffer, update modifiers.
///
/// Returns an [`ActionResult`] so the caller can update the UI accordingly.
///
/// `text_buf` is the mutable text buffer (replaces FLTK TextBuffer).
/// `update_display` controls whether text edits are applied.
pub fn execute_action(
    action:         Action,
    scancode:       u16,
    layout_i:       usize,
    text_buf:       &mut String,
    hook:           &dyn KeyHook,
    mod_state:      &mut ModState,
    update_display: bool,
) -> ActionResult {
    // For Regular keys, compute the text to insert respecting Shift / CapsLock.
    let regular_text: Option<String> = if let Action::Regular(slot) = action {
        let key = &keyboards::get_layouts()[layout_i].keys[slot];
        Some(if !key.label_shifted.is_empty() && (mod_state.lshift || mod_state.rshift) {
            key.insert_shifted.clone()
        } else if key.label_shifted.is_empty() && (mod_state.caps || mod_state.lshift || mod_state.rshift) {
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
    let modifier_bits: u8 =
        (if mod_state.ctrl   { 0x01 } else { 0 })
            | (if mod_state.lshift { 0x02 } else { 0 })
            | (if mod_state.alt    { 0x04 } else { 0 })
            | (if mod_state.win    { 0x08 } else { 0 })
            | (if mod_state.rshift { 0x20 } else { 0 })
            | (if mod_state.altgr  { 0x40 } else { 0 });

    let mut result = ActionResult {
        key_str:                key_str.to_string(),
        modifier_toggled:       None,
        modifier_peer_released: None,
        text_edit:              None,
    };

    if is_modifier(action) {
        let now_active = mod_state.toggle(action);
        result.modifier_toggled = Some((action, now_active));

        // Synchronize paired modifier keys.
        let peer = {
            let p = mod_state.release_shift_peer(action);
            if p.is_some() { p } else { mod_state.release_alt_peer(action) }
        };
        result.modifier_peer_released = peer;
    } else {
        // Non-modifier key: apply text edit and auto-release sticky modifiers.
        if update_display {
            let edit = match action {
                Action::Regular(_) => {
                    let text = regular_text.as_deref().unwrap_or("").to_string();
                    text_buf.push_str(&text);
                    Some(TextEdit::Append(text))
                }
                Action::Backspace => {
                    if !text_buf.is_empty() {
                        text_buf.pop();
                    }
                    Some(TextEdit::DeleteLast)
                }
                Action::Tab   => { text_buf.push('\t'); Some(TextEdit::Append("\t".to_string())) }
                Action::Enter => { text_buf.clear();    Some(TextEdit::Clear) }
                Action::Space => { text_buf.push(' ');  Some(TextEdit::Append(" ".to_string())) }
                _ => None,
            };
            result.text_edit = edit;
        }

        mod_state.release_sticky();
    }

    hook.on_key_action(scancode, key_str, modifier_bits);
    result
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
    all_btns: &[Vec<BtnData>],
    shifted: bool,
) -> String {
    match sel {
        NavSel::Lang(li) => {
            format!("lang_{}", keyboards::get_layouts()[li].name.to_lowercase())
        }
        NavSel::Key(row, col) => action_audio_slug(all_btns[row][col].action, layout_idx, shifted),
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
        Action::Regular(slot) => match slot {
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
            29 | 32 => 987.77,  // B5 (F and J)
            _ => 880.00,        // A5
        },
        Action::F1  =>  55.00,
        Action::F2  =>  65.41,
        Action::F3  =>  73.42,
        Action::F4  =>  82.41,
        Action::F5  =>  98.00,
        Action::F6  => 110.00,
        Action::F7  => 130.81,
        Action::F8  => 146.83,
        Action::F9  => 164.81,
        Action::F10 => 196.00,
        Action::F11 => 220.00,
        Action::F12 => 246.94,
        Action::Esc        => 1760.00,
        Action::Backspace  =>  415.30,
        Action::Tab        =>  369.99,
        Action::CapsLock   =>  932.33,
        Action::Enter      =>  554.37,
        Action::LShift | Action::RShift => 311.13,
        Action::Ctrl       =>  277.18,
        Action::Win        =>  174.61,
        Action::Alt        =>  233.08,
        Action::AltGr      =>  207.65,
        Action::Space      => 1046.50,
        Action::Insert     => 1318.51,
        Action::Delete     => 1244.51,
        Action::Home       => 1174.66,
        Action::End        => 1108.73,
        Action::PageUp     => 1396.91,
        Action::PageDown   => 1567.98,
        Action::ArrowUp    =>  783.99,
        Action::ArrowDown  =>  739.99,
        Action::ArrowLeft  =>  698.46,
        Action::ArrowRight =>  622.25,
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
            1..=10 => tone_freq_for_action(action),
            29 | 32 => 987.77,  // B5
            _ => 0.0,
        },
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
    all_btns: &[Vec<BtnData>],
    mode: &config::AudioMode,
) -> f32 {
    match sel {
        NavSel::Lang(_) => LANG_BTN_TONE_HZ,
        NavSel::Key(row, col) => action_tone_hz(all_btns[row][col].action, mode),
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
    gp:         &mut Option<Gamepad>,
    sel:        NavSel,
    all_btns:   &[Vec<BtnData>],
    layout_idx: usize,
    narrator:   &mut Narrator,
    audio_mode: &config::AudioMode,
    shifted:    bool,
) {
    if !changed { return; }
    if do_rumble {
        if let Some(ref mut g) = *gp {
            g.rumble();
        }
    }
    let slug     = nav_audio_slug(sel, layout_idx, all_btns, shifted);
    let fallback = if shifted { nav_audio_slug(sel, layout_idx, all_btns, false) } else { String::new() };
    narrator.play_with_fallback(
        &slug,
        &fallback,
        nav_tone_freq(sel, all_btns, audio_mode),
    );
}

// =============================================================================
// Layout metrics
// =============================================================================

/// Pre-computed layout dimensions and positions derived from screen size and
/// configuration.  Mirrors the layout computation that was previously embedded
/// in the FLTK `build_ui` function.
pub struct LayoutMetrics {
    pub sw: i32,
    pub sh: i32,
    pub key_w: i32,
    pub key_h: i32,
    pub space_w: i32,
    pub pad_left: i32,
    pub pad_top: i32,
    pub kbd_y: i32,
    pub gap: i32,
    pub pad: i32,
    pub lbl_size: i32,
    pub big_lbl_size: i32,
    pub disp_size: i32,
    pub btn_size: i32,
    pub status_h: i32,
    pub status_lbl_size: i32,
    pub ind_w: i32,
    pub ind_h: i32,
    pub ind_gap: i32,
    pub ind_pad: i32,
    pub display_h: i32,
    pub lang_btn_h: i32,
    pub lang_w: i32,
    pub lang_y: i32,
}

/// Compute layout positions and sizes from screen dimensions and configuration.
pub fn compute_layout(sw: i32, sh: i32, cfg: &config::Config) -> LayoutMetrics {
    let layouts = keyboards::get_layouts();

    let pad = 3i32;
    let gap = 1i32;

    let avail_w = sw - 2 * pad;

    let display_h = if cfg.ui.show_text_display {
        ((sh as f32 / 12.0) as i32).max(10)
    } else {
        0
    };
    let lang_btn_h = if layouts.len() <= 1 {
        0
    } else {
        ((sh as f32 / 12.0) as i32).max(10)
    };

    let status_h = (sh / 24).max(18).min(32);

    let kbd_h = (sh - status_h - display_h - lang_btn_h - 2 * pad - gap).min(avail_w / 3);
    let key_h = (kbd_h - 5 * gap) / 6;

    let pad_top = status_h + pad + (sh - status_h - display_h - lang_btn_h - 6 * key_h - 7 * gap) / 2;
    let kbd_y = pad_top + display_h + lang_btn_h + 2 * gap;

    let key_w   = ((avail_w - 16 * gap) / 17).max(10);
    let space_w = 6 * key_w + 5 * gap;
    let pad_left = pad + (avail_w - 17 * key_w - 16 * gap) / 2;

    let lbl_size     = (key_w / 4).max(10);
    let big_lbl_size = lbl_size * 2;
    let disp_size    = (display_h * 2 / 5).max(12).min(28);
    let btn_size     = lbl_size;

    let status_lbl_size = (lbl_size * 3 / 4).max(8);
    let ind_gap = 4i32;
    let ind_pad = 2i32;
    let ind_h   = status_h - 2 * ind_pad;
    let ind_w   = status_lbl_size * 4;

    let lang_y = pad_top + display_h + gap;
    let lang_w = key_w;

    LayoutMetrics {
        sw, sh,
        key_w, key_h, space_w,
        pad_left, pad_top, kbd_y,
        gap, pad,
        lbl_size, big_lbl_size, disp_size, btn_size,
        status_h, status_lbl_size,
        ind_w, ind_h, ind_gap, ind_pad,
        display_h, lang_btn_h, lang_w,
        lang_y,
    }
}

// =============================================================================
// Grid / button builders
// =============================================================================

/// Build the 2D grid of [`BtnData`] from the physical [`KEYS`] layout,
/// computing x positions for each button.
pub fn build_btn_grid(metrics: &LayoutMetrics, colors: &Colors) -> Vec<Vec<BtnData>> {
    let px = |kw: KW| match kw {
        KW::Space            => metrics.space_w,
        KW::Std | KW::Spacer => metrics.key_w,
    };

    let mut grid: Vec<Vec<BtnData>> = Vec::new();

    for row in KEYS.iter() {
        let mut x = metrics.pad_left;
        let mut btn_row: Vec<BtnData> = Vec::new();

        for phys in row.iter() {
            let w = px(phys.kw);

            if matches!(phys.kw, KW::Spacer) {
                x += w + metrics.gap;
                continue;
            }

            let base_color = match phys.action {
                Action::Regular(_) | Action::Space => colors.key_normal,
                _                                  => colors.key_mod,
            };

            btn_row.push(BtnData {
                action:   phys.action,
                scancode: phys.scancode,
                base_color,
                x,
                w,
            });
            x += w + metrics.gap;
        }
        grid.push(btn_row);
    }
    grid
}

/// Build the language-toggle button data from the active layouts.
pub fn build_lang_btns(metrics: &LayoutMetrics) -> Vec<LangBtnData> {
    let layouts = keyboards::get_layouts();
    if layouts.len() <= 1 {
        return Vec::new();
    }
    layouts
        .iter()
        .enumerate()
        .map(|(li, def)| LangBtnData {
            name: def.name.clone(),
            x: metrics.pad_left + li as i32 * (metrics.lang_w + metrics.gap),
            w: metrics.lang_w,
        })
        .collect()
}
