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
        // Directional keys need a release event so that mouse-mode
        // auto-repeat knows when to stop moving the pointer.
        if k == nav_keys.up    { return evt(UserInputAction::Up);    }
        if k == nav_keys.down  { return evt(UserInputAction::Down);  }
        if k == nav_keys.left  { return evt(UserInputAction::Left);  }
        if k == nav_keys.right { return evt(UserInputAction::Right); }

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
    // Persistent mouse-movement state, shared between the event handler and
    // the auto-repeat timer via Rc<RefCell<…>>.
    let mouse_state = std::rc::Rc::new(std::cell::RefCell::new(crate::MouseMoveState::new()));

    // Clone ctx for the auto-repeat timer before the event handler moves it.
    let ctx_timer = ctx.clone();

    // Extract activate keys for the timer closure (before nav_keys is moved
    // into the window handler closure).
    let activate_key = nav_keys.activate;
    let activate_shift_key = nav_keys.activate_shift;

    // false = our handler fires BEFORE FLTK routes the event to child widgets.
    win.super_handle_first(false);
    let mouse_ev = mouse_state.clone();
    win.handle(move |_w, ev| {
        let k = app::event_key();

        match ev {
            Event::KeyDown => {
                #[cfg(debug_assertions)]
                eprintln!("[keyboard] keydown=0x{:04x}", k.bits());

                // Suppress Escape so FLTK does not close the window.
                if k == Key::Escape {
                    return true;
                }

                // In mouse mode, sync the activate-key physical state
                // with the mouse-button bitmask before processing the
                // event.  FLTK sends keyboard events to the focused
                // button widget first; `handle_mouse_mode_key` in
                // display.rs sets the bitmask there.  However, when
                // the user presses a direction key while holding the
                // activate key, FLTK may deliver a spurious activate
                // key-up (from X11 auto-repeat filtering) that clears
                // the bitmask.  Re-syncing from `event_key_down` —
                // which reflects the true physical key state — restores
                // the correct bitmask so the very first movement report
                // includes the mouse button for drag operations.
                if *ctx.mouse_mode.borrow() {
                    let mut mb = ctx.mouse_buttons.borrow_mut();
                    if app::event_key_down(nav_keys.activate) {
                        *mb |= 0x01;
                    }
                    if nav_keys.activate_shift.map_or(false, |ask| app::event_key_down(ask)) {
                        *mb |= 0x02;
                    }
                }

                let events = translate_key_event(k, true, &nav_keys);
                if events.is_empty() {
                    return false;
                }

                // Physical keyboard has no rumble; pass rumble = false.
                process_input_events(&events, &mut ctx, &mut mouse_ev.borrow_mut(), false);
                true
            }

            Event::KeyUp => {
                #[cfg(debug_assertions)]
                eprintln!("[keyboard] keyup=0x{:04x}", k.bits());

                let events = translate_key_event(k, false, &nav_keys);
                if events.is_empty() {
                    return false;
                }

                // Physical keyboard has no rumble; pass rumble = false.
                process_input_events(&events, &mut ctx, &mut mouse_ev.borrow_mut(), false);
                true
            }

            _ => false,
        }
    });

    // Auto-repeat timer for mouse-mode movement (same 60 Hz rate as
    // gamepad / GPIO sources).
    let mouse_timer = mouse_state;
    app::add_timeout(0.016, move |handle| {
        // Sync the physical keyboard's activate-key state with the mouse
        // button bitmask.  FLTK's event dispatch sends Space/Enter to the
        // focused button widget *before* the window handler, so the
        // window-level handler never sees those keys.  The button-level
        // `handle_mouse_mode_key` sets the bitmask, but when the user
        // presses an arrow key while Space is held, FLTK may (on X11)
        // deliver a spurious Space key-up as part of its auto-repeat
        // filtering.  Checking `event_key_down` — which queries the
        // physical key state tracked by FLTK's internal key vector —
        // ensures the bitmask stays correct for drag operations.
        if *ctx_timer.mouse_mode.borrow() {
            let mut mb = ctx_timer.mouse_buttons.borrow_mut();
            if app::event_key_down(activate_key) {
                *mb |= 0x01;
            } else {
                *mb &= !0x01;
            }
            if let Some(ask) = activate_shift_key {
                if app::event_key_down(ask) {
                    *mb |= 0x02;
                } else {
                    *mb &= !0x02;
                }
            }
        }
        crate::mouse_auto_repeat(&ctx_timer, &mut mouse_timer.borrow_mut());
        app::repeat_timeout(0.016, handle);
    });
}
