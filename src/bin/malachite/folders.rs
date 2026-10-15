//! Folders: how the note list is arranged into a tree, and making, renaming,
//! deleting folders and moving notes between them. Folder paths here are
//! relative to the vault's root, with the empty path for its top level.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use malachite::{Note, Vault};

use crate::files::NewNotes;
use crate::{gone, natural_cmp, App, Focus};

/// One line of the note list.
pub(crate) enum Row {
    Folder { path: PathBuf, depth: usize },
    /// The note `notes[index]`.
    Note { index: usize, depth: usize },
}

/// Put `notes` in the order the list shows them, and work out its rows: each
/// folder has its subfolders first, then its notes, both in natural order.
pub(crate) fn arrange(notes: Vec<Note>, folders: &[PathBuf], vault: &Vault) -> (Vec<Note>, Vec<Row>) {
    let mut by_folder: HashMap<PathBuf, Vec<Note>> = HashMap::new();
    for note in notes {
        by_folder.entry(vault.folder_of(&note.path)).or_default().push(note);
    }
    let mut tree = Tree { folders, by_folder, notes: Vec::new(), rows: Vec::new() };
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
    notes: Vec<Note>,
    rows: Vec<Row>,
}

impl Tree<'_> {
    /// Add what is in `folder`, whose own contents sit at `depth`.
    fn add(&mut self, folder: &Path, depth: usize) {
        let mut inside: Vec<&PathBuf> = self.folders.iter().filter(|f| f.parent() == Some(folder)).collect();
        inside.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
        for child in inside {
            self.rows.push(Row::Folder { path: child.clone(), depth });
            self.add(child, depth + 1);
        }
        let mut notes = self.by_folder.remove(folder).unwrap_or_default();
        notes.sort_by(|a, b| natural_cmp(&a.title, &b.title));
        for note in notes {
            self.rows.push(Row::Note { index: self.notes.len(), depth });
            self.notes.push(note);
        }
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
        let moved = |path: &Path, from: &Path, to: &Path| match path.strip_prefix(from) {
            Ok(rest) if rest.as_os_str().is_empty() => Some(to.to_path_buf()),
            Ok(rest) => Some(to.join(rest)),
            Err(_) => None,
        };
        let root = self.vault.root();
        if let Some(open) = self.open_path.as_deref().and_then(|p| moved(p, &root.join(folder), &root.join(&renamed))) {
            self.open_path = Some(open);
        }
        // Folders folded away inside it stay folded.
        self.collapsed = self.collapsed.drain().map(|f| moved(&f, folder, &renamed).unwrap_or(f)).collect();
        self.reload()?;
        self.info(format!("Renamed the folder to {}", name(&renamed)));
        Ok(())
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
