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
// Physical key-state helpers for mouse mode
// =============================================================================

/// Synchronise the `mouse_buttons` bitmask with the physical state of the
/// activate keys.
///
/// `app::event_key_down(k)` queries the low-level key-state bitmap
/// (`fl_key_vector` on X11 / `key_vector` on Wayland) which is updated as
/// soon as the underlying event is received — *before* FLTK's widget event
/// dispatch runs.  This makes it reliable even when a focused button widget
/// consumes the Space / Enter KeyDown (via `handle_mouse_mode_key` in
/// display.rs) before the window handler sees it.
fn sync_mouse_buttons(ctx: &InputCtx, nav: &NavKeys) {
    let mut mb = ctx.mouse_buttons.borrow_mut();
    // Left mouse button: activate key.
    if app::event_key_down(nav.activate) { *mb |= 0x01; } else { *mb &= !0x01; }
    // Right mouse button: activate-shift key.
    if let Some(ak) = nav.activate_shift {
        if app::event_key_down(ak) { *mb |= 0x02; } else { *mb &= !0x02; }
    }
}

/// Synchronise the `MouseMoveState` direction with the physical state of the
/// direction keys.
///
/// If any direction key is currently held (according to the low-level
/// key-state bitmap), the corresponding movement axis is started; when no
/// direction keys are held any more, movement stops.  This catches
/// direction-key events that may be lost in FLTK's widget dispatch chain
/// (e.g. when the focused button widget or an intermediate Group consumes
/// the FL_KEYBOARD event before the window handler sees it).
fn sync_mouse_direction(
    ctx:   &InputCtx,
    nav:   &NavKeys,
    mouse: &mut crate::MouseMoveState,
) {
    let right = app::event_key_down(nav.right);
    let left  = app::event_key_down(nav.left);
    let down  = app::event_key_down(nav.down);
    let up    = app::event_key_down(nav.up);

    let new_dx: i8 = if right && !left { 1 } else if left && !right { -1 } else { 0 };
    let new_dy: i8 = if down && !up { 1 } else if up && !down { -1 } else { 0 };

    if new_dx != mouse.dx || new_dy != mouse.dy {
        mouse.dx = new_dx;
        mouse.dy = new_dy;

        if new_dx != 0 || new_dy != 0 {
            // A new direction appeared — kick the auto-repeat timing.
            let now = std::time::Instant::now();
            if mouse.start.is_none() {
                mouse.start = Some(now);
            }
            let interval = std::time::Duration::from_millis(ctx.mouse_cfg.repeat_interval);
            mouse.next = Some(now + interval);
        } else {
            mouse.start = None;
            mouse.next  = None;
        }
    }
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

                // In mouse mode, sync the activate-key physical state into
                // the mouse_buttons bitmask.  FLTK keyboard dispatch sends
                // events to the focused button widget *first*; when the
                // button's `handle_mouse_mode_key` consumes the Space /
                // Enter event, this window handler never sees it.  Between
                // auto-repeat KeyRelease / KeyDown pairs the bitmask may
                // briefly toggle to 0, causing a direction-key report to be
                // sent without the button bit.  Querying the physical state
                // via `event_key_down` ensures the bitmask is accurate
                // regardless of dispatch ordering.
                if *ctx.mouse_mode.borrow() {
                    sync_mouse_buttons(&ctx, &nav_keys);
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

                if *ctx.mouse_mode.borrow() {
                    sync_mouse_buttons(&ctx, &nav_keys);
                }

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
    //
    // In mouse mode the timer also polls the physical key state via
    // `event_key_down` to keep the direction state and `mouse_buttons`
    // bitmask in sync.  This overcomes FLTK dispatch quirks where the
    // focused button widget may consume activate-key events (Space /
    // Enter) before the window handler sees them, and where direction-key
    // events may be lost during auto-repeat interleaving.
    let mouse_timer = mouse_state;
    let timer_nav = nav_keys;
    app::add_timeout(0.016, move |handle| {
        if *ctx_timer.mouse_mode.borrow() {
            // Always sync direction from the physical key state; this is
            // per-source state (keyboard's own MouseMoveState) so it cannot
            // interfere with gamepad / GPIO.
            sync_mouse_direction(&ctx_timer, &timer_nav, &mut mouse_timer.borrow_mut());

            // Sync activate-key → mouse_buttons only while the keyboard is
            // actively producing mouse movement.  This avoids clobbering
            // button state that a different input source (gamepad / GPIO)
            // wrote to the shared bitmask.
            {
                let m = mouse_timer.borrow();
                if m.dx != 0 || m.dy != 0 {
                    drop(m);
                    sync_mouse_buttons(&ctx_timer, &timer_nav);
                }
            }
        }
        crate::mouse_auto_repeat(&ctx_timer, &mut mouse_timer.borrow_mut());
        app::repeat_timeout(0.016, handle);
    });
}
