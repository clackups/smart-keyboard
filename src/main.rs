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

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Color, Element, Length, Subscription, Task};
use iced::keyboard;
use iced::event;
use iced::widget;

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
// Menu constants
// =============================================================================

const MENU_ITEM_CONFIGURATION: usize = 0;
const MENU_ITEM_DISCONNECT_BLE: usize = 1;
const MENU_ITEM_EXIT: usize = 2;
const MENU_ITEM_COUNT: usize = 3;

// Well-known scancodes for menu navigation (fallbacks when not overridden by config).
const SC_ENTER: u32 = 28;
const SC_ESC: u32 = 1;
const SC_ARROW_UP: u32 = 103;
const SC_ARROW_DOWN: u32 = 108;
const SC_LCTRL: u32 = 29;
const SC_RCTRL: u32 = 97;
const SC_S: u32 = 31;
const SC_TAB: u32 = 15;
const SC_LSHIFT: u32 = 42;
const SC_RSHIFT: u32 = 54;

// =============================================================================
// Configuration editor state
// =============================================================================

struct ConfigEditorState {
    /// Flattened (dotted.key, value) pairs loaded from config.toml.
    pairs: Vec<(String, String)>,
    /// Currently selected row in the list (for keyboard navigation).
    sel: usize,
    /// Whether a text_input widget is being edited.
    editing: bool,
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
    MenuDisconnectBle,
    MenuExitApp,
    MenuOpenConfig,
    ConfigValueChanged(usize, String),
    ConfigSave,
    ConfigCancel,
    ConfigFieldSubmit,
    GotScreenSize(f32, f32),
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
    menu_sel: usize,
    config_editor: Option<ConfigEditorState>,
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

        // Use a reasonable default screen size for initial layout computation.
        // The layout is recomputed once the actual window size is obtained
        // via the GotScreenSize startup task.
        let (sw, sh) = (800i32, 480i32);
        let metrics = display::compute_layout(sw, sh, cfg.ui.show_text_display);

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
            menu_sel: 0,
            config_editor: None,
        };

        // Query the actual window size so the layout matches the real screen.
        let size_task = iced::window::oldest().and_then(|id| {
            iced::window::size(id)
        }).map(|sz| Message::GotScreenSize(sz.width, sz.height));

        (app, size_task)
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
                // Allow key-repeat in config editor navigation (held
                // arrows / Tab should scroll through items continuously).
                let is_repeat = !self.pressed_keys.insert(sc);
                let in_config = self.config_editor.is_some();

                if is_repeat && !in_config {
                    // Outside config editor, suppress OS key-repeat.
                    return Task::none();
                }

                // --- Config editor: when editing a text field, let iced ---
                // --- handle all keyboard input natively.               ---
                if self.config_editor.as_ref().map_or(false, |c| c.editing) {
                    // Escape cancels editing (unfocuses the field).
                    if sc == SC_ESC {
                        if let Some(ref mut ce) = self.config_editor {
                            ce.editing = false;
                        }
                    }
                    // Ctrl+S -> Save & Restart even while editing.
                    let ctrl_held = self.pressed_keys.contains(&SC_LCTRL)
                        || self.pressed_keys.contains(&SC_RCTRL);
                    if ctrl_held && sc == SC_S {
                        return self.update(Message::ConfigSave);
                    }
                    // Tab / Shift+Tab: leave editing and move to next/prev item.
                    if sc == SC_TAB {
                        let shift_held = self.pressed_keys.contains(&SC_LSHIFT)
                            || self.pressed_keys.contains(&SC_RSHIFT);
                        if let Some(ref mut ce) = self.config_editor {
                            ce.editing = false;
                            if shift_held {
                                ce.sel = ce.sel.saturating_sub(1);
                            } else if ce.sel < ce.pairs.len().saturating_sub(1) {
                                ce.sel += 1;
                            }
                        }
                        return self.config_scroll_to_sel();
                    }
                    return Task::none();
                }

                // --- Config editor navigation (not editing) ---
                if in_config {
                    return self.handle_config_key_press(sc);
                }

                // --- Main menu navigation ---
                if self.showing_menu {
                    return self.handle_menu_key_press(sc);
                }

                let events =
                    phys_keyboard::translate_key_event(sc, true, &self.nav_keys);
                return self.process_input_events(&events, InputSource::Keyboard);
            }

            Message::KeyReleased(sc) => {
                self.pressed_keys.remove(&sc);

                // Suppress release processing while menu/config is active.
                if self.showing_menu || self.config_editor.is_some() {
                    return Task::none();
                }

                let events =
                    phys_keyboard::translate_key_event(sc, false, &self.nav_keys);
                return self.process_input_events(&events, InputSource::Keyboard);
            }

            Message::PollTick => {
                let mut tasks = Vec::new();
                // Poll gamepad
                if self.gp_cfg.is_some() {
                    tasks.push(self.poll_gamepad());
                }
                // Poll GPIO
                if self.gpio_cfg.is_some() {
                    tasks.push(self.poll_gpio());
                }
                // Mouse auto-repeat for all sources (only when not in menu)
                if !self.showing_menu && self.config_editor.is_none() {
                    self.do_mouse_auto_repeat(InputSource::Keyboard);
                    self.do_mouse_auto_repeat(InputSource::Gamepad);
                    self.do_mouse_auto_repeat(InputSource::Gpio);
                }
                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
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
                self.menu_sel = 0;
            }

            Message::MenuClose => {
                self.showing_menu = false;
                self.config_editor = None;
            }

            Message::MenuDisconnectBle => {
                if let Some(ref rc) = self.ble_conn {
                    rc.borrow_mut().send_disconnect();
                    self.ble_state = BleState::Disconnected;
                }
                self.showing_menu = false;
            }

            Message::MenuExitApp => {
                return iced::exit();
            }

            Message::MenuOpenConfig => {
                let pairs = menu::load_config_pairs();
                self.config_editor = Some(ConfigEditorState {
                    pairs,
                    sel: 0,
                    editing: false,
                });
            }

            Message::ConfigValueChanged(idx, val) => {
                if let Some(ref mut ce) = self.config_editor {
                    if idx < ce.pairs.len() {
                        ce.pairs[idx].1 = val;
                    }
                }
            }

            Message::ConfigFieldSubmit => {
                if let Some(ref mut ce) = self.config_editor {
                    ce.editing = false;
                }
            }

            Message::ConfigSave => {
                if let Some(ref ce) = self.config_editor {
                    let pairs: Vec<(&str, String)> = ce.pairs.iter()
                        .map(|(k, v)| (k.as_str(), v.clone()))
                        .collect();
                    match menu::build_toml_and_save(&pairs) {
                        Ok(()) => {
                            eprintln!("[menu] config saved, restarting...");
                            menu::restart_application();
                        }
                        Err(e) => {
                            eprintln!("[menu] failed to save config: {}", e);
                        }
                    }
                }
            }

            Message::ConfigCancel => {
                self.config_editor = None;
            }

            Message::GotScreenSize(w, h) => {
                let sw = w as i32;
                let sh = h as i32;
                if sw != self.metrics.sw || sh != self.metrics.sh {
                    self.metrics = display::compute_layout(sw, sh, self.show_text_display);
                    self.all_btns = display::build_btn_grid(&self.metrics, &self.colors);
                    self.lang_btns = display::build_lang_btns(&self.metrics);
                    self.mod_btns = build_mod_btns(&self.all_btns, &self.colors);
                }
            }
        }

        Task::none()
    }

    // =========================================================================
    // View
    // =========================================================================

    fn view(&self) -> Element<'_, Message> {
        if self.config_editor.is_some() {
            return self.view_config_editor();
        }
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
            let num_lang = self.lang_btns.len() as u16;
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
                        .align_y(iced::alignment::Vertical::Center)
                )
                .width(Length::FillPortion(1))
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
            // Pad with trailing spacers so lang buttons match the 17-column grid width.
            for _ in 0..keyboards::GRID_COLS.saturating_sub(num_lang) {
                lr = lr.push(Space::new().width(Length::FillPortion(1)));
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
        let ctrl_lbl_size = metrics.ctrl_lbl_size as f32;
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
                                .align_y(iced::alignment::Vertical::Center)
                                .width(Length::Fill)
                                .into()
                        }
                    }
                    Action::Space => {
                        Space::new().into()
                    }
                    other => {
                        // Modifier / function / arrow key.
                        // Single-char labels (arrows) stay big; multi-char
                        // control labels use a smaller size so words like
                        // "Shift" or "Enter" fit inside the button.
                        let lbl = keyboards::special_label(other).to_string();
                        text(lbl)
                            .size(ctrl_lbl_size)
                            .color(label_color)
                            .align_x(iced::alignment::Horizontal::Center)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Length::Fill)
                            .into()
                    }
                };

                let btn = button(btn_content)
                .width(Length::FillPortion(portion))
                .height(Length::Fixed(metrics.key_h as f32))
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

        let items: [(&str, Message, bool); MENU_ITEM_COUNT] = [
            ("Configuration",    Message::MenuOpenConfig,     true),
            ("Disconnect BLE",   Message::MenuDisconnectBle,  self.ble_can_disconnect()),
            ("Exit Application", Message::MenuExitApp,        true),
        ];

        let mut menu_col = column![title].spacing(12)
            .align_x(iced::alignment::Horizontal::Center);

        for (i, (label, msg, enabled)) in items.into_iter().enumerate() {
            let is_selected = i == self.menu_sel;
            let bg = if is_selected {
                colors.nav_sel
            } else {
                Color::from_rgb(0.24, 0.24, 0.26)
            };
            let label_color = if !enabled {
                Color::from_rgb(0.35, 0.35, 0.35)
            } else if is_selected {
                Color::BLACK
            } else {
                Color::WHITE
            };

            let btn = button(
                text(label)
                    .size(20)
                    .color(label_color)
                    .align_x(iced::alignment::Horizontal::Center)
            )
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(50.0))
            .style(move |_theme: &iced::Theme, _status| button::Style {
                background: Some(iced::Background::Color(bg)),
                text_color: label_color,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                shadow: iced::Shadow::default(),
                snap: true,
            });

            let btn = if enabled { btn.on_press(msg) } else { btn };
            menu_col = menu_col.push(btn);
        }

        // "Close" at the bottom.
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
            background: Some(iced::Background::Color(Color::from_rgb(0.18, 0.18, 0.20))),
            text_color: Color::WHITE,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow::default(),
            snap: true,
        });

        menu_col = menu_col.push(Space::new().height(Length::Fixed(8.0)));
        menu_col = menu_col.push(close_btn);

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

    fn view_config_editor(&self) -> Element<'_, Message> {
        let colors = self.colors;
        let ce = self.config_editor.as_ref().unwrap();

        let title = text("Configuration")
            .size(26)
            .color(Color::WHITE);

        // Build scrollable list of config items.
        let mut list_col = column![].spacing(4).width(Length::Fill);
        let mut current_section = String::new();

        for (i, (key, val)) in ce.pairs.iter().enumerate() {
            // Section header when the section prefix changes.
            let section = match key.rfind('.') {
                Some(pos) => &key[..pos],
                None => "",
            };
            if section != current_section {
                current_section = section.to_string();
                if !current_section.is_empty() {
                    let header = text(format!("[{}]", current_section))
                        .size(14)
                        .color(Color::from_rgb(0.85, 0.85, 0.95));
                    list_col = list_col.push(
                        container(header)
                            .padding([6, 8])
                            .width(Length::Fill)
                            .style(|_theme: &iced::Theme| container::Style {
                                background: Some(iced::Background::Color(
                                    Color::from_rgb(0.08, 0.08, 0.10),
                                )),
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })
                    );
                }
            }

            let field_name = match key.rfind('.') {
                Some(pos) => &key[pos + 1..],
                None => key.as_str(),
            };

            let is_selected = i == ce.sel;
            let row_bg = if is_selected {
                Color::from_rgb(0.22, 0.22, 0.28)
            } else {
                Color::from_rgb(0.15, 0.15, 0.17)
            };

            let label = text(field_name)
                .size(14)
                .color(Color::from_rgb(0.75, 0.75, 0.8))
                .width(Length::FillPortion(2));

            let field_id = widget::Id::from(format!("cfg-{}", i));
            let input = text_input("", val)
                .id(field_id)
                .size(14)
                .on_input(move |v| Message::ConfigValueChanged(i, v))
                .on_submit(Message::ConfigFieldSubmit)
                .width(Length::FillPortion(3));

            let r = container(
                row![label, input].spacing(8).align_y(iced::alignment::Vertical::Center)
            )
            .padding([4, 8])
            .style(move |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(row_bg)),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

            list_col = list_col.push(r);
        }

        let scroll = scrollable(list_col)
            .id(widget::Id::from("cfg-scroll"))
            .width(Length::Fill)
            .height(Length::Fill);

        // Bottom buttons.
        let save_btn = button(
            text("Save & Restart").size(18).color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center)
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(44.0))
        .on_press(Message::ConfigSave)
        .style(|_theme: &iced::Theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.2, 0.5, 0.3))),
            text_color: Color::WHITE,
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            shadow: iced::Shadow::default(),
            snap: true,
        });

        let cancel_btn = button(
            text("Cancel").size(18).color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center)
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fixed(44.0))
        .on_press(Message::ConfigCancel)
        .style(|_theme: &iced::Theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.3, 0.18, 0.18))),
            text_color: Color::WHITE,
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            shadow: iced::Shadow::default(),
            snap: true,
        });

        let btn_row = row![save_btn, Space::new().width(Length::Fixed(20.0)), cancel_btn]
            .align_y(iced::alignment::Vertical::Center);

        let main_col = column![title, scroll, btn_row]
            .spacing(8)
            .padding(12)
            .align_x(iced::alignment::Horizontal::Center)
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
    // Menu / config keyboard navigation
    // =========================================================================

    /// Returns `true` when "Disconnect BLE" should be selectable.
    fn ble_can_disconnect(&self) -> bool {
        self.ble_conn.is_some() && self.ble_state == BleState::Connected
    }

    /// Handle a physical key-press while the main menu is showing.
    fn handle_menu_key_press(&mut self, sc: u32) -> Task<Message> {
        let is_up   = sc == self.nav_keys.up   || sc == SC_ARROW_UP;
        let is_down = sc == self.nav_keys.down  || sc == SC_ARROW_DOWN;
        let is_activate = sc == self.nav_keys.activate || sc == SC_ENTER;
        let is_close = sc == self.nav_keys.menu || sc == SC_ESC;

        if is_up && self.menu_sel > 0 {
            self.menu_sel -= 1;
        }
        if is_down && self.menu_sel < MENU_ITEM_COUNT - 1 {
            self.menu_sel += 1;
        }
        if is_activate {
            return self.activate_menu_item();
        }
        if is_close {
            self.showing_menu = false;
        }
        Task::none()
    }

    /// Activate the currently highlighted menu item.
    fn activate_menu_item(&mut self) -> Task<Message> {
        match self.menu_sel {
            MENU_ITEM_CONFIGURATION => {
                let pairs = menu::load_config_pairs();
                self.config_editor = Some(ConfigEditorState {
                    pairs,
                    sel: 0,
                    editing: false,
                });
                Task::none()
            }
            MENU_ITEM_DISCONNECT_BLE => {
                if self.ble_can_disconnect() {
                    if let Some(ref rc) = self.ble_conn {
                        rc.borrow_mut().send_disconnect();
                        self.ble_state = BleState::Disconnected;
                    }
                    self.showing_menu = false;
                }
                Task::none()
            }
            MENU_ITEM_EXIT => {
                iced::exit()
            }
            _ => Task::none(),
        }
    }

    /// Handle a physical key-press while the config editor is showing
    /// (but not editing a text field).
    fn handle_config_key_press(&mut self, sc: u32) -> Task<Message> {
        let is_up   = sc == self.nav_keys.up   || sc == SC_ARROW_UP;
        let is_down = sc == self.nav_keys.down  || sc == SC_ARROW_DOWN;
        let is_activate = sc == self.nav_keys.activate || sc == SC_ENTER;
        let is_close = sc == self.nav_keys.menu || sc == SC_ESC;

        // Ctrl+S -> Save & Restart.
        let ctrl_held = self.pressed_keys.contains(&SC_LCTRL)
            || self.pressed_keys.contains(&SC_RCTRL);
        if ctrl_held && sc == SC_S {
            return self.update(Message::ConfigSave);
        }

        // Tab / Shift+Tab -> move selection down / up.
        let shift_held = self.pressed_keys.contains(&SC_LSHIFT)
            || self.pressed_keys.contains(&SC_RSHIFT);
        let tab_down = sc == SC_TAB && !shift_held;
        let tab_up   = sc == SC_TAB && shift_held;

        if let Some(ref mut ce) = self.config_editor {
            if (is_up || tab_up) && ce.sel > 0 {
                ce.sel -= 1;
            }
            if (is_down || tab_down) && ce.sel < ce.pairs.len().saturating_sub(1) {
                ce.sel += 1;
            }
            if is_activate {
                ce.editing = true;
                let id = widget::Id::from(format!("cfg-{}", ce.sel));
                return widget::operation::focus(id);
            }
            if is_close {
                self.config_editor = None;
                return Task::none();
            }
            // Auto-scroll the list to keep the selected item visible.
            if is_up || is_down || tab_down || tab_up {
                return self.config_scroll_to_sel();
            }
        }
        Task::none()
    }

    /// Return a `snap_to` Task that scrolls the config list so the currently
    /// selected item is visible.
    fn config_scroll_to_sel(&self) -> Task<Message> {
        if let Some(ref ce) = self.config_editor {
            if ce.pairs.is_empty() {
                return Task::none();
            }
            let last = ce.pairs.len().saturating_sub(1).max(1);
            let frac = ce.sel as f32 / last as f32;
            return widget::operation::snap_to(
                widget::Id::from("cfg-scroll"),
                scrollable::RelativeOffset { x: 0.0, y: frac },
            );
        }
        Task::none()
    }

    // =========================================================================
    // Input event processing for overlays (menu / config editor)
    // =========================================================================

    /// Handle gamepad/GPIO events while the main menu or config editor is
    /// showing.  Translates directional + activate + menu actions to
    /// menu/config navigation.
    fn process_overlay_input_events(&mut self, events: &[UserInputEvent]) -> Task<Message> {
        for evt in events {
            if !evt.pressed { continue; }

            if self.config_editor.is_some() {
                let mut moved = false;
                match evt.action {
                    UserInputAction::Up => {
                        if let Some(ref mut ce) = self.config_editor {
                            if ce.sel > 0 {
                                ce.sel -= 1;
                                moved = true;
                            }
                        }
                    }
                    UserInputAction::Down => {
                        if let Some(ref mut ce) = self.config_editor {
                            let max = ce.pairs.len().saturating_sub(1);
                            if ce.sel < max {
                                ce.sel += 1;
                                moved = true;
                            }
                        }
                    }
                    UserInputAction::Menu => {
                        self.config_editor = None;
                    }
                    _ => {}
                }
                if moved {
                    return self.config_scroll_to_sel();
                }
            } else {
                // Main menu
                match evt.action {
                    UserInputAction::Up => {
                        self.menu_sel = self.menu_sel.saturating_sub(1);
                    }
                    UserInputAction::Down => {
                        if self.menu_sel < MENU_ITEM_COUNT - 1 {
                            self.menu_sel += 1;
                        }
                    }
                    UserInputAction::Activate => {
                        return self.activate_menu_item();
                    }
                    UserInputAction::Menu => {
                        self.showing_menu = false;
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }

    // =========================================================================
    // Input event processing (replaces old process_input_events + InputCtx)
    // =========================================================================

    fn process_input_events(&mut self, events: &[UserInputEvent], source: InputSource) -> Task<Message> {
        // Redirect gamepad/GPIO events when the menu or config editor is open.
        if self.showing_menu || self.config_editor.is_some() {
            return self.process_overlay_input_events(events);
        }

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
                        if source == InputSource::Keyboard {
                            self.sync_mouse_buttons_for_keyboard();
                        }
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
        Task::none()
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
    /// Synchronise `mouse_buttons` from physical keyboard state only.
    /// Only called for InputSource::Keyboard so that gamepad/GPIO button
    /// state is not overwritten.
    fn sync_mouse_buttons_for_keyboard(&mut self) {
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
        if source == InputSource::Keyboard {
            self.sync_mouse_buttons_for_keyboard();
        }
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

    fn poll_gamepad(&mut self) -> Task<Message> {
        let gp_cfg = match &self.gp_cfg {
            Some(cfg) => cfg.clone(),
            None => return Task::none(),
        };

        // Try reconnect if disconnected.
        if self.gp_cell.is_none() {
            if let Some(gp) = Gamepad::open(&gp_cfg) {
                eprintln!("[gamepad] reconnected");
                self.gp_cell = Some(gp);
                self.gp_connected = true;
            }
            return Task::none();
        }

        let mut evt_buf: Vec<UserInputEvent> = Vec::new();
        let still_alive = self.gp_cell.as_mut().unwrap().poll(&mut evt_buf);

        if !still_alive {
            eprintln!("[gamepad] disconnected");
            self.gp_cell = None;
            self.gp_connected = false;
            return Task::none();
        }

        self.process_input_events(&evt_buf, InputSource::Gamepad)
    }

    fn poll_gpio(&mut self) -> Task<Message> {
        let gpio_cfg = match &self.gpio_cfg {
            Some(cfg) => cfg.clone(),
            None => return Task::none(),
        };

        // Try open if not yet available.
        if self.gpio_cell.is_none() {
            if let Some(gpio) = GpioInput::open(&gpio_cfg) {
                eprintln!("[gpio] opened");
                self.gpio_cell = Some(gpio);
                self.gpio_connected = true;
            }
            return Task::none();
        }

        let mut evt_buf: Vec<UserInputEvent> = Vec::new();
        self.gpio_cell.as_mut().unwrap().poll(&mut evt_buf);

        self.process_input_events(&evt_buf, InputSource::Gpio)
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

#[derive(Clone, Copy, PartialEq)]
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
