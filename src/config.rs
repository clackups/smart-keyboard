// src/config.rs
//
// Loads and exposes the application configuration from `config.toml`.
// All fields have sensible defaults so the file is entirely optional.

use serde::Deserialize;

// =============================================================================
// Top-level configuration
// =============================================================================

#[derive(Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub input: InputConfig,
}

// =============================================================================
// Input configuration
// =============================================================================

#[derive(Deserialize, Default)]
pub struct InputConfig {
    #[serde(default)]
    pub keyboard: KeyboardConfig,
    #[serde(default)]
    pub gamepad: GamepadConfig,
}

// -----------------------------------------------------------------------------
// Keyboard navigation
// -----------------------------------------------------------------------------

/// Maps logical navigation actions to physical key names.
///
/// Key name strings are matched in `main.rs` against FLTK `Key` values.
/// Supported names: "Up", "Down", "Left", "Right", "Space", "Escape" / "Esc",
/// "Enter", "Tab", and any single ASCII character (e.g. "w", "s", "a", "d").
#[derive(Deserialize)]
pub struct KeyboardConfig {
    /// Key that moves the selection cursor up.
    #[serde(default = "default_navigate_up")]
    pub navigate_up: String,
    /// Key that moves the selection cursor down.
    #[serde(default = "default_navigate_down")]
    pub navigate_down: String,
    /// Key that moves the selection cursor left.
    #[serde(default = "default_navigate_left")]
    pub navigate_left: String,
    /// Key that moves the selection cursor right.
    #[serde(default = "default_navigate_right")]
    pub navigate_right: String,
    /// Key that fires (activates) the currently highlighted button.
    #[serde(default = "default_activate")]
    pub activate: String,
    /// Key that is suppressed so FLTK does not close the window.
    #[serde(default = "default_escape")]
    pub escape: String,
}

fn default_navigate_up() -> String {
    "Up".to_string()
}
fn default_navigate_down() -> String {
    "Down".to_string()
}
fn default_navigate_left() -> String {
    "Left".to_string()
}
fn default_navigate_right() -> String {
    "Right".to_string()
}
fn default_activate() -> String {
    "Space".to_string()
}
fn default_escape() -> String {
    "Escape".to_string()
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            navigate_up: default_navigate_up(),
            navigate_down: default_navigate_down(),
            navigate_left: default_navigate_left(),
            navigate_right: default_navigate_right(),
            activate: default_activate(),
            escape: default_escape(),
        }
    }
}

// -----------------------------------------------------------------------------
// Gamepad / joystick
// -----------------------------------------------------------------------------

/// Maps logical navigation actions to gamepad button names.
///
/// Button name strings are matched in `gamepad.rs` against `gilrs::Button`
/// values.  Supported names: "South", "East", "North", "West", "DpadUp",
/// "DpadDown", "DpadLeft", "DpadRight", "Select", "Start", "LeftTrigger",
/// "RightTrigger", "LeftThumb", "RightThumb".
#[derive(Deserialize, Clone)]
pub struct GamepadConfig {
    /// Enable gamepad navigation.  Defaults to `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Device path or `"auto"` to accept the first available gamepad.
    #[serde(default = "default_device")]
    pub device: String,
    /// Button that moves the selection cursor up.
    #[serde(default = "default_gp_navigate_up")]
    pub navigate_up: String,
    /// Button that moves the selection cursor down.
    #[serde(default = "default_gp_navigate_down")]
    pub navigate_down: String,
    /// Button that moves the selection cursor left.
    #[serde(default = "default_gp_navigate_left")]
    pub navigate_left: String,
    /// Button that moves the selection cursor right.
    #[serde(default = "default_gp_navigate_right")]
    pub navigate_right: String,
    /// Button that fires (activates) the currently highlighted button.
    /// Default: "South" (Xbox A / PlayStation ✕).
    #[serde(default = "default_gp_activate")]
    pub activate: String,
}

fn default_device() -> String {
    "auto".to_string()
}
fn default_gp_navigate_up() -> String {
    "DpadUp".to_string()
}
fn default_gp_navigate_down() -> String {
    "DpadDown".to_string()
}
fn default_gp_navigate_left() -> String {
    "DpadLeft".to_string()
}
fn default_gp_navigate_right() -> String {
    "DpadRight".to_string()
}
fn default_gp_activate() -> String {
    "South".to_string()
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device: default_device(),
            navigate_up: default_gp_navigate_up(),
            navigate_down: default_gp_navigate_down(),
            navigate_left: default_gp_navigate_left(),
            navigate_right: default_gp_navigate_right(),
            activate: default_gp_activate(),
        }
    }
}

// =============================================================================
// Loading
// =============================================================================

impl Config {
    /// Load configuration from `config.toml` next to the executable.
    ///
    /// If the file does not exist the function silently returns defaults.
    /// If the file exists but cannot be parsed, a warning is printed to
    /// stderr and defaults are returned.
    pub fn load() -> Self {
        // Look for config.toml next to the executable first, then in the
        // current working directory as a fallback.
        let candidates: &[&str] = &["config.toml"];
        for path in candidates {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str::<Config>(&content) {
                    Ok(cfg) => return cfg,
                    Err(e) => {
                        eprintln!("[config] failed to parse {}: {}", path, e);
                        return Config::default();
                    }
                },
                Err(_) => continue,
            }
        }
        Config::default()
    }
}
