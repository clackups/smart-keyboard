// src/gpio.rs
//
// Non-blocking GPIO input using the Linux GPIO character device interface
// (gpiod v1 ABI).
//
// Primary path: GPIO_GET_LINEEVENT_IOCTL registers a per-line event fd that
// becomes readable on each edge transition.  This requires the GPIO chip to
// expose a per-line hardware IRQ (gpiod_to_irq must succeed in the kernel).
//
// Polling fallback: when LINEEVENT fails (e.g. the GPIO chip is an expander
// with no per-line IRQ), the code retries with GPIO_GET_LINEHANDLE_IOCTL and
// reads the current line value on every poll() call.  This is the same
// mechanism used by gpioget(1) and works on all GPIO chips that support read
// access.
//
// Wrong-chip fallback: when both ioctls fail with EINVAL the most common cause
// is that the configured chip (/dev/gpiochip0 by default) has fewer GPIO lines
// than the requested line offset (e.g. gpioget 1 76 uses /dev/gpiochip1, not
// /dev/gpiochip0).  The code uses GPIO_GET_CHIPINFO_IOCTL to confirm this and
// then probes /dev/gpiochip0 .. /dev/gpiochip7 to find the chip that hosts the
// line.  A hint is printed so the user can set chip = "/dev/gpiochipN" in their
// config to avoid the scan on future runs.
//
// The poll() method handles both event and polling modes without blocking.

use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, FromRawFd};

use crate::config::{GpioInputConfig, GpioPull, GpioSignal};

// =============================================================================
// Linux GPIO character device v1 ABI constants
// =============================================================================

/// `GPIOHANDLE_REQUEST_INPUT`: configure the line as an input.
const GPIOHANDLE_REQUEST_INPUT: u32 = 1 << 0;
/// `GPIOHANDLE_REQUEST_BIAS_PULL_DOWN`: enable pull-down resistor (kernel >= 5.5).
const GPIOHANDLE_REQUEST_BIAS_PULL_DOWN: u32 = 1 << 6;
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

/// `GPIO_GET_LINEHANDLE_IOCTL` = `_IOWR('B', 0x03, struct gpiohandle_request)`.
///
/// Computed as:
///   direction (read+write = 3) << 30                  = 0xC000_0000
///   size (sizeof gpiohandle_request = 364 = 0x16C) << 16 = 0x016C_0000
///   type ('B' = 0x42) << 8                            = 0x0000_4200
///   number (0x03)                                     = 0x0000_0003
///   total                                             = 0xC16C_4203
const GPIO_GET_LINEHANDLE_IOCTL: libc::c_ulong = 0xC16C_4203;

/// `GPIOHANDLE_GET_LINE_VALUES_IOCTL` = `_IOWR('B', 0x08, struct gpiohandle_data)`.
///
/// Computed as:
///   direction (read+write = 3) << 30              = 0xC000_0000
///   size (sizeof gpiohandle_data = 64 = 0x40) << 16 = 0x0040_0000
///   type ('B' = 0x42) << 8                        = 0x0000_4200
///   number (0x08)                                 = 0x0000_0008
///   total                                         = 0xC040_4208
const GPIOHANDLE_GET_LINE_VALUES_IOCTL: libc::c_ulong = 0xC040_4208;

/// `GPIO_GET_CHIPINFO_IOCTL` = `_IOR('B', 0x01, struct gpiochip_info)`.
///
/// Computed as:
///   direction (read = 2) << 30                   = 0x8000_0000
///   size (sizeof gpiochip_info = 68 = 0x44) << 16 = 0x0044_0000
///   type ('B' = 0x42) << 8                       = 0x0000_4200
///   number (0x01)                                = 0x0000_0001
///   total                                        = 0x8044_4201
const GPIO_GET_CHIPINFO_IOCTL: libc::c_ulong = 0x8044_4201;

// Compile-time sanity checks.
const _: () = assert!(GPIO_GET_LINEEVENT_IOCTL == 0xC0304204);
const _: () = assert!(GPIO_GET_LINEHANDLE_IOCTL == 0xC16C4203);
const _: () = assert!(GPIOHANDLE_GET_LINE_VALUES_IOCTL == 0xC0404208);
const _: () = assert!(GPIO_GET_CHIPINFO_IOCTL == 0x80444201);

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

/// `struct gpiohandle_request` from `<linux/gpio.h>` (364 bytes).
///
/// Used with `GPIO_GET_LINEHANDLE_IOCTL` to open a line for value-polling.
/// The kernel fills `fd` with a handle file descriptor on success.
///
/// Layout (all naturally aligned, no implicit padding):
/// - offset   0: lineoffsets     ([u32; 64], 256 bytes)
/// - offset 256: flags           (u32,         4 bytes)
/// - offset 260: default_values  ([u8; 64],   64 bytes)
/// - offset 324: consumer_label  ([u8; 32],   32 bytes)
/// - offset 356: lines           (u32,         4 bytes)
/// - offset 360: fd              (i32,         4 bytes)
/// - total: 364 bytes
#[repr(C)]
struct GpioHandleRequest {
    lineoffsets:    [u32; 64],
    flags:          u32,
    default_values: [u8; 64],
    consumer_label: [u8; 32],
    lines:          u32,
    fd:             i32,
}

const _: () = assert!(std::mem::size_of::<GpioHandleRequest>() == 364);

/// `struct gpiohandle_data` from `<linux/gpio.h>` (64 bytes).
///
/// Used with `GPIOHANDLE_GET_LINE_VALUES_IOCTL` to read line values from a
/// handle fd.  `values[i]` is 1 (high) or 0 (low) for each requested line.
#[repr(C)]
struct GpioHandleData {
    values: [u8; 64],
}

const _: () = assert!(std::mem::size_of::<GpioHandleData>() == 64);

/// `struct gpiochip_info` from `<linux/gpio.h>` (68 bytes).
///
/// Used with `GPIO_GET_CHIPINFO_IOCTL` to read the name, label, and number of
/// GPIO lines exposed by a chip device.
///
/// Layout:
/// - offset  0: name   ([u8; 32], 32 bytes)
/// - offset 32: label  ([u8; 32], 32 bytes)
/// - offset 64: lines  (u32,       4 bytes)
/// - total: 68 bytes
#[repr(C)]
struct GpioChipInfo {
    name:  [u8; 32],
    label: [u8; 32],
    lines: u32,
}

const _: () = assert!(std::mem::size_of::<GpioChipInfo>() == 68);

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
    /// Produce the Left Arrow output directly.
    ActivateArrowLeft,
    /// Produce the Right Arrow output directly.
    ActivateArrowRight,
    /// Produce the Up Arrow output directly.
    ActivateArrowUp,
    /// Produce the Down Arrow output directly.
    ActivateArrowDown,
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
    /// File descriptor returned by either `GPIO_GET_LINEEVENT_IOCTL` or
    /// `GPIO_GET_LINEHANDLE_IOCTL`, wrapped in a `File` so it is closed
    /// automatically when `GpioInput` is dropped.
    file:        File,
    /// The keyboard action this line is bound to.
    action:      GpioAction,
    /// When `true`, a rising edge (low -> high) means "pressed";
    /// when `false`, a falling edge (high -> low) means "pressed".
    active_high: bool,
    /// Last known logical pressed state (used to suppress duplicate events).
    pressed:     bool,
    /// When `true` the line was opened via `GPIO_GET_LINEHANDLE_IOCTL` and its
    /// state is read by calling `GPIOHANDLE_GET_LINE_VALUES_IOCTL` on every
    /// `poll()` invocation.  When `false` the line was opened via
    /// `GPIO_GET_LINEEVENT_IOCTL` and delivers edge-event records via `read()`.
    polled:      bool,
}

/// Non-blocking reader for a set of Linux GPIO lines.
pub struct GpioInput {
    lines: Vec<GpioLine>,
}

// =============================================================================
// Per-chip helpers (free functions)
// =============================================================================

/// Return the number of GPIO lines on `chip_fd` via `GPIO_GET_CHIPINFO_IOCTL`.
///
/// Returns `0` on failure (caller treats 0 as "unknown").
fn chip_ngpio(chip_fd: libc::c_int) -> u32 {
    let mut info = GpioChipInfo { name: [0u8; 32], label: [0u8; 32], lines: 0 };
    // SAFETY: `chip_fd` is a valid open chip fd; `info` is a correctly
    // laid-out `GpioChipInfo` and `GPIO_GET_CHIPINFO_IOCTL` expects exactly
    // that type at that size (68 bytes).
    let ret = unsafe { libc::ioctl(chip_fd, GPIO_GET_CHIPINFO_IOCTL, &mut info) };
    if ret >= 0 { info.lines } else { 0 }
}

/// Try to open a single GPIO line on `chip_fd`.
///
/// Attempts `GPIO_GET_LINEEVENT_IOCTL` first (edge-event delivery, requires a
/// per-line hardware IRQ).  On failure, falls back to
/// `GPIO_GET_LINEHANDLE_IOCTL` (polling mode, no IRQ required).  When
/// `bias_flags` are non-zero and the first attempt returns `EINVAL` (kernel
/// < 5.5 does not support bias flags), each path is automatically retried
/// without bias flags.
///
/// Returns `Ok((file, polled))` on success:
///   - `polled = false`: event mode (LINEEVENT), edge records via `read()`.
///   - `polled = true`:  polling mode (LINEHANDLE), value via ioctl.
///
/// Returns `Err(last_error)` when both paths fail; `last_error` is the errno
/// from the final LINEHANDLE attempt.
fn try_line_on_chip_fd(
    chip_fd:     libc::c_int,
    line_offset: u32,
    handle_flags: u32,
    bias_flags:  u32,
    event_flags: u32,
    label:       &[u8],
) -> Result<(File, bool), std::io::Error> {
    // ---- LINEEVENT path ----
    let mut req = GpioEventRequest {
        lineoffset:     line_offset,
        handleflags:    handle_flags,
        eventflags:     event_flags,
        consumer_label: [0u8; 32],
        fd:             -1,
    };
    let copy_len = label.len().min(req.consumer_label.len() - 1);
    req.consumer_label[..copy_len].copy_from_slice(&label[..copy_len]);

    // SAFETY: `chip_fd` is a valid open chip fd; `req` is a correctly
    // laid-out `GpioEventRequest`.
    let ret = unsafe { libc::ioctl(chip_fd, GPIO_GET_LINEEVENT_IOCTL, &mut req) };
    let le_err = std::io::Error::last_os_error();

    // On EINVAL with bias flags: kernel < 5.5 does not support them; retry.
    let ret = if ret < 0 && bias_flags != 0
        && le_err.raw_os_error() == Some(libc::EINVAL)
    {
        req.handleflags = GPIOHANDLE_REQUEST_INPUT;
        // SAFETY: same as above.
        unsafe { libc::ioctl(chip_fd, GPIO_GET_LINEEVENT_IOCTL, &mut req) }
    } else {
        ret
    };

    if ret >= 0 {
        // Set the event fd non-blocking.
        // SAFETY: `req.fd` is a valid kernel-assigned fd.
        let nb = unsafe { libc::fcntl(req.fd, libc::F_SETFL, libc::O_NONBLOCK) };
        if nb < 0 {
            let e = std::io::Error::last_os_error();
            // SAFETY: `req.fd` is a valid, owned fd.
            unsafe { libc::close(req.fd); }
            return Err(e);
        }
        // SAFETY: `req.fd` is a valid, owned fd.
        return Ok((unsafe { File::from_raw_fd(req.fd) }, false));
    }

    // ---- LINEHANDLE path (polling fallback) ----
    let mut hreq = GpioHandleRequest {
        lineoffsets:    [0u32; 64],
        flags:          handle_flags,
        default_values: [0u8; 64],
        consumer_label: [0u8; 32],
        lines:          1,
        fd:             -1,
    };
    hreq.lineoffsets[0] = line_offset;
    let hcopy = label.len().min(hreq.consumer_label.len() - 1);
    hreq.consumer_label[..hcopy].copy_from_slice(&label[..hcopy]);

    // SAFETY: `chip_fd` is a valid open chip fd; `hreq` is a correctly
    // laid-out `GpioHandleRequest`.
    let hret = unsafe { libc::ioctl(chip_fd, GPIO_GET_LINEHANDLE_IOCTL, &mut hreq) };
    let hfirst_err = std::io::Error::last_os_error();

    // On EINVAL with bias flags: retry without them.
    let hret = if hret < 0 && bias_flags != 0
        && hfirst_err.raw_os_error() == Some(libc::EINVAL)
    {
        hreq.flags = GPIOHANDLE_REQUEST_INPUT;
        // SAFETY: same as above.
        unsafe { libc::ioctl(chip_fd, GPIO_GET_LINEHANDLE_IOCTL, &mut hreq) }
    } else {
        hret
    };

    if hret >= 0 {
        // SAFETY: `hreq.fd` is a valid, owned fd.
        return Ok((unsafe { File::from_raw_fd(hreq.fd) }, true));
    }

    Err(std::io::Error::last_os_error())
}

impl GpioInput {
    /// Open all configured GPIO lines.
    ///
    /// For each configured line the method first tries the configured chip
    /// device (`cfg.chip`, default `/dev/gpiochip0`).  If both
    /// `GPIO_GET_LINEEVENT_IOCTL` and `GPIO_GET_LINEHANDLE_IOCTL` fail with
    /// `EINVAL` and the chip reports that the line offset is beyond its range,
    /// the method automatically probes `/dev/gpiochip0` .. `/dev/gpiochip7` to
    /// find the chip that actually owns the line.  A hint is logged so the user
    /// can make the selection permanent by adding `chip = "/dev/gpiochipN"` to
    /// the `[input.gpio]` section in their config.
    ///
    /// Lines that cannot be opened on any chip are skipped with a warning.
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
            (cfg.activate_arrow_left,  GpioAction::ActivateArrowLeft),
            (cfg.activate_arrow_right, GpioAction::ActivateArrowRight),
            (cfg.activate_arrow_up,    GpioAction::ActivateArrowUp),
            (cfg.activate_arrow_down,  GpioAction::ActivateArrowDown),
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

        // Open the configured GPIO chip (read+write required by the ABI).
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
        // Note: GPIOHANDLE_REQUEST_BIAS_PULL_UP / _PULL_DOWN require kernel >= 5.5.
        // When gpio_pull = "null" (the default), no bias flags are set (0) so
        // that the ioctl succeeds on all kernel versions.  Explicitly disabling
        // the bias via GPIOHANDLE_REQUEST_BIAS_DISABLE (1 << 7, kernel >= 5.5)
        // is not necessary here because the line hardware default already has no
        // pull when no bias flag is requested.
        let bias_flags: u32 = match cfg.gpio_pull {
            GpioPull::Up   => GPIOHANDLE_REQUEST_BIAS_PULL_UP,
            GpioPull::Down => GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
            GpioPull::Null => 0, // no bias flags: works on all kernel versions
        };
        let handle_flags = GPIOHANDLE_REQUEST_INPUT | bias_flags;

        // Request both edges so we can see both press and release.
        let event_flags = GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE;

        let label = b"smart-keyboard";
        let mut lines: Vec<GpioLine> = Vec::with_capacity(configured.len());

        for (line_offset, action) in configured {
            match try_line_on_chip_fd(chip_fd, line_offset, handle_flags,
                                      bias_flags, event_flags, label)
            {
                Ok((file, polled)) => {
                    eprintln!(
                        "[gpio] line {} opened for action {:?}{}",
                        line_offset, action,
                        if polled { " (polling mode)" } else { "" },
                    );
                    lines.push(GpioLine {
                        file, action, active_high, pressed: false, polled,
                    });
                }

                Err(ref e) if e.raw_os_error() == Some(libc::EINVAL) => {
                    // Both LINEEVENT and LINEHANDLE returned EINVAL.
                    // The most likely cause is that the line offset exceeds the
                    // configured chip's gpio count — the line is on a different
                    // chip device (e.g. /dev/gpiochip1 instead of /dev/gpiochip0).
                    //
                    // Confirm via CHIPINFO and, if out-of-range, scan other chips.
                    let ngpio = chip_ngpio(chip_fd);
                    let out_of_range = ngpio > 0 && line_offset >= ngpio;

                    if out_of_range {
                        eprintln!(
                            "[gpio] line {} out of range for {:?} ({} lines); \
                             searching other gpiochip devices",
                            line_offset, cfg.chip, ngpio,
                        );
                    } else {
                        eprintln!(
                            "[gpio] cannot open line {} ({:?}) on {:?}: {}; \
                             trying other gpiochip devices",
                            line_offset, action, cfg.chip, e,
                        );
                    }

                    // Probe /dev/gpiochip0 .. /dev/gpiochip7, skipping the
                    // already-tried configured chip.
                    let mut found = false;
                    for n in 0u8..=7 {
                        let alt_path = format!("/dev/gpiochip{}", n);
                        if alt_path == cfg.chip {
                            continue;
                        }
                        let alt_file = match OpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(&alt_path)
                        {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        let alt_fd = alt_file.as_raw_fd();

                        match try_line_on_chip_fd(alt_fd, line_offset, handle_flags,
                                                  bias_flags, event_flags, label)
                        {
                            Ok((file, polled)) => {
                                eprintln!(
                                    "[gpio] line {} found on {} for action {:?}{} \
                                     (hint: add chip = {:?} to [input.gpio] in config)",
                                    line_offset, alt_path, action,
                                    if polled { " (polling mode)" } else { "" },
                                    alt_path,
                                );
                                lines.push(GpioLine {
                                    file, action, active_high, pressed: false, polled,
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
                }

                Err(e) => {
                    eprintln!(
                        "[gpio] cannot open line {} ({:?}): {}",
                        line_offset, action, e,
                    );
                }
            }
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
    /// For event-based lines (opened via `GPIO_GET_LINEEVENT_IOCTL`): reads all
    /// pending edge-event records from the non-blocking event fd.
    ///
    /// For polled lines (opened via `GPIO_GET_LINEHANDLE_IOCTL`): reads the
    /// current line value via `GPIOHANDLE_GET_LINE_VALUES_IOCTL` and synthesises
    /// a press or release event whenever the value changes.
    ///
    /// `out` is cleared before filling.  Always returns `true` because neither
    /// mode disconnects like a gamepad device.
    pub fn poll(&mut self, out: &mut Vec<GpioEvent>) -> bool {
        out.clear();

        for line in &mut self.lines {
            if line.polled {
                // Polling mode: read the current GPIO value via ioctl.
                let mut data = GpioHandleData { values: [0u8; 64] };

                // SAFETY: `line.file.as_raw_fd()` is a valid handle fd;
                // `data` is a correctly laid-out `GpioHandleData`.
                let ret = unsafe {
                    libc::ioctl(
                        line.file.as_raw_fd(),
                        GPIOHANDLE_GET_LINE_VALUES_IOCTL,
                        &mut data,
                    )
                };
                if ret < 0 {
                    eprintln!(
                        "[gpio] poll read error on {:?}: {}",
                        line.action,
                        std::io::Error::last_os_error(),
                    );
                    continue;
                }

                // values[0] is 1 (high) or 0 (low) for the single requested line.
                let pressed = if line.active_high {
                    data.values[0] != 0
                } else {
                    data.values[0] == 0
                };

                if pressed != line.pressed {
                    line.pressed = pressed;
                    out.push(GpioEvent { action: line.action, pressed });
                }
            } else {
                // Event mode: drain all pending edge-event records.
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
        }

        true
    }
}
