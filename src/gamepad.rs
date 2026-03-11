// src/gamepad.rs
//
// Non-blocking gamepad event polling using the Linux joystick API
// (/dev/input/js*).  Each call to `Gamepad::poll` drains all pending events
// and returns a list of `GamepadEvent` values without blocking.
//
// Force-feedback (rumble) is driven through the evdev event interface
// (/dev/input/event*), which is separate from the joystick read interface.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::GamepadInputConfig;

/// Maximum number of joystick device indices to probe during auto-detection.
const MAX_JOYSTICK_DEVICES: u8 = 8;

/// Time a directional axis must be held before the first repeat event fires.
const REPEAT_DELAY: Duration = Duration::from_millis(300);

/// Interval between successive repeat events once repeating has begun.
const REPEAT_INTERVAL: Duration = Duration::from_millis(100);

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
    Menu,
    /// Emitted in `absolute_axes` mode: the joystick is at a position that maps
    /// to a specific key.  `horiz` and `vert` are normalised to `0.0` (min axis
    /// value) … `1.0` (max axis value).
    AbsolutePos { horiz: f32, vert: f32 },
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
// Linux force-feedback types (used for rumble)
// =============================================================================

/// Trigger conditions for a force-feedback effect (both fields unused for
/// simple one-shot rumble; set to 0).
#[repr(C)]
#[derive(Clone, Copy)]
struct FfTrigger {
    button:   u16,
    interval: u16,
}

/// Playback timing for a force-feedback effect.
#[repr(C)]
#[derive(Clone, Copy)]
struct FfReplay {
    /// Duration of the effect in milliseconds.
    length: u16,
    /// Delay before the effect starts in milliseconds.
    delay:  u16,
}

/// Rumble (dual-motor) force-feedback effect parameters.
#[repr(C)]
#[derive(Clone, Copy)]
struct FfRumbleEffect {
    strong_magnitude: u16,
    weak_magnitude:   u16,
}

/// Union holding the type-specific parameters of a force-feedback effect.
///
/// The `align(8)` ensures that on 64-bit Linux the union starts at the same
/// offset (16) as the C union inside `struct ff_effect`, matching the kernel's
/// layout (the largest C union member, `ff_periodic_effect`, contains a
/// pointer and therefore has 8-byte alignment).
#[repr(C, align(8))]
union FfEffectUnion {
    rumble: FfRumbleEffect,
    /// Padding to match the size of the largest C union member (32 bytes on
    /// 64-bit Linux, occupied by `struct ff_periodic_effect`).
    _pad: [u8; 32],
}

/// `struct ff_effect` from `<linux/input.h>`.
///
/// Layout on 64-bit Linux:
/// - offset  0: type (2), id (2), direction (2), trigger (4), replay (4)
/// - offset 14: 2 bytes implicit padding (aligns the union to 8 bytes)
/// - offset 16: union (32 bytes)
/// - total: 48 bytes
#[repr(C)]
struct FfEffect {
    effect_type: u16,
    /// Effect ID; set to -1 when uploading so the kernel assigns a new ID.
    id:          i16,
    direction:   u16,
    trigger:     FfTrigger,
    replay:      FfReplay,
    // 2 bytes implicit padding inserted by the compiler to align `u` to 8.
    u: FfEffectUnion,
}

// Compile-time check: FfEffect must be exactly 48 bytes on 64-bit Linux.
// If this assertion fails the struct layout does not match the kernel ABI.
const _: () = assert!(std::mem::size_of::<FfEffect>() == 48);

/// `struct input_event` from `<linux/input.h>` (64-bit layout: 24 bytes).
#[repr(C)]
struct InputEvent {
    /// Timestamp (ignored when writing); 16 bytes (`struct timeval` on 64-bit).
    time:    [u8; 16],
    ev_type: u16,
    code:    u16,
    value:   i32,
}

/// Force-feedback rumble effect type (`FF_RUMBLE`).
const FF_RUMBLE: u16 = 0x50;

/// EV_FF event type (used to play / stop uploaded effects).
const EV_FF: u16 = 0x15;

/// `EVIOCSFF` ioctl: upload / update a force-feedback effect.
///
/// Computed as `_IOW('E', 0x80, struct ff_effect)`:
///   (IOC_WRITE=1 << 30) | ('E' << 8) | 0x80 | (sizeof(ff_effect) << 16)
///   = 0x40000000 | 0x4500 | 0x0080 | (48 << 16)
///   = 0x40304580
const EVIOCSFF: libc::c_ulong =
    (1u64 << 30) as libc::c_ulong
    | ((b'E' as u64) << 8) as libc::c_ulong
    | 0x80
    | ((std::mem::size_of::<FfEffect>() as u64) << 16) as libc::c_ulong;

// Validate that the EVIOCSFF constant matches the expected value.
const _: () = assert!(EVIOCSFF == 0x40304580);

// =============================================================================
// Gamepad
// =============================================================================

/// Active direction on an analog axis (above the deadzone threshold).
#[derive(Clone, Copy, Debug, PartialEq)]
enum AxisDir { Negative, Positive }

/// Non-blocking reader for a Linux joystick device (`/dev/input/js*`).
pub struct Gamepad {
    file:           File,
    navigate_up:    Option<u32>,
    navigate_down:  Option<u32>,
    navigate_left:  Option<u32>,
    navigate_right: Option<u32>,
    activate:       Option<u32>,
    menu:           Option<u32>,
    // Axis configuration
    axis_horizontal: Option<u32>,         // axis index for left/right (None = disabled)
    axis_vertical:   Option<u32>,         // axis index for up/down (None = disabled)
    axis_activate:   Option<u32>,         // axis index for activate (None = disabled)
    axis_menu:       Option<u32>,         // axis index for menu (None = disabled)
    axis_threshold:  i32,                 // minimum |value| to register as active
    // Axis state (tracks previous active direction to generate press/release)
    horiz_dir:   Option<AxisDir>,
    vert_dir:    Option<AxisDir>,
    act_active:  bool,
    menu_active: bool,
    // Repeat timers: `Some(t)` means "fire next repeat event at time t".
    horiz_repeat_at: Option<Instant>,
    vert_repeat_at:  Option<Instant>,
    // Absolute-axes mode
    axis_absolute:  bool,    // true when absolute_axes = true in config
    abs_horiz_raw:  i16,     // last raw horizontal axis value (absolute mode)
    abs_vert_raw:   i16,     // last raw vertical axis value (absolute mode)
    // Force-feedback (rumble)
    rumble_file:      Option<File>,  // event device opened for writing FF events
    rumble_effect_id: i16,           // ID assigned by the kernel for the rumble effect
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

        // Optionally open the corresponding evdev event device for force feedback.
        let (rumble_file, rumble_effect_id) = if cfg.rumble {
            match open_rumble(&path, cfg.rumble_duration_ms, cfg.rumble_magnitude) {
                Some((f, id)) => (Some(f), id),
                None => {
                    eprintln!("[gamepad] rumble requested but not available on {:?}", path);
                    (None, -1)
                }
            }
        } else {
            (None, -1)
        };

        Some(Gamepad {
            file,
            navigate_up:    cfg.navigate_up,
            navigate_down:  cfg.navigate_down,
            navigate_left:  cfg.navigate_left,
            navigate_right: cfg.navigate_right,
            activate:       cfg.activate,
            menu:           cfg.menu,
            axis_horizontal: cfg.axis_navigate_horizontal,
            axis_vertical:   cfg.axis_navigate_vertical,
            axis_activate:   cfg.axis_activate,
            axis_menu:       cfg.axis_menu,
            axis_threshold:  cfg.axis_threshold,
            horiz_dir:       None,
            vert_dir:        None,
            act_active:      false,
            menu_active:     false,
            horiz_repeat_at: None,
            vert_repeat_at:  None,
            axis_absolute:   cfg.absolute_axes,
            abs_horiz_raw:   0,
            abs_vert_raw:    0,
            rumble_file,
            rumble_effect_id,
        })
    }

    /// Play a short rumble on the gamepad, if force-feedback is available.
    ///
    /// Does nothing when the device was opened without rumble support or when
    /// the effect upload failed at open time.
    pub fn rumble(&mut self) {
        let Some(ref rumble_file) = self.rumble_file else { return };
        if self.rumble_effect_id < 0 { return; }

        let ev = InputEvent {
            time:    [0u8; 16],
            ev_type: EV_FF,
            code:    self.rumble_effect_id as u16,
            value:   1, // play once
        };
        // SAFETY: `InputEvent` is `repr(C)` with no padding; the byte slice
        // covers exactly the bytes that would be written to the kernel.
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                &ev as *const InputEvent as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };
        use std::io::Write;
        if let Err(e) = (&*rumble_file).write_all(bytes) {
            eprintln!("[gamepad] rumble: write failed: {}", e);
        }
    }

    /// Drain all pending joystick events into `out` without blocking.
    ///
    /// `out` is cleared before filling so the caller can reuse the same
    /// allocation across calls.
    ///
    /// Returns `true` if the device is still connected, `false` if a device
    /// error was encountered (e.g. the gamepad was unplugged).
    pub fn poll(&mut self, out: &mut Vec<GamepadEvent>) -> bool {
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
                        if !self.axis_absolute && (value.abs() as i32) > self.axis_threshold {
                            eprintln!("[gamepad] axis=0x{:02x} value={}", number, value);
                        }

                        self.handle_axis(number, value, out);
                    }
                }
                // Partial read – should not occur with 8-byte structs, skip.
                Ok(_) => break,
                // No more events available right now.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // Any other error (device disconnected, etc.) – signal disconnection.
                Err(_) => return false,
            }
        }

        // Emit repeat press events for any directional axis that is still held.
        // (Not applicable in absolute_axes mode.)
        if !self.axis_absolute {
            let now = Instant::now();
            if let Some(t) = self.horiz_repeat_at {
                if now >= t {
                    if let Some(dir) = self.horiz_dir {
                        let action = match dir {
                            AxisDir::Negative => GamepadAction::Left,
                            AxisDir::Positive => GamepadAction::Right,
                        };
                        out.push(GamepadEvent { action, pressed: true });
                        self.horiz_repeat_at = Some(now + REPEAT_INTERVAL);
                    } else {
                        self.horiz_repeat_at = None;
                    }
                }
            }
            if let Some(t) = self.vert_repeat_at {
                if now >= t {
                    if let Some(dir) = self.vert_dir {
                        let action = match dir {
                            AxisDir::Negative => GamepadAction::Up,
                            AxisDir::Positive => GamepadAction::Down,
                        };
                        out.push(GamepadEvent { action, pressed: true });
                        self.vert_repeat_at = Some(now + REPEAT_INTERVAL);
                    } else {
                        self.vert_repeat_at = None;
                    }
                }
            }
        }

        true
    }

    /// Map a raw button index to a `GamepadAction`, or `None` if unconfigured.
    fn map_button(&self, code: u32) -> Option<GamepadAction> {
        if self.navigate_up    == Some(code) { return Some(GamepadAction::Up);       }
        if self.navigate_down  == Some(code) { return Some(GamepadAction::Down);     }
        if self.navigate_left  == Some(code) { return Some(GamepadAction::Left);     }
        if self.navigate_right == Some(code) { return Some(GamepadAction::Right);    }
        if self.activate       == Some(code) { return Some(GamepadAction::Activate); }
        if self.menu           == Some(code) { return Some(GamepadAction::Menu);     }
        None
    }

    /// Process a single axis event, emitting press/release `GamepadEvent`s into
    /// `out` whenever the axis crosses the configured threshold.
    ///
    /// Each configured axis has a remembered active direction (`horiz_dir`,
    /// `vert_dir`, `act_active`).  A transition from neutral → active emits a
    /// *press* event; active → neutral emits a *release* event.
    ///
    /// When `axis_absolute` is `true` the horizontal and vertical axes instead
    /// emit a [`GamepadAction::AbsolutePos`] event whose `horiz` / `vert`
    /// values are normalised to `0.0 … 1.0`.  The event is only emitted when
    /// the raw axis value changes, so that `main` can update the selection only
    /// on actual movement.
    fn handle_axis(&mut self, axis: u32, value: i16, out: &mut Vec<GamepadEvent>) {
        let v = value as i32;

        if self.axis_absolute {
            // --- Absolute-position mode ---
            if self.axis_horizontal == Some(axis) {
                if value != self.abs_horiz_raw {
                    self.abs_horiz_raw = value;
                    out.push(GamepadEvent {
                        action: GamepadAction::AbsolutePos {
                            horiz: raw_to_frac(self.abs_horiz_raw),
                            vert:  raw_to_frac(self.abs_vert_raw),
                        },
                        pressed: false,
                    });
                }
            } else if self.axis_vertical == Some(axis) {
                if value != self.abs_vert_raw {
                    self.abs_vert_raw = value;
                    out.push(GamepadEvent {
                        action: GamepadAction::AbsolutePos {
                            horiz: raw_to_frac(self.abs_horiz_raw),
                            vert:  raw_to_frac(self.abs_vert_raw),
                        },
                        pressed: false,
                    });
                }
            } else if self.axis_activate == Some(axis) {
                let active = v > self.axis_threshold;
                if active != self.act_active {
                    out.push(GamepadEvent {
                        action:  GamepadAction::Activate,
                        pressed: active,
                    });
                    self.act_active = active;
                }
            } else if self.axis_menu == Some(axis) {
                let active = v > self.axis_threshold;
                if active != self.menu_active {
                    out.push(GamepadEvent {
                        action:  GamepadAction::Menu,
                        pressed: active,
                    });
                    self.menu_active = active;
                }
            }
            return;
        }

        // --- Relative (threshold) mode ---
        if self.axis_horizontal == Some(axis) {
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
                self.horiz_repeat_at = if new_dir.is_some() {
                    Some(Instant::now() + REPEAT_DELAY)
                } else {
                    None
                };
            }
        } else if self.axis_vertical == Some(axis) {
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
                self.vert_repeat_at = if new_dir.is_some() {
                    Some(Instant::now() + REPEAT_DELAY)
                } else {
                    None
                };
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
        } else if self.axis_menu == Some(axis) {
            // Menu uses positive values only.
            let active = v > self.axis_threshold;
            if active != self.menu_active {
                out.push(GamepadEvent {
                    action:  GamepadAction::Menu,
                    pressed: active,
                });
                self.menu_active = active;
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

/// Find the `/dev/input/eventN` device that corresponds to a joystick path
/// such as `/dev/input/js0` by inspecting the sysfs class tree.
///
/// Returns `None` if no matching event device is found.
fn find_event_device_for_joystick(js_path: &std::path::Path) -> Option<PathBuf> {
    let js_name = js_path.file_name()?.to_str()?;
    // Enumerate /sys/class/input/<jsN>/device/ looking for eventN children.
    let sysfs_device = format!("/sys/class/input/{}/device", js_name);
    for entry in std::fs::read_dir(&sysfs_device).ok()?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("event") {
            let event_path = PathBuf::from(format!("/dev/input/{}", name_str));
            if event_path.exists() {
                return Some(event_path);
            }
        }
    }
    None
}

/// Open the evdev event device corresponding to `js_path` for force-feedback
/// and upload a rumble effect with the given duration and magnitude.
///
/// Returns `Some((file, effect_id))` on success, or `None` when the event
/// device cannot be found / opened or the ioctl fails.
fn open_rumble(
    js_path:     &std::path::Path,
    duration_ms: u16,
    magnitude:   u16,
) -> Option<(File, i16)> {
    let event_path = find_event_device_for_joystick(js_path)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&event_path)
        .ok()?;

    eprintln!("[gamepad] rumble: opened {:?}", event_path);

    // Upload the rumble effect.  The kernel fills in `id` on success; we pass
    // -1 to request a fresh slot.
    let mut effect = FfEffect {
        effect_type: FF_RUMBLE,
        id:          -1,
        direction:   0,
        trigger:     FfTrigger  { button: 0, interval: 0 },
        replay:      FfReplay   { length: duration_ms, delay: 0 },
        u:           FfEffectUnion { rumble: FfRumbleEffect {
            strong_magnitude: magnitude,
            weak_magnitude:   magnitude,
        }},
    };

    // SAFETY: `effect` is a valid `FfEffect`; `EVIOCSFF` is the correct ioctl
    // for this pointer type on the event device file descriptor.
    let ret = unsafe {
        libc::ioctl(file.as_raw_fd(), EVIOCSFF, &mut effect as *mut FfEffect)
    };

    if ret < 0 {
        eprintln!(
            "[gamepad] rumble: EVIOCSFF failed: {}",
            std::io::Error::last_os_error()
        );
        return None;
    }

    eprintln!("[gamepad] rumble effect id={}", effect.id);
    Some((file, effect.id))
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

/// Full span of the symmetric i16 axis range used for normalisation.
///
/// Maps the range `−32767 … +32767` (width = 65534) to `0.0 … 1.0`.
const AXIS_FULL_SPAN: f32 = 65534.0;

/// Normalise a raw i16 axis value to the range `0.0 … 1.0`.
///
/// The symmetric axis range `−32767 … +32767` maps to `0.0 … 1.0`.
/// Values outside that range are clamped first.
fn raw_to_frac(v: i16) -> f32 {
    let clamped = (v as i32).clamp(-32767, 32767);
    (clamped + 32767) as f32 / AXIS_FULL_SPAN
}
