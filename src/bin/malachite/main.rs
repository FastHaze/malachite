//! `malachite` — the app: a graphical editor for the vault in `lib.rs`.
//!
//! The layout: note list on the left, editor in the middle,
//! outgoing links / backlinks / tags on the right. Edits are saved automatically.
//!
//! A note is shown in one of three ways: Live Preview and Source
//! mode (`live.rs`) are editors, Reading view (`reading.rs`) is the rendered note.
//!
//! This file holds the app's state and what can be done to a note, and
//! `folders.rs` what can be done with folders. `panels.rs` draws everything
//! around the note, `tree.rs` the list of notes, and `editor.rs` the note itself.
//! `commands.rs` names what the keyboard can ask for, and `picker.rs` is the
//! command palette and the quick switcher. `find.rs` is find and replace in
//! the open note; searching all notes is the note list's (`tree.rs`).
//! What a note is made of, its headings and what folds, is worked out in the
//! library's `outline`; the outline is drawn in `panels.rs`, folds in `live.rs`
//! and `reading.rs`.
//! `graph.rs` is the graph view, which takes the note's place while it is up.

// No console window behind the GUI in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod autoscroll;
mod cache;
mod commands;
mod conf;
mod editor;
mod embed;
mod files;
mod find;
mod folders;
mod graph;
mod live;
mod panels;
mod picker;
mod reading;
mod settings;
mod settings_dialog;
mod theme;
mod tree;
mod window;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use appearance::Appearance;
use autoscroll::{Autoscroll, Wheel};
use commands::{Command, EditFn, Run};
use eframe::egui;
use folders::Row;
use live::Hit;
use malachite::markdown::{Link, Place};
use malachite::{outline, template, Note, Vault};
use picker::Picker;
use settings::Settings;

/// What the app is called, in its window's title bar and on the taskbar.
const APP_NAME: &str = "Malachite";

/// The app's icon: a square picture, as red, green, blue and opacity for
/// each of its pixels. `build.rs` makes it from the drawing in `assets/`.
const ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon.rgba"));

/// The icon as the window takes it, for its title bar and the taskbar.
fn icon() -> egui::IconData {
    let side = ((ICON.len() / 4) as f64).sqrt() as u32;
    egui::IconData { rgba: ICON.to_vec(), width: side, height: side }
}

/// Notes are saved automatically, shortly after the last keystroke.
const AUTOSAVE_DELAY: Duration = Duration::from_millis(800);

/// How long a message such as "Renamed" stays in the status bar. Errors get twice as long.
const MESSAGE_TIME: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Formatted while editing; syntax shows only around the cursor.
    Live,
    /// Formatted while editing, with all syntax shown.
    Source,
    /// Rendered, not editable (apart from ticking tasks).
    Reading,
}

fn main() -> eframe::Result {
    let settings_path = settings::path();
    // Under a new name the app starts with what it had under the last one.
    // Not when it has been told where its settings are: that is a test's doing.
    if let Some(path) = settings_path.as_deref().filter(|_| std::env::var_os("MALACHITE_SETTINGS").is_none()) {
        settings::inherit(path);
    }
    let saved = settings_path.as_deref().map(Settings::load).unwrap_or_default();
    // `--graph` starts on the graph; anything else is the vault to open.
    let (flags, named): (Vec<_>, Vec<_>) = std::env::args_os().skip(1).partition(|arg| arg.to_string_lossy().starts_with("--"));
    let graph = flags.iter().any(|flag| flag == "--graph");
    let root = starting_vault(named.into_iter().next().map(PathBuf::from), &saved);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([640.0, 400.0])
            // No title bar: the app has the whole window (see `window.rs`).
            .with_decorations(false)
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            window::dress(cc);
            let faces = theme::install(&cc.egui_ctx);
            let mut app = App::new(root, faces, settings_path)?;
            app.show_graph = graph;
            // Start at the remembered zoom, not at 100% for a frame.
            cc.egui_ctx.set_zoom_factor(app.zoom);
            Ok(Box::new(app))
        }),
    )
}

/// The vault to start in: the one named as the first argument, or else the
/// one that was open last time if its folder is still there, or else `vault`
/// in the current folder, which is the CLI's default too, if there is one.
/// Failing all of those, `Malachite` in the user's documents: an app opened
/// from the system's own launcher starts in a folder of the system's
/// choosing, on macOS one it cannot write in.
fn starting_vault(named: Option<PathBuf>, saved: &Settings) -> PathBuf {
    let last = Some(PathBuf::from(&saved.vault)).filter(|last| last.is_dir());
    let here = Some(PathBuf::from("vault")).filter(|here| here.is_dir());
    named.or(last).or(here).or_else(documents_vault).unwrap_or_else(|| PathBuf::from("vault"))
}

/// `Malachite` in the user's documents folder, or in their home folder
/// where there is no documents folder.
fn documents_vault() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)?;
    let documents = Some(home.join("Documents")).filter(|documents| documents.is_dir()).unwrap_or(home);
    Some(documents.join("Malachite"))
}

/// How to say that something was deleted in this way: the words before its
/// name and after it.
fn gone(how: malachite::Discard) -> (&'static str, &'static str) {
    match how {
        malachite::Discard::Trash => ("Moved", " to the vault's .trash folder"),
        malachite::Discard::System => ("Moved", " to the Recycle Bin"),
        malachite::Discard::Delete => ("Deleted", " for good"),
    }
}

/// UI events are collected while drawing and applied afterwards,
/// so drawing code never mutates the note list it is iterating.
enum Action {
    /// Open the note with this title: what a link or a backlink names.
    Open(String),
    /// Open the note in this file: what a row of the note list stands for.
    OpenAt(PathBuf),
    /// Open a file that is not a note in the program the system has for it.
    OpenFile(PathBuf),
    /// Follow a `[[wikilink]]`: open the note, creating it if it does not
    /// exist yet, and go to the heading or block in it that the link names.
    Follow(String, Option<Place>),
    /// Rename the note in this file, updating the links to it or not: the
    /// answer to the question that asks which.
    RenameNow { path: PathBuf, to: String, relink: bool },
    /// Put the note in this file in focus in the graph, without opening it:
    /// what a link in the side panel does while the graph is showing, so that
    /// a note is only ever opened by a click on it in the graph itself.
    Look(PathBuf),
    /// A new note, in the folder of the one that is open.
    New,
    /// A new note in this folder.
    NewIn(PathBuf),
    /// A new note in this folder, with this name: what a link to a note that
    /// is not there yet asks for, when the note it is in is not the open one.
    NewNamed(PathBuf, String),
    Daily,
    Rename,
    /// Move the note in this file to the trash.
    Trash(PathBuf),
    /// A new folder inside this one.
    NewFolder(PathBuf),
    RenameFolder(PathBuf, String),
    TrashFolder(PathBuf),
    /// Fold every folder in the list away, or with `false` unfold them all.
    FoldFolders(bool),
    /// Fold every folder away but this one and the folders it is in.
    FoldOthers(PathBuf),
    Move { note: PathBuf, folder: PathBuf },
    /// Move a folder and everything in it into another, or to the top level.
    MoveFolder { folder: PathBuf, into: PathBuf },
    Filter(String),
    /// Put the keyboard in the note, once the actions before this have run.
    FocusEditor,
    /// Select this part of the open note's text and bring it into view.
    Jump(std::ops::Range<usize>),
    /// Do what a hotkey or the command palette asked for.
    Run(&'static Command),
    /// Put the text of the template in this file into the open note.
    InsertTemplate(PathBuf),
    /// Leave this vault for the one in this folder, made if it is not there yet.
    OpenVault(PathBuf),
    /// Ask for a folder with the system's dialog, and open it as a vault.
    BrowseVault,
}

/// A field that should take the keyboard on the next frame.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Title,
    Editor,
    Search,
    /// The field of the find bar.
    Find,
    /// The name of the folder being renamed in the note list.
    Folder,
}

/// Something that is only done after the user has said yes to it.
enum Confirm {
    /// Deleting the note in this file.
    Note(PathBuf),
    /// Deleting this folder and everything in it.
    Folder(PathBuf),
    /// Whether to update the links in `linking` notes to the note in the
    /// file `path`, which is to be called `to`.
    Rename { path: PathBuf, to: String, linking: usize },
}

/// Something to tell the user in the status bar for a little while.
struct Message {
    text: String,
    error: bool,
    since: Instant,
}

struct App {
    vault: Vault,
    /// The vaults that have been open, this one first.
    recent: Vec<PathBuf>,
    /// All notes, in the order the list shows them.
    notes: Vec<Note>,
    /// All folders, as paths relative to the vault's root.
    folders: Vec<PathBuf>,
    /// The lines of the note list: folders, and the notes inside them.
    rows: Vec<Row>,
    /// Folders whose contents are folded away in the list.
    collapsed: HashSet<PathBuf>,
    /// The folders left folded in the other remembered vaults, by vault, as
    /// the settings file has them: kept to be written back, and taken up when
    /// one of those vaults is opened again.
    folded_elsewhere: HashMap<String, Vec<String>>,
    /// The folder whose name is being typed in the list, and the name so far.
    renaming: Option<(PathBuf, String)>,
    /// Title of the note in the editor.
    open: Option<String>,
    /// Its file. Notes in different folders can share a title, so this, not
    /// the title, says which note is open.
    open_path: Option<PathBuf>,
    buffer: String,
    title_buf: String,
    dirty: bool,
    last_edit: Instant,
    filter: String,
    /// A deletion waiting for the user's answer.
    confirm: Option<Confirm>,
    message: Option<Message>,
    mode: Mode,
    /// The editing mode Ctrl+E returns to from Reading view.
    edit_mode: Mode,
    /// Whether bold and italic font faces were found (see `theme::install`).
    faces: bool,
    show_list: bool,
    show_info: bool,
    focus: Option<Focus>,
    /// Scroll the note list to the open note the next time it is drawn.
    reveal: bool,
    /// What the window's title bar says.
    window_title: String,
    /// Scale of everything in the window; 1.0 is 100%.
    zoom: f32,
    /// Where settings are kept between runs, and what is in that file now.
    settings_path: Option<PathBuf>,
    saved: Settings,
    /// The settings file, `malachite.conf`, which is kept beside the other,
    /// and what it says of how the app looks; and the open vault's own,
    /// `.malachite/malachite.conf` in it, and what that says of its notes as
    /// files (see `settings_dialog.rs`, `appearance.rs` and `files.rs`).
    conf: settings_dialog::ConfFile,
    look: Appearance,
    vault_conf: settings_dialog::ConfFile,
    files: files::Files,
    /// The look has changed and the window is yet to be styled after it.
    restyle: bool,
    /// Whether the settings dialog is up.
    show_settings: bool,
    /// The system's folder dialog has been asked for, to open a vault with.
    /// It is brought up after the frame, by `update`, and never by a test:
    /// it is on the real screen, and waits for a person to answer it.
    browse: bool,
    /// Autoscroll started by a click of the scroll wheel, while it is on.
    autoscroll: Option<Autoscroll>,
    wheel: Wheel,
    /// Whether the mouse was over the note, with nothing on top, last frame.
    over_note: bool,
    /// The notes that were open before this one, latest last, and those gone
    /// back from: where Ctrl+Alt+Left and Ctrl+Alt+Right lead.
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    /// The command palette or quick switcher, while it is up.
    picker: Option<Picker>,
    /// Whether the graph is showing where the note otherwise is, and the
    /// graph itself, which is kept while it is hidden so that it comes back
    /// as it was left.
    show_graph: bool,
    graph: Option<graph::GraphView>,
    /// Counts the changes to `notes`, so that what was worked out from them
    /// (the results of a search) is known to be out of date.
    version: u64,
    /// Counts the times the vault's files have been gone through. A save
    /// changes a note, and so `version`, but not which files there are.
    scanned: u64,
    results: Option<tree::Results>,
    /// What has been worked out from the notes, from the open note's text,
    /// and for the editor's layout, kept until what it came from changes
    /// (see `cache.rs` and `live::build_kept`).
    index: Option<std::rc::Rc<cache::Index>>,
    digest: Option<std::rc::Rc<cache::Digest>>,
    /// The same of the note the graph has in focus, for the side panel.
    graph_digest: Option<std::rc::Rc<cache::Digest>>,
    layout: Option<live::Kept>,
    /// The pictures that notes embed, by what they are embedded as, and how
    /// tall each embedded note was when the editor last drew it (see `embed.rs`).
    pictures: HashMap<String, embed::Kept>,
    embed_heights: HashMap<Link, f32>,
    /// How tall each table of the open note was drawn in Live Preview, by its text.
    table_heights: HashMap<String, f32>,
    /// The find bar, while it is up; and where what it looks for is in the
    /// note, with which of those places is the current one.
    find: Option<find::Find>,
    /// The same bar in the graph, where it looks for a note by its name.
    graph_find: Option<find::GraphFind>,
    /// A note to put in focus in the graph on the next frame: what a link in
    /// the side panel asks for while the graph is showing.
    graph_look: Option<PathBuf>,
    marks: Vec<std::ops::Range<usize>>,
    mark: usize,
    /// A part of the note for the editor to select and scroll to.
    jump: Option<std::ops::Range<usize>>,
    /// The selection in the editor when it was last drawn, in bytes of the note.
    selection: Option<std::ops::Range<usize>>,
    /// A command from the palette that the editor is to carry out when it is next drawn.
    pending_edit: Option<EditFn>,
    /// Text, a filled-in template, for the editor to put in at the caret.
    pending_insert: Option<String>,
    /// Where the editor is to put the caret, without scrolling to it.
    caret: Option<usize>,
    /// When the caret in the editor last moved or the note was last typed in,
    /// by egui's clock: the caret shows steadily then, and blinks after.
    caret_since: f64,
    /// The colour last picked for a highlight of one's own.
    highlight: [u8; 3],
    /// The folded lines of each note, by its file, as they are written (see
    /// `toggle_fold`). Kept for as long as the app runs.
    folds: HashMap<PathBuf, HashSet<String>>,
    /// Whether a dialog or the palette has been drawn this frame, and whether
    /// one was in the frame before. egui keeps the keyboard from everything
    /// under a modal until the frame after it was last drawn, so what is to
    /// take the keyboard when one closes has to wait that long.
    modal_drawn: bool,
    under_modal: bool,
    /// The edge of the window that the mouse has just taken hold of, for the
    /// system to be told (see `window.rs`).
    resize: Option<egui::viewport::ResizeDirection>,
    /// What the palette or switcher last listed.
    #[cfg(test)]
    picked_from: Vec<String>,
    /// Where the editor's clickable links and checkboxes were last drawn.
    #[cfg(test)]
    hot_spots: Vec<(egui::Pos2, Hit)>,
    /// How far down the note was last scrolled.
    #[cfg(test)]
    scroll_offset: f32,
    /// The editor's text field, once it has been drawn.
    #[cfg(test)]
    editor_id: Option<egui::Id>,
    /// Where on screen the note's text starts, below its name.
    #[cfg(test)]
    content_top: f32,
    /// The rows of the note list as last drawn: what each says and where it is.
    #[cfg(test)]
    list_rows: Vec<(String, egui::Rect)>,
    /// The headings in the outline as last drawn, likewise.
    #[cfg(test)]
    outline_rows: Vec<(String, egui::Rect)>,
    /// Where the buttons that are pictures were last drawn, by what they are
    /// for: the tools above the note list, the panel toggles, the window's own.
    #[cfg(test)]
    spots: HashMap<&'static str, egui::Rect>,
    /// The links and backlinks under the outline, likewise, and the name the
    /// panel was headed by, if it was.
    #[cfg(test)]
    link_rows: Vec<(String, egui::Rect)>,
    #[cfg(test)]
    info_name: Option<String>,
    /// The middle of the fold arrow of every line of the note that folds,
    /// with that line.
    #[cfg(test)]
    fold_spots: Vec<(egui::Pos2, String)>,
    /// The middle of each choice in the note's right-click menu, by name,
    /// while that menu is up.
    #[cfg(test)]
    menu_spots: Vec<(String, egui::Pos2)>,
    /// What the status bar last said about the size of the note.
    #[cfg(test)]
    counted: String,
    /// How wide the column of the note's text was last drawn.
    #[cfg(test)]
    column_width: f32,
}

impl App {
    /// Open the vault at `root`. Settings are read from, and later saved to,
    /// `settings_path`; without one the app starts from the defaults.
    fn new(root: PathBuf, faces: bool, settings_path: Option<PathBuf>) -> Result<Self> {
        // Absolute so the status bar shows where notes really live.
        let root = std::path::absolute(&root).unwrap_or(root);
        let saved = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let conf_path = settings_path.as_deref().and_then(Path::parent).map(|folder| folder.join(appearance::FILE));
        // Said as the app now says it, and written back if the file said it another way.
        let mut conf = settings_dialog::ConfFile::load(settings_dialog::Whose::App, conf_path);
        let look = conf.look();
        let mut vault_conf = settings_dialog::ConfFile::load(settings_dialog::Whose::Vault, Some(files::vault_file(&root)));
        let files = vault_conf.files();
        let edit_mode = if saved.source_mode { Mode::Source } else { Mode::Live };
        let mut app = Self {
            vault: Vault::open(root)?,
            recent: saved.recent.iter().map(PathBuf::from).collect(),
            notes: Vec::new(),
            #[cfg(test)]
            menu_spots: Vec::new(),
            folders: Vec::new(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            folded_elsewhere: saved.folded.clone(),
            renaming: None,
            open: None,
            open_path: None,
            buffer: String::new(),
            title_buf: String::new(),
            dirty: false,
            last_edit: Instant::now(),
            filter: String::new(),
            confirm: None,
            message: None,
            mode: edit_mode,
            edit_mode,
            faces,
            show_list: saved.show_list,
            show_info: saved.show_info,
            focus: None,
            reveal: false,
            window_title: String::new(),
            zoom: saved.zoom,
            highlight: saved.highlight,
            settings_path,
            saved,
            conf,
            vault_conf,
            look,
            files,
            restyle: true,
            show_settings: false,
            browse: false,
            autoscroll: None,
            wheel: Wheel::default(),
            over_note: false,
            back: Vec::new(),
            forward: Vec::new(),
            picker: None,
            show_graph: false,
            graph: None,
            version: 0,
            scanned: 0,
            results: None,
            index: None,
            digest: None,
            graph_digest: None,
            layout: None,
            pictures: HashMap::new(),
            embed_heights: HashMap::new(),
            table_heights: HashMap::new(),
            find: None,
            graph_find: None,
            graph_look: None,
            marks: Vec::new(),
            mark: 0,
            jump: None,
            selection: None,
            pending_edit: None,
            pending_insert: None,
            caret: None,
            caret_since: 0.0,
            folds: HashMap::new(),
            modal_drawn: false,
            under_modal: false,
            resize: None,
            #[cfg(test)]
            picked_from: Vec::new(),
            #[cfg(test)]
            hot_spots: Vec::new(),
            #[cfg(test)]
            scroll_offset: 0.0,
            #[cfg(test)]
            editor_id: None,
            #[cfg(test)]
            content_top: 0.0,
            #[cfg(test)]
            list_rows: Vec::new(),
            #[cfg(test)]
            outline_rows: Vec::new(),
            #[cfg(test)]
            spots: HashMap::new(),
            #[cfg(test)]
            link_rows: Vec::new(),
            #[cfg(test)]
            info_name: None,
            #[cfg(test)]
            fold_spots: Vec::new(),
            #[cfg(test)]
            counted: String::new(),
            #[cfg(test)]
            column_width: 0.0,
        };
        app.report_conf();
        app.take_over_files();
        app.reload()?;
        app.remember_vault();
        // Start on the note that was open last time, if this is the same
        // vault and the note is still there; otherwise on the first one.
        let root = app.vault.root();
        let last = (!app.saved.note.is_empty() && Path::new(&app.saved.vault) == root).then(|| root.join(&app.saved.note));
        let last = last.and_then(|path| app.notes.iter().position(|n| n.path == path));
        if let Some(index) = last.or((!app.notes.is_empty()).then_some(0)) {
            app.show_note(index);
        }
        // The list as it was left, even if that hides the note just opened.
        app.take_up_folded();
        // Only what changes from here on is worth writing to the settings file.
        app.saved = app.settings();
        Ok(app)
    }

    /// Put this vault at the head of the remembered ones.
    fn remember_vault(&mut self) {
        let root = self.vault.root().to_path_buf();
        self.recent.retain(|vault| *vault != root);
        self.recent.insert(0, root);
        self.recent.truncate(settings::RECENT_VAULTS);
        // What was folded in a vault no longer remembered is forgotten with it.
        let recent = &self.recent;
        self.folded_elsewhere.retain(|vault, _| recent.iter().any(|kept| kept.to_string_lossy() == vault.as_str()));
    }

    /// Leave this vault for the one in the folder `root`, which is made if
    /// it does not exist. Nothing changes unless the other vault can be read.
    fn open_vault(&mut self, root: &Path) -> Result<()> {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        if root == self.vault.root() {
            return Ok(());
        }
        self.save();
        if self.dirty {
            bail!("could not save the open note before leaving this vault");
        }
        let vault = Vault::open(&root)?;
        let folders = vault.folders()?;
        let (notes, rows) = folders::arrange(vault.notes()?, vault.attachments()?, &folders, &vault);

        // Nothing of the old vault applies to the new one, its settings
        // included, which are written first if they have been changed.
        self.write_conf();
        self.vault_conf = settings_dialog::ConfFile::load(settings_dialog::Whose::Vault, Some(files::vault_file(&root)));
        self.read_conf();
        self.close_note();
        // Its folded folders are remembered with the others, for coming back to.
        let left = self.vault.root().to_string_lossy().into_owned();
        match self.folded_here() {
            folded if folded.is_empty() => self.folded_elsewhere.remove(&left),
            folded => self.folded_elsewhere.insert(left, folded),
        };
        (self.vault, self.notes, self.folders, self.rows) = (vault, notes, folders, rows);
        self.version += 1;
        self.scanned += 1;
        // Another vault is another graph, to be come upon afresh.
        self.graph = None;
        self.folds.clear();
        self.pictures.clear();
        self.embed_heights.clear();
        self.table_heights.clear();
        self.back.clear();
        self.forward.clear();
        self.filter.clear();
        self.renaming = None;
        self.confirm = None;
        self.remember_vault();
        if !self.notes.is_empty() {
            self.show_note(0);
        }
        self.take_up_folded();
        self.info(format!("Opened the vault in {}", root.display()));
        Ok(())
    }

    /// Open a vault in a folder picked in the system's dialog, which is put
    /// up in front of `window`. The app stands still until the dialog is
    /// answered, as it does for any dialog of the system's.
    fn browse_for_vault(&mut self, window: &eframe::Frame) {
        if let Some(folder) = picker::choose_folder(self.vault.root(), window) {
            if let Err(e) = self.open_vault(&folder) {
                self.fail(format!("Error: {e:#}"));
            }
        }
    }

    /// Read the notes and folders from disk again.
    fn reload(&mut self) -> Result<()> {
        let folders = self.vault.folders()?;
        let (notes, rows) = folders::arrange(self.vault.notes()?, self.vault.attachments()?, &folders, &self.vault);
        self.notes = notes;
        self.folders = folders;
        self.rows = rows;
        self.version += 1;
        self.scanned += 1;
        Ok(())
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: false, since: Instant::now() });
    }

    fn fail(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: true, since: Instant::now() });
    }

    #[cfg(test)]
    fn failed(&self) -> bool {
        self.message.as_ref().is_some_and(|m| m.error)
    }

    /// Write the editor buffer to disk if it has unsaved changes.
    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = &self.open_path else { return };
        let Some(note) = self.notes.iter_mut().find(|n| &n.path == path) else {
            let text = format!("Cannot save: '{}' is no longer in the vault", path.display());
            self.fail(text);
            return;
        };
        match std::fs::write(&note.path, &self.buffer) {
            Ok(()) => {
                note.content = self.buffer.clone();
                self.dirty = false;
                self.version += 1;
            }
            Err(e) => self.fail(format!("Save failed: {e}")),
        }
    }

    /// Notes can change behind the app's back: the CLI, a sync tool, another
    /// editor. Called when the window gets the focus back, this picks up new,
    /// removed and renamed notes, and the open note's text if it has changed
    /// and there is nothing unsaved here to lose.
    fn refresh_from_disk(&mut self) {
        if let Err(e) = self.reload() {
            self.fail(format!("Error: {e:#}"));
            return;
        }
        let (Some(title), Some(path)) = (self.open.clone(), self.open_path.clone()) else { return };
        if self.dirty {
            return;
        }
        match self.notes.iter().find(|n| n.path == path) {
            Some(note) if note.content != self.buffer => {
                self.buffer = note.content.clone();
                self.info(format!("Reloaded {title}: it was changed outside the app"));
            }
            Some(_) => {}
            None => {
                self.close_note();
                self.info(format!("{title} was removed outside the app"));
            }
        }
    }

    /// Open the note with this title, or failing that with this alias. If
    /// several folders have one, the first in the list wins.
    fn open_note(&mut self, title: &str) {
        self.save();
        if let Some(index) = malachite::resolve(&self.notes, title) {
            self.show_note(index);
        }
    }

    /// Open the note in this file.
    fn open_at(&mut self, path: &Path) {
        self.save();
        if let Some(index) = self.notes.iter().position(|n| n.path == path) {
            self.show_note(index);
        }
    }

    /// Put `notes[index]` in the editor, remembering the note it replaces as
    /// the one to go back to. Anything unsaved must be saved first.
    fn show_note(&mut self, index: usize) {
        let next = &self.notes[index].path;
        // A note that was renamed or deleted is no longer there to go back to.
        let left = self.open_path.take().filter(|open| open != next && self.notes.iter().any(|n| &n.path == open));
        if let Some(left) = left {
            self.back.push(left);
            self.forward.clear();
        }
        self.load_note(index);
    }

    /// Go to the note that was open before this one, or with `back` false to
    /// the one that going back came from, as a browser's arrows do.
    fn travel(&mut self, back: bool) {
        self.save();
        loop {
            let Some(path) = (if back { self.back.pop() } else { self.forward.pop() }) else { return };
            // Notes that have gone since are passed over.
            let Some(index) = self.notes.iter().position(|n| n.path == path) else { continue };
            if let Some(here) = self.open_path.clone() {
                if back { self.forward.push(here) } else { self.back.push(here) }
            }
            return self.load_note(index);
        }
    }

    fn load_note(&mut self, index: usize) {
        let note = &self.notes[index];
        let folder = self.vault.folder_of(&note.path);
        self.open = Some(note.title.clone());
        self.open_path = Some(note.path.clone());
        self.buffer = note.content.clone();
        self.title_buf = note.title.clone();
        self.dirty = false;
        self.reveal = true;
        // Where the caret was, or was to go, belongs to the note that was open.
        self.selection = None;
        self.caret = None;
        // A note has been asked for: it is what should be showing.
        self.show_graph = false;
        // The list can only show it if the folders it is in are unfolded.
        for ancestor in folder.ancestors() {
            self.collapsed.remove(ancestor);
        }
    }

    fn close_note(&mut self) {
        self.open = None;
        self.open_path = None;
        self.buffer.clear();
        self.title_buf.clear();
        self.dirty = false;
    }

    /// Start a note in `folder` (relative to the vault's root).
    fn create_in(&mut self, folder: &Path, title: Option<&str>) -> Result<()> {
        self.save();
        let n = self.vault.create_in(folder, title, "", &[])?;
        self.reload()?;
        self.open_at(&n.path);
        // There is nothing to read in a new note.
        self.mode = self.edit_mode;
        // A note made from a link already has its name; a blank one is waiting for one.
        self.focus = Some(if title.is_some() { Focus::Editor } else { Focus::Title });
        self.info(format!("Created {}", n.title));
        Ok(())
    }

    fn follow(&mut self, title: &str, place: Option<&Place>) -> Result<()> {
        if malachite::resolve(&self.notes, title).is_none() {
            // A note that does not exist yet starts next to the one linking
            // to it, and has no headings to go to.
            return self.create_in(&self.new_note_folder(), Some(title));
        }
        self.open_note(title);
        if let Some(place) = place {
            self.go_to(place);
        }
        Ok(())
    }

    /// Go to the heading or block of the open note that a link names.
    fn go_to(&mut self, place: &Place) {
        match outline::find(&self.buffer, place) {
            Some(line) => self.jump = Some(line.start..line.start),
            None => self.info(match place {
                Place::Heading(name) => format!("There is no heading \"{name}\" in this note"),
                Place::Block(id) => format!("There is no block ^{id} in this note"),
            }),
        }
    }

    /// Open the daily note before the one that is open, or with `later` the
    /// one after it. From any other note it is the days around today.
    fn daily_step(&mut self, later: bool) -> Result<()> {
        let format = &self.files.daily_format;
        let open = self.open.as_deref().and_then(|title| malachite::daily_date(title, format));
        let from = open.unwrap_or_else(|| chrono::Local::now().date_naive());
        match malachite::daily_neighbour(&self.notes, from, later, format) {
            Some(index) => {
                self.save();
                self.show_note(index);
                self.focus = Some(Focus::Editor);
            }
            None => self.info(if later { "There is no later daily note" } else { "There is no earlier daily note" }),
        }
        Ok(())
    }

    /// Put the template in the file `path` into the open note at the caret,
    /// with the note's name and the date and time filled in.
    fn insert_template(&mut self, path: &Path) -> Result<()> {
        let Some(title) = self.open.clone() else { bail!("open a note to put the template in") };
        let Some(note) = self.notes.iter().find(|n| n.path == path) else { bail!("that template is no longer there") };
        self.pending_insert = Some(template::fill(&note.content, &title, chrono::Local::now().naive_local()));
        // A template is written into the note, so it has to be one being written.
        if self.mode == Mode::Reading {
            self.mode = self.edit_mode;
        }
        self.show_graph = false;
        Ok(())
    }

    /// The templates of this vault: the notes in its templates folder.
    fn templates(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(|n| self.vault.is_template(&n.path, &self.files.templates))
    }

    /// Open today's daily note, making it if there is none: in the folder,
    /// with the name and from the template that the settings say.
    fn daily(&mut self) -> Result<()> {
        self.save();
        let template = match self.files.daily_template_in(self.vault.root()) {
            Some(file) => match std::fs::read_to_string(&file) {
                Ok(text) => Some(text),
                Err(e) => bail!("could not read the daily note's template {}: {e}", file.display()),
            },
            None => None,
        };
        let now = chrono::Local::now().naive_local();
        let body = |title: &str| match &template {
            Some(text) => template::fill(text, title, now),
            None => format!("# {title}\n"),
        };
        let (n, created) = self.vault.daily_in(&self.files.daily_folder, &self.files.daily_format, body)?;
        self.reload()?;
        self.open_at(&n.path);
        if created {
            self.mode = self.edit_mode;
        }
        // The daily note is for jotting things down: be ready to type.
        self.focus = Some(Focus::Editor);
        Ok(())
    }

    /// Rename the open note to whatever is in the title field, updating the
    /// links to it as the settings say: always, never, or after asking, if
    /// there are any to update.
    fn rename(&mut self) -> Result<()> {
        let (Some(old), Some(path)) = (self.open.clone(), self.open_path.clone()) else { return Ok(()) };
        let new = self.title_buf.trim().to_string();
        if new.is_empty() || new == old {
            self.title_buf = old;
            return Ok(());
        }
        let relink = match self.files.relink {
            files::Relink::Always => true,
            files::Relink::Never => false,
            files::Relink::Ask => {
                // The open note may hold links that are not saved yet.
                self.save();
                match malachite::linking(&self.notes, &old) {
                    0 => true,
                    linking => {
                        self.confirm = Some(Confirm::Rename { path, to: new, linking });
                        return Ok(());
                    }
                }
            }
        };
        self.rename_now(&path, &new, relink)
    }

    /// Rename the note in the file `path`, which is the open one, to `new`;
    /// with `relink`, rewriting the links to it in other notes as well.
    fn rename_now(&mut self, path: &Path, new: &str, relink: bool) -> Result<()> {
        self.confirm = None;
        let Some(old) = self.open.clone().filter(|_| self.open_path.as_deref() == Some(path)) else { return Ok(()) };
        self.save();
        let renamed = if self.dirty {
            Err(anyhow::anyhow!("could not save '{old}' before renaming"))
        } else {
            self.vault.rename_at(path, new, relink)
        };
        let (_, to, updated) = match renamed {
            Ok(r) => r,
            Err(e) => {
                self.title_buf = old;
                return Err(e);
            }
        };
        // Links inside the note itself may have been rewritten, so re-read it.
        self.reload()?;
        self.open_at(&path.with_file_name(format!("{to}.md")));
        self.info(match updated {
            0 if !relink => format!("Renamed to {to}, and left the links to {old} as they were"),
            0 => format!("Renamed to {to}"),
            1 => format!("Renamed to {to}, and updated the link in 1 note"),
            n => format!("Renamed to {to}, and updated links in {n} notes"),
        });
        Ok(())
    }

    /// Delete the note in the file `path`: to the vault's `.trash` folder,
    /// the system's bin, or for good, as the settings say.
    fn trash(&mut self, path: &Path) -> Result<()> {
        self.confirm = None;
        let Some(position) = self.notes.iter().position(|n| n.path == path) else { return Ok(()) };
        let title = self.notes[position].title.clone();
        let was_open = self.open_path.as_deref() == Some(path);
        if was_open {
            self.save();
            if self.dirty {
                bail!("could not save '{title}' before deleting");
            }
        }
        let how = self.files.deleted;
        self.vault.discard_at(path, how)?;
        if was_open {
            self.close_note();
        }
        self.reload()?;
        // Carry on with the note that has taken its place in the list.
        if let Some(next) = self.notes.len().checked_sub(1).filter(|_| was_open) {
            self.show_note(position.min(next));
        }
        self.info(format!("{} {title}{}", gone(how).0, gone(how).1));
        Ok(())
    }

    /// Zoom in or out by a number of 10% steps, landing on a round percentage.
    fn zoom_by(&mut self, steps: i32) {
        let tenths = (self.zoom * 10.0).round() as i32 + steps;
        self.zoom = settings::clamp_zoom(tenths as f32 / 10.0);
    }

    /// The fonts and colours to draw a note in: what the settings say, in
    /// the scheme the window is in.
    fn theme(&self, ctx: &egui::Context) -> theme::Theme {
        theme::Theme::of(&self.look, ctx.style().visuals.dark_mode, self.faces)
    }

    /// What should be remembered for the next run.
    fn settings(&self) -> Settings {
        let root = self.vault.root();
        let note = self.open_path.as_deref().and_then(|path| path.strip_prefix(root).ok());
        Settings {
            zoom: self.zoom,
            show_list: self.show_list,
            show_info: self.show_info,
            source_mode: self.edit_mode == Mode::Source,
            vault: root.to_string_lossy().into_owned(),
            note: note.map(folders::display).unwrap_or_default(),
            recent: self.recent.iter().map(|vault| vault.to_string_lossy().into_owned()).collect(),
            folded: self.folded_to_remember(),
            highlight: self.highlight,
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        self.show_graph = false;
        self.mode = mode;
        if mode != Mode::Reading {
            self.edit_mode = mode;
        }
    }

    /// Ctrl+E: switch between editing and Reading view.
    fn toggle_reading(&mut self) {
        self.show_graph = false;
        self.mode = if self.mode == Mode::Reading { self.edit_mode } else { Mode::Reading };
    }

    /// React to a click on a link or checkbox inside the note.
    fn on_hit(&mut self, ctx: &egui::Context, hit: Hit, actions: &mut Vec<Action>) {
        match hit {
            Hit::Checkbox(range) => {
                let Some(current) = self.buffer.get(range.clone()) else { return };
                let toggled = if current == "[ ]" { "[x]" } else { "[ ]" };
                self.buffer.replace_range(range, toggled);
                self.dirty = true;
                self.last_edit = Instant::now();
            }
            // `[[#Heading]]` has no note part: it points into this note.
            Hit::Link(Link::Note(title, place)) if title.is_empty() => {
                if let Some(place) = place {
                    self.go_to(&place);
                }
            }
            Hit::Link(Link::Note(title, place)) => actions.push(Action::Follow(title, place)),
            // A click on an embedded note opens it; a picture is only looked at.
            Hit::Link(Link::Embed { target, place, .. }) => {
                if !malachite::is_attachment(&target) {
                    actions.push(Action::Follow(target, place));
                }
            }
            Hit::Link(Link::Footnote(id)) => {
                let defines = |line: &malachite::markdown::Line| matches!(&line.kind, malachite::markdown::Kind::Footnote(defined) if *defined == id);
                match malachite::markdown::parse(&self.buffer).iter().find(|line| defines(line)) {
                    Some(line) => self.jump = Some(line.marker.end..line.marker.end),
                    None => self.info(format!("There is no footnote [^{id}] in this note")),
                }
            }
            Hit::Link(Link::Tag(tag)) => actions.push(Action::Filter(format!("#{tag}"))),
            Hit::Link(Link::Url(url)) => {
                // Notes can come from anywhere; only web and mail links go to the system.
                if ["https://", "http://", "mailto:"].iter().any(|scheme| url.starts_with(scheme)) {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let result = match action {
            Action::Open(title) => {
                self.open_note(&title);
                Ok(())
            }
            Action::OpenFile(path) => settings_dialog::open_outside(&path, false).with_context(|| format!("could not open {}", path.display())),
            Action::OpenAt(path) => {
                self.open_at(&path);
                Ok(())
            }
            Action::Follow(title, place) => self.follow(&title, place.as_ref()),
            Action::RenameNow { path, to, relink } => self.rename_now(&path, &to, relink),
            Action::Look(path) => {
                self.graph_look = Some(path);
                Ok(())
            }
            Action::InsertTemplate(path) => self.insert_template(&path),
            Action::New => self.create_in(&self.new_note_folder(), None),
            Action::NewIn(folder) => self.create_in(&folder, None),
            Action::NewNamed(folder, title) => self.create_in(&folder, Some(&title)),
            Action::Daily => self.daily(),
            Action::Rename => self.rename(),
            Action::Trash(path) => self.trash(&path),
            Action::NewFolder(parent) => self.new_folder(&parent),
            Action::RenameFolder(folder, name) => self.rename_folder(&folder, &name),
            Action::TrashFolder(folder) => self.trash_folder(&folder),
            Action::FoldFolders(fold) => {
                self.fold_folders(fold);
                Ok(())
            }
            Action::FoldOthers(folder) => {
                self.fold_others(&folder);
                Ok(())
            }
            Action::Move { note, folder } => self.move_note(&note, &folder),
            Action::MoveFolder { folder, into } => self.move_folder(&folder, &into),
            Action::Filter(query) => {
                self.filter = query;
                // The filter acts on the note list, so it has to be in view.
                self.show_list = true;
                Ok(())
            }
            Action::FocusEditor => {
                self.focus = Some(Focus::Editor);
                Ok(())
            }
            Action::Jump(range) => {
                self.jump = Some(range);
                Ok(())
            }
            Action::OpenVault(root) => self.open_vault(&root),
            Action::BrowseVault => {
                // Not from here: only `update` has the window to show it in front of.
                self.browse = true;
                Ok(())
            }
            Action::Run(command) => match command.run {
                Run::App(run) => run(self),
                Run::Edit(run) => {
                    self.pending_edit = Some(run);
                    Ok(())
                }
            },
        };
        if let Err(e) = result {
            self.fail(format!("Error: {e:#}"));
        }
    }

    fn ui(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let mut actions = Vec::new();
        self.under_modal = std::mem::take(&mut self.modal_drawn);
        // Set again below if a note is drawn with the mouse over it.
        self.over_note = false;

        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            self.refresh_from_disk();
            // The settings files may have been edited meanwhile, as the notes may.
            self.reload_conf();
        }
        if std::mem::take(&mut self.restyle) {
            theme::apply(ctx, &self.look);
        }
        if let Some(edge) = self.resize.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(edge));
        }

        // Hotkeys for the app itself. Those that edit the note are the
        // editor's to take, when it has the keyboard (see `typing_keys`).
        // The palette and the dialogs keep the keyboard to themselves.
        if self.picker.is_none() && self.confirm.is_none() && !self.show_settings {
            if let Some(command) = commands::pressed(ctx, |command| matches!(command.run, Run::App(_))) {
                // Now, not with the actions, so that a field which is to take
                // the keyboard has it before the next key arrives.
                self.apply(Action::Run(command));
            }
        }
        if let Some(picker) = &mut self.picker {
            picker.take_keyboard(ctx);
        }

        // Zoom, as in browsers: Ctrl with + or -, Ctrl+0 for
        // 100%, and Ctrl with the scroll wheel (or a pinch on a touchpad).
        let (reset, steps, pinch) = ctx.input_mut(|i| {
            let mut pressed = |key| i.count_and_consume_key(Modifiers::COMMAND, key) as i32;
            let mut steps = pressed(Key::Plus) + pressed(Key::Equals) - pressed(Key::Minus);
            let mut pinch = 1.0;
            for event in &i.events {
                match event {
                    // A notch of the wheel is one step; a touchpad scrolls smoothly.
                    egui::Event::MouseWheel { unit, delta, modifiers } if modifiers.command => match unit {
                        egui::MouseWheelUnit::Point => pinch *= (delta.y / 200.0).exp(),
                        _ if delta.y > 0.0 => steps += 1,
                        _ if delta.y < 0.0 => steps -= 1,
                        _ => {}
                    },
                    egui::Event::Zoom(factor) => pinch *= factor,
                    _ => {}
                }
            }
            (i.consume_key(Modifiers::COMMAND, Key::Num0), steps, pinch)
        });
        if reset {
            self.zoom = 1.0;
        }
        if steps != 0 {
            self.zoom_by(steps);
        }
        if pinch != 1.0 {
            self.zoom = settings::clamp_zoom(self.zoom * pinch);
        }
        if ctx.zoom_factor() != self.zoom {
            ctx.set_zoom_factor(self.zoom);
        }
        if self.dirty {
            if self.last_edit.elapsed() >= AUTOSAVE_DELAY {
                self.save();
            } else {
                ctx.request_repaint_after(AUTOSAVE_DELAY);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save();
        }

        // Name the open note in the title bar and the taskbar.
        let title = self.open.as_ref().map_or_else(|| APP_NAME.to_owned(), |open| format!("{open} - {APP_NAME}"));
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        // Panels claim space in the order they are added; the editor gets what is left.
        self.status_bar(ctx);
        if self.show_list {
            self.note_list(ctx, &mut actions);
        }
        if self.show_info {
            self.note_info(ctx, &mut actions);
        }
        if self.show_graph {
            self.graph_view(ctx, &mut actions);
        } else {
            // What is looked for in the graph goes away with the graph.
            self.graph_find = None;
            self.graph_look = None;
            self.editor(ctx, &mut actions);
        }
        self.confirm_dialog(ctx, &mut actions);
        self.picker(ctx, &mut actions);
        self.settings_dialog(ctx, &mut actions);

        // A field that was to take the keyboard has done so by now, if it is
        // on screen and was not still under a dialog that has just closed.
        let closing = self.under_modal && self.picker.is_none() && self.confirm.is_none() && !self.show_settings;
        if !closing {
            self.focus = None;
        }
        if let Some(message) = &self.message {
            let life = if message.error { 2 * MESSAGE_TIME } else { MESSAGE_TIME };
            match life.checked_sub(message.since.elapsed()) {
                Some(left) => ctx.request_repaint_after(left),
                None => self.message = None,
            }
        }

        // A pending rename belongs to the note that is open now, so it goes
        // first; focus goes last, to whatever note the others have opened.
        actions.sort_by_key(|action| match action {
            Action::Rename => 0,
            Action::FocusEditor => 2,
            _ => 1,
        });
        for action in actions {
            self.apply(action);
        }
        // A caret to put somewhere waits for the editor, which Reading view does not show.
        let parked = self.caret.is_some() && self.mode != Mode::Reading;
        let waiting = self.pending_edit.is_some() || self.pending_insert.is_some() || self.jump.is_some() || parked;
        if self.focus.is_some() || waiting {
            // Something is waiting for the next frame: do not wait for the mouse to move.
            ctx.request_repaint();
        }

        // Remember the zoom, panels and editing mode as soon as they change.
        let settings = self.settings();
        if settings != self.saved {
            if let Some(Err(e)) = self.settings_path.as_deref().map(|path| settings.save(path)) {
                self.fail(format!("Could not save settings: {e}"));
            }
            self.saved = settings;
        }
        // And the settings file, once a change to it has been let go of: a
        // slider being dragged is many changes, and one worth writing.
        if (self.conf.unsaved || self.vault_conf.unsaved) && !ctx.input(|i| i.pointer.any_down()) {
            self.write_conf();
        }
        window::edge_cursor(ctx);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ui(ctx);
        if std::mem::take(&mut self.browse) {
            self.browse_for_vault(frame);
            ctx.request_repaint();
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.take_edge_press(raw_input);
        // The scroll wheel's button belongs to autoscroll while the mouse is
        // over the note, and for as long as autoscroll is on.
        self.wheel.take_from(raw_input, self.over_note || self.autoscroll.is_some());
    }
}

/// Order titles the way a person would: ignoring case, and with numbers by
/// value, so "Note 2" comes before "Note 10" and "Untitled" before "Untitled 1".
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        let mut value = 0u64;
        while let Some(digit) = chars.peek().and_then(|c| c.to_digit(10)) {
            value = value.saturating_mul(10).saturating_add(u64::from(digit));
            chars.next();
        }
        value
    };
    loop {
        let order = match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => number(&mut a).cmp(&number(&mut b)),
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                x.to_lowercase().cmp(y.to_lowercase())
            }
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

#[cfg(test)]
mod tests;
