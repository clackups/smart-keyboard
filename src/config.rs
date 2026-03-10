// src/config.rs
//
// Application configuration loaded from config.toml.
// Input codes use Linux evdev scan codes (linux/input-event-codes.h).

// =============================================================================
// Keyboard input configuration
// =============================================================================

/// Keyboard navigation and activation scan codes (Linux evdev).
#[derive(Debug, Clone)]
pub struct KeyboardInputConfig {
    /// Evdev scan code that moves the on-screen selection up.
    pub navigate_up: u32,
    /// Evdev scan code that moves the on-screen selection down.
    pub navigate_down: u32,
    /// Evdev scan code that moves the on-screen selection left.
    pub navigate_left: u32,
    /// Evdev scan code that moves the on-screen selection right.
    pub navigate_right: u32,
    /// Evdev scan code that activates (fires) the currently selected button.
    pub activate: u32,
}

impl Default for KeyboardInputConfig {
    fn default() -> Self {
        Self {
            navigate_up:    0x67, // KEY_UP
            navigate_down:  0x6C, // KEY_DOWN
            navigate_left:  0x69, // KEY_LEFT
            navigate_right: 0x6A, // KEY_RIGHT
            activate:       0x39, // KEY_SPACE
        }
    }
}

// =============================================================================
// Gamepad input configuration
// =============================================================================

/// Gamepad navigation and activation button codes.
#[derive(Debug, Clone)]
pub struct GamepadInputConfig {
    /// Whether gamepad input is enabled.
    pub enabled: bool,
    /// Device path or "auto" to select the first detected gamepad.
    pub device: String,
    /// Button code that moves the on-screen selection up.
    pub navigate_up: u32,
    /// Button code that moves the on-screen selection down.
    pub navigate_down: u32,
    /// Button code that moves the on-screen selection left.
    pub navigate_left: u32,
    /// Button code that moves the on-screen selection right.
    pub navigate_right: u32,
    /// Button code that activates (fires) the currently selected button.
    pub activate: u32,
}

impl Default for GamepadInputConfig {
    fn default() -> Self {
        Self {
            enabled:        true,
            device:         "auto".to_string(),
            navigate_up:    0x00,
            navigate_down:  0x01,
            navigate_left:  0x02,
            navigate_right: 0x03,
            activate:       0x04, // A/South button
        }
    }
}

// =============================================================================
// Top-level config
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad:  GamepadInputConfig,
}

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub input: InputConfig,
}

// =============================================================================
// Parsing helpers
// =============================================================================

/// Extract an integer value (supporting hex literals) from a TOML value.
fn parse_u32(val: &toml::Value) -> Option<u32> {
    val.as_integer().map(|i| i as u32)
}

/// Extract a boolean from a TOML value.
fn parse_bool(val: &toml::Value) -> Option<bool> {
    val.as_bool()
}

/// Extract a string from a TOML value.
fn parse_str(val: &toml::Value) -> Option<String> {
    val.as_str().map(str::to_string)
}

// =============================================================================
// Loading
// =============================================================================

impl AppConfig {
    /// Load configuration from `config.toml` in the current working directory.
    /// Falls back to compiled-in defaults if the file is absent or malformed.
    pub fn load() -> Self {
        Self::load_from_path("config.toml")
    }

    /// Load configuration from the given file path.
    /// Falls back to defaults on any error.
    pub fn load_from_path(path: &str) -> Self {
        let mut cfg = Self::default();

        let content = match std::fs::read_to_string(path) {
            Ok(s)  => s,
            Err(_) => return cfg, // File not found – use defaults silently.
        };

        let table: toml::Value = match content.parse() {
            Ok(v)  => v,
            Err(e) => {
                eprintln!("Warning: failed to parse {}: {}", path, e);
                return cfg;
            }
        };

        let Some(input) = table.get("input") else { return cfg; };

        // --- [input.keyboard] ---
        if let Some(kb) = input.get("keyboard") {
            if let Some(v) = kb.get("navigate_up").and_then(parse_u32) {
                cfg.input.keyboard.navigate_up = v;
            }
            if let Some(v) = kb.get("navigate_down").and_then(parse_u32) {
                cfg.input.keyboard.navigate_down = v;
            }
            if let Some(v) = kb.get("navigate_left").and_then(parse_u32) {
                cfg.input.keyboard.navigate_left = v;
            }
            if let Some(v) = kb.get("navigate_right").and_then(parse_u32) {
                cfg.input.keyboard.navigate_right = v;
            }
            if let Some(v) = kb.get("activate").and_then(parse_u32) {
                cfg.input.keyboard.activate = v;
            }
        }

        // --- [input.gamepad] ---
        if let Some(gp) = input.get("gamepad") {
            if let Some(v) = gp.get("enabled").and_then(parse_bool) {
                cfg.input.gamepad.enabled = v;
            }
            if let Some(v) = gp.get("device").and_then(parse_str) {
                cfg.input.gamepad.device = v;
            }
            if let Some(v) = gp.get("navigate_up").and_then(parse_u32) {
                cfg.input.gamepad.navigate_up = v;
            }
            if let Some(v) = gp.get("navigate_down").and_then(parse_u32) {
                cfg.input.gamepad.navigate_down = v;
            }
            if let Some(v) = gp.get("navigate_left").and_then(parse_u32) {
                cfg.input.gamepad.navigate_left = v;
            }
            if let Some(v) = gp.get("navigate_right").and_then(parse_u32) {
                cfg.input.gamepad.navigate_right = v;
            }
            if let Some(v) = gp.get("activate").and_then(parse_u32) {
                cfg.input.gamepad.activate = v;
            }
        }

        cfg
    }
}
