//! The command palette (Ctrl+P), the quick switcher (Ctrl+O), the vault
//! switcher and the list of templates: a box to type in over a list that
//! narrows as you type. The arrow keys move through the list, Enter takes
//! the highlighted row, Esc or a click outside closes it.
//!
//! The palette lists the commands in `commands.rs`. The quick switcher lists
//! the notes, and offers to create the note that was asked for when there is
//! none; a note is found by its aliases as well as by its name. The list of
//! templates puts the one picked into the open note.
//! The vault switcher lists the vaults that have been open before, and
//! opens any other folder as a vault: typed as a path, or picked in the
//! system's own dialog.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Key, Layout, Modifiers};

use crate::commands::{Command, Run, COMMANDS};
use crate::{folders, Action, App, Mode};

/// How much of the list shows before it scrolls.
const LIST_HEIGHT: f32 = 320.0;

/// The highlight behind the row that Enter would take: how far it reaches
/// past the row's text on either side, and how round its corners are, which
/// is as round as those of the open note in the note list.
const ROW_PADDING: i8 = 4;
const ROW_CORNERS: f32 = 4.0;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Commands,
    Notes,
    Vaults,
    Templates,
}

pub(crate) struct Picker {
    kind: Kind,
    query: String,
    /// The highlighted row.
    selected: usize,
    /// Just opened: the field has yet to take the keyboard.
    fresh: bool,
}

impl Picker {
    pub(crate) fn new(kind: Kind) -> Self {
        Self { kind, query: String::new(), selected: 0, fresh: true }
    }

    /// Take the keyboard away from the note when the picker has just opened.
    /// Called before anything is drawn, so that nothing typed straight after
    /// the hotkey ends up in the note.
    pub(crate) fn take_keyboard(&self, ctx: &egui::Context) {
        if self.fresh {
            ctx.memory_mut(|memory| memory.request_focus(field_id()));
        }
    }
}

/// What taking a row does.
enum Choice {
    Run(&'static Command),
    Open(PathBuf),
    /// Start a note with this name.
    Create(String),
    Vault(PathBuf),
    /// Put this template into the open note.
    Template(PathBuf),
    /// Pick the vault's folder in the system's dialog.
    Browse,
}

struct Row {
    label: String,
    /// Shown faintly at the row's right: a command's hotkey, a note's folder.
    aside: String,
    choice: Choice,
}

impl App {
    pub(crate) fn picker(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(mut picker) = self.picker.take() else { return };
        self.modal_drawn = true;
        let rows = self.picker_rows(ctx, &picker);
        #[cfg(test)]
        {
            self.picked_from = rows.iter().map(|row| row.label.clone()).collect();
        }

        // Taken before the text field sees them: these keys belong to the list.
        let (up, down, enter) = ctx.input_mut(|i| {
            let mut pressed = |key| i.consume_key(Modifiers::NONE, key);
            (pressed(Key::ArrowUp), pressed(Key::ArrowDown), pressed(Key::Enter))
        });
        let last = rows.len().saturating_sub(1);
        picker.selected = (picker.selected + usize::from(down)).saturating_sub(usize::from(up)).min(last);

        // Not the Enter that brought the list up: a button that has the
        // keyboard is pressed with Enter too, and the same press must not go
        // on to take the first row of what it opened, unseen.
        let mut chosen = (enter && !picker.fresh).then_some(picker.selected);
        let modal = egui::Modal::new(egui::Id::new("picker")).show(ctx, |ui| {
            ui.set_width(440.0);
            let hint = match picker.kind {
                Kind::Commands => "Type a command",
                Kind::Notes => "Find or create a note",
                Kind::Vaults => "Open a vault you had open before, or type a folder's path",
                Kind::Templates => "Insert a template",
            };
            // The field is one with the menu, with only the
            // line under it: a box would be outlined for as long as it is typed
            // in, which here is always.
            let field = egui::TextEdit::singleline(&mut picker.query).id(field_id()).hint_text(hint).frame(false).desired_width(f32::INFINITY).show(ui);
            if std::mem::take(&mut picker.fresh) {
                // egui lays a new dialog out once, unseen and unusable, before
                // showing it, and that costs the field the keyboard it was given.
                field.response.request_focus();
            }
            if field.response.changed() {
                // The list is about to change under the highlight.
                picker.selected = 0;
                ui.ctx().request_repaint();
            }
            ui.separator();
            if rows.is_empty() {
                ui.weak("Nothing matches");
            }
            // Room of its own for the list. egui lays a dialog out in the room
            // it took when last drawn, so a list left to take what room there
            // is could shrink, as the rows are narrowed down, and never grow
            // again: not when the search is cleared, and not the next time the
            // dialog is opened, with all the rows.
            let room = egui::vec2(ui.available_width(), LIST_HEIGHT);
            ui.allocate_ui(room, |ui| egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
                // Set in from the sides, where the highlight has its room around a row's text.
                let rows_in = egui::Frame::new().inner_margin(egui::Margin::symmetric(ROW_PADDING, 0));
                rows_in.show(ui, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                        for (index, row) in rows.iter().enumerate() {
                            let selected = index == picker.selected;
                            // The highlighted row is told by the fill behind it and
                            // nothing more: a button that egui knows
                            // to be selected gets a line around it as well.
                            let behind = ui.painter().add(egui::Shape::Noop);
                            let response = ui.add(egui::Button::new(&row.label).shortcut_text(&row.aside).frame(false));
                            if selected {
                                let rect = response.rect.expand2(egui::vec2(f32::from(ROW_PADDING), 1.0));
                                ui.painter().set(behind, egui::Shape::rect_filled(rect, ROW_CORNERS, ui.visuals().selection.bg_fill));
                            }
                            if selected && (up || down) {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
                });
            }));
        });

        match chosen.and_then(|index| rows.into_iter().nth(index)) {
            Some(row) => match row.choice {
                Choice::Run(command) => actions.push(Action::Run(command)),
                Choice::Open(path) => actions.extend([Action::OpenAt(path), Action::FocusEditor]),
                Choice::Create(title) => actions.push(Action::Follow(title, None)),
                Choice::Template(path) => actions.push(Action::InsertTemplate(path)),
                Choice::Vault(root) => actions.extend([Action::OpenVault(root), Action::FocusEditor]),
                Choice::Browse => actions.extend([Action::BrowseVault, Action::FocusEditor]),
            },
            // Back to the note, where the keyboard was.
            None if modal.should_close() => actions.push(Action::FocusEditor),
            None => self.picker = Some(picker),
        }
    }

    /// The rows matching what has been typed, best match first.
    fn picker_rows(&self, ctx: &egui::Context, picker: &Picker) -> Vec<Row> {
        let query = picker.query.trim();
        let mut rows: Vec<(u8, Row)> = match picker.kind {
            Kind::Commands => {
                // Commands that write Markdown need a note that can be written in.
                let editing = self.open.is_some() && self.mode != Mode::Reading;
                let listed = |c: &&Command| c.id != "command-palette:open" && (editing || matches!(c.run, Run::App(_)));
                let row = |command: &'static Command| {
                    let aside = command.hotkey.map(|hotkey| ctx.format_shortcut(&hotkey)).unwrap_or_default();
                    Row { label: command.name.to_owned(), aside, choice: Choice::Run(command) }
                };
                COMMANDS.iter().filter(listed).filter_map(|c| Some((rank(query, c.name)?, row(c)))).collect()
            }
            Kind::Notes => {
                let row = |note: &malachite::Note| {
                    let aside = folders::display(&self.vault.folder_of(&note.path));
                    Row { label: note.title.clone(), aside, choice: Choice::Open(note.path.clone()) }
                };
                let titled = self.notes.iter().filter_map(|n| Some((rank(query, &n.title)?, row(n))));
                // An alias is a row of its own, saying which note it is for.
                let alias = |note: &malachite::Note, alias: String| {
                    let rank = rank(query, &alias)?;
                    Some((rank, Row { label: alias, aside: format!("→ {}", note.title), choice: Choice::Open(note.path.clone()) }))
                };
                let aliased = self.notes.iter().flat_map(|n| malachite::aliases(&n.content).into_iter().filter_map(move |a| alias(n, a)));
                titled.chain(aliased).collect()
            }
            Kind::Templates => {
                let row = |note: &malachite::Note| {
                    let aside = folders::display(&self.vault.folder_of(&note.path));
                    Row { label: note.title.clone(), aside, choice: Choice::Template(note.path.clone()) }
                };
                self.templates().filter_map(|n| Some((rank(query, &n.title)?, row(n)))).collect()
            }
            Kind::Vaults => {
                // The others, that is, and only those whose folder is still there.
                let others = self.recent.iter().filter(|vault| *vault != self.vault.root() && vault.is_dir());
                let row = |vault: &PathBuf| {
                    let (label, aside) = (vault_name(vault), vault.display().to_string());
                    let rank = rank(query, &label).or_else(|| rank(query, &aside))?;
                    Some((rank, Row { label, aside, choice: Choice::Vault(vault.clone()) }))
                };
                others.filter_map(row).collect()
            }
        };
        // Stable, so equally good matches stay in the order they are listed in.
        rows.sort_by_key(|(rank, _)| *rank);
        let mut rows: Vec<Row> = rows.into_iter().map(|(_, row)| row).collect();
        if rows.is_empty() && picker.kind == Kind::Notes && !query.is_empty() {
            rows.push(Row { label: query.to_owned(), aside: "Enter to create".to_owned(), choice: Choice::Create(query.to_owned()) });
        }
        if picker.kind == Kind::Vaults {
            // A full path names a folder to open, whether or not it has been
            // open before. A partial one could end up anywhere, so it is not offered.
            let typed = Path::new(query);
            let known = typed == self.vault.root() || self.recent.iter().any(|vault| vault == typed);
            if typed.is_absolute() && !typed.is_file() && !known {
                let aside = if typed.is_dir() { "Open this folder as a vault" } else { "Create a vault here" };
                rows.push(Row { label: query.to_owned(), aside: aside.to_owned(), choice: Choice::Vault(typed.to_path_buf()) });
            }
            if CAN_BROWSE {
                rows.push(Row { label: "Choose a folder…".to_owned(), aside: String::new(), choice: Choice::Browse });
            }
        }
        rows
    }
}

/// What a vault is called: the name of its folder.
pub(crate) fn vault_name(root: &Path) -> String {
    root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned())
}

/// Whether there is a system dialog to pick a folder in (see `Cargo.toml`).
pub(crate) const CAN_BROWSE: bool = cfg!(any(windows, target_os = "macos"));

/// Ask for a folder with the system's dialog, which starts beside the vault
/// in `near`. `None` if the dialog was cancelled.
///
/// The dialog is given the app's `window` as the one it belongs to. The app
/// does nothing until it is answered, and a dialog with no window of its own
/// can come up behind the app's, or be left there by a click: an app that has
/// stopped answering, with nothing to show why.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn choose_folder(near: &Path, window: &eframe::Frame) -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new().set_title("Open folder as vault").set_parent(window);
    near.parent().map_or(dialog.clone(), |beside| dialog.set_directory(beside)).pick_folder()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn choose_folder(_near: &Path, _window: &eframe::Frame) -> Option<PathBuf> {
    None
}

fn field_id() -> egui::Id {
    egui::Id::new("picker field")
}

/// How well `text` matches what was typed, lower being better, or `None` if
/// it does not: at its start, somewhere inside it, or only as letters in the
/// same order with others in between ("tgb" finds "Toggle bold").
pub(crate) fn rank(query: &str, text: &str) -> Option<u8> {
    let (query, text) = (query.to_lowercase(), text.to_lowercase());
    if text.starts_with(&query) {
        return Some(0);
    }
    if text.contains(&query) {
        return Some(1);
    }
    let mut letters = text.chars();
    query.chars().filter(|c| !c.is_whitespace()).all(|wanted| letters.any(|c| c == wanted)).then_some(2)
}

#[cfg(test)]
mod tests {
    use super::rank;

    #[test]
    fn matches_are_ranked_by_how_directly_they_match() {
        assert_eq!(rank("", "Anything"), Some(0));
        assert_eq!(rank("tog", "Toggle bold"), Some(0));
        assert_eq!(rank("BOLD", "Toggle bold"), Some(1));
        assert_eq!(rank("tgb", "Toggle bold"), Some(2));
        assert_eq!(rank("t b", "Toggle bold"), Some(2), "spaces between the letters do not count");
        assert_eq!(rank("bt", "Toggle bold"), None, "the letters must come in order");
        assert_eq!(rank("x", "Toggle bold"), None);
    }
}
