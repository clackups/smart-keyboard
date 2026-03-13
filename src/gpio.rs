// src/gpio.rs
//
// Non-blocking GPIO input using the Linux GPIO character device interface
// (gpiod v1 ABI).  Each configured GPIO line is registered via the
// GPIO_GET_LINEEVENT_IOCTL on the chip device, which returns a per-line event
// file descriptor that becomes readable whenever the line changes state.
//
// The poll() method drains all pending events without blocking, mapping each
// edge transition to a press or release GpioEvent according to the configured
// signal polarity (gpio_signal = "high" / "low").

use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, FromRawFd};

use crate::config::{GpioInputConfig, GpioPull, GpioSignal};

// =============================================================================
// Linux GPIO character device v1 ABI constants
// =============================================================================

/// `GPIOHANDLE_REQUEST_INPUT`: configure the line as an input.
const GPIOHANDLE_REQUEST_INPUT: u32 = 1 << 0;
/// `GPIOHANDLE_REQUEST_BIAS_DISABLE`: disable pull resistors (kernel >= 5.5).
const GPIOHANDLE_REQUEST_BIAS_DISABLE: u32 = 1 << 3;
/// `GPIOHANDLE_REQUEST_BIAS_PULL_DOWN`: enable pull-down resistor (kernel >= 5.5).
const GPIOHANDLE_REQUEST_BIAS_PULL_DOWN: u32 = 1 << 4;
/// `GPIOHANDLE_REQUEST_BIAS_PULL_UP`: enable pull-up resistor (kernel >= 5.5).
const GPIOHANDLE_REQUEST_BIAS_PULL_UP: u32 = 1 << 5;

/// `GPIOEVENT_REQUEST_RISING_EDGE`: request rising-edge (low -> high) events.
const GPIOEVENT_REQUEST_RISING_EDGE: u32 = 1 << 0;
/// `GPIOEVENT_REQUEST_FALLING_EDGE`: request falling-edge (high -> low) events.
const GPIOEVENT_REQUEST_FALLING_EDGE: u32 = 1 << 1;

/// `gpioevent_data.id` value for a rising-edge event (line went low -> high).
const GPIOEVENT_EVENT_RISING_EDGE: u32 = 0x01;

/// `GPIO_GET_LINEEVENT_IOCTL` = `_IOWR('B', 0x04, struct gpioevent_request)`.
///
/// Computed as:
///   direction (read+write = 3) << 30   = 0xC000_0000
///   type ('B' = 0x42) << 8             = 0x0000_4200
///   number (0x04)                      = 0x0000_0004
///   size (sizeof gpioevent_request = 48) << 16 = 0x0030_0000
///   total                              = 0xC030_4204
const GPIO_GET_LINEEVENT_IOCTL: libc::c_ulong = 0xC030_4204;

// Compile-time sanity check.
const _: () = assert!(GPIO_GET_LINEEVENT_IOCTL == 0xC0304204);

// =============================================================================
// Linux GPIO v1 ABI structs
// =============================================================================

/// `struct gpioevent_request` from `<linux/gpio.h>`.
///
/// Layout (all fields are naturally aligned):
/// - offset  0: lineoffset      (u32, 4 bytes)
/// - offset  4: handleflags     (u32, 4 bytes)
/// - offset  8: eventflags      (u32, 4 bytes)
/// - offset 12: consumer_label  ([u8; 32], 32 bytes)
/// - offset 44: fd              (i32, 4 bytes)
/// - total: 48 bytes
#[repr(C)]
struct GpioEventRequest {
    lineoffset:     u32,
    handleflags:    u32,
    eventflags:     u32,
    consumer_label: [u8; 32],
    fd:             i32,
}

const _: () = assert!(std::mem::size_of::<GpioEventRequest>() == 48);

/// `struct gpioevent_data` from `<linux/gpio.h>` (12 bytes, no padding).
///
/// Layout:
/// - offset 0: timestamp  (u64, 8 bytes) - nanoseconds since system boot
/// - offset 8: id         (u32, 4 bytes) - GPIOEVENT_EVENT_RISING/FALLING_EDGE
#[repr(C)]
struct GpioEventData {
    timestamp: u64,
    id:        u32,
}

// =============================================================================
// Public types
// =============================================================================

/// An action produced by a GPIO line state change.
///
/// The variants mirror those of `GamepadAction` (minus the gamepad-specific
/// `AbsolutePos` variant); the same key-action semantics apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GpioAction {
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
    /// Produce the Enter output directly.
    ActivateEnter,
    /// Produce the Space output directly.
    ActivateSpace,
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
// GpioInput
// =============================================================================

/// One monitored GPIO line.
struct GpioLine {
    /// Event file descriptor returned by `GPIO_GET_LINEEVENT_IOCTL`, wrapped in
    /// a `File` so it is closed automatically when `GpioInput` is dropped.
    file:        File,
    /// The keyboard action this line is bound to.
    action:      GpioAction,
    /// When `true`, a rising edge (low -> high) means "pressed";
    /// when `false`, a falling edge (high -> low) means "pressed".
    active_high: bool,
    /// Last known logical pressed state (used to suppress duplicate events).
    pressed:     bool,
}

/// Non-blocking reader for a set of Linux GPIO lines.
pub struct GpioInput {
    lines: Vec<GpioLine>,
}

impl GpioInput {
    /// Open all configured GPIO lines on the chip device.
    ///
    /// Requests both rising- and falling-edge events for every configured line
    /// so that press *and* release can be reported.  Lines that cannot be opened
    /// (e.g. not exported, permission denied) are skipped with a warning.
    ///
    /// Returns `None` when no lines are configured or none could be opened.
    pub fn open(cfg: &GpioInputConfig) -> Option<Self> {
        // Build the (line_offset, action) pairs from the config.
        let assignments: &[(Option<u32>, GpioAction)] = &[
            (cfg.navigate_up,    GpioAction::Up),
            (cfg.navigate_down,  GpioAction::Down),
            (cfg.navigate_left,  GpioAction::Left),
            (cfg.navigate_right, GpioAction::Right),
            (cfg.activate,       GpioAction::Activate),
            (cfg.menu,           GpioAction::Menu),
            (cfg.activate_shift,  GpioAction::ActivateShift),
            (cfg.activate_ctrl,   GpioAction::ActivateCtrl),
            (cfg.activate_alt,    GpioAction::ActivateAlt),
            (cfg.activate_altgr,  GpioAction::ActivateAltGr),
            (cfg.activate_enter,  GpioAction::ActivateEnter),
            (cfg.activate_space,  GpioAction::ActivateSpace),
            (cfg.navigate_center, GpioAction::NavigateCenter),
        ];

        let configured: Vec<(u32, GpioAction)> = assignments
            .iter()
            .filter_map(|(opt, act)| opt.map(|n| (n, *act)))
            .collect();

        if configured.is_empty() {
            eprintln!("[gpio] no lines configured");
            return None;
        }

        // Open the GPIO chip character device (read+write required by the ABI).
        let chip_file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(&cfg.chip)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[gpio] cannot open chip {:?}: {}", cfg.chip, e);
                return None;
            }
        };
        let chip_fd = chip_file.as_raw_fd();

        let active_high = matches!(cfg.gpio_signal, GpioSignal::High);

        // Translate pull configuration to handleflags bits.
        let bias_flags: u32 = match cfg.gpio_pull {
            GpioPull::Up   => GPIOHANDLE_REQUEST_BIAS_PULL_UP,
            GpioPull::Down => GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
            GpioPull::Null => GPIOHANDLE_REQUEST_BIAS_DISABLE,
        };
        let handle_flags = GPIOHANDLE_REQUEST_INPUT | bias_flags;

        // Request both edges so we can see both press and release.
        let event_flags = GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE;

        let mut lines: Vec<GpioLine> = Vec::with_capacity(configured.len());

        for (line_offset, action) in configured {
            let mut req = GpioEventRequest {
                lineoffset:     line_offset,
                handleflags:    handle_flags,
                eventflags:     event_flags,
                consumer_label: [0u8; 32],
                fd:             -1,
            };

            // Fill a NUL-terminated consumer label for kernel diagnostics.
            let label = b"smart-keyboard";
            let copy_len = label.len().min(req.consumer_label.len() - 1);
            req.consumer_label[..copy_len].copy_from_slice(&label[..copy_len]);

            // SAFETY: `chip_fd` is a valid open file descriptor; `req` is a
            // correctly laid-out `GpioEventRequest` and `GPIO_GET_LINEEVENT_IOCTL`
            // expects exactly that type at that size (48 bytes).
            let ret = unsafe {
                libc::ioctl(chip_fd, GPIO_GET_LINEEVENT_IOCTL, &mut req)
            };

            if ret < 0 {
                eprintln!(
                    "[gpio] LINEEVENT ioctl failed for line {} ({:?}): {}",
                    line_offset, action,
                    std::io::Error::last_os_error(),
                );
                continue;
            }

            // The kernel filled `req.fd` with a new event file descriptor.
            // Set it non-blocking so poll() can drain it without stalling.
            // SAFETY: `req.fd` is a valid kernel-assigned file descriptor.
            let nb = unsafe { libc::fcntl(req.fd, libc::F_SETFL, libc::O_NONBLOCK) };
            if nb < 0 {
                eprintln!(
                    "[gpio] fcntl O_NONBLOCK failed for line {} ({:?}): {}",
                    line_offset, action,
                    std::io::Error::last_os_error(),
                );
                // SAFETY: `req.fd` is a valid, owned file descriptor.
                unsafe { libc::close(req.fd); }
                continue;
            }

            // Wrap in a File so the fd is closed on drop.
            // SAFETY: `req.fd` is a valid, owned file descriptor.
            let file = unsafe { File::from_raw_fd(req.fd) };

            eprintln!("[gpio] line {} opened for action {:?}", line_offset, action);
            lines.push(GpioLine { file, action, active_high, pressed: false });
        }

        // The chip fd is only needed for the ioctl calls above.
        drop(chip_file);

        if lines.is_empty() {
            eprintln!("[gpio] no GPIO lines could be opened");
            return None;
        }

        Some(GpioInput { lines })
    }

    /// Drain all pending GPIO events into `out` without blocking.
    ///
    /// `out` is cleared before filling.  Always returns `true` because GPIO
    /// event file descriptors do not disconnect like a gamepad device.
    pub fn poll(&mut self, out: &mut Vec<GpioEvent>) -> bool {
        out.clear();

        for line in &mut self.lines {
            loop {
                let mut data = GpioEventData { timestamp: 0, id: 0 };
                let expected = std::mem::size_of::<GpioEventData>();

                // SAFETY: `data` is a valid, correctly-sized `GpioEventData`;
                // `line.file.as_raw_fd()` is a valid non-blocking event fd.
                let n = unsafe {
                    libc::read(
                        line.file.as_raw_fd(),
                        &mut data as *mut GpioEventData as *mut libc::c_void,
                        expected,
                    )
                };

                if n < 0 {
                    let e = std::io::Error::last_os_error();
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        break; // No more events on this line right now.
                    }
                    eprintln!("[gpio] read error on {:?}: {}", line.action, e);
                    break;
                }

                // Partial reads should not occur with fixed-size structs; skip.
                if (n as usize) != expected {
                    break;
                }

                // Determine the new logical pressed state from the edge direction
                // and the configured signal polarity.
                let pressed = if line.active_high {
                    data.id == GPIOEVENT_EVENT_RISING_EDGE
                } else {
                    data.id != GPIOEVENT_EVENT_RISING_EDGE // falling edge
                };

                // Only emit an event when the logical state actually changes.
                if pressed != line.pressed {
                    line.pressed = pressed;
                    out.push(GpioEvent { action: line.action, pressed });
                }
            }
        }

        true
    }
}
