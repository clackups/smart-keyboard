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


## Prerequisites (Debian / Ubuntu)

```sh
sudo apt install -y \
    cage \
    git cmake g++ \
    libwayland-dev wayland-protocols \
    libxkbcommon-dev libcairo2-dev libpango1.0-dev libudev-dev \
    libxfixes-dev libxcursor-dev libxinerama-dev libdbus-1-dev

# Optional packages to enable audio output
sudo apt install -y \
    dbus-user-session pipewire pipewire-pulse wireplumber \
    pipewire-alsa alsa-utils pulseaudio-utils
```

## Setting up a kiosk display

As described in [Cage wiki](https://github.com/cage-kiosk/cage/wiki/Starting-Cage-on-boot-with-systemd):

```
sudo -i

useradd -c 'Cage Kiosk' -d /opt/smartkbd -m -r -s /bin/bash smartkbd
loginctl enable-linger smartkbd

# optional access to GPIO
groupadd gpio
usermod -aG gpio,input smartkbd

# Allow members of the smartkbd group access the BLE dongle serial interface
cat >/etc/udev/rules.d/99-esp_hid_serial_bridge.rules << 'EOF'
SUBSYSTEM=="tty", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="bbd1", GROUP="smartkbd", MODE="0660"
EOF

# Allow members of the gpio group to access GPIO character devices
cat >/etc/udev/rules.d/99-gpio.rules << 'EOF'
SUBSYSTEM=="gpio", KERNEL=="gpiochip*", GROUP="gpio", MODE="0660"
SUBSYSTEM=="gpio", GROUP="gpio", MODE="0660"
EOF

udevadm control --reload-rules
udevadm trigger

cat >/etc/systemd/system/smartkbd@.service <<'EOT'
# This is a system unit for launching Cage with auto-login as the
# user configured here. For this to work, wlroots must be built
# with systemd logind support.
[Unit]
Description=Smartkbd Wayland compositor on %I
After=systemd-user-sessions.service plymouth-quit-wait.service
Before=graphical.target
ConditionPathExists=/dev/tty0
Wants=dbus.socket systemd-logind.service
After=dbus.socket systemd-logind.service
Conflicts=getty@%i.service
After=getty@%i.service
[Service]
Type=simple
Environment="SMART_KBD_CONFIG_PATH=/opt/smartkbd/etc/"
Environment="SMART_KBD_AUDIO_PATH=/opt/smartkbd/share/audio/"
ExecStart=/usr/bin/cage -s /opt/smartkbd/bin/smart-keyboard 
ExecStartPost=+sh -c "tty_name='%i'; exec chvt $${tty_name#tty}"
Restart=always
User=smartkbd
UtmpIdentifier=%I
UtmpMode=user
TTYPath=/dev/%I
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
StandardInput=tty-fail
StandardOutput=journal
StandardError=journal
PAMName=smartkbd
[Install]
WantedBy=graphical.target
DefaultInstance=tty7
EOT

cat >/etc/pam.d/smartkbd <<'EOT'
auth           required        pam_unix.so nullok
account        required        pam_unix.so
session        required        pam_unix.so
session        required        pam_systemd.so
EOT


su - smartkbd
# build the app under smartkbd user
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"

mkdir $HOME/build
cd $HOME/build
git clone https://github.com/clackups/smart-keyboard.git
cd smart-keyboard/
cargo build --release

mkdir -p $HOME/bin $HOME/etc $HOME/share
cp target/release/smart-keyboard $HOME/bin/
cp config.toml keymap_*.toml $HOME/etc
cp -r audio/ $HOME/share/

# Optionally, enable audio
systemctl --user daemon-reload
systemctl --user --now enable pipewire pipewire-pulse wireplumber
# finished, goo back to root
exit


systemctl enable smartkbd@tty1.service
systemctl set-default graphical.target

# see if the smart keyboard starts on your screen
systemctl start smartkbd@tty1.service

# service startup journal
journalctl -u smartkbd@tty1.service -f

# smart-keyboard log is sent to the common journal
journalctl -f

# The keyboard opens full-screen on the active Wayland display.
```



## Configuration

The application reads its configuration from `config.toml` inside the current
working directory. You can override the directory with the
`SMART_KBD_CONFIG_PATH` environment variable:

```sh
SMART_KBD_CONFIG_PATH=/etc/smart-keyboard cargo run --release
```

If the file is missing or cannot be parsed, built-in defaults are used
silently.

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SMART_KBD_CONFIG_PATH` | `.` (current directory) | Directory that contains `config.toml` and the `keymap_*.toml` files. If unset, the current working directory is used. If the file is absent or unparseable, built-in defaults are used silently. |
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
| `activate_arrow_left` | *(disabled)* | Produces the Left Arrow output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_arrow_right` | *(disabled)* | Produces the Right Arrow output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_arrow_up` | *(disabled)* | Produces the Up Arrow output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_arrow_down` | *(disabled)* | Produces the Down Arrow output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `activate_bksp` | *(disabled)* | Produces the Backspace output regardless of which key is currently selected. Remove or set to `null` to disable. |
| `navigate_center` | *(disabled)* | Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). Remove or set to `null` to disable. |
| `mouse_toggle` | *(disabled)* | Toggle mouse-pointer mode on/off. When active, directional keys move the mouse pointer; `activate` sends a left-click and `activate_shift` sends a right-click. Remove or set to `null` to disable. |

**Example**

```toml
[input.keyboard]
navigate_up    = 0xff52   # Key::Up
navigate_down  = 0xff54   # Key::Down
navigate_left  = 0xff51   # Key::Left
navigate_right = 0xff53   # Key::Right
activate       = 0x20     # Space
menu           = 0x6d     # 'm'
# activate_shift       = null
# activate_ctrl        = null
# activate_alt         = null
# activate_altgr       = null
# activate_enter       = null
# activate_space       = null
# activate_arrow_left  = null
# activate_arrow_right = null
# activate_arrow_up    = null
# activate_arrow_down  = null
# activate_bksp        = null
# navigate_center      = null
# mouse_toggle         = null
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

#### Button / axis navigation

Each of the following options accepts either a **button index** (plain integer,
e.g. `0x05`) or an **axis specifier** (string `"a:N"`, where N is the axis
index; positive axis values above `axis_threshold` trigger the action, like
analog triggers).  Use `jstest` to find button/axis indices for your gamepad.
All options default to `null` (disabled); remove or set to `null` to disable.

| Key | Default | Description |
|-----|---------|-------------|
| `navigate_up` | *(disabled)* | Button or axis input for move-up |
| `navigate_down` | *(disabled)* | Button or axis input for move-down |
| `navigate_left` | *(disabled)* | Button or axis input for move-left |
| `navigate_right` | *(disabled)* | Button or axis input for move-right |
| `activate` | *(disabled)* | Button or axis input for activate (type selected key). |
| `menu` | *(disabled)* | Button or axis input for opening the application pop-up menu. |
| `activate_shift` | *(disabled)* | Button or axis input for activate-with-Shift. Equivalent to `activate` when Shift is held. |
| `activate_ctrl` | *(disabled)* | Button or axis input for activate-with-Ctrl. Equivalent to `activate` when Ctrl is held. |
| `activate_alt` | *(disabled)* | Button or axis input for activate-with-Alt. Equivalent to `activate` when Alt is held. |
| `activate_altgr` | *(disabled)* | Button or axis input for activate-with-AltGr. Equivalent to `activate` when AltGr is held. |
| `activate_enter` | *(disabled)* | Button or axis input for activate-Enter. Produces the Enter output regardless of which key is selected. |
| `activate_space` | *(disabled)* | Button or axis input for activate-Space. Produces the Space output regardless of which key is selected. |
| `activate_arrow_left` | *(disabled)* | Button or axis input for activate-Left Arrow. Produces the Left Arrow output regardless of which key is selected. |
| `activate_arrow_right` | *(disabled)* | Button or axis input for activate-Right Arrow. Produces the Right Arrow output regardless of which key is selected. |
| `activate_arrow_up` | *(disabled)* | Button or axis input for activate-Up Arrow. Produces the Up Arrow output regardless of which key is selected. |
| `activate_arrow_down` | *(disabled)* | Button or axis input for activate-Down Arrow. Produces the Down Arrow output regardless of which key is selected. |
| `activate_bksp` | *(disabled)* | Button or axis input for activate-Backspace. Produces the Backspace output regardless of which key is selected. |
| `navigate_center` | *(disabled)* | Button or axis input for navigate-center. Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). |
| `mouse_toggle` | *(disabled)* | Button or axis input to toggle mouse-pointer mode on/off. When active, directional inputs move the mouse pointer; `activate` sends a left-click and `activate_shift` sends a right-click. |

#### Analog stick / axis navigation

| Key | Default | Description |
|-----|---------|-------------|
| `axis_navigate_horizontal` | `[0, "normal"]` | Axis configuration for left/right navigation (left stick X on most gamepads). Accepts either a plain axis index (e.g. `0`) or a two-element array `[axis_index, transformation]` where transformation is `"normal"` (default) or `"inverted"`. With `"normal"`: negative values → Left, positive → Right. With `"inverted"` the directions are swapped. Remove/null to disable. |
| `axis_navigate_vertical` | `[1, "normal"]` | Axis configuration for up/down navigation (left stick Y on most gamepads). Accepts either a plain axis index (e.g. `1`) or a two-element array `[axis_index, transformation]` where transformation is `"normal"` (default) or `"inverted"`. With `"normal"`: negative values → Up, positive → Down. With `"inverted"` the directions are swapped. Remove/null to disable. |
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

#### Auto-repeat

When a directional axis or button is held, navigation events repeat automatically.

| Key | Default | Description |
|-----|---------|-------------|
| `repeat_delay_ms` | `300` | Time in milliseconds that a directional input must be held before the first repeat event fires. |
| `repeat_interval_ms` | `100` | Interval in milliseconds between successive repeat events once repeating has started. |

**Example**

```toml
[input.gamepad]
enabled = true
device  = "auto"

# Button / axis navigation (all default to null/disabled).
# Use a plain integer for a button ID, or "a:N" for axis N (positive trigger).
# navigate_up    = 0x00
# navigate_down  = 0x01
# navigate_left  = 0x02
# navigate_right = 0x03
# activate = 0x05   # A/South button (button)
# activate = "a:2"  # axis 2 positive (analog trigger) -- alternative
# menu     = 0x08   # Start/Menu button

# Activate-with-modifier inputs (all default to null/disabled)
# activate_shift       = null
# activate_ctrl        = null
# activate_alt         = null
# activate_altgr       = null
# activate_enter       = null
# activate_space       = null
# activate_arrow_left  = null
# activate_arrow_right = null
# activate_arrow_up    = null
# activate_arrow_down  = null
# activate_bksp        = null
# navigate_center      = null
# mouse_toggle         = null

# Analog stick navigation
# axis_navigate_horizontal = [0, "normal"]   # [axis_index, "normal"|"inverted"]
# axis_navigate_vertical   = [1, "normal"]   # [axis_index, "normal"|"inverted"]
# axis_threshold           = 16384

# Absolute-axes mode (for touchpad-style controllers)
# absolute_axes = false

# Rumble feedback
# rumble             = false
# rumble_duration_ms = 50
# rumble_magnitude   = 16384

# Auto-repeat for held directional inputs
# repeat_delay_ms    = 300
# repeat_interval_ms = 100
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
| `activate_arrow_left` | *(disabled)* | GPIO line number for activate-Left Arrow. Produces the Left Arrow output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_arrow_right` | *(disabled)* | GPIO line number for activate-Right Arrow. Produces the Right Arrow output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_arrow_up` | *(disabled)* | GPIO line number for activate-Up Arrow. Produces the Up Arrow output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_arrow_down` | *(disabled)* | GPIO line number for activate-Down Arrow. Produces the Down Arrow output regardless of which key is selected. Remove or set to `null` to disable. |
| `activate_bksp` | *(disabled)* | GPIO line number for activate-Backspace. Produces the Backspace output regardless of which key is selected. Remove or set to `null` to disable. |
| `navigate_center` | *(disabled)* | GPIO line number for navigate-center. Moves the selection to the key configured by `[navigate] center_key` (default: `"h"`). Remove or set to `null` to disable. |
| `mouse_toggle` | *(disabled)* | GPIO line number to toggle mouse-pointer mode on/off. When active, directional buttons move the mouse pointer; `activate` sends a left-click and `activate_shift` sends a right-click. Remove or set to `null` to disable. |

#### Auto-repeat

When a directional button (up / down / left / right) is held pressed, navigation
events repeat automatically — just like holding an arrow key on a keyboard.

| Key | Default | Description |
|-----|---------|-------------|
| `repeat_delay_ms` | `300` | Time in milliseconds that a directional button must be held before the first repeat event fires. |
| `repeat_interval_ms` | `100` | Interval in milliseconds between successive repeat events once repeating has started. |

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

# Auto-repeat for held directional buttons
# repeat_delay_ms    = 300
# repeat_interval_ms = 100
```

**Status indicator** — when GPIO input is enabled a `P` icon appears in the
status bar (left of the gamepad icon when the gamepad is also enabled).  The
icon is red when no GPIO lines could be opened and green once at least one
line is successfully registered.

---

### `[mouse]`

Controls the behaviour of mouse-pointer mode.  Mouse mode is activated and
deactivated by the `mouse_toggle` action (configured in `[input.keyboard]`,
`[input.gamepad]`, or `[input.gpio]`).  While active, the **MOUSE** indicator
in the status bar is highlighted and directional inputs move the system mouse
pointer instead of navigating the on-screen keyboard.

| Key | Default | Description |
|-----|---------|-------------|
| `move_max_size` | `20` | Maximum pointer delta in pixels sent in a single HID mouse report. Movement speed ramps linearly from 1 px up to this value over `move_max_time` milliseconds while a direction is held. |
| `repeat_interval` | `20` | Interval in milliseconds between successive HID mouse movement reports while a directional input is held. Lower values produce smoother movement. |
| `move_max_time` | `1000` | Time in milliseconds over which the pointer speed ramps from 1 px to `move_max_size`. Set to `0` to jump immediately to `move_max_size`. |

While mouse mode is active:
- **Directional inputs** (Up / Down / Left / Right, gamepad stick / D-pad, GPIO buttons) move the pointer.
- **`activate`** sends a left mouse button click (press on key-down, release on key-up).
- **`activate_shift`** sends a right mouse button click.
- All other keyboard-navigation actions are suppressed until mouse mode is toggled off.

**Example**

```toml
[mouse]
move_max_size   = 20
repeat_interval = 20
move_max_time   = 1000
```

---

### `[navigate]`

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
| `audio` | `"none"` | Audio feedback mode on navigation selection changes. `"none"` is silent. `"narrate"` plays a WAV clip naming each key (clips loaded from the `audio/` directory, or the path in `SMART_KBD_AUDIO_PATH`). `"tone"` plays a short synthesised musical tone that varies by key category (letters/punctuation, home-row bump keys F/J, digits 1–0, function keys F1–F12, and special keys each have a distinct pitch). `"tone_hint"` is like `"tone"` but all letter and punctuation keys are silent except for F and J (the physical home-row bump keys); digit keys and special keys still play their tones. |

**Example**

```toml
[output]
mode = "print"
# "none" (default) | "narrate" | "tone" | "tone_hint"
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
| `key_release_delay` | `20000` | Delay in **microseconds** between the key-press HID report and the key-release report (`K0000`). Gives the remote Bluetooth host time to register the key press before it is released. Set to `0` to send the release immediately. |
| `lang_switch_release_delay` | `200000` | Delay in **microseconds** between the language-switch key-press HID report and the key-release report (`K0000`) in `on_lang_switch()`. Language-switch combos (e.g. Ctrl+Shift+1) typically need a longer hold time than regular keys so the OS registers the shortcut reliably. Set to `0` to send the release immediately. |

**Example**

```toml
[output.ble]
vid    = 0x1209
pid    = 0xbbd1
# serial = "your_dongle_serial_string"
# key_release_delay = 20000
# lang_switch_release_delay = 200000
```

---

### `[ui]`

General UI settings.

| Key | Default | Description |
|-----|---------|-------------|
| `show_text_display` | `false` | When `true`, a read-only text display is shown at the top of the keyboard window, reflecting the characters typed so far. Pressing Enter clears the display. When `false` (the default) the display is hidden and no CPU is spent updating the text buffer. |
| `active_keymaps` | `["us", "ua"]` | Ordered list of keymap names to show in the language strip at the bottom of the keyboard. Each name must have a corresponding built-in layout or a `keymap_<name>.toml` file in the config directory. The first entry is the default layout shown on startup. |

**Example**

```toml
[ui]
show_text_display = true
active_keymaps    = ["us", "ua"]
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

---

### `[keymap.xx]`

Per-keymap configuration in `config.toml`.  Replace `xx` with the keymap name
(e.g. `us`, `ua`, `de`, `fr`).  Each keymap listed in `[ui] active_keymaps`
may have an optional `[keymap.xx]` section.

| Key | Default | Description |
|-----|---------|-------------|
| `switch_scancode` | *(empty)* | Raw HID report bytes sent to the output device when the user switches to this keymap (e.g. by pressing its language button). The array is `[modifier_byte, keycode, ...]` — the same format as a USB HID keyboard report. When empty (the default), no scancode is sent on switch. |

**Example** — send Ctrl+Shift+1 when switching to the US layout,
Ctrl+Shift+4 when switching to Ukrainian, Ctrl+Shift+3 for German, and
Ctrl+Shift+2 for French:

```toml
[keymap.us]
# Ctrl+Shift+1 (modifier=0x03, HID keycode 0x1e)
switch_scancode = [0x03, 0x1e]

[keymap.ua]
# Ctrl+Shift+4 (modifier=0x03, HID keycode 0x21)
switch_scancode = [0x03, 0x21]

[keymap.de]
# Ctrl+Shift+3 (modifier=0x03, HID keycode 0x20)
switch_scancode = [0x03, 0x20]

[keymap.fr]
# Ctrl+Shift+2 (modifier=0x03, HID keycode 0x1f)
switch_scancode = [0x03, 0x1f]
```

---

### Keymap TOML files

Each active keymap may have a TOML file named `keymap_<name>.toml` placed in
the same directory as `config.toml`.  If such a file exists it takes
precedence over the built-in layout for that name.  If no file is found, the
application falls back to the built-in layout (currently available for `us`
and `ua`).  If neither exists, the keymap is skipped with a warning.

The following keymap TOML files are included in the repository:

| File | Layout | Description |
|------|--------|-------------|
| `keymap_us.toml` | US | Standard US QWERTY layout |
| `keymap_ua.toml` | UA | Ukrainian QWERTY layout |
| `keymap_de.toml` | DE | German QWERTZ layout |
| `keymap_fr.toml` | FR | French AZERTY layout |

Narrator WAV clips for all four layouts are provided in the `audio/`
directory.  For a new layout you add yourself, create the corresponding
`audio/<lang>_<slug>.wav` clips and a `audio/lang_<lang>.wav` clip for the
language-button announcement (where `<lang>` is the lowercase `name` field
from the keymap file).

A keymap file must contain a `name` string (the human-readable label shown on
the language button) and a `[[keys]]` array listing every key in the keyboard
grid, in row-major order (left to right, top to bottom).

Each `[[keys]]` entry supports the following fields:

| Field | Description |
|-------|-------------|
| `label_unshifted` | Text displayed on the button face in the unshifted state. |
| `insert_unshifted` | String inserted when the key is activated without Shift. Required; must be present in every `[[keys]]` entry. |
| `label_shifted` | Text displayed on the button face in the shifted state. Use an empty string for letter keys — the display will use the automatic uppercase of `insert_unshifted`. |
| `insert_shifted` | String inserted when Shift is held. Use an empty string for letter keys — the inserted text will be the automatic uppercase of `insert_unshifted`. |

**Example** — a snippet from the German (`de`) keymap showing a regular letter
key (auto-uppercased on Shift) and a non-letter key with an explicit shifted
character:

```toml
name = "DE"

# Letter key: leave label_shifted/insert_shifted empty — Shift gives Ä automatically
[[keys]]
label_unshifted = "\u00e4"
insert_unshifted = "\u00e4"
label_shifted = ""
insert_shifted = ""

# Non-letter key: provide explicit shifted character
[[keys]]
label_unshifted = "\u00df"
insert_unshifted = "\u00df"
label_shifted = "?"
insert_shifted = "?"
```

Place the file alongside your `config.toml` and add `"de"` (or `"fr"`, etc.)
to `[ui] active_keymaps`:

```toml
[ui]
active_keymaps = ["us", "de", "fr"]

[keymap.de]
# Ctrl+Shift+3 switches the OS input method to German
switch_scancode = [0x03, 0x20]

[keymap.fr]
# Ctrl+Shift+2 switches the OS input method to French
switch_scancode = [0x03, 0x1f]
```
