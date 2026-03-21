// src/phys_keyboard.rs
//
// Physical keyboard input: translates evdev scancodes into `UserInputEvent`
// values.  The iced application layer calls `physical_to_scancode` to convert
// `iced::keyboard::key::Physical` values into scancodes, then passes them to
// `translate_key_event` which maps them to the unified `UserInputEvent` type
// used by `process_input_events`.
//
// Only key-navigation events are handled here.

use iced::keyboard::key::{Code, NativeCode, Physical};

use crate::config::NavKeys;
use crate::user_input::{UserInputAction, UserInputEvent};

// =============================================================================
// Physical → evdev scancode conversion
// =============================================================================

/// Convert an `iced::keyboard::key::Physical` key identifier to an evdev
/// scancode (linux/input-event-codes.h numbering).
///
/// On Linux the `NativeCode::Xkb` variant carries an XKB keycode which is
/// the evdev scancode plus 8.  For `Physical::Code` variants a built-in
/// lookup table is used.
pub fn physical_to_scancode(phys: Physical) -> Option<u32> {
    match phys {
        Physical::Code(code) => keycode_to_scancode(code),
        Physical::Unidentified(native) => match native {
            NativeCode::Xkb(xkb) if xkb >= 8 => Some(xkb - 8),
            NativeCode::Xkb(_) => None,
            _ => None,
        },
    }
}

/// Map an `iced::keyboard::key::Code` variant to the corresponding evdev
/// scancode.  Returns `None` for codes that have no mapping in our table.
pub fn keycode_to_scancode(code: Code) -> Option<u32> {
    let sc = match code {
        Code::Escape       => 1,
        Code::Digit1       => 2,
        Code::Digit2       => 3,
        Code::Digit3       => 4,
        Code::Digit4       => 5,
        Code::Digit5       => 6,
        Code::Digit6       => 7,
        Code::Digit7       => 8,
        Code::Digit8       => 9,
        Code::Digit9       => 10,
        Code::Digit0       => 11,
        Code::Minus        => 12,
        Code::Equal        => 13,
        Code::Backspace    => 14,
        Code::Tab          => 15,
        Code::KeyQ         => 16,
        Code::KeyW         => 17,
        Code::KeyE         => 18,
        Code::KeyR         => 19,
        Code::KeyT         => 20,
        Code::KeyY         => 21,
        Code::KeyU         => 22,
        Code::KeyI         => 23,
        Code::KeyO         => 24,
        Code::KeyP         => 25,
        Code::BracketLeft  => 26,
        Code::BracketRight => 27,
        Code::Enter        => 28,
        Code::ControlLeft  => 29,
        Code::KeyA         => 30,
        Code::KeyS         => 31,
        Code::KeyD         => 32,
        Code::KeyF         => 33,
        Code::KeyG         => 34,
        Code::KeyH         => 35,
        Code::KeyJ         => 36,
        Code::KeyK         => 37,
        Code::KeyL         => 38,
        Code::Semicolon    => 39,
        Code::Quote        => 40,
        Code::Backquote    => 41,
        Code::ShiftLeft    => 42,
        Code::Backslash    => 43,
        Code::KeyZ         => 44,
        Code::KeyX         => 45,
        Code::KeyC         => 46,
        Code::KeyV         => 47,
        Code::KeyB         => 48,
        Code::KeyN         => 49,
        Code::KeyM         => 50,
        Code::Comma        => 51,
        Code::Period       => 52,
        Code::Slash        => 53,
        Code::ShiftRight   => 54,
        Code::AltLeft      => 56,
        Code::Space        => 57,
        Code::CapsLock     => 58,
        Code::F1           => 59,
        Code::F2           => 60,
        Code::F3           => 61,
        Code::F4           => 62,
        Code::F5           => 63,
        Code::F6           => 64,
        Code::F7           => 65,
        Code::F8           => 66,
        Code::F9           => 67,
        Code::F10          => 68,
        Code::NumLock      => 69,
        Code::ScrollLock   => 70,
        Code::F11          => 87,
        Code::F12          => 88,
        Code::ControlRight => 97,
        Code::AltRight     => 100,
        Code::Home         => 102,
        Code::ArrowUp      => 103,
        Code::PageUp       => 104,
        Code::ArrowLeft    => 105,
        Code::ArrowRight   => 106,
        Code::End          => 107,
        Code::ArrowDown    => 108,
        Code::PageDown     => 109,
        Code::Insert       => 110,
        Code::Delete       => 111,
        Code::SuperLeft    => 125,
        Code::SuperRight   => 126,
        _ => return None,
    };
    Some(sc)
}

// =============================================================================
// Key-event translation
// =============================================================================

/// Translate a single `(scancode, pressed)` pair to zero or more
/// `UserInputEvent` values.
///
/// Returns an empty `Vec` for scancodes that are not mapped to any navigation
/// action (they are simply ignored).  Returns exactly one element for every
/// recognised navigation key.
///
/// `pressed` should be `true` for key-down and `false` for key-up.  Only a
/// subset of actions produce a meaningful release event; the rest return an
/// empty vec on release so the caller can skip them.
pub fn translate_key_event(
    scancode: u32,
    pressed:  bool,
    nav_keys: &NavKeys,
) -> Vec<UserInputEvent> {
    let evt = |action: UserInputAction| vec![UserInputEvent { action, pressed }];

    if pressed {
        // -- Press events -------------------------------------------------
        if scancode == nav_keys.up    { return evt(UserInputAction::Up);    }
        if scancode == nav_keys.down  { return evt(UserInputAction::Down);  }
        if scancode == nav_keys.left  { return evt(UserInputAction::Left);  }
        if scancode == nav_keys.right { return evt(UserInputAction::Right); }

        if scancode == nav_keys.activate { return evt(UserInputAction::Activate); }
        if scancode == nav_keys.menu     { return evt(UserInputAction::Menu);     }

        if nav_keys.mouse_toggle   .map_or(false, |mk| scancode == mk) { return evt(UserInputAction::MouseToggle);    }
        if nav_keys.navigate_center.map_or(false, |nk| scancode == nk) { return evt(UserInputAction::NavigateCenter); }

        if nav_keys.activate_enter      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateEnter);      }
        if nav_keys.activate_space      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateSpace);      }
        if nav_keys.activate_arrow_left .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowLeft);  }
        if nav_keys.activate_arrow_right.map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowRight); }
        if nav_keys.activate_arrow_up   .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowUp);    }
        if nav_keys.activate_arrow_down .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowDown);  }
        if nav_keys.activate_bksp       .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateBksp);       }

        if nav_keys.activate_shift .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateShift); }
        if nav_keys.activate_ctrl  .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateCtrl);  }
        if nav_keys.activate_alt   .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateAlt);   }
        if nav_keys.activate_altgr .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateAltGr); }
    } else {
        // -- Release events -----------------------------------------------
        // Directional keys need a release event so that mouse-mode
        // auto-repeat knows when to stop moving the pointer.
        if scancode == nav_keys.up    { return evt(UserInputAction::Up);    }
        if scancode == nav_keys.down  { return evt(UserInputAction::Down);  }
        if scancode == nav_keys.left  { return evt(UserInputAction::Left);  }
        if scancode == nav_keys.right { return evt(UserInputAction::Right); }

        if scancode == nav_keys.activate {
            return evt(UserInputAction::Activate);
        }

        // Determine which activate variant (if any) so process_input_events
        // knows which action's release to handle.
        if nav_keys.activate_shift      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateShift);      }
        if nav_keys.activate_ctrl       .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateCtrl);       }
        if nav_keys.activate_alt        .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateAlt);        }
        if nav_keys.activate_altgr      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateAltGr);      }
        if nav_keys.activate_enter      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateEnter);      }
        if nav_keys.activate_space      .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateSpace);      }
        if nav_keys.activate_arrow_left .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowLeft);  }
        if nav_keys.activate_arrow_right.map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowRight); }
        if nav_keys.activate_arrow_up   .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowUp);    }
        if nav_keys.activate_arrow_down .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateArrowDown);  }
        if nav_keys.activate_bksp       .map_or(false, |ak| scancode == ak) { return evt(UserInputAction::ActivateBksp);       }
    }

    Vec::new()
}
