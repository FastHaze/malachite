//! The format of the settings file, `malachite.conf`: the kind of file that is
//! edited by hand.
//!
//! A line is `name = value`, under a `[section]` heading. A line that starts
//! with `#` is a comment, and a blank line is nothing. There are no comments
//! at the end of a line: a colour is written `#a882ff`, and that is a value.
//! Names and sections are read without regard to capitals, and with `-` for
//! `_`.
//!
//! The file belongs to the person who edits it. When the app changes a
//! setting it rewrites the one line that setting is on, and everything else
//! in the file stays as it was written, comments and order included. This
//! module knows the format only; what the settings are is `appearance.rs`'s.

/// A setting as it stands in the file.
#[derive(Debug, PartialEq)]
pub struct Entry {
    /// The line it is on, counting from 1.
    pub line: usize,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// What one line of the file is.
#[derive(Debug, PartialEq)]
enum Line {
    Blank,
    /// A comment, without its `#`.
    Comment(String),
    Section(String),
    Setting(String, String),
    /// Something that is none of these.
    Stray,
}

/// A name as it is compared: `Text-Size` is `text_size`.
fn name(written: &str) -> String {
    written.trim().to_lowercase().replace('-', "_")
}

/// `name = value`, if that is what `text` is.
fn setting(text: &str) -> Option<(String, String)> {
    let (key, value) = text.split_once('=')?;
    let key = name(key);
    let word = !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    word.then(|| (key, value.trim().to_owned()))
}

fn classify(line: &str) -> Line {
    let line = line.trim();
    if line.is_empty() {
        return Line::Blank;
    }
    if let Some(comment) = line.strip_prefix(['#', ';']) {
        return Line::Comment(comment.trim_start_matches('#').trim().to_owned());
    }
    if let Some(section) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Line::Section(name(section));
    }
    setting(line).map_or(Line::Stray, |(key, value)| Line::Setting(key, value))
}

/// The settings in `text`, in the order they are written, and the numbers of
/// the lines that are not settings, sections, comments or blank.
pub fn read(text: &str) -> (Vec<Entry>, Vec<usize>) {
    let (mut entries, mut stray) = (Vec::new(), Vec::new());
    let mut section = String::new();
    for (index, line) in text.lines().enumerate() {
        match classify(line) {
            Line::Section(name) => section = name,
            Line::Setting(key, value) => entries.push(Entry { line: index + 1, section: section.clone(), key, value }),
            Line::Stray => stray.push(index + 1),
            Line::Blank | Line::Comment(_) => {}
        }
    }
    (entries, stray)
}

/// Where `key` is in `section`: the lines that set it, and the first comment
/// that would set it if it were not one (`# key = value`). With them, the
/// line after which a new setting of that section would go.
fn find(lines: &[String], section: &str, key: &str) -> (Vec<usize>, Option<usize>, Option<usize>) {
    let (mut set, mut commented, mut end) = (Vec::new(), None, None);
    let mut current = String::new();
    for (index, line) in lines.iter().enumerate() {
        let line = classify(line);
        if let Line::Section(name) = &line {
            current.clone_from(name);
        }
        if current != section {
            continue;
        }
        match line {
            Line::Setting(found, _) => {
                end = Some(index + 1);
                if found == key {
                    set.push(index);
                }
            }
            Line::Comment(comment) => {
                // A setting that is commented out is part of the section; other
                // comments may be about what comes after it.
                if let Some((found, _)) = setting(&comment) {
                    end = Some(index + 1);
                    if found == key && commented.is_none() {
                        commented = Some(index);
                    }
                }
            }
            Line::Section(_) => end = Some(index + 1),
            Line::Blank | Line::Stray => {}
        }
    }
    (set, commented, end)
}

/// The lines of `text`, and the line break it uses.
fn lines(text: &str) -> (Vec<String>, &'static str) {
    (text.lines().map(str::to_owned).collect(), if text.contains("\r\n") { "\r\n" } else { "\n" })
}

fn join(lines: &[String], line_break: &str) -> String {
    let mut text = lines.join(line_break);
    if !text.is_empty() {
        text.push_str(line_break);
    }
    text
}

/// `text` with `key` in `section` set to `value`. The line that sets it now
/// is rewritten, the last one if there are several, as that is the one that
/// counts. Otherwise a line that has it commented out becomes the setting,
/// or else it is added at the end of its section, which is made if need be.
pub fn set(text: &str, section: &str, key: &str, value: &str) -> String {
    let (mut lines, line_break) = lines(text);
    let setting = format!("{key} = {value}");
    let (set, commented, end) = find(&lines, section, key);
    match (set.last().copied().or(commented), end) {
        (Some(at), _) => lines[at] = setting,
        (None, Some(end)) => lines.insert(end, setting),
        (None, None) => {
            if lines.last().is_some_and(|last| !last.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.extend([format!("[{section}]"), setting]);
        }
    }
    join(&lines, line_break)
}

/// `text` without `section`: its heading and every line under it, but for
/// the comments at its end that come after a blank line, which are about the
/// section after it. A blank line that the section leaves at the end of the
/// file, or one of two in a row, goes with it.
pub fn without(text: &str, section: &str) -> String {
    let (lines, line_break) = lines(text);
    let mut kept: Vec<String> = Vec::new();
    let mut inside: Vec<String> = Vec::new();
    let mut leaving = false;
    for line in lines {
        let heading = matches!(classify(&line), Line::Section(_));
        if heading && leaving {
            // The comments since the last blank line are the next section's.
            let start = inside.iter().rposition(|line| line.trim().is_empty()).map_or(inside.len(), |blank| blank + 1);
            kept.extend(inside.drain(start..));
            leaving = false;
        }
        if heading && matches!(classify(&line), Line::Section(name) if name == section) {
            leaving = true;
            inside.clear();
            continue;
        }
        match leaving {
            true => inside.push(line),
            false => kept.push(line),
        }
    }
    let mut tidy: Vec<String> = Vec::new();
    for line in kept {
        if line.trim().is_empty() && tidy.last().is_some_and(|last| last.trim().is_empty()) {
            continue;
        }
        tidy.push(line);
    }
    while tidy.last().is_some_and(|last| last.trim().is_empty()) {
        tidy.pop();
    }
    join(&tidy, line_break)
}

/// `text` with `key` in `section` no longer set: the line that set it is
/// left as a comment showing `default`, which is what now holds.
pub fn unset(text: &str, section: &str, key: &str, default: &str) -> String {
    let (mut lines, line_break) = lines(text);
    let (set, _, _) = find(&lines, section, key);
    let Some((last, earlier)) = set.split_last() else { return text.to_owned() };
    lines[*last] = format!("# {key} = {default}");
    for at in earlier.iter().rev() {
        lines.remove(*at);
    }
    join(&lines, line_break)
}

/// `text` with the settings it has commented out (`# key = value`) shown at
/// other values: `shown` is asked about each, by its section, its name and
/// the value it shows, and says what it is to show instead, if anything.
/// Those lines are where the file says what a setting is when it is left
/// alone, and that can come to be something else.
pub fn recomment(text: &str, shown: impl Fn(&str, &str, &str) -> Option<String>) -> String {
    let (mut lines, line_break) = lines(text);
    let mut section = String::new();
    for line in &mut lines {
        match classify(line) {
            Line::Section(name) => section = name,
            Line::Comment(comment) => {
                let now = setting(&comment).and_then(|(key, value)| Some(format!("# {key} = {}", shown(&section, &key, &value)?)));
                *line = now.unwrap_or_else(|| std::mem::take(line));
            }
            Line::Blank | Line::Setting(..) | Line::Stray => {}
        }
    }
    join(&lines, line_break)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(line: usize, section: &str, key: &str, value: &str) -> Entry {
        Entry { line, section: section.into(), key: key.into(), value: value.into() }
    }

    #[test]
    fn settings_are_read_under_their_sections() {
        let text = "# A comment\ntop = 1\n\n[Appearance]\n  Text-Size=18 \n; another comment\naccent = #a882ff\n[colors.dark]\npage = #000000\n";
        let (entries, stray) = read(text);
        assert_eq!(
            entries,
            [entry(2, "", "top", "1"), entry(5, "appearance", "text_size", "18"), entry(7, "appearance", "accent", "#a882ff"), entry(9, "colors.dark", "page", "#000000")]
        );
        assert!(stray.is_empty());
    }

    #[test]
    fn lines_that_are_nothing_are_told_apart_from_comments() {
        let (entries, stray) = read("[a]\njust words\n= no name\nwhat is this = 1\n# fine = 2\nkey = a = b\n");
        assert_eq!(entries, [entry(6, "a", "key", "a = b")], "a value may have an `=` of its own");
        assert_eq!(stray, [2, 3, 4]);
    }

    #[test]
    fn setting_rewrites_only_the_line_of_that_setting() {
        let text = "# Mine.\n[appearance]\n# The size of the text.\ntext_size = 16\n\n[colors.dark]\n# page = #1e1e1e\n";
        let bigger = set(text, "appearance", "text_size", "20");
        assert_eq!(bigger, text.replace("text_size = 16", "text_size = 20"));
        // A setting that is commented out is taken up where it stands.
        let paged = set(&bigger, "colors.dark", "page", "#000000");
        assert_eq!(paged, bigger.replace("# page = #1e1e1e", "page = #000000"));
        assert_eq!(read(&paged).0.len(), 2);
    }

    #[test]
    fn a_new_setting_goes_at_the_end_of_its_section() {
        let text = "[appearance]\ntheme = dark\n\n# About the colours.\n[colors.dark]\npage = #000000\n";
        assert_eq!(set(text, "appearance", "text_size", "18"), text.replace("theme = dark\n", "theme = dark\ntext_size = 18\n"));
        assert_eq!(set(text, "colors.dark", "text", "#ffffff"), format!("{text}text = #ffffff\n"));
        // And in a section of its own if there is none for it yet.
        assert_eq!(set(text, "colors.light", "page", "#ffffff"), format!("{text}\n[colors.light]\npage = #ffffff\n"));
        assert_eq!(set("", "appearance", "theme", "light"), "[appearance]\ntheme = light\n");
        assert_eq!(set("[appearance]", "appearance", "theme", "light"), "[appearance]\ntheme = light\n");
    }

    #[test]
    fn the_line_that_counts_is_the_last_of_several() {
        let text = "[a]\nkey = 1\nother = x\nkey = 2\n[b]\nkey = 3\n";
        assert_eq!(set(text, "a", "key", "9"), "[a]\nkey = 1\nother = x\nkey = 9\n[b]\nkey = 3\n");
        assert_eq!(unset(text, "a", "key", "0"), "[a]\nother = x\n# key = 0\n[b]\nkey = 3\n");
        assert_eq!(unset(text, "a", "missing", "0"), text);
        // What was unset can be set again, on the same line.
        assert_eq!(set(&unset(text, "b", "key", "0"), "b", "key", "5"), text.replace("key = 3", "key = 5"));
    }

    #[test]
    fn what_a_commented_setting_shows_can_be_changed() {
        let text = "[a]\n# About it.\n# key = 1\nkey = 1\n# other = 1\n[b]\n# key = 1\n";
        let shown = recomment(text, |section, key, value| (section == "a" && key == "key").then(|| format!("{value}0")));
        assert_eq!(shown, "[a]\n# About it.\n# key = 10\nkey = 1\n# other = 1\n[b]\n# key = 1\n");
        assert_eq!(recomment(text, |_, _, _| None), text);
    }

    #[test]
    fn the_line_breaks_of_the_file_are_kept() {
        let text = "[a]\r\nkey = 1\r\n";
        assert_eq!(set(text, "a", "key", "2"), "[a]\r\nkey = 2\r\n");
        assert_eq!(set(text, "a", "more", "3"), "[a]\r\nkey = 1\r\nmore = 3\r\n");
    }

    #[test]
    fn a_section_can_be_taken_out_whole() {
        let text = "[a]\nx = 1\n\n[gone]\n# About y.\ny = 2\n\n# About the section after.\n[b]\nz = 3\n\n[gone]\nw = 4\n";
        assert_eq!(without(text, "gone"), "[a]\nx = 1\n\n# About the section after.\n[b]\nz = 3\n");
        assert_eq!(without("[a]\r\nx = 1\r\n", "gone"), "[a]\r\nx = 1\r\n", "nothing to take, nothing taken");
    }
}
