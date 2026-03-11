# smart-keyboard

An on-screen keyboard for Wayland compositors, built with [FLTK](https://www.fltk.org/) and Rust.

## Build prerequisites

The project uses the `fltk` crate with its `use-wayland` feature, which compiles
FLTK from source using CMake. The following system packages must be installed
**before** running `cargo build`:

### Debian / Ubuntu

```sh
sudo apt-get install \
    cmake g++ \
    libwayland-dev wayland-protocols \
    libxkbcommon-dev \
    libcairo2-dev \
    libpango1.0-dev
```

### Fedora / RHEL / CentOS

```sh
sudo dnf install \
    cmake gcc-c++ \
    wayland-devel wayland-protocols-devel \
    libxkbcommon-devel \
    cairo-devel \
    pango-devel
```

### Arch Linux

```sh
sudo pacman -S \
    cmake \
    wayland wayland-protocols \
    libxkbcommon \
    cairo \
    pango
```

## Building

```sh
cargo build --release
```

## Running

```sh
cargo run --release
```

The keyboard opens full-screen on the active Wayland display.

## Configuration

The application reads its configuration from `config.toml` in the current
working directory. You can override the path with the
`SMART_KBD_CONFIG_PATH` environment variable:

```sh
SMART_KBD_CONFIG_PATH=/etc/smart-keyboard/config.toml cargo run --release
```

If the file is missing or cannot be parsed, built-in defaults are used
silently.

---

### `[input.keyboard]`

Controls which physical keyboard keys are used to navigate the on-screen
keyboard and activate (type) the selected key.  Values are Linux evdev scan
codes in decimal or hexadecimal (see `/usr/include/linux/input-event-codes.h`).

| Key | Default | Description |
|-----|---------|-------------|
| `navigate_up` | `0x67` (`KEY_UP`) | Move selection one row up |
| `navigate_down` | `0x6c` (`KEY_DOWN`) | Move selection one row down |
| `navigate_left` | `0x69` (`KEY_LEFT`) | Move selection one column left |
| `navigate_right` | `0x6a` (`KEY_RIGHT`) | Move selection one column right |
| `activate` | `0x39` (`KEY_SPACE`) | Type the currently selected key |

**Example**

```toml
[input.keyboard]
navigate_up    = 0x67   # KEY_UP
navigate_down  = 0x6c   # KEY_DOWN
navigate_left  = 0x69   # KEY_LEFT
navigate_right = 0x6a   # KEY_RIGHT
activate       = 0x39   # KEY_SPACE
```

---

### `[input.gamepad]`

Controls gamepad / joystick input.  The application uses the Linux joystick
API (`/dev/input/jsN`) for button and axis events and the evdev force-feedback
API for rumble.

#### Basic settings

| Key | Default | Description |
|-----|---------|-------------|
| `enabled` | `true` | Enable gamepad input. Set to `false` to disable entirely. |
| `device` | `"auto"` | Path to the joystick device (e.g. `"/dev/input/js0"`). `"auto"` opens the first available `/dev/input/js0`–`js7`. |

#### Button navigation

Button indices are reported by the driver.  Use a tool such as `jstest` to
find the index for a specific button on your gamepad.  Remove or comment out
a key to disable that action.

| Key | Default | Description |
|-----|---------|-------------|
| `navigate_up` | *(disabled)* | Button index for move-up |
| `navigate_down` | *(disabled)* | Button index for move-down |
| `navigate_left` | *(disabled)* | Button index for move-left |
| `navigate_right` | *(disabled)* | Button index for move-right |
| `activate` | `0x05` | Button index for activate (type selected key). `0x05` is the A/South button on most gamepads. |

#### Analog stick / axis navigation

| Key | Default | Description |
|-----|---------|-------------|
| `axis_navigate_horizontal` | `0` | Axis index for left/right navigation (left stick X on most gamepads). Negative values → Left, positive → Right. Remove/null to disable. |
| `axis_navigate_vertical` | `1` | Axis index for up/down navigation (left stick Y on most gamepads). Negative values → Up, positive → Down. Remove/null to disable. |
| `axis_activate` | `0x05` | Axis index whose positive values trigger Activate (e.g. a trigger axis). Remove/null to disable. |
| `axis_threshold` | `16384` | Minimum absolute axis value (0–32767) required to register a direction or activation. Raw axis values range from −32767 to +32767; `16384` corresponds to approximately half-deflection. |

#### Absolute-axes mode

| Key | Default | Description |
|-----|---------|-------------|
| `absolute_axes` | `false` | When `true`, the horizontal and vertical axis values are treated as **absolute coordinates** that map directly to a key position, rather than directional inputs. The full axis range (−32767 … +32767) is divided evenly across the available columns/rows. This is useful for touchpad-style controllers or joysticks that report absolute position. |

#### Rumble (force feedback)

| Key | Default | Description |
|-----|---------|-------------|
| `rumble` | `false` | When `true`, a short force-feedback rumble is played on the gamepad each time the navigation selection changes. Requires a gamepad with a corresponding evdev event device that supports `FF_RUMBLE`. |
| `rumble_duration_ms` | `50` | Duration of the rumble effect in milliseconds. |
| `rumble_magnitude` | `16384` | Intensity of both rumble motors. Range: `0` (silent) to `65535` (maximum). `16384` is `0x4000`, approximately 25 % of maximum. |

**Example**

```toml
[input.gamepad]
enabled = true
device  = "auto"

# Button navigation (comment out to disable)
# navigate_up    = 0x00
# navigate_down  = 0x01
# navigate_left  = 0x02
# navigate_right = 0x03
activate = 0x05   # A/South button

# Analog stick navigation
# axis_navigate_horizontal = 0
# axis_navigate_vertical   = 1
# axis_threshold           = 16384
axis_activate = 0x05

# Absolute-axes mode (for touchpad-style controllers)
# absolute_axes = false

# Rumble feedback
# rumble             = false
# rumble_duration_ms = 50
# rumble_magnitude   = 16384
```

---

### `[output]`

Controls how key events are forwarded to the host or peripheral.

| Key | Default | Description |
|-----|---------|-------------|
| `mode` | `"print"` | Output mode. `"print"` writes key events to stdout (useful for debugging). `"ble"` sends USB HID reports to the BLE dongle (`esp_hid_serial_bridge`). |

**Example**

```toml
[output]
mode = "print"
```

---

### `[output.ble]`

Settings for the BLE dongle, used only when `[output] mode = "ble"`.

| Key | Default | Description |
|-----|---------|-------------|
| `vid` | `0x1209` | USB Vendor ID of the dongle. |
| `pid` | `0xbbd1` | USB Product ID of the dongle. |
| `serial` | *(absent)* | USB serial string of the dongle. When absent, the first matching VID/PID device is used. Set this when multiple matching dongles are connected. |

**Example**

```toml
[output.ble]
vid    = 0x1209
pid    = 0xbbd1
# serial = "your_dongle_serial_string"
```
