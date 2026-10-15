//! Autoscroll on a click of the scroll wheel, as Chromium does it on Windows.
//!
//! Pressing the wheel over the note drops an anchor. While it is there the
//! note scrolls towards the mouse, faster the further the mouse is from the
//! anchor. A quick click leaves autoscroll on until the next click, key press
//! or turn of the wheel; holding the wheel down and dragging scrolls only
//! until it is let go.

use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, PointerButton, Pos2, Rect, Sense, Shape, Stroke};

/// Within this distance of the anchor nothing moves (Chromium's radius).
const DEAD_ZONE: f32 = 15.0;

pub struct Autoscroll {
    anchor: Pos2,
    /// The wheel is still held down from the press that started this.
    held: bool,
    /// The mouse left the dead zone while the wheel was held, which makes
    /// this a drag: letting go ends it.
    dragged: bool,
}

/// What the scroll wheel's button is doing.
///
/// Its presses are taken out of the input before the interface sees them:
/// left in, a press would also move the text cursor in the editor or start a
/// selection in Reading view, like any other mouse button.
#[derive(Default)]
pub struct Wheel {
    /// Where the wheel was pressed since the last frame.
    pressed: Option<Pos2>,
    down: bool,
}

impl Wheel {
    /// Take the wheel button's events out of `input`. A press only counts,
    /// and is only taken, when `wanted`: over the note, or with autoscroll on.
    pub fn take_from(&mut self, input: &mut egui::RawInput, wanted: bool) {
        input.events.retain(|event| match event {
            egui::Event::PointerButton { button: PointerButton::Middle, pressed: true, pos, .. } if wanted => {
                self.pressed = Some(*pos);
                self.down = true;
                false
            }
            // The release of a press that was taken goes with it.
            egui::Event::PointerButton { button: PointerButton::Middle, pressed: false, .. } => !std::mem::take(&mut self.down),
            _ => true,
        });
    }
}

/// Scrolling speed in points per second for a mouse `distance` points below
/// (negative: above) the anchor. This is Chromium's curve: gentle near the
/// anchor, steep further out.
pub fn speed(distance: f32) -> f32 {
    if distance.abs() <= DEAD_ZONE {
        return 0.0;
    }
    distance.signum() * 0.008 * distance.abs().powf(2.2)
}

impl Autoscroll {
    /// Run autoscroll over the note area `view` for one frame; `slot` holds
    /// it while it is on. `wheel` is what the wheel's button did, from
    /// [`Wheel::take_from`]. Returns how far down to scroll the note this
    /// frame, in points (negative: up).
    pub fn update(slot: &mut Option<Self>, ui: &egui::Ui, view: Rect, wheel: &mut Wheel) -> f32 {
        let ctx = ui.ctx();
        let pressed = wheel.pressed.take();
        let (pointer, any_pressed, interrupted, dt) = ctx.input(|i| {
            let key = i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
            (i.pointer.latest_pos(), i.pointer.any_pressed(), key || i.raw_scroll_delta != egui::Vec2::ZERO, i.stable_dt)
        });

        let Some(state) = slot else {
            if let Some(anchor) = pressed {
                *slot = Some(Self { anchor, held: true, dragged: false });
                ctx.request_repaint();
            }
            return 0.0;
        };

        let offset = pointer.map_or(egui::Vec2::ZERO, |p| p - state.anchor);
        let stop = if state.held {
            state.dragged |= offset.length() > DEAD_ZONE;
            state.held = wheel.down;
            !wheel.down && state.dragged
        } else {
            pressed.is_some() || any_pressed || interrupted
        };
        if stop {
            *slot = None;
            return 0.0;
        }

        // A layer over the note takes the click that ends autoscroll, so it
        // does not also land on a link or move the text cursor.
        let anchor = state.anchor;
        egui::Area::new(egui::Id::new("autoscroll")).order(egui::Order::Foreground).fixed_pos(view.min).show(ctx, |ui| {
            ui.allocate_rect(view, Sense::click_and_drag());
            paint_anchor(ui, anchor);
        });
        let speed = speed(offset.y);
        ctx.set_cursor_icon(match speed {
            s if s < 0.0 => CursorIcon::ResizeNorth,
            s if s > 0.0 => CursorIcon::ResizeSouth,
            _ => CursorIcon::ResizeVertical,
        });
        // Keep going even while the mouse is still.
        ctx.request_repaint();
        speed * dt
    }
}

/// The mark left where the wheel was pressed: a disc with an arrow up and down.
fn paint_anchor(ui: &egui::Ui, at: Pos2) {
    let visuals = ui.visuals();
    let ink = visuals.text_color();
    let painter = ui.painter();
    painter.circle(at, 14.0, visuals.window_fill.gamma_multiply(0.92), Stroke::new(1.0, ink.gamma_multiply(0.5)));
    painter.circle_filled(at, 1.5, ink);
    for direction in [-1.0, 1.0] {
        let tip = at + vec2(0.0, 10.0 * direction);
        let base = at.y + 5.0 * direction;
        let arrow = vec![tip, pos2(at.x - 4.0, base), pos2(at.x + 4.0, base)];
        painter.add(Shape::convex_polygon(arrow, ink, Stroke::new(0.0, Color32::TRANSPARENT)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_grows_with_distance_from_the_anchor() {
        assert_eq!(speed(0.0), 0.0);
        assert_eq!(speed(15.0), 0.0, "inside the dead zone");
        assert_eq!(speed(-15.0), 0.0);
        assert!(speed(16.0) > 0.0 && speed(-16.0) < 0.0);
        assert_eq!(speed(-80.0), -speed(80.0));
        // Roughly a slow crawl near the anchor and a page a second far from it.
        assert!((30.0..60.0).contains(&speed(50.0)), "{}", speed(50.0));
        assert!((800.0..1100.0).contains(&speed(200.0)), "{}", speed(200.0));
        assert!(speed(300.0) > 2.0 * speed(200.0));
    }
}
