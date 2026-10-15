//! Searching notes with a small query language.
//!
//! Words are all wanted, in the note's name or its text, in any order and
//! ignoring case: `meeting notes`. Around that:
//! - `"a phrase"` is found as written, spaces and all
//! - `a OR b` wants either; `-a` wants a note without it; `( )` groups
//! - `#tag` and `tag:tag` want the tag, or one nested under it
//! - `file:` looks in the note's name, `path:` in its folders and name,
//!   `content:` in its text only
//! - `line:(a b)` wants the words on one line, `block:( )` in one paragraph,
//!   `section:( )` under one heading
//! - `task:`, `task-todo:` and `task-done:` look in tasks, in those still to
//!   do, and in those done
//! - `match-case:` and `ignore-case:` say whether capitals matter
//!
//! `/Regular expressions/` are not understood; a `/` is a character
//! like any other.

use std::cell::OnceCell;
use std::ops::Range;
use std::path::Path;

use crate::markdown::{self, Kind, Line};
use crate::{tags, Note};

/// A parsed search. Any text is a query: what cannot be made sense of as
/// syntax is searched for as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Query(Expr);

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    /// What an empty query is, and an operator with nothing after it.
    All,
    Text(String),
    Tag(String),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    /// The inner query, asked of one part of the note at a time.
    In(Part, Box<Expr>),
    /// The inner query, with capitals mattering or not.
    Case(bool, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    File,
    Path,
    Content,
    Line,
    Block,
    Section,
    /// Tasks: all of them, or only those that are done or not done.
    Task(Option<bool>),
}

/// A line of a note that a search found something on.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// Counted from 1, as editors do.
    pub line: usize,
    /// The line in the note's text, without its line break.
    pub range: Range<usize>,
    /// The first thing found on it.
    pub hit: Range<usize>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        let mut parser = Parser { tokens: lex(text), at: 0 };
        let mut parts = Vec::new();
        loop {
            parts.push(parser.any_of());
            // Only a `)` with nothing to close stops the parser early: step over it.
            if parser.next().is_none() {
                break;
            }
        }
        Self(all_of(parts))
    }

    /// Whether this is the query that every note matches.
    pub fn is_empty(&self) -> bool {
        self.0 == Expr::All
    }

    /// Whether `note` matches. `root` is the vault's folder, which `path:` leaves out.
    pub fn matches(&self, note: &Note, root: &Path) -> bool {
        let path = note.path.strip_prefix(root).unwrap_or(&note.path).with_extension("");
        let subject = Subject { note, path: path.to_string_lossy().replace('\\', "/"), cache: Cache::default() };
        subject.is(&self.0, None, false)
    }

    /// The lines of `note` that show why it matches: those with one of the
    /// words searched for, leaving out words the query wants absent or looks
    /// for in the note's name. A note can match with no such line.
    pub fn lines(&self, note: &Note) -> Vec<Found> {
        let mut needles = Vec::new();
        collect(&self.0, false, None, &mut needles);
        if needles.is_empty() {
            return Vec::new();
        }
        let text = note.content.as_str();
        let mut found = Vec::new();
        for (index, line) in markdown::parse(text).iter().enumerate() {
            let hit = needles.iter().filter(|needle| needle.wants(line)).filter_map(|needle| {
                let at = find(&text[line.range.clone()], &needle.text, needle.case)?;
                Some(line.range.start + at.start..line.range.start + at.end)
            });
            if let Some(hit) = hit.min_by_key(|hit| hit.start) {
                found.push(Found { line: index + 1, range: line.range.clone(), hit });
            }
        }
        found
    }
}

/// Every place `needle` is in `text`, one after the other without overlap.
pub fn occurrences(text: &str, needle: &str, case: bool) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&text[from..], needle, case) {
        found.push(from + at.start..from + at.end);
        from += at.end;
    }
    found
}

/// The first place `needle` is in `text`. Without `case`, capitals do not
/// matter; what is found can then differ in length from what was looked for.
fn find(text: &str, needle: &str, case: bool) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    if case {
        return text.find(needle).map(|start| start..start + needle.len());
    }
    text.char_indices().find_map(|(start, _)| Some(start..start + prefix(&text[start..], needle)?))
}

/// How much of the start of `text` is `needle`, capitals aside.
fn prefix(text: &str, needle: &str) -> Option<usize> {
    let mut wanted = needle.chars().flat_map(char::to_lowercase).peekable();
    for (at, c) in text.char_indices() {
        for lower in c.to_lowercase() {
            if wanted.next() != Some(lower) {
                return None;
            }
        }
        if wanted.peek().is_none() {
            return Some(at + c.len_utf8());
        }
    }
    None
}

fn contains(text: &str, needle: &str, case: bool) -> bool {
    if case {
        text.contains(needle)
    } else {
        text.to_lowercase().contains(&needle.to_lowercase())
    }
}

/// A note being asked whether it matches, with what is worked out on the way.
struct Subject<'a> {
    note: &'a Note,
    /// Its place in the vault: folders and name, with `/` between them.
    path: String,
    cache: Cache,
}

#[derive(Default)]
struct Cache {
    lower_title: OnceCell<String>,
    lower_content: OnceCell<String>,
    tags: OnceCell<Vec<String>>,
    lines: OnceCell<Vec<Line>>,
}

impl Subject<'_> {
    /// Whether `expr` holds. `part` is the piece of the note it is asked of,
    /// or `None` for the note as a whole: its name and its text.
    fn is(&self, expr: &Expr, part: Option<&str>, case: bool) -> bool {
        let content = self.note.content.as_str();
        match expr {
            Expr::All => true,
            Expr::Text(text) => match part {
                Some(part) => contains(part, text, case),
                None if case => self.note.title.contains(text) || content.contains(text),
                None => {
                    let text = text.to_lowercase();
                    let title = self.cache.lower_title.get_or_init(|| self.note.title.to_lowercase());
                    title.contains(&text) || self.cache.lower_content.get_or_init(|| content.to_lowercase()).contains(&text)
                }
            },
            Expr::Tag(tag) => match part {
                Some(part) => contains(part, &format!("#{tag}"), false),
                None => {
                    let tag = tag.to_lowercase();
                    let nested = format!("{tag}/");
                    let tags = self.cache.tags.get_or_init(|| tags(content).iter().map(|t| t.to_lowercase()).collect());
                    tags.iter().any(|t| *t == tag || t.starts_with(&nested))
                }
            },
            Expr::Not(inner) => !self.is(inner, part, case),
            Expr::And(all) => all.iter().all(|inner| self.is(inner, part, case)),
            Expr::Or(any) => any.iter().any(|inner| self.is(inner, part, case)),
            Expr::Case(case, inner) => self.is(inner, part, *case),
            Expr::In(Part::File, inner) => self.is(inner, Some(&self.note.title), case),
            Expr::In(Part::Path, inner) => self.is(inner, Some(&self.path), case),
            Expr::In(Part::Content, inner) => self.is(inner, Some(content), case),
            Expr::In(Part::Line, inner) => content.lines().any(|line| self.is(inner, Some(line), case)),
            Expr::In(Part::Block, inner) => self.runs(|line| line.kind == Kind::Blank, false).any(|run| self.is(inner, Some(run), case)),
            Expr::In(Part::Section, inner) => {
                self.runs(|line| matches!(line.kind, Kind::Heading(_)), true).any(|run| self.is(inner, Some(run), case))
            }
            Expr::In(Part::Task(wanted), inner) => self.lines().iter().any(|line| match &line.kind {
                Kind::Task { done, .. } if wanted.map_or(true, |wanted| wanted == *done) => {
                    self.is(inner, Some(&content[line.marker.end..line.range.end]), case)
                }
                _ => false,
            }),
        }
    }

    fn lines(&self) -> &[Line] {
        self.cache.lines.get_or_init(|| markdown::parse(&self.note.content))
    }

    /// The note's text cut at every line that `breaks`. Such a line starts
    /// the next piece if it is to be `kept`, and is otherwise left out.
    fn runs<'s>(&'s self, breaks: impl Fn(&Line) -> bool + 's, kept: bool) -> impl Iterator<Item = &'s str> + 's {
        let lines = self.lines();
        let mut start = 0;
        let mut pieces = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if breaks(line) {
                pieces.push(start..index);
                start = if kept { index } else { index + 1 };
            }
        }
        pieces.push(start..lines.len());
        let text = move |piece: Range<usize>| &self.note.content[lines[piece.start].range.start..lines[piece.end - 1].range.end];
        pieces.into_iter().filter(|piece| !piece.is_empty()).map(text)
    }
}

/// Something to look for on a line when showing what a search found.
struct Needle {
    text: String,
    case: bool,
    /// Set when it only counts in a task, and then in which.
    task: Option<Option<bool>>,
}

impl Needle {
    fn wants(&self, line: &Line) -> bool {
        match (self.task, &line.kind) {
            (None, _) => true,
            (Some(wanted), Kind::Task { done, .. }) => wanted.map_or(true, |wanted| wanted == *done),
            (Some(_), _) => false,
        }
    }
}

fn collect(expr: &Expr, case: bool, task: Option<Option<bool>>, out: &mut Vec<Needle>) {
    match expr {
        Expr::Text(text) => out.push(Needle { text: text.clone(), case, task }),
        Expr::Tag(tag) => out.push(Needle { text: format!("#{tag}"), case: false, task }),
        Expr::And(inner) | Expr::Or(inner) => inner.iter().for_each(|inner| collect(inner, case, task, out)),
        Expr::Case(case, inner) => collect(inner, *case, task, out),
        Expr::In(Part::Task(wanted), inner) => collect(inner, case, Some(*wanted), out),
        // A note's name is not a line of it, and what must be absent is not there to show.
        Expr::In(Part::File | Part::Path, _) | Expr::Not(_) | Expr::All => {}
        Expr::In(_, inner) => collect(inner, case, task, out),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Open,
    Close,
    Or,
    Not,
    Tag,
    In(Part),
    Case(bool),
    Word { text: String, quoted: bool },
}

/// The operator a word names once a `:` follows it.
fn operator(name: &str) -> Option<Token> {
    Some(match name {
        "file" => Token::In(Part::File),
        "path" => Token::In(Part::Path),
        "content" => Token::In(Part::Content),
        "line" => Token::In(Part::Line),
        "block" => Token::In(Part::Block),
        "section" => Token::In(Part::Section),
        "task" => Token::In(Part::Task(None)),
        "task-todo" => Token::In(Part::Task(Some(false))),
        "task-done" => Token::In(Part::Task(Some(true))),
        "tag" => Token::Tag,
        "match-case" => Token::Case(true),
        "ignore-case" => Token::Case(false),
        _ => return None,
    })
}

fn lex(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            // A dash on its own is something to look for, not a "not".
            '-' if chars.peek().is_some_and(|next| !next.is_whitespace()) => tokens.push(Token::Not),
            '"' => {
                let text: String = chars.by_ref().take_while(|&c| c != '"').collect();
                if !text.is_empty() {
                    tokens.push(Token::Word { text, quoted: true });
                }
            }
            _ => {
                let mut word = String::from(c);
                let mut token = None;
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '(' | ')' | '"') {
                        break;
                    }
                    chars.next();
                    if next == ':' {
                        token = operator(&word);
                        if token.is_some() {
                            break;
                        }
                    }
                    word.push(next);
                }
                tokens.push(token.unwrap_or(if word == "OR" { Token::Or } else { Token::Word { text: word, quoted: false } }));
            }
        }
    }
    tokens
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += usize::from(token.is_some());
        token
    }

    /// Alternatives with `OR` between them.
    fn any_of(&mut self) -> Expr {
        let mut alternatives = vec![self.all_of()];
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            alternatives.push(self.all_of());
        }
        match alternatives.len() {
            1 => alternatives.remove(0),
            _ => Expr::Or(alternatives),
        }
    }

    /// Terms side by side, which are all wanted.
    fn all_of(&mut self) -> Expr {
        let mut terms = Vec::new();
        while !matches!(self.peek(), None | Some(Token::Close | Token::Or)) {
            terms.push(self.term());
        }
        all_of(terms)
    }

    fn term(&mut self) -> Expr {
        match self.next() {
            Some(Token::Not) => Expr::Not(Box::new(self.term())),
            Some(Token::Open) => {
                let inner = self.any_of();
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                }
                inner
            }
            Some(Token::Word { text, quoted: false }) if text.len() > 1 && text.starts_with('#') => Expr::Tag(text[1..].to_owned()),
            Some(Token::Word { text, .. }) => Expr::Text(text),
            Some(operator) => {
                // An operator still being typed, with nothing after it yet, asks for nothing.
                if matches!(self.peek(), None | Some(Token::Close | Token::Or)) {
                    return Expr::All;
                }
                let inner = self.term();
                match (operator, inner) {
                    (Token::In(part), inner) => Expr::In(part, Box::new(inner)),
                    (Token::Case(case), inner) => Expr::Case(case, Box::new(inner)),
                    (Token::Tag, Expr::Text(tag)) => Expr::Tag(tag.trim_start_matches('#').to_owned()),
                    (Token::Tag, Expr::Tag(tag)) => Expr::Tag(tag),
                    _ => Expr::All,
                }
            }
            None => Expr::All,
        }
    }
}

fn all_of(mut terms: Vec<Expr>) -> Expr {
    terms.retain(|term| *term != Expr::All);
    match terms.len() {
        0 => Expr::All,
        1 => terms.remove(0),
        _ => Expr::And(terms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn note(path: &str, content: &str) -> Note {
        let path = PathBuf::from("vault").join(path);
        Note { title: path.file_stem().unwrap().to_string_lossy().into_owned(), path, content: content.into() }
    }

    fn matches(query: &str, note: &Note) -> bool {
        Query::parse(query).matches(note, Path::new("vault"))
    }

    /// The numbers of the lines a search shows, with what was found on each.
    fn lines<'a>(query: &str, note: &'a Note) -> Vec<(usize, &'a str)> {
        Query::parse(query).lines(note).into_iter().map(|found| (found.line, &note.content[found.hit])).collect()
    }

    #[test]
    fn words_are_all_wanted_in_the_name_or_the_text() {
        let plan = note("Plan.md", "Buy milk #todo\nand Bread");
        assert!(matches("", &plan) && matches("   ", &plan));
        for query in ["plan", "milk", "MILK bread", "bread plan", "lk", "#todo"] {
            assert!(matches(query, &plan), "{query}");
        }
        for query in ["eggs", "milk eggs", "#milk", "#tod"] {
            assert!(!matches(query, &plan), "{query}");
        }
    }

    #[test]
    fn a_phrase_in_quotes_is_found_as_written() {
        let plan = note("Plan.md", "buy milk and bread OR cake");
        assert!(matches("\"milk and\"", &plan));
        assert!(!matches("\"milk bread\"", &plan));
        assert!(matches("milk bread", &plan));
        assert!(matches("\"OR\" \"#\"", &note("N.md", "this OR that, item #1")), "quotes make words of syntax");
        assert!(matches("\"unclosed quote", &note("N.md", "an unclosed quote")));
    }

    #[test]
    fn or_not_and_brackets_combine_words() {
        let plan = note("Plan.md", "milk and bread");
        assert!(matches("eggs OR milk", &plan));
        assert!(!matches("eggs OR cake", &plan));
        assert!(matches("milk -eggs", &plan));
        assert!(!matches("milk -bread", &plan));
        assert!(matches("(eggs OR milk) bread", &plan));
        assert!(!matches("(eggs OR milk) cake", &plan));
        assert!(matches("-(eggs cake)", &plan));
        assert!(!matches("or", &plan), "only OR in capitals is the operator");
        assert!(matches("self-made - done", &note("N.md", "self-made - done")), "a dash inside or alone is just a dash");
    }

    #[test]
    fn half_typed_queries_still_mean_something() {
        let plan = note("Plan.md", "milk");
        for query in ["milk OR", "OR milk", "(milk", "milk)", "milk ) (", "file:", "milk tag:", "line:("] {
            assert!(matches(query, &plan), "{query}");
        }
        assert!(Query::parse("tag:").is_empty() && Query::parse(" ").is_empty());
        assert!(!Query::parse("milk").is_empty());
        assert!(matches("a:b", &note("N.md", "ratio a:b")), "an unknown operator is a word with a colon in it");
    }

    #[test]
    fn tags_match_whole_and_nested_but_not_in_part() {
        let plan = note("Plan.md", "---\ntags: [Home]\n---\n#work/urgent and `#code`");
        assert!(matches("tag:work", &plan) && matches("tag:#work", &plan) && matches("#work", &plan));
        assert!(matches("tag:work/urgent", &plan) && matches("tag:WORK", &plan));
        assert!(matches("tag:home", &plan), "tags in the properties count");
        assert!(!matches("tag:wor", &plan) && !matches("tag:urgent", &plan));
        assert!(!matches("tag:code", &plan), "what is in code is not a tag");
        assert!(matches("-tag:play", &plan));
    }

    #[test]
    fn file_path_and_content_say_where_to_look() {
        let plan = note("Projects/2026/Plan.md", "see the budget");
        assert!(matches("file:plan", &plan) && !matches("file:budget", &plan) && !matches("file:projects", &plan));
        assert!(matches("path:projects/2026", &plan) && matches("path:plan", &plan) && !matches("path:.md", &plan));
        assert!(matches("content:budget", &plan) && !matches("content:plan", &plan));
        assert!(matches("file:(plan OR draft) budget", &plan));
        assert!(matches("file:\"plan\"", &plan));
    }

    #[test]
    fn line_block_and_section_want_the_words_together() {
        let text = "# Shopping\nmilk today\nbread tomorrow\n\ncake\n## Later\neggs";
        let list = note("List.md", text);
        assert!(matches("milk bread", &list));
        assert!(matches("line:(milk today)", &list) && !matches("line:(milk bread)", &list));
        assert!(matches("block:(milk bread)", &list) && !matches("block:(milk cake)", &list));
        assert!(matches("section:(milk cake)", &list) && !matches("section:(milk eggs)", &list));
        assert!(matches("section:(later eggs)", &list), "a heading belongs to its own section");
        assert!(matches("line:milk", &list), "a single word needs no brackets");
    }

    #[test]
    fn task_operators_look_only_in_tasks() {
        let list = note("List.md", "- [ ] call Anna\n- [x] pay rent\n- plain call\nrent is due");
        assert!(matches("task:call", &list) && matches("task:rent", &list));
        assert!(matches("task-todo:call", &list) && !matches("task-todo:rent", &list));
        assert!(matches("task-done:rent", &list) && !matches("task-done:call", &list));
        assert!(!matches("task:due", &list) && !matches("task:plain", &list));
        assert!(!matches("task:\"[ ]\"", &list), "the box is not part of the task's text");
        assert_eq!(lines("task-done:rent", &list), [(2, "rent")], "and only tasks are shown for them");
    }

    #[test]
    fn capitals_matter_only_when_asked() {
        let plan = note("Plan.md", "Milk and Ünïcode");
        assert!(matches("milk ÜNÏCODE", &plan));
        assert!(matches("match-case:Milk", &plan) && !matches("match-case:milk", &plan));
        assert!(matches("match-case:(Milk ignore-case:AND)", &plan));
        assert!(matches("match-case:file:Plan", &plan) && !matches("match-case:file:plan", &plan));
    }

    #[test]
    fn the_lines_shown_are_those_with_the_words_on_them() {
        let plan = note("Milk.md", "Buy MILK today\r\nnothing here\n- [ ] milk again #Todo\nÜber milk");
        assert_eq!(lines("milk", &plan), [(1, "MILK"), (3, "milk"), (4, "milk")]);
        assert_eq!(lines("today OR again", &plan), [(1, "today"), (3, "again")]);
        assert_eq!(lines("milk -nothing", &plan).len(), 3, "what must be absent is not shown");
        assert_eq!(lines("#todo", &plan), [(3, "#Todo")]);
        assert_eq!(lines("über", &plan), [(4, "Über")]);
        assert_eq!(lines("file:milk", &plan), [], "the name is not a line");
        assert_eq!(lines("", &plan), []);
        assert_eq!(&plan.content[Query::parse("today").lines(&plan)[0].range.clone()], "Buy MILK today", "without the line break");
    }

    #[test]
    fn occurrences_are_found_one_after_the_other() {
        assert_eq!(occurrences("aXa xA", "xa", false), [1..3, 4..6]);
        assert_eq!(occurrences("aXa xA", "xa", true), []);
        assert_eq!(occurrences("aaaa", "aa", false), [0..2, 2..4], "without overlap");
        assert_eq!(occurrences("é É e", "É", false), [0..2, 3..5]);
        assert_eq!(occurrences("é É e", "É", true), [3..5]);
        assert_eq!(occurrences("text", "", false), []);
        assert_eq!(occurrences("", "x", false), []);
    }
}
