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

The `.cargo/config.toml` in this repository sets `CFLTK_WAYLAND_ONLY=1`,
which compiles FLTK in pure-Wayland mode (no X11 dependency).

## Running

```sh
cargo run --release
```

The keyboard opens full-screen on the active Wayland display.
