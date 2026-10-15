//! Everything around the note: the list of notes, the panel with the note's
//! outline, links and tags, the bar above the note, the status bar and the
//! delete dialog.

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::text_edit::TextEditOutput;
use eframe::egui::{self, vec2, Align, Key, Layout, Modifiers, Sense, Stroke};
use malachite::markdown::Style;

use crate::commands::COMMANDS;
use crate::picker::{vault_name, Kind, Picker};
use crate::theme::Theme;
use crate::tree::{tool, Icon};
use crate::files::{self, NewNotes};
use crate::{folders, window, Action, App, Confirm, Focus, Mode};

/// What hovering over the search box says.
const SEARCH_HELP: &str = "Search all notes (Ctrl+Shift+F)

Words are all looked for, in names and text.
\"a phrase\"  as written        a OR b   either
-word       without it         ( )      group
#tag  tag:  file:  path:  content:
line:( )  block:( )  section:( )   together
task:  task-todo:  task-done:      in tasks
match-case:  ignore-case:";

/// The arrows on the back and forward buttons.
pub(crate) const BACK: &str = "⏴";
pub(crate) const FORWARD: &str = "⏵";

/// The least width the note is left with between the side panels: they give
/// way to it when the window is made narrow.
const NOTE_ROOM: f32 = 340.0;

/// How wide the side panels can be dragged, and so how narrow.
const LIST_WIDTH: std::ops::RangeInclusive<f32> = 170.0..=420.0;
const INFO_WIDTH: std::ops::RangeInclusive<f32> = 150.0..=380.0;

/// The widths of the bar above the note below which it no longer holds
/// everything on one row: first the note's place and the Delete button go,
/// then the view buttons move to a row of their own.
const HEADER_FULL: f32 = 480.0;
const HEADER_ROW: f32 = 340.0;

/// The range of widths for a side panel when the other one, if it is
/// showing, is `other` wide: its usual range, cut down so that the two of
/// them leave `NOTE_ROOM` of the window for the note.
fn panel_range(ctx: &egui::Context, usual: std::ops::RangeInclusive<f32>, other: f32) -> std::ops::RangeInclusive<f32> {
    let room = ctx.screen_rect().width() - other - NOTE_ROOM;
    *usual.start()..=room.clamp(*usual.start(), *usual.end())
}

/// How wide the side panel with this id was when it was last drawn.
fn panel_width(ctx: &egui::Context, id: &str) -> Option<f32> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(id)).map(|state| state.rect.width())
}

/// Mode buttons, listed right to left as they are laid out.
const MODES: [(Mode, &str, &str); 3] = [
    (Mode::Reading, "Read", "Reading view: the finished note (Ctrl+E)"),
    (Mode::Source, "Source", "Source mode: formatted, with all Markdown syntax shown"),
    (Mode::Live, "Live", "Live Preview: syntax stays hidden until the cursor touches it"),
];

impl App {
    pub(crate) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
            ui.horizontal(|ui| {
                // The folder's name is enough to tell vaults apart; the full
                // path is a hover away, and the other vaults a click.
                let root = self.vault.root();
                let name = egui::RichText::new(vault_name(root)).weak();
                let hint = format!("Vault folder: {}\nClick to open another vault.", root.display());
                if ui.add(egui::Button::new(name).frame(false)).on_hover_text(hint).clicked() {
                    self.picker = Some(Picker::new(Kind::Vaults));
                }
                ui.weak(match self.notes.len() {
                    1 => "· 1 note".to_owned(),
                    n => format!("· {n} notes"),
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new("⚙").frame(false)).on_hover_text("Settings (Ctrl+,)").clicked() {
                        self.show_settings = true;
                    }
                    ui.separator();
                    self.zoom_control(ui);
                    ui.separator();
                    if self.open.is_some() {
                        self.save_light(ui);
                        // Of what is selected, or else of the whole note.
                        let selected = self.selection.clone().filter(|selection| !selection.is_empty() && self.mode != Mode::Reading);
                        let (words, characters) = match selected.and_then(|selection| self.buffer.get(selection)) {
                            Some(selected) => (selected.split_whitespace().count(), selected.chars().count()),
                            None => {
                                let digest = self.digest();
                                (digest.words, digest.characters)
                            }
                        };
                        let size = format!("{words} words · {characters} characters");
                        #[cfg(test)]
                        {
                            self.counted = size.clone();
                        }
                        ui.weak(size);
                    }
                    match &self.message {
                        Some(message) if message.error => {
                            ui.colored_label(ui.visuals().error_fg_color, &message.text);
                        }
                        Some(message) => {
                            ui.weak(&message.text);
                        }
                        None => {}
                    }
                });
            });
        });
    }

    /// The zoom level, with buttons either side of it. Laid out right to left.
    pub(crate) fn zoom_control(&mut self, ui: &mut egui::Ui) {
        let quiet = |label: &str| egui::Button::new(label).frame(false);
        if ui.add(quiet("+")).on_hover_text("Zoom in (Ctrl +)").clicked() {
            self.zoom_by(1);
        }
        let percent = egui::Label::new(format!("{:.0}%", self.zoom * 100.0)).sense(Sense::click());
        if ui.add(percent).on_hover_text("Zoom of the whole window. Click for 100% (Ctrl+0).").clicked() {
            self.zoom = 1.0;
        }
        if ui.add(quiet("−")).on_hover_text("Zoom out (Ctrl −)").clicked() {
            self.zoom_by(-1);
        }
    }

    /// A dot that is hollow while there are changes still to be saved. Saving
    /// is automatic, so this is reassurance, not something to act on.
    fn save_light(&self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        if self.dirty {
            let ring = Stroke::new(1.5, ui.visuals().hyperlink_color);
            ui.painter().circle_stroke(rect.center(), 3.0, ring);
        } else {
            ui.painter().circle_filled(rect.center(), 3.5, ui.visuals().weak_text_color());
        }
        response.on_hover_text(if self.dirty { "Saving…" } else { "All changes are saved" });
    }

    pub(crate) fn note_list(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // The other panel's width, as far as it is known: its narrowest otherwise.
        let info = if self.info_showing() { panel_width(ctx, "info").unwrap_or(*INFO_WIDTH.start()) } else { 0.0 };
        let range = panel_range(ctx, LIST_WIDTH, info);
        egui::SidePanel::left("notes").default_width(230.0).width_range(range).show(ctx, |ui| {
            // The tools of the list, as pictures: a new note, a new folder,
            // today's note. Their row is one of the bars the window is moved by.
            window::title_bar(ui);
            ui.horizontal(|ui| {
                let here = "in the folder of the open note";
                let new_here = match self.files.new_notes {
                    NewNotes::Beside => here.to_owned(),
                    NewNotes::Top => "at the top of the vault".to_owned(),
                    NewNotes::Folder => format!("in {}", folders::display(&self.files.new_notes_folder)),
                };
                let note = tool(ui, Icon::Note, true);
                let folder = tool(ui, Icon::Folder { open: false }, true);
                let today = tool(ui, Icon::Daily, false);
                #[cfg(test)]
                self.spots.extend([("New note", note.rect), ("New folder", folder.rect), ("Today", today.rect)]);
                if note.on_hover_text(format!("New note {new_here} (Ctrl+N)")).clicked() {
                    actions.push(Action::New);
                }
                if folder.on_hover_text(format!("New folder {here}")).clicked() {
                    actions.push(Action::NewFolder(self.current_folder()));
                }
                if today.on_hover_text("Open today's daily note").clicked() {
                    actions.push(Action::Daily);
                }
            });
            self.search_box(ui, actions);
            ui.separator();

            // Folders are for browsing; a search cuts across them.
            if self.filter.trim().is_empty() {
                self.tree(ui, actions);
            } else {
                self.search_results(ui, actions);
            }
        });
    }

    /// The search field: Enter opens the first match, Esc clears it.
    fn search_box(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let id = egui::Id::new("search");
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.filter.is_empty() && ui.small_button("×").on_hover_text("Clear the search (Esc)").clicked() {
                    self.filter.clear();
                }
                let field = egui::TextEdit::singleline(&mut self.filter)
                    .id(id)
                    .hint_text("Search all notes")
                    .desired_width(f32::INFINITY)
                    .show(ui);
                let field = field_with_hint(field, SEARCH_HELP);
                if self.focus == Some(Focus::Search) {
                    take_keyboard(ui, field, id, &self.filter);
                } else if field.response.lost_focus() {
                    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    if escape {
                        self.filter.clear();
                    } else if enter {
                        if let Some(first) = self.search().first() {
                            actions.push(Action::OpenAt(self.notes[first.note].path.clone()));
                            actions.push(Action::FocusEditor);
                        }
                    }
                }
            });
        });
    }

    /// Whether the right-hand panel is on screen: it has been asked for, and
    /// there is a note for it to be about.
    pub(crate) fn info_showing(&self) -> bool {
        self.show_info && (self.open.is_some() || self.graph_focus().is_some())
    }

    /// What is at the top right corner of the window: its three buttons, and
    /// beside them the button that folds the right-hand panel away. They are
    /// at the top of that panel while it is showing, and at the end of the
    /// bar above the note while it is not, which is the same place on the
    /// screen. To be called first thing in a bar laid out from the right.
    fn corner(&mut self, ui: &mut egui::Ui) {
        self.window_buttons(ui);
        let toggle = panel_toggle(ui, true, self.show_info);
        #[cfg(test)]
        self.spots.insert("Right panel", toggle.rect);
        if toggle.on_hover_text("Show or hide the outline, links and tags").clicked() {
            self.show_info = !self.show_info;
        }
    }

    /// Right-hand panel: the outline of a note, and its outgoing links,
    /// backlinks and tags. The note is the open one, or the one the graph has
    /// in focus: a click there looks at a note without opening it, and this
    /// is where there is something to see of it.
    pub(crate) fn note_info(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let looked_at = self.graph_focus();
        let Some(title) = looked_at.map(|note| self.notes[note].title.clone()).or_else(|| self.open.clone()) else { return };
        // Read from the notes when they change, not on every frame. The open
        // note is read as it is being written, another as it was saved.
        let other = looked_at.filter(|&note| Some(&self.notes[note].path) != self.open_path.as_ref());
        let digest = match other {
            Some(note) => self.digest_of(note),
            None => self.digest(),
        };
        let index = self.index();
        // The graph is where the note would be, so a heading can only be gone
        // to once the note has been put there: this is its file.
        let behind_graph = if self.show_graph { looked_at.map(|note| self.notes[note].path.clone()).or_else(|| self.open_path.clone()) } else { None };
        // A note that a link asks for is made beside the note the link is
        // in, if that is where new notes go.
        let beside = other.map(|note| self.vault.folder_of(&self.notes[note].path)).filter(|_| self.files.new_notes == NewNotes::Beside);
        #[cfg(test)]
        {
            self.link_rows.clear();
            self.info_name = looked_at.map(|_| title.clone());
        }
        let list = if self.show_list { panel_width(ctx, "notes").unwrap_or(*LIST_WIDTH.start()) } else { 0.0 };
        let range = panel_range(ctx, INFO_WIDTH, list);
        egui::SidePanel::right("info").default_width(200.0).width_range(range).show(ctx, |ui| {
            // While it is showing, this panel has the corner of the window.
            window::title_bar(ui);
            ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.corner(ui)));
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui.add_space(4.0);
                if looked_at.is_some() {
                    // Whose all this is: not, as otherwise, the note named in the bar above.
                    ui.strong(&title).on_hover_text("The note in focus in the graph. Click it there again, or press Enter, to open it.");
                    ui.separator();
                }
                self.outline(ui, &digest.headings, behind_graph.as_deref(), actions);

                ui.add_space(10.0);
                ui.strong("Links");
                if digest.links.is_empty() {
                    ui.weak("None");
                }
                // While the graph is showing, a link goes to its note there and
                // only looks at it: the note is opened by a click in the graph.
                let open_hint = if self.show_graph { "Show this note in the graph" } else { "Open this note" };
                for link in &digest.links {
                    let exists = index.names.contains(&link.to_lowercase());
                    let (label, hint) = if exists {
                        (link.clone(), open_hint)
                    } else {
                        (format!("{link} (new)"), "This note does not exist yet. Click to create it.")
                    };
                    let row = ui.link(&label);
                    #[cfg(test)]
                    self.link_rows.push((label, row.rect));
                    if row.on_hover_text(hint).clicked() {
                        let in_graph = malachite::resolve(&self.notes, link).filter(|_| self.show_graph);
                        actions.push(match (&beside, in_graph) {
                            (_, Some(note)) => Action::Look(self.notes[note].path.clone()),
                            (Some(folder), None) if !exists => Action::NewNamed(folder.clone(), link.clone()),
                            _ => Action::Follow(link.clone(), None),
                        });
                    }
                }

                ui.add_space(10.0);
                ui.strong("Backlinks");
                let mut any = false;
                // A link may call this note by one of its aliases.
                let names: Vec<&str> = std::iter::once(title.as_str()).chain(digest.aliases.iter().map(String::as_str)).collect();
                for (n, links) in self.notes.iter().zip(&index.links).filter(|(n, _)| n.title != title) {
                    if links.iter().any(|l| names.iter().any(|name| name.eq_ignore_ascii_case(l))) {
                        any = true;
                        let row = ui.link(&n.title);
                        #[cfg(test)]
                        self.link_rows.push((n.title.clone(), row.rect));
                        if row.on_hover_text("This note links here").clicked() {
                            actions.push(if self.show_graph { Action::Look(n.path.clone()) } else { Action::Open(n.title.clone()) });
                        }
                    }
                }
                if !any {
                    ui.weak("None");
                }

                ui.add_space(10.0);
                ui.strong("Tags");
                if digest.tags.is_empty() {
                    ui.weak("None");
                }
                ui.horizontal_wrapped(|ui| {
                    for tag in &digest.tags {
                        let tag = format!("#{tag}");
                        if ui.link(&tag).on_hover_text("Show notes with this tag").clicked() {
                            actions.push(Action::Filter(tag));
                        }
                    }
                });
            });
        });
    }

    /// The outline pane: the headings of a note, nested by their level. A
    /// click goes to the heading, in the note in the file `behind_graph`
    /// first if the graph is showing in the note's place.
    fn outline(&mut self, ui: &mut egui::Ui, headings: &[malachite::outline::Heading], behind_graph: Option<&std::path::Path>, actions: &mut Vec<Action>) {
        ui.strong("Outline");
        if headings.is_empty() {
            ui.weak("No headings");
        }
        // A note that starts at `##` is not indented for the level it skips.
        let top = headings.iter().map(|heading| heading.level).min().unwrap_or(1);
        #[cfg(test)]
        self.outline_rows.clear();
        let branch = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        let gap = ui.spacing().item_spacing.y;
        for heading in headings {
            let depth = heading.level - top;
            // The branches of the tree: a line down for each level the
            // heading is under, as in the note list.
            let from = ui.cursor().min;
            let height = ui.spacing().interact_size.y;
            for level in 0..depth {
                ui.painter().vline(from.x + f32::from(level) * 12.0 + 5.0, from.y - gap..=from.y + height, branch);
            }
            ui.horizontal(|ui| {
                ui.add_space(f32::from(depth) * 12.0);
                let title = if heading.title.is_empty() { "…" } else { heading.title.as_str() };
                let row = ui.add(egui::Button::new(title).frame(false));
                #[cfg(test)]
                self.outline_rows.push((heading.title.clone(), row.rect));
                if row.on_hover_text("Go to this heading").clicked() {
                    actions.extend(behind_graph.map(|file| Action::OpenAt(file.to_owned())));
                    actions.extend([Action::Jump(heading.line.start..heading.line.start), Action::FocusEditor]);
                }
            });
        }
    }

    /// The bar above the note: the view to show it in, the buttons that fold
    /// the side panels away, and the note's name in small, which stays in
    /// sight when the big one in the note has scrolled away. It is there
    /// without a note as well, with less in it: it is one of the bars the
    /// window is moved by, and where the window's own buttons are when the
    /// right-hand panel is not there to hold them.
    ///
    /// A narrow bar holds less, so that nothing is drawn over anything else:
    /// see `HEADER_FULL` and `HEADER_ROW`.
    pub(crate) fn header(&mut self, ui: &mut egui::Ui, open: Option<&str>, actions: &mut Vec<Action>) {
        let corner = !self.info_showing();
        let width = ui.available_width() - if corner { window::BUTTONS } else { 0.0 };
        let (full, one_row) = (width >= HEADER_FULL, width >= HEADER_ROW);
        window::title_bar(ui);
        ui.horizontal(|ui| {
            let toggle = panel_toggle(ui, false, self.show_list);
            #[cfg(test)]
            self.spots.insert("Note list", toggle.rect);
            if toggle.on_hover_text("Show or hide the note list").clicked() {
                self.show_list = !self.show_list;
            }
            // Back and forward through the notes that have been open, as in a browser.
            for (id, label, hint, enabled) in [
                ("app:go-back", BACK, "Back to the note before this one (Ctrl+Alt+Left)", !self.back.is_empty()),
                ("app:go-forward", FORWARD, "Forward again (Ctrl+Alt+Right)", !self.forward.is_empty()),
            ] {
                let arrow = ui.add_enabled(enabled, egui::Button::new(label).frame(false));
                if arrow.on_hover_text(hint).clicked() {
                    actions.extend(COMMANDS.iter().find(|command| command.id == id).map(Action::Run));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if corner {
                    self.corner(ui);
                }
                let Some(open) = open else {
                    // No note, so no views of one: only the graph of them all.
                    self.graph_button(ui);
                    return;
                };
                // Deleting is also in the command palette and the note list's menu.
                if full {
                    if ui.button("Delete").on_hover_text("Move this note to the trash").clicked() {
                        self.confirm = self.open_path.clone().map(Confirm::Note);
                    }
                    ui.separator();
                }
                if one_row {
                    self.view_buttons(ui, true);
                }
                if full {
                    ui.separator();
                    // Where the note is: its folders, then its name.
                    let place = folders::display(&self.current_folder().join(open));
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        // Not to be selected: it is by its name that a window is dragged.
                        ui.add(egui::Label::new(egui::RichText::new(place).weak()).truncate().selectable(false));
                    });
                }
            });
        });
        if open.is_some() && !one_row {
            ui.horizontal_wrapped(|ui| self.view_buttons(ui, false));
        }
    }

    /// The button that shows the graph in the note's place, and takes it away again.
    fn graph_button(&mut self, ui: &mut egui::Ui) {
        if ui.selectable_label(self.show_graph, "Graph").on_hover_text("The vault as a graph of linked notes (Ctrl+G)").clicked() {
            self.toggle_graph();
        }
    }

    /// The buttons that choose what is shown of the note: the graph or one of
    /// its three views. `reversed` is for a layout that runs right to left.
    fn view_buttons(&mut self, ui: &mut egui::Ui, reversed: bool) {
        if !reversed {
            self.graph_button(ui);
        }
        let modes: Vec<_> = if reversed { MODES.to_vec() } else { MODES.into_iter().rev().collect() };
        for (mode, label, hint) in modes {
            if ui.selectable_label(self.mode == mode && !self.show_graph, label).on_hover_text(hint).clicked() {
                self.set_mode(mode);
            }
        }
        if reversed {
            self.graph_button(ui);
        }
    }

    /// The note's name at the top of the note, set like a first-level
    /// heading (an "inline title"). It is the file name: editing it
    /// renames the note. Enter finishes the name and moves on to the text;
    /// Esc backs out.
    pub(crate) fn title(&mut self, ui: &mut egui::Ui, open: &str, theme: &Theme, actions: &mut Vec<Action>) {
        let id = egui::Id::new("title");
        // A long name wraps, so this is a multi-line field, but a name is
        // still one line: Enter is taken here, before it can become a line break.
        let entered = ui.memory(|m| m.has_focus(id)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        if entered {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        let heading = theme.format(Style { bold: true, ..Style::default() }, theme.heading_size(1), theme.text);
        let field = egui::TextEdit::multiline(&mut self.title_buf)
            .id(id)
            .font(heading.font_id)
            .text_color(heading.color)
            .frame(false)
            .margin(egui::Margin::ZERO)
            .desired_rows(1)
            .desired_width(f32::INFINITY)
            .hint_text("Name this note")
            .return_key(None)
            .show(ui);
        if self.title_buf.contains(['\n', '\r']) {
            // Pasted text can still bring line breaks with it.
            self.title_buf.retain(|c| c != '\n' && c != '\r');
        }
        if self.focus == Some(Focus::Title) {
            field.response.scroll_to_me(None);
            take_keyboard(ui, field, id, &self.title_buf);
        } else if entered || field.response.lost_focus() {
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.title_buf = open.to_owned();
            } else if self.title_buf != open {
                actions.push(Action::Rename);
            }
            if entered {
                // "Done naming, on to writing."
                actions.push(Action::FocusEditor);
            }
        }
    }

    /// Ask before deleting a note or a folder, and before renaming a note
    /// that other notes link to, when the settings say to ask about that.
    pub(crate) fn confirm_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let gone = files::discard_where(self.files.deleted);
        // What it asks, what it says, and the answers: the one Enter gives,
        // and maybe another, besides Cancel.
        let (heading, what, (yes, done), other) = match &self.confirm {
            None => return,
            Some(Confirm::Note(path)) => {
                let title = path.file_stem().unwrap_or_default().to_string_lossy();
                ("Delete this note?", format!("\"{title}\" will be {gone}"), ("Delete", Action::Trash(path.clone())), None)
            }
            Some(Confirm::Folder(folder)) => {
                let contents = match self.notes_in(folder) {
                    0 => String::new(),
                    1 => " and the note in it".to_owned(),
                    n => format!(" and the {n} notes in it"),
                };
                let what = format!("The folder \"{}\"{contents} will be {gone}", folders::display(folder));
                ("Delete this folder?", what, ("Delete", Action::TrashFolder(folder.clone())), None)
            }
            Some(Confirm::Rename { path, to, linking }) => {
                let old = path.file_stem().unwrap_or_default().to_string_lossy();
                let notes = if *linking == 1 { "1 note links".to_owned() } else { format!("{linking} notes link") };
                let what = format!("{notes} to \"{old}\". Change those links to \"{to}\" as well? Left as they are, they lead to a note that is not there.");
                let rename = |relink| Action::RenameNow { path: path.clone(), to: to.clone(), relink };
                ("Update links?", what, ("Update links", rename(true)), Some(("Don't update", rename(false))))
            }
        };
        let deleting = matches!(self.confirm, Some(Confirm::Note(_) | Confirm::Folder(_)));
        self.modal_drawn = true;
        // A modal: nothing behind it reacts until it is answered. Esc or a
        // click outside cancels; Enter confirms, but not the Enter that
        // brought the dialog up, which may have been a press of the Delete
        // button: the question has to have been seen to be answered.
        let seen = self.under_modal;
        // The first answer, the second, or Cancel.
        let mut answer: Option<Option<bool>> = None;
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(heading);
            ui.add_space(4.0);
            ui.label(what);
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut label = egui::RichText::new(yes);
                if deleting {
                    label = label.color(ui.visuals().error_fg_color);
                }
                if ui.button(label).clicked() || (seen && ui.input(|i| i.key_pressed(Key::Enter))) {
                    answer = Some(Some(true));
                }
                if let Some((no, _)) = &other {
                    let button = ui.button(*no);
                    #[cfg(test)]
                    self.spots.insert(no, button.rect);
                    if button.clicked() {
                        answer = Some(Some(false));
                    }
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(None);
                }
            });
        });
        let answer = answer.or(modal.should_close().then_some(None));
        match answer {
            Some(Some(true)) => actions.push(done),
            Some(Some(false)) => actions.extend(other.map(|(_, action)| action)),
            Some(None) => {
                // A rename asked about and called off leaves the note its name.
                if matches!(self.confirm, Some(Confirm::Rename { .. })) {
                    self.title_buf = self.open.clone().unwrap_or_default();
                }
                self.confirm = None;
            }
            None => {}
        }
    }
}

/// Add a hover hint to a text field.
fn field_with_hint(mut field: TextEditOutput, hint: &str) -> TextEditOutput {
    field.response = field.response.on_hover_text(hint);
    field
}

/// Give a one-line text field the keyboard, with its text selected so that
/// typing replaces it.
pub(crate) fn take_keyboard(ui: &egui::Ui, mut field: TextEditOutput, id: egui::Id, text: &str) {
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    field.state.cursor.set_char_range(Some(all));
    field.state.store(ui.ctx(), id);
    field.response.request_focus();
}

/// A button drawn as a window with a panel down one side, filled in while
/// that panel is showing.
fn panel_toggle(ui: &mut egui::Ui, right: bool, shown: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
    }
    let ink = if shown { visuals.fg_stroke.color } else { ui.visuals().weak_text_color() };
    let stroke = Stroke::new(1.2, ink);
    let window = rect.shrink2(vec2(6.0, 5.0));
    ui.painter().rect_stroke(window, 2.0, stroke, egui::StrokeKind::Inside);
    let split = if right { window.right() - 5.0 } else { window.left() + 5.0 };
    ui.painter().vline(split, window.y_range(), stroke);
    if shown {
        let panel = if right { split..=window.right() } else { window.left()..=split };
        let panel = egui::Rect::from_x_y_ranges(panel, window.y_range());
        ui.painter().rect_filled(panel, 1.0, ink.gamma_multiply(0.5));
    }
    response
}
