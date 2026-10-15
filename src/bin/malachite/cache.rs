//! What is worked out from the notes and kept until they change.
//!
//! The window is drawn again for every mouse move and key press, and reading
//! links out of a note means parsing it. So nothing here is done per frame:
//! what comes from the whole vault (`Index`) is redone when `App::version`
//! says the notes have changed, and what comes from the open note (`Digest`)
//! when its text has. Both are handed out by `Rc`, so that drawing code can
//! hold one while it changes other things on the app. The note the graph has
//! in focus is shown in the side panel too, and has a digest of its own.

use std::collections::HashSet;
use std::rc::Rc;

use malachite::markdown::{self, Link};
use malachite::outline::{self, Heading};
use malachite::{aliases, tags, wikilinks};

use crate::App;

/// What the notes of the vault say about each other.
pub(crate) struct Index {
    version: u64,
    /// Everything a link can call a note, in lower case: titles and aliases.
    pub names: HashSet<String>,
    /// The notes each note links to, as its links name them; in step with `App::notes`.
    pub links: Vec<Vec<String>>,
}

/// What the side panel and the status bar show of a note.
pub(crate) struct Digest {
    /// The text this was read from.
    text: String,
    pub headings: Vec<Heading>,
    /// The notes it links to, each once, in order of name.
    pub links: Vec<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// The embeds that have a line to themselves, which are shown as blocks.
    pub embeds: Vec<Link>,
    pub words: usize,
    pub characters: usize,
}

impl App {
    /// The index of the vault as it is now.
    pub(crate) fn index(&mut self) -> Rc<Index> {
        if let Some(index) = self.index.as_ref().filter(|index| index.version == self.version) {
            return index.clone();
        }
        let names = self.notes.iter().flat_map(|n| std::iter::once(n.title.clone()).chain(aliases(&n.content)));
        let index = Rc::new(Index {
            version: self.version,
            names: names.map(|name| name.to_lowercase()).collect(),
            links: self.notes.iter().map(|n| wikilinks(&n.content)).collect(),
        });
        self.index = Some(index.clone());
        index
    }

    /// The digest of the open note as its text is now, unsaved changes and all.
    pub(crate) fn digest(&mut self) -> Rc<Digest> {
        Digest::kept(&mut self.digest, &self.buffer)
    }

    /// The digest of `notes[note]` as it was saved: of the note the graph has
    /// in focus, when that is not the open one. It is kept apart from the
    /// open note's, which the status bar goes on asking for all the while.
    pub(crate) fn digest_of(&mut self, note: usize) -> Rc<Digest> {
        Digest::kept(&mut self.graph_digest, &self.notes[note].content)
    }
}

impl Digest {
    /// The digest of `text`: the one `kept`, if that is what it was read from.
    fn kept(kept: &mut Option<Rc<Digest>>, text: &str) -> Rc<Digest> {
        // Comparing the text is far cheaper than reading it again.
        if let Some(digest) = kept.as_ref().filter(|digest| digest.text == text) {
            return digest.clone();
        }
        let text = text.to_owned();
        let mut links = wikilinks(&text);
        links.sort();
        links.dedup();
        let digest = Rc::new(Digest {
            headings: outline::headings(&text),
            links,
            tags: tags(&text),
            aliases: aliases(&text),
            embeds: markdown::parse(&text).iter().filter_map(|line| markdown::lone_embed(&text, line).cloned()).collect(),
            words: text.split_whitespace().count(),
            characters: text.chars().count(),
            text,
        });
        *kept = Some(digest.clone());
        digest
    }
}
