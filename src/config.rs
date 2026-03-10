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
}

#[derive(Deserialize, Clone)]
pub struct GamepadInputConfig {
    pub enabled: bool,
    pub device: String,
    pub navigate_up: u32,
    pub navigate_down: u32,
    pub navigate_left: u32,
    pub navigate_right: u32,
    pub activate: u32,
    /// Axis index used for left/right navigation.
    /// Negative axis values → Left, positive → Right.
    /// Default: 0 (left stick X on most gamepads).
    #[serde(default = "default_axis_navigate_horizontal")]
    pub axis_navigate_horizontal: u32,
    /// Axis index used for up/down navigation.
    /// Negative axis values → Up, positive → Down.
    /// Default: 1 (left stick Y on most gamepads).
    #[serde(default = "default_axis_navigate_vertical")]
    pub axis_navigate_vertical: u32,
    /// Optional axis index for the activate action.
    /// Positive axis values above `axis_threshold` trigger Activate.
    /// Absent / `null` means disabled (default).
    #[serde(default)]
    pub axis_activate: Option<u32>,
    /// Minimum absolute axis value (0–32767) needed to register as active.
    /// Compared as `|value| > axis_threshold` against the raw i16 axis value.
    /// Default: 16384 (half of the maximum i16 range).
    #[serde(default = "default_axis_threshold")]
    pub axis_threshold: i32,
}

fn default_axis_navigate_horizontal() -> u32 { 0 }
fn default_axis_navigate_vertical()   -> u32 { 1 }
fn default_axis_threshold()           -> i32 { 16384 }

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
}

#[derive(Deserialize)]
pub struct Config {
    pub input: InputConfig,
    #[serde(default)]
    pub output: OutputConfig,
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
        }
    }
}

impl Default for GamepadInputConfig {
    fn default() -> Self {
        GamepadInputConfig {
            enabled:       true,
            device:        "auto".to_string(),
            navigate_up:    0x00,
            navigate_down:  0x01,
            navigate_left:  0x02,
            navigate_right: 0x03,
            activate:       0x04,
            axis_navigate_horizontal: default_axis_navigate_horizontal(),
            axis_navigate_vertical:   default_axis_navigate_vertical(),
            axis_activate:            None,
            axis_threshold:           default_axis_threshold(),
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
        }
    }
}
