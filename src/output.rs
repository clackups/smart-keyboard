// src/output.rs
//
// Concrete KeyHook implementations for the three configurable output modes:
//
//   PrintKeyHook  – prints key actions to stdout (mode = "print")
//   LocalKeyHook  – injects key events into the local host via Linux uinput
//                   (mode = "local")
//   BleKeyHook    – sends USB HID keyboard reports to the esp_hid_serial_bridge
//                   BLE dongle over a USB-serial connection (mode = "ble")

use std::cell::RefCell;
use std::io::Write;

use crate::KeyHook;

// =============================================================================
// Print hook (stdout)
// =============================================================================

/// Prints every key action to stdout.  This is the default "no hardware
/// output" mode; it is useful for testing and scripting.
pub struct PrintKeyHook;

impl KeyHook for PrintKeyHook {
    fn on_key_press(&self, _scancode: u16, _key: &str) {}
    fn on_key_release(&self, _scancode: u16, _key: &str) {}

    fn on_key_action(&self, scancode: u16, key: &str, _modifier_bits: u8) {
        println!("scancode=0x{:02x} key={:?}", scancode, key);
    }
}

// =============================================================================
// Local uinput hook
// =============================================================================

// Linux evdev / uinput constants.
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0x00;
const BUS_USB: u16 = 0x03;

const KEY_DOWN: i32 = 1;
const KEY_UP: i32 = 0;

// ioctl request numbers for /dev/uinput (Linux x86-64 / aarch64).
// _IO('U', 1)                               = 0x5501
// _IO('U', 2)                               = 0x5502
// _IOW('U', 100, int) where sizeof(int)=4   = 0x40045564
// _IOW('U', 101, int)                       = 0x40045565
const UI_DEV_CREATE: libc::c_ulong  = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_SET_EVBIT: libc::c_ulong   = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong  = 0x4004_5565;

/// Evdev key codes for the modifier keys we may need to inject.
const KEY_LEFTCTRL: u16   = 0x1d;
const KEY_LEFTSHIFT: u16  = 0x2a;
const KEY_RIGHTSHIFT: u16 = 0x36;
const KEY_LEFTALT: u16    = 0x38;
const KEY_RIGHTALT: u16   = 0x64; // AltGr

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor:  u16,
    product: u16,
    version: u16,
}

/// struct uinput_user_dev (linux/uinput.h).
/// Padding to 1116 bytes matches the kernel ABI on all supported archs.
#[repr(C)]
struct UinputUserDev {
    name:            [u8; 80],  // UINPUT_MAX_NAME_SIZE
    id:              InputId,
    ff_effects_max:  u32,
    absmax:          [i32; 64], // ABS_CNT = 64
    absmin:          [i32; 64],
    absfuzz:         [i32; 64],
    absflat:         [i32; 64],
}

/// struct input_event (linux/input.h) – timeval fields are 64-bit on all
/// modern 64-bit Linux (even on 32-bit archs with 64-bit time_t).
#[repr(C)]
struct InputEvent {
    tv_sec:  i64,
    tv_usec: i64,
    ev_type: u16,
    code:    u16,
    value:   i32,
}

/// Injects key events into the local host via Linux `/dev/uinput`.
///
/// A virtual keyboard device is created on construction and destroyed when
/// the hook is dropped.  Key events are injected once per `on_key_action`
/// call: modifier keys are pressed, the main key is pressed and released,
/// then modifier keys are released — matching the sticky-modifier behaviour
/// of the on-screen keyboard.
pub struct LocalKeyHook {
    fd: RefCell<i32>,
}

impl LocalKeyHook {
    /// Open `/dev/uinput` and set up a virtual keyboard device.
    /// Returns `None` if the device cannot be opened (e.g. missing permissions).
    pub fn new() -> Option<Self> {
        let path = std::ffi::CString::new("/dev/uinput").ok()?;
        let fd = unsafe {
            libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK)
        };
        if fd < 0 {
            eprintln!(
                "[output/local] cannot open /dev/uinput: {} \
                 (try: sudo chmod a+rw /dev/uinput  or  load uinput kernel module)",
                std::io::Error::last_os_error()
            );
            return None;
        }

        // Enable key events.
        if unsafe { libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_int) } < 0 {
            unsafe { libc::close(fd) };
            return None;
        }

        // Enable the key codes used by the smart keyboard layout.
        // These are the Linux evdev codes assigned to the physical keys in
        // keyboards.rs plus the modifier keys injected from modifier_bits.
        // Registering only the codes we actually emit keeps the virtual device
        // declaration minimal and avoids spurious caps-lock / num-lock LED
        // requests from the compositor.
        const USED_KEYS: &[u16] = &[
            // Row 0 – Esc, F1–F12
            0x01, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44,
            0x57, 0x58,
            // Row 1 – number row + Backspace; nav cluster
            0x29, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
            0x0c, 0x0d, 0x0e,
            0x6e, 0x66, 0x68,        // Insert, Home, PageUp
            // Row 2 – Tab, QWERTY row; nav cluster
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
            0x1a, 0x1b,
            0x6f, 0x6b, 0x6d,        // Delete, End, PageDown
            // Row 3 – CapsLock, home row + Enter
            0x3a, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
            0x28, 0x1c,
            // Row 4 – Shifts, bottom alpha row
            0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34,
            0x35, 0x36,
            // Row 5 – Ctrl, Win, Alt, Space, AltGr, Ctrl; arrow cluster
            0x1d, 0x7d, 0x38, 0x39, 0x64, 0x61,
            0x67, 0x6c, 0x69, 0x6a, // Up, Down, Left, Right
        ];
        for &code in USED_KEYS {
            unsafe { libc::ioctl(fd, UI_SET_KEYBIT, code as libc::c_int) };
        }

        // Fill in the virtual device descriptor and write it to the fd.
        let mut uidev: UinputUserDev = unsafe { std::mem::zeroed() };
        let name = b"smart-keyboard";
        uidev.name[..name.len()].copy_from_slice(name);
        uidev.id = InputId {
            bustype: BUS_USB,
            vendor:  0x0001,
            product: 0x0001,
            version: 1,
        };

        let written = unsafe {
            libc::write(
                fd,
                &uidev as *const UinputUserDev as *const libc::c_void,
                std::mem::size_of::<UinputUserDev>(),
            )
        };
        if written < 0 {
            unsafe { libc::close(fd) };
            return None;
        }

        if unsafe { libc::ioctl(fd, UI_DEV_CREATE) } < 0 {
            unsafe { libc::close(fd) };
            eprintln!(
                "[output/local] UI_DEV_CREATE failed: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }

        eprintln!("[output/local] virtual keyboard device created");
        Some(LocalKeyHook { fd: RefCell::new(fd) })
    }

    fn write_event(&self, ev_type: u16, code: u16, value: i32) {
        let fd = *self.fd.borrow();
        let event = InputEvent {
            tv_sec:  0,
            tv_usec: 0,
            ev_type,
            code,
            value,
        };
        unsafe {
            libc::write(
                fd,
                &event as *const InputEvent as *const libc::c_void,
                std::mem::size_of::<InputEvent>(),
            );
        }
    }

    fn sync(&self) {
        self.write_event(EV_SYN, SYN_REPORT, 0);
    }

    fn key_down(&self, code: u16) {
        self.write_event(EV_KEY, code, KEY_DOWN);
        self.sync();
    }

    fn key_up(&self, code: u16) {
        self.write_event(EV_KEY, code, KEY_UP);
        self.sync();
    }
}

impl Drop for LocalKeyHook {
    fn drop(&mut self) {
        let fd = *self.fd.borrow();
        if fd >= 0 {
            unsafe {
                libc::ioctl(fd, UI_DEV_DESTROY);
                libc::close(fd);
            }
        }
    }
}

impl KeyHook for LocalKeyHook {
    fn on_key_press(&self, _scancode: u16, _key: &str) {}
    fn on_key_release(&self, _scancode: u16, _key: &str) {}

    fn on_key_action(&self, scancode: u16, key_str: &str, modifier_bits: u8) {
        // Skip pure modifier-key events; their state is bundled into the
        // modifier_bits of the next regular-key action.
        if is_modifier_key_str(key_str) {
            return;
        }

        // Press modifier keys derived from modifier_bits.
        let mods = modifier_keys_from_bits(modifier_bits);
        for &m in &mods {
            self.key_down(m);
        }

        // Press and release the main key using the evdev scancode directly.
        self.key_down(scancode);
        self.key_up(scancode);

        // Release modifier keys (reverse order for symmetry).
        for &m in mods.iter().rev() {
            self.key_up(m);
        }
    }
}

// =============================================================================
// BLE dongle hook (esp_hid_serial_bridge)
// =============================================================================

/// Sends USB HID keyboard reports to an esp_hid_serial_bridge dongle over
/// its USB-serial interface.
///
/// Protocol: ASCII commands on a CDC-ACM virtual serial port.
/// `Kxxxxxx` = keyboard HID report (modifier byte, key count, key codes).
/// `K0000`   = key release (all keys up).
/// See https://github.com/clackups/esp_hid_serial_bridge for the full spec.
pub struct BleKeyHook {
    port: RefCell<Box<dyn serialport::SerialPort>>,
}

impl BleKeyHook {
    /// Search for the BLE dongle by USB VID/PID (and optionally serial string)
    /// and open the corresponding serial port.
    ///
    /// Returns `None` if no matching device is found or the port cannot be opened.
    pub fn new(vid: u16, pid: u16, serial: Option<&str>) -> Option<Self> {
        let ports = serialport::available_ports().unwrap_or_default();

        let port_name = ports.into_iter().find_map(|info| {
            if let serialport::SerialPortType::UsbPort(ref usb) = info.port_type {
                if usb.vid != vid || usb.pid != pid {
                    return None;
                }
                if let Some(filter) = serial {
                    if usb.serial_number.as_deref() != Some(filter) {
                        return None;
                    }
                }
                Some(info.port_name.clone())
            } else {
                None
            }
        })?;

        eprintln!("[output/ble] found dongle at {port_name}");

        let port = serialport::new(&port_name, 115_200)
            .timeout(std::time::Duration::from_millis(50))
            .open()
            .map_err(|e| eprintln!("[output/ble] cannot open {port_name}: {e}"))
            .ok()?;

        eprintln!("[output/ble] serial port opened");
        Some(BleKeyHook { port: RefCell::new(port) })
    }

    /// Send a raw ASCII command string to the dongle.
    fn send(&self, cmd: &str) {
        let mut port = self.port.borrow_mut();
        if let Err(e) = port.write_all(cmd.as_bytes()) {
            eprintln!("[output/ble] write error: {e}");
        }
    }

    /// Send a keyboard HID report followed immediately by a key-release report.
    ///
    /// `modifier` is the USB HID modifier byte (e.g. 0x02 = LEFTSHIFT).
    /// `keycode` is the USB HID usage-page-7 keycode (e.g. 0x04 = 'a').
    fn send_key(&self, modifier: u8, keycode: u8) {
        // Press:   K <modifier:02X> 01 <keycode:02X>
        // Release: K 00 00
        let cmd = format!("K{:02X}01{:02X}\nK0000\n", modifier, keycode);
        self.send(&cmd);
    }
}

impl KeyHook for BleKeyHook {
    fn on_key_press(&self, _scancode: u16, _key: &str) {}
    fn on_key_release(&self, _scancode: u16, _key: &str) {}

    fn on_key_action(&self, scancode: u16, key_str: &str, modifier_bits: u8) {
        // Skip pure modifier-key events; their state is bundled into modifier_bits
        // of the next regular-key on_key_action call, so no separate HID report
        // is needed.  CapsLock is handled below.
        if is_modifier_key_str(key_str) {
            return;
        }

        // Convert the Linux evdev scancode to a USB HID keyboard usage code.
        let Some(hid_code) = evdev_to_hid(scancode) else {
            // No HID mapping (e.g. Win key alone without a keycode) – skip.
            return;
        };

        // The modifier byte for the HID report.
        // If CapsLock caused an uppercase letter (not shift), add LEFTSHIFT so
        // the remote host types the correct uppercase character regardless of its
        // own CapsLock state.
        let mut hid_modifier = ble_modifier_byte(modifier_bits);
        // 0x22 = LEFTSHIFT (0x02) | RIGHTSHIFT (0x20) – check if any shift is active.
        if (hid_modifier & 0x22) == 0 {
            // No shift in modifier_bits → check if key_str implies uppercase.
            if is_uppercase_letter(key_str) {
                hid_modifier |= 0x02; // add LEFTSHIFT
            }
        }

        self.send_key(hid_modifier, hid_code);
    }
}

// =============================================================================
// Shared helpers
// =============================================================================

/// Returns true when `key_str` is the hook token for a modifier key (Ctrl,
/// Shift, Alt, AltGr, CapsLock). These keys do not generate a standalone
/// output event; their effect is included in `modifier_bits` of the next
/// regular key action.
fn is_modifier_key_str(key_str: &str) -> bool {
    matches!(
        key_str,
        "LShift" | "RShift" | "Ctrl" | "Alt" | "AltGr" | "CapsLock"
    )
}

/// Returns the evdev key codes for the modifier keys that are active according
/// to `modifier_bits` (the same bitmask passed to `on_key_action`).
///
/// Bit layout (USB HID, mirrored in our modifier_bits):
///   bit 0 (0x01) = LEFTCTRL
///   bit 1 (0x02) = LEFTSHIFT
///   bit 5 (0x20) = RIGHTSHIFT
///   bit 2 (0x04) = LEFTALT
///   bit 6 (0x40) = RIGHTALT (AltGr)
fn modifier_keys_from_bits(modifier_bits: u8) -> Vec<u16> {
    let mut keys = Vec::new();
    if modifier_bits & 0x01 != 0 { keys.push(KEY_LEFTCTRL); }
    if modifier_bits & 0x02 != 0 { keys.push(KEY_LEFTSHIFT); }
    if modifier_bits & 0x20 != 0 { keys.push(KEY_RIGHTSHIFT); }
    if modifier_bits & 0x04 != 0 { keys.push(KEY_LEFTALT); }
    if modifier_bits & 0x40 != 0 { keys.push(KEY_RIGHTALT); }
    keys
}

/// Convert our internal modifier_bits (see execute_action in main.rs) to the
/// USB HID modifier byte used by the BLE dongle protocol.
///
/// Internal bit layout:
///   0x01 = Ctrl (left)
///   0x02 = LShift
///   0x04 = Alt (left)
///   0x20 = RShift
///   0x40 = AltGr (right alt)
///
/// USB HID modifier byte (same layout, passed through directly):
///   bit 0 (0x01) = LEFTCTRL
///   bit 1 (0x02) = LEFTSHIFT
///   bit 2 (0x04) = LEFTALT
///   bit 5 (0x20) = RIGHTSHIFT
///   bit 6 (0x40) = RIGHTALT
fn ble_modifier_byte(modifier_bits: u8) -> u8 {
    // The bit layout is already aligned with USB HID – pass through as-is.
    modifier_bits
}

/// Returns true if `key_str` is a single uppercase ASCII letter.
fn is_uppercase_letter(key_str: &str) -> bool {
    let mut chars = key_str.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.is_ascii_uppercase(),
        _ => false,
    }
}

/// Convert a Linux evdev scancode to the corresponding USB HID keyboard usage
/// code (USB HID Usage Tables, page 7).
///
/// Returns `None` for modifier scancodes (they are encoded in the modifier byte
/// instead) and for unmapped codes.
fn evdev_to_hid(evdev: u16) -> Option<u8> {
    let hid: u8 = match evdev {
        0x01 => 0x29, // KEY_ESC
        0x02 => 0x1e, // KEY_1
        0x03 => 0x1f, // KEY_2
        0x04 => 0x20, // KEY_3
        0x05 => 0x21, // KEY_4
        0x06 => 0x22, // KEY_5
        0x07 => 0x23, // KEY_6
        0x08 => 0x24, // KEY_7
        0x09 => 0x25, // KEY_8
        0x0a => 0x26, // KEY_9
        0x0b => 0x27, // KEY_0
        0x0c => 0x2d, // KEY_MINUS
        0x0d => 0x2e, // KEY_EQUAL
        0x0e => 0x2a, // KEY_BACKSPACE
        0x0f => 0x2b, // KEY_TAB
        0x10 => 0x14, // KEY_Q
        0x11 => 0x1a, // KEY_W
        0x12 => 0x08, // KEY_E
        0x13 => 0x15, // KEY_R
        0x14 => 0x17, // KEY_T
        0x15 => 0x1c, // KEY_Y
        0x16 => 0x18, // KEY_U
        0x17 => 0x0c, // KEY_I
        0x18 => 0x12, // KEY_O
        0x19 => 0x13, // KEY_P
        0x1a => 0x2f, // KEY_LEFTBRACE
        0x1b => 0x30, // KEY_RIGHTBRACE
        0x1c => 0x28, // KEY_ENTER
        // 0x1d = KEY_LEFTCTRL  → modifier, no keycode
        0x1e => 0x04, // KEY_A
        0x1f => 0x16, // KEY_S
        0x20 => 0x07, // KEY_D
        0x21 => 0x09, // KEY_F
        0x22 => 0x0a, // KEY_G
        0x23 => 0x0b, // KEY_H
        0x24 => 0x0d, // KEY_J
        0x25 => 0x0e, // KEY_K
        0x26 => 0x0f, // KEY_L
        0x27 => 0x33, // KEY_SEMICOLON
        0x28 => 0x34, // KEY_APOSTROPHE
        0x29 => 0x35, // KEY_GRAVE
        // 0x2a = KEY_LEFTSHIFT  → modifier, no keycode
        0x2b => 0x31, // KEY_BACKSLASH
        0x2c => 0x1d, // KEY_Z
        0x2d => 0x1b, // KEY_X
        0x2e => 0x06, // KEY_C
        0x2f => 0x19, // KEY_V
        0x30 => 0x05, // KEY_B
        0x31 => 0x11, // KEY_N
        0x32 => 0x10, // KEY_M
        0x33 => 0x36, // KEY_COMMA
        0x34 => 0x37, // KEY_DOT
        0x35 => 0x38, // KEY_SLASH
        // 0x36 = KEY_RIGHTSHIFT → modifier, no keycode
        // 0x38 = KEY_LEFTALT    → modifier, no keycode
        0x39 => 0x2c, // KEY_SPACE
        0x3a => 0x39, // KEY_CAPSLOCK
        0x3b => 0x3a, // KEY_F1
        0x3c => 0x3b, // KEY_F2
        0x3d => 0x3c, // KEY_F3
        0x3e => 0x3d, // KEY_F4
        0x3f => 0x3e, // KEY_F5
        0x40 => 0x3f, // KEY_F6
        0x41 => 0x40, // KEY_F7
        0x42 => 0x41, // KEY_F8
        0x43 => 0x42, // KEY_F9
        0x44 => 0x43, // KEY_F10
        0x57 => 0x44, // KEY_F11
        0x58 => 0x45, // KEY_F12
        // 0x61 = KEY_RIGHTCTRL  → modifier, no keycode
        // 0x64 = KEY_RIGHTALT   → modifier, no keycode
        0x66 => 0x4a, // KEY_HOME
        0x67 => 0x52, // KEY_UP
        0x68 => 0x4b, // KEY_PAGEUP
        0x69 => 0x50, // KEY_LEFT
        0x6a => 0x4f, // KEY_RIGHT
        0x6b => 0x4d, // KEY_END
        0x6c => 0x51, // KEY_DOWN
        0x6d => 0x4e, // KEY_PAGEDOWN
        0x6e => 0x49, // KEY_INSERT
        0x6f => 0x4c, // KEY_DELETE
        0x7d => 0xe3, // KEY_LEFTMETA (Left GUI / Win)
        _ => return None,
    };
    Some(hid)
}
