//! The structure of a note: its headings, the places in it that a link can
//! point at, and what can be folded away.
//!
//! The outline pane lists a note's headings; `[[Note#Heading]]` and
//! `[[Note#^block]]` lead to a heading or to the block marked `^block`; and a
//! heading folds what is under it, a list item the items nested in it. All of
//! that is read from the same line model the note is drawn from, so a `#` in
//! a code block is not a heading here either.

use std::ops::Range;

use crate::markdown::{self, Kind, Line, Place};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    /// The heading as it reads, without its Markdown syntax.
    pub title: String,
    /// The heading's line in the note.
    pub line: Range<usize>,
}

/// The headings of a note, in order.
pub fn headings(text: &str) -> Vec<Heading> {
    headings_in(text, &markdown::parse(text))
}

fn headings_in(text: &str, lines: &[Line]) -> Vec<Heading> {
    let heading = |line: &Line| match line.kind {
        Kind::Heading(level) => Some(Heading { level, title: shown(text, line), line: line.range.clone() }),
        _ => None,
    };
    lines.iter().filter_map(heading).collect()
}

/// What a line says once its syntax is taken away.
fn shown(text: &str, line: &Line) -> String {
    let content = line.spans.iter().filter(|span| !span.marker && !span.style.comment);
    content.map(|span| &text[span.range.clone()]).collect::<String>().trim().to_string()
}

/// The line of the note that `place` names, if there is one. Headings are
/// found by what they say, capitals aside; `One#Two` is the heading Two
/// somewhere under the heading One.
pub fn find(text: &str, place: &Place) -> Option<Range<usize>> {
    let lines = markdown::parse(text);
    match place {
        Place::Heading(path) => {
            let headings = headings_in(text, &lines);
            // The headings still to look through, and the level they are under.
            let (mut from, mut under) = (0, 0);
            let mut found = None;
            for name in path.split('#').map(str::trim) {
                let mut inside = headings[from..].iter().take_while(|heading| heading.level > under);
                let at = from + inside.position(|heading| heading.title.to_lowercase() == name.to_lowercase())?;
                (from, under) = (at + 1, headings[at].level);
                found = Some(headings[at].line.clone());
            }
            found
        }
        Place::Block(id) => {
            let marked = lines.iter().position(|line| {
                let line = text[line.range.clone()].trim_end();
                line.strip_suffix(id.as_str()).and_then(|rest| rest.strip_suffix('^')).is_some_and(|rest| rest.is_empty() || rest.ends_with(' '))
            })?;
            // An id on a line of its own marks the block above it.
            let alone = text[lines[marked].range.clone()].trim().len() == id.len() + 1;
            let above = lines[..marked].iter().rposition(|line| line.kind != Kind::Blank).filter(|_| alone);
            Some(lines[above.unwrap_or(marked)].range.clone())
        }
    }
}

/// The part of a note that `place` names, as an embed shows it: a heading
/// with everything under it, or the one block.
pub fn extent(text: &str, place: &Place) -> Option<Range<usize>> {
    let line = find(text, place)?;
    let under = folds(text).into_iter().find(|fold| fold.line == line && matches!(place, Place::Heading(_)));
    Some(line.start..under.map_or(line.end, |fold| fold.hidden.end))
}

/// Something that folds: a heading with text under it, or a list item with
/// items nested in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    /// The line that stays in view.
    pub line: Range<usize>,
    /// What folding it hides: from the end of that line to the end of the
    /// last line under it, so the line breaks between them go too.
    pub hidden: Range<usize>,
}

/// Everything in a note that can be folded, in the order of its lines.
pub fn folds(text: &str) -> Vec<Fold> {
    folds_in(text, &markdown::parse(text))
}

/// As [`folds`], for a note that has been parsed into `lines` already.
pub fn folds_in(text: &str, lines: &[Line]) -> Vec<Fold> {
    // How far a line is indented: a tab counts as four spaces.
    let indent = |line: &Line| -> usize {
        let raw = &text[line.range.clone()];
        raw.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
    };
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let rest = &lines[index + 1..];
        // How many of the lines after this one belong to it.
        let under = match line.kind {
            Kind::Heading(level) => {
                let next = rest.iter().position(|l| matches!(l.kind, Kind::Heading(other) if other <= level));
                next.unwrap_or(rest.len())
            }
            Kind::Bullet | Kind::Numbered | Kind::Task { .. } => {
                let own = indent(line);
                let next = rest.iter().position(|l| l.kind != Kind::Blank && indent(l) <= own);
                next.unwrap_or(rest.len())
            }
            _ => continue,
        };
        // Blank lines at the end stay, to keep what follows apart.
        let Some(last) = rest[..under].iter().rfind(|l| l.kind != Kind::Blank) else { continue };
        out.push(Fold { line: line.range.clone(), hidden: line.range.end..last.range.end });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_are_listed_as_they_read() {
        let text = "# One **bold**\ntext\n## [[Note|Two]] %%hidden%%\n```\n# not a heading\n```\n#tag\n###### Six";
        let found: Vec<_> = headings(text).into_iter().map(|h| (h.level, h.title)).collect();
        assert_eq!(found, [(1, "One bold".to_string()), (2, "Two".to_string()), (6, "Six".to_string())]);
        assert_eq!(&text[headings(text)[1].line.clone()], "## [[Note|Two]] %%hidden%%");
    }

    fn line(text: &str, place: Place) -> Option<&str> {
        find(text, &place).map(|range| &text[range])
    }

    #[test]
    fn a_heading_is_found_by_name_whatever_its_capitals() {
        let text = "# Plan\n## Steps\n# Notes\n## Steps\n### Détails";
        let heading = |name: &str| line(text, Place::Heading(name.into()));
        assert_eq!(heading("plan"), Some("# Plan"));
        assert_eq!(find(text, &Place::Heading("Steps".into())), Some(7..15), "the first of two namesakes");
        assert_eq!(find(text, &Place::Heading("Notes#Steps".into())), Some(24..32), "the one under Notes");
        assert_eq!(heading("Notes#Steps#détails"), Some("### Détails"));
        assert_eq!(heading("Plan#Détails"), None, "it is not under Plan");
        assert_eq!(heading("Missing"), None);
    }

    #[test]
    fn a_block_is_found_by_the_id_at_its_end() {
        let text = "one ^a1\n- item ^b-2\n\nlong paragraph\n^c3\n\nnot^d4";
        let block = |id: &str| line(text, Place::Block(id.into()));
        assert_eq!(block("a1"), Some("one ^a1"));
        assert_eq!(block("b-2"), Some("- item ^b-2"));
        assert_eq!(block("c3"), Some("long paragraph"), "an id on its own line marks the block above");
        assert_eq!(block("d4"), None, "an id needs a space in front");
        assert_eq!(block("a"), None);
    }

    #[test]
    fn the_extent_of_a_heading_is_all_that_is_under_it() {
        let text = "# A\none\n## B\ntwo ^b\n# C";
        let part = |place| extent(text, &place).map(|range| &text[range]);
        assert_eq!(part(Place::Heading("A".into())), Some("# A\none\n## B\ntwo ^b"));
        assert_eq!(part(Place::Heading("C".into())), Some("# C"));
        assert_eq!(part(Place::Block("b".into())), Some("two ^b"));
        assert_eq!(part(Place::Heading("D".into())), None);
    }

    /// The folds of `text`, as (the line, what it hides).
    fn folded(text: &str) -> Vec<(&str, &str)> {
        folds(text).into_iter().map(|fold| (&text[fold.line], &text[fold.hidden])).collect()
    }

    #[test]
    fn a_heading_folds_up_to_the_next_one_of_its_level() {
        let text = "# A\none\n## B\ntwo\n\n## C\n# D\n\n";
        assert_eq!(folded(text), [("# A", "\none\n## B\ntwo\n\n## C"), ("## B", "\ntwo")]);
    }

    #[test]
    fn a_list_item_folds_the_items_nested_in_it() {
        let text = "- a\n  - b\n\t- c\n\n    deeper\n- d\n1. e\n   text\nafter";
        assert_eq!(folded(text), [("- a", "\n  - b\n\t- c\n\n    deeper"), ("  - b", "\n\t- c\n\n    deeper"), ("1. e", "\n   text")]);
    }

    #[test]
    fn nothing_folds_without_something_under_it() {
        assert_eq!(folded("# A\n\n# B\n- c\n- d\n"), [("# B", "\n- c\n- d")]);
        assert_eq!(folded("## A\n\n\n## B\n\n\n"), []);
    }
}
