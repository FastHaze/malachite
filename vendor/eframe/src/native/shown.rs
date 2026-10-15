//! Malachite's change to eframe: a frame that would draw exactly what the
//! window already shows is not drawn.
//!
//! egui runs a frame for every move of the mouse, and eframe drew each one in
//! full and handed it to the system, the whole window up to the screen's
//! refresh rate, though under a moving pointer there is mostly nothing that
//! changes. A browser draws nothing at all then. Here the frame is still run,
//! as that is how egui finds out whether anything changed, but if it is made
//! of the same shapes as the last frame drawn, at the same size and with the
//! same background, and no texture changed, it is not drawn or presented, and
//! the window goes on showing what it showed.
//!
//! Anything that can make the window lose what it shows has to `forget` it,
//! so that the next frame is drawn: `glow_integration.rs` does so for a new
//! surface and for the window events that can (a new size, which includes
//! being minimised and restored, a new scale, being covered or uncovered).

use std::time::{Duration, Instant};

use egui::epaint::ClippedShape;
use winit::window::Window;

/// While frames are run, one is drawn at least this often whatever it holds,
/// in case the system lost what the window showed without saying so.
const REDRAW_ANYWAY: Duration = Duration::from_secs(1);

/// What a window shows, as what its last frame drawn was made of.
#[derive(Default)]
pub struct Shown {
    last: Option<Drawn>,

    /// When the last frame ended, drawn or not.
    ended: Option<Instant>,

    /// How long the screen the window is on shows each picture for.
    interval: Option<Duration>,
}

struct Drawn {
    shapes: Vec<ClippedShape>,
    pixels_per_point: f32,
    size: [u32; 2],
    clear_color: [f32; 4],
    at: Instant,
}

impl Shown {
    /// Whether a frame of these shapes would draw what the window shows.
    pub fn unchanged(
        &self,
        shapes: &[ClippedShape],
        pixels_per_point: f32,
        size: [u32; 2],
        clear_color: [f32; 4],
    ) -> bool {
        self.last.as_ref().is_some_and(|last| {
            last.at.elapsed() < REDRAW_ANYWAY
                && last.size == size
                && last.pixels_per_point == pixels_per_point
                && last.clear_color == clear_color
                && last.shapes.as_slice() == shapes
        })
    }

    /// Remembers a frame that is being drawn.
    pub fn drawing(
        &mut self,
        shapes: &[ClippedShape],
        pixels_per_point: f32,
        size: [u32; 2],
        clear_color: [f32; 4],
    ) {
        self.last = Some(Drawn {
            shapes: shapes.to_vec(),
            pixels_per_point,
            size,
            clear_color,
            at: Instant::now(),
        });
    }

    /// Notes that a frame has ended, drawn or not.
    pub fn ended(&mut self) {
        self.ended = Some(Instant::now());
    }

    /// Waits out the rest of the screen's refresh interval after a frame that
    /// was not drawn. A drawn frame waits for the screen when it is presented,
    /// and that is what kept frames to the refresh rate: without it they would
    /// run as fast as the mouse reports moves, a thousand a second for some.
    pub fn pace(&mut self, window: &Window) {
        let interval = *self.interval.get_or_insert_with(|| {
            let millihertz = window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .filter(|&millihertz| millihertz > 0)
                .unwrap_or(60_000);
            Duration::from_secs_f64(1000.0 / millihertz as f64)
        });
        if let Some(since) = self.ended.map(|ended| ended.elapsed()) {
            if since < interval {
                std::thread::sleep(interval - since);
            }
        }
    }

    /// Makes the next frame be drawn.
    pub fn forget(&mut self) {
        self.last = None;
        self.interval = None;
    }

    /// The window has moved, maybe to a screen with another refresh rate. What
    /// it shows moves with it.
    pub fn moved(&mut self) {
        self.interval = None;
    }
}
