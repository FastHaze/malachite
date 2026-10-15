//! What a note shows of other files: the pictures it embeds
//! (`![[picture.png]]`, `![alt](picture.png)`) and the notes, or parts of
//! notes, that it embeds (`![[Note]]`, `![[Note#Heading]]`, `![[Note#^block]]`).
//!
//! Pictures are found the usual way for a vault, by path from the vault's
//! root or else by name anywhere in the vault, and only inside it: nothing is
//! fetched from the web. A picture is decoded once and kept as a texture;
//! when the notes are read from disk again its file is looked at, and it is
//! decoded again only if the file has changed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use eframe::egui::{self, vec2, Vec2};
use malachite::markdown::Place;
use malachite::{outline, Note, Vault};

/// The longest side a picture is kept at, in pixels: larger ones are scaled
/// down as they are read, which is all a note's column can show of them.
const LARGEST: u32 = 2048;

/// What is known of one embedded file.
pub(crate) struct Kept {
    /// The texture and the picture's size in pixels; `None` for a file that
    /// is missing or is not a picture that can be read.
    picture: Option<(egui::TextureHandle, Vec2)>,
    file: Option<(PathBuf, Option<SystemTime>)>,
    /// The `App::version` at which the file was last looked at.
    checked: u64,
    /// The `App::scanned` at which the file was last looked for by its name.
    sought: u64,
}

/// What drawing an embed needs of the app.
pub(crate) struct Embeds<'a> {
    pub pictures: &'a mut HashMap<String, Kept>,
    pub vault: &'a Vault,
    pub notes: &'a [Note],
    pub version: u64,
    /// Counts the times the vault's files have been gone through (`App::scanned`).
    pub scanned: u64,
}

impl Embeds<'_> {
    /// The picture a note embeds as `target`, and its size in pixels.
    pub fn picture(&mut self, ctx: &egui::Context, target: &str) -> Option<(egui::TextureId, Vec2)> {
        let shown = |kept: &Kept| kept.picture.as_ref().map(|(texture, size)| (texture.id(), *size));
        if let Some(kept) = self.pictures.get(target).filter(|kept| kept.checked == self.version) {
            return shown(kept);
        }
        // Where it was found before, if it is still there. Finding a file by
        // its name is a walk through the whole vault, and this is asked for
        // every picture of a note each time the notes change, which is every
        // save. So a picture is only looked for when it has gone from where
        // it was; and one that was not there is not looked for again until
        // the vault's files have been gone through again, since a save
        // brings no file into being.
        let kept = self.pictures.get(target);
        let before = kept.and_then(|kept| kept.file.as_ref()).map(|(path, _)| path.clone());
        let absent = kept.is_some_and(|kept| kept.file.is_none() && kept.sought == self.scanned);
        let sought = !absent && malachite::is_picture(target);
        let path = before.filter(|path| path.is_file()).or_else(|| sought.then(|| self.vault.attachment(target)).flatten());
        let file = path.map(|path| {
            let modified = std::fs::metadata(&path).and_then(|data| data.modified()).ok();
            (path, modified)
        });
        // The notes have changed since, but not this file: keep what was read.
        if let Some(kept) = self.pictures.get_mut(target).filter(|kept| kept.file == file) {
            (kept.checked, kept.sought) = (self.version, self.scanned);
            return shown(kept);
        }
        let picture = file.as_ref().and_then(|(path, _)| read(ctx, target, path));
        let kept = Kept { picture, file, checked: self.version, sought: self.scanned };
        let result = shown(&kept);
        self.pictures.insert(target.to_owned(), kept);
        result
    }

    /// The note, or the part of one, that is embedded as `target` and
    /// `place`: what to call it and its text. A whole note comes without its
    /// properties.
    pub fn note(&self, target: &str, place: Option<&Place>) -> Option<(String, String)> {
        let note = &self.notes[malachite::resolve(self.notes, target)?];
        match place {
            None => Some((note.title.clone(), body(&note.content).to_owned())),
            Some(place) => {
                let part = outline::extent(&note.content, place)?;
                let title = match place {
                    Place::Heading(name) => format!("{} › {}", note.title, name.replace('#', " › ")),
                    Place::Block(_) => note.title.clone(),
                };
                Some((title, note.content[part].to_owned()))
            }
        }
    }
}

/// Read and decode the picture in `path` into a texture.
fn read(ctx: &egui::Context, name: &str, path: &std::path::Path) -> Option<(egui::TextureHandle, Vec2)> {
    let mut picture = image::open(path).ok()?;
    if picture.width().max(picture.height()) > LARGEST {
        picture = picture.thumbnail(LARGEST, LARGEST);
    }
    let pixels = picture.to_rgba8();
    let size = [pixels.width() as usize, pixels.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
    Some((texture, vec2(size[0] as f32, size[1] as f32)))
}

/// A note's text without the frontmatter at its top.
fn body(text: &str) -> &str {
    let opened = text.strip_prefix("---").filter(|rest| rest.starts_with('\n') || rest.starts_with("\r\n"));
    let Some(rest) = opened else { return text };
    let mut at = text.len() - rest.len();
    // The first of these is what is left of the opening line: its line break.
    for line in rest.split_inclusive('\n') {
        at += line.len();
        if line.trim_end() == "---" {
            return &text[at..];
        }
    }
    text
}

/// The size to draw a picture of `size` at: the `width` its embed asks for,
/// or else its own, and never wider than the `room` there is.
pub(crate) fn fit(size: Vec2, width: Option<u32>, room: f32) -> Vec2 {
    if size.x <= 0.0 {
        return size;
    }
    let wide = width.map_or(size.x, |width| width as f32).min(room.max(1.0));
    vec2(wide, (size.y * wide / size.x).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_never_wider_than_the_room_for_it() {
        assert_eq!(fit(vec2(400.0, 200.0), None, 600.0), vec2(400.0, 200.0));
        assert_eq!(fit(vec2(400.0, 200.0), None, 100.0), vec2(100.0, 50.0));
        assert_eq!(fit(vec2(400.0, 200.0), Some(200), 600.0), vec2(200.0, 100.0), "at the width that was asked for");
        assert_eq!(fit(vec2(400.0, 200.0), Some(900), 600.0), vec2(600.0, 300.0));
    }

    #[test]
    fn the_body_of_a_note_starts_after_its_properties() {
        assert_eq!(body("---\ntags: [a]\n---\n# T\ntext"), "# T\ntext");
        assert_eq!(body("---\r\na: b\r\n---\r\nx"), "x");
        assert_eq!(body("# T\n---\n"), "# T\n---\n");
        assert_eq!(body("---\nnever closed"), "---\nnever closed");
    }
}
