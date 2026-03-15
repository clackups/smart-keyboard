// src/gpio.rs
//
// Non-blocking GPIO input backed by the `gpio-cdev` crate (Linux GPIO
// character device v1 ABI).
//
// Primary path: Line::events() registers a per-line event fd for both rising
// and falling edges.  This requires the GPIO chip to expose a per-line
// hardware IRQ (gpiod_to_irq() must succeed in the kernel).
//
// Polling fallback: when events() fails (e.g. the chip has no per-line IRQ),
// Line::request() is used instead.  The current line value is read with
// LineHandle::get_value() on every poll() call and press/release events are
// synthesised from value changes.  This is what gpioget(1) does internally.
//
// Wrong-chip fallback: when the configured chip (default /dev/gpiochip0) has
// fewer GPIO lines than the requested offset the code automatically probes
// /dev/gpiochip0 .. /dev/gpiochip7 for the chip that owns the line.
// gpio-cdev fills chip.num_lines() from the kernel CHIPINFO ioctl at open
// time, so no manual ioctl is needed for the range check.  A hint is printed
// so the user can set chip = "/dev/gpiochipN" in their config permanently.
//
// Bias flags: gpio-cdev 0.6 only defines LineRequestFlags up to OPEN_SOURCE
// (bit 4).  The pull-up (bit 5) and pull-down (bit 6) flags added in Linux
// 5.5 are not in the crate's constant set, but gpio-cdev passes flags.bits()
// directly to the kernel ioctl.  LineRequestFlags::from_bits_retain() carries
// any extra bits through, so bias flags work transparently.
//
// The poll() method handles event and polling modes without blocking.

use gpio_cdev::{
    Chip, EventRequestFlags, EventType, LineEventHandle, LineHandle,
    LineRequestFlags,
};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use crate::config::{GpioInputConfig, GpioPull, GpioSignal};

// =============================================================================
// Bias-flag constants missing from gpio-cdev 0.6
// =============================================================================

/// `GPIOHANDLE_REQUEST_BIAS_PULL_UP` (kernel >= 5.5).
const BIAS_PULL_UP: u32 = 1 << 5;
/// `GPIOHANDLE_REQUEST_BIAS_PULL_DOWN` (kernel >= 5.5).
const BIAS_PULL_DOWN: u32 = 1 << 6;

/// Consumer label written into every GPIO line request for kernel diagnostics.
const CONSUMER: &str = "smart-keyboard";

// =============================================================================
// Public types (unchanged interface consumed by main.rs)
// =============================================================================

/// An action produced by a GPIO line state change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GpioAction {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Menu,
    /// Activate with Shift held.
    ActivateShift,
    /// Activate with Ctrl held.
    ActivateCtrl,
    /// Activate with Alt held.
    ActivateAlt,
    /// Activate with AltGr held.
    ActivateAltGr,
    /// Produce the Enter output directly.
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
    /// Move the selection to the center of the keyboard.
    NavigateCenter,
}

/// A single GPIO input event (press or release).
#[derive(Clone, Copy, Debug)]
pub struct GpioEvent {
    pub action:  GpioAction,
    pub pressed: bool,
}

// =============================================================================
// Internal types
// =============================================================================

/// Wrapper around the two kinds of gpio-cdev handle.
enum GpioHandle {
    /// Edge-event fd (LINEEVENT path).  Non-blocking reads return `LineEvent`
    /// records; the fd was set O_NONBLOCK after opening.
    Event(LineEventHandle),
    /// Value-polling handle (LINEHANDLE path).  `get_value()` ioctl is
    /// instant; no blocking concern.
    Poll(LineHandle),
}

/// One monitored GPIO line.
struct GpioLine {
    handle:      GpioHandle,
    action:      GpioAction,
    /// When `true` a physical high (1) means the button is pressed.
    active_high: bool,
    /// Last emitted logical pressed state (suppresses duplicate events).
    pressed:     bool,
}

/// Non-blocking reader for a set of Linux GPIO lines.
pub struct GpioInput {
    lines:           Vec<GpioLine>,
    /// The directional action (Up/Down/Left/Right) that is currently held
    /// pressed, or `None` if no directional is held.
    held_dir:        Option<GpioAction>,
    /// Monotonic instant at which the next repeat event should be emitted,
    /// or `None` when no directional is held.
    repeat_at:       Option<Instant>,
    /// Time a directional button must be held before the first repeat fires.
    repeat_delay:    Duration,
    /// Interval between successive repeat events once repeating has started.
    repeat_interval: Duration,
}

// =============================================================================
// Helpers
// =============================================================================

/// Build `LineRequestFlags` for `INPUT` plus optional bias bits.
///
/// gpio-cdev 0.6 does not define the bias constants, but `from_bits_retain`
/// carries any extra bits through to the kernel ioctl unchanged.
fn make_flags(pull: &GpioPull) -> LineRequestFlags {
    let extra: u32 = match pull {
        GpioPull::Up   => BIAS_PULL_UP,
        GpioPull::Down => BIAS_PULL_DOWN,
        GpioPull::Null => 0,
    };
    LineRequestFlags::from_bits_retain(LineRequestFlags::INPUT.bits() | extra)
}

/// Try to open a single GPIO line on `chip`.
///
/// Attempts `line.events()` first (edge delivery, requires IRQ).  On failure,
/// falls back to `line.request()` (value polling, no IRQ required).  When
/// `flags` contain bias bits and the first call fails, each path is retried
/// without bias (for kernels < 5.5 that reject those bits with EINVAL).
///
/// Returns `Ok(GpioHandle)` on success or `Err` (the last ioctl error) when
/// both paths fail even without bias.
fn try_line_on_chip(
    chip:        &mut Chip,
    line_offset: u32,
    flags:       LineRequestFlags,
    has_bias:    bool,
) -> Result<GpioHandle, gpio_cdev::Error> {
    let line = chip.get_line(line_offset)?;

    // ---- LINEEVENT path ----
    // Clone flags because line.events() takes ownership and we may still
    // need flags below for the LINEHANDLE path.
    let ev = line.events(flags.clone(), EventRequestFlags::BOTH_EDGES, CONSUMER);
    let ev = if ev.is_err() && has_bias {
        // Retry without bias flags (kernels < 5.5 reject them with EINVAL).
        line.events(LineRequestFlags::INPUT, EventRequestFlags::BOTH_EDGES, CONSUMER)
    } else {
        ev
    };
    if let Ok(handle) = ev {
        // Set non-blocking so poll() can drain the fd without stalling.
        // SAFETY: `handle.as_raw_fd()` is a valid, open event fd.
        let nb = unsafe {
            libc::fcntl(handle.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK)
        };
        if nb < 0 {
            eprintln!(
                "[gpio] fcntl O_NONBLOCK failed for line {}: {}",
                line_offset,
                std::io::Error::last_os_error(),
            );
            // The line fd is owned by `handle` and will be closed on drop.
            // Fall through to the LINEHANDLE path below.
        } else {
            return Ok(GpioHandle::Event(handle));
        }
    }

    // ---- LINEHANDLE path (polling fallback) ----
    // flags is moved here (final use in this function).
    let poll = line.request(flags, 0, CONSUMER);
    let poll = if poll.is_err() && has_bias {
        // Retry without bias flags.
        line.request(LineRequestFlags::INPUT, 0, CONSUMER)
    } else {
        poll
    };
    Ok(GpioHandle::Poll(poll?))
}

// =============================================================================
// GpioInput implementation
// =============================================================================

impl GpioInput {
    /// Open all configured GPIO lines.
    ///
    /// For each line the method first tries the configured chip device
    /// (`cfg.chip`, default `/dev/gpiochip0`).  `gpio-cdev` reads chip
    /// capacity via the kernel CHIPINFO ioctl at `Chip::new()` time; if the
    /// line offset exceeds `chip.num_lines()` the code automatically probes
    /// `/dev/gpiochip0` .. `/dev/gpiochip7` to find the chip that hosts the
    /// line.  A hint is logged so the user can set `chip = "/dev/gpiochipN"`
    /// in `[input.gpio]` to skip the scan on future runs.
    ///
    /// Lines that cannot be opened are skipped with a warning.
    /// Returns `None` when no lines are configured or none could be opened.
    pub fn open(cfg: &GpioInputConfig) -> Option<Self> {
        let assignments: &[(Option<u32>, GpioAction)] = &[
            (cfg.navigate_up,           GpioAction::Up),
            (cfg.navigate_down,         GpioAction::Down),
            (cfg.navigate_left,         GpioAction::Left),
            (cfg.navigate_right,        GpioAction::Right),
            (cfg.activate,              GpioAction::Activate),
            (cfg.menu,                  GpioAction::Menu),
            (cfg.activate_shift,        GpioAction::ActivateShift),
            (cfg.activate_ctrl,         GpioAction::ActivateCtrl),
            (cfg.activate_alt,          GpioAction::ActivateAlt),
            (cfg.activate_altgr,        GpioAction::ActivateAltGr),
            (cfg.activate_enter,        GpioAction::ActivateEnter),
            (cfg.activate_space,        GpioAction::ActivateSpace),
            (cfg.activate_arrow_left,   GpioAction::ActivateArrowLeft),
            (cfg.activate_arrow_right,  GpioAction::ActivateArrowRight),
            (cfg.activate_arrow_up,     GpioAction::ActivateArrowUp),
            (cfg.activate_arrow_down,   GpioAction::ActivateArrowDown),
            (cfg.activate_bksp,         GpioAction::ActivateBksp),
            (cfg.navigate_center,       GpioAction::NavigateCenter),
        ];

        let configured: Vec<(u32, GpioAction)> = assignments
            .iter()
            .filter_map(|(opt, act)| opt.map(|n| (n, *act)))
            .collect();

        if configured.is_empty() {
            eprintln!("[gpio] no lines configured");
            return None;
        }

        let mut chip = match Chip::new(&cfg.chip) {
            Ok(c)  => c,
            Err(e) => {
                eprintln!("[gpio] cannot open chip {:?}: {}", cfg.chip, e);
                return None;
            }
        };

        let active_high = matches!(cfg.gpio_signal, GpioSignal::High);
        let flags   = make_flags(&cfg.gpio_pull);
        let has_bias = cfg.gpio_pull != GpioPull::Null;

        let mut lines: Vec<GpioLine> = Vec::with_capacity(configured.len());

        for (line_offset, action) in configured {
            // Check whether the line lives on the configured chip.
            if line_offset >= chip.num_lines() {
                // Wrong chip: scan gpiochip0..7 for the one that owns this line.
                eprintln!(
                    "[gpio] line {} out of range for {:?} ({} lines); \
                     searching other gpiochip devices",
                    line_offset, cfg.chip, chip.num_lines(),
                );

                let mut found = false;
                for n in 0u8..=7 {
                    let alt_path = format!("/dev/gpiochip{}", n);
                    if alt_path == cfg.chip {
                        continue;
                    }
                    let mut alt_chip = match Chip::new(&alt_path) {
                        Ok(c)  => c,
                        Err(_) => continue,
                    };
                    if line_offset >= alt_chip.num_lines() {
                        continue;
                    }
                    match try_line_on_chip(&mut alt_chip, line_offset, flags.clone(), has_bias) {
                        Ok(handle) => {
                            let polled = matches!(handle, GpioHandle::Poll(_));
                            eprintln!(
                                "[gpio] line {} found on {} for action {:?}{} \
                                 (hint: add chip = {:?} to [input.gpio] in config)",
                                line_offset, alt_path, action,
                                if polled { " (polling mode)" } else { "" },
                                alt_path,
                            );
                            lines.push(GpioLine {
                                handle, action, active_high, pressed: false,
                            });
                            found = true;
                            break;
                        }
                        Err(_) => {}
                    }
                }

                if !found {
                    eprintln!(
                        "[gpio] line {} ({:?}): not found on any gpiochip device \
                         (gpiochip0..7)",
                        line_offset, action,
                    );
                }
                continue;
            }

            // Line is within the configured chip's range.
            match try_line_on_chip(&mut chip, line_offset, flags.clone(), has_bias) {
                Ok(handle) => {
                    let polled = matches!(handle, GpioHandle::Poll(_));
                    eprintln!(
                        "[gpio] line {} opened for action {:?}{}",
                        line_offset, action,
                        if polled { " (polling mode)" } else { "" },
                    );
                    lines.push(GpioLine {
                        handle, action, active_high, pressed: false,
                    });
                }
                Err(e) => {
                    eprintln!(
                        "[gpio] cannot open line {} ({:?}): {}",
                        line_offset, action, e,
                    );
                }
            }
        }

        if lines.is_empty() {
            eprintln!("[gpio] no GPIO lines could be opened");
            return None;
        }

        Some(GpioInput {
            lines,
            held_dir:        None,
            repeat_at:       None,
            repeat_delay:    Duration::from_millis(cfg.repeat_delay_ms),
            repeat_interval: Duration::from_millis(cfg.repeat_interval_ms),
        })
    }

    /// Drain all pending GPIO events into `out` without blocking.
    ///
    /// For event-based lines: reads all pending `LineEvent` records from the
    /// non-blocking fd until `WouldBlock`.
    ///
    /// For polled lines: reads the current value with `get_value()` and
    /// synthesises press/release events on state changes.
    ///
    /// After draining hardware events, updates the directional-repeat state
    /// and appends a synthetic repeat press if the held direction's timer has
    /// elapsed (matching the gamepad axis-repeat behaviour).
    ///
    /// `out` is cleared before filling.  Always returns `true` (GPIO lines do
    /// not disconnect like gamepads).
    pub fn poll(&mut self, out: &mut Vec<GpioEvent>) -> bool {
        out.clear();
        let now = Instant::now();

        for line in &mut self.lines {
            match &mut line.handle {
                GpioHandle::Poll(handle) => {
                    match handle.get_value() {
                        Ok(v) => {
                            let pressed = if line.active_high {
                                v != 0
                            } else {
                                v == 0
                            };
                            if pressed != line.pressed {
                                line.pressed = pressed;
                                out.push(GpioEvent { action: line.action, pressed });
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[gpio] poll read error on {:?}: {}",
                                line.action, e,
                            );
                        }
                    }
                }

                GpioHandle::Event(handle) => {
                    // Drain all pending edge-event records.
                    loop {
                        match handle.get_event() {
                            Ok(event) => {
                                let rising = matches!(
                                    event.event_type(),
                                    EventType::RisingEdge
                                );
                                let pressed = if line.active_high {
                                    rising
                                } else {
                                    !rising
                                };
                                if pressed != line.pressed {
                                    line.pressed = pressed;
                                    out.push(GpioEvent {
                                        action: line.action, pressed,
                                    });
                                }
                            }
                            Err(ref e) => {
                                // WouldBlock means no more events queued right
                                // now; stop draining this line silently.
                                use std::error::Error as StdError;
                                let would_block = e
                                    .source()
                                    .and_then(|s| {
                                        s.downcast_ref::<std::io::Error>()
                                    })
                                    .map(|io| {
                                        io.kind()
                                            == std::io::ErrorKind::WouldBlock
                                    })
                                    .unwrap_or(false);
                                if !would_block {
                                    eprintln!(
                                        "[gpio] read error on {:?}: {}",
                                        line.action, e,
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Update the directional-hold state from the events collected above.
        // A press on a directional action arms the repeat timer; a release of
        // the currently-held direction disarms it.
        for evt in out.iter() {
            match evt.action {
                GpioAction::Up
                | GpioAction::Down
                | GpioAction::Left
                | GpioAction::Right => {
                    if evt.pressed {
                        self.held_dir  = Some(evt.action);
                        self.repeat_at = Some(now + self.repeat_delay);
                    } else if self.held_dir == Some(evt.action) {
                        self.held_dir  = None;
                        self.repeat_at = None;
                    }
                }
                _ => {}
            }
        }

        // Emit a repeat press if the timer has elapsed and a direction is held.
        if let (Some(dir), Some(t)) = (self.held_dir, self.repeat_at) {
            if now >= t {
                out.push(GpioEvent { action: dir, pressed: true });
                self.repeat_at = Some(now + self.repeat_interval);
            }
        }

        true
    }
}
