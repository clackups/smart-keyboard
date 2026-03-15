// src/config.rs
//
// Loads input and output configuration from a TOML file.  The directory is
// taken from the SMART_KBD_CONFIG_PATH environment variable; if unset, the
// current working directory is used.  The file name is always "config.toml".
// Falls back to built-in defaults if the file is missing or unparseable.

use std::env;
use std::fmt;
use std::fs;

use fltk::enums::Key;
use serde::Deserialize;

// =============================================================================
// Axis navigation configuration
// =============================================================================

/// Configuration for a single analog navigation axis.
///
/// Can be specified in `config.toml` as:
/// * A plain integer — just the axis index, transformation defaults to "normal".
/// * A two-element array `[axis_index, "normal" | "inverted"]`.
///
/// When `inverted` is `true` the sense of the axis is reversed:
///   * Negative axis values → Right (horizontal) or Down (vertical).
///   * Positive axis values → Left  (horizontal) or Up   (vertical).
#[derive(Clone)]
pub struct AxisConfig {
    pub axis:     u32,
    pub inverted: bool,
}

impl<'de> serde::Deserialize<'de> for AxisConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Vis;
        impl<'de> serde::de::Visitor<'de> for Vis {
            type Value = AxisConfig;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "an axis index (integer) or \
                     [axis_index, \"normal\"|\"inverted\"]",
                )
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<AxisConfig, E> {
                if v < 0 {
                    return Err(E::custom("axis index must be non-negative"));
                }
                Ok(AxisConfig { axis: v as u32, inverted: false })
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<AxisConfig, E> {
                Ok(AxisConfig { axis: v as u32, inverted: false })
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<AxisConfig, A::Error> {
                let axis: u32 = seq
                    .next_element::<u32>()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let transform: Option<String> = seq.next_element()?;
                let inverted = match transform.as_deref().unwrap_or("normal") {
                    "normal"   => false,
                    "inverted" => true,
                    other => {
                        return Err(serde::de::Error::unknown_variant(
                            other,
                            &["normal", "inverted"],
                        ))
                    }
                };
                Ok(AxisConfig { axis, inverted })
            }
        }
        deserializer.deserialize_any(Vis)
    }
}

// FLTK key code constants (X11 KeySym values) used as defaults.
// These match the values returned by `fltk::app::event_key().bits()`.
const FLTK_KEY_UP:    i32 = 0xff52;
const FLTK_KEY_DOWN:  i32 = 0xff54;
const FLTK_KEY_LEFT:  i32 = 0xff51;
const FLTK_KEY_RIGHT: i32 = 0xff53;
const FLTK_KEY_SPACE: i32 = 0x20;
const FLTK_KEY_M:     i32 = 0x6d;

// =============================================================================
// TOML-deserializable structs
// =============================================================================

#[derive(Deserialize)]
pub struct KeyboardInputConfig {
    /// FLTK key code for "navigate up" (as returned by `event_key().bits()`).
    /// Default: 0xff52 (Key::Up).
    pub navigate_up: u32,
    /// FLTK key code for "navigate down". Default: 0xff54 (Key::Down).
    pub navigate_down: u32,
    /// FLTK key code for "navigate left". Default: 0xff51 (Key::Left).
    pub navigate_left: u32,
    /// FLTK key code for "navigate right". Default: 0xff53 (Key::Right).
    pub navigate_right: u32,
    /// FLTK key code for "activate". Default: 0x20 (Space).
    pub activate: u32,
    /// FLTK key code for "menu". Default: 0x6d ('m').
    pub menu: u32,
    /// FLTK key code for "activate with Shift" (default: None / disabled).
    /// Equivalent to activate when Shift is held.
    #[serde(default)]
    pub activate_shift: Option<u32>,
    /// FLTK key code for "activate with Ctrl" (default: None / disabled).
    /// Equivalent to activate when Ctrl is held.
    #[serde(default)]
    pub activate_ctrl: Option<u32>,
    /// FLTK key code for "activate with Alt" (default: None / disabled).
    /// Equivalent to activate when Alt is held.
    #[serde(default)]
    pub activate_alt: Option<u32>,
    /// FLTK key code for "activate with AltGr" (default: None / disabled).
    /// Equivalent to activate when AltGr is held.
    #[serde(default)]
    pub activate_altgr: Option<u32>,
    /// FLTK key code for "activate Enter" (default: None / disabled).
    /// Produces the Enter output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_enter: Option<u32>,
    /// FLTK key code for "activate Space" (default: None / disabled).
    /// Produces the Space output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_space: Option<u32>,
    /// FLTK key code for "activate Arrow Left" (default: None / disabled).
    /// Produces the Left Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_left: Option<u32>,
    /// FLTK key code for "activate Arrow Right" (default: None / disabled).
    /// Produces the Right Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_right: Option<u32>,
    /// FLTK key code for "activate Arrow Up" (default: None / disabled).
    /// Produces the Up Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_up: Option<u32>,
    /// FLTK key code for "activate Arrow Down" (default: None / disabled).
    /// Produces the Down Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_down: Option<u32>,
    /// FLTK key code for "activate Backspace" (default: None / disabled).
    /// Produces the Backspace output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_bksp: Option<u32>,
    /// FLTK key code for "navigate center" (default: None / disabled).
    /// Moves the selection to the center of the keyboard.
    #[serde(default)]
    pub navigate_center: Option<u32>,
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
    /// Axis configuration used for left/right navigation.
    /// Negative axis values -> Left, positive -> Right (unless inverted).
    /// Absent / `null` means disabled.
    /// Default: axis 0 with "normal" transformation (left stick X on most gamepads).
    #[serde(default = "default_axis_navigate_horizontal")]
    pub axis_navigate_horizontal: Option<AxisConfig>,
    /// Axis configuration used for up/down navigation.
    /// Negative axis values -> Up, positive -> Down (unless inverted).
    /// Absent / `null` means disabled.
    /// Default: axis 1 with "normal" transformation (left stick Y on most gamepads).
    #[serde(default = "default_axis_navigate_vertical")]
    pub axis_navigate_vertical: Option<AxisConfig>,
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
    /// Minimum absolute axis value (0-32767) needed to register as active.
    /// Compared as `|value| > axis_threshold` against the raw i16 axis value.
    /// Default: 16384 (half of the maximum i16 range).
    #[serde(default = "default_axis_threshold")]
    pub axis_threshold: i32,
    /// When `true`, the raw values of `axis_navigate_horizontal` and
    /// `axis_navigate_vertical` are treated as absolute coordinates that map
    /// directly to a keyboard key position rather than as directional inputs.
    /// The full axis range (-32767 ... +32767) is divided evenly across the
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
    /// Button index for "activate with Shift"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_shift: Option<u32>,
    /// Button index for "activate with Ctrl"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_ctrl: Option<u32>,
    /// Button index for "activate with Alt"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_alt: Option<u32>,
    /// Button index for "activate with AltGr"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_altgr: Option<u32>,
    /// Button index for "activate Enter"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_enter: Option<u32>,
    /// Button index for "activate Space"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_space: Option<u32>,
    /// Button index for "activate Arrow Left"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_arrow_left: Option<u32>,
    /// Button index for "activate Arrow Right"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_arrow_right: Option<u32>,
    /// Button index for "activate Arrow Up"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_arrow_up: Option<u32>,
    /// Button index for "activate Arrow Down"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_arrow_down: Option<u32>,
    /// Button index for "activate Backspace"; absent / `null` means disabled.
    #[serde(default)]
    pub activate_bksp: Option<u32>,
    /// Button index for "navigate center"; absent / `null` means disabled.
    /// Moves the selection to the center of the keyboard.
    #[serde(default)]
    pub navigate_center: Option<u32>,
    /// Time in milliseconds that a directional input must be held before the
    /// first auto-repeat event fires.  Default: 300.
    #[serde(default = "default_repeat_delay_ms")]
    pub repeat_delay_ms: u64,
    /// Interval in milliseconds between successive auto-repeat events once
    /// repeating has started.  Default: 100.
    #[serde(default = "default_repeat_interval_ms")]
    pub repeat_interval_ms: u64,
}

fn default_activate()                 -> Option<u32>        { Some(0x05) }
fn default_menu()                     -> Option<u32>        { Some(0x08) }
fn default_axis_navigate_horizontal() -> Option<AxisConfig> { Some(AxisConfig { axis: 0, inverted: false }) }
fn default_axis_navigate_vertical()   -> Option<AxisConfig> { Some(AxisConfig { axis: 1, inverted: false }) }
fn default_axis_activate()            -> Option<u32>        { Some(0x05) }
fn default_axis_threshold()           -> i32  { 16384 }
fn default_rumble_duration_ms()       -> u16  { 50 }
fn default_rumble_magnitude()         -> u16  { 0x4000 }
fn default_repeat_delay_ms()          -> u64  { 300 }
fn default_repeat_interval_ms()       -> u64  { 100 }

/// Which signal level on a GPIO line means "active" (button pressed).
#[derive(Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GpioSignal {
    /// A high (1 / rising-edge) signal means the button is pressed.
    High,
    /// A low (0 / falling-edge) signal means the button is pressed (default).
    #[default]
    Low,
}

/// Internal pull-resistor configuration for GPIO input lines.
///
/// Requires Linux kernel 5.5 or newer (where the bias flags were added to the
/// GPIO v1 line-event ABI).  On older kernels the flags are ignored.
#[derive(Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GpioPull {
    /// Enable the internal pull-up resistor on the line.
    Up,
    /// Enable the internal pull-down resistor on the line.
    Down,
    /// No internal pull resistor (floating / disabled; default).
    #[default]
    Null,
}

fn default_gpio_chip() -> String { "/dev/gpiochip0".to_string() }

/// Configuration for the GPIO input source.
///
/// Each navigation / action key is mapped to a numeric GPIO line offset on the
/// configured chip device.  Setting a field to `null` (or omitting it) disables
/// that action.
#[derive(Deserialize, Clone)]
pub struct GpioInputConfig {
    /// Enable GPIO input.  Default: `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Path to the GPIO chip character device.  Default: `"/dev/gpiochip0"`.
    #[serde(default = "default_gpio_chip")]
    pub chip: String,
    /// GPIO line number for "navigate up"; `null` / absent = disabled.
    #[serde(default)]
    pub navigate_up: Option<u32>,
    /// GPIO line number for "navigate down"; `null` / absent = disabled.
    #[serde(default)]
    pub navigate_down: Option<u32>,
    /// GPIO line number for "navigate left"; `null` / absent = disabled.
    #[serde(default)]
    pub navigate_left: Option<u32>,
    /// GPIO line number for "navigate right"; `null` / absent = disabled.
    #[serde(default)]
    pub navigate_right: Option<u32>,
    /// GPIO line number for "activate"; `null` / absent = disabled.
    #[serde(default)]
    pub activate: Option<u32>,
    /// GPIO line number for "menu"; `null` / absent = disabled.
    #[serde(default)]
    pub menu: Option<u32>,
    /// GPIO line number for "activate with Shift"; `null` / absent = disabled.
    #[serde(default)]
    pub activate_shift: Option<u32>,
    /// GPIO line number for "activate with Ctrl"; `null` / absent = disabled.
    #[serde(default)]
    pub activate_ctrl: Option<u32>,
    /// GPIO line number for "activate with Alt"; `null` / absent = disabled.
    #[serde(default)]
    pub activate_alt: Option<u32>,
    /// GPIO line number for "activate with AltGr"; `null` / absent = disabled.
    #[serde(default)]
    pub activate_altgr: Option<u32>,
    /// GPIO line number for "activate Enter"; `null` / absent = disabled.
    /// Produces the Enter output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_enter: Option<u32>,
    /// GPIO line number for "activate Space"; `null` / absent = disabled.
    /// Produces the Space output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_space: Option<u32>,
    /// GPIO line number for "activate Arrow Left"; `null` / absent = disabled.
    /// Produces the Left Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_left: Option<u32>,
    /// GPIO line number for "activate Arrow Right"; `null` / absent = disabled.
    /// Produces the Right Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_right: Option<u32>,
    /// GPIO line number for "activate Arrow Up"; `null` / absent = disabled.
    /// Produces the Up Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_up: Option<u32>,
    /// GPIO line number for "activate Arrow Down"; `null` / absent = disabled.
    /// Produces the Down Arrow output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_arrow_down: Option<u32>,
    /// GPIO line number for "activate Backspace"; `null` / absent = disabled.
    /// Produces the Backspace output regardless of the current keyboard selection.
    #[serde(default)]
    pub activate_bksp: Option<u32>,
    /// GPIO line number for "navigate center"; `null` / absent = disabled.
    /// Moves the selection to the key configured by `[navigate] center_key`.
    #[serde(default)]
    pub navigate_center: Option<u32>,
    /// Which signal level on the GPIO line means "pressed".
    /// `"high"` = rising edge triggers press; `"low"` = falling edge (default).
    #[serde(default)]
    pub gpio_signal: GpioSignal,
    /// Internal pull-resistor configuration for all configured GPIO lines.
    /// `"up"` / `"down"` / `"null"` (default: `"null"` = no pull).
    #[serde(default)]
    pub gpio_pull: GpioPull,
    /// Time in milliseconds that a directional button must be held before the
    /// first auto-repeat event fires.  Default: 300.
    #[serde(default = "default_repeat_delay_ms")]
    pub repeat_delay_ms: u64,
    /// Interval in milliseconds between successive auto-repeat events once
    /// repeating has started.  Default: 100.
    #[serde(default = "default_repeat_interval_ms")]
    pub repeat_interval_ms: u64,
}

impl Default for GpioInputConfig {
    fn default() -> Self {
        GpioInputConfig {
            enabled:         false,
            chip:            default_gpio_chip(),
            navigate_up:     None,
            navigate_down:   None,
            navigate_left:   None,
            navigate_right:  None,
            activate:        None,
            menu:            None,
            activate_shift:  None,
            activate_ctrl:   None,
            activate_alt:    None,
            activate_altgr:  None,
            activate_enter:  None,
            activate_space:  None,
            activate_arrow_left:  None,
            activate_arrow_right: None,
            activate_arrow_up:    None,
            activate_arrow_down:  None,
            activate_bksp:        None,
            navigate_center: None,
            gpio_signal:     GpioSignal::Low,
            gpio_pull:       GpioPull::Null,
            repeat_delay_ms:    default_repeat_delay_ms(),
            repeat_interval_ms: default_repeat_interval_ms(),
        }
    }
}

#[derive(Deserialize)]
pub struct InputConfig {
    pub keyboard: KeyboardInputConfig,
    pub gamepad: GamepadInputConfig,
    #[serde(default)]
    pub gpio: GpioInputConfig,
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
#[serde(rename_all = "snake_case")]
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
    /// function keys (F1-F12) play a lower ascending scale; all other
    /// special keys have their own unique tones.
    Tone,
    /// Like `Tone`, but all letter and punctuation keys are silent except
    /// for F and J (the physical home-row bump keys), which play a
    /// distinctive tone.  Digit keys and all special keys (Space, Enter,
    /// arrows, ...) still play their tones as in `Tone` mode.
    ToneHint,
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
    /// Delay in microseconds between the key-press report and the key-release
    /// report (K0000).  Gives the remote host time to register the key press
    /// before the key is released.  Default: 20000 (20 ms).
    #[serde(default = "BleOutputConfig::default_key_release_delay")]
    pub key_release_delay: u32,
    /// Delay in microseconds between the language-switch key-press report and
    /// the key-release report (K0000) in on_lang_switch().  Language-switch
    /// combos (e.g. Ctrl+Shift+1) typically need a longer hold time than
    /// regular keys so the OS can register the shortcut.  Default: 200000
    /// (200 ms).
    #[serde(default = "BleOutputConfig::default_lang_switch_release_delay")]
    pub lang_switch_release_delay: u32,
}

impl BleOutputConfig {
    fn default_key_release_delay() -> u32 { 20000 }
    fn default_lang_switch_release_delay() -> u32 { 200000 }
}

impl Default for BleOutputConfig {
    fn default() -> Self {
        BleOutputConfig {
            vid:                        0x1209,
            pid:                        0xbbd1,
            serial:                     None,
            key_release_delay:          Self::default_key_release_delay(),
            lang_switch_release_delay:  Self::default_lang_switch_release_delay(),
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
    /// "none"       - silent (default)
    /// "narrate"    - play a WAV clip naming each button
    /// "tone"       - play a short musical tone that varies by key category
    /// "tone_hint"  - like "tone" but only F and J produce a tone; all other
    ///                letter/punctuation keys are silent
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

fn default_active_keymaps() -> Vec<String> {
    vec!["us".to_string(), "ua".to_string()]
}

/// Per-keymap configuration (switch scancode, etc.).
#[derive(Deserialize, Default, Clone)]
pub struct KeymapConfig {
    /// Switch scancode bytes: [modifier_byte, hid_keycode, ...].
    /// Sent to the output when the user switches to this keymap.
    /// If empty (default), nothing is sent.
    #[serde(default)]
    pub switch_scancode: Vec<u8>,
}

#[derive(Deserialize)]
pub struct UiConfig {
    /// UI colour palette.
    #[serde(default)]
    pub colors: ColorsConfig,
    /// When `true`, show the typed-text display at the top of the keyboard
    /// window.  When `false` (the default) the display is hidden and no CPU
    /// is spent updating the text buffer.
    #[serde(default)]
    pub show_text_display: bool,
    /// List of keymap names to display in the UI.
    /// Default: ["us", "ua"].
    #[serde(default = "default_active_keymaps")]
    pub active_keymaps: Vec<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            colors:            ColorsConfig::default(),
            show_text_display: false,
            active_keymaps:    default_active_keymaps(),
        }
    }
}

fn default_center_key() -> String { "h".to_string() }

/// Navigation behaviour configuration.
#[derive(Deserialize)]
pub struct NavigateConfig {
    /// When `true`, navigation wraps around at the edges of the keyboard.
    /// Moving past the last column brings the cursor to the first column, and
    /// vice-versa; moving past the last row brings the cursor to the first row
    /// (within the keyboard grid), and vice-versa.  Default: false.
    #[serde(default)]
    pub rollover: bool,
    /// Button label used as the center reference point.
    ///
    /// The `navigate_center` action moves the selection to this button.
    /// When `absolute_axes = true`, the joystick's neutral position (centre of
    /// the axis range) maps to this button rather than to the physical centre
    /// of the keyboard grid.  Default: `"h"`.
    #[serde(default = "default_center_key")]
    pub center_key: String,
    /// When `true`, the navigation selection jumps to the center button
    /// (defined by `center_key`) immediately after any activate action
    /// (including all `activate_*` variants).  Default: false.
    #[serde(default)]
    pub center_after_activate: bool,
}

impl Default for NavigateConfig {
    fn default() -> Self {
        NavigateConfig {
            rollover:             false,
            center_key:           default_center_key(),
            center_after_activate: false,
        }
    }
}

#[derive(Deserialize)]
pub struct Config {
    pub input: InputConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub navigate: NavigateConfig,
    /// Per-keymap configuration (switch scancode, etc.).
    #[serde(default)]
    pub keymap: std::collections::HashMap<String, KeymapConfig>,
}

// =============================================================================
// Default configuration (mirrors config.toml)
// =============================================================================

impl Default for KeyboardInputConfig {
    fn default() -> Self {
        KeyboardInputConfig {
            navigate_up:    FLTK_KEY_UP    as u32,
            navigate_down:  FLTK_KEY_DOWN  as u32,
            navigate_left:  FLTK_KEY_LEFT  as u32,
            navigate_right: FLTK_KEY_RIGHT as u32,
            activate:       FLTK_KEY_SPACE as u32,
            menu:           FLTK_KEY_M     as u32,
            activate_shift: None,
            activate_ctrl:  None,
            activate_alt:   None,
            activate_altgr: None,
            activate_enter: None,
            activate_space: None,
            activate_arrow_left:  None,
            activate_arrow_right: None,
            activate_arrow_up:    None,
            activate_arrow_down:  None,
            activate_bksp:        None,
            navigate_center: None,
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
            activate_shift:           None,
            activate_ctrl:            None,
            activate_alt:             None,
            activate_altgr:           None,
            activate_enter:           None,
            activate_space:           None,
            activate_arrow_left:      None,
            activate_arrow_right:     None,
            activate_arrow_up:        None,
            activate_arrow_down:      None,
            activate_bksp:            None,
            navigate_center:          None,
            repeat_delay_ms:          default_repeat_delay_ms(),
            repeat_interval_ms:       default_repeat_interval_ms(),
        }
    }
}
impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            keyboard: KeyboardInputConfig::default(),
            gamepad:  GamepadInputConfig::default(),
            gpio:     GpioInputConfig::default(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            input:    InputConfig::default(),
            output:   OutputConfig::default(),
            ui:       UiConfig::default(),
            navigate: NavigateConfig::default(),
            keymap:   std::collections::HashMap::new(),
        }
    }
}

// =============================================================================
// Loading
// =============================================================================

impl Config {
    /// Load configuration from `config.toml` inside the directory given by the
    /// `SMART_KBD_CONFIG_PATH` environment variable, or from `config.toml` in
    /// the current working directory if the variable is not set.
    /// Falls back silently to built-in defaults on any error.
    pub fn load() -> Self {
        let dir = env::var("SMART_KBD_CONFIG_PATH")
            .unwrap_or_else(|_| ".".into());
        let path = std::path::Path::new(&dir).join("config.toml");
        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        toml::from_str(&content).unwrap_or_default()
    }
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
    /// Key that activates the current selection with Shift held (None = disabled).
    pub activate_shift: Option<Key>,
    /// Key that activates the current selection with Ctrl held (None = disabled).
    pub activate_ctrl:  Option<Key>,
    /// Key that activates the current selection with Alt held (None = disabled).
    pub activate_alt:   Option<Key>,
    /// Key that activates the current selection with AltGr held (None = disabled).
    pub activate_altgr: Option<Key>,
    /// Key that produces the Enter output directly (None = disabled).
    pub activate_enter: Option<Key>,
    /// Key that produces the Space output directly (None = disabled).
    pub activate_space: Option<Key>,
    /// Key that produces the Left Arrow output directly (None = disabled).
    pub activate_arrow_left: Option<Key>,
    /// Key that produces the Right Arrow output directly (None = disabled).
    pub activate_arrow_right: Option<Key>,
    /// Key that produces the Up Arrow output directly (None = disabled).
    pub activate_arrow_up: Option<Key>,
    /// Key that produces the Down Arrow output directly (None = disabled).
    pub activate_arrow_down: Option<Key>,
    /// Key that produces the Backspace output directly (None = disabled).
    pub activate_bksp: Option<Key>,
    /// Key that moves the selection to the center of the keyboard (None = disabled).
    pub navigate_center: Option<Key>,
}

impl NavKeys {
    /// Build from the keyboard config.  Each field is a FLTK key code
    /// (the integer returned by `event_key().bits()`), stored directly in
    /// the config without any translation layer.
    pub fn from_config(cfg: &KeyboardInputConfig) -> Self {
        NavKeys {
            up:       Key::from_i32(cfg.navigate_up    as i32),
            down:     Key::from_i32(cfg.navigate_down  as i32),
            left:     Key::from_i32(cfg.navigate_left  as i32),
            right:    Key::from_i32(cfg.navigate_right as i32),
            activate: Key::from_i32(cfg.activate       as i32),
            menu:     Key::from_i32(cfg.menu           as i32),
            activate_shift:  cfg.activate_shift .map(|v| Key::from_i32(v as i32)),
            activate_ctrl:   cfg.activate_ctrl  .map(|v| Key::from_i32(v as i32)),
            activate_alt:    cfg.activate_alt   .map(|v| Key::from_i32(v as i32)),
            activate_altgr:  cfg.activate_altgr .map(|v| Key::from_i32(v as i32)),
            activate_enter:  cfg.activate_enter .map(|v| Key::from_i32(v as i32)),
            activate_space:  cfg.activate_space .map(|v| Key::from_i32(v as i32)),
            activate_arrow_left:  cfg.activate_arrow_left .map(|v| Key::from_i32(v as i32)),
            activate_arrow_right: cfg.activate_arrow_right.map(|v| Key::from_i32(v as i32)),
            activate_arrow_up:    cfg.activate_arrow_up   .map(|v| Key::from_i32(v as i32)),
            activate_arrow_down:  cfg.activate_arrow_down .map(|v| Key::from_i32(v as i32)),
            activate_bksp:        cfg.activate_bksp       .map(|v| Key::from_i32(v as i32)),
            navigate_center: cfg.navigate_center.map(|v| Key::from_i32(v as i32)),
        }
    }
}
