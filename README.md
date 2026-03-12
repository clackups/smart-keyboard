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
| `menu` | `0x32` (`KEY_M`) | Open the application pop-up menu |
| `activate_shift` | *(disabled)* | Equivalent to `activate` when Shift is held. The current selection is typed as if Shift were pressed. Remove or set to `null` to disable. |
| `activate_ctrl` | *(disabled)* | Equivalent to `activate` when Ctrl is held. Remove or set to `null` to disable. |
| `activate_alt` | *(disabled)* | Equivalent to `activate` when Alt is held. Remove or set to `null` to disable. |
| `activate_altgr` | *(disabled)* | Equivalent to `activate` when AltGr is held. Remove or set to `null` to disable. |
| `activate_enter` | *(disabled)* | Produces the Enter output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_space` | *(disabled)* | Produces the Space output regardless of which key is currently selected. Remove or set to `null` to disable. |

**Example**

```toml
[input.keyboard]
navigate_up    = 0x67   # KEY_UP
navigate_down  = 0x6c   # KEY_DOWN
navigate_left  = 0x69   # KEY_LEFT
navigate_right = 0x6a   # KEY_RIGHT
activate       = 0x39   # KEY_SPACE
menu           = 0x32   # KEY_M
# activate_shift = null
# activate_ctrl  = null
# activate_alt   = null
# activate_altgr = null
# activate_enter = null
# activate_space = null
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
| `menu` | `0x08` | Button index for opening the application pop-up menu. `0x08` is typically the Start/Menu button. Remove or set to `null` to disable. |
| `activate_shift` | *(disabled)* | Button index for activate-with-Shift. Equivalent to `activate` when Shift is held. Remove or set to `null` to disable. |
| `activate_ctrl` | *(disabled)* | Button index for activate-with-Ctrl. Equivalent to `activate` when Ctrl is held. Remove or set to `null` to disable. |
| `activate_alt` | *(disabled)* | Button index for activate-with-Alt. Equivalent to `activate` when Alt is held. Remove or set to `null` to disable. |
| `activate_altgr` | *(disabled)* | Button index for activate-with-AltGr. Equivalent to `activate` when AltGr is held. Remove or set to `null` to disable. |
| `activate_enter` | *(disabled)* | Button index for activate-Enter. Produces the Enter output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_space` | *(disabled)* | Button index for activate-Space. Produces the Space output regardless of which key is selected. Remove or set to `null` to disable. |

#### Analog stick / axis navigation

| Key | Default | Description |
|-----|---------|-------------|
| `axis_navigate_horizontal` | `0` | Axis index for left/right navigation (left stick X on most gamepads). Negative values → Left, positive → Right. Remove/null to disable. |
| `axis_navigate_vertical` | `1` | Axis index for up/down navigation (left stick Y on most gamepads). Negative values → Up, positive → Down. Remove/null to disable. |
| `axis_activate` | `0x05` | Axis index whose positive values trigger Activate (e.g. a trigger axis). Remove/null to disable. |
| `axis_menu` | *(disabled)* | Axis index whose positive values trigger Menu (e.g. a trigger axis). Remove/null to disable. |
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
menu     = 0x08   # Start/Menu button

# Activate-with-modifier buttons (comment out to disable)
# activate_shift = null
# activate_ctrl  = null
# activate_alt   = null
# activate_altgr = null
# activate_enter = null
# activate_space = null

# Analog stick navigation
# axis_navigate_horizontal = 0
# axis_navigate_vertical   = 1
# axis_threshold           = 16384
axis_activate = 0x05
# axis_menu     = null

# Absolute-axes mode (for touchpad-style controllers)
# absolute_axes = false

# Rumble feedback
# rumble             = false
# rumble_duration_ms = 50
# rumble_magnitude   = 16384
```

---

### `[navigate]`

Controls navigation behaviour.

| Key | Default | Description |
|-----|---------|-------------|
| `rollover` | `false` | When `true`, navigation wraps around at the edges of the keyboard. Moving left past the first column of a row brings the cursor to the last column of that row, and vice-versa. Moving up past the top edge (language strip) wraps to the last keyboard row, and moving down past the last row wraps back to the top. |

**Example**

```toml
[navigate]
rollover = true
```

---

### Pop-up menu

When the menu key (`input.keyboard.menu`, default: `M`) or gamepad menu button
(`input.gamepad.menu`, default: `0x08`) is pressed, a pop-up menu appears in
the centre of the screen.

The cursor starts on the first enabled item.  Navigate vertically with the
standard navigation keys (up / down or the gamepad stick / D-pad) and confirm
the selection with the activate key (Space or gamepad activate button).  Press
Escape or the menu key again to close the menu without taking any action.

If all menu items are currently disabled, the menu event is silently ignored
and the menu does not appear.

#### Menu items

| Item | Description | Enabled when |
|------|-------------|--------------|
| **Disconnect BLE** | Sends the `Z` disconnect command to the BLE dongle, dropping the active BLE link to the remote host. | BLE output mode is active (`[output] mode = "ble"`) **and** the dongle is currently connected. |

More items can be added in the future by appending `MenuItemDef` entries to
the `menu_item_defs` vector in `src/main.rs`.

---

### `[output]`

Controls how key events are forwarded to the host or peripheral.

| Key | Default | Description |
|-----|---------|-------------|
| `mode` | `"print"` | Output mode. `"print"` writes key events to stdout (useful for debugging). `"ble"` sends USB HID reports to the BLE dongle (`esp_hid_serial_bridge`). |
| `audio` | `"none"` | Audio feedback mode on navigation selection changes. `"none"` is silent. `"narrate"` plays a WAV clip naming each key (clips loaded from the `audio/` directory, or the path in `SMART_KBD_AUDIO_PATH`). `"tone"` plays a short synthesised musical tone that varies by key category (letters/punctuation, home-row bump keys F/J, digits 1–0, function keys F1–F12, and special keys each have a distinct pitch). |

**Example**

```toml
[output]
mode = "print"
# "none" (default) | "narrate" | "tone"
audio = "none"
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

---

### `[ui.colors]`

All colours of the on-screen keyboard UI.  Every value is a **6-digit hex
string** in `"#RRGGBB"` format.  Remove or comment out any entry to keep the
built-in default.

#### Key buttons

| Key | Default | Description |
|-----|---------|-------------|
| `key_normal` | `"#dadade"` | Background of regular keys (letters, digits, symbols, Space). |
| `key_mod` | `"#64646e"` | Background of modifier / function / navigation keys when inactive. |
| `mod_active` | `"#4682b4"` | Background of a modifier key that is currently active, and of the selected language button. |
| `nav_sel` | `"#ffc800"` | Navigation-cursor highlight (the amber outline / fill on the focused key). |
| `key_label_normal` | `"#141414"` | Text colour on regular keys (dark text on a light background). |
| `key_label_mod` | `"#d2d2d2"` | Text colour on modifier / function keys (light text on a dark background). |

#### Language buttons

| Key | Default | Description |
|-----|---------|-------------|
| `lang_btn_inactive` | `"#505050"` | Background of a language button that is **not** the currently active layout. |
| `lang_btn_label` | `"#ffffff"` | Text colour of language buttons. |

#### Text display

| Key | Default | Description |
|-----|---------|-------------|
| `disp_bg` | `"#1c1c1c"` | Background of the typed-text display at the top of the keyboard. |
| `disp_text` | `"#b4ffb4"` | Foreground (text) colour of the typed-text display. |

#### Window background

| Key | Default | Description |
|-----|---------|-------------|
| `win_bg` | `"#28282b"` | Window / keyboard-area background colour. |

#### Status bar

| Key | Default | Description |
|-----|---------|-------------|
| `status_bar_bg` | `"#19191c"` | Background of the status strip at the top of the window. |
| `status_ind_bg` | `"#2d2d32"` | Background of each status indicator pill (CAPS, SHIFT, CTRL, …) when inactive. |
| `status_ind_text` | `"#5a5a5f"` | Label colour of an inactive status indicator pill. |
| `status_ind_active_text` | `"#ffffff"` | Label colour of an **active** status indicator pill (modifier is on). |

#### Connectivity icons

| Key | Default | Description |
|-----|---------|-------------|
| `conn_disconnected` | `"#dc3c3c"` | Icon colour when the BLE dongle / gamepad is **not** found (red). |
| `conn_connecting` | `"#dc9628"` | Icon colour when the BLE dongle is open but the remote host is not yet paired (amber). |
| `conn_connected` | `"#50dc50"` | Icon colour when the BLE link / gamepad is connected and ready (green). |

**Example** — swap to a light theme for the key area

```toml
[ui.colors]
win_bg         = "#f0f0f0"
key_normal     = "#ffffff"
key_mod        = "#cccccc"
key_label_normal = "#000000"
key_label_mod    = "#333333"
nav_sel        = "#0078d7"
disp_bg        = "#ffffff"
disp_text      = "#003300"
```
