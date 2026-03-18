// src/menu.rs
//
// Full-screen menu window with its own modal event loop.
//
// When the user presses the menu key the application opens a new FLTK
// window that covers the whole screen and runs a local event loop
// (`while win.shown() { app::wait(); }`).  Because the loop is local,
// the main application's custom input handlers (gamepad, GPIO,
// physical-keyboard remapping) never fire while the menu is visible.
// The user navigates purely with a standard physical keyboard or mouse.
//
// Top-level items:
//   * Configuration   -- opens the config editor dialogue
//   * Disconnect BLE  -- only when BLE dongle is active
//   * Exit application

use std::cell::RefCell;
use std::rc::Rc;

use fltk::{
    app,
    button::{Button, CheckButton},
    enums::{Align, Color, Event, FrameType},
    frame::Frame,
    group::{Group, Scroll, ScrollType, Pack},
    input::Input,
    menu::Choice,
    prelude::*,
    window::Window,
};

use crate::config;
use crate::output;

// ---------------------------------------------------------------------------
// Colour helpers
// ---------------------------------------------------------------------------

const BG:       Color = Color::from_hex(0x77767b);
const BTN_BG:   Color = Color::from_hex(0x3c3c42);
const TEXT_FG:  Color = Color::from_hex(0xe0e0e0);
const LABEL_FG: Color = Color::from_hex(0xf6f5f4);
const TITLE_FG: Color = Color::from_hex(0xffffff);
const DISABLED: Color = Color::from_hex(0x5a5a5a);

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Open the full-screen menu window with its own modal event loop.
///
/// This function **blocks** until the menu window is closed.  While the
/// menu is visible, the main application event handlers do not fire
/// because the FLTK event loop is driven locally here.
///
/// `ble_conn_opt` should be `Some(...)` when the output mode is `"ble"` so
/// that the "Disconnect BLE" item can be shown; pass `None` otherwise.
pub fn open_menu(
    ble_conn_opt: Option<Rc<RefCell<output::BleConnection>>>,
) {
    let (sw, sh) = app::screen_size();

    let mut win = Window::new(0, 0, sw, sh, "Menu");
    win.set_color(BG);
    win.set_border(false);
    win.fullscreen(true);

    let lbl_size = ((sh as f32 / 30.0) as i32).clamp(14, 28);
    let btn_h    = ((sh as f32 / 14.0) as i32).clamp(30, 64);
    let col_w    = (sw / 2).clamp(200, 700);
    let col_x    = (sw - col_w) / 2;
    let mut cy   = (sh / 6).max(40);

    // Title
    let mut title = Frame::new(col_x, cy, col_w, btn_h, "Menu");
    title.set_label_size(lbl_size + 4);
    title.set_label_color(TITLE_FG);
    title.set_frame(FrameType::FlatBox);
    title.set_color(BG);
    cy += btn_h + 10;

    // --- "Configuration" button ---
    let mut btn_cfg = Button::new(col_x, cy, col_w, btn_h, "Configuration");
    style_btn(&mut btn_cfg, lbl_size);
    cy += btn_h + 8;

    // --- "Disconnect BLE" button (conditionally enabled) ---
    let ble_active = ble_conn_opt.is_some();
    let mut btn_ble = Button::new(col_x, cy, col_w, btn_h, "Disconnect BLE");
    style_btn(&mut btn_ble, lbl_size);
    if !ble_active {
        btn_ble.deactivate();
        btn_ble.set_label_color(DISABLED);
    }
    cy += btn_h + 8;

    // --- "Exit application" button ---
    let mut btn_exit = Button::new(col_x, cy, col_w, btn_h, "Exit application");
    style_btn(&mut btn_exit, lbl_size);

    win.end();
    win.show();

    // Give the first button keyboard focus so arrow/Tab/Enter work
    // immediately without requiring a mouse click.
    btn_cfg.take_focus().ok();

    // --- Callbacks ---

    // Local flag: set by the Configuration callback so we know to open
    // the config editor after the menu window closes.
    let open_cfg = Rc::new(RefCell::new(false));

    let open_cfg_c = open_cfg.clone();
    let mut win_cfg  = win.clone();
    let mut win_exit = win.clone();

    btn_cfg.set_callback(move |_| {
        *open_cfg_c.borrow_mut() = true;
        win_cfg.hide();
    });

    if let Some(conn) = ble_conn_opt {
        let mut win_ble2 = win.clone();
        btn_ble.set_callback(move |_| {
            if !conn.borrow_mut().send_disconnect() {
                eprintln!("[menu] Disconnect BLE: failed to send disconnect command");
            }
            win_ble2.hide();
        });
    }

    btn_exit.set_callback(move |_| {
        win_exit.hide();
        app::quit();
    });

    // Keyboard navigation: Tab and arrow keys move focus between buttons
    // (FLTK built-in).  Enter/Return activates the focused button instead
    // of moving focus (which is FLTK's default for Return).  Escape closes.
    win.handle(|w, ev| {
        if ev == Event::KeyDown {
            let key = app::event_key();
            if key == fltk::enums::Key::Escape {
                w.hide();
                return true;
            }
            if key == fltk::enums::Key::Enter {
                // Activate the currently focused widget.
                if let Some(mut focused) = app::focus() {
                    focused.do_callback();
                }
                return true;
            }
        }
        false
    });

    // --- Modal event loop: blocks until the menu window is closed. ---
    while win.shown() {
        app::wait();
    }

    // If Configuration was selected, open the config editor now
    // (after the menu window has closed).
    if *open_cfg.borrow() {
        open_config_editor();
    }
}

// ---------------------------------------------------------------------------
// Configuration editor
// ---------------------------------------------------------------------------

/// Open a full-screen configuration editor window with its own modal
/// event loop.
///
/// This function **blocks** until the editor window is closed.
/// The editor reads the current config.toml, presents every setting with an
/// appropriate widget (checkbox, choice, text input), and lets the user save
/// changes.  On save the application restarts (exec-replace).
fn open_config_editor() {
    let (sw, sh) = app::screen_size();

    let mut win = Window::new(0, 0, sw, sh, "Configuration");
    win.set_color(BG);
    win.set_border(false);
    win.fullscreen(true);

    let lbl_size = ((sh as f32 / 36.0) as i32).clamp(12, 22);
    let row_h    = ((sh as f32 / 22.0) as i32).clamp(24, 48);
    let pad      = 8;

    // Scrollable area.  The Pack fills the full Scroll width so that
    // FLTK cannot shift it horizontally when scrolling begins.
    // Visual centering is achieved by positioning children at pack_x
    // inside each row Group.
    let mut scroll = Scroll::new(0, 0, sw, sh - row_h - pad * 2, "");
    scroll.set_type(ScrollType::Vertical);
    scroll.set_color(BG);
    scroll.set_frame(FrameType::FlatBox);
    let pack_w = (sw - 40).min(900);
    let pack_x = (sw - pack_w) / 2;
    let mut pack = Pack::new(0, pad, sw, 0, "");
    pack.set_spacing(4);

    // Load current config as raw TOML text for defaults.
    let cfg = config::Config::load();

    // We collect closures that extract the current widget values.
    let collectors: Rc<RefCell<Vec<(&'static str, Box<dyn Fn() -> String>)>>> =
        Rc::new(RefCell::new(Vec::new()));

    // Collect focusable (interactive) widgets so we can wrap Tab / Shift-Tab
    // at the boundaries instead of letting focus escape to the Cancel/Save
    // buttons (which would trigger an endless auto-scroll).
    //
    // Safety: we use `Widget::from_widget_ptr()` to create generic handles
    // from concrete widget pointers.  This is safe because every widget
    // stored here lives inside the Pack which is owned by the Scroll which
    // is owned by the Window; all of them outlive the `focusables` Vec.
    let focusables: Rc<RefCell<Vec<fltk::widget::Widget>>> =
        Rc::new(RefCell::new(Vec::new()));

    // =====================================================================
    // Helper closures to add widgets
    // =====================================================================

    // -- Section header --
    let add_section = |pack: &mut Pack, label: &str, lbl_size: i32, pack_w: i32, row_h: i32| {
        let mut grp = Group::new(0, 0, pack_w, row_h, "");
        grp.set_frame(FrameType::FlatBox);
        grp.set_color(BG);
        let mut f = Frame::new(pack_x, 0, pack_w, row_h, None);
        f.set_label(label);
        f.set_label_size(lbl_size + 2);
        f.set_label_color(TITLE_FG);
        f.set_frame(FrameType::FlatBox);
        f.set_color(Color::from_hex(0x1e1e22));
        f.set_align(Align::Inside | Align::Left);
        grp.end();
        pack.add(&grp);
    };

    // -- Boolean checkbox --
    let add_bool = {
        let cols = collectors.clone();
        let focs = focusables.clone();
        move |pack: &mut Pack, key: &'static str, label: &str, val: bool, lbl_size: i32, pack_w: i32, row_h: i32| {
            let mut grp = Group::new(0, 0, pack_w, row_h, "");
            grp.set_frame(FrameType::FlatBox);
            grp.set_color(BG);
            let mut lbl = Frame::new(pack_x, 0, pack_w / 2, row_h, None);
            lbl.set_label(label);
            lbl.set_label_size(lbl_size);
            lbl.set_label_color(LABEL_FG);
            lbl.set_frame(FrameType::FlatBox);
            lbl.set_color(BG);
            lbl.set_align(Align::Inside | Align::Left);
            let mut cb = CheckButton::new(pack_x + pack_w / 2, 0, pack_w / 2, row_h, "");
            cb.set_value(val);
            cb.set_label_size(lbl_size);
            cb.set_label_color(LABEL_FG);
            cb.set_selection_color(TITLE_FG);
            cb.set_visible_focus();
            focs.borrow_mut().push(unsafe { fltk::widget::Widget::from_widget_ptr(cb.as_widget_ptr()) });
            grp.end();
            pack.add(&grp);
            let cb_c = cb.clone();
            cols.borrow_mut().push((key, Box::new(move || {
                if cb_c.value() { "true".to_string() } else { "false".to_string() }
            })));
        }
    };

    // -- Choice list --
    let add_choice = {
        let cols = collectors.clone();
        let focs = focusables.clone();
        move |pack: &mut Pack, key: &'static str, label: &str, options: &[&str], current: &str, lbl_size: i32, pack_w: i32, row_h: i32| {
            let mut grp = Group::new(0, 0, pack_w, row_h, "");
            grp.set_frame(FrameType::FlatBox);
            grp.set_color(BG);
            let mut lbl = Frame::new(pack_x, 0, pack_w / 2, row_h, None);
            lbl.set_label(label);
            lbl.set_label_size(lbl_size);
            lbl.set_label_color(LABEL_FG);
            lbl.set_frame(FrameType::FlatBox);
            lbl.set_color(BG);
            lbl.set_align(Align::Inside | Align::Left);
            let mut ch = Choice::new(pack_x + pack_w / 2, 0, pack_w / 2, row_h, "");
            ch.set_text_size(lbl_size);
            for opt in options {
                ch.add_choice(opt);
            }
            // Select the current value.
            for (i, opt) in options.iter().enumerate() {
                if *opt == current {
                    ch.set_value(i as i32);
                    break;
                }
            }
            focs.borrow_mut().push(unsafe { fltk::widget::Widget::from_widget_ptr(ch.as_widget_ptr()) });
            grp.end();
            pack.add(&grp);
            let ch_c = ch.clone();
            let opts: Vec<String> = options.iter().map(|s| s.to_string()).collect();
            cols.borrow_mut().push((key, Box::new(move || {
                let idx = ch_c.value();
                if idx >= 0 && (idx as usize) < opts.len() {
                    opts[idx as usize].clone()
                } else {
                    String::new()
                }
            })));
        }
    };

    // -- Numeric text input --
    let add_number = {
        let cols = collectors.clone();
        let focs = focusables.clone();
        move |pack: &mut Pack, key: &'static str, label: &str, val: &str, lbl_size: i32, pack_w: i32, row_h: i32| {
            let mut grp = Group::new(0, 0, pack_w, row_h, "");
            grp.set_frame(FrameType::FlatBox);
            grp.set_color(BG);
            let mut lbl = Frame::new(pack_x, 0, pack_w / 2, row_h, None);
            lbl.set_label(label);
            lbl.set_label_size(lbl_size);
            lbl.set_label_color(LABEL_FG);
            lbl.set_frame(FrameType::FlatBox);
            lbl.set_color(BG);
            lbl.set_align(Align::Inside | Align::Left);
            let mut inp = Input::new(pack_x + pack_w / 2, 0, pack_w / 2, row_h, "");
            inp.set_value(val);
            inp.set_text_size(lbl_size);
            inp.set_text_color(TEXT_FG);
            inp.set_color(Color::from_hex(0x1c1c1c));
            inp.set_cursor_color(TEXT_FG);
            focs.borrow_mut().push(unsafe { fltk::widget::Widget::from_widget_ptr(inp.as_widget_ptr()) });
            grp.end();
            pack.add(&grp);
            let inp_c = inp.clone();
            cols.borrow_mut().push((key, Box::new(move || {
                inp_c.value()
            })));
        }
    };

    // -- Text input (string) --
    let add_text = {
        let cols = collectors.clone();
        let focs = focusables.clone();
        move |pack: &mut Pack, key: &'static str, label: &str, val: &str, lbl_size: i32, pack_w: i32, row_h: i32| {
            let mut grp = Group::new(0, 0, pack_w, row_h, "");
            grp.set_frame(FrameType::FlatBox);
            grp.set_color(BG);
            let mut lbl = Frame::new(pack_x, 0, pack_w / 2, row_h, None);
            lbl.set_label(label);
            lbl.set_label_size(lbl_size);
            lbl.set_label_color(LABEL_FG);
            lbl.set_frame(FrameType::FlatBox);
            lbl.set_color(BG);
            lbl.set_align(Align::Inside | Align::Left);
            let mut inp = Input::new(pack_x + pack_w / 2, 0, pack_w / 2, row_h, "");
            inp.set_value(val);
            inp.set_text_size(lbl_size);
            inp.set_text_color(TEXT_FG);
            inp.set_color(Color::from_hex(0x1c1c1c));
            inp.set_cursor_color(TEXT_FG);
            focs.borrow_mut().push(unsafe { fltk::widget::Widget::from_widget_ptr(inp.as_widget_ptr()) });
            grp.end();
            pack.add(&grp);
            let inp_c = inp.clone();
            cols.borrow_mut().push((key, Box::new(move || {
                inp_c.value()
            })));
        }
    };

    // -- Color input (hex string) --
    let add_color = {
        let cols = collectors.clone();
        let focs = focusables.clone();
        move |pack: &mut Pack, key: &'static str, label: &str, val: &str, lbl_size: i32, pack_w: i32, row_h: i32| {
            let mut grp = Group::new(0, 0, pack_w, row_h, "");
            grp.set_frame(FrameType::FlatBox);
            grp.set_color(BG);
            let mut lbl = Frame::new(pack_x, 0, pack_w / 2, row_h, None);
            lbl.set_label(label);
            lbl.set_label_size(lbl_size);
            lbl.set_label_color(LABEL_FG);
            lbl.set_frame(FrameType::FlatBox);
            lbl.set_color(BG);
            lbl.set_align(Align::Inside | Align::Left);
            let mut inp = Input::new(pack_x + pack_w / 2, 0, pack_w / 2, row_h, "");
            inp.set_value(val);
            inp.set_text_size(lbl_size);
            inp.set_text_color(TEXT_FG);
            inp.set_color(Color::from_hex(0x1c1c1c));
            inp.set_cursor_color(TEXT_FG);
            focs.borrow_mut().push(unsafe { fltk::widget::Widget::from_widget_ptr(inp.as_widget_ptr()) });
            grp.end();
            pack.add(&grp);
            let inp_c = inp.clone();
            cols.borrow_mut().push((key, Box::new(move || {
                inp_c.value()
            })));
        }
    };

    // =====================================================================
    // Populate settings
    // =====================================================================

    // --- [input.keyboard] ---
    add_section(&mut pack, "  Input: Keyboard", lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.navigate_up",    "  Navigate Up (hex key code)", &format!("0x{:04x}", cfg.input.keyboard.navigate_up), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.navigate_down",  "  Navigate Down",  &format!("0x{:04x}", cfg.input.keyboard.navigate_down), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.navigate_left",  "  Navigate Left",  &format!("0x{:04x}", cfg.input.keyboard.navigate_left), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.navigate_right", "  Navigate Right", &format!("0x{:04x}", cfg.input.keyboard.navigate_right), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.activate",       "  Activate",       &format!("0x{:04x}", cfg.input.keyboard.activate), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.keyboard.menu",           "  Menu",           &format!("0x{:04x}", cfg.input.keyboard.menu), lbl_size, pack_w, row_h);

    // --- [input.gamepad] ---
    add_section(&mut pack, "  Input: Gamepad", lbl_size, pack_w, row_h);
    add_bool(&mut pack, "input.gamepad.enabled",  "  Enabled",  cfg.input.gamepad.enabled, lbl_size, pack_w, row_h);
    add_text(&mut pack, "input.gamepad.device",   "  Device",   &cfg.input.gamepad.device, lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gamepad.axis_threshold", "  Axis threshold", &cfg.input.gamepad.axis_threshold.to_string(), lbl_size, pack_w, row_h);
    add_bool(&mut pack, "input.gamepad.absolute_axes", "  Absolute axes", cfg.input.gamepad.absolute_axes, lbl_size, pack_w, row_h);
    add_bool(&mut pack, "input.gamepad.rumble",        "  Rumble",         cfg.input.gamepad.rumble, lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gamepad.rumble_duration_ms", "  Rumble duration (ms)", &cfg.input.gamepad.rumble_duration_ms.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gamepad.rumble_magnitude",   "  Rumble magnitude",     &cfg.input.gamepad.rumble_magnitude.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gamepad.repeat_delay_ms",    "  Repeat delay (ms)",    &cfg.input.gamepad.repeat_delay_ms.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gamepad.repeat_interval_ms", "  Repeat interval (ms)", &cfg.input.gamepad.repeat_interval_ms.to_string(), lbl_size, pack_w, row_h);

    // --- [input.gpio] ---
    add_section(&mut pack, "  Input: GPIO", lbl_size, pack_w, row_h);
    add_bool(&mut pack, "input.gpio.enabled", "  Enabled", cfg.input.gpio.enabled, lbl_size, pack_w, row_h);
    add_text(&mut pack, "input.gpio.chip",    "  Chip device", &cfg.input.gpio.chip, lbl_size, pack_w, row_h);
    {
        let sig = match cfg.input.gpio.gpio_signal {
            config::GpioSignal::High => "high",
            config::GpioSignal::Low  => "low",
        };
        add_choice(&mut pack, "input.gpio.gpio_signal", "  GPIO signal", &["low", "high"], sig, lbl_size, pack_w, row_h);
    }
    {
        let pull = match cfg.input.gpio.gpio_pull {
            config::GpioPull::Up   => "up",
            config::GpioPull::Down => "down",
            config::GpioPull::Null => "null",
        };
        add_choice(&mut pack, "input.gpio.gpio_pull", "  GPIO pull", &["null", "up", "down"], pull, lbl_size, pack_w, row_h);
    }
    add_number(&mut pack, "input.gpio.repeat_delay_ms",    "  Repeat delay (ms)",    &cfg.input.gpio.repeat_delay_ms.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "input.gpio.repeat_interval_ms", "  Repeat interval (ms)", &cfg.input.gpio.repeat_interval_ms.to_string(), lbl_size, pack_w, row_h);

    // --- [mouse] ---
    add_section(&mut pack, "  Mouse", lbl_size, pack_w, row_h);
    add_number(&mut pack, "mouse.move_max_size",    "  Move max size (px)",  &cfg.mouse.move_max_size.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "mouse.repeat_interval",  "  Repeat interval (ms)", &cfg.mouse.repeat_interval.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "mouse.move_max_time",    "  Move max time (ms)",  &cfg.mouse.move_max_time.to_string(), lbl_size, pack_w, row_h);

    // --- [navigate] ---
    add_section(&mut pack, "  Navigation", lbl_size, pack_w, row_h);
    add_bool(&mut pack, "navigate.rollover",             "  Rollover",             cfg.navigate.rollover, lbl_size, pack_w, row_h);
    add_text(&mut pack, "navigate.center_key",           "  Center key",           &cfg.navigate.center_key, lbl_size, pack_w, row_h);
    add_bool(&mut pack, "navigate.center_after_activate","  Center after activate", cfg.navigate.center_after_activate, lbl_size, pack_w, row_h);

    // --- [output] ---
    add_section(&mut pack, "  Output", lbl_size, pack_w, row_h);
    {
        let mode = match cfg.output.mode {
            config::OutputMode::Print => "print",
            config::OutputMode::Ble   => "ble",
        };
        add_choice(&mut pack, "output.mode", "  Mode", &["print", "ble"], mode, lbl_size, pack_w, row_h);
    }
    {
        let audio = match cfg.output.audio {
            config::AudioMode::None     => "none",
            config::AudioMode::Narrate  => "narrate",
            config::AudioMode::Tone     => "tone",
            config::AudioMode::ToneHint => "tone_hint",
        };
        add_choice(&mut pack, "output.audio", "  Audio", &["none", "narrate", "tone", "tone_hint"], audio, lbl_size, pack_w, row_h);
    }

    // --- [output.ble] ---
    add_section(&mut pack, "  Output: BLE", lbl_size, pack_w, row_h);
    add_number(&mut pack, "output.ble.vid",                      "  VID",                      &format!("0x{:04x}", cfg.output.ble.vid), lbl_size, pack_w, row_h);
    add_number(&mut pack, "output.ble.pid",                      "  PID",                      &format!("0x{:04x}", cfg.output.ble.pid), lbl_size, pack_w, row_h);
    add_text(&mut pack,   "output.ble.serial",                   "  Serial",                   cfg.output.ble.serial.as_deref().unwrap_or(""), lbl_size, pack_w, row_h);
    add_number(&mut pack, "output.ble.key_release_delay",        "  Key release delay (us)",   &cfg.output.ble.key_release_delay.to_string(), lbl_size, pack_w, row_h);
    add_number(&mut pack, "output.ble.lang_switch_release_delay","  Lang switch delay (us)",   &cfg.output.ble.lang_switch_release_delay.to_string(), lbl_size, pack_w, row_h);

    // --- [ui] ---
    add_section(&mut pack, "  UI", lbl_size, pack_w, row_h);
    add_bool(&mut pack, "ui.show_text_display", "  Show text display", cfg.ui.show_text_display, lbl_size, pack_w, row_h);
    add_text(&mut pack, "ui.active_keymaps",    "  Active keymaps (comma-separated)", &cfg.ui.active_keymaps.join(","), lbl_size, pack_w, row_h);

    // --- [ui.colors] ---
    add_section(&mut pack, "  UI: Colors", lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.key_normal",              "  Key normal",              &color_to_hex(cfg.ui.colors.key_normal), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.key_mod",                 "  Key modifier",            &color_to_hex(cfg.ui.colors.key_mod), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.mod_active",              "  Modifier active",         &color_to_hex(cfg.ui.colors.mod_active), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.nav_sel",                 "  Nav selection",            &color_to_hex(cfg.ui.colors.nav_sel), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.status_bar_bg",           "  Status bar BG",           &color_to_hex(cfg.ui.colors.status_bar_bg), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.status_ind_bg",           "  Status indicator BG",     &color_to_hex(cfg.ui.colors.status_ind_bg), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.status_ind_text",         "  Status indicator text",   &color_to_hex(cfg.ui.colors.status_ind_text), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.status_ind_active_text",  "  Status indicator active", &color_to_hex(cfg.ui.colors.status_ind_active_text), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.conn_disconnected",       "  Disconnected color",      &color_to_hex(cfg.ui.colors.conn_disconnected), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.conn_connecting",         "  Connecting color",         &color_to_hex(cfg.ui.colors.conn_connecting), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.conn_connected",          "  Connected color",          &color_to_hex(cfg.ui.colors.conn_connected), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.win_bg",                  "  Window BG",               &color_to_hex(cfg.ui.colors.win_bg), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.disp_bg",                 "  Display BG",              &color_to_hex(cfg.ui.colors.disp_bg), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.disp_text",               "  Display text",            &color_to_hex(cfg.ui.colors.disp_text), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.lang_btn_inactive",       "  Lang button inactive",    &color_to_hex(cfg.ui.colors.lang_btn_inactive), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.lang_btn_label",          "  Lang button label",       &color_to_hex(cfg.ui.colors.lang_btn_label), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.key_label_normal",        "  Key label normal",        &color_to_hex(cfg.ui.colors.key_label_normal), lbl_size, pack_w, row_h);
    add_color(&mut pack, "ui.colors.key_label_mod",           "  Key label modifier",      &color_to_hex(cfg.ui.colors.key_label_mod), lbl_size, pack_w, row_h);

    pack.end();

    // The Pack was created with height 0.  FLTK Pack only auto-sizes
    // during draw(), which is too late for the parent Scroll to determine
    // the scrollable area.  Compute and set the height explicitly.
    let n_rows = pack.children();
    if n_rows > 0 {
        let spacing = pack.spacing();
        let total_h = n_rows as i32 * row_h + (n_rows as i32 - 1) * spacing;
        pack.resize(pack.x(), pack.y(), sw, total_h);
    }

    scroll.end();

    // --- Bottom bar: Cancel / Save & Reload ---
    let bar_y  = sh - row_h - pad;
    let half_w = pack_w / 2 - 4;

    let mut btn_cancel = Button::new(pack_x, bar_y, half_w, row_h, "Cancel");
    style_btn(&mut btn_cancel, lbl_size);

    let mut btn_save = Button::new(pack_x + half_w + 8, bar_y, half_w, row_h, "Save && Reload");
    style_btn(&mut btn_save, lbl_size);
    btn_save.set_color(Color::from_hex(0x2e7d32));

    win.end();
    win.show();

    let mut win_cancel = win.clone();
    btn_cancel.set_callback(move |_| {
        win_cancel.hide();
    });

    let mut win_save = win.clone();
    btn_save.set_callback(move |_| {
        // Collect all values and build TOML text.
        let pairs: Vec<(&'static str, String)> = collectors.borrow().iter()
            .map(|(k, f)| (*k, f()))
            .collect();

        match build_toml_and_save(&pairs) {
            Ok(_) => {
                win_save.hide();
                // Restart application by re-exec'ing ourselves.
                restart_application();
            }
            Err(e) => {
                fltk::dialog::alert(&format!("Failed to save configuration:\n{}", e));
            }
        }
    });

    // Keyboard navigation: Tab/Shift-Tab wrap around within focusable
    // widgets so focus never escapes to the Cancel/Save buttons (which
    // would trigger an endless auto-scroll).  Enter is consumed to
    // prevent FLTK's default focus-move.  Escape closes the editor.
    // Ctrl+S triggers Save & Reload.
    let mut btn_save_k = btn_save.clone();
    let focs_handle = focusables.clone();
    let mut scroll_handle = scroll.clone();
    win.handle(move |w, ev| {
        if ev == Event::KeyDown {
            let key = app::event_key();
            if key == fltk::enums::Key::Escape {
                w.hide();
                return true;
            }
            // Ctrl+S → Save & Reload
            if key == fltk::enums::Key::from_char('s')
                && app::event_state().contains(fltk::enums::Shortcut::Ctrl)
            {
                btn_save_k.do_callback();
                return true;
            }
            if key == fltk::enums::Key::Enter {
                // Consume Enter so it does not move focus between widgets.
                return true;
            }
            // Tab / Shift-Tab: wrap around at boundaries.
            if key == fltk::enums::Key::Tab {
                let focs = focs_handle.borrow();
                if let Some(focused) = app::focus() {
                    let fptr = focused.as_widget_ptr();
                    let shift = app::event_state().contains(fltk::enums::Shortcut::Shift);
                    if !shift {
                        // Forward Tab: if on last focusable, wrap to first.
                        if let Some(last) = focs.last() {
                            if last.as_widget_ptr() == fptr {
                                if let Some(first) = focs.first() {
                                    let mut fw = first.clone();
                                    let _ = fw.take_focus();
                                    scroll_handle.scroll_to(0, 0);
                                    return true;
                                }
                            }
                        }
                    } else {
                        // Backward Tab: if on first focusable, wrap to last.
                        if let Some(first) = focs.first() {
                            if first.as_widget_ptr() == fptr {
                                if let Some(last) = focs.last() {
                                    let mut lw = last.clone();
                                    let _ = lw.take_focus();
                                    // Scroll to bottom so last widget is visible.
                                    let max_yp = (scroll_handle.child(0).map_or(0, |c| c.h()) - scroll_handle.h()).max(0);
                                    scroll_handle.scroll_to(0, max_yp);
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Forward mouse-wheel events to the scroll widget so that
        // scrolling works even when an input field has focus.
        if ev == Event::MouseWheel {
            let dy = app::event_dy();
            let step = 40;
            let yp = scroll_handle.yposition();
            let max_yp = (scroll_handle.child(0).map_or(0, |c| c.h()) - scroll_handle.h()).max(0);
            let new_yp = match dy {
                app::MouseWheel::Up   => (yp - step).max(0),
                app::MouseWheel::Down => (yp + step).min(max_yp),
                _ => yp,
            };
            if new_yp != yp {
                scroll_handle.scroll_to(0, new_yp);
            }
            return true;
        }
        false
    });

    // --- Modal event loop: blocks until the config window is closed. ---
    // Auto-scroll the Scroll widget so that the focused widget is visible,
    // but **only when focus actually changes** (e.g. Tab / arrow-key
    // navigation).  Without this guard every scrollbar-drag or
    // mouse-wheel scroll would be immediately undone by auto-scroll
    // snapping back to the (unchanged) focused widget.
    let mut scroll_loop = scroll.clone();
    let content_h = pack.h();
    let mut last_focus_addr: usize = 0;
    while win.shown() {
        app::wait();
        if let Some(focused) = app::focus() {
            // Compare raw widget pointer addresses so we detect when a
            // *different* widget receives focus (e.g. after Tab).
            let cur_addr = focused.as_widget_ptr() as usize;
            if cur_addr != last_focus_addr {
                last_focus_addr = cur_addr;
                let fy = focused.y();
                let fh = focused.h();
                let vis_top = scroll_loop.y();
                let vis_bot = vis_top + scroll_loop.h();
                let yp = scroll_loop.yposition();
                let max_yp = (content_h - scroll_loop.h()).max(0);
                if fy < vis_top && yp > 0 {
                    let delta = vis_top - fy;
                    scroll_loop.scroll_to(0, (yp - delta).max(0));
                } else if fy + fh > vis_bot && yp < max_yp {
                    let delta = (fy + fh) - vis_bot;
                    scroll_loop.scroll_to(0, (yp + delta).min(max_yp));
                }
            }
        } else {
            last_focus_addr = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Style a menu button.
fn style_btn(btn: &mut Button, lbl_size: i32) {
    btn.set_color(BTN_BG);
    btn.set_label_color(TEXT_FG);
    btn.set_frame(FrameType::FlatBox);
    btn.set_label_size(lbl_size);
    btn.set_align(Align::Inside | Align::Left);
}

/// Convert a `ColorRgb` to a `"#RRGGBB"` hex string.
fn color_to_hex(c: config::ColorRgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Parse a string that may be decimal, hex (0x...), or a bare number.
/// Returns `None` on parse failure.
fn parse_int_relaxed(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// Build TOML text from the key-value pairs collected by the editor widgets
/// and write it to `config.toml`.
fn build_toml_and_save(pairs: &[(&str, String)]) -> Result<(), String> {
    let dir = std::env::var("SMART_KBD_CONFIG_PATH").unwrap_or_else(|_| ".".into());
    let path = std::path::Path::new(&dir).join("config.toml");

    // Read the existing file so we can preserve comment structure as much as
    // possible.  We will rewrite only the keys that we manage.
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let out = build_toml_text(&existing, pairs);
    std::fs::write(&path, &out).map_err(|e| format!("{}", e))
}

/// Rewrite TOML text: for every key in `pairs`, replace its value in the
/// existing text (if it appears as an uncommented `key = value` line) or
/// append it at the end of the correct section.  Sections that do not exist
/// in the original text are appended at the end.
fn build_toml_text(existing: &str, pairs: &[(&str, String)]) -> String {
    // Build a lookup from dotted key -> new value.
    let mut updates: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (k, v) in pairs {
        updates.insert(k, v.as_str());
    }

    // Strategy: rewrite the file line by line; when we encounter a key we
    // manage, replace the value.  Keys not found in the existing file are
    // appended at the end of their existing section (not under a duplicate
    // section header).  Keys for entirely new sections are appended at EOF.
    let mut out = String::with_capacity(existing.len() + 512);
    let mut current_section = String::new();
    let mut written_keys: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Macro: flush any unwritten keys that belong to `$section` into `out`.
    macro_rules! flush_section {
        ($section:expr) => {
            for (key, val) in pairs {
                if written_keys.contains(key) { continue; }
                let sec = match key.rfind('.') {
                    Some(i) => &key[..i],
                    None => "",
                };
                if sec == $section {
                    let field = match key.rfind('.') {
                        Some(i) => &key[i+1..],
                        None => *key,
                    };
                    let formatted = format_toml_value(key, val);
                    out.push_str(&format!("{} = {}\n", field, formatted));
                    written_keys.insert(*key);
                }
            }
        };
    }

    for line in existing.lines() {
        let trimmed = line.trim();

        // Track section headers.
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            // Before switching sections, append any unwritten keys that
            // belong to the current (outgoing) section.
            flush_section!(&current_section);

            let section = trimmed.trim_start_matches('[')
                .split(']').next().unwrap_or("").trim().to_string();
            current_section = section;
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Check for key = value lines.
        if let Some(eq_pos) = trimmed.find('=') {
            let raw_key = trimmed[..eq_pos].trim();
            // Skip commented-out lines.
            if !raw_key.starts_with('#') {
                let dotted = if current_section.is_empty() {
                    raw_key.to_string()
                } else {
                    format!("{}.{}", current_section, raw_key)
                };

                if let Some(new_val) = updates.get(dotted.as_str()) {
                    written_keys.insert(updates.keys().find(|k| **k == dotted.as_str()).copied().unwrap_or(""));
                    // Write updated line.
                    let formatted = format_toml_value(&dotted, new_val);
                    out.push_str(&format!("{} = {}\n", raw_key, formatted));
                    continue;
                }
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    // Flush unwritten keys for the last section in the file.
    flush_section!(&current_section);

    // Append keys for sections that never appeared in the file.
    let mut pending_sections: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (key, val) in pairs {
        if written_keys.contains(key) { continue; }
        let (section, field) = match key.rfind('.') {
            Some(i) => (&key[..i], &key[i+1..]),
            None => ("", *key),
        };
        let formatted = format_toml_value(key, val);
        let line = format!("{} = {}", field, formatted);
        pending_sections.entry(section.to_string())
            .or_default()
            .push(line);
    }
    for (section, lines) in &pending_sections {
        if !section.is_empty() {
            out.push_str(&format!("\n[{}]\n", section));
        }
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

/// Format a value for TOML output based on the key name.
fn format_toml_value(key: &str, val: &str) -> String {
    // Boolean fields.
    if val == "true" || val == "false" {
        return val.to_string();
    }

    // Hex key-code fields (input.keyboard.*).
    if key.starts_with("input.keyboard.") && val.starts_with("0x") {
        return val.to_string();
    }

    // BLE VID/PID.
    if (key == "output.ble.vid" || key == "output.ble.pid") && val.starts_with("0x") {
        return val.to_string();
    }

    // Numeric fields -- try parsing as number.
    if let Some(_n) = parse_int_relaxed(val) {
        return val.to_string();
    }

    // Active keymaps: convert comma-separated back to TOML array.
    if key == "ui.active_keymaps" {
        let items: Vec<String> = val.split(',')
            .map(|s| format!("\"{}\"", s.trim()))
            .collect();
        return format!("[{}]", items.join(", "));
    }

    // Empty strings -> omit (null equivalent).
    if val.is_empty() {
        return "\"\"".to_string();
    }

    // Default: quoted string.
    format!("\"{}\"", val)
}

/// Restart the application by re-exec'ing the current binary.
fn restart_application() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[menu] cannot determine executable path; quitting instead");
            app::quit();
            return;
        }
    };
    let args: Vec<String> = std::env::args().collect();
    eprintln!("[menu] restarting: {:?} {:?}", exe, &args[1..]);
    // Use exec to replace the current process.
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&exe)
        .args(&args[1..])
        .exec();
    // exec() only returns on error.
    eprintln!("[menu] exec failed: {}", err);
    app::quit();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Saving settings for a section that has commented-out keys must NOT
    /// produce a duplicate section header.
    #[test]
    fn no_duplicate_sections() {
        let existing = "\
[input.keyboard]
navigate_up = 0xff52

[input.gamepad]
enabled = true
device = \"auto\"
# axis_threshold = 16384
# rumble = false

[output]
mode = \"print\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.keyboard.navigate_up",       "0xff52".into()),
            ("input.gamepad.enabled",            "true".into()),
            ("input.gamepad.device",             "auto".into()),
            ("input.gamepad.axis_threshold",     "16384".into()),
            ("input.gamepad.rumble",             "false".into()),
            ("output.mode",                      "print".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        // Count occurrences of [input.gamepad] section header.
        let count = result.lines()
            .filter(|l| l.trim() == "[input.gamepad]")
            .count();
        assert_eq!(count, 1, "Expected exactly one [input.gamepad] section, got {}.\nOutput:\n{}", count, result);

        // The new keys must appear in the output.
        assert!(result.contains("axis_threshold = 16384"), "axis_threshold missing:\n{}", result);
        assert!(result.contains("rumble = false"), "rumble missing:\n{}", result);
    }

    /// Keys for a brand-new section (not in existing file) are appended.
    #[test]
    fn new_section_appended() {
        let existing = "\
[output]
mode = \"print\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("output.mode",           "ble".into()),
            ("navigate.rollover",     "true".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("[navigate]"), "Missing [navigate] section:\n{}", result);
        assert!(result.contains("rollover = true"), "Missing rollover:\n{}", result);

        // output.mode should be updated in place.
        assert!(result.contains("mode = \"ble\""), "mode not updated:\n{}", result);
    }

    /// Existing uncommented keys are updated in place.
    #[test]
    fn existing_keys_updated() {
        let existing = "\
[input.gamepad]
enabled = true
device = \"auto\"
";
        let pairs: Vec<(&str, String)> = vec![
            ("input.gamepad.enabled", "false".into()),
            ("input.gamepad.device",  "xbox".into()),
        ];
        let result = build_toml_text(existing, &pairs);

        assert!(result.contains("enabled = false"), "enabled not updated:\n{}", result);
        assert!(result.contains("device = \"xbox\""), "device not updated:\n{}", result);

        let count = result.lines()
            .filter(|l| l.trim() == "[input.gamepad]")
            .count();
        assert_eq!(count, 1, "Duplicate section headers:\n{}", result);
    }
}
