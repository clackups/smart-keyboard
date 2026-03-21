mod config;
mod display;
mod gamepad;
mod gpio;
mod keyboards;
mod menu;
mod narrator;
mod output;
mod phys_keyboard;
mod user_input;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Length, Subscription, Task};
use iced::keyboard;
use iced::event;

use keyboards::{Action, REGULAR_KEY_COUNT};
use gamepad::Gamepad;
use gpio::GpioInput;
use narrator::Narrator;
use user_input::{UserInputAction, UserInputEvent};
use display::{
    NavSel, BtnData, LangBtnData,
    nav_set, nav_move, find_center_key, find_btn_by_action,
    execute_action,
    on_nav_changed,
    action_audio_slug, action_tone_hz,
    Colors, ModBtn, ModState, LayoutMetrics,
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
// Per-input-source mouse-movement auto-repeat state
// =============================================================================

/// Per-input-source mouse-movement auto-repeat state.
///
/// Each physical input source (gamepad, GPIO, ...) keeps its own independent
/// mouse-movement state so that simultaneous use of two sources does not
/// interfere.
pub(crate) struct MouseMoveState {
    pub(crate) dx:    i8,
    pub(crate) dy:    i8,
    pub(crate) start: Option<Instant>,
    pub(crate) next:  Option<Instant>,
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

// =============================================================================
// BLE connection state
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum BleState {
    Disconnected,
    Connecting,
    Connected,
}

// =============================================================================
// Message enum
// =============================================================================

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    KeyPressed(u32),
    KeyReleased(u32),
    PollTick,
    BleTick,
    ButtonClicked(usize, usize),
    LangClicked(usize),
    MenuOpen,
    MenuClose,
}

// =============================================================================
// SmartKeyboard state
// =============================================================================

struct SmartKeyboard {
    // Button grid data
    all_btns: Vec<Vec<BtnData>>,
    lang_btns: Vec<LangBtnData>,
    layout_idx: usize,
    mod_state: ModState,
    #[allow(dead_code)]
    mod_btns: Vec<ModBtn>,
    sel: NavSel,
    text_buffer: String,
    preferred_cx: i32,

    // Output
    hook: Box<dyn KeyHook>,

    // Active key tracking
    active_nav_key: Option<(u16, String)>,
    active_btn_pressed: Option<(usize, usize)>,

    // Gamepad
    gp_cell: Option<Gamepad>,

    // Audio
    narrator: Narrator,
    audio_mode: config::AudioMode,

    // Config
    nav_keys: config::NavKeys,
    rollover: bool,
    center_key: String,
    center_after_activate: bool,
    show_text_display: bool,
    mouse_cfg: config::MouseConfig,

    // Mouse mode
    mouse_mode: bool,
    mouse_buttons: u8,
    mouse_state_kb: MouseMoveState,
    mouse_state_gp: MouseMoveState,
    mouse_state_gpio: MouseMoveState,

    // Input sources
    gp_cfg: Option<config::GamepadInputConfig>,
    gpio_cell: Option<GpioInput>,
    gpio_cfg: Option<config::GpioInputConfig>,

    // BLE – shared with BleKeyHook so both use the same serial connection
    ble_conn: Option<Rc<RefCell<output::BleConnection>>>,
    ble_state: BleState,

    // Keyboard physical key tracking
    pressed_keys: HashSet<u32>,

    // UI
    colors: Colors,
    metrics: LayoutMetrics,
    switch_scancodes: Vec<Vec<u8>>,

    // Gamepad / GPIO connection tracking
    gp_connected: bool,
    gpio_connected: bool,
    gp_rumble: bool,

    // Menu
    showing_menu: bool,
}

// SAFETY: All iced update/view/subscription callbacks run on the main GUI
// thread.  The non-Send fields (BleKeyHook via Rc<RefCell<BleConnection>>)
// are never accessed from another thread.
unsafe impl Send for SmartKeyboard {}

// =============================================================================
// Application implementation
// =============================================================================

impl SmartKeyboard {
    fn new() -> (Self, Task<Message>) {
        let cfg = config::Config::load();

        let config_dir = std::env::var("SMART_KBD_CONFIG_PATH")
            .unwrap_or_else(|_| ".".into());

        let active_keymaps = cfg.ui.active_keymaps.clone();
        let loaded_layouts = keyboards::load_active_layouts(&active_keymaps, &config_dir);
        keyboards::set_layouts(loaded_layouts);
        let layouts = keyboards::get_layouts();

        debug_assert!(
            layouts.iter().all(|l| l.keys.len() == REGULAR_KEY_COUNT),
            "every LayoutDef must have exactly REGULAR_KEY_COUNT entries"
        );

        let switch_scancodes: Vec<Vec<u8>> = active_keymaps
            .iter()
            .map(|name| match cfg.keymap.get(name) {
                None     => keyboards::default_switch_scancode_for(name),
                Some(kc) => kc.switch_scancode.clone(),
            })
            .collect();

        let narrator = Narrator::new(cfg.output.audio.clone());
        let audio_mode = cfg.output.audio.clone();

        let mut ble_conn_opt: Option<Rc<RefCell<output::BleConnection>>> = None;
        let hook: Box<dyn KeyHook> = match cfg.output.mode {
            config::OutputMode::Print => Box::new(output::PrintKeyHook),
            config::OutputMode::Ble => {
                let ble_cfg = &cfg.output.ble;
                let (ble_hook, ble_conn) = output::BleKeyHook::new(
                    ble_cfg.vid,
                    ble_cfg.pid,
                    ble_cfg.serial.clone(),
                    ble_cfg.key_release_delay,
                    ble_cfg.lang_switch_release_delay,
                );
                // Keep the shared Rc so manage_ble_connection() operates on
                // the same BleConnection instance the hook uses for output.
                ble_conn_opt = Some(ble_conn);
                Box::new(ble_hook)
            }
        };

        let colors = Colors::from_config(&cfg.ui.colors);

        // Use a reasonable default screen size for layout computation.
        // iced will handle the actual window sizing.
        let (sw, sh) = (800i32, 480i32);
        let metrics = display::compute_layout(sw, sh, &cfg);

        let all_btns = display::build_btn_grid(&metrics, &colors);
        let lang_btns = display::build_lang_btns(&metrics);

        let mod_btns = build_mod_btns(&all_btns, &colors);

        // Set initial selection to center key or first key.
        let initial_sel = find_center_key(&all_btns, 0, &cfg.navigate.center_key)
            .unwrap_or(NavSel::Key(0, 0));

        let nav_keys = config::NavKeys::from_config(&cfg.input.keyboard);

        // Open gamepad if enabled.
        let (gp_cell, gp_connected, gp_cfg, gp_rumble) = if cfg.input.gamepad.enabled {
            let gp = Gamepad::open(&cfg.input.gamepad);
            let connected = gp.is_some();
            (gp, connected, Some(cfg.input.gamepad.clone()), cfg.input.gamepad.rumble)
        } else {
            (None, false, None, false)
        };

        // Open GPIO if enabled.
        let (gpio_cell, gpio_connected, gpio_cfg) = if cfg.input.gpio.enabled {
            let gpio = GpioInput::open(&cfg.input.gpio);
            let connected = gpio.is_some();
            (gpio, connected, Some(cfg.input.gpio.clone()))
        } else {
            (None, false, None)
        };

        let ble_state = if ble_conn_opt.is_some() {
            BleState::Disconnected
        } else {
            BleState::Disconnected
        };

        let app = SmartKeyboard {
            all_btns,
            lang_btns,
            layout_idx: 0,
            mod_state: ModState::default(),
            mod_btns,
            sel: initial_sel,
            text_buffer: String::new(),
            preferred_cx: 0,
            hook,
            active_nav_key: None,
            active_btn_pressed: None,
            gp_cell,
            narrator,
            audio_mode,
            nav_keys,
            rollover: cfg.navigate.rollover,
            center_key: cfg.navigate.center_key.clone(),
            center_after_activate: cfg.navigate.center_after_activate,
            show_text_display: cfg.ui.show_text_display,
            mouse_cfg: cfg.mouse.clone(),
            mouse_mode: false,
            mouse_buttons: 0,
            mouse_state_kb: MouseMoveState::new(),
            mouse_state_gp: MouseMoveState::new(),
            mouse_state_gpio: MouseMoveState::new(),
            gp_cfg,
            gpio_cell,
            gpio_cfg,
            ble_conn: ble_conn_opt,
            ble_state,
            pressed_keys: HashSet::new(),
            colors,
            metrics,
            switch_scancodes,
            gp_connected,
            gpio_connected,
            gp_rumble,
            showing_menu: false,
        };

        (app, Task::none())
    }

    // =========================================================================
    // Subscription
    // =========================================================================

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            // Keyboard events
            event::listen_with(|evt, _status, _id| match evt {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    physical_key,
                    ..
                }) => phys_keyboard::physical_to_scancode(physical_key)
                    .map(Message::KeyPressed),
                iced::Event::Keyboard(keyboard::Event::KeyReleased {
                    physical_key,
                    ..
                }) => phys_keyboard::physical_to_scancode(physical_key)
                    .map(Message::KeyReleased),
                _ => None,
            }),
            // 16ms poll tick for gamepad/GPIO/mouse auto-repeat
            iced::time::every(Duration::from_millis(16)).map(|_| Message::PollTick),
        ];

        // BLE management tick (1s)
        if self.ble_conn.is_some() {
            subs.push(
                iced::time::every(Duration::from_secs(1)).map(|_| Message::BleTick),
            );
        }

        Subscription::batch(subs)
    }

    // =========================================================================
    // Update
    // =========================================================================

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::KeyPressed(sc) => {
                if !self.pressed_keys.insert(sc) {
                    // Already pressed (key repeat); ignore.
                    return Task::none();
                }
                let events =
                    phys_keyboard::translate_key_event(sc, true, &self.nav_keys);
                self.process_input_events(&events, InputSource::Keyboard);
            }

            Message::KeyReleased(sc) => {
                self.pressed_keys.remove(&sc);
                let events =
                    phys_keyboard::translate_key_event(sc, false, &self.nav_keys);
                self.process_input_events(&events, InputSource::Keyboard);
            }

            Message::PollTick => {
                // Poll gamepad
                if self.gp_cfg.is_some() {
                    self.poll_gamepad();
                }
                // Poll GPIO
                if self.gpio_cfg.is_some() {
                    self.poll_gpio();
                }
                // Mouse auto-repeat for all sources
                self.do_mouse_auto_repeat(InputSource::Keyboard);
                self.do_mouse_auto_repeat(InputSource::Gamepad);
                self.do_mouse_auto_repeat(InputSource::Gpio);
            }

            Message::BleTick => {
                self.manage_ble_connection();
            }

            Message::ButtonClicked(row, col) => {
                self.handle_button_click(row, col);
            }

            Message::LangClicked(li) => {
                self.handle_lang_click(li);
            }

            Message::MenuOpen => {
                // Release any held key before entering menu.
                if let Some((sc, ks)) = self.active_nav_key.take() {
                    self.hook.on_key_release(sc, &ks);
                }
                self.showing_menu = true;
            }

            Message::MenuClose => {
                self.showing_menu = false;
            }
        }

        Task::none()
    }

    // =========================================================================
    // View
    // =========================================================================

    fn view(&self) -> Element<'_, Message> {
        if self.showing_menu {
            return self.view_menu();
        }

        let colors = self.colors;
        let metrics = &self.metrics;

        // --- Status bar ---
        let status_bar = self.view_status_bar();

        // --- Text display (optional) ---
        let text_display: Element<Message> = if self.show_text_display {
            let display_text = text(&self.text_buffer)
                .size(metrics.disp_size as f32)
                .color(colors.disp_text);
            container(display_text)
                .width(Length::Fill)
                .height(Length::Fixed(metrics.display_h as f32))
                .style(move |_theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(colors.disp_bg)),
                    ..Default::default()
                })
                .padding(4)
                .into()
        } else {
            Space::new().into()
        };

        // --- Language buttons ---
        let lang_row: Element<Message> = if self.lang_btns.is_empty() {
            Space::new().into()
        } else {
            let mut lr = row![].spacing(metrics.gap as f32);
            for (li, lang) in self.lang_btns.iter().enumerate() {
                let is_active = li == self.layout_idx;
                let is_selected = self.sel == NavSel::Lang(li);

                let bg = if is_selected {
                    colors.nav_sel
                } else if is_active {
                    colors.mod_active
                } else {
                    colors.lang_btn_inactive
                };

                let label_color = colors.lang_btn_label;
                let label = lang.name.clone();
                let h = metrics.lang_btn_h as f32;
                let lbl_size = metrics.lbl_size as f32;

                let btn = button(
                    text(label).size(lbl_size).color(label_color)
                        .align_x(iced::alignment::Horizontal::Center)
                )
                .width(Length::Fill)
                .height(Length::Fixed(h))
                .on_press(Message::LangClicked(li))
                .style(move |_theme: &iced::Theme, _status| button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: label_color,
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: true,
                });

                lr = lr.push(btn);
            }
            container(lr)
                .width(Length::Fill)
                .padding([0, metrics.pad as u16])
                .into()
        };

        // --- Keyboard grid ---
        let keyboard_grid = self.view_keyboard_grid();

        // --- Assemble main layout ---
        let main_col = column![status_bar, text_display, lang_row, keyboard_grid]
            .spacing(metrics.gap as f32)
            .width(Length::Fill)
            .height(Length::Fill);

        container(main_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.win_bg)),
                ..Default::default()
            })
            .into()
    }

    // =========================================================================
    // View helpers
    // =========================================================================

    fn view_status_bar(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let metrics = &self.metrics;
        let lbl_size = metrics.status_lbl_size as f32;
        let ind_w = metrics.ind_w as f32;
        let ind_h = metrics.ind_h as f32;

        let mut pills = row![].spacing(metrics.ind_gap as f32);

        // Modifier indicator pills
        let mod_indicators = [
            ("Caps", self.mod_state.caps),
            ("Shift", self.mod_state.lshift || self.mod_state.rshift),
            ("Ctrl", self.mod_state.ctrl),
            ("Alt", self.mod_state.alt),
            ("AltGr", self.mod_state.altgr),
            ("Win", self.mod_state.win),
            ("Mouse", self.mouse_mode),
        ];

        for (label, active) in mod_indicators {
            let text_color = if active {
                colors.status_ind_active_text
            } else {
                colors.status_ind_text
            };

            let pill = container(
                text(label).size(lbl_size).color(text_color)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
            )
            .width(Length::Fixed(ind_w))
            .height(Length::Fixed(ind_h))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.status_ind_bg)),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

            pills = pills.push(pill);
        }

        // Connection status icons
        pills = pills.push(Space::new().width(Length::Fill));

        // Gamepad connection icon
        if self.gp_cfg.is_some() {
            let gp_color = if self.gp_connected {
                colors.conn_connected
            } else {
                colors.conn_disconnected
            };
            let gp_icon = container(
                text("GP").size(lbl_size).color(gp_color)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
            )
            .width(Length::Fixed(ind_w))
            .height(Length::Fixed(ind_h))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.status_ind_bg)),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            pills = pills.push(gp_icon);
        }

        // GPIO connection icon
        if self.gpio_cfg.is_some() {
            let gpio_color = if self.gpio_connected {
                colors.conn_connected
            } else {
                colors.conn_disconnected
            };
            let gpio_icon = container(
                text("IO").size(lbl_size).color(gpio_color)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
            )
            .width(Length::Fixed(ind_w))
            .height(Length::Fixed(ind_h))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.status_ind_bg)),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            pills = pills.push(gpio_icon);
        }

        // BLE connection icon
        if self.ble_conn.is_some() {
            let ble_color = match self.ble_state {
                BleState::Connected    => colors.conn_connected,
                BleState::Connecting   => colors.conn_connecting,
                BleState::Disconnected => colors.conn_disconnected,
            };
            let ble_icon = container(
                text("BLE").size(lbl_size).color(ble_color)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
            )
            .width(Length::Fixed(ind_w))
            .height(Length::Fixed(ind_h))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.status_ind_bg)),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            pills = pills.push(ble_icon);
        }

        container(pills.padding(metrics.ind_pad as u16))
            .width(Length::Fill)
            .height(Length::Fixed(metrics.status_h as f32))
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.status_bar_bg)),
                ..Default::default()
            })
            .into()
    }

    fn view_keyboard_grid(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let metrics = &self.metrics;
        let lbl_size = metrics.lbl_size as f32;
        let big_lbl_size = metrics.big_lbl_size as f32;
        let layouts = keyboards::get_layouts();

        let mut grid = column![].spacing(metrics.gap as f32);

        for (ri, btn_row) in self.all_btns.iter().enumerate() {
            let mut r = row![].spacing(metrics.gap as f32);
            let mut total_cols: u16 = 0;

            for (ci, btn_data) in btn_row.iter().enumerate() {
                let action = btn_data.action;
                let is_selected = self.sel == NavSel::Key(ri, ci);
                let is_active_pressed =
                    self.active_btn_pressed == Some((ri, ci));

                // Insert invisible spacer(s) to maintain the visual gap where
                // Spacer slots exist in the physical layout (e.g. arrow cluster
                // separator).
                for _ in 0..btn_data.spacers_before {
                    r = r.push(Space::new().width(Length::FillPortion(1)));
                    total_cols += 1;
                }

                // Ortholinear: all standard keys get portion 1; space bar
                // spans SPACE_COLS columns so columns align across rows.
                let portion = if action == Action::Space {
                    keyboards::SPACE_COLS
                } else {
                    1u16
                };
                total_cols += portion;

                // Determine background color
                let bg = if is_selected || is_active_pressed {
                    colors.nav_sel
                } else if keyboards::is_modifier(action) && self.mod_state.is_active(action) {
                    colors.mod_active
                } else {
                    btn_data.base_color
                };

                let label_color = match action {
                    Action::Regular(_) | Action::Space => colors.key_label_normal,
                    _ => colors.key_label_mod,
                };

                // Build button content.
                //
                // Two font sizes for efficient surface use:
                //   big_lbl_size — single-row labels (letters, modifiers,
                //                  function keys, arrows)
                //   lbl_size     — two-row labels (number / punctuation keys
                //                  with a shifted variant)
                let btn_content: Element<'_, Message> = match action {
                    Action::Regular(n) if self.layout_idx < layouts.len() => {
                        let key = &layouts[self.layout_idx].keys[n];
                        if !key.label_shifted.is_empty() {
                            // Number / punctuation key: two-line label (smaller)
                            let top = text(key.label_shifted.clone())
                                .size(lbl_size)
                                .color(label_color)
                                .align_x(iced::alignment::Horizontal::Center);
                            let bottom = text(key.label_unshifted.clone())
                                .size(lbl_size)
                                .color(label_color)
                                .align_x(iced::alignment::Horizontal::Center);
                            column![top, bottom]
                                .align_x(iced::alignment::Horizontal::Center)
                                .width(Length::Fill)
                                .into()
                        } else {
                            // Letter key: single line, big label
                            let lbl = if self.mod_state.caps {
                                key.label_unshifted.to_uppercase()
                            } else {
                                key.label_unshifted.clone()
                            };
                            text(lbl)
                                .size(big_lbl_size)
                                .color(label_color)
                                .align_x(iced::alignment::Horizontal::Center)
                                .width(Length::Fill)
                                .into()
                        }
                    }
                    Action::Space => {
                        Space::new().into()
                    }
                    other => {
                        // Modifier / function / arrow key: single line, big label
                        let lbl = keyboards::special_label(other).to_string();
                        text(lbl)
                            .size(big_lbl_size)
                            .color(label_color)
                            .align_x(iced::alignment::Horizontal::Center)
                            .width(Length::Fill)
                            .into()
                    }
                };

                let btn = button(btn_content)
                .width(Length::FillPortion(portion))
                .height(Length::Fill)
                .on_press(Message::ButtonClicked(ri, ci))
                .style(move |_theme: &iced::Theme, _status| button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: label_color,
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    shadow: iced::Shadow::default(),
                    snap: true,
                });

                r = r.push(btn);
            }

            // Pad row with trailing spacers to reach GRID_COLS so every
            // row occupies the same total column count (ortholinear grid).
            for _ in 0..keyboards::GRID_COLS.saturating_sub(total_cols) {
                r = r.push(Space::new().width(Length::FillPortion(1)));
            }

            grid = grid.push(r);
        }

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([0, metrics.pad as u16])
            .into()
    }

    fn view_menu(&self) -> Element<'_, Message> {
        let colors = self.colors;

        let title = text("Menu")
            .size(28)
            .color(Color::WHITE);

        let close_btn = button(
            text("Close")
                .size(20)
                .color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center)
        )
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(50.0))
        .on_press(Message::MenuClose)
        .style(|_theme: &iced::Theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.24, 0.24, 0.26))),
            text_color: Color::WHITE,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow::default(),
                    snap: true,
        });

        let ble_label = if self.ble_conn.is_some() {
            "Disconnect BLE"
        } else {
            "Disconnect BLE (N/A)"
        };

        let ble_btn = button(
            text(ble_label)
                .size(20)
                .color(if self.ble_conn.is_some() {
                    Color::WHITE
                } else {
                    Color::from_rgb(0.35, 0.35, 0.35)
                })
                .align_x(iced::alignment::Horizontal::Center)
        )
        .width(Length::Fixed(300.0))
        .height(Length::Fixed(50.0))
        .on_press(Message::MenuClose)
        .style(|_theme: &iced::Theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.24, 0.24, 0.26))),
            text_color: Color::WHITE,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow::default(),
                    snap: true,
        });

        let menu_col = column![title, ble_btn, close_btn]
            .spacing(12)
            .align_x(iced::alignment::Horizontal::Center);

        container(menu_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(colors.win_bg)),
                ..Default::default()
            })
            .into()
    }

    // =========================================================================
    // Input event processing (replaces old process_input_events + InputCtx)
    // =========================================================================

    fn process_input_events(&mut self, events: &[UserInputEvent], source: InputSource) {
        let rumble = matches!(source, InputSource::Gamepad) && self.gp_rumble;

        for evt in events {
            match evt.action {
                UserInputAction::Menu => {
                    if !evt.pressed { continue; }
                    if let Some((sc, ks)) = self.active_nav_key.take() {
                        self.hook.on_key_release(sc, &ks);
                    }
                    self.showing_menu = true;
                }

                UserInputAction::Up
                | UserInputAction::Down
                | UserInputAction::Left
                | UserInputAction::Right => {
                    if !evt.pressed {
                        if self.mouse_mode {
                            let (ddx, ddy) = dir_to_mouse_delta(evt.action);
                            let mouse = self.mouse_state_for(source);
                            if ddx != 0 && mouse.dx == ddx { mouse.dx = 0; }
                            if ddy != 0 && mouse.dy == ddy { mouse.dy = 0; }
                            if mouse.dx == 0 && mouse.dy == 0 {
                                mouse.start = None;
                                mouse.next  = None;
                            }
                        }
                        continue;
                    }
                    if self.mouse_mode {
                        let (ddx, ddy) = dir_to_mouse_delta(evt.action);
                        let interval = Duration::from_millis(self.mouse_cfg.repeat_interval);
                        self.sync_mouse_buttons();
                        let mouse_buttons = self.mouse_buttons;
                        let mouse = self.mouse_state_for(source);
                        if ddx != 0 { mouse.dx = ddx; }
                        if ddy != 0 { mouse.dy = ddy; }
                        let now = Instant::now();
                        if mouse.start.is_none() {
                            mouse.start = Some(now);
                        }
                        mouse.next = Some(now + interval);
                        self.hook.on_mouse_report(mouse_buttons, ddx, ddy);
                        continue;
                    }
                    let (dr, dc) = match evt.action {
                        UserInputAction::Up    => (-1,  0),
                        UserInputAction::Down  => ( 1,  0),
                        UserInputAction::Left  => ( 0, -1),
                        _                      => ( 0,  1),
                    };
                    let changed = nav_move(
                        &self.all_btns, &self.lang_btns,
                        &mut self.sel,
                        dr, dc,
                        self.rollover,
                        &mut self.preferred_cx,
                    );
                    on_nav_changed(
                        changed, rumble, &mut self.gp_cell,
                        self.sel, &self.all_btns, self.layout_idx,
                        &mut self.narrator, &self.audio_mode,
                        self.mod_state.is_shifted(),
                    );
                }

                UserInputAction::Activate => {
                    if self.mouse_mode {
                        let btns = self.update_mouse_buttons(0x01, evt.pressed);
                        self.hook.on_mouse_report(btns, 0, 0);
                        continue;
                    }
                    if evt.pressed {
                        self.do_activate(rumble);
                    } else {
                        if let Some((sc, ks)) = self.active_nav_key.take() {
                            self.hook.on_key_release(sc, &ks);
                        }
                    }
                }

                UserInputAction::ActivateEnter => {
                    if self.mouse_mode { continue; }
                    self.activate_direct_key(evt.pressed, Action::Enter, 0x1c, rumble);
                }

                UserInputAction::ActivateSpace => {
                    if self.mouse_mode { continue; }
                    self.activate_direct_key(evt.pressed, Action::Space, 0x39, rumble);
                }

                UserInputAction::ActivateArrowLeft
                | UserInputAction::ActivateArrowRight
                | UserInputAction::ActivateArrowUp
                | UserInputAction::ActivateArrowDown => {
                    if self.mouse_mode { continue; }
                    let (arrow_action, arrow_sc) = match evt.action {
                        UserInputAction::ActivateArrowLeft  => (Action::ArrowLeft,  0x69u16),
                        UserInputAction::ActivateArrowRight => (Action::ArrowRight, 0x6au16),
                        UserInputAction::ActivateArrowUp    => (Action::ArrowUp,    0x67u16),
                        _                                   => (Action::ArrowDown,  0x6cu16),
                    };
                    self.activate_direct_key(evt.pressed, arrow_action, arrow_sc, rumble);
                }

                UserInputAction::ActivateBksp => {
                    if self.mouse_mode { continue; }
                    self.activate_direct_key(evt.pressed, Action::Backspace, 0x0e, rumble);
                }

                UserInputAction::ActivateShift
                | UserInputAction::ActivateCtrl
                | UserInputAction::ActivateAlt
                | UserInputAction::ActivateAltGr => {
                    if self.mouse_mode {
                        if evt.action == UserInputAction::ActivateShift {
                            let btns = self.update_mouse_buttons(0x02, evt.pressed);
                            self.hook.on_mouse_report(btns, 0, 0);
                        }
                        continue;
                    }
                    if evt.pressed {
                        match evt.action {
                            UserInputAction::ActivateShift => self.mod_state.lshift = true,
                            UserInputAction::ActivateCtrl  => self.mod_state.ctrl   = true,
                            UserInputAction::ActivateAlt   => self.mod_state.alt    = true,
                            _                              => self.mod_state.altgr  = true,
                        }
                        self.do_activate(rumble);
                    } else {
                        if let Some((sc, ks)) = self.active_nav_key.take() {
                            self.hook.on_key_release(sc, &ks);
                        }
                    }
                }

                UserInputAction::NavigateCenter => {
                    if !evt.pressed { continue; }
                    if let Some(center) = find_center_key(
                        &self.all_btns, self.layout_idx, &self.center_key,
                    ) {
                        let changed = nav_set(&mut self.sel, center);
                        on_nav_changed(
                            changed, rumble, &mut self.gp_cell,
                            self.sel, &self.all_btns, self.layout_idx,
                            &mut self.narrator, &self.audio_mode,
                            self.mod_state.is_shifted(),
                        );
                    }
                }

                UserInputAction::MouseToggle => {
                    if !evt.pressed { continue; }
                    self.mouse_mode = !self.mouse_mode;
                    if !self.mouse_mode {
                        self.mouse_state_for(source).stop();
                        self.mouse_buttons = 0;
                    }
                }

                UserInputAction::AbsolutePos { horiz, vert } => {
                    let new_sel = self.compute_absolute_sel(horiz, vert);
                    #[cfg(debug_assertions)]
                    if new_sel != self.sel {
                        match new_sel {
                            NavSel::Lang(li) =>
                                eprintln!(
                                    "[gamepad] abs_pos horiz={:.3} vert={:.3} -> lang={}",
                                    horiz, vert, li
                                ),
                            NavSel::Key(r, c) =>
                                eprintln!(
                                    "[gamepad] abs_pos horiz={:.3} vert={:.3} -> row={} col={}",
                                    horiz, vert, r, c
                                ),
                        }
                    }
                    let changed = nav_set(&mut self.sel, new_sel);
                    on_nav_changed(
                        changed, rumble, &mut self.gp_cell,
                        self.sel, &self.all_btns, self.layout_idx,
                        &mut self.narrator, &self.audio_mode,
                        self.mod_state.is_shifted(),
                    );
                }
            }
        }
    }

    // =========================================================================
    // Activation helpers
    // =========================================================================

    /// Execute the action of the currently selected button.
    fn do_activate(&mut self, rumble: bool) {
        match self.sel {
            NavSel::Lang(li) => {
                self.handle_lang_click(li);
                self.active_nav_key = None;
                self.maybe_center_after_activate(rumble);
            }
            NavSel::Key(row, col) => {
                let shifted_pre = self.mod_state.is_shifted();
                let action = self.all_btns[row][col].action;
                let scancode = self.all_btns[row][col].scancode;

                let result = execute_action(
                    action, scancode, self.layout_idx,
                    &mut self.text_buffer, &*self.hook,
                    &mut self.mod_state,
                    self.show_text_display,
                );
                self.active_nav_key = Some((scancode, result.key_str.clone()));

                let jumped = self.maybe_center_after_activate(rumble);
                if !jumped {
                    let slug = action_audio_slug(
                        action, self.layout_idx, shifted_pre,
                    );
                    let fallback = if shifted_pre {
                        action_audio_slug(action, self.layout_idx, false)
                    } else {
                        String::new()
                    };
                    self.narrator.play_with_fallback(
                        &slug, &fallback,
                        action_tone_hz(action, &self.audio_mode),
                    );
                }
            }
        }
    }

    /// Activate a "direct" key (Enter, Space, arrows, Backspace).
    fn activate_direct_key(
        &mut self,
        pressed: bool,
        action:  Action,
        scancode: u16,
        rumble:  bool,
    ) {
        if pressed {
            let shifted_pre = self.mod_state.is_shifted();
            let btn_pos = find_btn_by_action(&self.all_btns, action);
            self.active_btn_pressed = btn_pos;

            let result = execute_action(
                action, scancode, self.layout_idx,
                &mut self.text_buffer, &*self.hook,
                &mut self.mod_state,
                self.show_text_display,
            );
            self.active_nav_key = Some((scancode, result.key_str.clone()));

            let jumped = self.maybe_center_after_activate(rumble);
            if !jumped {
                let slug = action_audio_slug(action, self.layout_idx, shifted_pre);
                let fallback = if shifted_pre {
                    action_audio_slug(action, self.layout_idx, false)
                } else {
                    String::new()
                };
                self.narrator.play_with_fallback(
                    &slug, &fallback,
                    action_tone_hz(action, &self.audio_mode),
                );
            }
        } else {
            self.active_btn_pressed = None;
            if let Some((sc, ks)) = self.active_nav_key.take() {
                self.hook.on_key_release(sc, &ks);
            }
        }
    }

    /// If `center_after_activate` is configured, move the navigation cursor to
    /// the center key after an activation.  Returns `true` when the jump
    /// occurred and was narrated.
    fn maybe_center_after_activate(&mut self, rumble: bool) -> bool {
        if !self.center_after_activate { return false; }
        if let Some(center) = find_center_key(
            &self.all_btns, self.layout_idx, &self.center_key,
        ) {
            let changed = nav_set(&mut self.sel, center);
            on_nav_changed(
                changed, rumble, &mut self.gp_cell,
                self.sel, &self.all_btns, self.layout_idx,
                &mut self.narrator, &self.audio_mode,
                self.mod_state.is_shifted(),
            );
            changed
        } else {
            false
        }
    }

    /// Handle a button click from the GUI.
    fn handle_button_click(&mut self, row: usize, col: usize) {
        // In mouse mode, button activations are handled via the Activate
        // action in process_input_events; ignore widget-level clicks so we
        // don't send spurious keyboard HID reports.
        if self.mouse_mode { return; }

        // Move selection to the clicked button.
        let _ = nav_set(&mut self.sel, NavSel::Key(row, col));

        let shifted_pre = self.mod_state.is_shifted();
        let action = self.all_btns[row][col].action;
        let scancode = self.all_btns[row][col].scancode;

        let result = execute_action(
            action, scancode, self.layout_idx,
            &mut self.text_buffer, &*self.hook,
            &mut self.mod_state,
            self.show_text_display,
        );

        // Immediately release non-modifier keys.
        if !keyboards::is_modifier(action) {
            self.hook.on_key_release(scancode, &result.key_str);
        }

        let slug = action_audio_slug(action, self.layout_idx, shifted_pre);
        let fallback = if shifted_pre {
            action_audio_slug(action, self.layout_idx, false)
        } else {
            String::new()
        };
        self.narrator.play_with_fallback(
            &slug, &fallback,
            action_tone_hz(action, &self.audio_mode),
        );
    }

    /// Handle a language button click.
    fn handle_lang_click(&mut self, li: usize) {
        if self.mouse_mode { return; }

        let layouts = keyboards::get_layouts();
        if li >= layouts.len() { return; }

        // Send the switch scancode to the hook.
        if li < self.switch_scancodes.len() {
            self.hook.on_lang_switch(&self.switch_scancodes[li]);
        }

        self.layout_idx = li;

        // Narrate the language switch.
        let slug = format!("lang_{}", layouts[li].name.to_lowercase());
        self.narrator.play_with_fallback(
            &slug, "",
            display::LANG_BTN_TONE_HZ,
        );
    }

    // =========================================================================
    // Mouse helpers
    // =========================================================================

    fn mouse_state_for(&mut self, source: InputSource) -> &mut MouseMoveState {
        match source {
            InputSource::Keyboard => &mut self.mouse_state_kb,
            InputSource::Gamepad  => &mut self.mouse_state_gp,
            InputSource::Gpio     => &mut self.mouse_state_gpio,
        }
    }

    fn update_mouse_buttons(&mut self, bit: u8, pressed: bool) -> u8 {
        if pressed { self.mouse_buttons |= bit; } else { self.mouse_buttons &= !bit; }
        self.mouse_buttons
    }

    /// Synchronise `mouse_buttons` bitmask from `pressed_keys`.
    ///
    /// Polls the pressed-key set to ensure the left-click and right-click
    /// bits accurately reflect whether the activate / activate_shift keys
    /// are physically held.  Called in the auto-repeat timer and before
    /// sending direction-key mouse reports to overcome event ordering
    /// issues (e.g. iced widget captures preventing timely delivery).
    fn sync_mouse_buttons(&mut self) {
        if !self.mouse_mode { return; }
        // Left button ↔ activate key
        let left_down = self.pressed_keys.contains(&self.nav_keys.activate);
        if left_down { self.mouse_buttons |= 0x01; } else { self.mouse_buttons &= !0x01; }
        // Right button ↔ activate_shift key
        if let Some(sc) = self.nav_keys.activate_shift {
            let right_down = self.pressed_keys.contains(&sc);
            if right_down { self.mouse_buttons |= 0x02; } else { self.mouse_buttons &= !0x02; }
        }
    }

    fn do_mouse_auto_repeat(&mut self, source: InputSource) {
        let mouse_cfg = self.mouse_cfg.clone();
        self.sync_mouse_buttons();
        let mouse_buttons = self.mouse_buttons;
        let mouse = self.mouse_state_for(source);
        if mouse.dx == 0 && mouse.dy == 0 { return; }
        let now = Instant::now();
        if let Some(next) = mouse.next {
            if now >= next {
                let elapsed_ms = mouse.start
                    .map_or(0, |s| now.duration_since(s).as_millis() as u64);
                let max_size = mouse_cfg.move_max_size.max(1) as u64;
                let ramp_ms  = mouse_cfg.move_max_time.max(1);
                let delta = ((elapsed_ms.min(ramp_ms) * max_size / ramp_ms) as i8).max(1);
                let dx = if mouse.dx > 0 { delta } else if mouse.dx < 0 { -delta } else { 0i8 };
                let dy = if mouse.dy > 0 { delta } else if mouse.dy < 0 { -delta } else { 0i8 };
                self.hook.on_mouse_report(mouse_buttons, dx, dy);
                let interval = Duration::from_millis(mouse_cfg.repeat_interval);
                let mouse = self.mouse_state_for(source);
                mouse.next = Some(now + interval);
            }
        }
    }

    // =========================================================================
    // Gamepad / GPIO polling
    // =========================================================================

    fn poll_gamepad(&mut self) {
        let gp_cfg = match &self.gp_cfg {
            Some(cfg) => cfg.clone(),
            None => return,
        };

        // Try reconnect if disconnected.
        if self.gp_cell.is_none() {
            if let Some(gp) = Gamepad::open(&gp_cfg) {
                eprintln!("[gamepad] reconnected");
                self.gp_cell = Some(gp);
                self.gp_connected = true;
            }
            return;
        }

        let mut evt_buf: Vec<UserInputEvent> = Vec::new();
        let still_alive = self.gp_cell.as_mut().unwrap().poll(&mut evt_buf);

        if !still_alive {
            eprintln!("[gamepad] disconnected");
            self.gp_cell = None;
            self.gp_connected = false;
            return;
        }

        self.process_input_events(&evt_buf, InputSource::Gamepad);
    }

    fn poll_gpio(&mut self) {
        let gpio_cfg = match &self.gpio_cfg {
            Some(cfg) => cfg.clone(),
            None => return,
        };

        // Try open if not yet available.
        if self.gpio_cell.is_none() {
            if let Some(gpio) = GpioInput::open(&gpio_cfg) {
                eprintln!("[gpio] opened");
                self.gpio_cell = Some(gpio);
                self.gpio_connected = true;
            }
            return;
        }

        let mut evt_buf: Vec<UserInputEvent> = Vec::new();
        self.gpio_cell.as_mut().unwrap().poll(&mut evt_buf);

        self.process_input_events(&evt_buf, InputSource::Gpio);
    }

    // =========================================================================
    // BLE connection management
    // =========================================================================

    fn manage_ble_connection(&mut self) {
        let rc = match self.ble_conn.as_ref() {
            Some(c) => c,
            None => return,
        };
        let conn = &mut *rc.borrow_mut();

        if !conn.is_connected() {
            if conn.try_connect() {
                self.ble_state = BleState::Connecting;
            }
            return;
        }

        match conn.check_status() {
            Ok(Some(ref s)) if s.starts_with("STATUS:CONNECTED:") => {
                self.ble_state = BleState::Connected;
            }
            Ok(Some(_)) | Ok(None) => {
                self.ble_state = BleState::Connecting;
            }
            Err(()) => {
                self.ble_state = BleState::Disconnected;
            }
        }
    }

    // =========================================================================
    // Absolute position helper
    // =========================================================================

    fn compute_absolute_sel(&self, horiz: f32, vert: f32) -> NavSel {
        let num_rows = self.all_btns.len();
        let num_lang = self.lang_btns.len();
        if num_rows == 0 { return NavSel::Key(0, 0); }
        let has_lang = num_lang > 0;
        let total_bands = if has_lang { 1 + num_rows } else { num_rows };

        let (center_band, center_horiz_frac) =
            match find_center_key(&self.all_btns, self.layout_idx, &self.center_key) {
                Some(NavSel::Key(r, c)) => {
                    let band = if has_lang { r + 1 } else { r };
                    let frac = (c as f32 + 0.5) / self.all_btns[r].len() as f32;
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
            let row = if has_lang { band - 1 } else { band };
            let num_cols = self.all_btns[row].len();
            let col = (mapped_horiz * num_cols as f32)
                .floor()
                .clamp(0.0, num_cols as f32 - 1.0) as usize;
            NavSel::Key(row, col)
        }
    }
}

// =============================================================================
// Input source identifier
// =============================================================================

#[derive(Clone, Copy)]
enum InputSource {
    Keyboard,
    Gamepad,
    Gpio,
}

// =============================================================================
// Helper: convert directional action to mouse delta
// =============================================================================

#[inline]
fn dir_to_mouse_delta(action: UserInputAction) -> (i8, i8) {
    match action {
        UserInputAction::Up    => ( 0i8, -1i8),
        UserInputAction::Down  => ( 0i8,  1i8),
        UserInputAction::Left  => (-1i8,  0i8),
        _                      => ( 1i8,  0i8),
    }
}

// =============================================================================
// Helper: build modifier-button descriptors from the grid
// =============================================================================

fn build_mod_btns(all_btns: &[Vec<BtnData>], _colors: &Colors) -> Vec<ModBtn> {
    let mut v = Vec::new();
    for (r, row) in all_btns.iter().enumerate() {
        for (c, btn) in row.iter().enumerate() {
            if keyboards::is_modifier(btn.action) {
                v.push(ModBtn {
                    row: r,
                    col: c,
                    action: btn.action,
                    base_color: btn.base_color,
                });
            }
        }
    }
    v
}

// =============================================================================
// Main
// =============================================================================

fn main() -> iced::Result {
    iced::application(SmartKeyboard::new, SmartKeyboard::update, SmartKeyboard::view)
        .title("Smart Keyboard")
        .subscription(SmartKeyboard::subscription)
        .window(iced::window::Settings {
            decorations: false,
            ..Default::default()
        })
        .run()
}
