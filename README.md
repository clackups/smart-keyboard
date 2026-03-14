# smart keyboard

This project aims building a Linux device that acts as an accessible
virtual keyboard for users with disabilities. The device has a small
screen that displays a virtual keyboard, and the user is given rich
possibilities to navigate the keyboard and enter the text.

The device uses an [ESP32-S3 USB
dongle](https://github.com/clackups/esp_hid_serial_bridge) that
simulates a Bluetooth keyboard toward the main computer.

The user's input can include a mouse, a keyboard, a game controller,
or button switches attached to GPIO pins on the device.

The application is implemented in Riust, using FLTK library, and it's
designed to use any available Wayland
compositor. [Cage](https://github.com/cage-kiosk/cage) is the
recommended compositor, although Weston, Sway and others can also be
used.


## Build prerequisites (Debian / Ubuntu)

```sh
sudo apt install -y \
    cage \
    git cmake g++ \
    libwayland-dev wayland-protocols \
    libxkbcommon-dev libcairo2-dev libpango1.0-dev libudev-dev \
    libxfixes-dev libxcursor-dev libxinerama-dev libdbus-1-dev
```

## Setting up a kiosk display

As described in [Cage wiki](https://github.com/cage-kiosk/cage/wiki/Starting-Cage-on-boot-with-systemd):

```
sudo -i

useradd -c 'Cage Kiosk' -d /opt/cage -m -r -s /bin/bash cage

cat >/etc/systemd/system/cage@.service <<'EOT'
# This is a system unit for launching Cage with auto-login as the
# user configured here. For this to work, wlroots must be built
# with systemd logind support.
[Unit]
Description=Cage Wayland compositor on %I
After=systemd-user-sessions.service plymouth-quit-wait.service
Before=graphical.target
ConditionPathExists=/dev/tty0
Wants=dbus.socket systemd-logind.service
After=dbus.socket systemd-logind.service
Conflicts=getty@%i.service
After=getty@%i.service
[Service]
Type=simple
ExecStart=/usr/bin/cage -s /opt/cage/build/smart-keyboard/target/release/smart-keyboard 
ExecStartPost=+sh -c "tty_name='%i'; exec chvt $${tty_name#tty}"
Restart=always
User=cage
UtmpIdentifier=%I
UtmpMode=user
TTYPath=/dev/%I
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
StandardInput=tty-fail
StandardOutput=journal
StandardError=journal
PAMName=cage
[Install]
WantedBy=graphical.target
DefaultInstance=tty7
EOT

cat >/etc/pam.d/cage <<'EOT'
auth           required        pam_unix.so nullok
account        required        pam_unix.so
session        required        pam_unix.so
session        required        pam_systemd.so
EOT

systemctl enable cage@tty1.service

systemctl set-default graphical.target

su - cage
# build the app under cage user
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"

mkdir $HOME/build
cd $HOME/build
git clone https://github.com/clackups/smart-keyboard.git
cd smart-keyboard/
cargo build --release
# finished, goo back to root
exit

# see if the smart keyboard starts on your screen
systemctl start cage@tty1.service

# service startup journal
journalctl -u cage@tty1.service -f

# smart-keyboard log is sent to the common journal
journalctl -f

# The keyboard opens full-screen on the active Wayland display.
# TODO: audio
```



## Configuration

The application reads its configuration from `config.toml` in the current
working directory. You can override the path with the
`SMART_KBD_CONFIG_PATH` environment variable:

```sh
SMART_KBD_CONFIG_PATH=/etc/smart-keyboard/config.toml cargo run --release
```

If the file is missing or cannot be parsed, built-in defaults are used
silently.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SMART_KBD_CONFIG_PATH` | `config.toml` | Path to the TOML configuration file. If unset, `config.toml` in the current working directory is used. If the file is absent or unparseable, built-in defaults are used silently. |
| `SMART_KBD_AUDIO_PATH` | `audio` | Directory that contains the WAV narration clips used by `audio = "narrate"` mode. Each clip is named `<layout>_<key>.wav` (e.g. `us_a.wav`, `ua_u0430.wav`) or `<action>.wav` (e.g. `enter.wav`, `backspace.wav`). If unset, the `audio/` sub-directory of the current working directory is used. |

---

### `[input.keyboard]`

Controls which physical keyboard keys are used to navigate the on-screen
keyboard and activate (type) the selected key.  Values are FLTK key codes,
as reported by `event_key().bits()` (and printed by the `[keyboard]` debug
output in a debug build).  On Linux these are X11 KeySym values; for
printable characters they equal the lowercase ASCII code.

| Key | Default | Description |
|-----|---------|-------------|
| `navigate_up` | `0xff52` (`Key::Up`) | Move selection one row up |
| `navigate_down` | `0xff54` (`Key::Down`) | Move selection one row down |
| `navigate_left` | `0xff51` (`Key::Left`) | Move selection one column left |
| `navigate_right` | `0xff53` (`Key::Right`) | Move selection one column right |
| `activate` | `0x20` (Space) | Type the currently selected key |
| `menu` | `0x6d` (`'m'`) | Open the application pop-up menu |
| `activate_shift` | *(disabled)* | Equivalent to `activate` when Shift is held. The current selection is typed as if Shift were pressed. Remove or set to `null` to disable. |
| `activate_ctrl` | *(disabled)* | Equivalent to `activate` when Ctrl is held. Remove or set to `null` to disable. |
| `activate_alt` | *(disabled)* | Equivalent to `activate` when Alt is held. Remove or set to `null` to disable. |
| `activate_altgr` | *(disabled)* | Equivalent to `activate` when AltGr is held. Remove or set to `null` to disable. |
| `activate_enter` | *(disabled)* | Produces the Enter output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_space` | *(disabled)* | Produces the Space output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `navigate_center` | *(disabled)* | Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). Remove or set to `null` to disable. |

**Example**

```toml
[input.keyboard]
navigate_up    = 0xff52   # Key::Up
navigate_down  = 0xff54   # Key::Down
navigate_left  = 0xff51   # Key::Left
navigate_right = 0xff53   # Key::Right
activate       = 0x20     # Space
menu           = 0x6d     # 'm'
# activate_shift  = null
# activate_ctrl   = null
# activate_alt    = null
# activate_altgr  = null
# activate_enter  = null
# activate_space  = null
# navigate_center = null
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
| `navigate_center` | *(disabled)* | Button index for navigate-center. Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). Remove or set to `null` to disable. |

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
| `absolute_axes` | `false` | When `true`, the horizontal and vertical axis values are treated as **absolute coordinates** that map directly to a key position, rather than directional inputs. The mapping is piecewise-linear and centred on the key configured by `[navigate] center_key` (default: `"h"`): the joystick's neutral position maps to that key, and each half of the axis range covers the corresponding half of the keyboard grid. This is useful for touchpad-style controllers or joysticks that report absolute position. |

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
# activate_shift  = null
# activate_ctrl   = null
# activate_alt    = null
# activate_altgr  = null
# activate_enter  = null
# activate_space  = null
# navigate_center = null

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

### `[input.gpio]`

Controls GPIO input using the Linux GPIO character device interface (gpiod v1
ABI, available on kernel 4.8+).  GPIO is **disabled by default**.  When enabled,
each navigation and action key is mapped to a numeric GPIO line offset on the
configured chip device.

The application registers each configured line for both rising- and
falling-edge events so that button press *and* release are both reported.

#### Basic settings

| Key | Default | Description |
|-----|---------|-------------|
| `enabled` | `false` | Enable GPIO input. Set to `true` to activate it. |
| `chip` | `"/dev/gpiochip0"` | Path to the GPIO chip character device. Change if your GPIO lines are on a different chip (e.g. `"/dev/gpiochip1"`). |

#### Signal polarity and pull resistors

| Key | Default | Description |
|-----|---------|-------------|
| `gpio_signal` | `"low"` | Which signal level on the line means "pressed". `"low"` — falling edge triggers press (typical with a pull-up resistor and a button that pulls to ground). `"high"` — rising edge triggers press (typical with a pull-down resistor). |
| `gpio_pull` | `"null"` | Internal pull-resistor configuration applied to **all** configured GPIO lines. `"up"` enables the internal pull-up, `"down"` enables the internal pull-down, and `"null"` leaves the line floating (no internal pull). Requires Linux kernel 5.5 or newer; on older kernels the setting is silently ignored. |

#### Button navigation

GPIO line numbers are the numeric offset of the line on the chip.  Use a
tool such as `gpioinfo` (from the `gpiod` package) to list line offsets.
Remove or set a field to `null` to disable that action.

| Key | Default | Description |
|-----|---------|-------------|
| `navigate_up` | *(disabled)* | GPIO line number for move-up |
| `navigate_down` | *(disabled)* | GPIO line number for move-down |
| `navigate_left` | *(disabled)* | GPIO line number for move-left |
| `navigate_right` | *(disabled)* | GPIO line number for move-right |
| `activate` | *(disabled)* | GPIO line number for activate (type the selected key) |
| `menu` | *(disabled)* | GPIO line number for opening the application pop-up menu |
| `activate_shift` | *(disabled)* | GPIO line number for activate-with-Shift. Remove or set to `null` to disable. |
| `activate_ctrl` | *(disabled)* | GPIO line number for activate-with-Ctrl. Remove or set to `null` to disable. |
| `activate_alt` | *(disabled)* | GPIO line number for activate-with-Alt. Remove or set to `null` to disable. |
| `activate_altgr` | *(disabled)* | GPIO line number for activate-with-AltGr. Remove or set to `null` to disable. |
| `activate_enter` | *(disabled)* | GPIO line number for activate-Enter. Produces the Enter output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_space` | *(disabled)* | GPIO line number for activate-Space. Produces the Space output regardless of which key is selected. Remove or set to `null` to disable. |
| `navigate_center` | *(disabled)* | GPIO line number for navigate-center. Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). Remove or set to `null` to disable. |

**Example** — four directional buttons and an activate button using a pull-up
resistor with active-low logic (buttons pull lines to ground when pressed):

```toml
[input.gpio]
enabled     = true
chip        = "/dev/gpiochip0"
gpio_pull   = "up"    # enable internal pull-ups on all configured lines
gpio_signal = "low"   # falling edge = pressed (default; button pulls to GND)

navigate_up    = 17
navigate_down  = 27
navigate_left  = 22
navigate_right = 23
activate       = 24
# menu           = null
```

**Status indicator** — when GPIO input is enabled a `P` icon appears in the
status bar (left of the gamepad icon when the gamepad is also enabled).  The
icon is red when no GPIO lines could be opened and green once at least one
line is successfully registered.

---



Controls navigation behaviour.

| Key | Default | Description |
|-----|---------|-------------|
| `rollover` | `false` | When `true`, navigation wraps around at the edges of the keyboard. Moving left past the first column of a row brings the cursor to the last column of that row, and vice-versa. Moving up past the top edge (language strip) wraps to the last keyboard row, and moving down past the last row wraps back to the top. |
| `center_key` | `"h"` | Button label used as the center reference point. The `navigate_center` action moves the selection to this key. When `absolute_axes = true`, the joystick's neutral position maps to this key. The value is matched against the key's unshifted label in the current layout. |
| `center_after_activate` | `false` | When `true`, the navigation selection jumps to the center button (defined by `center_key`) immediately after any activate action (including all `activate_*` variants). |

**Example**

```toml
[navigate]
rollover              = true
center_key            = "h"
center_after_activate = true
```

---

### Pop-up menu

When the menu key (`input.keyboard.menu`, default: `M`), gamepad menu button
(`input.gamepad.menu`, default: `0x08`), or GPIO menu line (`input.gpio.menu`)
is pressed, a pop-up menu appears in the centre of the screen.

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

### `[ui]`

General UI settings.

| Key | Default | Description |
|-----|---------|-------------|
| `show_text_display` | `false` | When `true`, a read-only text display is shown at the top of the keyboard window, reflecting the characters typed so far. Pressing Enter clears the display. When `false` (the default) the display is hidden and no CPU is spent updating the text buffer. |

**Example**

```toml
[ui]
show_text_display = true
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

These colours are shared by the BLE (`●`), gamepad (`G`), and GPIO (`P`)
status icons in the status bar.

| Key | Default | Description |
|-----|---------|-------------|
| `conn_disconnected` | `"#dc3c3c"` | Icon colour when the BLE dongle / gamepad / GPIO is **not** found (red). |
| `conn_connecting` | `"#dc9628"` | Icon colour when the BLE dongle is open but the remote host is not yet paired (amber). |
| `conn_connected` | `"#50dc50"` | Icon colour when the BLE link / gamepad / GPIO lines are open and ready (green). |

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
