# Clicker

A Wayland auto clicker and macro recorder for Linux. Record keyboard and mouse
presses with their exact timing, edit the steps in place, and play them back
in a loop, optionally across several virtual input devices at once. It has a
global start/stop key, a click-speed (CPS) test and a typing-speed (LPS) test.

The interface is written with [iced](https://iced.rs) and follows your KDE
(or GTK) colors, fonts and icon theme. Playback goes through `uinput`, so it
works in any Wayland application.

## Build

Requires Rust 1.88 or newer and a Wayland session.

```sh
cargo build --release
./target/release/rust-clicker
```

On Arch, `cd packaging && makepkg -si` builds and installs a package with the
desktop entry and icon.

## Input permissions

Clicker needs read/write access to `/dev/uinput` and read access to
`/dev/input/event*`. The app tells you when access is missing. On Arch, add
yourself to the `input` group and let it use `uinput`:

```sh
echo 'KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' \
  | sudo tee /etc/udev/rules.d/70-rust-clicker.rules
sudo usermod -aG input "$USER"
sudo modprobe uinput
```

Then log out and back in.

## License

GPL-3.0-or-later. See `LICENSE`.
