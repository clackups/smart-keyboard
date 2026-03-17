// src/phys_keyboard.rs
//
// Physical keyboard input: translates FLTK key events into `UserInputEvent`
// values and wires up the window-level event handler that calls
// `process_input_events` -- the same unified dispatcher used by the gamepad
// and GPIO input sources.
//
// Only key-navigation events are handled here.  Direct key-press callbacks on
// individual on-screen buttons (Push / Released events) stay in display.rs
// because they are tightly coupled to the widget layout.

use fltk::{app, enums::{Event, Key}, prelude::*};

use crate::config::NavKeys;
use crate::user_input::{UserInputAction, UserInputEvent};
use crate::{InputCtx, process_input_events};

// =============================================================================
// Key-event translation
// =============================================================================

/// Translate a single FLTK `(key, pressed)` pair to zero or more
/// `UserInputEvent` values.
///
/// Returns an empty `Vec` for keys that are not mapped to any navigation
/// action (they are simply ignored).  Returns exactly one element for every
/// recognised navigation key.
///
/// `pressed` should be `true` for `Event::KeyDown` and `false` for
/// `Event::KeyUp`.  Only a subset of actions produce a meaningful release
/// event; the rest return an empty vec on release so the caller can skip them.
pub fn translate_key_event(
    k:        Key,
    pressed:  bool,
    nav_keys: &NavKeys,
) -> Vec<UserInputEvent> {
    let evt = |action: UserInputAction| vec![UserInputEvent { action, pressed }];

    if pressed {
        // -- Press events -------------------------------------------------
        if k == nav_keys.up    { return evt(UserInputAction::Up);    }
        if k == nav_keys.down  { return evt(UserInputAction::Down);  }
        if k == nav_keys.left  { return evt(UserInputAction::Left);  }
        if k == nav_keys.right { return evt(UserInputAction::Right); }

        if k == nav_keys.activate { return evt(UserInputAction::Activate); }
        if k == nav_keys.menu     { return evt(UserInputAction::Menu);     }

        if nav_keys.mouse_toggle   .map_or(false, |mk| k == mk) { return evt(UserInputAction::MouseToggle);    }
        if nav_keys.navigate_center.map_or(false, |nk| k == nk) { return evt(UserInputAction::NavigateCenter); }

        if nav_keys.activate_enter      .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateEnter);      }
        if nav_keys.activate_space      .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateSpace);      }
        if nav_keys.activate_arrow_left .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowLeft);  }
        if nav_keys.activate_arrow_right.map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowRight); }
        if nav_keys.activate_arrow_up   .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowUp);    }
        if nav_keys.activate_arrow_down .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowDown);  }
        if nav_keys.activate_bksp       .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateBksp);       }

        if nav_keys.activate_shift .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateShift); }
        if nav_keys.activate_ctrl  .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateCtrl);  }
        if nav_keys.activate_alt   .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateAlt);   }
        if nav_keys.activate_altgr .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateAltGr); }
    } else {
        // -- Release events -----------------------------------------------
        // Only activate-style keys produce a release event; directional and
        // toggle keys do not hold state that needs an explicit release.
        if k == nav_keys.activate {
            return evt(UserInputAction::Activate);
        }
        let is_activate_variant =
            nav_keys.activate_shift      .map_or(false, |ak| k == ak)
            || nav_keys.activate_ctrl    .map_or(false, |ak| k == ak)
            || nav_keys.activate_alt     .map_or(false, |ak| k == ak)
            || nav_keys.activate_altgr   .map_or(false, |ak| k == ak)
            || nav_keys.activate_enter   .map_or(false, |ak| k == ak)
            || nav_keys.activate_space   .map_or(false, |ak| k == ak)
            || nav_keys.activate_arrow_left .map_or(false, |ak| k == ak)
            || nav_keys.activate_arrow_right.map_or(false, |ak| k == ak)
            || nav_keys.activate_arrow_up   .map_or(false, |ak| k == ak)
            || nav_keys.activate_arrow_down .map_or(false, |ak| k == ak)
            || nav_keys.activate_bksp       .map_or(false, |ak| k == ak);
        if is_activate_variant {
            // Determine which variant so process_input_events knows which
            // action's release to handle.
            if nav_keys.activate_shift .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateShift); }
            if nav_keys.activate_ctrl  .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateCtrl);  }
            if nav_keys.activate_alt   .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateAlt);   }
            if nav_keys.activate_altgr .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateAltGr); }
            if nav_keys.activate_enter .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateEnter); }
            if nav_keys.activate_space .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateSpace); }
            if nav_keys.activate_arrow_left .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowLeft); }
            if nav_keys.activate_arrow_right.map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowRight); }
            if nav_keys.activate_arrow_up   .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowUp);   }
            if nav_keys.activate_arrow_down .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateArrowDown); }
            if nav_keys.activate_bksp       .map_or(false, |ak| k == ak) { return evt(UserInputAction::ActivateBksp);      }
        }
    }

    Vec::new()
}

// =============================================================================
// Window event handler setup
// =============================================================================

/// Register the window-level FLTK event handler for physical-keyboard
/// navigation.
///
/// `win.super_handle_first(false)` causes our handler to run BEFORE FLTK
/// routes events to any child widget, so arrow keys and the activate key are
/// intercepted regardless of which button currently holds FLTK keyboard focus.
///
/// This must be called after [`crate::display::build_ui`] returns.  The
/// handler shares state with the gamepad/GPIO handlers through the `Rc<RefCell<...>>`
/// fields inside `ctx`.
pub fn setup_keyboard_handler(
    win:      &mut fltk::window::Window,
    nav_keys: NavKeys,
    mut ctx:  InputCtx,
) {
    // false = our handler fires BEFORE FLTK routes the event to child widgets.
    win.super_handle_first(false);
    win.handle(move |_w, ev| {
        let k = app::event_key();

        match ev {
            Event::KeyDown => {
                #[cfg(debug_assertions)]
                eprintln!("[keyboard] keydown=0x{:04x}", k.bits());

                // Suppress Escape so FLTK does not close the window.
                if k == Key::Escape {
                    // If the menu is open, close it.
                    // If the config editor is open, close it.
                    // Otherwise just consume.
                    if ctx.menu_sel.borrow().is_some() {
                        *ctx.menu_sel.borrow_mut() = None;
                        ctx.menu_group.hide();
                        app::redraw();
                    } else if *ctx.config_editor_open.borrow() {
                        *ctx.config_editor_open.borrow_mut() = false;
                        ctx.config_editor_group.hide();
                        app::redraw();
                    }
                    return true;
                }

                let events = translate_key_event(k, true, &nav_keys);
                if events.is_empty() {
                    return false;
                }

                // Physical keyboard has no rumble; pass rumble = false.
                process_input_events(&events, &mut ctx, &mut crate::MouseMoveState::new(), false);
                true
            }

            Event::KeyUp => {
                #[cfg(debug_assertions)]
                eprintln!("[keyboard] keyup=0x{:04x}", k.bits());

                // Consume all key-up events while menu is open.
                if ctx.menu_sel.borrow().is_some() {
                    return true;
                }

                let events = translate_key_event(k, false, &nav_keys);
                if events.is_empty() {
                    return false;
                }

                // Physical keyboard has no rumble; pass rumble = false.
                process_input_events(&events, &mut ctx, &mut crate::MouseMoveState::new(), false);
                true
            }

            Event::Push => {
                // Block mouse clicks that land outside the menu overlay when
                // the menu is open, so on-screen keyboard buttons in those
                // areas cannot fire accidentally.
                if ctx.menu_sel.borrow().is_some() {
                    let ex = app::event_x();
                    let ey = app::event_y();
                    let mx = ctx.menu_group.x();
                    let my = ctx.menu_group.y();
                    let mw = ctx.menu_group.w();
                    let mh = ctx.menu_group.h();
                    if ex < mx || ex >= mx + mw || ey < my || ey >= my + mh {
                        return true; // absorb the click
                    }
                }
                // The config editor overlay covers the full screen below the
                // status bar, so clicks always land on editor widgets.  We
                // consume key-up events but allow FLTK to route Push events
                // naturally so the TextEditor and the buttons stay clickable.
                false
            }

            _ => false,
        }
    });
}
