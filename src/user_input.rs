// src/user_input.rs
//
// Unified user input event types for all physical input sources (gamepad,
// GPIO buttons, physical keyboard).
//
// Each input source (gamepad.rs, gpio.rs, phys_keyboard.rs) translates its
// hardware-specific events into `UserInputEvent` values.  main.rs processes
// those events in a single, source-agnostic handler, which eliminates the
// per-source repetition of context-switching logic (virtual keyboard, mouse
// mode, menu).

// =============================================================================
// Public types
// =============================================================================

/// Abstract user interaction action, independent of which physical device
/// produced it.
///
/// The variants mirror the action sets of `GamepadAction` and `GpioAction`.
/// `AbsolutePos` is gamepad-specific but is included here so that a single
/// handler can cover all sources without a separate code path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UserInputAction {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Menu,
    /// Activate the current selection with Shift held.
    ActivateShift,
    /// Activate the current selection with Ctrl held.
    ActivateCtrl,
    /// Activate the current selection with Alt held.
    ActivateAlt,
    /// Activate the current selection with AltGr held.
    ActivateAltGr,
    /// Produce the Enter output directly (without navigating to Enter first).
    ActivateEnter,
    /// Produce the Space output directly.
    ActivateSpace,
    /// Produce the Left Arrow output directly.
    ActivateArrowLeft,
    /// Produce the Right Arrow output directly.
    ActivateArrowRight,
    /// Produce the Up Arrow output directly.
    ActivateArrowUp,
    /// Produce the Down Arrow output directly.
    ActivateArrowDown,
    /// Produce the Backspace output directly.
    ActivateBksp,
    /// Move the navigation cursor to the configured center key.
    NavigateCenter,
    /// Toggle mouse mode on/off.
    MouseToggle,
    /// Gamepad absolute-position event: joystick resting at a specific
    /// normalised position (`0.0` = minimum axis value, `1.0` = maximum).
    AbsolutePos { horiz: f32, vert: f32 },
}

/// A single user interaction event produced by any physical input source.
#[derive(Clone, Copy, Debug)]
pub struct UserInputEvent {
    pub action:  UserInputAction,
    /// `true` on button/key press, `false` on release.
    pub pressed: bool,
}
