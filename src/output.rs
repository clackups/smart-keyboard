// src/output.rs
//
// Concrete KeyHook implementations for the two configurable output modes:
//
//   PrintKeyHook  - prints key actions to stdout (mode = "print")
//   BleKeyHook    - sends USB HID keyboard reports to the esp_hid_serial_bridge
//                   BLE dongle over a USB-serial connection (mode = "ble")

use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use crate::KeyHook;

// =============================================================================
// Print hook (stdout)
// =============================================================================

/// Prints every key action to stdout.  This is the default "no hardware
/// output" mode; it is useful for testing and scripting.
pub struct PrintKeyHook;

impl KeyHook for PrintKeyHook {
    fn on_key_press(&self, _scancode: u16, _key: &str) {}

    fn on_key_release(&self, scancode: u16, key: &str) {
        println!("key_release: scancode=0x{:02x} key={:?}", scancode, key);
    }

    fn on_key_action(&self, scancode: u16, key: &str, modifier_bits: u8) {
        println!("scancode=0x{:02x} key={:?} modifier_bits=0x{:02x}", scancode, key, modifier_bits);
    }
}

// =============================================================================
// BLE dongle hook (esp_hid_serial_bridge)
// =============================================================================

/// Read buffer size for serial responses from the BLE dongle.
const STATUS_BUF_LEN: usize = 64;

/// Mutable connection state for the BLE dongle.
///
/// Wrapped in `Rc<RefCell<>>` so it can be shared between `BleKeyHook`
/// (which forwards key events) and the periodic connection-management timer
/// in `main.rs`.
pub struct BleConnection {
    /// Open serial port, or `None` when the dongle is not connected.
    port:       Option<Box<dyn serialport::SerialPort>>,
    pub vid:    u16,
    pub pid:    u16,
    pub serial: Option<String>,
}

impl BleConnection {
    pub fn new(vid: u16, pid: u16, serial: Option<String>) -> Self {
        BleConnection { port: None, vid, pid, serial }
    }

    /// Returns `true` if a serial port is currently open.
    pub fn is_connected(&self) -> bool {
        self.port.is_some()
    }

    /// Search for the dongle by VID/PID (and optional serial string) and
    /// attempt to open its serial port.  Sets `self.port` on success and
    /// returns `true`; returns `false` if the device is not found or the
    /// port cannot be opened.
    pub fn try_connect(&mut self) -> bool {
        let ports = serialport::available_ports().unwrap_or_default();
        let port_name = ports.into_iter().find_map(|info| {
            if let serialport::SerialPortType::UsbPort(ref usb) = info.port_type {
                if usb.vid != self.vid || usb.pid != self.pid {
                    return None;
                }
                if let Some(ref filter) = self.serial {
                    if usb.serial_number.as_deref() != Some(filter.as_str()) {
                        return None;
                    }
                }
                Some(info.port_name.clone())
            } else {
                None
            }
        });

        let port_name = match port_name {
            Some(n) => n,
            None => return false,
        };

        eprintln!("[output/ble] found dongle at {port_name}");

        match serialport::new(&port_name, 115_200)
            .timeout(std::time::Duration::from_millis(50))
            .open()
        {
            Ok(p) => {
                eprintln!("[output/ble] serial port opened");
                self.port = Some(p);
                true
            }
            Err(e) => {
                eprintln!("[output/ble] cannot open {port_name}: {e}");
                false
            }
        }
    }

    /// Send a raw ASCII command string to the dongle.
    ///
    /// Returns `true` on success.  On write failure the port is closed and
    /// `false` is returned; the caller should schedule a reconnect attempt.
    pub fn send(&mut self, cmd: &str) -> bool {
        let port = match self.port.as_mut() {
            Some(p) => p,
            None => return false,
        };
        if let Err(e) = port.write_all(cmd.as_bytes()) {
            eprintln!("[output/ble] write error: {e}");
            self.port = None;
            return false;
        }
        true
    }

    /// Send a keyboard HID key-press report.
    ///
    /// `modifier` is the USB HID modifier byte (e.g. 0x02 = LEFTSHIFT).
    /// `keycode` is the USB HID usage-page-7 keycode (e.g. 0x04 = 'a').
    ///
    /// This sends only the key-press command; the matching key-release (`K0000`)
    /// must be sent separately via [`send_key_release`].
    pub fn send_key(&mut self, modifier: u8, keycode: u8) {
        // Press: K <modifier:02X> 01 <keycode:02X>
        let cmd = format!("K{:02X}01{:02X}\n", modifier, keycode);
        self.send(&cmd);
    }

    /// Send the key-release report (`K0000`) - releases all keys on the dongle.
    pub fn send_key_release(&mut self) {
        self.send("K0000\n");
    }

    /// Send the `Z` disconnect command to the BLE dongle.
    ///
    /// This requests the dongle to drop the active BLE connection to the remote
    /// host.  Returns `true` if the command was sent successfully, `false` if
    /// the port is not open or the write failed.
    pub fn send_disconnect(&mut self) -> bool {
        self.send("Z\n")
    }

    /// Send the `S` status command and read the dongle's response.
    ///
    /// Returns:
    /// * `Err(())` - the write failed; the connection has been closed
    ///   (caller should revert to retry mode).
    /// * `Ok(Some(response))` - a non-empty response was received; if it
    ///   starts with `"STATUS:CONNECTED:"` the dongle is ready.
    /// * `Ok(None)` - the write succeeded but the read timed out or returned
    ///   an empty buffer (dongle connected but remote host not yet paired).
    pub fn check_status(&mut self) -> Result<Option<String>, ()> {
        if !self.send("S\n") {
            return Err(());
        }
        let port = match self.port.as_mut() {
            Some(p) => p,
            None => return Err(()),
        };
        let mut buf = [0u8; STATUS_BUF_LEN];
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                let s = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                Ok(Some(s))
            }
            _ => Ok(None),
        }
    }
}

/// Sends USB HID keyboard reports to an esp_hid_serial_bridge dongle over
/// its USB-serial interface.
///
/// Protocol: ASCII commands on a CDC-ACM virtual serial port.
/// `Kxxxxxx` = keyboard HID report (modifier byte, key count, key codes).
/// `K0000`   = key release (all keys up).
/// See https://github.com/clackups/esp_hid_serial_bridge for the full spec.
pub struct BleKeyHook {
    conn: Rc<RefCell<BleConnection>>,
}

impl BleKeyHook {
    /// Create a new `BleKeyHook` together with the shared [`BleConnection`].
    ///
    /// The hook starts in the *disconnected* state.  Call
    /// [`BleConnection::try_connect`] (through the returned `Rc`) to
    /// establish the first connection, and set up a timer to retry
    /// periodically until the dongle is found.
    pub fn new(vid: u16, pid: u16, serial: Option<String>) -> (Self, Rc<RefCell<BleConnection>>) {
        let conn = Rc::new(RefCell::new(BleConnection::new(vid, pid, serial)));
        (BleKeyHook { conn: conn.clone() }, conn)
    }
}

impl KeyHook for BleKeyHook {
    fn on_key_press(&self, _scancode: u16, _key: &str) {}

    /// Send the key-release report (`K0000`) when the physical activation key
    /// or gamepad button is released.  Modifier-only actions are skipped because
    /// they do not generate a preceding key-press report.
    fn on_key_release(&self, _scancode: u16, key_str: &str) {
        if is_modifier_key_str(key_str) {
            return;
        }
        self.conn.borrow_mut().send_key_release();
    }

    fn on_key_action(&self, scancode: u16, key_str: &str, modifier_bits: u8) {
        // Skip pure modifier-key events; their state is bundled into modifier_bits
        // of the next regular-key on_key_action call, so no separate HID report
        // is needed.  CapsLock is handled below.
        if is_modifier_key_str(key_str) {
            return;
        }

        // Convert the Linux evdev scancode to a USB HID keyboard usage code.
        let Some(hid_code) = evdev_to_hid(scancode) else {
            // No HID mapping (e.g. Win key alone without a keycode) - skip.
            return;
        };

        // The modifier byte for the HID report.
        // If CapsLock caused an uppercase letter (not shift), add LEFTSHIFT so
        // the remote host types the correct uppercase character regardless of its
        // own CapsLock state.
        let mut hid_modifier = ble_modifier_byte(modifier_bits);
        // 0x22 = LEFTSHIFT (0x02) | RIGHTSHIFT (0x20) - check if any shift is active.
        if (hid_modifier & 0x22) == 0 {
            // No shift in modifier_bits -> check if key_str implies uppercase.
            if is_uppercase_letter(key_str) {
                hid_modifier |= 0x02; // add LEFTSHIFT
            }
        }

        self.conn.borrow_mut().send_key(hid_modifier, hid_code);
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
    // The bit layout is already aligned with USB HID - pass through as-is.
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
        // 0x1d = KEY_LEFTCTRL  -> modifier, no keycode
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
        // 0x2a = KEY_LEFTSHIFT  -> modifier, no keycode
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
        // 0x36 = KEY_RIGHTSHIFT -> modifier, no keycode
        // 0x38 = KEY_LEFTALT    -> modifier, no keycode
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
        // 0x61 = KEY_RIGHTCTRL  -> modifier, no keycode
        // 0x64 = KEY_RIGHTALT   -> modifier, no keycode
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
