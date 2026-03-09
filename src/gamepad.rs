// src/gamepad.rs
//
// Gamepad / joystick support.
//
// A background thread polls for gilrs events and forwards navigation events
// to the main FLTK thread via an `fltk::app::Sender`.  The main thread drains
// the channel inside a periodic timeout callback.

use fltk::app;
use gilrs::{Button, Event, EventType, Gilrs};

use crate::config::GamepadConfig;

// =============================================================================
// Navigation event type
// =============================================================================

/// A gamepad-originated navigation action sent to the main thread.
#[derive(Clone, Copy, PartialEq)]
pub enum GamepadNavEvent {
    Up,
    Down,
    Left,
    Right,
    Activate,
}

// =============================================================================
// Button name parsing
// =============================================================================

/// Convert a button name string (as used in `config.toml`) to a `gilrs::Button`.
///
/// Returns `None` for unknown names; a warning is printed to stderr.
pub fn parse_button(name: &str) -> Option<Button> {
    match name {
        "South" => Some(Button::South),
        "East" => Some(Button::East),
        "North" => Some(Button::North),
        "West" => Some(Button::West),
        "DpadUp" => Some(Button::DPadUp),
        "DpadDown" => Some(Button::DPadDown),
        "DpadLeft" => Some(Button::DPadLeft),
        "DpadRight" => Some(Button::DPadRight),
        "Select" => Some(Button::Select),
        "Start" => Some(Button::Start),
        "Mode" => Some(Button::Mode),
        "LeftTrigger" => Some(Button::LeftTrigger),
        "RightTrigger" => Some(Button::RightTrigger),
        "LeftThumb" => Some(Button::LeftThumb),
        "RightThumb" => Some(Button::RightThumb),
        other => {
            eprintln!("[gamepad] unknown button name {:?}; ignoring", other);
            None
        }
    }
}

// =============================================================================
// Background thread
// =============================================================================

/// Spawn a background thread that polls for gamepad events and forwards
/// navigation actions to `sender`.
///
/// Does nothing (returns immediately) when `config.enabled` is `false`.
///
/// After each event is sent the thread calls `app::awake()` so the FLTK
/// event loop wakes up and drains the channel promptly.
pub fn start(config: GamepadConfig, sender: app::Sender<GamepadNavEvent>) {
    if !config.enabled {
        return;
    }

    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[gamepad] failed to initialise gilrs: {}", e);
                return;
            }
        };

        // Pre-resolve button names so the hot loop does only comparisons.
        let btn_up = parse_button(&config.navigate_up);
        let btn_down = parse_button(&config.navigate_down);
        let btn_left = parse_button(&config.navigate_left);
        let btn_right = parse_button(&config.navigate_right);
        let btn_activate = parse_button(&config.activate);

        loop {
            // Drain all pending gilrs events.
            while let Some(Event { event, .. }) = gilrs.next_event() {
                if let EventType::ButtonPressed(button, _) = event {
                    let nav = if Some(button) == btn_up {
                        Some(GamepadNavEvent::Up)
                    } else if Some(button) == btn_down {
                        Some(GamepadNavEvent::Down)
                    } else if Some(button) == btn_left {
                        Some(GamepadNavEvent::Left)
                    } else if Some(button) == btn_right {
                        Some(GamepadNavEvent::Right)
                    } else if Some(button) == btn_activate {
                        Some(GamepadNavEvent::Activate)
                    } else {
                        None
                    };

                    if let Some(ev) = nav {
                        sender.send(ev);
                        app::awake();
                    }
                }
            }

            // ~60 Hz polling; gilrs has no blocking wait in its standard API.
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
}
