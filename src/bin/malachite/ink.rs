//! Drawing over a note in Reading view: freehand, shapes, text and badges.
//!
//! A round button at the top right of the note turns drawing on and off,
//! and while it is on the tools hang below it, two to a row: pen and
//! highlighter, line and arrow (straight or curved), rectangle and ellipse,
//! text and numbered badges, an eraser, a laser pointer whose trail fades and
//! is never kept, and a selection, then a colour, a width, undo and redo. The
//! tools are ScreenInk's, made the way it makes them (`malachite::ink::shape`).
//! The drawings are the note's, kept in a file of their own beside the
//! vault's settings (`Vault::ink_file`), and `malachite::ink` says how they
//! are written down and what each kind of mark is.
//!
//! Only Reading view draws them, as only there does the note keep still: it
//! has no caret, and no syntax that comes and goes around one. A mark is
//! drawn from the block of the note it was begun on, at the place that block
//! is drawn (`reading::Read::blocks`), so it moves with the block when text
//! is written above it, and is not drawn while a fold hides it. Turning to
//! another view turns drawing off.
//!
//! While drawing is on, a press on the note draws rather than following a
//! link or ticking a box; the wheel still scrolls. Each mark is written to
//! the file as the pointer lets go of it, and an eraser's sweep, a move, an
//! undo and a redo likewise. Undo goes back through the drawings of the open
//! note, and is forgotten when another note is opened.
//!
//! Text is written in a field put where it goes, and becomes a mark when it
//! is finished: with Esc, a press elsewhere, another tool, or drawing being
//! turned off. The selection is of marks: a click picks one (Shift adds or
//! takes away), a drag from nothing picks those it goes round, and a drag of
//! one moves them all. Delete takes them away, a colour recolours them, and
//! Ctrl+] brings them in front of the others. A double click on a text, or a
//! click on it with the text tool, writes in it again. A line or an arrow
//! selected alone shows three handles (`ink::handles`): its ends, and its
//! middle, which bends it so that its curve goes through the handle; a double
//! click on that one straightens it.
//!
//! Every stroke is of one width: on Windows a pen's pressure reaches egui as
//! the force of a touch, but only the pointer is read here.

use std::ops::Range;
use std::path::{Path, PathBuf};

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use malachite::ink::{self, Tool};

use crate::theme::Theme;
use crate::App;

/// What a press on the note does while drawing is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Use {
    #[default]
    Pen,
    Highlighter,
    Line,
    Arrow,
    /// An arrow drawn straight, then bent by the mouse until a click.
    CurvedArrow,
    Rectangle,
    Ellipse,
    Text,
    Badge,
    /// Takes away the marks it passes over, whole.
    Eraser,
    /// A dot to point with, whose trail fades away and is never kept.
    Laser,
    Select,
}

impl Use {
    /// The kind of mark this makes, if it makes one.
    fn tool(self) -> Option<Tool> {
        Some(match self {
            Use::Pen => Tool::Pen,
            Use::Highlighter => Tool::Highlighter,
            Use::Line => Tool::Line,
            Use::Arrow | Use::CurvedArrow => Tool::Arrow,
            Use::Rectangle => Tool::Rectangle,
            Use::Ellipse => Tool::Ellipse,
            Use::Text => Tool::Text,
            Use::Badge => Tool::Badge,
            Use::Eraser | Use::Laser | Use::Select => return None,
        })
    }
}

/// The tools in the order they are offered, two to a row, and what is said of
/// each under the mouse, with its key (`commands.rs` has them, ScreenInk's);
/// a test finds each by the words before its key or the colon.
const TOOLS: [(Use, &str); 12] = [
    (Use::Pen, "Pen (P): hold Alt for a highlighter"),
    (Use::Highlighter, "Highlighter (H)"),
    (Use::Line, "Line (L): hold Shift for steps of 45°; hold Alt as you let go to bend it after"),
    (Use::Arrow, "Arrow (A): hold Shift for steps of 45°; hold Alt as you let go to bend it after"),
    (Use::CurvedArrow, "Curved arrow: drag it out straight, let go, move the mouse to bend it, and click"),
    (Use::Rectangle, "Rectangle (R): hold Shift for a square"),
    (Use::Ellipse, "Ellipse (O): hold Shift for a circle"),
    (Use::Text, "Text (T): click to write, Esc to finish"),
    (Use::Badge, "Numbered badge (N): click to put the next number"),
    (Use::Eraser, "Eraser (E): takes away whole marks"),
    (Use::Laser, "Laser pointer: points, and draws nothing"),
    (Use::Select, "Select (V): click or drag round marks, or Ctrl+A for all, to move, recolour (1–8), delete (Delete) or bring to front (Ctrl+]); a line or arrow alone has handles to move its ends and bend it"),
];

/// The sizes offered, smallest first, by what they are of: a pen's or a
/// shape's line, a highlighter's, a text's letters, a badge's radius.
const PEN_WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
const HIGHLIGHTER_WIDTHS: [f32; 3] = [10.0, 16.0, 26.0];
const TEXT_SIZES: [f32; 3] = [14.0, 20.0, 32.0];
const BADGE_RADII: [f32; 3] = [13.0, 18.0, 28.0];

/// How much of the page a highlighter's stroke lets through.
const SEEN_THROUGH: u8 = 96;
/// How dark the veil over the note is while drawing.
const DIM: u8 = 90;

/// The laser's colour, how long its trail takes to fade, and how wide it is.
const LASER: Color32 = Color32::from_rgb(255, 48, 48);
const LASER_FADE: f64 = 0.8;
const LASER_WIDTH: f32 = 5.0;

/// How far from the eraser a mark is taken away, besides its own width, and
/// how far from a mark a click still picks it.
const ERASER_REACH: f32 = 6.0;
const PICK_REACH: f32 = 4.0;
/// How big a line's handles are drawn, and how far from one a press still holds it.
const HANDLE: f32 = 4.5;
const HANDLE_REACH: f32 = 8.0;

/// The round button's size, and how far it is kept in from the note's corner.
const BUTTON: f32 = 30.0;
const INSET: f32 = 26.0;

/// The colours to draw in: the colour of the text, then the highlight colours.
fn colours(theme: &Theme) -> Vec<Color32> {
    let highlights = crate::editor::HIGHLIGHTS.iter().map(|(_, _, [r, g, b])| Color32::from_rgb(*r, *g, *b));
    std::iter::once(theme.text).chain(highlights).collect()
}

/// The drawing tools, and the drawings of the open note.
#[derive(Default)]
pub(crate) struct Ink {
    /// Whether a press on the note draws.
    pub on: bool,
    pub tool: Use,
    /// Which of `colours` and of the widths, by their place.
    pub colour: usize,
    pub width: usize,
    /// The note whose drawings these are.
    note: Option<PathBuf>,
    /// Its file of drawings could not be read, so it is not to be written
    /// over: that would lose what is in it.
    unreadable: bool,
    pub(crate) strokes: Vec<ink::Stroke>,
    /// Each mark's outline (`ink::outline`), from its anchor.
    paths: Vec<Vec<Pos2>>,
    /// The drawings as they were before each change, latest last.
    undo: Vec<Vec<ink::Stroke>>,
    redo: Vec<Vec<ink::Stroke>>,
    /// Where each stroke's line starts in the note's text, if it is still
    /// there, worked out for the text kept with it.
    placed: Option<(String, Vec<Option<usize>>)>,
    /// What the pointer is doing on the note.
    pressed: Option<Pressed>,
    /// Where the laser has been on the screen lately, when, and whether
    /// that was where a press began, which is not joined to what was before.
    pub(crate) trail: Vec<(Pos2, f64, bool)>,
    /// Whether the laser was pressed the frame before.
    pointing: bool,
    /// The marks selected, by their place in `strokes`.
    pub(crate) selected: Vec<usize>,
    /// A text being written.
    pub(crate) typing: Option<Typing>,
    /// The colours offered, as the tools last showed them, for the keys 1 to 8.
    palette: Vec<Color32>,
    /// A line or an arrow drawn straight and let go of, being bent through
    /// where the mouse is until a click, from the block at the byte `.0`.
    pub(crate) curving: Option<(usize, ink::Stroke)>,
}

/// A text being written over the note, not yet a mark.
pub(crate) struct Typing {
    /// The byte of the note whose block it is drawn from, and where from there.
    at: usize,
    offset: Vec2,
    pub(crate) text: String,
    color: [u8; 4],
    size: f32,
    /// The text mark it is written in place of, if it was one already.
    replacing: Option<usize>,
    /// Not yet given the keyboard.
    fresh: bool,
}

/// A press on the note that has not been let go of.
enum Pressed {
    /// A stroke being drawn, from the block that starts at this byte of the note.
    Drawing(usize, ink::Stroke),
    /// A shape being dragged out from a point, from the block at this byte.
    Shaping(usize, [f32; 2], ink::Stroke),
    /// An eraser being swept, and the drawings as they were before it.
    Erasing(Vec<ink::Stroke>),
    /// The selected marks being moved, the drawings as they were before,
    /// where the pointer was the frame before, and the mark pressed on, which
    /// a click that moves nothing leaves selected alone.
    Moving(Vec<ink::Stroke>, Pos2, Option<usize>),
    /// A handle of the line or arrow `.1` being dragged (`ink::handles`, by
    /// its place `.2`), and the drawings as they were before.
    Bending(Vec<ink::Stroke>, usize, usize),
    /// A box being dragged from a point, and what was selected before it.
    Marquee(Pos2, Vec<usize>),
    /// A press that has done what it does already.
    Done,
}

impl Ink {
    /// Have the drawings of the note in `file` at hand.
    fn load(&mut self, vault: &malachite::Vault, file: &Path) -> anyhow::Result<()> {
        if self.note.as_deref() == Some(file) {
            return Ok(());
        }
        self.note = Some(file.to_path_buf());
        (self.undo, self.redo, self.pressed, self.selected, self.typing, self.curving) = (Vec::new(), Vec::new(), None, Vec::new(), None, None);
        let read = vault.read_ink(file);
        self.unreadable = read.is_err();
        self.set(read.as_ref().map_or_else(|_| Vec::new(), Clone::clone));
        read.map(|_| ())
    }

    /// Put `strokes` in place of the drawings there are.
    fn set(&mut self, strokes: Vec<ink::Stroke>) {
        self.paths = strokes.iter().map(outline).collect();
        self.strokes = strokes;
        self.placed = None;
    }

    /// Where each stroke's line starts in `text`.
    fn placed(&mut self, text: &str) -> &[Option<usize>] {
        if self.placed.as_ref().is_none_or(|(kept, _)| kept != text) {
            let starts = self.strokes.iter().map(|stroke| ink::line_of(text, &stroke.anchor, stroke.nth)).collect();
            self.placed = Some((text.to_owned(), starts));
        }
        &self.placed.as_ref().unwrap().1
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// A mark's outline, from its anchor.
fn outline(stroke: &ink::Stroke) -> Vec<Pos2> {
    ink::outline(stroke).into_iter().map(|[x, y]| pos2(x, y)).collect()
}

/// Whether a mark being made has the points its kind needs to be drawn.
fn whole(stroke: &ink::Stroke) -> bool {
    let needs = match stroke.tool {
        Tool::Line | Tool::Arrow | Tool::Rectangle | Tool::Ellipse => 2,
        _ => 1,
    };
    stroke.points.len() >= needs
}

fn rgba([r, g, b, a]: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn at(point: [f32; 2]) -> Vec2 {
    vec2(point[0], point[1])
}

/// The block of `blocks` that the byte `at` of the note is in, if it is drawn.
fn block_of(blocks: &[(Range<usize>, egui::Rangef)], at: usize) -> Option<&(Range<usize>, egui::Rangef)> {
    let after = blocks.partition_point(|(lines, _)| lines.start <= at);
    after.checked_sub(1).map(|i| &blocks[i]).filter(|(lines, _)| at <= lines.end)
}

/// The block under the height `y`, or the nearest one.
fn block_near(blocks: &[(Range<usize>, egui::Rangef)], y: f32) -> Option<&(Range<usize>, egui::Rangef)> {
    let distance = |rows: &egui::Rangef| if rows.contains(y) { 0.0 } else { (rows.min - y).abs().min((rows.max - y).abs()) };
    blocks.iter().min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1)))
}

/// How big a badge's number is written, for one of `radius`: smaller as it is longer.
fn badge_font(label: &str, radius: f32) -> FontId {
    let length = label.chars().count().max(1) as f32;
    FontId::proportional(radius * if length <= 2.0 { 1.1 } else { 2.2 / length })
}

/// White on a dark colour, black on a light one.
fn contrasting(colour: Color32) -> Color32 {
    let [r, g, b, _] = colour.to_srgba_unmultiplied().map(|c| c as f32 / 255.0);
    if 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.6 { Color32::BLACK } else { Color32::WHITE }
}

/// What a highlighter's colour, which lets the page through, comes out as
/// on `page`: the same colour, solid, for a stroke drawn under the text.
fn on_page([r, g, b, a]: [u8; 4], page: Color32) -> Color32 {
    let a = a as f32 / 255.0;
    let mix = |over: u8, under: u8| (over as f32 * a + under as f32 * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(r, page.r()), mix(g, page.g()), mix(b, page.b()))
}

/// Draw `stroke`, whose outline is `path`, from `origin`. A highlighter's
/// stroke goes in `under`, to be drawn under the text, solid: drawn seen
/// through, it was darker wherever it went over itself, as egui lays over
/// each other the pieces of a line that turns or crosses itself.
fn paint(painter: &egui::Painter, stroke: &ink::Stroke, path: &[Pos2], origin: Pos2, under: &mut Vec<Shape>, page: Color32) {
    let colour = rgba(stroke.color);
    let line = Stroke::new(stroke.width, colour);
    let points: Vec<Pos2> = path.iter().map(|point| origin + point.to_vec2()).collect();
    let point = |i: usize| origin + at(stroke.points[i]);
    let round_ends = |points: &[Pos2]| {
        for end in [points.first(), points.last()].into_iter().flatten() {
            painter.circle_filled(*end, stroke.width / 2.0, colour);
        }
    };
    match stroke.tool {
        Tool::Highlighter => {
            let solid = on_page(stroke.color, page);
            under.push(match &points[..] {
                [only] => Shape::circle_filled(*only, stroke.width / 2.0, solid),
                _ => Shape::line(points, Stroke::new(stroke.width, solid)),
            });
        }
        Tool::Pen => match &points[..] {
            [] => {}
            [only] => {
                painter.circle_filled(*only, stroke.width / 2.0, colour);
            }
            _ => {
                round_ends(&points);
                painter.add(Shape::line(points, line));
            }
        },
        Tool::Line => {
            round_ends(&points);
            painter.add(Shape::line(points, line));
        }
        Tool::Arrow => {
            if !points.is_empty() {
                painter.circle_filled(points[0], stroke.width / 2.0, colour);
                painter.add(Shape::line(points, line));
            }
            if let Some(head) = ink::arrow_head(stroke) {
                painter.add(Shape::convex_polygon(head.iter().map(|p| origin + at(*p)).collect(), colour, Stroke::NONE));
            }
        }
        Tool::Rectangle => {
            let rect = Rect::from_two_pos(point(0), point(1));
            let radius = 6.0f32.min(rect.width().min(rect.height()) / 2.0);
            painter.rect_stroke(rect, radius, line, egui::StrokeKind::Middle);
        }
        Tool::Ellipse => {
            painter.add(Shape::closed_line(points[..points.len().saturating_sub(1)].to_vec(), line));
        }
        Tool::Text => {
            painter.text(point(0), Align2::LEFT_TOP, &stroke.text, FontId::proportional(stroke.width), colour);
        }
        Tool::Badge => {
            painter.circle_filled(point(0), stroke.width, colour);
            painter.text(point(0), Align2::CENTER_CENTER, &stroke.text, badge_font(&stroke.text, stroke.width), contrasting(colour));
        }
    }
}

/// The box a mark takes, from its anchor.
fn extent(painter: &egui::Painter, stroke: &ink::Stroke, path: &[Pos2]) -> Rect {
    let point = at(stroke.points[0]).to_pos2();
    match stroke.tool {
        Tool::Text => {
            let galley = painter.layout_no_wrap(stroke.text.clone(), FontId::proportional(stroke.width), Color32::WHITE);
            Rect::from_min_size(point, galley.size())
        }
        Tool::Badge => Rect::from_center_size(point, Vec2::splat(stroke.width * 2.0)),
        _ => {
            let head = ink::arrow_head(stroke).into_iter().flatten().map(|p| at(p).to_pos2());
            let corners = stroke.points.iter().take(2).filter(|_| stroke.tool == Tool::Rectangle).map(|p| at(*p).to_pos2());
            let points: Vec<Pos2> = path.iter().copied().chain(head).chain(corners).collect();
            Rect::from_points(&points).expand(stroke.width / 2.0)
        }
    }
}

/// Whether `point`, from the mark's anchor, is on the mark or within `reach` of it.
fn hits(painter: &egui::Painter, stroke: &ink::Stroke, path: &[Pos2], point: Vec2, reach: f32) -> bool {
    if stroke.tool.has_text() {
        return extent(painter, stroke, path).expand(reach).contains(point.to_pos2());
    }
    let flat = |points: &[Pos2]| points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
    let reach = reach + stroke.width / 2.0;
    let head = ink::arrow_head(stroke).map(|[a, b, c]| vec![a, b, c, a]).unwrap_or_default();
    ink::near(&flat(path), [point.x, point.y], reach) || ink::near(&head, [point.x, point.y], reach)
}

impl App {
    /// Draw the open note's drawings over Reading view, which has just drawn
    /// its `blocks` in `ui`, and while drawing is on, draw with the pointer.
    /// The highlighters' strokes go in `under`, a place kept before the text.
    pub(crate) fn ink_layer(&mut self, ui: &mut egui::Ui, file: &Path, blocks: &[(Range<usize>, egui::Rangef)], theme: &Theme, under: egui::layers::ShapeIdx) {
        if self.ink.note.as_deref() != Some(file) {
            self.finish_typing();
            self.finish_curving();
        }
        if let Err(e) = self.ink.load(&self.vault, file) {
            self.fail(format!("Could not read the drawings over this note: {e:#}"));
        }
        self.ink.palette = colours(theme);
        let left = ui.max_rect().left();
        let painter = ui.painter().clone();
        if self.ink.on {
            // The note is dimmed under the marks while drawing, so they stand out.
            painter.rect_filled(ui.clip_rect(), 0.0, Color32::from_black_alpha(DIM));
        }
        let origin = |at: usize| block_of(blocks, at).map(|(_, rows)| pos2(left, rows.min));

        let placed = self.ink.placed(&self.buffer).to_vec();
        let origins: Vec<Option<Pos2>> = placed.iter().map(|at| at.and_then(origin)).collect();
        let count = self.ink.strokes.len();
        self.ink.selected.retain(|&i| i < count);
        let hidden = self.ink.typing.as_ref().and_then(|typing| typing.replacing);
        let mut highlights = Vec::new();
        for (i, (stroke, path)) in self.ink.strokes.iter().zip(&self.ink.paths).enumerate() {
            if let Some(origin) = origins[i].filter(|_| Some(i) != hidden) {
                paint(&painter, stroke, path, origin, &mut highlights, theme.page);
            }
        }
        if let Some(Pressed::Drawing(at, stroke) | Pressed::Shaping(at, _, stroke)) = &self.ink.pressed {
            if let Some(origin) = origin(*at).filter(|_| whole(stroke)) {
                paint(&painter, stroke, &outline(stroke), origin, &mut highlights, theme.page);
            }
        }
        // A line let go of straight follows the mouse with its middle, until a click.
        if let Some((block, stroke)) = &mut self.ink.curving {
            if let Some(origin) = origin(*block) {
                // The middle of its curve is where the mouse is.
                if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                    ink::move_handle(stroke, 2, [mouse.x - origin.x, mouse.y - origin.y]);
                }
                paint(&painter, stroke, &outline(stroke), origin, &mut highlights, theme.page);
                if let Some([_, _, middle]) = ink::handles(stroke) {
                    painter.circle_filled(origin + at(middle), HANDLE, theme.accent);
                }
            }
        }
        painter.set(under, Shape::Vec(highlights));
        let accent = Stroke::new(1.0, theme.accent);
        for &i in &self.ink.selected {
            if let Some(origin) = origins[i] {
                let rect = extent(&painter, &self.ink.strokes[i], &self.ink.paths[i]).translate(origin.to_vec2()).expand(4.0);
                let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
                painter.extend(Shape::dashed_line(&corners, accent, 4.0, 3.0));
            }
        }
        // A line or an arrow selected alone is held by its handles: its ends, and its middle to bend it by.
        let handles = self.handles_at(&origins);
        if let Some((_, handles)) = handles {
            for (n, handle) in handles.into_iter().enumerate() {
                let fill = if n == 2 { theme.accent } else { theme.page };
                painter.circle(handle, HANDLE, fill, Stroke::new(1.5, theme.accent));
            }
        }
        let handle_at = |point: Pos2| handles.and_then(|(i, handles)| Some((i, handles.iter().position(|h| h.distance(point) <= HANDLE_REACH)?)));

        if !self.ink.on {
            self.finish_typing();
            self.finish_curving();
            self.ink.pressed = None;
            self.ink.selected.clear();
            return;
        }
        // Over everything Reading view has drawn, so that a press here draws.
        let area = ui.clip_rect();
        let response = ui.interact(area, ui.id().with("ink"), Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hit = |app: &App, point: Pos2, reach: f32| {
            (0..app.ink.strokes.len()).rev().find(|&i| {
                let Some(origin) = origins[i] else { return false };
                Some(i) != hidden && hits(&painter, &app.ink.strokes[i], &app.ink.paths[i], point - origin, reach)
            })
        };
        if let Some(hover) = response.hover_pos() {
            let cursor = match self.ink.tool {
                Use::Eraser => egui::CursorIcon::Cell,
                Use::Laser => egui::CursorIcon::None,
                Use::Text => egui::CursorIcon::Text,
                Use::Select if handle_at(hover).is_some() => egui::CursorIcon::Grab,
                Use::Select if hit(self, hover, PICK_REACH).is_some() => egui::CursorIcon::Move,
                Use::Select => egui::CursorIcon::Default,
                _ => egui::CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        if self.ink.tool == Use::Laser {
            self.ink.pressed = None;
            return self.laser(ui, &response);
        }
        // After the layer, so that a press in the field is the field's.
        self.typing_field(ui, &origin);
        if !response.is_pointer_button_down_on() {
            // A curved line or arrow is let go of straight, and then bent by the mouse.
            let alt = ui.input(|i| i.modifiers.alt);
            if let Some(Pressed::Shaping(_, _, stroke)) = &self.ink.pressed {
                if whole(stroke) && matches!(stroke.tool, Tool::Line | Tool::Arrow) && (self.ink.tool == Use::CurvedArrow || alt) {
                    if let Some(Pressed::Shaping(block, _, stroke)) = self.ink.pressed.take() {
                        self.ink.curving = Some((block, stroke));
                    }
                    return;
                }
            }
            self.settle(&painter, blocks, left, &origin, &origins);
            self.let_go();
            // egui knows a click is a double one only as it is let go. A double
            // click on a line's middle handle straightens it; on a text, writes in it.
            let double = response.double_clicked().then(|| response.interact_pointer_pos()).flatten();
            if let Some(at) = double.filter(|_| self.ink.tool == Use::Select) {
                if let Some((i, 2)) = handle_at(at) {
                    let mut strokes = self.ink.strokes.clone();
                    strokes[i].points.truncate(2);
                    if strokes != self.ink.strokes {
                        self.change_ink(strokes);
                    }
                } else if let Some(i) = hit(self, at, PICK_REACH).filter(|&i| self.ink.strokes[i].tool == Tool::Text) {
                    self.write_in(i, placed[i]);
                }
            }
            return;
        }
        let Some(pointer) = pointer else { return };
        if self.ink.pressed.is_none() {
            let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
            let shift = ui.input(|i| i.modifiers.shift);
            // A click fixes the bend of a line being bent, and does no more.
            if let Some((block, stroke)) = self.ink.curving.take() {
                self.ink.pressed = Some(Pressed::Shaping(block, [0.0, 0.0], stroke));
                self.settle(&painter, blocks, left, &origin, &origins);
                self.let_go();
                self.ink.pressed = Some(Pressed::Done);
                return;
            }
            // A press elsewhere finishes a text; with the text tool, that is all it does.
            if self.ink.typing.is_some() {
                self.finish_typing();
                if self.ink.tool == Use::Text {
                    self.ink.pressed = Some(Pressed::Done);
                    return;
                }
            }
            let Some((lines, rows)) = block_near(blocks, press.y).cloned() else { return };
            let from = press - pos2(left, rows.min);
            self.ink.pressed = Some(match self.ink.tool {
                Use::Eraser => Pressed::Erasing(self.ink.strokes.clone()),
                Use::Pen | Use::Highlighter => {
                    // Alt held with the pen is a highlighter for as long as the stroke.
                    let pen = self.ink.tool;
                    if pen == Use::Pen && ui.input(|i| i.modifiers.alt) {
                        self.ink.tool = Use::Highlighter;
                    }
                    let stroke = self.mark(theme, lines.start, vec![[from.x, from.y]], String::new());
                    self.ink.tool = pen;
                    Pressed::Drawing(lines.start, stroke)
                }
                Use::Badge => {
                    let number = ink::next_badge(&self.ink.strokes).to_string();
                    let badge = self.mark(theme, lines.start, vec![[from.x, from.y]], number);
                    let mut strokes = self.ink.strokes.clone();
                    strokes.push(badge);
                    self.change_ink(strokes);
                    Pressed::Done
                }
                Use::Text => {
                    match hit(self, press, PICK_REACH).filter(|&i| self.ink.strokes[i].tool == Tool::Text) {
                        Some(i) => self.write_in(i, placed[i]),
                        None => {
                            let size = TEXT_SIZES[self.ink.width.min(TEXT_SIZES.len() - 1)];
                            // The click is at the middle of the first line, where the caret shows.
                            let offset = from - vec2(0.0, size * 0.6);
                            let color = self.mark(theme, lines.start, Vec::new(), String::new()).color;
                            self.ink.typing = Some(Typing { at: lines.start, offset, text: String::new(), color, size, replacing: None, fresh: true });
                        }
                    }
                    Pressed::Done
                }
                Use::Select if handle_at(press).is_some() => {
                    let (i, n) = handle_at(press).unwrap();
                    Pressed::Bending(self.ink.strokes.clone(), i, n)
                }
                Use::Select => match hit(self, press, PICK_REACH) {
                    Some(i) => {
                        if shift {
                            match self.ink.selected.iter().position(|&s| s == i) {
                                Some(place) => drop(self.ink.selected.remove(place)),
                                None => self.ink.selected.push(i),
                            }
                        } else if !self.ink.selected.contains(&i) {
                            self.ink.selected = vec![i];
                        }
                        Pressed::Moving(self.ink.strokes.clone(), press, Some(i).filter(|_| !shift))
                    }
                    None => {
                        let kept = if shift { self.ink.selected.clone() } else { Vec::new() };
                        self.ink.selected = kept.clone();
                        Pressed::Marquee(press, kept)
                    }
                },
                Use::Laser => Pressed::Done,
                _ => Pressed::Shaping(lines.start, [from.x, from.y], self.mark(theme, lines.start, vec![[from.x, from.y]], String::new())),
            });
        }
        let shift = ui.input(|i| i.modifiers.shift);
        match &mut self.ink.pressed {
            Some(Pressed::Drawing(at, stroke)) => {
                if let Some(origin) = origin(*at) {
                    let point = pointer - origin;
                    let last = stroke.points.last().map_or(Pos2::ZERO, |[x, y]| pos2(*x, *y));
                    // Finer than this is only the hand shaking.
                    if (point.to_pos2() - last).length() >= 0.75 {
                        stroke.points.push([point.x, point.y]);
                    }
                }
            }
            Some(Pressed::Shaping(at, start, stroke)) => {
                if let Some(origin) = origin(*at) {
                    let end = pointer - origin;
                    stroke.points = ink::shape(stroke.tool, *start, [end.x, end.y], shift).unwrap_or_else(|| vec![*start]);
                }
            }
            Some(Pressed::Moving(_, last, _)) => {
                let delta = pointer - *last;
                *last = pointer;
                if delta != Vec2::ZERO {
                    let mut strokes = std::mem::take(&mut self.ink.strokes);
                    for &i in &self.ink.selected {
                        for point in &mut strokes[i].points {
                            *point = [point[0] + delta.x, point[1] + delta.y];
                        }
                    }
                    self.ink.set(strokes);
                }
            }
            Some(Pressed::Bending(_, i, n)) => {
                if let Some(origin) = origins[*i] {
                    let to = pointer - origin;
                    let mut strokes = std::mem::take(&mut self.ink.strokes);
                    ink::move_handle(&mut strokes[*i], *n, [to.x, to.y]);
                    self.ink.set(strokes);
                }
            }
            Some(Pressed::Marquee(from, kept)) => {
                let rect = Rect::from_two_pos(*from, pointer);
                let mut selected = kept.clone();
                for i in 0..self.ink.strokes.len() {
                    let Some(origin) = origins[i] else { continue };
                    let inside = extent(&painter, &self.ink.strokes[i], &self.ink.paths[i]).translate(origin.to_vec2()).intersects(rect);
                    if inside && !selected.contains(&i) {
                        selected.push(i);
                    }
                }
                self.ink.selected = selected;
                painter.rect(rect, 0.0, theme.accent.gamma_multiply(0.08), accent, egui::StrokeKind::Inside);
            }
            Some(Pressed::Erasing(_)) => {
                let gone: Vec<usize> = (0..self.ink.strokes.len()).filter(|&i| {
                    let Some(origin) = origins[i] else { return false };
                    hits(&painter, &self.ink.strokes[i], &self.ink.paths[i], pointer - origin, ERASER_REACH)
                }).collect();
                if !gone.is_empty() {
                    let kept = self.ink.strokes.iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, s)| s.clone()).collect();
                    self.ink.selected.clear();
                    self.ink.set(kept);
                }
            }
            Some(Pressed::Done) | None => {}
        }
    }

    /// As the pointer lets go, give a mark just made, or the marks just moved,
    /// the line under their middle, which they are drawn from from now on:
    /// they were drawn from where the press began, and a mark that is mostly
    /// over the lines under a heading should fold away with them.
    fn settle(
        &mut self,
        painter: &egui::Painter,
        blocks: &[(Range<usize>, egui::Rangef)],
        left: f32,
        origin: &impl Fn(usize) -> Option<Pos2>,
        origins: &[Option<Pos2>],
    ) {
        let again = |buffer: &str, stroke: &mut ink::Stroke, from: Pos2| {
            let middle = extent(painter, stroke, &outline(stroke)).center() + from.to_vec2();
            let Some((lines, rows)) = block_near(blocks, middle.y) else { return };
            let shift = from - pos2(left, rows.min);
            for point in &mut stroke.points {
                *point = [point[0] + shift.x, point[1] + shift.y];
            }
            (stroke.anchor, stroke.nth) = ink::anchor_at(buffer, lines.start);
        };
        match &mut self.ink.pressed {
            Some(Pressed::Drawing(at, stroke) | Pressed::Shaping(at, _, stroke)) if whole(stroke) => {
                if let Some(from) = origin(*at) {
                    again(&self.buffer, stroke, from);
                }
            }
            Some(Pressed::Moving(before, _, _) | Pressed::Bending(before, _, _)) if *before != self.ink.strokes => {
                let mut strokes = std::mem::take(&mut self.ink.strokes);
                for &i in &self.ink.selected {
                    if let Some(from) = origins[i] {
                        again(&self.buffer, &mut strokes[i], from);
                    }
                }
                self.ink.set(strokes);
            }
            _ => {}
        }
    }

    /// The mark whose handles are showing, and where they are on the screen:
    /// a line or an arrow, selected alone with the selection in hand.
    fn handles_at(&self, origins: &[Option<Pos2>]) -> Option<(usize, [Pos2; 3])> {
        let [i] = self.ink.selected[..] else { return None };
        // Not while the line is being moved, or a box dragged.
        let showing = self.ink.tool == Use::Select && !matches!(self.ink.pressed, Some(Pressed::Moving(..) | Pressed::Marquee(..)));
        let origin = origins[i].filter(|_| showing)?;
        let handles = ink::handles(&self.ink.strokes[i])?;
        Some((i, handles.map(|h| origin + at(h))))
    }

    /// A new mark made with the tool in hand, in its colour and width, from
    /// the block at the byte `at` of the note.
    fn mark(&self, theme: &Theme, at: usize, points: Vec<[f32; 2]>, text: String) -> ink::Stroke {
        let (anchor, nth) = ink::anchor_at(&self.buffer, at);
        let tool = self.ink.tool.tool().unwrap_or(Tool::Pen);
        let colours = colours(theme);
        let [r, g, b, _] = colours[self.ink.colour.min(colours.len() - 1)].to_srgba_unmultiplied();
        let widths = match tool {
            Tool::Highlighter => HIGHLIGHTER_WIDTHS,
            Tool::Text => TEXT_SIZES,
            Tool::Badge => BADGE_RADII,
            _ => PEN_WIDTHS,
        };
        let color = [r, g, b, if tool == Tool::Highlighter { SEEN_THROUGH } else { 255 }];
        ink::Stroke { anchor, nth, tool, color, width: widths[self.ink.width.min(widths.len() - 1)], points, text }
    }

    /// Write in the text mark `i` again, whose line starts at the byte `line`.
    fn write_in(&mut self, i: usize, line: Option<usize>) {
        let Some(line) = line else { return };
        let stroke = &self.ink.strokes[i];
        self.ink.selected.clear();
        self.ink.typing = Some(Typing {
            at: line,
            offset: at(stroke.points[0]),
            text: stroke.text.clone(),
            color: stroke.color,
            size: stroke.width,
            replacing: Some(i),
            fresh: true,
        });
    }

    /// The field a text is written in, where it will be drawn.
    fn typing_field(&mut self, ui: &mut egui::Ui, origin: &impl Fn(usize) -> Option<Pos2>) {
        let Some(typing) = &mut self.ink.typing else { return };
        let Some(origin) = origin(typing.at) else { return };
        let colour = rgba(typing.color);
        let font = FontId::proportional(typing.size);
        // As wide as the text, and a letter more to type into.
        let galley = ui.painter().layout_no_wrap(format!("{} ", typing.text), font.clone(), colour);
        let rect = Rect::from_min_size(origin + typing.offset, galley.size() + vec2(typing.size, 0.0));
        let id = egui::Id::new("ink text");
        let field = egui::TextEdit::multiline(&mut typing.text)
            .id(id)
            .frame(false)
            .font(font)
            .text_color(colour)
            .margin(egui::Margin::ZERO)
            .desired_width(rect.width())
            .desired_rows(1);
        let response = ui.put(rect, field);
        if std::mem::take(&mut typing.fresh) {
            response.request_focus();
            // The caret at the end, to write on.
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let end = egui::text::CCursor::new(typing.text.chars().count());
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
            state.store(ui.ctx(), id);
        }
    }

    /// Make the text being written a mark, or with nothing in it, none.
    pub(crate) fn finish_typing(&mut self) {
        let Some(typing) = self.ink.typing.take() else { return };
        let text = typing.text.trim().to_owned();
        let mut strokes = self.ink.strokes.clone();
        match typing.replacing {
            Some(i) if i >= strokes.len() => return,
            Some(i) if text.is_empty() => drop(strokes.remove(i)),
            Some(i) => strokes[i].text = text,
            None if text.is_empty() => return,
            None => {
                let (anchor, nth) = ink::anchor_at(&self.buffer, typing.at);
                let points = vec![[typing.offset.x, typing.offset.y]];
                strokes.push(ink::Stroke { anchor, nth, tool: Tool::Text, color: typing.color, width: typing.size, points, text });
            }
        }
        if strokes != self.ink.strokes {
            self.change_ink(strokes);
        }
    }

    /// Keep a line being bent as it is bent now, without the click that would fix it.
    pub(crate) fn finish_curving(&mut self) {
        if let Some((_, stroke)) = self.ink.curving.take() {
            let mut strokes = self.ink.strokes.clone();
            strokes.push(stroke);
            self.change_ink(strokes);
        }
    }

    /// Whether a text is being written over the note, and so has the keyboard.
    pub(crate) fn typing_ink(&self) -> bool {
        self.ink.typing.is_some()
    }

    /// Take away the selected marks.
    pub(crate) fn delete_selected_ink(&mut self) {
        let selected = std::mem::take(&mut self.ink.selected);
        if selected.is_empty() {
            return;
        }
        let kept = self.ink.strokes.iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, s)| s.clone()).collect();
        self.change_ink(kept);
    }

    /// Take `tool` in hand, finishing what the one before was doing.
    pub(crate) fn choose_tool(&mut self, tool: Use) {
        if self.ink.tool == tool {
            return;
        }
        self.finish_typing();
        self.finish_curving();
        if tool != Use::Select {
            self.ink.selected.clear();
        }
        self.ink.tool = tool;
    }

    /// Draw in the colour at `place` among those offered; what is selected is
    /// recoloured, and a tool that draws nothing gives way to the pen.
    pub(crate) fn pick_colour(&mut self, place: usize) {
        let Some(&colour) = self.ink.palette.get(place) else { return };
        self.ink.colour = place;
        if !self.ink.selected.is_empty() {
            self.recolour_ink(colour);
        } else if matches!(self.ink.tool, Use::Eraser | Use::Laser | Use::Select) {
            self.choose_tool(Use::Pen);
        }
    }

    /// Select every mark over the open note, with the selection in hand.
    pub(crate) fn select_all_ink(&mut self) {
        if self.ink.note == self.open_path {
            self.choose_tool(Use::Select);
            self.ink.selected = (0..self.ink.strokes.len()).collect();
        }
    }

    /// Take away every mark over the open note, as one step to undo.
    pub(crate) fn erase_drawings(&mut self) {
        // The drawings at hand may be of a note shown before, if this one has not been read yet.
        if self.ink.note != self.open_path {
            return;
        }
        self.finish_typing();
        self.ink.curving = None;
        self.ink.selected.clear();
        if !self.ink.strokes.is_empty() {
            self.change_ink(Vec::new());
        }
    }

    /// Bring the selected marks in front of the others, keeping their order.
    pub(crate) fn bring_ink_to_front(&mut self) {
        let mut selected = self.ink.selected.clone();
        selected.sort_unstable();
        let (front, rest): (Vec<_>, Vec<_>) = self.ink.strokes.iter().cloned().enumerate().partition(|(i, _)| selected.contains(i));
        let count = rest.len();
        let strokes: Vec<ink::Stroke> = rest.into_iter().chain(front).map(|(_, s)| s).collect();
        if strokes != self.ink.strokes {
            self.change_ink(strokes);
            self.ink.selected = (count..self.ink.strokes.len()).collect();
        }
    }

    /// Give the selected marks the colour `colour`, each keeping how much it lets through.
    fn recolour_ink(&mut self, colour: Color32) {
        let [r, g, b, _] = colour.to_srgba_unmultiplied();
        let mut strokes = self.ink.strokes.clone();
        for &i in &self.ink.selected {
            let a = strokes[i].color[3];
            strokes[i].color = [r, g, b, a];
        }
        if strokes != self.ink.strokes {
            self.change_ink(strokes);
        }
    }

    /// The laser: a glowing dot in place of the mouse, and while pressed, a
    /// trail behind it that fades out. It is held on the screen, not the note.
    fn laser(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let now = ui.input(|i| i.time);
        let trail = &mut self.ink.trail;
        trail.retain(|(_, at, _)| now - at < LASER_FADE);
        let down = response.is_pointer_button_down_on();
        if down {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let begins = !self.ink.pointing;
                if begins || trail.last().is_none_or(|(last, _, _)| (*last - pointer).length() >= 0.75) {
                    trail.push((pointer, now, begins));
                }
            }
        }
        self.ink.pointing = down;
        let painter = ui.painter();
        for pair in trail.windows(2).filter(|pair| !pair[1].2) {
            let left = (1.0 - (now - pair[0].1) / LASER_FADE) as f32;
            painter.line_segment([pair[0].0, pair[1].0], Stroke::new(LASER_WIDTH * left, LASER.gamma_multiply(left)));
        }
        if let Some(pointer) = response.hover_pos() {
            painter.circle_filled(pointer, 10.0, LASER.gamma_multiply(0.25));
            painter.circle_filled(pointer, 4.0, LASER);
        }
        if !trail.is_empty() {
            ui.ctx().request_repaint();
        }
    }

    /// The pointer has let go of the note: what it drew, moved or swept away is kept.
    fn let_go(&mut self) {
        match self.ink.pressed.take() {
            Some(Pressed::Drawing(_, stroke)) => {
                let mut strokes = self.ink.strokes.clone();
                strokes.push(stroke);
                self.change_ink(strokes);
            }
            Some(Pressed::Shaping(_, _, stroke)) if whole(&stroke) => {
                let mut strokes = self.ink.strokes.clone();
                strokes.push(stroke);
                self.change_ink(strokes);
            }
            Some(Pressed::Moving(before, _, clicked)) if before == self.ink.strokes => {
                if let Some(i) = clicked {
                    self.ink.selected = vec![i];
                }
            }
            Some(Pressed::Erasing(before) | Pressed::Moving(before, _, _) | Pressed::Bending(before, _, _)) if before != self.ink.strokes => {
                let after = std::mem::replace(&mut self.ink.strokes, before);
                self.change_ink(after);
            }
            _ => {}
        }
    }

    /// Make `strokes` the open note's drawings, as one step to undo, and write them down.
    fn change_ink(&mut self, strokes: Vec<ink::Stroke>) {
        let before = std::mem::take(&mut self.ink.strokes);
        self.ink.undo.push(before);
        self.ink.redo.clear();
        self.ink.set(strokes);
        self.write_ink();
    }

    /// Undo the last change to the open note's drawings, or with `back` false redo it.
    pub(crate) fn undo_ink(&mut self, back: bool) {
        let (from, to) = if back { (&mut self.ink.undo, &mut self.ink.redo) } else { (&mut self.ink.redo, &mut self.ink.undo) };
        let Some(strokes) = from.pop() else { return };
        to.push(std::mem::take(&mut self.ink.strokes));
        self.ink.selected.clear();
        self.ink.set(strokes);
        self.write_ink();
    }

    fn write_ink(&mut self) {
        let Some(note) = self.ink.note.clone() else { return };
        if self.ink.unreadable {
            return self.fail("The drawings over this note were not saved: their file could not be read, and would have been written over");
        }
        if let Err(e) = self.vault.write_ink(&note, &self.ink.strokes) {
            self.fail(format!("Could not save the drawings: {e:#}"));
        }
    }

    /// Turn drawing on, in Reading view, or turn it off.
    pub(crate) fn toggle_drawing(&mut self) {
        self.ink.on = !self.ink.on && self.open.is_some();
        if self.ink.on && self.mode != crate::Mode::Reading {
            self.set_mode(crate::Mode::Reading);
        }
        self.show_graph = false;
    }

    /// Esc while drawing: it finishes a text being written, else lets go of
    /// what is selected, else stops drawing.
    pub(crate) fn escape_drawing(&mut self) {
        if self.ink.typing.is_some() {
            self.finish_typing();
        } else if self.ink.curving.is_some() {
            self.finish_curving();
        } else if !self.ink.selected.is_empty() {
            self.ink.selected.clear();
        } else {
            self.ink.on = false;
        }
    }

    /// Whether a press on the note draws.
    pub(crate) fn drawing(&self) -> bool {
        self.ink.on && self.mode == crate::Mode::Reading && !self.show_graph && self.open.is_some()
    }

    /// The round button at the top right of `view`, the note's part of the
    /// window, and below it, while drawing is on, the tools.
    pub(crate) fn ink_tools(&mut self, ctx: &egui::Context, view: Rect, theme: &Theme) {
        let at = pos2(view.right() - INSET - BUTTON, view.top() + 8.0);
        let area = egui::Area::new(egui::Id::new("ink tools")).order(egui::Order::Middle).fixed_pos(at);
        area.show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
            let on = self.drawing();
            let (rect, button) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
            #[cfg(test)]
            self.spots.insert("Draw", rect);
            let ground = if on { theme.accent } else if button.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { ui.visuals().panel_fill };
            ui.painter().circle(rect.center(), BUTTON / 2.0, ground, Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color));
            paint_pen(ui.painter(), rect.center(), if on { theme.page } else { ui.visuals().text_color() });
            let hint = if on { "Stop drawing" } else { "Draw over this note, in Reading view" };
            if button.on_hover_text(hint).clicked() {
                self.toggle_drawing();
            }
            if self.drawing() {
                self.tool_strip(ui, theme);
            }
        });
    }

    /// The tools, two to a row.
    fn tool_strip(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let frame = egui::Frame::popup(ui.style()).inner_margin(4.0).corner_radius(BUTTON / 2.0);
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let size = BUTTON - 8.0;
            let spot = |ui: &mut egui::Ui, chosen: bool, hint: &str| {
                let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
                if chosen || response.hovered() {
                    let fill = if chosen { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill };
                    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
                }
                (rect, response.on_hover_text(hint))
            };
            let ink = ui.visuals().text_color();
            let ground = ui.visuals().panel_fill;
            for row in TOOLS.chunks(2) {
                ui.horizontal(|ui| {
                    for &(tool, hint) in row {
                        let (rect, response) = spot(ui, self.ink.tool == tool, hint);
                        #[cfg(test)]
                        self.spots.insert(hint.split([':', '(']).next().unwrap_or(hint).trim_end(), rect);
                        paint_tool(ui.painter(), rect.center(), tool, ink, ground);
                        if response.clicked() {
                            self.choose_tool(tool);
                        }
                    }
                });
            }
            ui.separator();
            let colours = colours(theme);
            for (row, chunk) in colours.chunks(2).enumerate() {
                ui.horizontal(|ui| {
                    for (j, colour) in chunk.iter().enumerate() {
                        let i = row * 2 + j;
                        let (rect, response) = spot(ui, self.ink.colour == i, "Colour (1–8)");
                        #[cfg(test)]
                        self.spots.insert(["Colour 1", "Colour 2", "Colour 3", "Colour 4", "Colour 5", "Colour 6", "Colour 7", "Colour 8"][i.min(7)], rect);
                        ui.painter().circle_filled(rect.center(), 5.5, *colour);
                        if response.clicked() {
                            self.pick_colour(i);
                        }
                    }
                });
            }
            ui.separator();
            for (row, chunk) in PEN_WIDTHS.chunks(2).enumerate() {
                ui.horizontal(|ui| {
                    for (j, width) in chunk.iter().enumerate() {
                        let i = row * 2 + j;
                        let (rect, response) = spot(ui, self.ink.width == i, "Size ([ and ])");
                        ui.painter().circle_filled(rect.center(), width / 2.0 + 1.0, ink);
                        if response.clicked() {
                            self.ink.width = i;
                        }
                    }
                });
            }
            if self.ink.tool == Use::Select {
                ui.separator();
                ui.horizontal(|ui| {
                    let can = !self.ink.selected.is_empty();
                    let colour = if can { ink } else { ui.visuals().weak_text_color() };
                    let (rect, front) = spot(ui, false, "Bring to front (Ctrl+])");
                    #[cfg(test)]
                    self.spots.insert("Bring to front", rect);
                    paint_front(ui.painter(), rect.center(), colour, ground);
                    let (rect, delete) = spot(ui, false, "Delete (Delete)");
                    #[cfg(test)]
                    self.spots.insert("Delete marks", rect);
                    paint_cross(ui.painter(), rect.center(), colour);
                    if front.clicked() {
                        self.bring_ink_to_front();
                    }
                    if delete.clicked() {
                        self.delete_selected_ink();
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                for (back, hint) in [(true, "Undo (Ctrl+Z)"), (false, "Redo (Ctrl+Shift+Z)")] {
                    let can = if back { self.ink.can_undo() } else { self.ink.can_redo() };
                    let (rect, response) = spot(ui, false, hint);
                    #[cfg(test)]
                    self.spots.insert(if back { "Undo drawing" } else { "Redo drawing" }, rect);
                    let colour = if can { ink } else { ui.visuals().weak_text_color() };
                    paint_turn(ui.painter(), rect.center(), back, colour);
                    if response.clicked() && can {
                        self.undo_ink(back);
                    }
                }
            });
            let can = !self.ink.strokes.is_empty();
            let (rect, response) = spot(ui, false, "Erase all the drawings over this note (Ctrl+Z brings them back)");
            #[cfg(test)]
            self.spots.insert("Erase drawings", rect);
            paint_bin(ui.painter(), rect.center(), if can { ink } else { ui.visuals().weak_text_color() });
            if response.clicked() && can {
                self.erase_drawings();
            }
        });
    }
}

/// A tool's picture in the strip, around `centre`, in `ink` on `ground`.
fn paint_tool(painter: &egui::Painter, centre: Pos2, tool: Use, ink: Color32, ground: Color32) {
    let p = |x: f32, y: f32| centre + vec2(x, y);
    let stroke = Stroke::new(1.4, ink);
    match tool {
        Use::Pen => paint_pen(painter, centre, ink),
        Use::Highlighter => {
            painter.rect_filled(Rect::from_center_size(centre, vec2(12.0, 6.0)), 1.0, ink.gamma_multiply(0.45));
        }
        Use::Line => {
            painter.line_segment([p(-6.0, 6.0), p(6.0, -6.0)], stroke);
        }
        Use::Arrow => {
            painter.line_segment([p(-6.0, 6.0), p(5.5, -5.5)], stroke);
            painter.add(Shape::line(vec![p(-0.5, -6.0), p(6.0, -6.0), p(6.0, 0.5)], stroke));
        }
        Use::CurvedArrow => {
            let curve: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let t = i as f32 / 10.0;
                    let u = 1.0 - t;
                    let (a, c, b) = (p(-6.5, 5.0), p(-5.0, -6.5), p(6.0, -4.0));
                    pos2(u * u * a.x + 2.0 * u * t * c.x + t * t * b.x, u * u * a.y + 2.0 * u * t * c.y + t * t * b.y)
                })
                .collect();
            painter.add(Shape::line(curve, stroke));
            painter.add(Shape::line(vec![p(1.5, -7.5), p(6.0, -4.0), p(2.0, -0.5)], stroke));
        }
        Use::Rectangle => {
            painter.rect_stroke(Rect::from_center_size(centre, vec2(14.0, 10.0)), 2.0, stroke, egui::StrokeKind::Middle);
        }
        Use::Ellipse => {
            let round: Vec<Pos2> = (0..24).map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / 24.0;
                p(7.5 * angle.cos(), 5.5 * angle.sin())
            }).collect();
            painter.add(Shape::closed_line(round, stroke));
        }
        Use::Text => {
            painter.text(centre, Align2::CENTER_CENTER, "T", FontId::proportional(15.0), ink);
        }
        Use::Badge => {
            painter.circle_filled(centre, 7.5, ink);
            painter.text(centre, Align2::CENTER_CENTER, "1", FontId::proportional(10.0), ground);
        }
        Use::Eraser => paint_eraser(painter, centre, ink),
        Use::Laser => {
            painter.circle_filled(centre, 6.5, LASER.gamma_multiply(0.3));
            painter.circle_filled(centre, 3.0, LASER);
        }
        Use::Select => {
            let arrow = vec![p(-4.0, -7.0), p(-4.0, 5.0), p(-1.0, 2.2), p(1.5, 7.0), p(3.3, 6.2), p(0.9, 1.5), p(4.5, 1.5)];
            painter.add(Shape::closed_line(arrow, stroke));
        }
    }
}

/// A pen, slanting up to the right, around `centre`.
fn paint_pen(painter: &egui::Painter, centre: Pos2, colour: Color32) {
    let at = |x: f32, y: f32| centre + vec2(x, y);
    let stroke = Stroke::new(1.3, colour);
    // The barrel, as two sides and a cap, and the tip.
    painter.add(Shape::closed_line(vec![at(3.0, -6.5), at(6.5, -3.0), at(-2.5, 6.0), at(-6.0, 2.5)], stroke));
    painter.add(Shape::convex_polygon(vec![at(-6.0, 2.5), at(-2.5, 6.0), at(-7.0, 7.0)], colour, Stroke::NONE));
    painter.line_segment([at(1.0, -4.5), at(4.5, -1.0)], stroke);
}

/// An eraser stick, slanting up to the right, with its rubber end down at the left.
fn paint_eraser(painter: &egui::Painter, centre: Pos2, colour: Color32) {
    let along = vec2(1.0, -1.0).normalized();
    let across = vec2(1.0, 1.0).normalized() * 2.8;
    let (bottom, top) = (centre - along * 8.0, centre + along * 8.0);
    let stick = vec![bottom + across, top + across, top - across, bottom - across];
    painter.add(Shape::closed_line(stick, Stroke::new(1.3, colour)));
    // The rubber end, a third of its length, filled.
    let band = bottom + along * 5.5;
    painter.add(Shape::convex_polygon(vec![bottom + across, band + across, band - across, bottom - across], colour, Stroke::NONE));
}

/// Two squares, one in front of the other: bring to front.
fn paint_front(painter: &egui::Painter, centre: Pos2, colour: Color32, ground: Color32) {
    let stroke = Stroke::new(1.3, colour);
    painter.rect_stroke(Rect::from_center_size(centre + vec2(-2.5, -2.5), vec2(9.0, 9.0)), 1.5, stroke, egui::StrokeKind::Middle);
    let front = Rect::from_center_size(centre + vec2(2.5, 2.5), vec2(9.0, 9.0));
    painter.rect_filled(front, 1.5, ground);
    painter.rect_filled(front.shrink(1.5), 1.0, colour);
}

/// A bin with a lid: erase everything.
fn paint_bin(painter: &egui::Painter, centre: Pos2, colour: Color32) {
    let p = |x: f32, y: f32| centre + vec2(x, y);
    let stroke = Stroke::new(1.3, colour);
    painter.add(Shape::closed_line(vec![p(-4.5, -3.5), p(4.5, -3.5), p(3.5, 6.5), p(-3.5, 6.5)], stroke));
    painter.line_segment([p(-6.5, -5.5), p(6.5, -5.5)], stroke);
    painter.line_segment([p(-1.5, -7.5), p(1.5, -7.5)], stroke);
    for x in [-1.5, 1.5] {
        painter.line_segment([p(x, -1.0), p(x, 4.0)], stroke);
    }
}

/// A cross: delete.
fn paint_cross(painter: &egui::Painter, centre: Pos2, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    painter.line_segment([centre + vec2(-5.0, -5.0), centre + vec2(5.0, 5.0)], stroke);
    painter.line_segment([centre + vec2(-5.0, 5.0), centre + vec2(5.0, -5.0)], stroke);
}

/// A curved arrow turning back to the left (undo), or on to the right (redo).
fn paint_turn(painter: &egui::Painter, centre: Pos2, back: bool, colour: Color32) {
    let side = if back { 1.0 } else { -1.0 };
    let at = |x: f32, y: f32| centre + vec2(side * x, y);
    let stroke = Stroke::new(1.3, colour);
    let arc: Vec<Pos2> = (0..=8).map(|i| {
        let angle = std::f32::consts::PI * (1.0 - i as f32 / 8.0);
        at(-angle.cos() * 5.0, 1.5 - angle.sin() * 4.5)
    }).collect();
    painter.add(Shape::line(arc, stroke));
    // The head, at the start of the arc on the left.
    painter.add(Shape::line(vec![at(-7.5, -1.0), at(-5.0, 1.8), at(-2.0, -0.5)], stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_highlighter_goes_under_the_text_solid_in_the_colour_it_shows_as() {
        let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), Rect::EVERYTHING);
        let points = vec![[0.0, 0.0], [20.0, 5.0], [0.0, 10.0], [20.0, 0.0]];
        let stroke = ink::Stroke { anchor: String::new(), nth: 0, tool: Tool::Highlighter, color: [255, 208, 0, 96], width: 10.0, points, text: String::new() };
        let mut under = Vec::new();
        paint(&painter, &stroke, &outline(&stroke), Pos2::ZERO, &mut under, Color32::WHITE);
        let [Shape::Path(line)] = &under[..] else { panic!("one line under the text: {under:?}") };
        assert_eq!(line.stroke.color, egui::epaint::ColorMode::Solid(Color32::from_rgb(255, 237, 159)), "the yellow it is over white, with nothing let through");
    }
}
