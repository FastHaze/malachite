//! The note list: a tree of folders and notes, or the flat results of a search.
//!
//! Notes are moved by dragging them onto a folder (or onto a note in it, or
//! below the list for the top level), or from the menu a right click gives.
//! Folders are moved by dragging them in the same way.
//!
//! Every row starts with a small picture of what it is: a folder, open or
//! shut, a note, or a calendar page for a daily note. They are drawn with
//! the painter, a few strokes each, so they need no icon font and take the
//! colour of the row's text. The tools above the list are the same pictures,
//! as buttons (`tool`), and one of their own: a pair of chevrons that folds
//! every folder away, or unfolds them all once the list shows them all folded.

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{self, pos2, vec2, Key, Sense, Shape, Stroke, TextFormat};
use malachite::search::Query;
use malachite::{Note, Vault};

use crate::folders::{display, name, Row};
use crate::panels::take_keyboard;
use crate::{Action, App, Confirm, Focus};

/// How many of the lines found in a note are listed under it.
const LINES_SHOWN: usize = 5;

/// What is being dragged in the list: a note, by its file, or a folder.
#[derive(Clone)]
enum Dragged {
    Note(PathBuf),
    Folder(PathBuf),
}

impl Dragged {
    /// What letting go of it over `folder`, or over a note in it, asks for.
    fn drop_in(&self, folder: PathBuf) -> Action {
        match self {
            Dragged::Note(note) => Action::Move { note: note.clone(), folder },
            Dragged::Folder(moved) => Action::MoveFolder { folder: moved.clone(), into: folder },
        }
    }

    fn name(&self) -> String {
        match self {
            Dragged::Note(note) => note.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            Dragged::Folder(folder) => name(folder),
        }
    }
}

/// What the search in the box above the list found, kept for as long as
/// neither what is searched for nor the notes change.
pub(crate) struct Results {
    query: String,
    version: u64,
    found: Rc<[Match]>,
}

/// A note that a search found.
pub(crate) struct Match {
    /// Its place in `App::notes`.
    pub note: usize,
    /// The lines that show why: where on each the first thing found is, and
    /// the line itself.
    pub lines: Vec<(Range<usize>, String)>,
}

/// How far each level of folders is indented.
const INDENT: f32 = 14.0;
/// Room in front of every name for a folder's arrow, so names line up.
const ARROW: f32 = 14.0;
/// Room after that for the row's picture.
const ICON: f32 = 20.0;

/// The picture in front of a row's name.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Icon {
    Folder { open: bool },
    Note,
    /// A note for a day: `2026-10-13`.
    Daily,
    /// Not a row's: the tool that folds every folder away, or with `fold`
    /// false unfolds them all.
    FoldAll { fold: bool },
    /// A file that is not a note: a picture, say.
    File,
    /// Not a row's either: the vault, on its button in the status bar.
    Vault,
}

impl Icon {
    /// A daily note's is its own: one named for a day in the daily notes' format.
    fn of(note: &Note, daily_format: &str) -> Self {
        if malachite::daily_date(&note.title, daily_format).is_some() {
            Icon::Daily
        } else {
            Icon::Note
        }
    }
}
const PADDING: f32 = 6.0;

impl App {
    /// The folder tree.
    pub(crate) fn tree(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let daily_format = self.files.daily_format.clone();
        let daily_format = daily_format.as_str();
        if self.rows.is_empty() {
            ui.weak("No notes yet. Ctrl+N starts one.");
        }
        #[cfg(test)]
        self.list_rows.clear();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            // The depth of the folded folder whose contents are being skipped.
            let mut folded: Option<usize> = None;
            for row in &self.rows {
                let (Row::Folder { depth, .. } | Row::Note { depth, .. } | Row::File { depth, .. }) = row;
                if folded.is_some_and(|hidden| *depth > hidden) {
                    continue;
                }
                folded = None;

                match row {
                    Row::Folder { path, depth, notes } => {
                        let collapsed = self.collapsed.contains(path);
                        if collapsed {
                            folded = Some(*depth);
                        }
                        if let Some(renaming) = self.renaming.as_mut().filter(|renaming| renaming.0 == *path) {
                            // Its name is being typed: a text field in place of the row.
                            let typed = &mut renaming.1;
                            let id = egui::Id::new("folder name");
                            let field = ui
                                .horizontal(|ui| {
                                    ui.add_space(PADDING + *depth as f32 * INDENT + ARROW + ICON - 4.0);
                                    egui::TextEdit::singleline(typed).id(id).desired_width(f32::INFINITY).show(ui)
                                })
                                .inner;
                            if self.focus == Some(Focus::Folder) {
                                take_keyboard(ui, field, id, typed);
                            } else if field.response.lost_focus() {
                                // Esc keeps the old name; anything else takes the new one.
                                if !ui.input(|i| i.key_pressed(Key::Escape)) {
                                    actions.push(Action::RenameFolder(path.clone(), typed.clone()));
                                }
                                self.renaming = None;
                            }
                            continue;
                        }

                        let label = name(path);
                        let icon = Some(Icon::Folder { open: !collapsed });
                        let look = Look { depth: *depth, label: &label, detail: None, count: Some(*notes), selected: false, quiet: false, folder: Some(!collapsed), icon };
                        let line = tree_row(ui, look);
                        #[cfg(test)]
                        self.list_rows.push((label.clone(), line.rect));
                        if line.clicked() {
                            if !self.collapsed.remove(path) {
                                self.collapsed.insert(path.clone());
                            }
                        }
                        if line.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), Dragged::Folder(path.clone()));
                        }
                        if let Some(dragged) = line.dnd_release_payload::<Dragged>() {
                            actions.push(dragged.drop_in(path.clone()));
                        }
                        line.context_menu(|ui| {
                            if ui.button("New note here").clicked() {
                                actions.push(Action::NewIn(path.clone()));
                                ui.close_menu();
                            }
                            if ui.button("New folder here").clicked() {
                                actions.push(Action::NewFolder(path.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Collapse others").clicked() {
                                actions.push(Action::FoldOthers(path.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Rename").clicked() {
                                self.renaming = Some((path.clone(), label.clone()));
                                self.focus = Some(Focus::Folder);
                                ui.close_menu();
                            }
                            if ui.button("Delete").clicked() {
                                self.confirm = Some(Confirm::Folder(path.clone()));
                                ui.close_menu();
                            }
                        });
                    }
                    Row::Note { index, depth } => {
                        let note = &self.notes[*index];
                        let selected = self.open_path.as_deref() == Some(note.path.as_path());
                        let look = Look { depth: *depth, label: &note.title, detail: None, count: None, selected, quiet: false, folder: None, icon: Some(Icon::of(note, daily_format)) };
                        let line = tree_row(ui, look);
                        #[cfg(test)]
                        self.list_rows.push((note.title.clone(), line.rect));
                        if selected && self.reveal {
                            line.scroll_to_me(None);
                        }
                        if line.clicked() {
                            actions.push(Action::OpenAt(note.path.clone()));
                        }
                        if line.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), Dragged::Note(note.path.clone()));
                        }
                        if let Some(dragged) = line.dnd_release_payload::<Dragged>() {
                            // Dropped on a note: it joins that note in its folder.
                            actions.push(dragged.drop_in(self.vault.folder_of(&note.path)));
                        }
                        note_menu(&line, note, &self.vault, &self.folders, &mut self.confirm, actions);
                    }
                    Row::File { path, depth } => {
                        let label = path.file_name().unwrap_or_default().to_string_lossy();
                        let look = Look { depth: *depth, label: &label, detail: None, count: None, selected: false, quiet: false, folder: None, icon: Some(Icon::File) };
                        let line = tree_row(ui, look).on_hover_text("Opens in the program the system has for it");
                        #[cfg(test)]
                        self.list_rows.push((label.to_string(), line.rect));
                        if line.clicked() {
                            actions.push(Action::OpenFile(path.clone()));
                        }
                    }
                }
            }

            // The space under the list takes drops for the top level.
            let rest = ui.available_rect_before_wrap();
            let below = ui.interact(rest, ui.id().with("top level"), Sense::hover());
            if let Some(dragged) = below.dnd_release_payload::<Dragged>() {
                actions.push(dragged.drop_in(PathBuf::new()));
            }
        });
        dragged_label(ui.ctx());
        self.reveal = false;
    }

    /// The notes that match what is in the search box, in the order of the list.
    pub(crate) fn search(&mut self) -> Rc<[Match]> {
        let query = self.filter.trim();
        if let Some(results) = self.results.as_ref().filter(|r| r.query == query && r.version == self.version) {
            return results.found.clone();
        }
        let parsed = Query::parse(query);
        let matches = self.notes.iter().enumerate().filter(|(_, note)| parsed.matches(note, self.vault.root()));
        let found: Rc<[Match]> = matches
            .map(|(index, note)| {
                let line = |found: malachite::search::Found| (found.hit, note.content[found.range].trim().to_owned());
                Match { note: index, lines: parsed.lines(note).into_iter().map(line).collect() }
            })
            .collect();
        self.results = Some(Results { query: query.to_owned(), version: self.version, found: found.clone() });
        found
    }

    /// The notes matching a search, as a flat list with their folders noted
    /// and under each the lines the search found in it.
    pub(crate) fn search_results(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let daily_format = self.files.daily_format.clone();
        let daily_format = daily_format.as_str();
        let found = self.search();
        if found.is_empty() {
            ui.weak("No notes match.");
        } else {
            ui.weak(format!("{} of {}", found.len(), self.notes.len()));
        }
        #[cfg(test)]
        self.list_rows.clear();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for found in found.iter() {
                let note = &self.notes[found.note];
                let folder = self.vault.folder_of(&note.path);
                let detail = (!folder.as_os_str().is_empty()).then(|| display(&folder));
                let selected = self.open_path.as_deref() == Some(note.path.as_path());
                let look = Look { depth: 0, label: &note.title, detail: detail.as_deref(), count: None, selected, quiet: false, folder: None, icon: Some(Icon::of(note, daily_format)) };
                let line = tree_row(ui, look);
                #[cfg(test)]
                self.list_rows.push((note.title.clone(), line.rect));
                if selected && self.reveal {
                    line.scroll_to_me(None);
                }
                if line.clicked() {
                    actions.push(Action::OpenAt(note.path.clone()));
                }
                note_menu(&line, note, &self.vault, &self.folders, &mut self.confirm, actions);

                for (hit, text) in found.lines.iter().take(LINES_SHOWN) {
                    let look = Look { depth: 1, label: text, detail: None, count: None, selected: false, quiet: true, folder: None, icon: None };
                    let line = tree_row(ui, look);
                    #[cfg(test)]
                    self.list_rows.push((text.clone(), line.rect));
                    if line.clicked() {
                        // To the note, and in it to what was found on this line.
                        actions.extend([Action::OpenAt(note.path.clone()), Action::Jump(hit.clone()), Action::FocusEditor]);
                    }
                }
                if let Some(more) = found.lines.len().checked_sub(LINES_SHOWN).filter(|more| *more > 0) {
                    let label = format!("and {more} more");
                    tree_row(ui, Look { depth: 1, label: &label, detail: None, count: None, selected: false, quiet: true, folder: None, icon: None });
                }
            }
        });
        self.reveal = false;
    }
}

/// The right-click menu of a note in the list.
fn note_menu(
    line: &egui::Response,
    note: &Note,
    vault: &Vault,
    folders: &[PathBuf],
    confirm: &mut Option<Confirm>,
    actions: &mut Vec<Action>,
) {
    line.context_menu(|ui| {
        ui.menu_button("Move to", |ui| {
            let here = vault.folder_of(&note.path);
            let mut target = |ui: &mut egui::Ui, label: String, folder: PathBuf| {
                // Where it already is cannot be chosen.
                if ui.add_enabled(folder != here, egui::Button::new(label)).clicked() {
                    actions.push(Action::Move { note: note.path.clone(), folder });
                    ui.close_menu();
                }
            };
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                target(ui, "Top level".to_owned(), PathBuf::new());
                for folder in folders {
                    target(ui, display(folder), folder.clone());
                }
            });
        });
        if ui.button("Delete").clicked() {
            *confirm = Some(Confirm::Note(note.path.clone()));
            ui.close_menu();
        }
    });
}

/// While a note or a folder is being dragged, show its name by the pointer.
fn dragged_label(ctx: &egui::Context) {
    let (Some(dragged), Some(pointer)) = (egui::DragAndDrop::payload::<Dragged>(ctx), ctx.pointer_latest_pos()) else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let visuals = ctx.style().visuals.clone();
    let font = egui::TextStyle::Button.resolve(&ctx.style());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dragged note")));
    let text = painter.layout_no_wrap(dragged.name(), font, visuals.strong_text_color());
    let padding = vec2(8.0, 4.0);
    let frame = egui::Rect::from_min_size(pointer + vec2(14.0, 10.0), text.size() + 2.0 * padding);
    painter.rect_filled(frame, 4.0, visuals.selection.bg_fill.to_opaque());
    painter.galley(frame.min + padding, text, visuals.strong_text_color());
}

/// A button that is one of the list's pictures and nothing else: one of the
/// tools above the list. With `adds`, a plus is set into the corner of the
/// picture: the button makes a new one of what it shows.
pub(crate) fn tool(ui: &mut egui::Ui, icon: Icon, adds: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, crate::window::BAR), Sense::click());
    let visuals = ui.style().interact(&response);
    let ground = if response.hovered() { visuals.weak_bg_fill } else { ui.visuals().panel_fill };
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, ground);
    }
    let ink = visuals.fg_stroke.color;
    // The plus hangs off the lower right, so the picture makes room for it.
    let centre = rect.center() - if adds { vec2(1.5, 1.0) } else { vec2(0.0, 0.0) };
    paint_icon(ui.painter(), icon, centre, ink);
    if adds {
        // On a patch of the ground cleared for it, so that it is not crossed
        // by the picture's own lines.
        let at = centre + vec2(5.5, 4.5);
        ui.painter().circle_filled(at, 4.5, ground);
        let stroke = Stroke::new(1.4, ink);
        ui.painter().hline(at.x - 3.0..=at.x + 3.0, at.y, stroke);
        ui.painter().vline(at.x, at.y - 3.0..=at.y + 3.0, stroke);
    }
    response
}

/// Draw a row's picture around `centre`, in a box some 14 points across.
pub(crate) fn paint_icon(painter: &egui::Painter, icon: Icon, centre: egui::Pos2, color: egui::Color32) {
    let stroke = Stroke::new(1.2, color);
    let at = |x: f32, y: f32| centre + vec2(x, y);
    match icon {
        Icon::Folder { open } => {
            // A tab at the top left, and the body below it: filled in while
            // the folder is open, as if lit from inside.
            let body = egui::Rect::from_min_max(at(-6.5, -3.0), at(6.5, 5.5));
            if open {
                painter.rect_filled(body, 1.5, color.gamma_multiply(0.3));
            }
            painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Inside);
            painter.add(Shape::line(vec![at(-6.0, -3.0), at(-6.0, -5.5), at(-1.5, -5.5), at(0.5, -3.0)], stroke));
        }
        Icon::Note => {
            // A page with its top right corner turned down, and lines of writing.
            let page = vec![at(-4.5, -6.5), at(1.5, -6.5), at(4.5, -3.5), at(4.5, 6.5), at(-4.5, 6.5)];
            painter.add(Shape::closed_line(page, stroke));
            painter.add(Shape::line(vec![at(1.5, -6.5), at(1.5, -3.5), at(4.5, -3.5)], stroke));
            let faint = Stroke::new(1.0, color.gamma_multiply(0.7));
            for y in [0.0, 3.0] {
                painter.hline(centre.x - 2.5..=centre.x + 2.5, centre.y + y, faint);
            }
        }
        Icon::File => {
            // A page with its corner turned down, as a note's, but nothing written on it.
            let page = vec![at(-4.5, -6.5), at(1.5, -6.5), at(4.5, -3.5), at(4.5, 6.5), at(-4.5, 6.5)];
            painter.add(Shape::closed_line(page, stroke));
            painter.add(Shape::line(vec![at(1.5, -6.5), at(1.5, -3.5), at(4.5, -3.5)], stroke));
            painter.circle_filled(at(0.0, 2.0), 1.5, color);
        }
        Icon::Daily => {
            // A calendar: a page with a band across the top and two rings.
            let page = egui::Rect::from_min_max(at(-5.5, -5.0), at(5.5, 6.0));
            painter.rect_stroke(page, 1.5, stroke, egui::StrokeKind::Inside);
            painter.hline(page.x_range(), centre.y - 1.5, stroke);
            for x in [-2.5, 2.5] {
                painter.vline(centre.x + x, centre.y - 6.5..=centre.y - 3.5, stroke);
            }
            painter.circle_filled(at(0.0, 2.5), 1.2, color);
        }
        Icon::FoldAll { fold } => {
            // Two chevrons, one above the other: pointing in at each other to
            // fold everything up, out away from each other to open it.
            let (tips, ends) = if fold { (1.5, 6.0) } else { (6.0, 1.5) };
            for side in [-1.0, 1.0] {
                let chevron = vec![at(-4.5, side * ends), at(0.0, side * tips), at(4.5, side * ends)];
                painter.add(Shape::line(chevron, stroke));
            }
        }
        Icon::Vault => {
            // A strongbox: its door, and the dial on it.
            painter.rect_stroke(egui::Rect::from_center_size(centre, vec2(11.0, 10.0)), 1.5, stroke, egui::StrokeKind::Inside);
            painter.circle_stroke(centre, 2.2, stroke);
        }
    }
}

/// How a row of the list looks.
struct Look<'a> {
    /// How many folders deep it is.
    depth: usize,
    label: &'a str,
    /// Something to add after the label in a quieter voice, such as a folder.
    detail: Option<&'a str>,
    /// For a folder: how many notes are in it, shown at the end of the row
    /// unless there are none.
    count: Option<usize>,
    selected: bool,
    /// Smaller and fainter: a line of a note rather than a note.
    quiet: bool,
    /// For a folder: whether it is unfolded.
    folder: Option<bool>,
    /// The picture in front of the label. A row without one, a line found
    /// in a note, starts where the pictures of the notes above it do.
    icon: Option<Icon>,
}

/// Draw one row across the width of the list and return its response. A row
/// senses clicks and drags, and lights up while a dragged note or folder is
/// over it.
fn tree_row(ui: &mut egui::Ui, look: Look) -> egui::Response {
    let font = if look.quiet { egui::TextStyle::Small } else { egui::TextStyle::Button }.resolve(ui.style());
    let height = ui.fonts(|f| f.row_height(&font)) + 2.0 * ui.spacing().button_padding.y + if look.quiet { 0.0 } else { 2.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let visuals = ui.visuals();
    let dropping = response.dnd_hover_payload::<Dragged>().is_some();
    let (fill, ink) = if look.selected {
        (Some(visuals.selection.bg_fill), visuals.selection.stroke.color)
    } else if dropping {
        (Some(visuals.selection.bg_fill.gamma_multiply(0.5)), visuals.strong_text_color())
    } else if response.hovered() {
        (Some(visuals.widgets.hovered.weak_bg_fill), visuals.strong_text_color())
    } else if look.quiet {
        (None, visuals.weak_text_color())
    } else {
        (None, visuals.text_color())
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }

    // The branches of the tree: a line down from each folder this row is
    // in, under that folder's arrow, joined up across the gap between rows.
    let branch = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);
    let gap = ui.spacing().item_spacing.y;
    for level in 0..look.depth {
        let x = rect.left() + PADDING + level as f32 * INDENT + 4.0;
        ui.painter().vline(x, rect.top() - gap..=rect.bottom(), branch);
    }

    let left = rect.left() + PADDING + look.depth as f32 * INDENT;
    if let Some(unfolded) = look.folder {
        // An arrow: pointing down when unfolded, right when folded.
        let centre = pos2(left + 4.0, rect.center().y);
        let arrow = match unfolded {
            true => vec![centre + vec2(-4.0, -2.0), centre + vec2(4.0, -2.0), centre + vec2(0.0, 3.0)],
            false => vec![centre + vec2(-2.0, -4.0), centre + vec2(3.0, 0.0), centre + vec2(-2.0, 4.0)],
        };
        ui.painter().add(Shape::convex_polygon(arrow, visuals.weak_text_color(), Stroke::NONE));
    }

    let mut text_left = left + ARROW;
    if let Some(icon) = look.icon {
        // Quieter than the name, unless the row is lit up.
        let quiet = fill.is_none() || (response.hovered() && !look.selected && !dropping);
        let color = if quiet { visuals.weak_text_color() } else { ink };
        paint_icon(ui.painter(), icon, pos2(text_left + 7.0, rect.center().y), color);
        text_left += ICON;
    }
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut text_right = rect.right() - PADDING;
    if let Some(count) = look.count.filter(|count| *count > 0) {
        // Against the right edge, so that the counts of a list make a column.
        let count = ui.fonts(|f| f.layout_no_wrap(count.to_string(), small.clone(), visuals.weak_text_color()));
        text_right -= count.size().x;
        ui.painter().galley(pos2(text_right, rect.center().y - count.size().y / 2.0), count, visuals.weak_text_color());
        text_right -= 8.0;
    }
    let mut job = LayoutJob::single_section(look.label.to_owned(), TextFormat { font_id: font.clone(), color: ink, ..Default::default() });
    if let Some(detail) = look.detail {
        job.append(detail, 8.0, TextFormat { font_id: small, color: visuals.weak_text_color(), ..Default::default() });
    }
    // A long name is cut short rather than pushing the panel wider.
    job.wrap = TextWrapping::truncate_at_width((text_right - text_left).max(0.0));
    let text = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(pos2(text_left, rect.center().y - text.size().y / 2.0), text, ink);
    response
}
