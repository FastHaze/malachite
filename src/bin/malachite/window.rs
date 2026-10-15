//! The window's own frame.
//!
//! The system's title bar is not asked for: its room goes to the notes, and
//! what it did is done here. The three buttons of a window (minimise,
//! maximise or restore, close) are drawn at its top right corner; the bars
//! along the top of the window move it when they are dragged by a part that
//! is no button, and fill the screen with it when that is double clicked;
//! and the window is made larger or smaller by its edges.
//!
//! The moving and the resizing themselves are left to the system: the app
//! only says when one is to begin, so that a window dragged to the side of
//! the screen settles there as any other window does.

use eframe::egui::viewport::ResizeDirection;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, ViewportCommand};

use crate::App;

/// How tall the bars along the top of the window are, inside the margins of
/// their panels: as tall as the buttons in them.
pub(crate) const BAR: f32 = 22.0;

/// How far below the top of the window they start.
const TOP: f32 = 8.0;

/// How wide each of the window's three buttons is.
const BUTTON: f32 = 34.0;

/// How much of a bar the three buttons take, from where the bar would
/// otherwise end.
pub(crate) const BUTTONS: f32 = 3.0 * BUTTON;

/// How near an edge of the window the mouse takes hold of that edge, and how
/// far along an edge from a corner it takes hold of the corner.
const EDGE: f32 = 6.0;
const CORNER: f32 = 14.0;

/// The red a close button turns under the mouse, on this window as on any.
const CLOSE: Color32 = Color32::from_rgb(196, 43, 28);

/// One of the window's buttons.
#[derive(Clone, Copy, PartialEq)]
enum Control {
    Minimise,
    Maximise,
    /// What the maximise button is while the window fills the screen.
    Restore,
    Close,
}

impl Control {
    fn name(self) -> &'static str {
        match self {
            Control::Minimise => "Minimise",
            Control::Maximise => "Maximise",
            Control::Restore => "Restore",
            Control::Close => "Close",
        }
    }
}

/// Whether the window fills the screen.
fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|input| input.viewport().maximized.unwrap_or(false))
}

/// Make a handle for the window of the bar that is about to be laid out at
/// the top of this panel: of all of it, from the panel's top edge down, that
/// is not a button. It has to be called before the bar's buttons are added,
/// since what is added later is on top and takes the mouse where it is.
///
/// The bar is also put at the height the bars are at in every panel, so that
/// they make one line across the window whatever margins the panels have.
pub(crate) fn title_bar(ui: &mut egui::Ui) {
    let panel = ui.clip_rect();
    ui.add_space((panel.top() + TOP - ui.cursor().top()).max(0.0));
    let bar = Rect::from_min_max(panel.min, pos2(panel.right(), ui.cursor().top() + BAR));
    let response = ui.interact(bar, ui.id().with("title bar"), Sense::click_and_drag());
    if response.double_clicked() {
        let fill = !maximized(ui.ctx());
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(fill));
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Ask Windows to draw the window as it draws its others: with round corners
/// and a hairline round it. A window without a title bar is otherwise a bare
/// rectangle, which on a dark screen has no edge to be seen. This is one of
/// the app's two calls into the system that no library makes for it; the
/// other is the Recycle Bin's, in `recycle.rs`.
#[cfg(windows)]
pub(crate) fn dress(window: &impl raw_window_handle::HasWindowHandle) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(window: isize, attribute: u32, value: *const u32, size: u32) -> i32;
    }
    /// Which corners a window is to have, and the answer: round ones.
    const CORNER_PREFERENCE: u32 = 33;
    const ROUND: u32 = 2;
    let Ok(raw_window_handle::RawWindowHandle::Win32(handle)) = window.window_handle().map(|handle| handle.as_raw()) else { return };
    // Safety: the window is this program's own and is there, the value is
    // read while the call lasts, and its size is given. A Windows that has
    // no round corners to give (10, and older) says so and changes nothing.
    unsafe {
        DwmSetWindowAttribute(handle.hwnd.get(), CORNER_PREFERENCE, &ROUND, std::mem::size_of::<u32>() as u32);
    }
}

/// Elsewhere a window is left as its system makes it.
#[cfg(not(windows))]
pub(crate) fn dress<W>(_window: &W) {}

/// Which edge or corner of a window that is `screen` the mouse has hold of
/// at `at`, if any.
fn edge(screen: Rect, at: Pos2) -> Option<ResizeDirection> {
    use ResizeDirection::*;
    let near = |distance: f32, reach: f32| (0.0..reach).contains(&distance);
    let (left, right) = (at.x - screen.left(), screen.right() - at.x);
    let (top, bottom) = (at.y - screen.top(), screen.bottom() - at.y);
    if ![left, right, top, bottom].into_iter().any(|distance| near(distance, EDGE)) || !screen.contains(at) {
        return None;
    }
    // A corner is held from a little way along either of its edges.
    Some(match (near(top, CORNER), near(bottom, CORNER), near(left, CORNER), near(right, CORNER)) {
        (true, _, true, _) => NorthWest,
        (true, _, _, true) => NorthEast,
        (_, true, true, _) => SouthWest,
        (_, true, _, true) => SouthEast,
        (true, ..) => North,
        (_, true, ..) => South,
        (_, _, true, _) => West,
        _ => East,
    })
}

/// Whether a window in this state can be resized by its edges: not one that
/// fills the screen.
fn resizable(window: Option<&egui::ViewportInfo>) -> bool {
    !window.is_some_and(|window| window.maximized == Some(true) || window.fullscreen == Some(true))
}

/// Give the mouse the shape that says it is on an edge of the window. To be
/// called when everything has been drawn, so that it is this the mouse goes
/// by and not whatever is drawn at the edge.
pub(crate) fn edge_cursor(ctx: &egui::Context) {
    let held = ctx.input(|input| {
        let at = input.pointer.hover_pos().filter(|_| resizable(Some(input.viewport())))?;
        edge(input.screen_rect(), at)
    });
    if let Some(direction) = held {
        use ResizeDirection::*;
        ctx.set_cursor_icon(match direction {
            North | South => CursorIcon::ResizeVertical,
            East | West => CursorIcon::ResizeHorizontal,
            NorthEast | SouthWest => CursorIcon::ResizeNeSw,
            NorthWest | SouthEast => CursorIcon::ResizeNwSe,
        });
    }
}

impl App {
    /// From what has come in for a frame, before anything is drawn with it:
    /// take a press of the mouse on an edge of the window as taking hold of
    /// that edge. The press goes no further, so that what is drawn at the
    /// edge (a button, the end of a scroll bar) is not pressed as well.
    pub(crate) fn take_edge_press(&mut self, input: &mut egui::RawInput) {
        let Some(screen) = input.screen_rect.filter(|_| resizable(input.viewports.get(&input.viewport_id))) else { return };
        input.events.retain(|event| {
            let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } = event else { return true };
            let held = edge(screen, *pos);
            self.resize = held.or(self.resize);
            held.is_none()
        });
    }

    /// The window's three buttons, flush against the top right corner of the
    /// window. To be called first thing in a bar that is laid out from the
    /// right, in the panel that has that corner.
    pub(crate) fn window_buttons(&mut self, ui: &mut egui::Ui) {
        let panel = ui.clip_rect();
        // The buttons go out through the panel's margins, to the very edges of the window.
        let margin = panel.right() - ui.max_rect().right();
        let (held, _) = ui.allocate_exact_size(vec2(BUTTONS - margin, BAR), Sense::hover());
        let size = if maximized(ui.ctx()) { Control::Restore } else { Control::Maximise };
        for (place, control) in [Control::Close, size, Control::Minimise].into_iter().enumerate() {
            let right = panel.right() - place as f32 * BUTTON;
            let rect = Rect::from_min_max(pos2(right - BUTTON, panel.top()), pos2(right, held.bottom() + 3.0));
            let response = ui.interact(rect, ui.id().with(("window", place)), Sense::click());
            #[cfg(test)]
            self.spots.insert(control.name(), rect);
            let mut ink = ui.visuals().text_color();
            if response.hovered() {
                let closing = control == Control::Close;
                ui.painter().rect_filled(rect, 0.0, if closing { CLOSE } else { ui.visuals().widgets.hovered.weak_bg_fill });
                ink = if closing { Color32::WHITE } else { ui.visuals().strong_text_color() };
            }
            // In line with the other buttons of the bar, not with the middle of its own ground.
            paint(ui.painter(), control, pos2(rect.center().x, held.center().y), ink);
            if response.on_hover_text(control.name()).clicked() {
                ui.ctx().send_viewport_cmd(match control {
                    Control::Minimise => ViewportCommand::Minimized(true),
                    Control::Maximise => ViewportCommand::Maximized(true),
                    Control::Restore => ViewportCommand::Maximized(false),
                    Control::Close => ViewportCommand::Close,
                });
            }
        }
    }
}

/// Draw the sign of one of the window's buttons around `centre`: the signs
/// such buttons have everywhere, ten points across.
fn paint(painter: &egui::Painter, control: Control, centre: Pos2, ink: Color32) {
    let stroke = Stroke::new(1.0, ink);
    let at = |x: f32, y: f32| centre + vec2(x, y);
    match control {
        Control::Minimise => {
            painter.hline(centre.x - 5.0..=centre.x + 5.0, centre.y, stroke);
        }
        Control::Maximise => {
            painter.rect_stroke(Rect::from_min_max(at(-5.0, -5.0), at(5.0, 5.0)), 1.0, stroke, egui::StrokeKind::Inside);
        }
        Control::Restore => {
            // One window in front of another.
            painter.rect_stroke(Rect::from_min_max(at(-5.0, -3.0), at(3.0, 5.0)), 1.0, stroke, egui::StrokeKind::Inside);
            painter.add(Shape::line(vec![at(-3.0, -3.0), at(-3.0, -5.0), at(5.0, -5.0), at(5.0, 3.0), at(3.0, 3.0)], stroke));
        }
        Control::Close => {
            painter.line_segment([at(-5.0, -5.0), at(5.0, 5.0)], stroke);
            painter.line_segment([at(5.0, -5.0), at(-5.0, 5.0)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_and_corners_of_the_window_take_hold_of_it() {
        use ResizeDirection::*;
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let held = |x: f32, y: f32| edge(screen, pos2(x, y));
        assert_eq!(held(400.0, 300.0), None, "the middle of the window is the app's");
        assert_eq!(held(400.0, EDGE), None, "and so is everything further in than the edge reaches");
        assert_eq!((held(400.0, 2.0), held(400.0, 598.0)), (Some(North), Some(South)));
        assert_eq!((held(2.0, 300.0), held(798.0, 300.0)), (Some(West), Some(East)));
        assert_eq!((held(2.0, 2.0), held(798.0, 2.0)), (Some(NorthWest), Some(NorthEast)));
        assert_eq!((held(2.0, 598.0), held(798.0, 598.0)), (Some(SouthWest), Some(SouthEast)));
        assert_eq!(held(790.0, 2.0), Some(NorthEast), "a corner is held from a little way along its edges");
        assert_eq!(held(780.0, 2.0), Some(North));
        assert_eq!(held(790.0, 10.0), None, "but not from inside it");
        assert_eq!(held(-1.0, 300.0), None, "nor is anything held from outside the window");
    }
}
