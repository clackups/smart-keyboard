// src/config.rs
//
// Loads input and output configuration from a TOML file.  The path is taken
// from the SMART_KBD_CONFIG_PATH environment variable; if unset, "config.toml"
// in the current working directory is used.
// Falls back to built-in defaults if the file is missing or unparseable.

use std::env;
use std::fs;

use fltk::enums::Key;
use serde::Deserialize;

// =============================================================================
// TOML-deserializable structs
// =============================================================================

#[derive(Deserialize)]
pub struct KeyboardInputConfig {
    /// Linux evdev scan code for "navigate up" (default: 0x67 KEY_UP).
    pub navigate_up: u32,
    /// Linux evdev scan code for "navigate down" (default: 0x6c KEY_DOWN).
    pub navigate_down: u32,
    /// Linux evdev scan code for "navigate left" (default: 0x69 KEY_LEFT).
    pub navigate_left: u32,
    /// Linux evdev scan code for "navigate right" (default: 0x6a KEY_RIGHT).
    pub navigate_right: u32,
    /// Linux evdev scan code for "activate" (default: 0x39 KEY_SPACE).
    pub activate: u32,
    /// Linux evdev scan code for "menu" (default: 0x32 KEY_M).
    pub menu: u32,
}

#[derive(Deserialize, Clone)]
pub struct GamepadInputConfig {
    pub enabled: bool,
    pub device: String,
    /// Button index for "navigate up"; absent / `null` means disabled.
    #[serde(default)]
    pub navigate_up: Option<u32>,
    /// Button index for "navigate down"; absent / `null` means disabled.
    #[serde(default)]
    pub navigate_down: Option<u32>,
    /// Button index for "navigate left"; absent / `null` means disabled.
    #[serde(default)]
    pub navigate_left: Option<u32>,
    /// Button index for "navigate right"; absent / `null` means disabled.
    #[serde(default)]
    pub navigate_right: Option<u32>,
    /// Button index for "activate"; absent / `null` means disabled.
    /// Default: 0x05.
    #[serde(default = "default_activate")]
    pub activate: Option<u32>,
    /// Button index for "menu"; absent / `null` means disabled.
    /// Default: 0x08.
    #[serde(default = "default_menu")]
    pub menu: Option<u32>,
    /// Axis index used for left/right navigation.
    /// Negative axis values → Left, positive → Right.
    /// Absent / `null` means disabled.
    /// Default: 0 (left stick X on most gamepads).
    #[serde(default = "default_axis_navigate_horizontal")]
    pub axis_navigate_horizontal: Option<u32>,
    /// Axis index used for up/down navigation.
    /// Negative axis values → Up, positive → Down.
    /// Absent / `null` means disabled.
    /// Default: 1 (left stick Y on most gamepads).
    #[serde(default = "default_axis_navigate_vertical")]
    pub axis_navigate_vertical: Option<u32>,
    /// Axis index for the activate action.
    /// Positive axis values above `axis_threshold` trigger Activate.
    /// Absent / `null` means disabled.
    /// Default: 0x05.
    #[serde(default = "default_axis_activate")]
    pub axis_activate: Option<u32>,
    /// Axis index for the menu action.
    /// Positive axis values above `axis_threshold` trigger Menu.
    /// Absent / `null` means disabled.
    #[serde(default)]
    pub axis_menu: Option<u32>,
    /// Minimum absolute axis value (0–32767) needed to register as active.
    /// Compared as `|value| > axis_threshold` against the raw i16 axis value.
    /// Default: 16384 (half of the maximum i16 range).
    #[serde(default = "default_axis_threshold")]
    pub axis_threshold: i32,
    /// When `true`, the raw values of `axis_navigate_horizontal` and
    /// `axis_navigate_vertical` are treated as absolute coordinates that map
    /// directly to a keyboard key position rather than as directional inputs.
    /// The full axis range (−32767 … +32767) is divided evenly across the
    /// available rows/columns.  Default: false.
    #[serde(default)]
    pub absolute_axes: bool,
    /// When `true`, a short force-feedback rumble is sent to the gamepad on
    /// every change of the keyboard navigation selection.  Default: false.
    #[serde(default)]
    pub rumble: bool,
    /// Duration of the rumble effect in milliseconds.  Default: 50.
    #[serde(default = "default_rumble_duration_ms")]
    pub rumble_duration_ms: u16,
    /// Intensity of the rumble motors (0 = silent, 65535 = maximum).
    /// Applied to both the strong and weak motors.  Default: 0x4000 (~25 %).
    #[serde(default = "default_rumble_magnitude")]
    pub rumble_magnitude: u16,
}

fn default_activate()                 -> Option<u32> { Some(0x05) }
fn default_menu()                     -> Option<u32> { Some(0x08) }
fn default_axis_navigate_horizontal() -> Option<u32> { Some(0) }
fn default_axis_navigate_vertical()   -> Option<u32> { Some(1) }
fn default_axis_activate()            -> Option<u32> { Some(0x05) }
fn default_axis_threshold()           -> i32  { 16384 }
fn default_rumble_duration_ms()       -> u16  { 50 }
fn default_rumble_magnitude()         -> u16  { 0x4000 }

#[derive(Deserialize)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad: GamepadInputConfig,
}

// =============================================================================
// Output configuration
// =============================================================================

/// Which output backend the application uses to forward key events.
#[derive(Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Print key events to stdout; no hardware output (default).
    #[default]
    Print,
    /// Send USB HID reports to the BLE dongle over a USB-serial port.
    Ble,
}

/// Audio feedback mode for keyboard-navigation selection changes.
#[derive(Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioMode {
    /// No audio feedback (default).
    #[default]
    None,
    /// Play a WAV clip naming each button.  Clips are loaded from the
    /// directory given by `SMART_KBD_AUDIO_PATH` (env var) or from the
    /// `audio/` sub-directory of the current working directory.
    Narrate,
    /// Play a short musical tone whose pitch varies by key category:
    /// letter/punctuation keys share one tone; F and J have a distinctive
    /// tone; digit keys play an ascending scale (1 = lowest, 0 = highest);
    /// function keys (F1–F12) play a lower ascending scale; all other
    /// special keys have their own unique tones.
    Tone,
}

/// USB identification for the BLE dongle (esp_hid_serial_bridge).
#[derive(Deserialize, Clone)]
pub struct BleOutputConfig {
    /// USB Vendor ID (default: 0x1209).
    pub vid: u16,
    /// USB Product ID (default: 0xbbd1).
    pub pid: u16,
    /// Optional USB serial string; when absent, the first matching device is used.
    pub serial: Option<String>,
}

impl Default for BleOutputConfig {
    fn default() -> Self {
        BleOutputConfig {
            vid:    0x1209,
            pid:    0xbbd1,
            serial: None,
        }
    }
}

#[derive(Deserialize, Default)]
pub struct OutputConfig {
    /// Output mode (default: "print").
    #[serde(default)]
    pub mode: OutputMode,
    /// BLE dongle settings (used only when mode = "ble").
    #[serde(default)]
    pub ble: BleOutputConfig,
    /// Audio feedback mode for keyboard-navigation selection changes.
    /// "none"    – silent (default)
    /// "narrate" – play a WAV clip naming each button
    /// "tone"    – play a short musical tone that varies by key category
    #[serde(default)]
    pub audio: AudioMode,
}

// =============================================================================
// UI / colour configuration
// =============================================================================

/// An RGB colour stored as a `"#RRGGBB"` hex string in the TOML file.
///
/// Example: `key_normal = "#dadade"`
#[derive(Clone, Copy)]
pub struct ColorRgb(pub u8, pub u8, pub u8);

impl<'de> Deserialize<'de> for ColorRgb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let hex = s.trim_start_matches('#');
        if hex.len() != 6 {
            return Err(serde::de::Error::custom(
                "colour must be a 6-digit hex string like \"#RRGGBB\"",
            ));
        }
        let r = u8::from_str_radix(&hex[0..2], 16).map_err(serde::de::Error::custom)?;
        let g = u8::from_str_radix(&hex[2..4], 16).map_err(serde::de::Error::custom)?;
        let b = u8::from_str_radix(&hex[4..6], 16).map_err(serde::de::Error::custom)?;
        Ok(ColorRgb(r, g, b))
    }
}

fn default_col_key_normal()             -> ColorRgb { ColorRgb(218, 218, 222) }
fn default_col_key_mod()                -> ColorRgb { ColorRgb(100, 100, 110) }
fn default_col_mod_active()             -> ColorRgb { ColorRgb( 70, 130, 180) }
fn default_col_nav_sel()                -> ColorRgb { ColorRgb(255, 200,   0) }
fn default_col_status_bar_bg()          -> ColorRgb { ColorRgb( 25,  25,  28) }
fn default_col_status_ind_bg()          -> ColorRgb { ColorRgb( 45,  45,  50) }
fn default_col_status_ind_text()        -> ColorRgb { ColorRgb( 90,  90,  95) }
fn default_col_status_ind_active_text() -> ColorRgb { ColorRgb(255, 255, 255) }
fn default_col_conn_disconnected()      -> ColorRgb { ColorRgb(220,  60,  60) }
fn default_col_conn_connecting()        -> ColorRgb { ColorRgb(220, 150,  40) }
fn default_col_conn_connected()         -> ColorRgb { ColorRgb( 80, 220,  80) }
fn default_col_win_bg()                 -> ColorRgb { ColorRgb( 40,  40,  43) }
fn default_col_disp_bg()                -> ColorRgb { ColorRgb( 28,  28,  28) }
fn default_col_disp_text()              -> ColorRgb { ColorRgb(180, 255, 180) }
fn default_col_lang_btn_inactive()      -> ColorRgb { ColorRgb( 80,  80,  80) }
fn default_col_lang_btn_label()         -> ColorRgb { ColorRgb(255, 255, 255) }
fn default_col_key_label_normal()       -> ColorRgb { ColorRgb( 20,  20,  20) }
fn default_col_key_label_mod()          -> ColorRgb { ColorRgb(210, 210, 210) }

/// All configurable UI colours.  Each field defaults to the built-in palette
/// when absent from the TOML file.
#[derive(Deserialize, Clone)]
pub struct ColorsConfig {
    /// Regular key (letter / digit / symbol / Space) button background.
    #[serde(default = "default_col_key_normal")]
    pub key_normal:              ColorRgb,
    /// Modifier / function / navigation key button background (inactive).
    #[serde(default = "default_col_key_mod")]
    pub key_mod:                 ColorRgb,
    /// Active modifier key and selected language button background.
    #[serde(default = "default_col_mod_active")]
    pub mod_active:              ColorRgb,
    /// Keyboard-navigation cursor highlight colour.
    #[serde(default = "default_col_nav_sel")]
    pub nav_sel:                 ColorRgb,
    /// Status bar background strip.
    #[serde(default = "default_col_status_bar_bg")]
    pub status_bar_bg:           ColorRgb,
    /// Inactive status indicator (modifier pill) background.
    #[serde(default = "default_col_status_ind_bg")]
    pub status_ind_bg:           ColorRgb,
    /// Inactive status indicator label colour.
    #[serde(default = "default_col_status_ind_text")]
    pub status_ind_text:         ColorRgb,
    /// Active status indicator label colour (modifier is on).
    #[serde(default = "default_col_status_ind_active_text")]
    pub status_ind_active_text:  ColorRgb,
    /// BLE / gamepad icon: disconnected state.
    #[serde(default = "default_col_conn_disconnected")]
    pub conn_disconnected:       ColorRgb,
    /// BLE icon: connecting state (dongle found, host not yet paired).
    #[serde(default = "default_col_conn_connecting")]
    pub conn_connecting:         ColorRgb,
    /// BLE / gamepad icon: connected state.
    #[serde(default = "default_col_conn_connected")]
    pub conn_connected:          ColorRgb,
    /// Window / keyboard area background.
    #[serde(default = "default_col_win_bg")]
    pub win_bg:                  ColorRgb,
    /// Text display background.
    #[serde(default = "default_col_disp_bg")]
    pub disp_bg:                 ColorRgb,
    /// Text display foreground (typed characters).
    #[serde(default = "default_col_disp_text")]
    pub disp_text:               ColorRgb,
    /// Language button background when not the active layout.
    #[serde(default = "default_col_lang_btn_inactive")]
    pub lang_btn_inactive:       ColorRgb,
    /// Language button label colour.
    #[serde(default = "default_col_lang_btn_label")]
    pub lang_btn_label:          ColorRgb,
    /// Text label colour on regular keys (dark text on light background).
    #[serde(default = "default_col_key_label_normal")]
    pub key_label_normal:        ColorRgb,
    /// Text label colour on modifier / function keys (light text on dark background).
    #[serde(default = "default_col_key_label_mod")]
    pub key_label_mod:           ColorRgb,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        ColorsConfig {
            key_normal:              default_col_key_normal(),
            key_mod:                 default_col_key_mod(),
            mod_active:              default_col_mod_active(),
            nav_sel:                 default_col_nav_sel(),
            status_bar_bg:           default_col_status_bar_bg(),
            status_ind_bg:           default_col_status_ind_bg(),
            status_ind_text:         default_col_status_ind_text(),
            status_ind_active_text:  default_col_status_ind_active_text(),
            conn_disconnected:       default_col_conn_disconnected(),
            conn_connecting:         default_col_conn_connecting(),
            conn_connected:          default_col_conn_connected(),
            win_bg:                  default_col_win_bg(),
            disp_bg:                 default_col_disp_bg(),
            disp_text:               default_col_disp_text(),
            lang_btn_inactive:       default_col_lang_btn_inactive(),
            lang_btn_label:          default_col_lang_btn_label(),
            key_label_normal:        default_col_key_label_normal(),
            key_label_mod:           default_col_key_label_mod(),
        }
    }
}

#[derive(Deserialize, Default)]
pub struct UiConfig {
    /// UI colour palette.
    #[serde(default)]
    pub colors: ColorsConfig,
}

#[derive(Deserialize)]
pub struct Config {
    pub input: InputConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

// =============================================================================
// Default configuration (mirrors config.toml)
// =============================================================================

impl Default for KeyboardInputConfig {
    fn default() -> Self {
        KeyboardInputConfig {
            navigate_up:    0x67,
            navigate_down:  0x6c,
            navigate_left:  0x69,
            navigate_right: 0x6a,
            activate:       0x39,
            menu:           0x32,
        }
    }
}

impl Default for GamepadInputConfig {
    fn default() -> Self {
        GamepadInputConfig {
            enabled:       true,
            device:        "auto".to_string(),
            navigate_up:    None,
            navigate_down:  None,
            navigate_left:  None,
            navigate_right: None,
            activate:       default_activate(),
            menu:           default_menu(),
            axis_navigate_horizontal: default_axis_navigate_horizontal(),
            axis_navigate_vertical:   default_axis_navigate_vertical(),
            axis_activate:            default_axis_activate(),
            axis_menu:                None,
            axis_threshold:           default_axis_threshold(),
            absolute_axes:            false,
            rumble:                   false,
            rumble_duration_ms:       default_rumble_duration_ms(),
            rumble_magnitude:         default_rumble_magnitude(),
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            keyboard: KeyboardInputConfig::default(),
            gamepad:  GamepadInputConfig::default(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            input:  InputConfig::default(),
            output: OutputConfig::default(),
            ui:     UiConfig::default(),
        }
    }
}

// =============================================================================
// Loading
// =============================================================================

impl Config {
    /// Load configuration from the path given by the `SMART_KBD_CONFIG_PATH`
    /// environment variable, or from `config.toml` in the current working
    /// directory if the variable is not set.
    /// Falls back silently to built-in defaults on any error.
    pub fn load() -> Self {
        let path = env::var("SMART_KBD_CONFIG_PATH")
            .unwrap_or_else(|_| "config.toml".into());
        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        toml::from_str(&content).unwrap_or_default()
    }
}

// =============================================================================
// evdev scan-code → FLTK Key conversion
// =============================================================================

/// Convert a Linux evdev scan code (linux/input-event-codes.h) to the
/// corresponding FLTK [`Key`] value used by [`fltk::app::event_key`].
///
/// Returns `None` for scan codes not covered by the table.
pub fn evdev_to_fltk_key(evdev: u32) -> Option<Key> {
    // FLTK uses X11 KeySym values on Linux.
    let fltk_bits: i32 = match evdev {
        0x01 => 0xff1b,       // KEY_ESC        → Key::Escape
        0x0e => 0xff08,       // KEY_BACKSPACE   → Key::BackSpace
        0x0f => 0xff09,       // KEY_TAB         → Key::Tab
        0x1c => 0xff0d,       // KEY_ENTER       → Key::Enter
        0x32 => 0x6d,         // KEY_M           → 'm' (ASCII)
        0x39 => 0x20,         // KEY_SPACE       → space (ASCII)
        0x67 => 0xff52,       // KEY_UP          → Key::Up
        0x6c => 0xff54,       // KEY_DOWN        → Key::Down
        0x69 => 0xff51,       // KEY_LEFT        → Key::Left
        0x6a => 0xff53,       // KEY_RIGHT       → Key::Right
        0x66 => 0xff50,       // KEY_HOME        → Key::Home
        0x6b => 0xff57,       // KEY_END         → Key::End
        0x68 => 0xff55,       // KEY_PAGEUP      → Key::PageUp
        0x6d => 0xff56,       // KEY_PAGEDOWN    → Key::PageDown
        0x6e => 0xff63,       // KEY_INSERT      → Key::Insert
        0x6f => 0xffff,       // KEY_DELETE      → Key::Delete
        0x3b => 0xffbe,       // KEY_F1          → Key::F1
        0x3c => 0xffbf,       // KEY_F2          → Key::F2
        0x3d => 0xffc0,       // KEY_F3          → Key::F3
        0x3e => 0xffc1,       // KEY_F4          → Key::F4
        0x3f => 0xffc2,       // KEY_F5          → Key::F5
        0x40 => 0xffc3,       // KEY_F6          → Key::F6
        0x41 => 0xffc4,       // KEY_F7          → Key::F7
        0x42 => 0xffc5,       // KEY_F8          → Key::F8
        0x43 => 0xffc6,       // KEY_F9          → Key::F9
        0x44 => 0xffc7,       // KEY_F10         → Key::F10
        0x57 => 0xffc8,       // KEY_F11         → Key::F11
        0x58 => 0xffc9,       // KEY_F12         → Key::F12
        _ => return None,
    };
    Some(Key::from_i32(fltk_bits))
}

// =============================================================================
// Resolved navigation keys (FLTK Key values)
// =============================================================================

/// FLTK [`Key`] values resolved from the keyboard section of the config.
#[derive(Clone, Copy)]
pub struct NavKeys {
    pub up:    Key,
    pub down:  Key,
    pub left:  Key,
    pub right: Key,
    pub activate: Key,
    pub menu: Key,
}

impl NavKeys {
    /// Build from the keyboard config, substituting defaults for any scan code
    /// that cannot be mapped to a FLTK Key.
    pub fn from_config(cfg: &KeyboardInputConfig) -> Self {
        NavKeys {
            up:       evdev_to_fltk_key(cfg.navigate_up)    .unwrap_or(Key::Up),
            down:     evdev_to_fltk_key(cfg.navigate_down)  .unwrap_or(Key::Down),
            left:     evdev_to_fltk_key(cfg.navigate_left)  .unwrap_or(Key::Left),
            right:    evdev_to_fltk_key(cfg.navigate_right) .unwrap_or(Key::Right),
            activate: evdev_to_fltk_key(cfg.activate)       .unwrap_or(Key::from_char(' ')),
            menu:     evdev_to_fltk_key(cfg.menu)           .unwrap_or(Key::from_char('m')),
        }
    }
}
