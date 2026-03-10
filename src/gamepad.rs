// src/gamepad.rs
//
// Non-blocking gamepad event polling using the Linux joystick API
// (/dev/input/js*).  Each call to `Gamepad::poll` drains all pending events
// and returns a list of `GamepadEvent` values without blocking.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use crate::config::GamepadInputConfig;

/// Maximum number of joystick device indices to probe during auto-detection.
const MAX_JOYSTICK_DEVICES: u8 = 8;

// =============================================================================
// Public types
// =============================================================================

/// A navigation action produced by a gamepad button press or release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GamepadAction {
    Up,
    Down,
    Left,
    Right,
    Activate,
}

/// A single gamepad button event.
#[derive(Clone, Copy, Debug)]
pub struct GamepadEvent {
    pub action:  GamepadAction,
    pub pressed: bool,
}

// =============================================================================
// Linux joystick API constants
// =============================================================================

/// js_event.type: button event.
const JS_EVENT_BUTTON: u8 = 0x01;
/// js_event.type: axis event.
const JS_EVENT_AXIS:   u8 = 0x02;
/// js_event.type flag: synthetic init event (sent on open, not real input).
const JS_EVENT_INIT:   u8 = 0x80;

/// `O_NONBLOCK` flag from libc: do not block on read when no data is available.
const O_NONBLOCK: i32 = libc::O_NONBLOCK;

// =============================================================================
// Gamepad
// =============================================================================

/// Active direction on an analog axis (above the deadzone threshold).
#[derive(Clone, Copy, Debug, PartialEq)]
enum AxisDir { Negative, Positive }

/// Non-blocking reader for a Linux joystick device (`/dev/input/js*`).
pub struct Gamepad {
    file:           File,
    navigate_up:    u32,
    navigate_down:  u32,
    navigate_left:  u32,
    navigate_right: u32,
    activate:       u32,
    // Axis configuration
    axis_horizontal: u32,         // axis index for left/right
    axis_vertical:   u32,         // axis index for up/down
    axis_activate:   Option<u32>, // axis index for activate (None = disabled)
    axis_threshold:  i32,         // minimum |value| to register as active
    // Axis state (tracks previous active direction to generate press/release)
    horiz_dir:   Option<AxisDir>,
    vert_dir:    Option<AxisDir>,
    act_active:  bool,
}

impl Gamepad {
    /// Open the configured gamepad device.
    ///
    /// If `cfg.device == "auto"` the first available `/dev/input/jsN` (N = 0–7)
    /// is used.  Returns `None` if no device can be opened.
    pub fn open(cfg: &GamepadInputConfig) -> Option<Self> {
        let path = if cfg.device == "auto" {
            find_first_joystick()?
        } else {
            PathBuf::from(&cfg.device)
        };

        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(&path)
            .ok()?;

        eprintln!("[gamepad] opened {:?}", path);

        Some(Gamepad {
            file,
            navigate_up:    cfg.navigate_up,
            navigate_down:  cfg.navigate_down,
            navigate_left:  cfg.navigate_left,
            navigate_right: cfg.navigate_right,
            activate:       cfg.activate,
            axis_horizontal: cfg.axis_navigate_horizontal,
            axis_vertical:   cfg.axis_navigate_vertical,
            axis_activate:   cfg.axis_activate,
            axis_threshold:  cfg.axis_threshold,
            horiz_dir:       None,
            vert_dir:        None,
            act_active:      false,
        })
    }

    /// Drain all pending joystick events into `out` without blocking.
    ///
    /// `out` is cleared before filling so the caller can reuse the same
    /// allocation across calls.
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) {
        out.clear();
        // js_event is exactly 8 bytes (little-endian):
        //   [0..4]  u32  time    – milliseconds since driver start
        //   [4..6]  i16  value   – axis/button value
        //   [6]     u8   type    – JS_EVENT_BUTTON | JS_EVENT_AXIS | JS_EVENT_INIT
        //   [7]     u8   number  – button/axis index
        let mut buf = [0u8; 8];
        loop {
            match self.file.read(&mut buf) {
                Ok(8) => {
                    let event_type = buf[6];
                    let number     = buf[7] as u32;
                    let value      = i16::from_le_bytes([buf[4], buf[5]]);

                    // Discard synthetic init events replayed on open.
                    if event_type & JS_EVENT_INIT != 0 {
                        continue;
                    }

                    if event_type & JS_EVENT_BUTTON != 0 {
                        let pressed = value != 0;
                        #[cfg(debug_assertions)]
                        eprintln!("[gamepad] button=0x{:02x} pressed={}", number, pressed);

                        if let Some(action) = self.map_button(number) {
                            out.push(GamepadEvent { action, pressed });
                        }
                    } else if event_type & JS_EVENT_AXIS != 0 {
                        #[cfg(debug_assertions)]
                        eprintln!("[gamepad] axis=0x{:02x} value={}", number, value);

                        self.handle_axis(number, value, out);
                    }
                }
                // Partial read – should not occur with 8-byte structs, skip.
                Ok(_) => break,
                // No more events available right now.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // Any other error (device disconnected, etc.) – stop polling.
                Err(_) => break,
            }
        }
    }

    /// Map a raw button index to a `GamepadAction`, or `None` if unconfigured.
    fn map_button(&self, code: u32) -> Option<GamepadAction> {
        if code == self.navigate_up    { return Some(GamepadAction::Up);       }
        if code == self.navigate_down  { return Some(GamepadAction::Down);     }
        if code == self.navigate_left  { return Some(GamepadAction::Left);     }
        if code == self.navigate_right { return Some(GamepadAction::Right);    }
        if code == self.activate       { return Some(GamepadAction::Activate); }
        None
    }

    /// Process a single axis event, emitting press/release `GamepadEvent`s into
    /// `out` whenever the axis crosses the configured threshold.
    ///
    /// Each configured axis has a remembered active direction (`horiz_dir`,
    /// `vert_dir`, `act_active`).  A transition from neutral → active emits a
    /// *press* event; active → neutral emits a *release* event.
    fn handle_axis(&mut self, axis: u32, value: i16, out: &mut Vec<GamepadEvent>) {
        let v = value as i32;

        if axis == self.axis_horizontal {
            let new_dir = axis_dir(v, self.axis_threshold);
            if new_dir != self.horiz_dir {
                // Release previous direction.
                if let Some(prev) = self.horiz_dir {
                    let action = match prev {
                        AxisDir::Negative => GamepadAction::Left,
                        AxisDir::Positive => GamepadAction::Right,
                    };
                    out.push(GamepadEvent { action, pressed: false });
                }
                // Press new direction.
                if let Some(next) = new_dir {
                    let action = match next {
                        AxisDir::Negative => GamepadAction::Left,
                        AxisDir::Positive => GamepadAction::Right,
                    };
                    out.push(GamepadEvent { action, pressed: true });
                }
                self.horiz_dir = new_dir;
            }
        } else if axis == self.axis_vertical {
            let new_dir = axis_dir(v, self.axis_threshold);
            if new_dir != self.vert_dir {
                // Release previous direction.
                if let Some(prev) = self.vert_dir {
                    let action = match prev {
                        AxisDir::Negative => GamepadAction::Up,
                        AxisDir::Positive => GamepadAction::Down,
                    };
                    out.push(GamepadEvent { action, pressed: false });
                }
                // Press new direction.
                if let Some(next) = new_dir {
                    let action = match next {
                        AxisDir::Negative => GamepadAction::Up,
                        AxisDir::Positive => GamepadAction::Down,
                    };
                    out.push(GamepadEvent { action, pressed: true });
                }
                self.vert_dir = new_dir;
            }
        } else if self.axis_activate == Some(axis) {
            // Activate uses positive values only; this matches the physical
            // behaviour of analog triggers (range 0 → +32767).
            let active = v > self.axis_threshold;
            if active != self.act_active {
                out.push(GamepadEvent {
                    action:  GamepadAction::Activate,
                    pressed: active,
                });
                self.act_active = active;
            }
        }
    }
}

// =============================================================================
// Device discovery
// =============================================================================

/// Return the path of the first available `/dev/input/jsN` (N = 0–MAX-1), or
/// `None` if no joystick device is present.
fn find_first_joystick() -> Option<PathBuf> {
    for i in 0..MAX_JOYSTICK_DEVICES {
        let path = PathBuf::from(format!("/dev/input/js{}", i));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

// =============================================================================
// Axis helpers
// =============================================================================

/// Map a raw axis value to an [`AxisDir`] based on `threshold`.
///
/// Returns `Positive` when `value > threshold`, `Negative` when
/// `value < -threshold`, and `None` when the value is within the deadzone.
/// `threshold` must be in the range 0–32767.
fn axis_dir(value: i32, threshold: i32) -> Option<AxisDir> {
    if value > threshold       { Some(AxisDir::Positive) }
    else if value < -threshold { Some(AxisDir::Negative) }
    else                       { None }
}
