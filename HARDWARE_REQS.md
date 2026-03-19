# smart keyboard hardware requirements

The software will run on any Linux device with a screen and a USB host
to connect the BLE dongle and external controllers.

A low-cost example of such a device would be an Orange Pi Zero 2W
board with attached USB extension board and a HDMI display.

Optionally, the software may generate sounds for easier navigation. In
this case, the smart keyboard device needs a sound adapter and
speakers.

An ESP32-S3 device is required for Bluetooth communication with the
host computer. A LILYGO T-Dongle S3 (without screen) is an example of
a compact dongle that would plug directly into a USB port. Any other
ESP32-S3 board with a USB interface will work too.


Current testing environment:

* an Intel N5105 mini-PC with a small HDMI display with speakers
  (sound is transmitted via HDMI interface), running Ubuntu 24.04.

* an Orange Pi Zero 2W with the same interface and a mini-HDMI cable,
  running the latest Armbian Ubuntu release. I haven't managed yet to
  make the sound work. Everything else works, and the CPU is fast
  enough for the applicaiton needs.

