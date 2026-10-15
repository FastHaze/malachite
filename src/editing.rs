//! The small things an editor does for you while you type Markdown: wrapping
//! the selection in `**`, carrying a list on to the next line, indenting an
//! item, closing a bracket, and the commands that write a piece of syntax for
//! you. Each function takes the note's text and the selection (a byte range,
//! empty for a plain caret) and returns the edited text with the selection to
//! show afterwards, or `None` when there is nothing for it to do and the key
//! should just do what it always does.

use std::ops::Range;

use crate::markdown::{self, Kind, Line};

#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    /// Where the selection is in the new text.
    pub selection: Range<usize>,
}

/// Put `marker` (`**`, `*`) around the selection, or take it away if it is
/// already there. With nothing selected, the pair is inserted around the caret.
pub fn toggle_wrap(text: &str, selection: Range<usize>, marker: &str) -> Edit {
    let (start, end, len) = (selection.start, selection.end, marker.len());
    let (before, inside, after) = (&text[..start], &text[start..end], &text[end..]);

    if wrapped(before, after, marker) {
        // The markers sit just outside the selection, as they do when the
        // word was selected in Live Preview, where they are hidden.
        let text = format!("{}{inside}{}", &before[..start - len], &after[len..]);
        return Edit { text, selection: start - len..end - len };
    }
    if inside.len() >= 2 * len && wrapped(inside, inside, marker) {
        // The selection includes its markers.
        let text = format!("{before}{}{after}", &inside[len..inside.len() - len]);
        return Edit { text, selection: start..end - 2 * len };
    }
    Edit { text: format!("{before}{marker}{inside}{marker}{after}"), selection: start + len..end + len }
}

/// Whether `marker` is at the end of `before` and again at the start of
/// `after`, wrapping whatever lies between them.
///
/// `*` needs care, because the stars of `**bold**` are not italics: a run of
/// one or three stars has an italic in it, a run of two does not.
fn wrapped(before: &str, after: &str, marker: &str) -> bool {
    if marker != "*" {
        return before.ends_with(marker) && after.starts_with(marker);
    }
    let stars_before = before.bytes().rev().take_while(|&b| b == b'*').count();
    let stars_after = after.bytes().take_while(|&b| b == b'*').count();
    stars_before.min(stars_after) % 2 == 1
}

/// Enter at the end of a list item, task or quote starts the next one. On an
/// item that is still empty it ends the list instead, by clearing the line.
pub fn continue_list(text: &str, caret: usize) -> Option<Edit> {
    let line = line_at(text, caret)?;
    let lead = match &line.kind {
        Kind::Bullet => text[line.marker.clone()].to_owned(),
        Kind::Task { checkbox, .. } => format!("{}[ ] ", &text[line.marker.start..checkbox.start]),
        Kind::Numbered => {
            let marker = &text[line.marker.clone()];
            let digits = marker.bytes().take_while(u8::is_ascii_digit).count();
            let number: u64 = marker[..digits].parse().ok()?;
            format!("{}{}", number + 1, &marker[digits..])
        }
        // A callout's `[!type]` belongs to its first line only.
        Kind::Quote { tag, .. } => text[line.marker.start..tag.as_ref().map_or(line.marker.end, |t| t.start)].to_owned(),
        _ => return None,
    };
    if caret < line.marker.end {
        return None;
    }
    if text[line.marker.end..line.range.end].trim().is_empty() {
        let text = format!("{}{}", &text[..line.range.start], &text[line.range.end..]);
        return Some(Edit { text, selection: line.range.start..line.range.start });
    }
    let indent = &text[line.range.start..line.marker.start];
    let inserted = format!("\n{indent}{lead}");
    let after = caret + inserted.len();
    Some(Edit { text: format!("{}{inserted}{}", &text[..caret], &text[caret..]), selection: after..after })
}

/// Tab on a list item nests it one level deeper; with `outdent` (Shift+Tab)
/// it comes back out. The caret stays on the same character.
pub fn indent_item(text: &str, caret: usize, outdent: bool) -> Option<Edit> {
    let line = line_at(text, caret)?;
    if !matches!(line.kind, Kind::Bullet | Kind::Numbered | Kind::Task { .. }) {
        return None;
    }
    let start = line.range.start;
    if !outdent {
        let text = format!("{}\t{}", &text[..start], &text[start..]);
        return Some(Edit { text, selection: caret + 1..caret + 1 });
    }
    // One level is a tab, or up to four spaces.
    let indent = &text[start..line.marker.start];
    let removed = if indent.starts_with('\t') { 1 } else { indent.bytes().take(4).take_while(|&b| b == b' ').count() };
    if removed == 0 {
        return None;
    }
    let caret = caret.saturating_sub(removed).max(start);
    Some(Edit { text: format!("{}{}", &text[..start], &text[start + removed..]), selection: caret..caret })
}

/// Ctrl+L, "toggle checkbox status": tick or untick a task, and
/// make a task of a list item or a plain line.
pub fn toggle_task(text: &str, caret: usize) -> Option<Edit> {
    let line = line_at(text, caret)?;
    match &line.kind {
        Kind::Task { done, checkbox } => {
            let mark = if *done { "[ ]" } else { "[x]" };
            let text = format!("{}{mark}{}", &text[..checkbox.start], &text[checkbox.end..]);
            Some(Edit { text, selection: caret..caret })
        }
        Kind::Bullet | Kind::Numbered => Some(insert_at(text, line.marker.end, "[ ] ", caret)),
        Kind::Paragraph | Kind::Blank => Some(insert_at(text, line.marker.start, "- [ ] ", caret)),
        _ => None,
    }
}

/// Make the caret's line a heading of this level, or with level 0 plain text
/// again. Only headings and plain lines change; a list item stays a list item.
pub fn set_heading(text: &str, caret: usize, level: u8) -> Option<Edit> {
    let line = line_at(text, caret)?;
    let current = match line.kind {
        Kind::Heading(level) => level,
        Kind::Paragraph | Kind::Blank => 0,
        _ => return None,
    };
    if current == level {
        return None;
    }
    let old = line.marker;
    let marker = if level == 0 { String::new() } else { format!("{} ", "#".repeat(usize::from(level))) };
    // The caret stays on its character; inside the old marker it goes to the end of the new one.
    let caret = if caret >= old.end {
        caret - old.len() + marker.len()
    } else if caret > old.start {
        old.start + marker.len()
    } else {
        caret
    };
    Some(Edit { text: format!("{}{marker}{}", &text[..old.start], &text[old.end..]), selection: caret..caret })
}

/// Ctrl+D: delete the lines the selection touches. The caret lands on the
/// line that moves up into their place.
/// What Ctrl+Backspace deletes at the caret, or with `forward` Ctrl+Delete:
/// the word before it (after it), with any spaces between, and never more
/// than the rest of its line. At the very start of a line (or its end) it is
/// the line break, as Backspace (Delete) takes. A word is a run of letters and
/// digits in any script, or else a run of other marks, such as `**`; a word
/// in Cyrillic is as much a word as one in Latin letters.
pub fn delete_word(text: &str, caret: usize, forward: bool) -> Option<Edit> {
    let is_space = |c: char| c == ' ' || c == '\t';
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let line_break = |c: char| c == '\n' || c == '\r';
    // The characters walked over, from the caret outwards.
    let mut chars: Box<dyn Iterator<Item = char>> = if forward { Box::new(text[caret..].chars()) } else { Box::new(text[..caret].chars().rev()) };
    let mut taken = 0;
    let mut first = chars.next()?;
    if line_break(first) {
        // A line break is one, even when it is written as two.
        let second = chars.next();
        taken = first.len_utf8() + second.filter(|&c| line_break(c) && c != first).map_or(0, char::len_utf8);
    } else {
        while is_space(first) {
            taken += first.len_utf8();
            match chars.next() {
                Some(next) if !line_break(next) => first = next,
                _ => break,
            }
        }
        if !is_space(first) {
            let word = is_word(first);
            taken += first.len_utf8();
            for next in chars {
                if line_break(next) || is_space(next) || is_word(next) != word {
                    break;
                }
                taken += next.len_utf8();
            }
        }
    }
    let gone = if forward { caret..caret + taken } else { caret - taken..caret };
    Some(Edit { text: format!("{}{}", &text[..gone.start], &text[gone.end..]), selection: gone.start..gone.start })
}

pub fn delete_paragraph(text: &str, selection: Range<usize>) -> Edit {
    let lines = lines_of(text, &selection);
    // Take the line break after them, or for the last line the one before it.
    let (start, end) = match lines.end < text.len() {
        true => (lines.start, lines.end + 1),
        false => (lines.start.saturating_sub(1), lines.end),
    };
    Edit { text: format!("{}{}", &text[..start], &text[end..]), selection: start..start }
}

/// Move the lines the selection touches above the line before them, or with
/// `up` false below the line after them. The selection moves with them.
pub fn swap_line(text: &str, selection: Range<usize>, up: bool) -> Option<Edit> {
    let block = lines_of(text, &selection);
    if up {
        let before = block.start.checked_sub(1)?;
        let prev = lines_of(text, &(before..before));
        let shift = prev.len() + 1;
        let text = format!("{}{}\n{}{}", &text[..prev.start], &text[block.clone()], &text[prev], &text[block.end..]);
        Some(Edit { text, selection: selection.start - shift..selection.end - shift })
    } else {
        if block.end == text.len() {
            return None;
        }
        let next = lines_of(text, &(block.end + 1..block.end + 1));
        let shift = next.len() + 1;
        let text = format!("{}{}\n{}{}", &text[..block.start], &text[next.clone()], &text[block], &text[next.end..]);
        Some(Edit { text, selection: selection.start + shift..selection.end + shift })
    }
}

/// Ctrl+K: make a Markdown link of the selection, with the caret where its
/// address goes. With nothing selected the caret waits for the link's text.
pub fn insert_link(text: &str, selection: Range<usize>) -> Edit {
    let inside = &text[selection.clone()];
    let caret = if inside.is_empty() { selection.start + 1 } else { selection.end + 3 };
    let text = format!("{}[{inside}](){}", &text[..selection.start], &text[selection.end..]);
    Edit { text, selection: caret..caret }
}

/// A `> [!note]` callout, ready for its first line.
pub fn insert_callout(text: &str, caret: usize) -> Edit {
    let block = "> [!note]\n> ";
    insert_block(text, caret, block, block.len())
}

/// An empty code block with the caret inside it.
pub fn insert_code_block(text: &str, caret: usize) -> Edit {
    insert_block(text, caret, "```\n\n```", 4)
}

/// A table of two columns with the caret in its first heading.
pub fn insert_table(text: &str, caret: usize) -> Edit {
    insert_block(text, caret, "|  |  |\n| --- | --- |\n|  |  |", 2)
}

/// A horizontal rule, with the caret on a new line below it.
pub fn insert_rule(text: &str, caret: usize) -> Edit {
    insert_block(text, caret, "---\n", 4)
}

/// What a stretch of text is to be highlighted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlighter {
    /// Nothing: take the highlight away.
    Off,
    /// The plain yellow, written `==text==`.
    Yellow,
    /// A colour of one's own, as red, green and blue, written as a `<mark>`
    /// tag with that background, which other Markdown apps show in colour too.
    Color([u8; 3]),
}

/// How strong a coloured highlight is, out of 255: 40%.
pub const HIGHLIGHT_OPACITY: u8 = 0x66;

/// Highlight the selection in a colour, or change the colour of the
/// highlight the selection is in, or with [`Highlighter::Off`] remove it.
/// With nothing selected and no highlight around the caret, the markers go
/// around the caret. A highlight is something on one line: `None` for a
/// selection that runs over several, and when there is nothing to take off.
pub fn set_highlight(text: &str, selection: Range<usize>, to: Highlighter) -> Option<Edit> {
    let line = line_at(text, selection.start).filter(|line| selection.end <= line.range.end)?;
    let (open, close) = match to {
        Highlighter::Off => (String::new(), ""),
        Highlighter::Yellow => ("==".to_owned(), "=="),
        Highlighter::Color([r, g, b]) => (format!("<mark style=\"background: #{r:02X}{g:02X}{b:02X}{HIGHLIGHT_OPACITY:02X};\">"), "</mark>"),
    };
    // The highlight the selection is in, if it is in one: the innermost,
    // by the marker that opens it.
    let opens = |span: &&markdown::Span| {
        let written = &text[span.range.clone()];
        let inside = span.element.start <= selection.start && selection.end <= span.element.end;
        span.marker && span.range.start == span.element.start && inside && (written == "==" || written.starts_with("<mark"))
    };
    let Some(opening) = line.spans.iter().filter(opens).last() else {
        if to == Highlighter::Off {
            return None;
        }
        let start = selection.start + open.len();
        let text = format!("{}{open}{}{close}{}", &text[..selection.start], &text[selection.clone()], &text[selection.end..]);
        return Some(Edit { text, selection: start..start + selection.len() });
    };
    let closing = line.spans.iter().rfind(|span| span.marker && span.element == opening.element)?;
    let inner = opening.range.end..closing.range.start;
    // The selection stays on the text it was on, which the markers have moved.
    let moved = |at: usize| at.clamp(inner.start, inner.end) + open.len() - opening.range.len();
    let selection = moved(selection.start)..moved(selection.end);
    let text = format!("{}{open}{}{close}{}", &text[..opening.range.start], &text[inner], &text[closing.range.end..]);
    Some(Edit { text, selection })
}

/// Put `inserted` in place of the selection, with the caret after it: what
/// inserting a template does.
pub fn insert_text(text: &str, selection: Range<usize>, inserted: &str) -> Edit {
    let caret = selection.start + inserted.len();
    Edit { text: format!("{}{inserted}{}", &text[..selection.start], &text[selection.end..]), selection: caret..caret }
}

/// Opening characters that bring their closing one with them.
const PAIRS: [(char, char); 5] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];

/// Markdown markers that wrap a selection when typed over it. Typed on their
/// own they stay single: a `*` may be a bullet, a `_` part of a name.
const WRAPS: [char; 4] = ['*', '_', '=', '~'];

/// What typing `typed` does beyond inserting it ("auto-pair
/// brackets" and "auto-pair Markdown syntax"): over a selection, a bracket,
/// quote or Markdown marker wraps it; an opening bracket or quote brings its
/// partner; and a closing one typed in front of itself steps over it.
pub fn pair(text: &str, selection: Range<usize>, typed: char) -> Option<Edit> {
    let (start, end, len) = (selection.start, selection.end, typed.len_utf8());
    let (before, inside, after) = (&text[..start], &text[start..end], &text[end..]);
    let closer = PAIRS.iter().find(|pair| pair.0 == typed).map(|pair| pair.1);

    if !inside.is_empty() {
        let closer = closer.or(WRAPS.contains(&typed).then_some(typed))?;
        return Some(Edit { text: format!("{before}{typed}{inside}{closer}{after}"), selection: start + len..end + len });
    }
    if PAIRS.iter().any(|pair| pair.1 == typed) && after.starts_with(typed) {
        return Some(Edit { text: text.to_owned(), selection: start + len..start + len });
    }
    let closer = closer?;
    // A partner is only wanted where there is room for it, not in front of a word.
    let room = after.chars().next().map_or(true, |next| next.is_whitespace() || ")]}>.,;:!?\"'".contains(next));
    // A quote after a letter closes something or is an apostrophe, and a
    // third backtick is a code fence on its way.
    let closing = closer == typed && before.chars().next_back().is_some_and(|last| last.is_alphanumeric() || last == typed);
    if !room || closing {
        return None;
    }
    Some(Edit { text: format!("{before}{typed}{closer}{after}"), selection: start + len..start + len })
}

/// Put `inserted` in at `at`, keeping the caret on the character it was on.
fn insert_at(text: &str, at: usize, inserted: &str, caret: usize) -> Edit {
    let caret = if caret >= at { caret + inserted.len() } else { caret };
    Edit { text: format!("{}{inserted}{}", &text[..at], &text[at..]), selection: caret..caret }
}

/// Put `block` on lines of its own at the caret: in place of an empty line,
/// or else below the line the caret is on. `caret` is where in the block the
/// caret goes.
fn insert_block(text: &str, at: usize, block: &str, caret: usize) -> Edit {
    let line = lines_of(text, &(at..at));
    let lead = if text[line.clone()].trim().is_empty() { "" } else { "\n" };
    let caret = line.end + lead.len() + caret;
    Edit { text: format!("{}{lead}{block}{}", &text[..line.end], &text[line.end..]), selection: caret..caret }
}

/// The whole lines the selection touches, without the last one's line break.
fn lines_of(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start].rfind('\n').map_or(0, |at| at + 1);
    let end = text[selection.end..].find('\n').map_or(text.len(), |at| selection.end + at);
    start..end
}

/// The line the caret is on.
fn line_at(text: &str, caret: usize) -> Option<Line> {
    markdown::parse(text).into_iter().find(|line| line.range.start <= caret && caret <= line.range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `edit` to text written with `|` at each end of the selection (or
    /// one `|` for a caret), and write the result the same way.
    fn run(marked: &str, edit: impl Fn(&str, Range<usize>) -> Option<Edit>) -> Option<String> {
        let start = marked.find('|').expect("mark the caret with |");
        let text = marked.replacen('|', "", 2);
        let end = marked[start + 1..].find('|').map_or(start, |p| start + p);
        let Edit { mut text, selection } = edit(&text, start..end)?;
        if selection.start != selection.end {
            text.insert(selection.end, '|');
        }
        text.insert(selection.start, '|');
        Some(text)
    }

    fn wrap(marked: &str, marker: &str) -> String {
        run(marked, |text, selection| Some(toggle_wrap(text, selection, marker))).unwrap()
    }

    fn enter(marked: &str) -> Option<String> {
        run(marked, |text, selection| continue_list(text, selection.start))
    }

    fn tab(marked: &str, outdent: bool) -> Option<String> {
        run(marked, |text, selection| indent_item(text, selection.start, outdent))
    }

    #[test]
    fn bold_wraps_and_unwraps_the_selection() {
        assert_eq!(wrap("a |word| b", "**"), "a **|word|** b");
        assert_eq!(wrap("a **|word|** b", "**"), "a |word| b");
        assert_eq!(wrap("a |**word**| b", "**"), "a |word| b");
        assert_eq!(wrap("é |ü| é", "**"), "é **|ü|** é");
    }

    #[test]
    fn with_nothing_selected_the_markers_go_around_the_caret() {
        assert_eq!(wrap("a | b", "**"), "a **|** b");
        assert_eq!(wrap("a **|** b", "**"), "a | b");
        assert_eq!(wrap("|", "*"), "*|*");
    }

    #[test]
    fn italic_and_bold_combine() {
        assert_eq!(wrap("|word|", "*"), "*|word|*");
        assert_eq!(wrap("*|word|*", "*"), "|word|");
        assert_eq!(wrap("**|word|**", "*"), "***|word|***", "bold stars are not italics");
        assert_eq!(wrap("***|word|***", "*"), "**|word|**");
        assert_eq!(wrap("*|word|*", "**"), "***|word|***");
        assert_eq!(wrap("***|word|***", "**"), "*|word|*");
        // The same when the selection takes in the markers.
        assert_eq!(wrap("|*word*|", "*"), "|word|");
        assert_eq!(wrap("|**word**|", "*"), "*|**word**|*");
        assert_eq!(wrap("|ü|", "*"), "*|ü|*");
        assert_eq!(wrap("|é*|", "*"), "*|é*|*");
    }

    #[test]
    fn the_other_markers_wrap_like_bold_does() {
        assert_eq!(wrap("a |word| b", "~~"), "a ~~|word|~~ b");
        assert_eq!(wrap("a ==|word|== b", "=="), "a |word| b");
        assert_eq!(wrap("a |word| b", "`"), "a `|word|` b");
        assert_eq!(wrap("|a note to self|", "%%"), "%%|a note to self|%%");
    }

    fn task(marked: &str) -> Option<String> {
        run(marked, |text, selection| toggle_task(text, selection.start))
    }

    #[test]
    fn ctrl_l_ticks_and_unticks_a_task() {
        assert_eq!(task("- [ ] buy| milk").unwrap(), "- [x] buy| milk");
        assert_eq!(task("- [x] buy| milk").unwrap(), "- [ ] buy| milk");
        assert_eq!(task("one\n\t- [ ] two|").unwrap(), "one\n\t- [x] two|");
    }

    #[test]
    fn ctrl_l_makes_a_task_of_a_list_item_or_a_plain_line() {
        assert_eq!(task("- buy| milk").unwrap(), "- [ ] buy| milk");
        assert_eq!(task("1. buy| milk").unwrap(), "1. [ ] buy| milk");
        assert_eq!(task("buy| milk").unwrap(), "- [ ] buy| milk");
        assert_eq!(task("  indented|").unwrap(), "  - [ ] indented|");
        assert_eq!(task("|").unwrap(), "- [ ] |");
        assert_eq!(task("# Heading|"), None);
        assert_eq!(task("```\ncode|\n```"), None);
    }

    fn heading(marked: &str, level: u8) -> Option<String> {
        run(marked, |text, selection| set_heading(text, selection.start, level))
    }

    #[test]
    fn a_line_becomes_a_heading_changes_level_and_goes_back() {
        assert_eq!(heading("Ti|tle", 2).unwrap(), "## Ti|tle");
        assert_eq!(heading("## Ti|tle", 1).unwrap(), "# Ti|tle");
        assert_eq!(heading("# Ti|tle", 6).unwrap(), "###### Ti|tle");
        assert_eq!(heading("### Ti|tle", 0).unwrap(), "Ti|tle");
        assert_eq!(heading("before\n|Title\nafter", 3).unwrap(), "before\n### |Title\nafter");
        assert_eq!(heading("#|## Title", 1).unwrap(), "# |Title", "a caret in the marker ends up after it");
        assert_eq!(heading("|", 1).unwrap(), "# |");
    }

    #[test]
    fn a_heading_is_only_set_where_it_changes_something() {
        assert_eq!(heading("## Title|", 2), None);
        assert_eq!(heading("plain|", 0), None);
        assert_eq!(heading("- item|", 1), None, "a list item stays a list item");
    }

    fn delete(marked: &str) -> String {
        run(marked, |text, selection| Some(delete_paragraph(text, selection))).unwrap()
    }

    #[test]
    fn ctrl_d_deletes_the_line_the_caret_is_on() {
        assert_eq!(delete("one\ntw|o\nthree"), "one\n|three");
        assert_eq!(delete("on|e\ntwo"), "|two");
        assert_eq!(delete("one\ntw|o"), "one|", "after the last line there is nothing to move up");
        assert_eq!(delete("on|ly"), "|");
        assert_eq!(delete("one\n|\nthree"), "one\n|three");
        assert_eq!(delete("a\nb|b\ncc|\nd"), "a\n|d", "a selection takes every line it touches");
    }

    fn swap(marked: &str, up: bool) -> Option<String> {
        run(marked, |text, selection| swap_line(text, selection, up))
    }

    #[test]
    fn lines_move_up_and_down_with_the_caret_on_them() {
        assert_eq!(swap("one\ntw|o\nthree", true).unwrap(), "tw|o\none\nthree");
        assert_eq!(swap("one\ntw|o\nthree", false).unwrap(), "one\nthree\ntw|o");
        assert_eq!(swap("a\n|b\nc|\nd", true).unwrap(), "|b\nc|\na\nd");
        assert_eq!(swap("a\n|b\nc|\nd", false).unwrap(), "a\nd\n|b\nc|");
        assert_eq!(swap("é\nü|ü", true).unwrap(), "ü|ü\né");
        assert_eq!(swap("on|e\ntwo", true), None, "the first line has nowhere to go");
        assert_eq!(swap("one\ntw|o", false), None);
    }

    #[test]
    fn ctrl_k_makes_a_link_of_the_selection() {
        let link = |marked: &str| run(marked, |text, selection| Some(insert_link(text, selection))).unwrap();
        assert_eq!(link("see |the docs| here"), "see [the docs](|) here");
        assert_eq!(link("see | here"), "see [|]() here");
        assert_eq!(link("|é|"), "[é](|)");
    }

    fn highlight(marked: &str, to: Highlighter) -> Option<String> {
        run(marked, |text, selection| set_highlight(text, selection, to))
    }

    #[test]
    fn a_selection_is_highlighted_in_yellow_or_in_a_colour() {
        assert_eq!(highlight("a |word| b", Highlighter::Yellow).unwrap(), "a ==|word|== b");
        let red = Highlighter::Color([255, 0, 16]);
        assert_eq!(highlight("a |wórd| b", red).unwrap(), "a <mark style=\"background: #FF001066;\">|wórd|</mark> b");
        assert_eq!(highlight("a | b", Highlighter::Yellow).unwrap(), "a ==|== b", "with nothing selected, ready to type in");
        assert_eq!(highlight("a |one\ntwo| b", red), None, "a highlight does not run over lines");
    }

    #[test]
    fn a_highlight_changes_colour_or_goes_whole_from_anywhere_inside_it() {
        let green = Highlighter::Color([0, 128, 0]);
        let marked = "a <mark style=\"background: #00800066;\">w|or|d **x**</mark> b";
        assert_eq!(highlight("a ==w|or|d **x**== b", green).unwrap(), marked);
        assert_eq!(highlight(marked, Highlighter::Yellow).unwrap(), "a ==w|or|d **x**== b");
        assert_eq!(highlight(marked, Highlighter::Off).unwrap(), "a w|or|d **x** b");
        assert_eq!(highlight("a ==wo|rd== b", Highlighter::Off).unwrap(), "a wo|rd b");
        assert_eq!(highlight("a |==word==| b", Highlighter::Off).unwrap(), "a |word| b", "with its markers selected too");
        assert_eq!(highlight("a wo|rd b", Highlighter::Off), None, "there is none to take off");
        assert_eq!(highlight("==a== |b|", Highlighter::Yellow).unwrap(), "==a== ==|b|==", "a highlight elsewhere on the line is left alone");
    }

    #[test]
    fn inserted_text_takes_the_place_of_the_selection() {
        let edit = insert_text("one twó three", 4..8, "2é");
        assert_eq!((edit.text.as_str(), edit.selection), ("one 2é three", 7..7));
        assert_eq!(insert_text("", 0..0, "x").text, "x");
    }

    #[test]
    fn blocks_are_inserted_on_lines_of_their_own() {
        let at = |marked: &str, insert: fn(&str, usize) -> Edit| run(marked, |text, s| Some(insert(text, s.start))).unwrap();
        assert_eq!(at("|", insert_code_block), "```\n|\n```");
        assert_eq!(at("text|\nmore", insert_code_block), "text\n```\n|\n```\nmore");
        assert_eq!(at("te|xt", insert_callout), "text\n> [!note]\n> |");
        assert_eq!(at("one\n|\nthree", insert_table), "one\n| | |  |\n| --- | --- |\n|  |  |\nthree");
        assert_eq!(at("text|", insert_rule), "text\n---\n|");
    }

    #[test]
    fn inserted_blocks_are_what_the_parser_calls_them() {
        let kinds = |edit: Edit| markdown::parse(&edit.text).into_iter().map(|line| line.kind).collect::<Vec<_>>();
        assert!(matches!(&kinds(insert_callout("", 0))[..], [Kind::Quote { callout: Some(name), .. }, _] if name == "note"));
        assert_eq!(kinds(insert_code_block("", 0)), [Kind::Fence { open: true }, Kind::Code, Kind::Fence { open: false }]);
        assert!(matches!(&kinds(insert_table("", 0))[..], [Kind::TableRow { .. }, Kind::TableRule, Kind::TableRow { .. }]));
        assert_eq!(kinds(insert_rule("text", 4))[1], Kind::Rule);
    }

    fn type_char(marked: &str, typed: char) -> Option<String> {
        run(marked, |text, selection| pair(text, selection, typed))
    }

    #[test]
    fn a_typed_bracket_wraps_the_selection() {
        assert_eq!(type_char("a |word| b", '(').unwrap(), "a (|word|) b");
        assert_eq!(type_char("a |word| b", '[').unwrap(), "a [|word|] b");
        assert_eq!(type_char("a |word| b", '"').unwrap(), "a \"|word|\" b");
        assert_eq!(type_char("a |word| b", '*').unwrap(), "a *|word|* b");
        assert_eq!(type_char("a *|word|* b", '*').unwrap(), "a **|word|** b", "twice for bold");
        assert_eq!(type_char("a |wörd| b", '=').unwrap(), "a =|wörd|= b");
        assert_eq!(type_char("a |word| b", 'x'), None, "a letter replaces the selection as always");
    }

    #[test]
    fn an_opening_bracket_brings_its_partner() {
        assert_eq!(type_char("say |", '(').unwrap(), "say (|)");
        assert_eq!(type_char("|", '[').unwrap(), "[|]");
        assert_eq!(type_char("[|]", '[').unwrap(), "[[|]]", "which makes a wikilink of two");
        assert_eq!(type_char("say |", '"').unwrap(), "say \"|\"");
        assert_eq!(type_char("run |", '`').unwrap(), "run `|`");
        assert_eq!(type_char("f(|)", '{').unwrap(), "f({|})");
    }

    #[test]
    fn a_partner_is_left_out_where_it_would_be_in_the_way() {
        assert_eq!(type_char("|word", '('), None, "in front of a word");
        assert_eq!(type_char("don|", '"'), None, "a quote after a letter closes something");
        assert_eq!(type_char("``|", '`'), None, "three backticks are a code fence");
        assert_eq!(type_char("a |", '*'), None, "a star on its own may be a bullet");
        assert_eq!(type_char("snake|", '_'), None);
    }

    #[test]
    fn a_closing_bracket_typed_in_front_of_itself_steps_over_it() {
        assert_eq!(type_char("(a|)", ')').unwrap(), "(a)|");
        assert_eq!(type_char("[[Note|]]", ']').unwrap(), "[[Note]|]");
        assert_eq!(type_char("\"hi|\"", '"').unwrap(), "\"hi\"|");
        assert_eq!(type_char("`|`", '`').unwrap(), "``|");
        assert_eq!(type_char("(a|", ')'), None, "with nothing to step over it is typed");
    }

    #[test]
    fn enter_continues_lists_tasks_and_quotes() {
        assert_eq!(enter("- one|").unwrap(), "- one\n- |");
        assert_eq!(enter("* one|").unwrap(), "* one\n* |");
        assert_eq!(enter("  - nested|").unwrap(), "  - nested\n  - |");
        assert_eq!(enter("\t- tabbed|").unwrap(), "\t- tabbed\n\t- |");
        assert_eq!(enter("9. nine|").unwrap(), "9. nine\n10. |");
        assert_eq!(enter("1) one|").unwrap(), "1) one\n2) |");
        assert_eq!(enter("- [x] done|").unwrap(), "- [x] done\n- [ ] |", "the new task starts unticked");
        assert_eq!(enter("> quoted|").unwrap(), "> quoted\n> |");
        assert_eq!(enter("> [!tip] Title|").unwrap(), "> [!tip] Title\n> |");
        assert_eq!(enter("intro\n- one|\nafter").unwrap(), "intro\n- one\n- |\nafter");
    }

    #[test]
    fn enter_in_the_middle_of_an_item_splits_it() {
        assert_eq!(enter("- one| two").unwrap(), "- one\n- | two");
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        assert_eq!(enter("- one\n- |").unwrap(), "- one\n|");
        assert_eq!(enter("- one\n  - [ ] |\nafter").unwrap(), "- one\n|\nafter");
        assert_eq!(enter("> |").unwrap(), "|");
    }

    #[test]
    fn enter_elsewhere_is_left_alone() {
        assert_eq!(enter("plain text|"), None);
        assert_eq!(enter("# Heading|"), None);
        assert_eq!(enter("|- item"), None, "the caret is before the marker");
        assert_eq!(enter("```\n- code|\n```"), None, "a dash inside a code block is not a list");
        assert_eq!(enter("|"), None);
    }

    #[test]
    fn tab_nests_a_list_item_and_shift_tab_brings_it_back() {
        assert_eq!(tab("- a\n- b|", false).unwrap(), "- a\n\t- b|");
        assert_eq!(tab("- [ ] ta|sk", false).unwrap(), "\t- [ ] ta|sk");
        assert_eq!(tab("\t- b|", true).unwrap(), "- b|");
        assert_eq!(tab("    - b|", true).unwrap(), "- b|");
        assert_eq!(tab("  1. b|", true).unwrap(), "1. b|");
        assert_eq!(tab(" |  - b", true).unwrap(), "|- b", "the caret cannot end up on the line above");
        assert_eq!(tab("- b|", true), None, "nothing to remove");
        assert_eq!(tab("plain|", false), None, "Tab only nests list items");
    }

    #[test]
    fn ctrl_backspace_takes_one_word_of_any_script_and_stays_on_its_line() {
        let back = |text: &str| {
            let edit = delete_word(text, text.len(), false).unwrap();
            assert_eq!(edit.selection, edit.text.len()..edit.text.len());
            edit.text
        };
        assert_eq!(back("first line\nПривіт світе"), "first line\nПривіт ");
        assert_eq!(back("first line\nПривіт світе  "), "first line\nПривіт ", "the spaces go with the word");
        assert_eq!(back("Ελληνικά and **bold**"), "Ελληνικά and **bold", "marks are a word of their own");
        assert_eq!(back("one\n### Заголовок"), "one\n### ");
        assert_eq!(back("one\n### "), "one\n");
        assert_eq!(back("one\n   "), "one\n", "spaces alone are taken up to the line's start");
        assert_eq!(back("one\n"), "one", "at a line's start, the line break");
        assert_eq!(back("one\r\n"), "one", "which may be written as two");
        assert_eq!(back("снake_case"), "", "letters of two scripts in one word");
        assert_eq!(delete_word("", 0, false), None);
    }

    #[test]
    fn ctrl_delete_takes_the_word_after_the_caret() {
        let ahead = |text: &str, caret: usize| delete_word(text, caret, true).map(|edit| (edit.text, edit.selection));
        assert_eq!(ahead("Привіт світе\nдалі", 0), Some((" світе\nдалі".to_owned(), 0..0)));
        assert_eq!(ahead("Привіт світе\nдалі", "Привіт".len()), Some(("Привіт\nдалі".to_owned(), 12..12)));
        assert_eq!(ahead("один\r\nдва", "один".len()), Some(("одиндва".to_owned(), 8..8)));
        assert_eq!(ahead("end", 3), None);
    }
}
