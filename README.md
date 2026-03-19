# smart keyboard

This project aims building a Linux device that acts as an accessible
virtual keyboard and mouse for users with disabilities. The device has
a small screen that displays a virtual keyboard, and the user is given
rich possibilities to navigate the keyboard and enter the text.

The device uses an [ESP32-S3 USB
dongle](https://github.com/clackups/esp_hid_serial_bridge) that
simulates a Bluetooth keyboard toward the main computer.

The user's input can include a mouse, a keyboard, a game controller,
or button switches attached to GPIO pins on the device.

The application is implemented in Rust, using FLTK library, and it's
designed to use any available Wayland
compositor. [Cage](https://github.com/cage-kiosk/cage) is the
recommended compositor, although Weston, Sway and others can also be
used.

* [Hardware requirements](HARDWARE_REQS.md)
* [Installation instructions for Debian/Ubuntu](INSTALL.md)
* [Configuration](CONFIGURATION.md)


## Copyright and license

This work is licensed under the MIT License.

Copyright (c) 2026 clackups@gmail.com

Fediverse: [@clackups@social.noleron.com](https://social.noleron.com/@clackups)
