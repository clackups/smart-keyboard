# smart keyboard installation

The project will run on virtually any Linux computer with a
screen. The following instnructios are aiming Debian or Ubuntu
systems.

## Prerequisites

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

## Installing the softare and setting the kiosk display up

As described in [Cage
wiki](https://github.com/cage-kiosk/cage/wiki/Starting-Cage-on-boot-with-systemd):

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

# this is needed for systemctl --user
echo 'export XDG_RUNTIME_DIR=/run/user/$(id -u)' >>~/.bashrc
export XDG_RUNTIME_DIR=/run/user/$(id -u)


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

# Edit /opt/smartkbd/etc/config.toml to match your requitrements. 
```
