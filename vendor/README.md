# vendor

## eframe

`eframe/` is eframe 0.31.1 as published on crates.io, by Emil Ernerfeldt and
the egui contributors, under the MIT or Apache-2.0 licence (`eframe/LICENSE-MIT`).
`Cargo.toml` at the root builds the app with this copy in place of the
published crate (`[patch.crates-io]`).

**What is changed.** A frame that would draw exactly what the window already
shows is not drawn. egui runs a frame for every move of the mouse, and eframe
drew each one in full and handed it to the system, the whole window up to the
screen's refresh rate, though under a moving pointer there is mostly nothing
that changes. Measured on a laptop's integrated graphics (Iris Xe, a 144 Hz
screen, a window of 1318 × 839 pixels), the mouse moving over a note's text
took 13.6% of the graphics chip and a third of a processor core; with the
change it takes 0.1% and 6% of a core. Sweeping back and forth over the whole
window, where rows and buttons light up as the pointer crosses them and those
frames are drawn, it went from 14.6% to 2.3%.

The change is:

- `src/native/shown.rs`, new: what a window shows, whether a frame would draw
  the same, and the wait that keeps frames not drawn to the screen's refresh
  rate.
- `src/native/mod.rs`: the module.
- `src/native/glow_integration.rs`: `Viewport::shown`; in `run_ui_and_paint`,
  the window is cleared after the update rather than before it, and only when
  the frame is drawn; `on_window_event` forgets what is shown when the window
  gets a new size or scale or is covered or uncovered; a new surface forgets
  it too.

Each place is marked with a comment that starts "Malachite's change".
Immediate viewports and the wgpu back end, which the app does not use, are
not changed.

**Moving to another version of eframe** means copying the new version from
`~/.cargo/registry/src/*/eframe-<version>/` over this one (keeping
`LICENSE-MIT`), making the same change again, and changing the version in
`Cargo.toml` to match.
