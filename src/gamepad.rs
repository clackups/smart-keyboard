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
/// js_event.type flag: synthetic init event (sent on open, not real input).
const JS_EVENT_INIT:   u8 = 0x80;

/// `O_NONBLOCK` flag from libc: do not block on read when no data is available.
const O_NONBLOCK: i32 = libc::O_NONBLOCK;

// =============================================================================
// Gamepad
// =============================================================================

/// Non-blocking reader for a Linux joystick device (`/dev/input/js*`).
pub struct Gamepad {
    file:           File,
    navigate_up:    u32,
    navigate_down:  u32,
    navigate_left:  u32,
    navigate_right: u32,
    activate:       u32,
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

                    // We only care about button events.
                    if event_type & JS_EVENT_BUTTON == 0 {
                        continue;
                    }

                    let pressed = value != 0;
                    #[cfg(debug_assertions)]
                    eprintln!("[gamepad] button=0x{:02x} pressed={}", number, pressed);

                    if let Some(action) = self.map_button(number) {
                        out.push(GamepadEvent { action, pressed });
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
