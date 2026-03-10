// src/config.rs
//
// Loads input configuration from config.toml (Linux evdev scan codes in hex).
// Falls back to built-in defaults if the file is missing or unparseable.

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
    /// Linux evdev scan code for "navigate down" (default: 0x6C KEY_DOWN).
    pub navigate_down: u32,
    /// Linux evdev scan code for "navigate left" (default: 0x69 KEY_LEFT).
    pub navigate_left: u32,
    /// Linux evdev scan code for "navigate right" (default: 0x6A KEY_RIGHT).
    pub navigate_right: u32,
    /// Linux evdev scan code for "activate" (default: 0x39 KEY_SPACE).
    pub activate: u32,
}

#[derive(Deserialize)]
pub struct GamepadInputConfig {
    pub enabled: bool,
    pub device: String,
    pub navigate_up: u32,
    pub navigate_down: u32,
    pub navigate_left: u32,
    pub navigate_right: u32,
    pub activate: u32,
}

#[derive(Deserialize)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad: GamepadInputConfig,
}

#[derive(Deserialize)]
pub struct Config {
    pub input: InputConfig,
}

// =============================================================================
// Default configuration (mirrors config.toml)
// =============================================================================

impl Default for KeyboardInputConfig {
    fn default() -> Self {
        KeyboardInputConfig {
            navigate_up:    0x67,
            navigate_down:  0x6C,
            navigate_left:  0x69,
            navigate_right: 0x6A,
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
        Config { input: InputConfig::default() }
    }
}

// =============================================================================
// Loading
// =============================================================================

impl Config {
    /// Load configuration from `config.toml` in the current working directory.
    /// Falls back silently to built-in defaults on any error.
    pub fn load() -> Self {
        let content = match fs::read_to_string("config.toml") {
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
