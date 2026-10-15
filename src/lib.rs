//! Malachite's vault logic, shared by the app (`malachite`) and the command line (`malachite-cli`).
//!
//! The model:
//! - A *vault* is a plain folder; every note is a Markdown file (`<title>.md`).
//! - The note title is the file name. New notes default to `Untitled`,
//!   then `Untitled 1`, `Untitled 2`, ... to avoid overwriting.
//! - Notes may start with YAML frontmatter (`---` block).
//! - `[[Wikilinks]]` connect notes; backlinks are the notes linking *to* one.
//! - `#tags` mark notes; a daily note is named for its day (`YYYY-MM-DD`
//!   unless told another format).
//! - Notes can live in subfolders; `[[Note]]` resolves by file stem, and
//!   failing that by the `aliases` a note lists in its frontmatter.

pub mod editing;
pub mod graph;
pub mod highlight;
pub mod markdown;
pub mod outline;
mod recycle;
pub mod search;
pub mod template;

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDate};

use crate::markdown::{Kind, Link, Span};

pub struct Note {
    pub title: String,
    pub path: PathBuf,
    pub content: String,
}

impl Note {
    /// Whether a link to `name` can mean this note: by its title or by one
    /// of its aliases, capitals aside.
    pub fn is_called(&self, name: &str) -> bool {
        self.title.eq_ignore_ascii_case(name) || aliases(&self.content).iter().any(|alias| alias.eq_ignore_ascii_case(name))
    }
}

/// Which of `notes` a link to `name` leads to: the
/// note with that title, or else the first that has it as an alias.
pub fn resolve(notes: &[Note], name: &str) -> Option<usize> {
    let titled = notes.iter().position(|n| n.title.eq_ignore_ascii_case(name));
    titled.or_else(|| notes.iter().position(|n| n.is_called(name)))
}

/// What becomes of a note or a folder that is deleted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Discard {
    /// Moved to the vault's own `.trash` folder.
    #[default]
    Trash,
    /// Moved to the system's bin (the Recycle Bin on Windows), where there is one to be had.
    System,
    /// Gone for good.
    Delete,
}

/// Whether the system's bin can be used here.
pub const SYSTEM_BIN: bool = recycle::AVAILABLE;

/// The format a daily note's name is written in unless told another, in
/// Moment.js's letters (see `template`).
pub const DAILY_FORMAT: &str = "YYYY-MM-DD";

/// `text` with its links to the note `from` made links to `to`. Only the
/// note's name changes: a heading, a block or the text shown stays as it was.
fn relinked(text: &str, from: &str, to: &str) -> String {
    let mut text = text.to_owned();
    for suffix in ["]]", "|", "#"] {
        text = text.replace(&format!("[[{from}{suffix}"), &format!("[[{to}{suffix}"));
    }
    text
}

/// How many of `notes` link to the note called `title` in a way that
/// renaming it would rewrite.
pub fn linking(notes: &[Note], title: &str) -> usize {
    notes.iter().filter(|note| relinked(&note.content, title, "\0") != note.content).count()
}

pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Open a vault directory, creating it if missing.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// All notes in the vault, sorted by path.
    pub fn notes(&self) -> Result<Vec<Note>> {
        let mut paths = Vec::new();
        collect(&self.root, &mut paths)?;
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let title = path.file_stem().unwrap().to_string_lossy().into_owned();
                Ok(Note { title, path, content })
            })
            .collect()
    }

    /// The note with this title, or failing that with this alias.
    pub fn find(&self, title: &str) -> Result<Note> {
        let mut notes = self.notes()?;
        let index = resolve(&notes, title).with_context(|| format!("no note titled '{title}'"))?;
        Ok(notes.swap_remove(index))
    }

    /// The file a note embeds as `name`: at that path from the vault's root,
    /// or else the file with that name wherever it is
    /// in the vault. Never a file outside the vault or in a hidden folder.
    pub fn attachment(&self, name: &str) -> Option<PathBuf> {
        let wanted = Path::new(name);
        let inside = wanted.components().all(|part| matches!(part, Component::Normal(name) if !name.to_string_lossy().starts_with('.')));
        if !inside || name.is_empty() {
            return None;
        }
        let direct = self.root.join(wanted);
        if direct.is_file() {
            return Some(direct);
        }
        let file = wanted.file_name()?.to_string_lossy().to_lowercase();
        let mut found = Vec::new();
        collect_named(&self.root, &file, &mut found).ok()?;
        // The same one every time, when several folders have one.
        found.into_iter().min()
    }

    /// Whether the note in the file `note` is a template: one kept in the
    /// templates folder `templates` (relative to the root), or in a folder
    /// inside that. Capitals do not count, as they do not on Windows.
    pub fn is_template(&self, note: &Path, templates: &Path) -> bool {
        let parts = |path: &Path| path.components().map(|part| part.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>();
        let (folder, templates) = (parts(&self.folder_of(note)), parts(templates));
        !templates.is_empty() && folder.starts_with(&templates)
    }

    /// Every folder in the vault, as paths relative to its root, sorted.
    /// Hidden folders (`.trash`, the settings folders of other apps) are left out.
    pub fn folders(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        collect_folders(&self.root, Path::new(""), &mut out)?;
        out.sort();
        Ok(out)
    }

    /// The folder a note's file is in, relative to the vault's root. It is
    /// empty for a note at the top level.
    pub fn folder_of(&self, note: &Path) -> PathBuf {
        let folder = note.parent().and_then(|dir| dir.strip_prefix(&self.root).ok());
        folder.unwrap_or(Path::new("")).to_path_buf()
    }

    /// The directory of a folder given relative to the vault's root. A path
    /// that would lead out of the vault, or into a hidden folder, is refused.
    fn dir(&self, folder: &Path) -> Result<PathBuf> {
        let inside = folder
            .components()
            .all(|part| matches!(part, Component::Normal(name) if !name.to_string_lossy().starts_with('.')));
        if !inside {
            bail!("'{}' is not a folder in this vault", folder.display());
        }
        Ok(if folder.as_os_str().is_empty() { self.root.clone() } else { self.root.join(folder) })
    }

    /// Titles of all notes, lowercased.
    fn titles(&self) -> Result<HashSet<String>> {
        let mut paths = Vec::new();
        collect(&self.root, &mut paths)?;
        Ok(paths.iter().filter_map(|p| p.file_stem()).map(|stem| stem.to_string_lossy().to_lowercase()).collect())
    }

    /// Create a note at the top level of the vault. See [`Self::create_in`].
    pub fn create(&self, title: Option<&str>, body: &str, tags: &[String]) -> Result<Note> {
        self.create_in(Path::new(""), title, body, tags)
    }

    /// Create a note in `folder` (relative to the root; made if missing).
    /// The title defaults to "Untitled". Links find notes by title, so a
    /// title that any note in the vault already has is numbered until it is new.
    pub fn create_in(&self, folder: &Path, title: Option<&str>, body: &str, tags: &[String]) -> Result<Note> {
        let wanted = sanitize(title.unwrap_or("Untitled"));
        if wanted.is_empty() {
            bail!("title is empty after sanitizing");
        }
        let dir = self.dir(folder)?;
        fs::create_dir_all(&dir)?;
        let taken = self.titles()?;
        let mut title = wanted.clone();
        let mut n = 1;
        while taken.contains(&title.to_lowercase()) {
            title = format!("{wanted} {n}");
            n += 1;
        }
        let path = dir.join(format!("{title}.md"));
        let mut content = String::new();
        if !tags.is_empty() {
            content.push_str(&format!("---\ntags: [{}]\n---\n", tags.join(", ")));
        }
        content.push_str(body);
        fs::write(&path, &content)?;
        Ok(Note { title, path, content })
    }

    /// Move the note in the file `note` into `folder` (relative to the root;
    /// made if missing). Links are by title, so none need rewriting. Returns
    /// the note's new path.
    pub fn move_note(&self, note: &Path, folder: &Path) -> Result<PathBuf> {
        let name = note.file_name().with_context(|| format!("'{}' is not a note", note.display()))?;
        let dir = self.dir(folder)?;
        let dest = dir.join(name);
        if dest == note {
            return Ok(dest);
        }
        if dest.exists() {
            let title = Path::new(name).file_stem().unwrap_or(name).to_string_lossy();
            bail!("there is already a note called '{title}' there");
        }
        fs::create_dir_all(&dir)?;
        fs::rename(note, &dest)?;
        Ok(dest)
    }

    /// Make a folder called `name` inside `parent` (relative to the root),
    /// numbering the name if it is taken. Returns its path relative to the root.
    pub fn create_folder(&self, parent: &Path, name: &str) -> Result<PathBuf> {
        let wanted = folder_name(name)?;
        let dir = self.dir(parent)?;
        let mut name = wanted.clone();
        let mut n = 1;
        while dir.join(&name).exists() {
            name = format!("{wanted} {n}");
            n += 1;
        }
        fs::create_dir_all(dir.join(&name))?;
        Ok(parent.join(name))
    }

    /// Give `folder` (relative to the root) a new name, keeping it where it
    /// is. Returns its new path relative to the root.
    pub fn rename_folder(&self, folder: &Path, name: &str) -> Result<PathBuf> {
        let old = folder.file_name().context("the top level of the vault cannot be renamed")?;
        let from = self.dir(folder)?;
        let name = folder_name(name)?;
        let renamed = folder.with_file_name(&name);
        // On case-insensitive filesystems a case-only rename "exists" as the folder itself.
        if self.root.join(&renamed).exists() && !old.to_string_lossy().eq_ignore_ascii_case(&name) {
            bail!("there is already a folder called '{name}' there");
        }
        fs::rename(from, self.root.join(&renamed))?;
        Ok(renamed)
    }

    /// Move `folder` (relative to the root) and everything in it into the
    /// vault's `.trash` folder. Returns where it went.
    pub fn trash_folder(&self, folder: &Path) -> Result<PathBuf> {
        let name = folder.file_name().context("the top level of the vault cannot be deleted")?.to_string_lossy();
        let from = self.dir(folder)?;
        let trash = self.root.join(".trash");
        fs::create_dir_all(&trash)?;
        let mut dest = trash.join(&*name);
        let mut n = 1;
        while dest.exists() {
            dest = trash.join(format!("{name} {n}"));
            n += 1;
        }
        fs::rename(from, &dest)?;
        Ok(dest)
    }

    /// Today's daily note (`YYYY-MM-DD.md`) and whether it was just created.
    pub fn daily(&self) -> Result<(Note, bool)> {
        self.daily_in(Path::new(""), DAILY_FORMAT, |title| format!("# {title}\n"))
    }

    /// Today's daily note, named for today in `format` (Moment.js's letters,
    /// see `template`), and whether it was just created. It is wherever a
    /// note of that name is, as names are the vault's to give out once; or
    /// else it is made in `folder` (relative to the root), with the text
    /// `body` gives for its name.
    pub fn daily_in(&self, folder: &Path, format: &str, body: impl FnOnce(&str) -> String) -> Result<(Note, bool)> {
        let title = template::formatted(Local::now().naive_local(), format);
        if title.trim().is_empty() || sanitize(&title) != title {
            bail!("'{title}' cannot be the name of a note: the daily note format '{format}' gives characters a file name cannot have");
        }
        if let Some(note) = self.notes()?.into_iter().find(|note| note.title.eq_ignore_ascii_case(&title)) {
            return Ok((note, false));
        }
        let dir = self.dir(folder)?;
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{title}.md"));
        let content = body(&title);
        fs::write(&path, &content)?;
        Ok((Note { title, path, content }, true))
    }

    pub fn append(&self, title: &str, text: &str) -> Result<()> {
        let n = self.find(title)?;
        let mut c = n.content;
        if !c.is_empty() && !c.ends_with('\n') {
            c.push('\n');
        }
        c.push_str(text);
        c.push('\n');
        fs::write(&n.path, c)?;
        Ok(())
    }

    /// Rename a note and rewrite `[[links]]` pointing to it.
    /// Returns the old title, the new title and how many notes were updated.
    pub fn rename(&self, from: &str, to: &str) -> Result<(String, String, usize)> {
        let notes = self.notes()?;
        let found = notes.iter().position(|n| n.title.eq_ignore_ascii_case(from));
        rename_in(&notes, found.with_context(|| format!("no note titled '{from}'"))?, to, true)
    }

    /// Like [`Self::rename`], for the note in the file `path`. Notes in
    /// different folders can share a title; a path always means one note.
    /// Without `relink`, the links to it are left as they are, to a note
    /// that is no longer called that.
    pub fn rename_at(&self, path: &Path, to: &str, relink: bool) -> Result<(String, String, usize)> {
        let notes = self.notes()?;
        let found = notes.iter().position(|n| n.path == path);
        rename_in(&notes, found.with_context(|| format!("no note at '{}'", path.display()))?, to, relink)
    }

    /// Permanently delete a note.
    pub fn delete(&self, title: &str) -> Result<Note> {
        let n = self.find(title)?;
        fs::remove_file(&n.path)?;
        Ok(n)
    }

    /// Move a note into the vault's `.trash` folder: a delete that can be undone.
    pub fn trash(&self, title: &str) -> Result<PathBuf> {
        self.trash_at(&self.find(title)?.path)
    }

    /// Delete the note in the file `path` in the way `how` says. Returns
    /// where it went, if it went somewhere it can be found again here.
    pub fn discard_at(&self, path: &Path, how: Discard) -> Result<Option<PathBuf>> {
        if how == Discard::Trash {
            return self.trash_at(path).map(Some);
        }
        if !path.starts_with(&self.root) || path.extension().map_or(true, |ext| ext != "md") {
            bail!("'{}' is not a note in this vault", path.display());
        }
        match how {
            Discard::System => recycle::recycle(path)?,
            _ => fs::remove_file(path)?,
        }
        Ok(None)
    }

    /// Delete `folder` (relative to the root) and everything in it in the
    /// way `how` says. Returns where it went, as [`Self::discard_at`] does.
    pub fn discard_folder(&self, folder: &Path, how: Discard) -> Result<Option<PathBuf>> {
        if how == Discard::Trash {
            return self.trash_folder(folder).map(Some);
        }
        folder.file_name().context("the top level of the vault cannot be deleted")?;
        let dir = self.dir(folder)?;
        match how {
            Discard::System => recycle::recycle(&dir)?,
            _ => fs::remove_dir_all(&dir)?,
        }
        Ok(None)
    }

    /// Like [`Self::trash`], for the note in the file `path`.
    pub fn trash_at(&self, path: &Path) -> Result<PathBuf> {
        if !path.starts_with(&self.root) {
            bail!("'{}' is not in this vault", path.display());
        }
        let title = path.file_stem().with_context(|| format!("'{}' is not a note", path.display()))?;
        let dir = self.root.join(".trash");
        fs::create_dir_all(&dir)?;
        let dest = unique_path(&dir, &title.to_string_lossy());
        fs::rename(path, &dest)?;
        Ok(dest)
    }
}

/// Rename `notes[index]` to `to` and, with `relink`, rewrite the `[[links]]`
/// to it in all `notes`.
fn rename_in(notes: &[Note], index: usize, to: &str, relink: bool) -> Result<(String, String, usize)> {
    let n = &notes[index];
    let to = sanitize(to);
    if to.is_empty() {
        bail!("title is empty after sanitizing");
    }
    let new_path = n.path.with_file_name(format!("{to}.md"));
    // On case-insensitive filesystems a case-only rename "exists" as the note itself.
    if new_path.exists() && !to.eq_ignore_ascii_case(&n.title) {
        bail!("'{to}' already exists");
    }
    fs::rename(&n.path, &new_path)?;
    let mut updated = 0;
    for other in notes.iter().filter(|_| relink) {
        let text = relinked(&other.content, &n.title, &to);
        if text != other.content {
            let p = if other.path == n.path { &new_path } else { &other.path };
            fs::write(p, text)?;
            updated += 1;
        }
    }
    Ok((n.title.clone(), to, updated))
}

/// A usable folder name from what was typed.
fn folder_name(name: &str) -> Result<String> {
    let name = sanitize(name);
    if name.is_empty() {
        bail!("a folder needs a name");
    }
    if name.starts_with('.') {
        bail!("a folder's name cannot start with a dot: such folders are hidden");
    }
    Ok(name)
}

/// Folders under `dir`, as paths relative to the vault root, given that
/// `dir` itself is `relative` to it.
fn collect_folders(dir: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.path().is_dir() && !name.to_string_lossy().starts_with('.') {
            let folder = relative.join(&name);
            collect_folders(&entry.path(), &folder, out)?;
            out.push(folder);
        }
    }
    Ok(())
}

/// The files under `dir` called `name` (in lower case), capitals aside,
/// leaving hidden folders out.
fn collect_named(dir: &Path, name: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let own = entry.file_name().to_string_lossy().to_lowercase();
        if path.is_dir() {
            if !own.starts_with('.') {
                collect_named(&path, name, out)?;
            }
        } else if own == name {
            out.push(path);
        }
    }
    Ok(())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            // Apps keep files of their own in hidden folders of a vault; skip hidden dirs.
            if !p.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.')) {
                collect(&p, out)?;
            }
        } else if p.extension().map_or(false, |e| e == "md") {
            out.push(p);
        }
    }
    Ok(())
}

/// Characters not allowed in file names on common filesystems.
pub fn sanitize(title: &str) -> String {
    title
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// "Untitled", then "Untitled 1", "Untitled 2", ...
fn unique_path(dir: &Path, title: &str) -> PathBuf {
    let mut p = dir.join(format!("{title}.md"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{title} {n}.md"));
        n += 1;
    }
    p
}

/// Extract `[[target]]`, `[[target|alias]]`, `[[target#heading]]` targets.
/// A link inside a code block or inline code does not count.
pub fn wikilinks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in markdown::parse(text) {
        // Properties are not parsed as Markdown, but they can link to notes.
        let spans = match line.kind {
            Kind::Frontmatter => markdown::inline(text, line.range),
            _ => line.spans,
        };
        each_link(text, &spans, &mut |span, link| match link {
            // `[text](Note.md)` is a `Link::Note` too; only `[[...]]` is a wikilink.
            Link::Note(target, _) if !target.is_empty() && text[span.element.clone()].starts_with("[[") => {
                out.push(target.clone());
            }
            // A note shown inside another is linked to from it.
            Link::Embed { target, .. } if !target.is_empty() && !is_attachment(target) && text[span.element.clone()].starts_with("![[") => {
                out.push(target.clone());
            }
            _ => {}
        });
    }
    out
}

/// Inline `#tags` in the body plus `tags:` entries from frontmatter.
/// A `#word` inside a code block or inline code is not a tag.
pub fn tags(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in markdown::parse(text) {
        if line.kind == Kind::Frontmatter {
            if let Some(rest) = text[line.range.clone()].trim().strip_prefix("tags:") {
                let list = rest.trim().trim_start_matches('[').trim_end_matches(']');
                out.extend(
                    list.split(',')
                        .map(|t| t.trim().trim_start_matches('#').to_string())
                        .filter(|t| !t.is_empty()),
                );
            }
            continue;
        }
        each_link(text, &line.spans, &mut |_, link| {
            if let Link::Tag(tag) = link {
                out.push(tag.clone());
            }
        });
    }
    out.sort();
    out.dedup();
    out
}

/// The kinds of file a note can embed that this app shows: pictures.
pub const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "bmp"];

/// Other files a note may embed. They are known for what they
/// are, files and not notes, but are not shown.
const OTHER_ATTACHMENTS: [&str; 12] = ["webp", "svg", "avif", "pdf", "mp3", "wav", "m4a", "ogg", "flac", "mp4", "webm", "mov"];

fn extension(target: &str) -> Option<String> {
    Path::new(target).extension().map(|extension| extension.to_string_lossy().to_lowercase())
}

/// Whether what an embed names is a file rather than a note: `![[a.png]]`
/// is, `![[Version 1.2]]` is not.
pub fn is_attachment(target: &str) -> bool {
    extension(target).is_some_and(|e| PICTURES.contains(&e.as_str()) || OTHER_ATTACHMENTS.contains(&e.as_str()))
}

/// Whether what an embed names is a picture that can be shown.
pub fn is_picture(target: &str) -> bool {
    extension(target).is_some_and(|e| PICTURES.contains(&e.as_str()))
}

/// The other names a note answers to: the `aliases` (or `alias`) of its
/// frontmatter, written as `[one, two]`, as a single name, or as a list of
/// `- name` lines under the key.
pub fn aliases(text: &str) -> Vec<String> {
    let opened = text.strip_prefix("---").filter(|rest| rest.starts_with('\n') || rest.starts_with("\r\n"));
    let Some(rest) = opened else { return Vec::new() };
    let unquoted = |name: &str| name.trim().trim_matches(['"', '\'']).trim().to_string();
    let mut out = Vec::new();
    // Whether the lines being read are the `- name` lines of the key.
    let mut listing = false;
    for line in rest.lines().skip(1) {
        let entry = line.trim();
        if line.trim_end() == "---" {
            out.retain(|alias: &String| !alias.is_empty());
            return out;
        }
        let key = ["aliases:", "alias:"].into_iter().find_map(|key| line.strip_prefix(key));
        if let Some(value) = key.map(str::trim) {
            listing = value.is_empty();
            out.extend(value.trim_start_matches('[').trim_end_matches(']').split(',').map(unquoted));
        } else if let Some(name) = entry.strip_prefix('-').filter(|_| listing) {
            out.push(unquoted(name));
        } else {
            listing = false;
        }
    }
    // Frontmatter that is never closed is not frontmatter.
    Vec::new()
}

/// The day a daily note is for, if `title` is the name of one (`YYYY-MM-DD`).
pub fn daily_date(title: &str, format: &str) -> Option<NaiveDate> {
    template::read_date(title, format)
}

/// Which of `notes` is the daily note nearest before `date`, or with `later`
/// the nearest after it. Days without a note are passed over.
pub fn daily_neighbour(notes: &[Note], date: NaiveDate, later: bool, format: &str) -> Option<usize> {
    let days = notes.iter().enumerate().filter_map(|(index, note)| Some((daily_date(&note.title, format)?, index)));
    let nearest = if later {
        days.filter(|(day, _)| *day > date).min()
    } else {
        days.filter(|(day, _)| *day < date).max()
    };
    nearest.map(|(_, index)| index)
}

/// Call `found` for every link in `spans`, in order. The parser gives the
/// lines of a code block no spans and never looks inside inline code, so none
/// of these links come from code. A `%%comment%%` is one opaque span to it as
/// well, but what is written in a comment still counts, so those are read here.
fn each_link(text: &str, spans: &[Span], found: &mut impl FnMut(&Span, &Link)) {
    for span in spans {
        if span.style.comment {
            let inner = span.range.start + 2..span.range.end - 2;
            each_link(text, &markdown::inline(text, inner), found);
        } else if let Some(link) = &span.link {
            found(span, link);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh vault in the system temp dir, removed when dropped.
    struct TempVault(Vault);

    impl TempVault {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("malachite-lib-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(Vault::open(dir).unwrap())
        }
    }

    impl Drop for TempVault {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.root());
        }
    }

    fn titles(v: &Vault) -> Vec<String> {
        v.notes().unwrap().into_iter().map(|n| n.title).collect()
    }

    #[test]
    fn untitled_notes_are_numbered() {
        let v = TempVault::new("untitled");
        for _ in 0..3 {
            v.0.create(None, "", &[]).unwrap();
        }
        assert_eq!(titles(&v.0), ["Untitled 1", "Untitled 2", "Untitled"]);
    }

    #[test]
    fn create_sanitizes_title_and_writes_frontmatter() {
        let v = TempVault::new("create");
        let n = v.0.create(Some("a/b: c?"), "body", &["x".into(), "y".into()]).unwrap();
        assert_eq!(n.title, "a-b- c-");
        assert_eq!(fs::read_to_string(n.path).unwrap(), "---\ntags: [x, y]\n---\nbody");
        assert!(v.0.create(Some("  "), "", &[]).is_err());
    }

    #[test]
    fn wikilinks_handle_aliases_and_headings() {
        let text = "[[A]] and [[B|alias]] and [[C#heading]] [[ ]] [[broken";
        assert_eq!(wikilinks(text), ["A", "B", "C"]);
    }

    #[test]
    fn aliases_are_read_from_the_frontmatter_however_they_are_listed() {
        assert_eq!(aliases("---\naliases: [One, \"Two words\", ]\n---\ntext"), ["One", "Two words"]);
        assert_eq!(aliases("---\r\nalias: Single\r\n---\r\n"), ["Single"]);
        assert_eq!(aliases("---\ntags: [x]\naliases:\n  - One\n  - 'Two'\nother:\n  - not one\n---\n"), ["One", "Two"]);
        assert!(aliases("aliases: [One]").is_empty(), "only frontmatter has them");
        assert!(aliases("---\naliases: [One]\ntext").is_empty(), "and only when it is closed");
        assert!(aliases("---\n  aliases: [One]\n---\n").is_empty(), "a nested key is something else");
    }

    #[test]
    fn a_note_is_found_by_its_alias_when_no_title_matches() {
        let v = TempVault::new("aliases");
        v.0.create(Some("Long name"), "---\naliases: [LN, Other]\n---\nbody", &[]).unwrap();
        v.0.create(Some("Other"), "the real one", &[]).unwrap();
        assert_eq!(v.0.find("ln").unwrap().title, "Long name");
        assert_eq!(v.0.find("Other").unwrap().title, "Other", "a title comes before an alias");
        assert!(v.0.find("Nobody").is_err());
    }

    #[test]
    fn daily_notes_know_their_neighbours() {
        let note = |title: &str| Note { title: title.into(), path: PathBuf::new(), content: String::new() };
        let notes = ["2026-03-09", "Plan", "2026-03-01", "2026-02-27", "2026-13-40"].map(note);
        let day = |title: &str| daily_date(title, DAILY_FORMAT).unwrap();
        let near = |title: &str, later| daily_neighbour(&notes, day(title), later, DAILY_FORMAT).map(|index| notes[index].title.as_str());
        assert_eq!(near("2026-03-01", false), Some("2026-02-27"));
        assert_eq!(near("2026-03-01", true), Some("2026-03-09"));
        assert_eq!(near("2026-03-05", false), Some("2026-03-01"), "from a day that has no note");
        assert_eq!((near("2026-02-27", false), near("2026-03-09", true)), (None, None));
        assert!(daily_date("Plan", DAILY_FORMAT).is_none() && daily_date("2026-13-40", DAILY_FORMAT).is_none());
    }

    #[test]
    fn templates_are_the_notes_in_the_templates_folder() {
        let v = TempVault::new("templates");
        let inside = v.0.create_in(Path::new("templates/Work"), Some("Meeting"), "", &[]).unwrap();
        let outside = v.0.create_in(Path::new("Work/Templates"), Some("Plan"), "", &[]).unwrap();
        let templates = Path::new(template::FOLDER);
        assert!(v.0.is_template(&inside.path, templates));
        assert!(!v.0.is_template(&outside.path, templates));
        // Another folder, deeper down, when the settings say so.
        assert!(v.0.is_template(&outside.path, Path::new("work/templates")));
        assert!(!v.0.is_template(&inside.path, Path::new("work/templates")));
        assert!(!v.0.is_template(&inside.path, Path::new("")), "no folder is no templates");
    }

    #[test]
    fn an_embedded_note_is_linked_to_and_an_embedded_file_is_not() {
        let text = "![[Other]] ![[Other#Part]] ![[pic.png]] ![[Version 1.2]] ![alt](Third.md)";
        assert_eq!(wikilinks(text), ["Other", "Other", "Version 1.2"]);
        assert!(is_attachment("a/b/Scan.PDF") && is_picture("x.JPG") && !is_picture("x.pdf") && !is_attachment("Version 1.2"));
    }

    #[test]
    fn an_attachment_is_found_by_path_or_by_name_and_only_in_the_vault() {
        let v = TempVault::new("attachments");
        let root = v.0.root().to_path_buf();
        fs::create_dir_all(root.join("img/deep")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        for file in ["img/deep/Cat.png", "img/dog.png", ".hidden/secret.png"] {
            fs::write(root.join(file), "x").unwrap();
        }
        assert_eq!(v.0.attachment("img/dog.png"), Some(root.join("img/dog.png")));
        assert_eq!(v.0.attachment("cat.png"), Some(root.join("img/deep").join("Cat.png")), "by name, capitals aside");
        assert_eq!(v.0.attachment("nowhere.png"), None);
        assert_eq!(v.0.attachment("secret.png"), None);
        assert_eq!(v.0.attachment(".hidden/secret.png"), None);
        assert_eq!(v.0.attachment("../outside.png"), None);
        assert_eq!(v.0.attachment(&root.join("img/dog.png").to_string_lossy()), None, "no paths from outside the vault");
    }

    #[test]
    fn tags_come_from_frontmatter_and_body() {
        let text = "---\ntags: [project, #idea]\n---\n# Heading\nsome #rust, and #2024 text";
        assert_eq!(tags(text), ["idea", "project", "rust"]);
    }

    #[test]
    fn fenced_code_has_no_tags_or_wikilinks() {
        for fence in ["```", "~~~"] {
            let block = format!("{fence}cpp\n#include <iostream>\ncolor: #a882ff; #tag [[Note]]\n{fence}");
            assert!(tags(&block).is_empty(), "{block}");
            assert!(wikilinks(&block).is_empty(), "{block}");
            // The same tag and link around the block still count.
            let text = format!("#tag [[Note]]\n{block}\n#after [[Other]]");
            assert_eq!(tags(&text), ["after", "tag"], "{text}");
            assert_eq!(wikilinks(&text), ["Note", "Other"], "{text}");
        }
        // A block that is never closed runs to the end of the note.
        assert!(tags("```\n#tag").is_empty());
        assert!(wikilinks("~~~\n[[Note]]").is_empty());
    }

    #[test]
    fn inline_code_has_no_tags_or_wikilinks() {
        let code = "`#tag [[Note]]` and ``#tag ` [[Note]]``";
        assert!(tags(code).is_empty());
        assert!(wikilinks(code).is_empty());
        let text = format!("#tag {code} [[Note|alias]] `unclosed #real [[Real#heading]]");
        assert_eq!(tags(&text), ["real", "tag"]);
        assert_eq!(wikilinks(&text), ["Note", "Real"]);
        // Three backticks can open inline code too; that does not start a block.
        let text = "```#tag [[Note]]``` #real\n[[Real]]";
        assert_eq!(tags(text), ["real"]);
        assert_eq!(wikilinks(text), ["Real"]);
    }

    #[test]
    fn frontmatter_and_comments_are_still_read() {
        let text = "---\ntags: [a]\nup: \"[[Parent]]\" #yaml-comment\n---\n%% #hidden [[Hidden]] %% [text](Other.md)";
        assert_eq!(tags(text), ["a", "hidden"]);
        assert_eq!(wikilinks(text), ["Parent", "Hidden"]);
    }

    #[test]
    fn rename_updates_links_in_other_notes() {
        let v = TempVault::new("rename");
        v.0.create(Some("Old"), "self [[Old]]", &[]).unwrap();
        v.0.create(Some("Ref"), "[[Old]] [[Old|alias]] [[Old#h]] [[Older]]", &[]).unwrap();
        let (from, to, updated) = v.0.rename("old", "New").unwrap();
        assert_eq!((from.as_str(), to.as_str(), updated), ("Old", "New", 2));
        assert_eq!(v.0.find("New").unwrap().content, "self [[New]]");
        assert_eq!(v.0.find("Ref").unwrap().content, "[[New]] [[New|alias]] [[New#h]] [[Older]]");
        assert!(v.0.find("Old").is_err());
    }

    #[test]
    fn rename_refuses_to_overwrite() {
        let v = TempVault::new("rename-clash");
        v.0.create(Some("A"), "a", &[]).unwrap();
        v.0.create(Some("B"), "b", &[]).unwrap();
        assert!(v.0.rename("A", "B").is_err());
        assert_eq!(v.0.find("B").unwrap().content, "b");
    }

    #[test]
    fn trash_moves_note_out_of_the_listing() {
        let v = TempVault::new("trash");
        v.0.create(Some("Gone"), "keep me", &[]).unwrap();
        let dest = v.0.trash("Gone").unwrap();
        assert!(titles(&v.0).is_empty());
        assert_eq!(fs::read_to_string(dest).unwrap(), "keep me");
    }

    #[test]
    fn append_adds_a_line() {
        let v = TempVault::new("append");
        v.0.create(Some("Log"), "one", &[]).unwrap();
        v.0.append("Log", "two").unwrap();
        assert_eq!(v.0.find("Log").unwrap().content, "one\ntwo\n");
    }

    /// Folder paths as text with `/` between the parts, whatever the platform.
    fn folders(v: &Vault) -> Vec<String> {
        v.folders().unwrap().iter().map(|f| f.to_string_lossy().replace('\\', "/")).collect()
    }

    #[test]
    fn notes_are_created_in_folders_and_folders_are_listed() {
        let v = TempVault::new("folders");
        let note = v.0.create_in(Path::new("Projects/Notes app"), Some("Plan"), "p", &[]).unwrap();
        assert_eq!(v.0.folder_of(&note.path), Path::new("Projects/Notes app"));
        assert_eq!(v.0.folder_of(&v.0.create(Some("Top"), "", &[]).unwrap().path), Path::new(""));
        v.0.create_folder(Path::new(""), "Empty").unwrap();
        assert_eq!(folders(&v.0), ["Empty", "Projects", "Projects/Notes app"], "empty folders count too");
        assert_eq!(titles(&v.0), ["Plan", "Top"]);
        // The trash and other hidden folders are not part of the vault's tree.
        v.0.trash("Top").unwrap();
        assert_eq!(folders(&v.0), ["Empty", "Projects", "Projects/Notes app"]);
    }

    #[test]
    fn new_titles_are_unique_across_the_whole_vault() {
        let v = TempVault::new("unique");
        v.0.create_in(Path::new("A"), None, "", &[]).unwrap();
        let second = v.0.create_in(Path::new("B"), None, "", &[]).unwrap();
        assert_eq!(second.title, "Untitled 1", "a link to Untitled must mean one note");
        assert_eq!(v.0.create(Some("untitled"), "", &[]).unwrap().title, "untitled 2");
    }

    #[test]
    fn a_note_moves_between_folders_keeping_its_links() {
        let v = TempVault::new("move");
        let plan = v.0.create(Some("Plan"), "see [[Ideas]]", &[]).unwrap();
        v.0.create(Some("Ideas"), "back to [[Plan]]", &[]).unwrap();
        let moved = v.0.move_note(&plan.path, Path::new("Projects/2026")).unwrap();
        assert_eq!(v.0.folder_of(&moved), Path::new("Projects/2026"));
        assert_eq!(v.0.find("Plan").unwrap().content, "see [[Ideas]]");
        assert_eq!(v.0.find("Ideas").unwrap().content, "back to [[Plan]]", "links are by title, so still right");
        // Moving to where it already is changes nothing; back to the top level works.
        assert_eq!(v.0.move_note(&moved, Path::new("Projects/2026")).unwrap(), moved);
        let top = v.0.move_note(&moved, Path::new("")).unwrap();
        assert_eq!(v.0.folder_of(&top), Path::new(""));
    }

    #[test]
    fn a_move_never_overwrites_or_leaves_the_vault() {
        let v = TempVault::new("move-safe");
        let a = v.0.create_in(Path::new("A"), Some("Same"), "in a", &[]).unwrap();
        // The same title in two folders can only come from outside the app.
        fs::create_dir_all(v.0.root().join("B")).unwrap();
        fs::write(v.0.root().join("B").join("Same.md"), "in b").unwrap();
        assert!(v.0.move_note(&a.path, Path::new("B")).is_err());
        assert_eq!(fs::read_to_string(v.0.root().join("B").join("Same.md")).unwrap(), "in b");
        for outside in ["..", "../elsewhere", "A/../..", ".trash", "A/.hidden"] {
            assert!(v.0.move_note(&a.path, Path::new(outside)).is_err(), "{outside}");
            assert!(v.0.create_in(Path::new(outside), None, "", &[]).is_err(), "{outside}");
            assert!(v.0.create_folder(Path::new(outside), "x").is_err(), "{outside}");
        }
        assert!(a.path.exists());
    }

    #[test]
    fn folders_are_made_renamed_and_trashed() {
        let v = TempVault::new("folder-ops");
        let first = v.0.create_folder(Path::new(""), "New folder").unwrap();
        let second = v.0.create_folder(Path::new(""), "New folder").unwrap();
        assert_eq!((first.as_path(), second.as_path()), (Path::new("New folder"), Path::new("New folder 1")));
        let inner = v.0.create_folder(&first, "In: side?").unwrap();
        assert_eq!(inner, Path::new("New folder/In- side-"), "names are made safe for the file system");
        let note = v.0.create_in(&inner, Some("Deep"), "d", &[]).unwrap();

        let renamed = v.0.rename_folder(&first, "Work").unwrap();
        assert_eq!(renamed, Path::new("Work"));
        assert_eq!(v.0.folder_of(&v.0.find("Deep").unwrap().path), Path::new("Work/In- side-"));
        assert!(!note.path.exists());
        assert!(v.0.rename_folder(&renamed, "New folder 1").is_err(), "not onto another folder");
        assert!(v.0.rename_folder(&renamed, " ").is_err() && v.0.rename_folder(&renamed, ".hidden").is_err());
        assert!(v.0.rename_folder(Path::new(""), "x").is_err(), "the top level has no name to change");

        let gone = v.0.trash_folder(&renamed).unwrap();
        assert_eq!(folders(&v.0), ["New folder 1"]);
        assert!(titles(&v.0).is_empty());
        assert_eq!(fs::read_to_string(gone.join("In- side-").join("Deep.md")).unwrap(), "d", "its notes go with it");
        assert!(v.0.trash_folder(Path::new("")).is_err());
    }

    #[test]
    fn renaming_and_trashing_by_path_pick_the_right_one_of_two_namesakes() {
        let v = TempVault::new("namesakes");
        v.0.create_in(Path::new("A"), Some("Same"), "a", &[]).unwrap();
        let b = v.0.root().join("B").join("Same.md");
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        fs::write(&b, "b").unwrap();
        v.0.rename_at(&b, "Other", true).unwrap();
        assert_eq!(v.0.find("Other").unwrap().content, "b");
        assert_eq!(v.0.find("Same").unwrap().content, "a");
        v.0.trash_at(&b.with_file_name("Other.md")).unwrap();
        assert_eq!(titles(&v.0), ["Same"]);
        assert!(v.0.trash_at(Path::new("C:/somewhere/else.md")).is_err());
    }

    #[test]
    fn a_rename_can_leave_the_links_to_the_note_as_they_were() {
        let v = TempVault::new("rename-unlinked");
        let a = v.0.create(Some("A"), "x", &[]).unwrap();
        v.0.create(Some("B"), "see [[A]] and [[A#Part|there]]", &[]).unwrap();
        v.0.create(Some("C"), "[[A]]", &[]).unwrap();
        v.0.create(Some("D"), "[[Another]] `[[A]]`", &[]).unwrap();
        assert_eq!(linking(&v.0.notes().unwrap(), "A"), 3, "as text: a link in code is rewritten too");
        let (_, to, updated) = v.0.rename_at(&a.path, "Z", false).unwrap();
        assert_eq!((to.as_str(), updated), ("Z", 0));
        assert_eq!(v.0.find("B").unwrap().content, "see [[A]] and [[A#Part|there]]");
        assert_eq!(v.0.find("Z").unwrap().content, "x");
    }

    #[test]
    fn a_note_or_folder_can_be_deleted_for_good() {
        let v = TempVault::new("discard");
        let note = v.0.create_in(Path::new("F"), Some("Gone"), "g", &[]).unwrap();
        v.0.create_in(Path::new("F/Inner"), Some("Also"), "a", &[]).unwrap();
        assert_eq!(v.0.discard_at(&note.path, Discard::Delete).unwrap(), None);
        assert!(!note.path.exists() && !v.0.root().join(".trash").exists(), "not moved anywhere");
        assert_eq!(v.0.discard_folder(Path::new("F"), Discard::Delete).unwrap(), None);
        assert!(!v.0.root().join("F").exists());
        assert!(v.0.discard_folder(Path::new(""), Discard::Delete).is_err(), "never the vault itself");
        assert!(v.0.discard_folder(Path::new(".."), Discard::Delete).is_err(), "nor anything outside it");
        assert!(v.0.discard_at(Path::new("C:/somewhere/else.md"), Discard::Delete).is_err());
        let kept = v.0.create(Some("Kept"), "k", &[]).unwrap();
        let went = v.0.discard_at(&kept.path, Discard::Trash).unwrap().unwrap();
        assert!(went.starts_with(v.0.root().join(".trash")));
    }

    #[test]
    fn the_daily_note_goes_in_its_folder_with_a_name_in_its_format() {
        let v = TempVault::new("daily-in");
        let today = chrono::Local::now().naive_local();
        let (note, created) = v.0.daily_in(Path::new("Journal/Days"), "D MMMM YYYY", |title| format!("# {title}!\n")).unwrap();
        assert!(created);
        assert_eq!(note.title, template::formatted(today, "D MMMM YYYY"));
        assert_eq!(v.0.folder_of(&note.path), Path::new("Journal/Days"));
        assert_eq!(fs::read_to_string(&note.path).unwrap(), format!("# {}!\n", note.title));
        assert_eq!(daily_date(&note.title, "D MMMM YYYY"), Some(today.date()));
        // Moved elsewhere, it is still today's note: there is one note of a name.
        let moved = v.0.move_note(&note.path, Path::new("")).unwrap();
        let (again, created) = v.0.daily_in(Path::new("Journal/Days"), "D MMMM YYYY", |_| String::new()).unwrap();
        assert!(!created);
        assert_eq!(again.path, moved);
        assert!(v.0.daily_in(Path::new(""), "YYYY/MM/DD", |_| String::new()).is_err(), "a name cannot have a slash");
        assert!(v.0.daily_in(Path::new("../out"), "YYYY", |_| String::new()).is_err(), "nor the folder lead outside");
    }
}
