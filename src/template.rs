//! Templates: notes kept in one folder
//! whose text is inserted into another note, with `{{title}}`, `{{date}}`
//! and `{{time}}` filled in on the way.
//!
//! Which folder is a setting, `Templates` at the top of the vault unless the
//! settings file says otherwise. `{{date:…}}` and `{{time:…}}` take a format in
//! Moment.js's letters (`YYYY-MM-DD`, `dddd`, `HH:mm`), which is how
//! templates written for other note apps give one.

use chrono::{NaiveDate, NaiveDateTime};

/// The folder whose notes are the templates, unless the settings say another.
pub const FOLDER: &str = "Templates";

/// The text of `template` as it goes into the note called `title` at the
/// time `now`. Anything in braces that is not one of the three variables is
/// left as it is written.
pub fn fill(template: &str, title: &str, now: NaiveDateTime) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}").map(|close| open + close) else { break };
        out.push_str(&rest[..open]);
        let inner = &rest[open + 2..close];
        let (name, format) = inner.split_once(':').map_or((inner, None), |(name, format)| (name, Some(format)));
        match (name.trim(), format) {
            ("title", None) => out.push_str(title),
            ("date", format) => out.push_str(&formatted(now, format.unwrap_or("YYYY-MM-DD"))),
            ("time", format) => out.push_str(&formatted(now, format.unwrap_or("HH:mm"))),
            _ => out.push_str(&rest[open..close + 2]),
        }
        rest = &rest[close + 2..];
    }
    out + rest
}

/// Moment.js's format letters and what chrono calls the same thing, the
/// longer runs first so that `MMMM` is not read as two `MM`.
const TOKENS: [(&str, &str); 19] = [
    ("YYYY", "%Y"),
    ("YY", "%y"),
    ("MMMM", "%B"),
    ("MMM", "%b"),
    ("MM", "%m"),
    ("M", "%-m"),
    ("DD", "%d"),
    ("D", "%-d"),
    ("dddd", "%A"),
    ("ddd", "%a"),
    ("HH", "%H"),
    ("H", "%-H"),
    ("hh", "%I"),
    ("h", "%-I"),
    ("mm", "%M"),
    ("ss", "%S"),
    ("A", "%p"),
    ("a", "%P"),
    ("%", "%%"),
];

/// `now` written in a Moment.js format. Text in `[square brackets]` is taken
/// as written, as are letters that stand for nothing.
pub fn formatted(now: NaiveDateTime, format: &str) -> String {
    now.format(&spec(format)).to_string()
}

/// The day that `text` is, written in a Moment.js format: what a daily
/// note's name says. A format that does not say which day it is, or text not
/// written in it, is no day.
pub fn read_date(text: &str, format: &str) -> Option<NaiveDate> {
    // chrono reads a number of one digit or two the same way, and does not
    // know the `-` that leaves off a nought when writing one.
    NaiveDate::parse_from_str(text, &spec(format).replace("%-", "%")).ok()
}

/// A Moment.js format said in chrono's letters.
fn spec(format: &str) -> String {
    let mut spec = String::new();
    let mut rest = format;
    while let Some(first) = rest.chars().next() {
        if let Some(close) = rest.strip_prefix('[').and_then(|inner| inner.find(']')) {
            spec.push_str(&rest[1..close + 1].replace('%', "%%"));
            rest = &rest[close + 2..];
        } else if let Some((token, chrono)) = TOKENS.iter().find(|(token, _)| rest.starts_with(token)) {
            spec.push_str(chrono);
            rest = &rest[token.len()..];
        } else {
            spec.push(first);
            rest = &rest[first.len_utf8()..];
        }
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 5).unwrap().and_hms_opt(9, 7, 30).unwrap()
    }

    #[test]
    fn the_title_date_and_time_are_filled_in() {
        let filled = fill("# {{title}}\n{{date}} at {{time}}, {{ title }}", "Plan", now());
        assert_eq!(filled, "# Plan\n2026-03-05 at 09:07, Plan");
    }

    #[test]
    fn dates_and_times_take_a_format() {
        let filled = fill("{{date:dddd, D MMMM YYYY}} | {{time:h:mm A}} | {{date:[Day] DD/MM/YY}} | {{date:é 100%}}", "x", now());
        assert_eq!(filled, "Thursday, 5 March 2026 | 9:07 AM | Day 05/03/26 | é 100%");
    }

    #[test]
    fn other_braces_are_left_alone() {
        for text in ["{{other}} {{title:x}} {{", "a }} b {{title", "{{}}"] {
            assert_eq!(fill(text, "T", now()), text);
        }
        assert_eq!(fill("{{unknown}}{{title}}", "T", now()), "{{unknown}}T");
    }

    #[test]
    fn a_date_is_read_back_in_the_format_it_was_written_in() {
        for format in ["YYYY-MM-DD", "D MMMM YYYY", "dddd, D MMM YY", "[Day] YYYY.M.D"] {
            let written = formatted(now(), format);
            assert_eq!(read_date(&written, format), Some(now().date()), "{format}: {written}");
        }
        assert_eq!(read_date("2026-13-40", "YYYY-MM-DD"), None);
        assert_eq!(read_date("Plan", "YYYY-MM-DD"), None);
        assert_eq!(read_date("Thursday", "dddd"), None, "not a day without a date");
    }
}
