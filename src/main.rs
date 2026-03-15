mod config;
mod display;
mod gamepad;
mod gpio;
mod keyboards;
mod narrator;
mod output;

use std::cell::RefCell;
use std::rc::Rc;

use fltk::{app, prelude::*};
use keyboards::{Action, REGULAR_KEY_COUNT};
use gamepad::{Gamepad, GamepadAction, GamepadEvent};
use gpio::{GpioInput, GpioAction, GpioEvent};
use narrator::Narrator;
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

    if cfg.input.gamepad.enabled {
        // Clone config for use inside the reconnection closure.
        let gp_cfg = cfg.input.gamepad.clone();
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

        let all_btns_c        = ui.all_btns.clone();
        let lang_btns_c       = ui.lang_btns.clone();
        let layout_idx_c      = ui.layout_idx.clone();
        let mod_state_c       = ui.mod_state.clone();
        let mod_btns_c        = ui.mod_btns.clone();
        let sel_c             = ui.sel.clone();
        let mut buf_c         = ui.buf.clone();
        let mut disp_c        = ui.disp.clone();
        let hook_c            = Rc::clone(&hook);
        let active_nav_key_c  = ui.active_nav_key.clone();
        let active_btn_pressed_gp = ui.active_btn_pressed.clone();
        let mut gamepad_status_t = ui.gamepad_status.clone();
        let gp_cell_t         = ui.gp_cell.clone();
        let narrator_t        = narrator.clone();
        let audio_mode_t      = audio_mode.clone();
        let menu_sel_gp       = ui.menu_sel.clone();
        let menu_items_gp     = ui.menu_item_defs.clone();
        let mut menu_item_btns_gp = ui.menu_item_btns.clone();
        let mut menu_group_gp = ui.menu_group.clone();
        let gp_rollover             = cfg.navigate.rollover;
        let gp_center_key           = cfg.navigate.center_key.clone();
        let gp_center_after_activate = cfg.navigate.center_after_activate;
        let preferred_cx_gp         = ui.preferred_cx.clone();
        let show_text_display_gp    = ui.show_text_display;

        // Reuse a single Vec across poll calls to avoid repeated allocation.
        let mut gp_evt_buf: Vec<GamepadEvent> = Vec::new();

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
                                &mut *preferred_cx_gp.borrow_mut(),
                            )
                        };
                        on_nav_changed(
                            changed, gp_rumble, &gp_cell_t, &sel_c,
                            &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                            mod_state_c.borrow().is_shifted(),
                        );
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
                                        show_text_display_gp,
                                    );
                                    // Store the activated key so on_key_release can be
                                    // sent when the gamepad button is released.
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    // Re-apply nav_sel (execute_action may have changed
                                    // the colour for modifier keys) and redraw.
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                    app::redraw();
                                }
                            }
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
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
                            // Highlight the Enter button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Enter,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gp.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Enter, 0x1c,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gp,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x1c, key_str));
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gp.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateSpace => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Highlight the Space button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Space,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gp.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Space, 0x39,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gp,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x39, key_str));
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gp.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateArrowLeft
                    | GamepadAction::ActivateArrowRight
                    | GamepadAction::ActivateArrowUp
                    | GamepadAction::ActivateArrowDown => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        let (arrow_action, arrow_sc) = match evt.action {
                            GamepadAction::ActivateArrowLeft  => (Action::ArrowLeft,  0x69u16),
                            GamepadAction::ActivateArrowRight => (Action::ArrowRight, 0x6au16),
                            GamepadAction::ActivateArrowUp    => (Action::ArrowUp,    0x67u16),
                            _                                 => (Action::ArrowDown,  0x6cu16),
                        };
                        if evt.pressed {
                            // Highlight the corresponding arrow button.
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), arrow_action,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gp.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                arrow_action, arrow_sc,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gp,
                            );
                            *active_nav_key_c.borrow_mut() = Some((arrow_sc, key_str));
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gp.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GamepadAction::ActivateBksp => {
                        // Ignore while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Highlight the Backspace button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Backspace,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gp.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Backspace, 0x0e,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gp,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x0e, key_str));
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gp.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
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
                                        show_text_display_gp,
                                    );
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                    app::redraw();
                                }
                            }
                            if gp_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, gp_rumble, &gp_cell_t, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
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
                            find_center_key(&ab, *layout_idx_c.borrow(), &gp_center_key)
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
                            on_nav_changed(
                                changed, gp_rumble, &gp_cell_t, &sel_c,
                                &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                mod_state_c.borrow().is_shifted(),
                            );
                        }
                    }
                    GamepadAction::AbsolutePos { horiz, vert } => {
                        // Ignore absolute-position events while menu is open.
                        if menu_sel_gp.borrow().is_some() { continue; }
                        // Map normalised 0.0...1.0 coordinates to the full
                        // selectable area, which consists of the language-toggle
                        // strip followed by the keyboard key rows.
                        //
                        // The mapping is piecewise-linear: the configured
                        // `center_key` maps to joystick centre (0.5, 0.5).
                        // Each half of the axis range is mapped linearly to
                        // the corresponding half of the grid on either side of
                        // the center key.
                        let new_sel = {
                            let ab = all_btns_c.borrow();
                            let lb = lang_btns_c.borrow();
                            let num_rows = ab.len();
                            let num_lang = lb.len();
                            if num_rows == 0 { continue; }
                            let has_lang = num_lang > 0;
                            let total_bands = if has_lang { 1 + num_rows } else { num_rows };

                            // Determine center key's band and horizontal fraction.
                            let (center_band, center_horiz_frac) =
                                match find_center_key(
                                    &ab, *layout_idx_c.borrow(), &gp_center_key,
                                ) {
                                    Some(NavSel::Key(row, col)) => {
                                        let band = if has_lang { row + 1 } else { row };
                                        let frac = (col as f32 + 0.5) / ab[row].len() as f32;
                                        (band, frac)
                                    }
                                    _ => (total_bands / 2, 0.5f32),
                                };

                            // Piecewise linear vertical remapping: 0.5 -> center_band.
                            let cv = (center_band as f32 + 0.5) / total_bands as f32;
                            let mapped_vert = if vert <= 0.5 {
                                vert * (cv / 0.5)
                            } else {
                                cv + (vert - 0.5) * ((1.0 - cv) / 0.5)
                            };
                            let band = (mapped_vert * total_bands as f32)
                                .floor()
                                .clamp(0.0, total_bands as f32 - 1.0) as usize;

                            // Piecewise linear horizontal remapping: 0.5 -> center_horiz_frac.
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
                                let row = if has_lang { band - 1 } else { band };
                                let num_cols = ab[row].len();
                                let col = (mapped_horiz * num_cols as f32)
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
                        on_nav_changed(
                            changed, gp_rumble, &gp_cell_t, &sel_c,
                            &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                            mod_state_c.borrow().is_shifted(),
                        );
                    }
                }
            }
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

        let all_btns_c        = ui.all_btns.clone();
        let lang_btns_c       = ui.lang_btns.clone();
        let layout_idx_c      = ui.layout_idx.clone();
        let mod_state_c       = ui.mod_state.clone();
        let mod_btns_c        = ui.mod_btns.clone();
        let sel_c             = ui.sel.clone();
        let mut buf_c         = ui.buf.clone();
        let mut disp_c        = ui.disp.clone();
        let hook_c            = Rc::clone(&hook);
        let active_nav_key_c  = ui.active_nav_key.clone();
        let active_btn_pressed_gpio = ui.active_btn_pressed.clone();
        let mut gpio_status_t = ui.gpio_status.clone();
        let gpio_cell_t       = gpio_cell.clone();
        let gp_cell_gpio      = ui.gp_cell.clone();
        let narrator_t        = narrator.clone();
        let audio_mode_t      = audio_mode.clone();
        let menu_sel_gpio     = ui.menu_sel.clone();
        let menu_items_gpio   = ui.menu_item_defs.clone();
        let mut menu_item_btns_gpio = ui.menu_item_btns.clone();
        let mut menu_group_gpio     = ui.menu_group.clone();
        let gpio_rollover             = cfg.navigate.rollover;
        let gpio_center_key           = cfg.navigate.center_key.clone();
        let gpio_center_after_activate = cfg.navigate.center_after_activate;
        let preferred_cx_gpio         = ui.preferred_cx.clone();
        let show_text_display_gpio    = ui.show_text_display;

        let mut gpio_evt_buf: Vec<GpioEvent> = Vec::new();

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
            for evt in gpio_evt_buf.iter() {
                match evt.action {
                    GpioAction::Menu => {
                        if !evt.pressed { continue; }
                        if menu_sel_gpio.borrow().is_some() {
                            *menu_sel_gpio.borrow_mut() = None;
                            menu_group_gpio.hide();
                            app::redraw();
                        } else {
                            if let Some(first) = menu_first_enabled(&menu_items_gpio) {
                                if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                    hook_c.on_key_release(sc, &ks);
                                }
                                *menu_sel_gpio.borrow_mut() = Some(first);
                                menu_set_item_colors(
                                    Some(first), &menu_items_gpio,
                                    &mut menu_item_btns_gpio, colors,
                                );
                                let _ = menu_item_btns_gpio[first].take_focus();
                                menu_group_gpio.show();
                                app::redraw();
                            }
                        }
                    }
                    GpioAction::Up
                    | GpioAction::Down
                    | GpioAction::Left
                    | GpioAction::Right => {
                        if !evt.pressed { continue; }
                        if menu_sel_gpio.borrow().is_some() {
                            let dir = match evt.action {
                                GpioAction::Up   => -1i32,
                                GpioAction::Down =>  1i32,
                                _                => continue,
                            };
                            let cur = menu_sel_gpio.borrow().unwrap();
                            let next = menu_move_sel(cur, dir, &menu_items_gpio);
                            if next != cur {
                                *menu_sel_gpio.borrow_mut() = Some(next);
                                menu_set_item_colors(
                                    Some(next), &menu_items_gpio,
                                    &mut menu_item_btns_gpio, colors,
                                );
                                let _ = menu_item_btns_gpio[next].take_focus();
                                app::redraw();
                            }
                            continue;
                        }
                        let (dr, dc) = match evt.action {
                            GpioAction::Up    => (-1,  0),
                            GpioAction::Down  => ( 1,  0),
                            GpioAction::Left  => ( 0, -1),
                            _                 => ( 0,  1),
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
                                gpio_rollover,
                                &mut *preferred_cx_gpio.borrow_mut(),
                            )
                        };
                        on_nav_changed(
                            changed, false, &gp_cell_gpio, &sel_c,
                            &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                            mod_state_c.borrow().is_shifted(),
                        );
                    }
                    GpioAction::Activate => {
                        if menu_sel_gpio.borrow().is_some() {
                            if evt.pressed {
                                let idx = menu_sel_gpio.borrow().unwrap();
                                if (menu_items_gpio[idx].is_enabled)() {
                                    (menu_items_gpio[idx].execute)();
                                }
                                *menu_sel_gpio.borrow_mut() = None;
                                menu_group_gpio.hide();
                                app::redraw();
                            }
                            continue;
                        }
                        if evt.pressed {
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
                                        show_text_display_gpio,
                                    );
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    // Re-apply nav_sel and redraw.
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                    app::redraw();
                                }
                            }
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::ActivateEnter => {
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Highlight the Enter button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Enter,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gpio.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Enter, 0x1c,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gpio,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x1c, key_str));
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gpio.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::ActivateSpace => {
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Highlight the Space button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Space,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gpio.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Space, 0x39,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gpio,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x39, key_str));
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gpio.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::ActivateArrowLeft
                    | GpioAction::ActivateArrowRight
                    | GpioAction::ActivateArrowUp
                    | GpioAction::ActivateArrowDown => {
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        let (arrow_action, arrow_sc) = match evt.action {
                            GpioAction::ActivateArrowLeft  => (Action::ArrowLeft,  0x69u16),
                            GpioAction::ActivateArrowRight => (Action::ArrowRight, 0x6au16),
                            GpioAction::ActivateArrowUp    => (Action::ArrowUp,    0x67u16),
                            _                              => (Action::ArrowDown,  0x6cu16),
                        };
                        if evt.pressed {
                            // Highlight the corresponding arrow button.
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), arrow_action,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gpio.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                arrow_action, arrow_sc,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gpio,
                            );
                            *active_nav_key_c.borrow_mut() = Some((arrow_sc, key_str));
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gpio.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::ActivateBksp => {
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        if evt.pressed {
                            // Highlight the Backspace button while it is "pressed".
                            if let Some((r, c)) = find_btn_by_action(
                                &all_btns_c.borrow(), Action::Backspace,
                            ) {
                                all_btns_c.borrow_mut()[r][c].0.set_color(colors.nav_sel);
                                *active_btn_pressed_gpio.borrow_mut() = Some((r, c));
                            }
                            let key_str = execute_action(
                                Action::Backspace, 0x0e,
                                *layout_idx_c.borrow(),
                                &mut buf_c, &mut disp_c, &hook_c,
                                &mod_state_c, &mod_btns_c.borrow(), colors,
                                show_text_display_gpio,
                            );
                            *active_nav_key_c.borrow_mut() = Some((0x0e, key_str));
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((r, c)) = active_btn_pressed_gpio.borrow_mut().take() {
                                let restore = {
                                    let ab = all_btns_c.borrow();
                                    if *sel_c.borrow() == NavSel::Key(r, c) { colors.nav_sel }
                                    else { ab[r][c].3 }
                                };
                                all_btns_c.borrow_mut()[r][c].0.set_color(restore);
                                app::redraw();
                            }
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::ActivateShift
                    | GpioAction::ActivateCtrl
                    | GpioAction::ActivateAlt
                    | GpioAction::ActivateAltGr => {
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        if evt.pressed {
                            {
                                let mut ms = mod_state_c.borrow_mut();
                                match evt.action {
                                    GpioAction::ActivateShift => ms.lshift = true,
                                    GpioAction::ActivateCtrl  => ms.ctrl   = true,
                                    GpioAction::ActivateAlt   => ms.alt    = true,
                                    _                         => ms.altgr  = true,
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
                                        show_text_display_gpio,
                                    );
                                    *active_nav_key_c.borrow_mut() =
                                        Some((scancode, key_str));
                                    all_btns_c.borrow_mut()[row][col]
                                        .0.set_color(colors.nav_sel);
                                    app::redraw();
                                }
                            }
                            if gpio_center_after_activate {
                                if let Some(center) = {
                                    let ab = all_btns_c.borrow();
                                    find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
                                } {
                                    let changed = {
                                        let mut ab = all_btns_c.borrow_mut();
                                        let mut lb = lang_btns_c.borrow_mut();
                                        let mut s  = sel_c.borrow_mut();
                                        nav_set(&mut ab, &mut lb, *layout_idx_c.borrow(), &mut s, &mod_state_c, center, colors)
                                    };
                                    on_nav_changed(
                                        changed, false, &gp_cell_gpio, &sel_c,
                                        &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                        mod_state_c.borrow().is_shifted(),
                                    );
                                }
                            }
                        } else {
                            if let Some((sc, ks)) = active_nav_key_c.borrow_mut().take() {
                                hook_c.on_key_release(sc, &ks);
                            }
                        }
                    }
                    GpioAction::NavigateCenter => {
                        if !evt.pressed { continue; }
                        if menu_sel_gpio.borrow().is_some() { continue; }
                        if let Some(center) = {
                            let ab = all_btns_c.borrow();
                            find_center_key(&ab, *layout_idx_c.borrow(), &gpio_center_key)
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
                            on_nav_changed(
                                changed, false, &gp_cell_gpio, &sel_c,
                                &all_btns_c, *layout_idx_c.borrow(), &narrator_t, &audio_mode_t,
                                mod_state_c.borrow().is_shifted(),
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
