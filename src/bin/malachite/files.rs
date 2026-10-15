//! What a vault's settings say about its notes as files: where a new note
//! starts, whether renaming one rewrites the links to it, what becomes of a
//! note that is deleted, which folder holds the templates, and where today's
//! daily note goes, what it is called and what it starts with.
//!
//! These belong to the vault, not to the app: they name its folders, and
//! another vault is arranged otherwise. So they are kept in the vault, in
//! `.malachite/malachite.conf` (`VAULT_FILE`), a hidden folder the note list
//! and the notes leave out, which goes wherever the vault is copied or synced.
//! The file has the one section, `[files]`, in the format of `conf.rs`, and
//! is read the way the app's own settings file is: a line that cannot be
//! understood is passed over and reported rather than stopping anything. A
//! vault with no such file is at every default, and none is written into a
//! vault until one of its settings is changed.
//!
//! Folders are written as the note list shows them, from the top of the
//! vault with `/` between their parts. A folder that would lead out of the
//! vault, or into a hidden one, is not a folder here.

use std::path::{Component, Path, PathBuf};

use malachite::Discard;

use crate::conf;

/// The section of the file these are in.
pub const SECTION: &str = "files";

/// Where a vault keeps its settings, from its top.
pub const VAULT_FILE: &str = ".malachite/malachite.conf";

/// The settings file of the vault at `root`.
pub fn vault_file(root: &Path) -> PathBuf {
    VAULT_FILE.split('/').fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Where a new note starts: one made with Ctrl+N, or by following a link to
/// a note that is not there yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum NewNotes {
    /// In the folder of the note that is open, or that links to it.
    #[default]
    Beside,
    /// At the top of the vault.
    Top,
    /// In the folder the file names (`new_notes_folder`).
    Folder,
}

impl NewNotes {
    pub const ALL: [NewNotes; 3] = [NewNotes::Beside, NewNotes::Top, NewNotes::Folder];

    pub fn word(self) -> &'static str {
        match self {
            NewNotes::Beside => "beside",
            NewNotes::Top => "top",
            NewNotes::Folder => "folder",
        }
    }

    /// What the dialog calls it.
    pub fn name(self) -> &'static str {
        match self {
            NewNotes::Beside => "Beside",
            NewNotes::Top => "Top level",
            NewNotes::Folder => "A folder",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|it| it.word() == word)
    }
}

/// Whether renaming a note rewrites the links that other notes have to it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Relink {
    #[default]
    Always,
    /// Ask, each time there are links to rewrite.
    Ask,
    Never,
}

impl Relink {
    pub const ALL: [Relink; 3] = [Relink::Always, Relink::Ask, Relink::Never];

    pub fn word(self) -> &'static str {
        match self {
            Relink::Always => "always",
            Relink::Ask => "ask",
            Relink::Never => "never",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Relink::Always => "Always",
            Relink::Ask => "Ask",
            Relink::Never => "Never",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|it| it.word() == word)
    }
}

/// The ways there are of deleting, in the order the dialog offers them.
pub const DISCARDS: [Discard; 3] = [Discard::Trash, Discard::System, Discard::Delete];

/// The word for a way of deleting in the file.
pub fn discard_word(how: Discard) -> &'static str {
    match how {
        Discard::Trash => "vault",
        Discard::System => "system",
        Discard::Delete => "delete",
    }
}

/// What the dialog calls it.
pub fn discard_name(how: Discard) -> &'static str {
    match how {
        Discard::Trash => "Vault's .trash",
        Discard::System => "Recycle Bin",
        Discard::Delete => "Delete for good",
    }
}

/// Where a note goes, as the end of "… will be moved" or "… will be".
pub fn discard_where(how: Discard) -> &'static str {
    match how {
        Discard::Trash => "moved to the vault's .trash folder. You can get it back from there.",
        Discard::System => "moved to the Recycle Bin. You can get it back from there.",
        Discard::Delete => "deleted for good. This cannot be undone.",
    }
}

fn discard_named(word: &str) -> Option<Discard> {
    match word {
        "vault" | "trash" | ".trash" => Some(Discard::Trash),
        "system" | "bin" | "recycle_bin" => Some(Discard::System),
        "delete" | "permanently" => Some(Discard::Delete),
        _ => None,
    }
}

/// The folder templates are kept in unless the file says another.
pub const TEMPLATES: &str = malachite::template::FOLDER;

#[derive(Clone, Debug, PartialEq)]
pub struct Files {
    pub new_notes: NewNotes,
    /// The folder of `NewNotes::Folder`, from the top of the vault.
    pub new_notes_folder: PathBuf,
    pub relink: Relink,
    pub deleted: Discard,
    /// The folder whose notes are the templates. Empty, there are none.
    pub templates: PathBuf,
    /// The folder a daily note is made in. Empty is the top of the vault.
    pub daily_folder: PathBuf,
    /// The format of a daily note's name, in Moment.js's letters.
    pub daily_format: String,
    /// The note a daily note starts as, from the top of the vault and with
    /// or without its `.md`; filled in as a template is. Empty, a daily
    /// note starts with its name as a heading.
    pub daily_template: String,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            new_notes: NewNotes::default(),
            new_notes_folder: PathBuf::new(),
            relink: Relink::default(),
            deleted: Discard::default(),
            templates: PathBuf::from(TEMPLATES),
            daily_folder: PathBuf::new(),
            daily_format: malachite::DAILY_FORMAT.to_owned(),
            daily_template: String::new(),
        }
    }
}

impl Files {
    /// What a vault's settings file says, with what in it could not be made
    /// sense of, by the line it is on.
    pub fn parse(text: &str) -> (Self, Vec<(usize, String)>) {
        let mut files = Self::default();
        let (entries, stray) = conf::read(text);
        let mut problems: Vec<(usize, String)> = stray.into_iter().map(|line| (line, "is not a setting (`name = value`)".to_owned())).collect();
        for conf::Entry { line, section, key, value } in entries {
            let problem = match section.as_str() {
                SECTION => files.option(&key, &value).err(),
                "" => Some(format!("`{key}` is not under a [section]")),
                // How the app looks is the same in every vault.
                section if section == crate::appearance::SECTION || section.starts_with("colo") => {
                    Some(format!("[{section}] is the app's, not a vault's: it goes in the app's own {}", crate::appearance::FILE))
                }
                section => Some(format!("there is no section [{section}]")),
            };
            problems.extend(problem.map(|problem| (line, problem)));
        }
        (files, problems)
    }

    /// Take one setting, or say what is wrong with it.
    fn option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let word = value.trim().to_lowercase().replace(['-', ' '], "_");
        match key {
            "new_notes" => self.new_notes = NewNotes::named(&word).ok_or_else(|| format!("`{value}` is not where new notes go: it is beside, top or folder"))?,
            "new_notes_folder" => self.new_notes_folder = folder(value)?,
            "update_links" => self.relink = Relink::named(&word).ok_or_else(|| format!("`{value}` is not when to update links: it is always, ask or never"))?,
            "deleted" => self.deleted = discard_named(&word).ok_or_else(|| format!("`{value}` is not where deleted notes go: it is vault, system or delete"))?,
            "templates" => self.templates = folder(value)?,
            "daily_folder" => self.daily_folder = folder(value)?,
            "daily_format" => {
                let format = value.trim();
                if format.is_empty() {
                    return Err("a daily note needs a format for its name, such as YYYY-MM-DD".to_owned());
                }
                format.clone_into(&mut self.daily_format);
            }
            "daily_template" => {
                let note = value.trim().replace('\\', "/");
                folder(&note).map_err(|_| format!("`{}` is not a note inside the vault", value.trim()))?;
                self.daily_template = note.trim_matches('/').to_owned();
            }
            _ => return Err(format!("there is no setting called `{key}`")),
        }
        Ok(())
    }

    /// The file of the daily note's template in the vault at `root`, if it has one.
    pub fn daily_template_in(&self, root: &Path) -> Option<PathBuf> {
        if self.daily_template.is_empty() {
            return None;
        }
        let file = root.join(&self.daily_template);
        Some(if file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) { file } else { root.join(format!("{}.md", self.daily_template)) })
    }
}

/// A folder as the file writes it: from the top of the vault, with `/`
/// between its parts. Nothing at all is the top.
pub fn folder(written: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(written.trim().replace('\\', "/").trim_matches('/'));
    let inside = path.components().all(|part| matches!(part, Component::Normal(name) if !name.to_string_lossy().starts_with('.')));
    match inside {
        true => Ok(path),
        false => Err(format!("`{}` is not a folder inside the vault", written.trim())),
    }
}

/// How a folder is written in the file.
pub fn written(folder: &Path) -> String {
    crate::folders::display(folder)
}

/// A vault's settings file as it is first written: what it is, and then
/// every setting, commented out at its default, under what it is.
pub fn vault_template() -> String {
    format!(
        "# This vault's settings for Malachite.
#
# They are about its notes as files: where new ones start, what renaming and
# deleting do, and where its templates and daily notes are. They hold in this
# vault only, and go wherever it is copied. How the app looks is the same in
# every vault, and is in the app's own {}.
#
# This file is yours to edit. Malachite reads it when the vault is opened and
# whenever its window is come back to, and the settings dialog (Ctrl+,) writes
# what is changed there into it. A line is `name = value`; a line that starts
# with `#` is a comment. Every setting is here, commented out, with the value
# it has when left alone: take the `#` off a line and change its value.

{}",
        crate::appearance::FILE,
        template()
    )
}

/// The `[files]` section as the file is first written: every setting,
/// commented out at its default, under what it is.
pub fn template() -> String {
    let defaults = Files::default();
    let mut text = String::from("[files]\n");
    let mut line = |about: &str, key: &str, value: &str| text.push_str(&format!("# {about}\n# {key} = {value}\n"));
    line("Where a new note starts: beside the open note, at the top of the vault, or in the folder below.", "new_notes", defaults.new_notes.word());
    line("The folder for new notes, when new_notes is folder. Folders are written from the top of the vault.", "new_notes_folder", "");
    line("Whether renaming a note updates the links to it: always, ask or never.", "update_links", defaults.relink.word());
    line("Where deleted notes and folders go: the vault's .trash folder, the system's bin, or delete for good.", "deleted", discard_word(defaults.deleted));
    line("The folder whose notes are the templates.", "templates", &written(&defaults.templates));
    line("The folder a new daily note goes in. Empty is the top of the vault.", "daily_folder", "");
    line("The name of a daily note, in letters such as YYYY (year), MM (month), DD (day), dddd (weekday).", "daily_format", &defaults.daily_format);
    line("The note a new daily note starts as, if any, filled in as a template is.", "daily_template", "");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_says_nothing_of_files_is_the_defaults() {
        assert_eq!(Files::parse(""), (Files::default(), Vec::new()));
        assert_eq!(Files::parse(&vault_template()), (Files::default(), Vec::new()), "the file as first written sets nothing");
        let files = Files::default();
        assert_eq!((files.new_notes, files.relink, files.deleted), (NewNotes::Beside, Relink::Always, Discard::Trash), "what the app did before these were settings");
        assert_eq!(files.templates, Path::new("Templates"));
    }

    #[test]
    fn the_section_sets_what_it_names() {
        let text = "[Files]\nnew-notes = Folder\nnew_notes_folder = Inbox/\\Today\\\nupdate_links = ask\ndeleted = Delete\ntemplates = Meta/Templates\ndaily_folder = Journal\ndaily_format = D MMMM YYYY\ndaily_template = Meta/Templates/Day.md\n";
        let (files, problems) = Files::parse(text);
        assert_eq!(problems, Vec::new());
        assert_eq!(
            files,
            Files {
                new_notes: NewNotes::Folder,
                new_notes_folder: PathBuf::from("Inbox/Today"),
                relink: Relink::Ask,
                deleted: Discard::Delete,
                templates: PathBuf::from("Meta/Templates"),
                daily_folder: PathBuf::from("Journal"),
                daily_format: "D MMMM YYYY".to_owned(),
                daily_template: "Meta/Templates/Day.md".to_owned(),
            }
        );
        let root = Path::new("vault");
        assert_eq!(files.daily_template_in(root), Some(root.join("Meta/Templates/Day.md")));
        let bare = Files { daily_template: "Meta/Day".to_owned(), ..Files::default() };
        assert_eq!(bare.daily_template_in(root), Some(root.join("Meta/Day.md")), "the .md may be left off");
        assert_eq!(Files::default().daily_template_in(root), None);
    }

    #[test]
    fn what_cannot_be_understood_is_reported_by_its_line() {
        let text = "[files]\nnew_notes = sideways\ntemplates = ../Elsewhere\ndaily_folder = .hidden\ndeleted = shredder\nupdate_links = sometimes\ndaily_format =\ncolour = red\ndaily_template = C:/x.md\nnonsense\n[appearance]\ntheme = gem\n[editor]\ntabs = 4\n";
        let (files, mut problems) = Files::parse(text);
        problems.sort();
        assert_eq!(
            problems,
            [
                (2, "`sideways` is not where new notes go: it is beside, top or folder".to_owned()),
                (3, "`../Elsewhere` is not a folder inside the vault".to_owned()),
                (4, "`.hidden` is not a folder inside the vault".to_owned()),
                (5, "`shredder` is not where deleted notes go: it is vault, system or delete".to_owned()),
                (6, "`sometimes` is not when to update links: it is always, ask or never".to_owned()),
                (7, "a daily note needs a format for its name, such as YYYY-MM-DD".to_owned()),
                (8, "there is no setting called `colour`".to_owned()),
                (9, "`C:/x.md` is not a note inside the vault".to_owned()),
                (10, "is not a setting (`name = value`)".to_owned()),
                (12, "[appearance] is the app's, not a vault's: it goes in the app's own malachite.conf".to_owned()),
                (14, "there is no section [editor]".to_owned()),
            ]
        );
        assert_eq!(files, Files::default(), "and passed over");
    }
}
