//! Folders: how the note list is arranged into a tree, and making, renaming,
//! deleting folders and moving notes between them. Folder paths here are
//! relative to the vault's root, with the empty path for its top level.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use malachite::{Note, Vault};

use crate::files::{self, Files, NewNotes};
use crate::{gone, natural_cmp, App, Focus};

/// One line of the note list.
pub(crate) enum Row {
    /// A folder, and how many notes are in it, counting the folders inside it.
    Folder { path: PathBuf, depth: usize, notes: usize },
    /// The note `notes[index]`.
    Note { index: usize, depth: usize },
    /// A file that is not a note, such as a picture, by its path.
    File { path: PathBuf, depth: usize },
}

/// Put `notes` in the order the list shows them, and work out its rows: each
/// folder has its subfolders first, then its notes, both in natural order.
pub(crate) fn arrange(notes: Vec<Note>, files: Vec<PathBuf>, folders: &[PathBuf], vault: &Vault) -> (Vec<Note>, Vec<Row>) {
    let mut by_folder: HashMap<PathBuf, Vec<Note>> = HashMap::new();
    for note in notes {
        by_folder.entry(vault.folder_of(&note.path)).or_default().push(note);
    }
    let mut files_in: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for file in files {
        files_in.entry(vault.folder_of(&file)).or_default().push(file);
    }
    let mut tree = Tree { folders, by_folder, files: files_in, notes: Vec::new(), rows: Vec::new() };
    tree.add(Path::new(""), 0);
    // Nothing should be left, but a note is never dropped from the list.
    let mut strays: Vec<Note> = tree.by_folder.drain().flat_map(|(_, notes)| notes).collect();
    strays.sort_by(|a, b| natural_cmp(&a.title, &b.title));
    for note in strays {
        tree.rows.push(Row::Note { index: tree.notes.len(), depth: 0 });
        tree.notes.push(note);
    }
    (tree.notes, tree.rows)
}

struct Tree<'a> {
    folders: &'a [PathBuf],
    by_folder: HashMap<PathBuf, Vec<Note>>,
    files: HashMap<PathBuf, Vec<PathBuf>>,
    notes: Vec<Note>,
    rows: Vec<Row>,
}

impl Tree<'_> {
    /// Add what is in `folder`, whose own contents sit at `depth`, and say
    /// how many notes that was.
    fn add(&mut self, folder: &Path, depth: usize) -> usize {
        let mut inside: Vec<&PathBuf> = self.folders.iter().filter(|f| f.parent() == Some(folder)).collect();
        inside.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
        let mut count = 0;
        for child in inside {
            let row = self.rows.len();
            self.rows.push(Row::Folder { path: child.clone(), depth, notes: 0 });
            let added = self.add(child, depth + 1);
            if let Row::Folder { notes, .. } = &mut self.rows[row] {
                *notes = added;
            }
            count += added;
        }
        let mut notes = self.by_folder.remove(folder).unwrap_or_default();
        notes.sort_by(|a, b| natural_cmp(&a.title, &b.title));
        count += notes.len();
        for note in notes {
            self.rows.push(Row::Note { index: self.notes.len(), depth });
            self.notes.push(note);
        }
        // After the notes, and not counted with them.
        let mut files = self.files.remove(folder).unwrap_or_default();
        files.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
        self.rows.extend(files.into_iter().map(|path| Row::File { path, depth }));
        count
    }
}

/// A folder's own name, without the folders it is in.
pub(crate) fn name(folder: &Path) -> String {
    folder.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// A folder's whole path as people write it, with `/` between the parts.
pub(crate) fn display(folder: &Path) -> String {
    folder.to_string_lossy().replace('\\', "/")
}

/// Where `path`, a folder or note the settings name, is once the folder
/// `from` has become `to`: the same place under `to`, if it is `from` or
/// inside it. Settings are written by hand, so folders are told apart as
/// templates are found by theirs, without regard to case.
fn moved_with(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let parts = |path: &Path| path.iter().map(|part| part.to_string_lossy().to_lowercase()).collect::<Vec<_>>();
    let from_parts = parts(from);
    let inside = !from_parts.is_empty() && parts(path).starts_with(&from_parts);
    inside.then(|| to.iter().chain(path.iter().skip(from_parts.len())).collect())
}

impl App {
    /// The folder of the open note: where a new folder goes.
    pub(crate) fn current_folder(&self) -> PathBuf {
        self.open_path.as_deref().map(|path| self.vault.folder_of(path)).unwrap_or_default()
    }

    /// Where a new note goes, as the settings say: beside the open note, at
    /// the top of the vault, or in a folder of their choosing.
    pub(crate) fn new_note_folder(&self) -> PathBuf {
        match self.files.new_notes {
            NewNotes::Beside => self.current_folder(),
            NewNotes::Top => PathBuf::new(),
            NewNotes::Folder => self.files.new_notes_folder.clone(),
        }
    }

    /// How many notes are in `folder`, counting the folders inside it.
    pub(crate) fn notes_in(&self, folder: &Path) -> usize {
        let dir = self.vault.root().join(folder);
        self.notes.iter().filter(|n| n.path.starts_with(&dir)).count()
    }

    fn unfold(&mut self, folder: &Path) {
        for ancestor in folder.ancestors() {
            self.collapsed.remove(ancestor);
        }
    }

    /// Fold every folder in the list away, those inside others too, so that
    /// unfolding one shows its folders still folded; or unfold them all.
    pub(crate) fn fold_folders(&mut self, fold: bool) {
        match fold {
            true => self.collapsed = self.folders.iter().cloned().collect(),
            false => self.collapsed.clear(),
        }
        // A command can ask for this with the list put away: it is what shows it.
        self.show_list = true;
    }

    /// Fold every folder away but `folder` and the folders it is in, so that
    /// the list shows what is in it and little else. The folders inside it
    /// are folded too, as they are by folding everything.
    pub(crate) fn fold_others(&mut self, folder: &Path) {
        self.collapsed = self.folders.iter().filter(|other| !folder.starts_with(other)).cloned().collect();
    }

    /// The folders folded in this vault, as the settings file keeps them:
    /// with `/` between folders, in order.
    pub(crate) fn folded_here(&self) -> Vec<String> {
        let mut folded: Vec<String> = self.collapsed.iter().map(|folder| display(folder)).collect();
        folded.sort();
        folded
    }

    /// The folders left folded in each remembered vault, for the settings file.
    pub(crate) fn folded_to_remember(&self) -> HashMap<String, Vec<String>> {
        let here = self.vault.root();
        let folded = self.recent.iter().filter_map(|vault| {
            let folders = if vault == here { self.folded_here() } else { self.folded_elsewhere.get(&*vault.to_string_lossy())?.clone() };
            (!folders.is_empty()).then(|| (vault.to_string_lossy().into_owned(), folders))
        });
        folded.collect()
    }

    /// Fold the folders that were left folded the last time this vault was
    /// open, those that are still there.
    pub(crate) fn take_up_folded(&mut self) {
        let left = self.folded_elsewhere.remove(&*self.vault.root().to_string_lossy()).unwrap_or_default();
        let folders = left.iter().map(|folder| folder.split('/').collect::<PathBuf>());
        self.collapsed = folders.filter(|folder| self.folders.contains(folder)).collect();
    }

    /// Whether there are folders and the list shows each of them folded. One
    /// inside a folded folder is out of sight, so folded or not it does not count.
    pub(crate) fn all_folded(&self) -> bool {
        let showing_open = |folder: &PathBuf| folder.ancestors().all(|f| !self.collapsed.contains(f));
        !self.folders.is_empty() && !self.folders.iter().any(showing_open)
    }

    /// Move the note in the file `note` into `folder`.
    pub(crate) fn move_note(&mut self, note: &Path, folder: &Path) -> Result<()> {
        if self.vault.folder_of(note) == folder {
            // Dropped where it already was.
            return Ok(());
        }
        let was_open = self.open_path.as_deref() == Some(note);
        if was_open {
            self.save();
            if self.dirty {
                bail!("could not save the note before moving it");
            }
        }
        let moved = self.vault.move_note(note, folder)?;
        self.reload()?;
        if was_open {
            // Same note, same text: only its file is somewhere else now.
            self.open_path = Some(moved.clone());
            self.reveal = true;
        }
        self.unfold(folder);
        let title = moved.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        self.info(match folder.as_os_str().is_empty() {
            true => format!("Moved {title} to the top level"),
            false => format!("Moved {title} to {}", display(folder)),
        });
        Ok(())
    }

    /// Make a folder inside `parent`, with its name ready to be typed over.
    pub(crate) fn new_folder(&mut self, parent: &Path) -> Result<()> {
        let folder = self.vault.create_folder(parent, "New folder")?;
        self.reload()?;
        self.unfold(parent);
        // The tree is what shows it, and the tree gives way to search results.
        self.filter.clear();
        self.show_list = true;
        self.renaming = Some((folder.clone(), name(&folder)));
        self.focus = Some(Focus::Folder);
        Ok(())
    }

    pub(crate) fn rename_folder(&mut self, folder: &Path, new_name: &str) -> Result<()> {
        let new_name = new_name.trim();
        if new_name.is_empty() || name(folder) == new_name {
            return Ok(());
        }
        // The open note may be in there, and its file is about to move.
        self.save();
        if self.dirty {
            bail!("could not save the open note before renaming the folder");
        }
        let renamed = self.vault.rename_folder(folder, new_name)?;
        self.follow_folder(folder, &renamed);
        self.reload()?;
        self.info(format!("Renamed the folder to {}", name(&renamed)));
        Ok(())
    }

    /// Move `folder` and everything in it into the folder `into`.
    pub(crate) fn move_folder(&mut self, folder: &Path, into: &Path) -> Result<()> {
        if folder.parent() == Some(into) || folder == into {
            // Dropped where it already was, or on itself.
            return Ok(());
        }
        // The open note may be in there, and its file is about to move.
        self.save();
        if self.dirty {
            bail!("could not save the open note before moving the folder");
        }
        let moved = self.vault.move_folder(folder, into)?;
        self.follow_folder(folder, &moved);
        self.reload()?;
        self.unfold(into);
        self.info(match into.as_os_str().is_empty() {
            true => format!("Moved the folder {} to the top level", name(folder)),
            false => format!("Moved the folder {} into {}", name(folder), display(into)),
        });
        Ok(())
    }

    /// Keep up with a folder that has been renamed or moved from `from` to
    /// `to`: the open note may have been in it, folders folded away inside
    /// it stay folded, and the vault's settings that name it go on naming it.
    fn follow_folder(&mut self, from: &Path, to: &Path) {
        let moved = |path: &Path, from: &Path, to: &Path| match path.strip_prefix(from) {
            Ok(rest) if rest.as_os_str().is_empty() => Some(to.to_path_buf()),
            Ok(rest) => Some(to.join(rest)),
            Err(_) => None,
        };
        let root = self.vault.root();
        if let Some(open) = self.open_path.as_deref().and_then(|p| moved(p, &root.join(from), &root.join(to))) {
            self.open_path = Some(open);
        }
        self.collapsed = self.collapsed.drain().map(|f| moved(&f, from, to).unwrap_or(f)).collect();
        self.follow_in_settings(from, to);
    }

    /// Rewrite the vault's settings that name the folder `from`, or a folder
    /// inside it, to name it at `to`: the templates folder, the daily note's
    /// folder and the folder for new notes, and the daily note's template,
    /// which is a note that may be in it. Each is one line of the vault's
    /// file, and nothing else in it changes.
    fn follow_in_settings(&mut self, from: &Path, to: &Path) {
        let settings = self.files.clone();
        let templates = files::written(&Files::default().templates);
        let named = [("templates", &settings.templates, templates.as_str()), ("daily_folder", &settings.daily_folder, ""), ("new_notes_folder", &settings.new_notes_folder, "")];
        for (key, folder, default) in named {
            if let Some(now) = moved_with(folder, from, to) {
                self.set_option(files::SECTION, key, &files::written(&now), default);
            }
        }
        // A note at the top of the vault that is called what the folder was is not in it.
        let template = Path::new(&settings.daily_template);
        if let Some(now) = moved_with(template, from, to).filter(|now| now != to) {
            self.set_option(files::SECTION, "daily_template", &files::written(&now), "");
        }
    }

    /// Delete `folder` and everything in it, in the way the settings say.
    pub(crate) fn trash_folder(&mut self, folder: &Path) -> Result<()> {
        self.confirm = None;
        let dir = self.vault.root().join(folder);
        let had_open = self.open_path.as_ref().is_some_and(|path| path.starts_with(&dir));
        if had_open {
            self.save();
            if self.dirty {
                bail!("could not save the open note before deleting the folder");
            }
        }
        let how = self.files.deleted;
        self.vault.discard_folder(folder, how)?;
        if had_open {
            self.close_note();
        }
        self.collapsed.retain(|f| !f.starts_with(folder));
        self.reload()?;
        if had_open && !self.notes.is_empty() {
            self.show_note(0);
        }
        self.info(format!("{} the folder {}{}", gone(how).0, display(folder), gone(how).1));
        Ok(())
    }
}
