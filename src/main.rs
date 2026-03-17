mod config;
mod display;
mod gamepad;
mod gpio;
mod keyboards;
mod narrator;
mod output;
mod phys_keyboard;
mod user_input;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use fltk::{app, prelude::*};
use keyboards::{Action, REGULAR_KEY_COUNT};
use gamepad::Gamepad;
use gpio::GpioInput;
use narrator::Narrator;
use user_input::{UserInputAction, UserInputEvent};
use display::{
    NavSel, MenuItemDef,
    menu_first_enabled, menu_move_sel, menu_set_item_colors,
    nav_set, nav_move, find_center_key, find_btn_by_action,
    execute_action,
    on_nav_changed,
    BuildUiParams, build_ui,
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
/// There are two kinds of notifications:
///
/// * **Raw events** - `on_key_press` / `on_key_release` fire for every GUI
///   push/release event (mouse button down, mouse button up).  They may fire
///   twice per logical key action when the keyboard is operated by mouse or
///   touch (once from the widget's raw event handler and once from the
///   action-execution callback).  These are provided for hooks that need
///   immediate, low-latency feedback (e.g. audio click).
///
/// * **Action events** - `on_key_action` fires exactly once per logical key
///   action, after modifier state has been resolved.  `modifier_bits` carries
///   the USB HID modifier byte that was active at the time of the action:
///     bit 0 (0x01) = LEFTCTRL
///     bit 1 (0x02) = LEFTSHIFT
///     bit 2 (0x04) = LEFTALT
///     bit 5 (0x20) = RIGHTSHIFT
///     bit 6 (0x40) = RIGHTALT (AltGr)
///   This is the correct callback to use for hardware output (uinput, BLE, ...).
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

    /// Called when the user switches to a language layout.
    /// `switch_scancodes` is [modifier_byte, hid_keycode].
    /// If len < 2, nothing should be sent.
    fn on_lang_switch(&self, switch_scancodes: &[u8]) {
        let _ = switch_scancodes;
    }

    /// Called to send a mouse HID movement/click report.
    ///
    /// `buttons` is the USB HID mouse button byte (bit 0 = left, bit 1 = right,
    /// bit 2 = middle).  `dx` / `dy` are signed pixel deltas.
    /// The default implementation does nothing.
    fn on_mouse_report(&self, buttons: u8, dx: i8, dy: i8) {
        let _ = (buttons, dx, dy);
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
// Unified input event processing
// =============================================================================

use fltk::{button::Button, enums::Color, frame::Frame, group::Group, text::{TextBuffer, TextDisplay}};
use display::{Colors, ModBtn, ModState};

/// All shared UI state needed to process a `UserInputEvent`.
///
/// Both the gamepad and the GPIO input sources share the same set of UI widgets
/// and logical state (current selection, modifier state, menu, ...).  Bundling
/// them here lets [`process_input_events`] be called from any number of input
/// source callbacks without repeating the context-dispatching logic.
pub(crate) struct InputCtx {
    all_btns:              Rc<RefCell<Vec<Vec<(Button, Action, u16, Color)>>>>,
    lang_btns:             Rc<RefCell<Vec<Button>>>,
    layout_idx:            Rc<RefCell<usize>>,
    mod_state:             Rc<RefCell<ModState>>,
    mod_btns:              Rc<RefCell<Vec<ModBtn>>>,
    sel:                   Rc<RefCell<NavSel>>,
    buf:                   TextBuffer,
    disp:                  TextDisplay,
    hook:                  Rc<dyn KeyHook>,
    active_nav_key:        Rc<RefCell<Option<(u16, String)>>>,
    active_btn_pressed:    Rc<RefCell<Option<(usize, usize)>>>,
    /// Gamepad cell -- used for optional rumble feedback on navigation change.
    gp_cell:               Rc<RefCell<Option<Gamepad>>>,
    narrator:              Rc<RefCell<Narrator>>,
    audio_mode:            config::AudioMode,
    menu_sel:              Rc<RefCell<Option<usize>>>,
    menu_item_defs:        Rc<Vec<MenuItemDef>>,
    menu_item_btns:        Vec<Button>,
    menu_group:            Group,
    rollover:              bool,
    center_key:            String,
    center_after_activate: bool,
    preferred_cx:          Rc<RefCell<i32>>,
    show_text_display:     bool,
    mouse_mode:            Rc<RefCell<bool>>,
    mouse_mode_ind:        Frame,
    mouse_cfg:             config::MouseConfig,
    colors:                Colors,
}

impl Clone for InputCtx {
    fn clone(&self) -> Self {
        InputCtx {
            all_btns:              self.all_btns.clone(),
            lang_btns:             self.lang_btns.clone(),
            layout_idx:            self.layout_idx.clone(),
            mod_state:             self.mod_state.clone(),
            mod_btns:              self.mod_btns.clone(),
            sel:                   self.sel.clone(),
            buf:                   self.buf.clone(),
            disp:                  self.disp.clone(),
            hook:                  Rc::clone(&self.hook),
            active_nav_key:        self.active_nav_key.clone(),
            active_btn_pressed:    self.active_btn_pressed.clone(),
            gp_cell:               self.gp_cell.clone(),
            narrator:              self.narrator.clone(),
            audio_mode:            self.audio_mode.clone(),
            menu_sel:              self.menu_sel.clone(),
            menu_item_defs:        self.menu_item_defs.clone(),
            menu_item_btns:        self.menu_item_btns.clone(),
            menu_group:            self.menu_group.clone(),
            rollover:              self.rollover,
            center_key:            self.center_key.clone(),
            center_after_activate: self.center_after_activate,
            preferred_cx:          self.preferred_cx.clone(),
            show_text_display:     self.show_text_display,
            mouse_mode:            self.mouse_mode.clone(),
            mouse_mode_ind:        self.mouse_mode_ind.clone(),
            mouse_cfg:             self.mouse_cfg.clone(),
            colors:                self.colors,
        }
    }
}

/// Per-input-source mouse-movement auto-repeat state.
///
/// Each physical input source (gamepad, GPIO, ...) keeps its own independent
/// mouse-movement state so that simultaneous use of two sources does not
/// interfere.
pub(crate) struct MouseMoveState {
    dx:    i8,
    dy:    i8,
    start: Option<Instant>,
    next:  Option<Instant>,
}

impl MouseMoveState {
    pub(crate) fn new() -> Self {
        MouseMoveState { dx: 0, dy: 0, start: None, next: None }
    }

    /// Stop all active movement (e.g. when leaving mouse mode).
    fn stop(&mut self) {
        self.dx = 0;
        self.dy = 0;
        self.start = None;
        self.next  = None;
    }
}

/// Process a slice of `UserInputEvent`s against the current UI context.
///
/// This is the single place where context switching (virtual-keyboard mode,
/// mouse mode, menu mode) is handled.  All physical input sources (gamepad,
/// GPIO, physical keyboard) call this function after converting their
/// hardware-specific events into `UserInputEvent` values.
///
/// `rumble` -- whether to trigger gamepad force-feedback on navigation changes.
/// Pass `true` only for the gamepad source.
pub(crate) fn process_input_events(
    events:      &[UserInputEvent],
    ctx:         &mut InputCtx,
    mouse:       &mut MouseMoveState,
    rumble:      bool,
) {
    let colors = ctx.colors;

    for evt in events {
        match evt.action {
            UserInputAction::Menu => {
                if !evt.pressed { continue; }
                if ctx.menu_sel.borrow().is_some() {
                    // Menu is open: close it.
                    *ctx.menu_sel.borrow_mut() = None;
                    ctx.menu_group.hide();
                    app::redraw();
                } else {
                    // Menu is closed: open it if any items are enabled.
                    if let Some(first) = menu_first_enabled(&ctx.menu_item_defs) {
                        if let Some((sc, ks)) = ctx.active_nav_key.borrow_mut().take() {
                            ctx.hook.on_key_release(sc, &ks);
                        }
                        *ctx.menu_sel.borrow_mut() = Some(first);
                        menu_set_item_colors(
                            Some(first), &ctx.menu_item_defs,
                            &mut ctx.menu_item_btns, colors,
                        );
                        let _ = ctx.menu_item_btns[first].take_focus();
                        ctx.menu_group.show();
                        app::redraw();
                    }
                }
            }

            UserInputAction::Up
            | UserInputAction::Down
            | UserInputAction::Left
            | UserInputAction::Right => {
                if !evt.pressed {
                    // Release: stop mouse movement in this direction.
                    if *ctx.mouse_mode.borrow() {
                        let (ddx, ddy) = dir_to_mouse_delta(evt.action);
                        if ddx != 0 && mouse.dx == ddx { mouse.dx = 0; }
                        if ddy != 0 && mouse.dy == ddy { mouse.dy = 0; }
                        if mouse.dx == 0 && mouse.dy == 0 {
                            mouse.start = None;
                            mouse.next  = None;
                        }
                    }
                    continue;
                }
                // When menu is open, route vertical nav to menu.
                if ctx.menu_sel.borrow().is_some() {
                    let dir = match evt.action {
                        UserInputAction::Up   => -1i32,
                        UserInputAction::Down =>  1i32,
                        _                     => continue, // ignore left/right in menu
                    };
                    let cur  = ctx.menu_sel.borrow().unwrap();
                    let next = menu_move_sel(cur, dir, &ctx.menu_item_defs);
                    if next != cur {
                        *ctx.menu_sel.borrow_mut() = Some(next);
                        menu_set_item_colors(
                            Some(next), &ctx.menu_item_defs,
                            &mut ctx.menu_item_btns, colors,
                        );
                        let _ = ctx.menu_item_btns[next].take_focus();
                        app::redraw();
                    }
                    continue;
                }
                // Mouse mode: send a mouse movement report.
                if *ctx.mouse_mode.borrow() {
                    let (ddx, ddy) = dir_to_mouse_delta(evt.action);
                    if ddx != 0 { mouse.dx = ddx; }
                    if ddy != 0 { mouse.dy = ddy; }
                    let now = Instant::now();
                    if mouse.start.is_none() {
                        mouse.start = Some(now);
                    }
                    let interval = std::time::Duration::from_millis(ctx.mouse_cfg.repeat_interval);
                    mouse.next = Some(now + interval);
                    ctx.hook.on_mouse_report(0, ddx, ddy);
                    continue;
                }
                let (dr, dc) = match evt.action {
                    UserInputAction::Up    => (-1,  0),
                    UserInputAction::Down  => ( 1,  0),
                    UserInputAction::Left  => ( 0, -1),
                    _                      => ( 0,  1), // Right
                };
                let changed = {
                    let mut ab = ctx.all_btns.borrow_mut();
                    let mut lb = ctx.lang_btns.borrow_mut();
                    let mut s  = ctx.sel.borrow_mut();
                    nav_move(
                        &mut ab, &mut lb,
                        *ctx.layout_idx.borrow(),
                        &mut s, &ctx.mod_state,
                        dr, dc,
                        colors,
                        ctx.rollover,
                        &mut *ctx.preferred_cx.borrow_mut(),
                    )
                };
                on_nav_changed(
                    changed, rumble, &ctx.gp_cell, &ctx.sel,
                    &ctx.all_btns, *ctx.layout_idx.borrow(),
                    &ctx.narrator, &ctx.audio_mode,
                    ctx.mod_state.borrow().is_shifted(),
                );
            }

            UserInputAction::Activate => {
                // When menu is open, Activate executes the selected item.
                if ctx.menu_sel.borrow().is_some() {
                    if evt.pressed {
                        let idx = ctx.menu_sel.borrow().unwrap();
                        if (ctx.menu_item_defs[idx].is_enabled)() {
                            (ctx.menu_item_defs[idx].execute)();
                        }
                        *ctx.menu_sel.borrow_mut() = None;
                        ctx.menu_group.hide();
                        app::redraw();
                    }
                    continue;
                }
                // Mouse mode: left button press/release.
                if *ctx.mouse_mode.borrow() {
                    ctx.hook.on_mouse_report(if evt.pressed { 0x01 } else { 0x00 }, 0, 0);
                    continue;
                }
                if evt.pressed {
                    let cur_sel = *ctx.sel.borrow();
                    match cur_sel {
                        NavSel::Lang(li) => {
                            ctx.lang_btns.borrow_mut()[li].do_callback();
                            *ctx.active_nav_key.borrow_mut() = None;
                        }
                        NavSel::Key(row, col) => {
                            let (action, scancode) = {
                                let ab = ctx.all_btns.borrow();
                                (ab[row][col].1, ab[row][col].2)
                            };
                            let key_str = execute_action(
                                action, scancode,
                                *ctx.layout_idx.borrow(),
                                &mut ctx.buf, &mut ctx.disp, &ctx.hook,
                                &ctx.mod_state,
                                &ctx.mod_btns.borrow(),
                                colors,
                                ctx.show_text_display,
                            );
                            *ctx.active_nav_key.borrow_mut() = Some((scancode, key_str));
                            ctx.all_btns.borrow_mut()[row][col].0.set_color(colors.nav_sel);
                            app::redraw();
                        }
                    }
                    maybe_center_after_activate(ctx, rumble);
                } else {
                    if let Some((sc, ks)) = ctx.active_nav_key.borrow_mut().take() {
                        ctx.hook.on_key_release(sc, &ks);
                    }
                }
            }

            UserInputAction::ActivateEnter => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                activate_direct_key(
                    evt.pressed, Action::Enter, 0x1c, ctx, rumble,
                );
            }

            UserInputAction::ActivateSpace => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                activate_direct_key(
                    evt.pressed, Action::Space, 0x39, ctx, rumble,
                );
            }

            UserInputAction::ActivateArrowLeft
            | UserInputAction::ActivateArrowRight
            | UserInputAction::ActivateArrowUp
            | UserInputAction::ActivateArrowDown => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                let (arrow_action, arrow_sc) = match evt.action {
                    UserInputAction::ActivateArrowLeft  => (Action::ArrowLeft,  0x69u16),
                    UserInputAction::ActivateArrowRight => (Action::ArrowRight, 0x6au16),
                    UserInputAction::ActivateArrowUp    => (Action::ArrowUp,    0x67u16),
                    _                                   => (Action::ArrowDown,  0x6cu16),
                };
                activate_direct_key(evt.pressed, arrow_action, arrow_sc, ctx, rumble);
            }

            UserInputAction::ActivateBksp => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                activate_direct_key(
                    evt.pressed, Action::Backspace, 0x0e, ctx, rumble,
                );
            }

            UserInputAction::ActivateShift
            | UserInputAction::ActivateCtrl
            | UserInputAction::ActivateAlt
            | UserInputAction::ActivateAltGr => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                // Mouse mode: ActivateShift = right mouse button.
                if *ctx.mouse_mode.borrow()
                    && evt.action == UserInputAction::ActivateShift
                {
                    ctx.hook.on_mouse_report(if evt.pressed { 0x02 } else { 0x00 }, 0, 0);
                    continue;
                }
                if evt.pressed {
                    {
                        let mut ms = ctx.mod_state.borrow_mut();
                        match evt.action {
                            UserInputAction::ActivateShift => ms.lshift = true,
                            UserInputAction::ActivateCtrl  => ms.ctrl   = true,
                            UserInputAction::ActivateAlt   => ms.alt    = true,
                            _                              => ms.altgr  = true,
                        }
                    }
                    let cur_sel = *ctx.sel.borrow();
                    match cur_sel {
                        NavSel::Lang(li) => {
                            ctx.lang_btns.borrow_mut()[li].do_callback();
                            *ctx.active_nav_key.borrow_mut() = None;
                        }
                        NavSel::Key(row, col) => {
                            let (action, scancode) = {
                                let ab = ctx.all_btns.borrow();
                                (ab[row][col].1, ab[row][col].2)
                            };
                            let key_str = execute_action(
                                action, scancode,
                                *ctx.layout_idx.borrow(),
                                &mut ctx.buf, &mut ctx.disp, &ctx.hook,
                                &ctx.mod_state,
                                &ctx.mod_btns.borrow(),
                                colors,
                                ctx.show_text_display,
                            );
                            *ctx.active_nav_key.borrow_mut() = Some((scancode, key_str));
                            ctx.all_btns.borrow_mut()[row][col].0.set_color(colors.nav_sel);
                            app::redraw();
                        }
                    }
                    maybe_center_after_activate(ctx, rumble);
                } else {
                    if let Some((sc, ks)) = ctx.active_nav_key.borrow_mut().take() {
                        ctx.hook.on_key_release(sc, &ks);
                    }
                }
            }

            UserInputAction::NavigateCenter => {
                if !evt.pressed { continue; }
                if ctx.menu_sel.borrow().is_some() { continue; }
                if let Some(center) = {
                    let ab = ctx.all_btns.borrow();
                    find_center_key(&ab, *ctx.layout_idx.borrow(), &ctx.center_key)
                } {
                    let changed = {
                        let mut ab = ctx.all_btns.borrow_mut();
                        let mut lb = ctx.lang_btns.borrow_mut();
                        let mut s  = ctx.sel.borrow_mut();
                        nav_set(
                            &mut ab, &mut lb,
                            *ctx.layout_idx.borrow(),
                            &mut s, &ctx.mod_state,
                            center,
                            colors,
                        )
                    };
                    on_nav_changed(
                        changed, rumble, &ctx.gp_cell, &ctx.sel,
                        &ctx.all_btns, *ctx.layout_idx.borrow(),
                        &ctx.narrator, &ctx.audio_mode,
                        ctx.mod_state.borrow().is_shifted(),
                    );
                }
            }

            UserInputAction::MouseToggle => {
                if !evt.pressed { continue; }
                let new_val = !*ctx.mouse_mode.borrow();
                *ctx.mouse_mode.borrow_mut() = new_val;
                if !new_val {
                    mouse.stop();
                }
                if new_val {
                    ctx.mouse_mode_ind.set_label_color(colors.status_ind_active_text);
                } else {
                    ctx.mouse_mode_ind.set_label_color(colors.status_ind_text);
                }
                app::redraw();
            }

            UserInputAction::AbsolutePos { horiz, vert } => {
                if ctx.menu_sel.borrow().is_some() { continue; }
                let new_sel = {
                    let ab  = ctx.all_btns.borrow();
                    let lb  = ctx.lang_btns.borrow();
                    let num_rows = ab.len();
                    let num_lang = lb.len();
                    if num_rows == 0 { continue; }
                    let has_lang    = num_lang > 0;
                    let total_bands = if has_lang { 1 + num_rows } else { num_rows };

                    let (center_band, center_horiz_frac) =
                        match find_center_key(&ab, *ctx.layout_idx.borrow(), &ctx.center_key) {
                            Some(NavSel::Key(row, col)) => {
                                let band = if has_lang { row + 1 } else { row };
                                let frac = (col as f32 + 0.5) / ab[row].len() as f32;
                                (band, frac)
                            }
                            _ => (total_bands / 2, 0.5f32),
                        };

                    let cv = (center_band as f32 + 0.5) / total_bands as f32;
                    let mapped_vert = if vert <= 0.5 {
                        vert * (cv / 0.5)
                    } else {
                        cv + (vert - 0.5) * ((1.0 - cv) / 0.5)
                    };
                    let band = (mapped_vert * total_bands as f32)
                        .floor()
                        .clamp(0.0, total_bands as f32 - 1.0) as usize;

                    let ch = center_horiz_frac;
                    let mapped_horiz = if horiz <= 0.5 {
                        horiz * (ch / 0.5)
                    } else {
                        ch + (horiz - 0.5) * ((1.0 - ch) / 0.5)
                    };

                    if has_lang && band == 0 {
                        let li = (mapped_horiz * num_lang as f32)
                            .floor()
                            .clamp(0.0, num_lang as f32 - 1.0) as usize;
                        NavSel::Lang(li)
                    } else {
                        let row     = if has_lang { band - 1 } else { band };
                        let num_cols = ab[row].len();
                        let col = (mapped_horiz * num_cols as f32)
                            .floor()
                            .clamp(0.0, num_cols as f32 - 1.0) as usize;
                        NavSel::Key(row, col)
                    }
                };
                #[cfg(debug_assertions)]
                if new_sel != *ctx.sel.borrow() {
                    match new_sel {
                        NavSel::Lang(li) =>
                            eprintln!(
                                "[gamepad] abs_pos horiz={:.3} vert={:.3} -> lang={}",
                                horiz, vert, li
                            ),
                        NavSel::Key(row, col) =>
                            eprintln!(
                                "[gamepad] abs_pos horiz={:.3} vert={:.3} -> row={} col={}",
                                horiz, vert, row, col
                            ),
                    }
                }
                let changed = {
                    let mut ab = ctx.all_btns.borrow_mut();
                    let mut lb = ctx.lang_btns.borrow_mut();
                    let mut s  = ctx.sel.borrow_mut();
                    nav_set(
                        &mut ab, &mut lb,
                        *ctx.layout_idx.borrow(),
                        &mut s, &ctx.mod_state,
                        new_sel,
                        colors,
                    )
                };
                on_nav_changed(
                    changed, rumble, &ctx.gp_cell, &ctx.sel,
                    &ctx.all_btns, *ctx.layout_idx.borrow(),
                    &ctx.narrator, &ctx.audio_mode,
                    ctx.mod_state.borrow().is_shifted(),
                );
            }
        }
    }
}

/// Send auto-repeat mouse-movement reports if a direction is still active.
///
/// Called once per timer tick (after [`process_input_events`]) to keep the
/// pointer moving smoothly when a directional button is held.
fn mouse_auto_repeat(ctx: &InputCtx, mouse: &mut MouseMoveState) {
    if mouse.dx == 0 && mouse.dy == 0 { return; }
    let now = Instant::now();
    if let Some(next) = mouse.next {
        if now >= next {
            let elapsed_ms = mouse.start
                .map_or(0, |s| now.duration_since(s).as_millis() as u64);
            let max_size = ctx.mouse_cfg.move_max_size.max(1) as u64;
            let ramp_ms  = ctx.mouse_cfg.move_max_time.max(1);
            let delta = ((elapsed_ms.min(ramp_ms) * max_size / ramp_ms) as i8).max(1);
            let dx = if mouse.dx > 0 { delta } else if mouse.dx < 0 { -delta } else { 0i8 };
            let dy = if mouse.dy > 0 { delta } else if mouse.dy < 0 { -delta } else { 0i8 };
            ctx.hook.on_mouse_report(0, dx, dy);
            let interval = std::time::Duration::from_millis(ctx.mouse_cfg.repeat_interval);
            mouse.next = Some(now + interval);
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers used by process_input_events
// ---------------------------------------------------------------------------

/// Convert a directional `UserInputAction` to `(dx, dy)` mouse-movement deltas.
#[inline]
fn dir_to_mouse_delta(action: UserInputAction) -> (i8, i8) {
    match action {
        UserInputAction::Up    => ( 0i8, -1i8),
        UserInputAction::Down  => ( 0i8,  1i8),
        UserInputAction::Left  => (-1i8,  0i8),
        _                      => ( 1i8,  0i8), // Right
    }
}

/// Activate a "direct" key (Enter, Space, arrows, Backspace): highlight it,
/// execute the action on press, restore and release on release.
fn activate_direct_key(
    pressed:      bool,
    action:       Action,
    scancode:     u16,
    ctx:          &mut InputCtx,
    rumble:       bool,
) {
    if pressed {
        let btn_pos = find_btn_by_action(&ctx.all_btns.borrow(), action);
        if let Some((r, c)) = btn_pos {
            ctx.all_btns.borrow_mut()[r][c].0.set_color(ctx.colors.nav_sel);
            *ctx.active_btn_pressed.borrow_mut() = Some((r, c));
        }
        let key_str = execute_action(
            action, scancode,
            *ctx.layout_idx.borrow(),
            &mut ctx.buf, &mut ctx.disp, &ctx.hook,
            &ctx.mod_state, &ctx.mod_btns.borrow(), ctx.colors,
            ctx.show_text_display,
        );
        *ctx.active_nav_key.borrow_mut() = Some((scancode, key_str));
        maybe_center_after_activate(ctx, rumble);
    } else {
        if let Some((r, c)) = ctx.active_btn_pressed.borrow_mut().take() {
            let restore = {
                let ab = ctx.all_btns.borrow();
                if *ctx.sel.borrow() == NavSel::Key(r, c) { ctx.colors.nav_sel }
                else { ab[r][c].3 }
            };
            ctx.all_btns.borrow_mut()[r][c].0.set_color(restore);
            app::redraw();
        }
        if let Some((sc, ks)) = ctx.active_nav_key.borrow_mut().take() {
            ctx.hook.on_key_release(sc, &ks);
        }
    }
}

/// If `center_after_activate` is configured, move the navigation cursor to the
/// center key after an activation.
fn maybe_center_after_activate(ctx: &mut InputCtx, rumble: bool) {
    if !ctx.center_after_activate { return; }
    if let Some(center) = {
        let ab = ctx.all_btns.borrow();
        find_center_key(&ab, *ctx.layout_idx.borrow(), &ctx.center_key)
    } {
        let colors  = ctx.colors;
        let changed = {
            let mut ab = ctx.all_btns.borrow_mut();
            let mut lb = ctx.lang_btns.borrow_mut();
            let mut s  = ctx.sel.borrow_mut();
            nav_set(
                &mut ab, &mut lb,
                *ctx.layout_idx.borrow(),
                &mut s, &ctx.mod_state,
                center,
                colors,
            )
        };
        on_nav_changed(
            changed, rumble, &ctx.gp_cell, &ctx.sel,
            &ctx.all_btns, *ctx.layout_idx.borrow(),
            &ctx.narrator, &ctx.audio_mode,
            ctx.mod_state.borrow().is_shifted(),
        );
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let cfg = config::Config::load();

    // Determine config directory for keymap file lookup.
    let config_dir = std::env::var("SMART_KBD_CONFIG_PATH")
        .unwrap_or_else(|_| ".".into());

    // Load active layouts (from TOML files or built-in fallbacks).
    let active_keymaps = cfg.ui.active_keymaps.clone();
    let loaded_layouts = keyboards::load_active_layouts(&active_keymaps, &config_dir);
    keyboards::set_layouts(loaded_layouts);
    let layouts = keyboards::get_layouts();

    debug_assert!(
        layouts.iter().all(|l| l.keys.len() == REGULAR_KEY_COUNT),
        "every LayoutDef must have exactly REGULAR_KEY_COUNT entries"
    );

    // Build switch_scancodes: per-layout key combination to send on language switch.
    let switch_scancodes: Rc<Vec<Vec<u8>>> = Rc::new(
        active_keymaps.iter().map(|name| {
            match cfg.keymap.get(name) {
                None     => keyboards::default_switch_scancode_for(name),
                Some(kc) => kc.switch_scancode.clone(),
            }
        }).collect()
    );

    // Build the narrator early so it can be cloned into closures below.
    let narrator = Rc::new(RefCell::new(Narrator::new(cfg.output.audio.clone())));
    // Clone the audio mode so it can be captured independently by closures.
    let audio_mode = cfg.output.audio.clone();

    let a = app::App::default().with_scheme(app::Scheme::Gleam);

    let ble_mode = matches!(cfg.output.mode, config::OutputMode::Ble);
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
                ble_cfg.key_release_delay,
                ble_cfg.lang_switch_release_delay,
            );
            ble_conn_opt = Some(ble_conn);
            Rc::new(ble_hook)
        }
    };

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

    // "Quit Smart Keyboard": always enabled; terminates the application.
    menu_item_defs.push(MenuItemDef {
        label:      "Quit Smart Keyboard",
        is_enabled: Box::new(|| true),
        execute:    Box::new(|| {
            app::quit();
        }),
    });

    let mut ui = build_ui(BuildUiParams {
        cfg: &cfg,
        hook: hook.clone(),
        narrator: narrator.clone(),
        audio_mode: audio_mode.clone(),
        switch_scancodes: switch_scancodes.clone(),
        menu_item_defs,
        ble_conn_opt,
    });

    let colors = ui.colors;

    // Build the shared input-event context from the UI handles.  Both the
    // gamepad and GPIO input sources will use this same context.
    let shared_ctx = InputCtx {
        all_btns:              ui.all_btns.clone(),
        lang_btns:             ui.lang_btns.clone(),
        layout_idx:            ui.layout_idx.clone(),
        mod_state:             ui.mod_state.clone(),
        mod_btns:              ui.mod_btns.clone(),
        sel:                   ui.sel.clone(),
        buf:                   ui.buf.clone(),
        disp:                  ui.disp.clone(),
        hook:                  Rc::clone(&hook),
        active_nav_key:        ui.active_nav_key.clone(),
        active_btn_pressed:    ui.active_btn_pressed.clone(),
        gp_cell:               ui.gp_cell.clone(),
        narrator:              narrator.clone(),
        audio_mode:            audio_mode.clone(),
        menu_sel:              ui.menu_sel.clone(),
        menu_item_defs:        ui.menu_item_defs.clone(),
        menu_item_btns:        ui.menu_item_btns.clone(),
        menu_group:            ui.menu_group.clone(),
        rollover:              cfg.navigate.rollover,
        center_key:            cfg.navigate.center_key.clone(),
        center_after_activate: cfg.navigate.center_after_activate,
        preferred_cx:          ui.preferred_cx.clone(),
        show_text_display:     ui.show_text_display,
        mouse_mode:            ui.mouse_mode.clone(),
        mouse_mode_ind:        ui.mouse_mode_ind.clone(),
        mouse_cfg:             cfg.mouse.clone(),
        colors,
    };

    // --- Physical keyboard navigation ---
    phys_keyboard::setup_keyboard_handler(
        &mut ui.win,
        config::NavKeys::from_config(&cfg.input.keyboard),
        shared_ctx.clone(),
    );

    // --- Gamepad input (if enabled in config) ---
    if cfg.input.gamepad.enabled {
        let gp_cfg    = cfg.input.gamepad.clone();
        let gp_rumble = cfg.input.gamepad.rumble;

        // Open the gamepad now and store it in the shared gp_cell.
        *ui.gp_cell.borrow_mut() = Gamepad::open(&cfg.input.gamepad);

        // Update the initial gamepad icon state based on whether the device
        // was found at startup.
        if let Some(ref mut icon) = ui.gamepad_status {
            if ui.gp_cell.borrow().is_some() {
                icon.set_label_color(colors.conn_connected);
            }
            // If not connected the icon already shows red (set at creation).
        }

        let mut gamepad_status_t = ui.gamepad_status.clone();
        let gp_cell_t            = ui.gp_cell.clone();

        // Clone the shared context for capture in the closure.
        let mut ctx_gp = shared_ctx.clone();

        let mut gp_evt_buf:    Vec<UserInputEvent> = Vec::new();
        let mut mouse_gp = MouseMoveState::new();

        // Poll at ~60 Hz; this keeps input latency low without burning CPU
        // the way an idle callback would.  When the gamepad is disconnected
        // the timer slows to 1 Hz and retries opening the device.
        app::add_timeout3(0.016, move |handle| {
            // Phase 1 - reconnect if currently disconnected.
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

            // Phase 2 - poll for events; detect disconnection.
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

            // Phase 3 - process the events collected in Phase 2.
            process_input_events(&gp_evt_buf, &mut ctx_gp, &mut mouse_gp, gp_rumble);

            // Phase 4 - mouse-mode auto-repeat.
            mouse_auto_repeat(&ctx_gp, &mut mouse_gp);

            app::repeat_timeout3(0.016, handle);
        });
    }

    // --- GPIO input (if enabled in config) ---
    if cfg.input.gpio.enabled {
        let gpio_cfg = cfg.input.gpio.clone();

        // Open the GPIO lines now and store the result in a shared cell.
        let gpio_cell: Rc<RefCell<Option<GpioInput>>> =
            Rc::new(RefCell::new(GpioInput::open(&cfg.input.gpio)));

        // Update the initial GPIO icon colour.
        if let Some(ref mut icon) = ui.gpio_status {
            if gpio_cell.borrow().is_some() {
                icon.set_label_color(colors.conn_connected);
            }
            // If not opened the icon already shows red (set at creation).
        }

        let mut gpio_status_t = ui.gpio_status.clone();
        let gpio_cell_t       = gpio_cell.clone();

        // Clone the shared context for capture in the closure.
        let mut ctx_gpio = shared_ctx.clone();

        let mut gpio_evt_buf:  Vec<UserInputEvent> = Vec::new();
        let mut mouse_gpio = MouseMoveState::new();

        // Poll at ~60 Hz.  When lines are not yet open, retry every 1 s.
        app::add_timeout3(0.016, move |handle| {
            // Phase 1 - try to open if currently unavailable.
            if gpio_cell_t.borrow().is_none() {
                match GpioInput::open(&gpio_cfg) {
                    Some(gpio) => {
                        eprintln!("[gpio] opened");
                        *gpio_cell_t.borrow_mut() = Some(gpio);
                        if let Some(ref mut icon) = gpio_status_t {
                            icon.set_label_color(colors.conn_connected);
                            app::redraw();
                        }
                    }
                    None => {
                        app::repeat_timeout3(1.0, handle);
                        return;
                    }
                }
            }

            // Phase 2 - poll for events.
            {
                let mut opt = gpio_cell_t.borrow_mut();
                opt.as_mut().unwrap().poll(&mut gpio_evt_buf);
            }

            // Phase 3 - process collected events.
            process_input_events(&gpio_evt_buf, &mut ctx_gpio, &mut mouse_gpio, false);

            // Phase 4 - mouse-mode auto-repeat.
            mouse_auto_repeat(&ctx_gpio, &mut mouse_gpio);

            app::repeat_timeout3(0.016, handle);
        });
    }

    a.run().unwrap();
}
