//! A small Markdown model for showing notes.
//!
//! The model is line-oriented: every source line gets a block type
//! (heading, list item, quote, code, ...) and its text is split into spans of
//! formatted text and *syntax markers* (`**`, `[[`, `#`, ...). Live Preview
//! shows the markers only while the cursor touches the construct they belong
//! to; Reading view never shows them. This module produces exactly that model
//! and leaves drawing to the GUI. All ranges are byte offsets into the source.
//!
//! It covers the syntax note-takers reach for daily, not all of CommonMark.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    /// `==text==`, or `<mark>text</mark>`
    pub highlight: bool,
    /// The colour of a highlight that names one, as red, green, blue and
    /// opacity: `<mark style="background: #ff000066;">text</mark>`. Markdown
    /// has the one highlight, `==text==`, and no way to name a colour; HTML in
    /// a note is how a highlight of another colour is written.
    pub tint: Option<[u8; 4]>,
    pub code: bool,
    /// `%%text%%`: visible while editing, absent from Reading view.
    pub comment: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    /// `[[Note]]` or `[text](Note.md)`, and with `[[Note#Heading]]` or
    /// `[[Note#^block]]` the place in the note. The name is empty for
    /// `[[#Heading]]`, which points into the note the link is in.
    Note(String, Option<Place>),
    Url(String),
    Tag(String),
    /// `![[picture.png]]`, `![[Note]]`, `![[Note#Heading]]` or
    /// `![alt](picture.png)`: what it names is shown in the note in place of
    /// the link. `width` is the number after a bar, `![[picture.png|200]]`,
    /// which is the width to draw a picture at.
    Embed { target: String, place: Option<Place>, width: Option<u32> },
    /// `[^id]`: a reference to the footnote that a `[^id]: text` line defines.
    Footnote(String),
}

/// A place inside a note that a link can point at.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// `#Heading`. `#One#Two` is the heading Two under the heading One.
    Heading(String),
    /// `#^id`: the block that ends in `^id`.
    Block(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    /// The whole construct this span is part of: `**bold**` for both the word
    /// and its `**` markers. Plain text is its own element.
    pub element: Range<usize>,
    /// Syntax characters rather than content.
    pub marker: bool,
    pub style: Style,
    pub link: Option<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Blank,
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered,
    /// `- [ ] text`; `checkbox` covers the three characters `[ ]` or `[x]`.
    Task { done: bool, checkbox: Range<usize> },
    /// `> text`. Inside a `> [!type]` block `callout` names the type, and on
    /// the block's first line `tag` covers `[!type] `.
    Quote { callout: Option<String>, tag: Option<Range<usize>> },
    /// `[^id]: text`: the text of the footnote `id`.
    Footnote(String),
    /// `---`, `***` or `___`
    Rule,
    /// The ``` line that opens or closes a code block.
    Fence { open: bool },
    Code,
    /// The `---` lines around YAML frontmatter at the top of a note.
    FrontmatterFence,
    Frontmatter,
    /// `| a | b |`; `cells` are the trimmed cell contents.
    TableRow { cells: Vec<Range<usize>> },
    /// The `|---|---|` line under a table header.
    TableRule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The line without its line break.
    pub range: Range<usize>,
    pub kind: Kind,
    /// Block syntax in front of the content: `## `, `- `, `- [x] `, `> `.
    /// For rules and fences it is the whole line. Empty for plain paragraphs.
    pub marker: Range<usize>,
    /// Inline content, covering `marker.end..range.end` without gaps.
    pub spans: Vec<Span>,
    /// The run of lines drawn as one block (code block, quote, callout, table,
    /// frontmatter). A line on its own is its own block.
    pub block: Range<usize>,
}

/// Split a note into classified lines.
pub fn parse(text: &str) -> Vec<Line> {
    // Frontmatter only counts when the note starts with `---` and it is closed.
    let frontmatter_end = text
        .strip_prefix("---")
        .filter(|rest| rest.starts_with('\n') || rest.starts_with("\r\n"))
        .and_then(|_| text.split('\n').skip(1).position(|l| l.trim_end() == "---"))
        .map(|i| i + 1);

    let mut lines: Vec<Line> = Vec::new();
    let mut pos = 0;
    // Fence character and length of the code block we are inside, if any.
    let mut fence: Option<(u8, usize)> = None;

    for raw in text.split('\n') {
        let start = pos;
        pos += raw.len() + 1;
        let content = raw.strip_suffix('\r').unwrap_or(raw);
        let range = start..start + content.len();
        let body = content.trim_start();
        let body_start = range.end - body.len();
        let index = lines.len();

        let mut marker = body_start..body_start;
        let kind = if frontmatter_end.is_some_and(|end| index <= end) {
            marker = range.clone();
            if index == 0 || Some(index) == frontmatter_end {
                Kind::FrontmatterFence
            } else {
                Kind::Frontmatter
            }
        } else if let Some((ch, len)) = fence {
            marker = range.clone();
            let run = body.bytes().take_while(|&b| b == ch).count();
            if run >= len && body[run..].trim().is_empty() {
                fence = None;
                Kind::Fence { open: false }
            } else {
                Kind::Code
            }
        } else if let Some(len) = fence_len(body) {
            marker = range.clone();
            fence = Some((body.as_bytes()[0], len));
            Kind::Fence { open: true }
        } else if body.is_empty() {
            Kind::Blank
        } else if let Some((level, len)) = heading(body) {
            marker.end += len;
            Kind::Heading(level)
        } else if let Some((id, len)) = footnote(body) {
            marker.end += len;
            Kind::Footnote(id)
        } else if is_rule(body) {
            marker = range.clone();
            Kind::Rule
        } else if body.starts_with('>') {
            let len = quote_marker(body);
            marker.end += len;
            match callout(&body[len..]) {
                Some((name, tag_len)) => {
                    let tag = marker.end..marker.end + tag_len;
                    marker.end = tag.end;
                    Kind::Quote { callout: Some(name), tag: Some(tag) }
                }
                None => {
                    // Later lines of a callout inherit its type.
                    let callout = match lines.last().map(|l| &l.kind) {
                        Some(Kind::Quote { callout, .. }) => callout.clone(),
                        _ => None,
                    };
                    Kind::Quote { callout, tag: None }
                }
            }
        } else if body.starts_with('|') {
            let cells = table_cells(body, body_start);
            let dashes = !cells.is_empty()
                && cells.iter().all(|c| {
                    let cell = text[c.clone()].trim_matches(':');
                    !cell.is_empty() && cell.bytes().all(|b| b == b'-')
                });
            if dashes {
                Kind::TableRule
            } else {
                Kind::TableRow { cells }
            }
        } else if let Some((len, numbered)) = list_marker(body) {
            marker.end += len;
            match checkbox(&body[len..]) {
                Some((done, len)) => {
                    let checkbox = marker.end..marker.end + 3;
                    marker.end += len;
                    Kind::Task { done, checkbox }
                }
                None if numbered => Kind::Numbered,
                None => Kind::Bullet,
            }
        } else {
            Kind::Paragraph
        };

        let spans = match kind {
            Kind::Blank | Kind::Rule | Kind::Fence { .. } | Kind::Code => Vec::new(),
            Kind::FrontmatterFence | Kind::Frontmatter | Kind::TableRule => Vec::new(),
            _ => inline(text, marker.end..range.end),
        };
        lines.push(Line { block: range.clone(), range, kind, marker, spans });
    }

    // Give every line of a block the byte range of the whole block.
    let mut from = 0;
    for i in 1..=lines.len() {
        if i == lines.len() || !same_block(&lines[i - 1].kind, &lines[i].kind) {
            let block = lines[from].range.start..lines[i - 1].range.end;
            for line in &mut lines[from..i] {
                line.block = block.clone();
            }
            from = i;
        }
    }
    lines
}

/// Does a line of kind `cur` continue the block that `prev` is part of?
fn same_block(prev: &Kind, cur: &Kind) -> bool {
    use Kind::*;
    match (prev, cur) {
        (Fence { open: true } | Code, Code | Fence { open: false }) => true,
        (FrontmatterFence | Frontmatter, Frontmatter | FrontmatterFence) => true,
        // A new `[!type]` line starts a new callout.
        (Quote { .. }, Quote { tag: None, .. }) => true,
        (TableRow { .. } | TableRule, TableRow { .. } | TableRule) => true,
        _ => false,
    }
}

/// The language named on a code block's opening fence line: `rust` for
/// ```` ```rust ````, empty if there is none. Anything after the first word
/// (some tools put options there) is ignored.
pub fn fence_language(fence_line: &str) -> &str {
    let info = fence_line.trim_start().trim_start_matches(['`', '~']).trim_start();
    info.split(|c: char| c.is_whitespace() || matches!(c, '{' | ',')).next().unwrap_or("")
}

/// Length of a code fence (``` or ~~~) if `body` starts with one. More
/// backticks after a backtick fence make the line inline code instead, as in
/// ```` ```a``` ````.
fn fence_len(body: &str) -> Option<usize> {
    let ch = *body.as_bytes().first()?;
    let run = body.bytes().take_while(|&b| b == ch).count();
    let fence = match ch {
        b'`' => !body[run..].contains('`'),
        b'~' => true,
        _ => false,
    };
    (fence && run >= 3).then_some(run)
}

/// Heading level and marker length (`## ` is 3). The space is needed:
/// `#word` is a tag, not a heading.
fn heading(body: &str) -> Option<(u8, usize)> {
    let hashes = body.bytes().take_while(|&b| b == b'#').count();
    let rest = &body[hashes..];
    if !(1..=6).contains(&hashes) || !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((hashes as u8, hashes + rest.len() - rest.trim_start().len()))
}

/// `[^id]: ` at the start of a line: the footnote's id and the marker's length.
fn footnote(body: &str) -> Option<(String, usize)> {
    let close = body.strip_prefix("[^")?.find("]:")?;
    let id = &body[2..2 + close];
    if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == '[') {
        return None;
    }
    let after = &body[2 + close + 2..];
    Some((id.to_string(), 2 + close + 2 + after.len() - after.trim_start().len()))
}

/// The embed that is all there is on a line, if the line is one: a picture
/// or a note that is shown as a block of its own, where an embed in the
/// middle of a sentence is not.
pub fn lone_embed<'a>(text: &str, line: &'a Line) -> Option<&'a Link> {
    if line.kind != Kind::Paragraph {
        return None;
    }
    let mut written = line.spans.iter().filter(|span| !text[span.range.clone()].trim().is_empty());
    let first = written.next()?;
    let embed = line.spans.iter().filter(|span| span.element == first.element).find_map(|span| match &span.link {
        Some(link @ Link::Embed { .. }) => Some(link),
        _ => None,
    })?;
    written.all(|span| span.element == first.element).then_some(embed)
}

fn is_rule(body: &str) -> bool {
    let mut chars = body.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else { return false };
    matches!(first, '-' | '*' | '_') && chars.clone().count() >= 2 && chars.all(|c| c == first)
}

/// Length of the `> ` (or nested `> > `) in front of a quote.
fn quote_marker(body: &str) -> usize {
    let b = body.as_bytes();
    let mut len = 0;
    while b.get(len) == Some(&b'>') {
        len += 1;
        if b.get(len) == Some(&b' ') {
            len += 1;
        }
    }
    len
}

/// `[!type] ` at the start of a quote: the callout type and the tag's length.
fn callout(rest: &str) -> Option<(String, usize)> {
    let inner = rest.strip_prefix("[!")?;
    let close = inner.find(']')?;
    let name = &inner[..close];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return None;
    }
    let mut len = 2 + close + 1;
    // `+` / `-` make a callout foldable; we only need to skip them.
    if matches!(rest.as_bytes().get(len), Some(b'+' | b'-')) {
        len += 1;
    }
    len += rest[len..].len() - rest[len..].trim_start().len();
    Some((name.to_lowercase(), len))
}

/// Length of a list marker with its space (`- `, `12. `) and whether it is numbered.
fn list_marker(body: &str) -> Option<(usize, bool)> {
    let b = body.as_bytes();
    let token = if matches!(b.first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !matches!(b.get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    matches!(b.get(token), Some(b' ' | b'\t')).then_some((token + 1, token > 1))
}

/// `[ ] ` or `[x] ` after a list marker: whether it is checked, and its length.
/// Any character between the brackets counts as checked.
fn checkbox(rest: &str) -> Option<(bool, usize)> {
    let b = rest.as_bytes();
    if b.len() < 3 || b[0] != b'[' || b[2] != b']' || !b[1].is_ascii() {
        return None;
    }
    match b.get(3) {
        None => Some((b[1] != b' ', 3)),
        Some(b' ') => Some((b[1] != b' ', 4)),
        Some(_) => None,
    }
}

/// Trimmed cell ranges of a `| a | b |` row.
fn table_cells(body: &str, offset: usize) -> Vec<Range<usize>> {
    let b = body.as_bytes();
    let mut bars: Vec<usize> = (0..b.len())
        .filter(|&i| b[i] == b'|' && (i == 0 || b[i - 1] != b'\\'))
        .collect();
    // A row may or may not end with a closing `|`.
    if body.trim_end().len() != bars.last().map_or(0, |last| last + 1) {
        bars.push(body.trim_end().len());
    }
    bars.windows(2)
        .map(|w| {
            let cell = &body[w[0] + 1..w[1]];
            let start = w[0] + 1 + cell.len() - cell.trim_start().len();
            offset + start..offset + start + cell.trim().len()
        })
        .collect()
}

/// Split `range` of `text` into formatted spans and syntax markers.
pub fn inline(text: &str, range: Range<usize>) -> Vec<Span> {
    let mut out = Vec::new();
    scan(text, range, &Context { style: Style::default(), link: None, element: None }, &mut out);
    out
}

/// What surrounds the text being scanned.
struct Context<'a> {
    style: Style,
    link: Option<&'a Link>,
    element: Option<&'a Range<usize>>,
}

fn scan(text: &str, range: Range<usize>, cx: &Context, out: &mut Vec<Span>) {
    let b = text.as_bytes();
    let plain = |r: Range<usize>, out: &mut Vec<Span>| {
        if !r.is_empty() {
            let element = cx.element.cloned().unwrap_or_else(|| r.clone());
            out.push(Span { range: r, element, marker: false, style: cx.style, link: cx.link.cloned() });
        }
    };
    let mut i = range.start;
    let mut pending = i;
    while i < range.end {
        let special = matches!(b[i], b'\\' | b'`' | b'%' | b'[' | b'#' | b'h' | b'*' | b'_' | b'~' | b'=' | b'<' | b'!');
        let mut found = Vec::new();
        match special.then(|| construct(text, i, &range, cx, &mut found)).flatten() {
            Some(next) => {
                plain(pending..i, out);
                out.append(&mut found);
                i = next;
                pending = next;
            }
            // Step over one whole character.
            None => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    plain(pending..range.end, out);
}

/// Try to read one inline construct starting at `i`; returns where it ends.
fn construct(
    text: &str,
    i: usize,
    range: &Range<usize>,
    cx: &Context,
    out: &mut Vec<Span>,
) -> Option<usize> {
    let b = text.as_bytes();
    let end = range.end;
    let rest = &text[i..end];
    let prev = text[range.start..i].chars().next_back();
    let after_space = prev.map_or(true, char::is_whitespace);
    let marker = |range: Range<usize>, element: &Range<usize>| Span {
        range,
        element: element.clone(),
        marker: true,
        style: cx.style,
        link: None,
    };
    let content = |range: Range<usize>, element: &Range<usize>, style: Style, link: Option<Link>| {
        Span { range, element: element.clone(), marker: false, style, link }
    };

    match b[i] {
        // `\*` is a literal `*`.
        b'\\' if rest.len() > 1 && b[i + 1].is_ascii_punctuation() => {
            let element = i..i + 2;
            out.push(marker(i..i + 1, &element));
            out.push(content(i + 1..i + 2, &element, cx.style, cx.link.cloned()));
            Some(element.end)
        }
        b'`' => {
            let ticks = rest.bytes().take_while(|&c| c == b'`').count();
            let close = find_backticks(rest, ticks)?;
            let element = i..i + close + ticks;
            let style = Style { code: true, ..cx.style };
            out.push(marker(i..i + ticks, &element));
            out.push(content(i + ticks..i + close, &element, style, cx.link.cloned()));
            out.push(marker(i + close..element.end, &element));
            Some(element.end)
        }
        b'%' if rest.starts_with("%%") => {
            let element = i..i + rest[2..].find("%%")? + 4;
            let style = Style { comment: true, ..cx.style };
            out.push(content(element.clone(), &element, style, None));
            Some(element.end)
        }
        // ![[picture.png]], ![[Note#Heading]], ![[picture.png|200]]
        b'!' if rest.starts_with("![[") && cx.link.is_none() => {
            let close = rest.find("]]")?;
            let inner = &rest[3..close];
            if inner.is_empty() || inner.contains('[') {
                return None;
            }
            let element = i..i + close + 2;
            let (target, option) = inner.split_once('|').map_or((inner, ""), |(target, option)| (target, option));
            let (target, place) = note_target(target);
            let link = Link::Embed { target, place, width: embed_width(option) };
            out.push(marker(i..i + 3, &element));
            out.push(content(i + 3..i + close, &element, cx.style, Some(link)));
            out.push(marker(i + close..element.end, &element));
            Some(element.end)
        }
        // ![alt](picture.png), ![alt|200](picture.png)
        b'!' if rest.starts_with("![") && cx.link.is_none() => {
            let close = rest.find("](")?;
            let alt = &rest[2..close];
            let len = rest[close + 2..].find(')')?;
            let source = i + close + 2..i + close + 2 + len;
            if alt.contains('[') || text[source.clone()].trim().is_empty() {
                return None;
            }
            let element = i..source.end + 1;
            let width = embed_width(alt.rsplit_once('|').map_or("", |(_, width)| width));
            let link = Link::Embed { target: text[source.clone()].trim().replace("%20", " "), place: None, width };
            // What shows while the syntax is hidden is the alt text, or
            // failing that the address.
            let shown = if alt.is_empty() { source } else { i + 2..i + close };
            out.push(marker(i..shown.start, &element));
            out.push(content(shown.clone(), &element, cx.style, Some(link)));
            out.push(marker(shown.end..element.end, &element));
            Some(element.end)
        }
        // [^1]: a reference to a footnote
        b'[' if rest.starts_with("[^") && cx.link.is_none() => {
            let close = rest.find(']')?;
            let id = &rest[2..close];
            if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == '[') {
                return None;
            }
            let element = i..i + close + 1;
            out.push(content(element.clone(), &element, cx.style, Some(Link::Footnote(id.to_string()))));
            Some(element.end)
        }
        // <mark>text</mark>, with or without a colour in its style
        b'<' if rest.starts_with("<mark") => {
            let open = rest.find('>')?;
            let attributes = &rest[5..open];
            // `<marker>` is some other tag.
            if !attributes.is_empty() && !attributes.starts_with(char::is_whitespace) {
                return None;
            }
            let close = open + 1 + rest[open + 1..].find("</mark>")?;
            let element = i..i + close + "</mark>".len();
            let style = Style { highlight: true, tint: mark_color(attributes).or(cx.style.tint), ..cx.style };
            out.push(marker(i..i + open + 1, &element));
            let inner = Context { style, link: cx.link, element: Some(&element) };
            scan(text, i + open + 1..i + close, &inner, out);
            out.push(marker(i + close..element.end, &element));
            Some(element.end)
        }
        // [[Note]], [[Note|shown text]], [[Note#Heading]]
        b'[' if rest.starts_with("[[") && cx.link.is_none() => {
            let close = rest.find("]]")?;
            let inner = &rest[2..close];
            if inner.is_empty() || inner.contains('[') {
                return None;
            }
            let element = i..i + close + 2;
            let (target, shown) = match inner.find('|') {
                Some(bar) => (&inner[..bar], i + 2 + bar + 1),
                None => (inner, i + 2),
            };
            let (note, place) = note_target(target);
            out.push(marker(i..shown, &element));
            out.push(content(shown..i + close, &element, cx.style, Some(Link::Note(note, place))));
            out.push(marker(i + close..element.end, &element));
            Some(element.end)
        }
        // [text](target)
        b'[' if cx.link.is_none() => {
            let close = rest.find("](")?;
            if rest[1..close].contains('[') {
                return None;
            }
            let target_len = rest[close + 2..].find(')')?;
            let target = rest[close + 2..close + 2 + target_len].trim();
            if target.is_empty() {
                return None;
            }
            let link = if target.contains("://") || target.starts_with("mailto:") {
                Link::Url(target.to_string())
            } else {
                let (note, place) = note_target(&target.replace("%20", " "));
                Link::Note(note.trim_end_matches(".md").to_string(), place)
            };
            let element = i..i + close + 2 + target_len + 1;
            out.push(marker(i..i + 1, &element));
            let inner = Context { style: cx.style, link: Some(&link), element: Some(&element) };
            scan(text, i + 1..i + close, &inner, out);
            out.push(marker(i + close..element.end, &element));
            Some(element.end)
        }
        b'#' if after_space && cx.link.is_none() => {
            let name: String = rest[1..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
                .collect();
            // A tag needs at least one character that is not a digit.
            if name.is_empty() || name.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let element = i..i + 1 + name.len();
            out.push(content(element.clone(), &element, cx.style, Some(Link::Tag(name))));
            Some(element.end)
        }
        b'h' if after_space && cx.link.is_none() => {
            let scheme = ["https://", "http://"].into_iter().find(|s| rest.starts_with(*s))?;
            let url = rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())]
                .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '\'']);
            if url.len() == scheme.len() {
                return None;
            }
            let element = i..i + url.len();
            out.push(content(element.clone(), &element, cx.style, Some(Link::Url(url.to_string()))));
            Some(element.end)
        }
        ch @ (b'*' | b'_' | b'~' | b'=') => {
            let run = rest.bytes().take_while(|&c| c == ch).count();
            let len = delimiter_len(ch, run)?;
            // `snake_case` is not emphasis.
            if ch == b'_' && prev.is_some_and(char::is_alphanumeric) {
                return None;
            }
            let close = find_close(text, i + len, end, ch, len, false)?;
            let element = i..close + len;
            let mut style = cx.style;
            match ch {
                b'~' => style.strike = true,
                b'=' => style.highlight = true,
                _ => {
                    style.italic |= len != 2;
                    style.bold |= len >= 2;
                }
            }
            out.push(marker(i..i + len, &element));
            let inner = Context { style, link: cx.link, element: Some(&element) };
            scan(text, i + len..close, &inner, out);
            out.push(marker(close..element.end, &element));
            Some(element.end)
        }
        _ => None,
    }
}

/// The width asked of an embed: the number in `200`, or the first in `200x100`.
fn embed_width(option: &str) -> Option<u32> {
    let digits = option.trim().split('x').next()?;
    digits.parse().ok().filter(|width| *width > 0)
}

/// The colour a `<mark>` tag asks for: the `background` (or
/// `background-color`) of its `style`, when that is a hex colour. Named
/// colours and `rgb()` are not read, and such a mark is the usual yellow.
fn mark_color(attributes: &str) -> Option<[u8; 4]> {
    let style = &attributes[attributes.find("background")?..];
    let value = style[style.find(':')? + 1..].trim_start();
    let hex = value.strip_prefix('#')?;
    let hex = &hex[..hex.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(hex.len())];
    let pair = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    // `#f80` is short for `#ff8800`.
    let single = |at: usize| u8::from_str_radix(&hex[at..at + 1], 16).ok().map(|digit| digit * 17);
    match hex.len() {
        3 => Some([single(0)?, single(1)?, single(2)?, 255]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

/// The note a link names, and the place in it after the first `#`, if any.
fn note_target(target: &str) -> (String, Option<Place>) {
    let (note, place) = target.split_once('#').unwrap_or((target, ""));
    let place = match place.trim().strip_prefix('^') {
        Some(id) => Some(Place::Block(id.trim().to_string())),
        None => Some(Place::Heading(place.trim().to_string())),
    };
    (note.trim().to_string(), place.filter(|place| !matches!(place, Place::Heading(name) | Place::Block(name) if name.is_empty())))
}

/// How many characters of a run of `ch` form one delimiter:
/// `*`/`_` italic, `**`/`__` bold, `***` both, `~~` strikethrough, `==` highlight.
fn delimiter_len(ch: u8, run: usize) -> Option<usize> {
    match ch {
        b'*' | b'_' => Some(run.min(3)),
        _ => (run >= 2).then_some(2),
    }
}

/// Offset in `rest` of the next run of exactly `n` backticks after the opening one.
fn find_backticks(rest: &str, n: usize) -> Option<usize> {
    let b = rest.as_bytes();
    let mut j = n;
    while j < b.len() {
        if b[j] != b'`' {
            j += 1;
            continue;
        }
        let run = b[j..].iter().take_while(|&&c| c == b'`').count();
        if run == n {
            return Some(j);
        }
        j += run;
    }
    None
}

/// Where the emphasis whose opening delimiter ends at `from` closes: the
/// offset of its closing delimiter. A closing run can be longer than the
/// delimiter (`**a *b***` ends two spans at once); `first` picks which end of
/// the run this span takes.
fn find_close(text: &str, from: usize, end: usize, ch: u8, len: usize, first: bool) -> Option<usize> {
    let b = text.as_bytes();
    // An opening delimiter hugs the text it formats.
    if from >= end || b[from].is_ascii_whitespace() || b[from] == ch {
        return None;
    }
    let mut j = from;
    while j < end {
        if b[j] == b'\\' {
            j += 2;
            continue;
        }
        if b[j] == b'`' {
            // Delimiters inside inline code do not count.
            let ticks = b[j..end].iter().take_while(|&&c| c == b'`').count();
            j += find_backticks(&text[j..end], ticks).map_or(ticks, |close| close + ticks);
            continue;
        }
        if b[j] != ch {
            j += 1;
            continue;
        }
        let run = b[j..end].iter().take_while(|&&c| c == ch).count();
        let after = j + run;
        if b[j - 1].is_ascii_whitespace() {
            // Whitespace in front makes this run the start of a nested span: skip it whole.
            j = delimiter_len(ch, run)
                .and_then(|nested| Some(find_close(text, after, end, ch, nested, true)? + nested))
                .unwrap_or(after);
            continue;
        }
        let inside_word = ch == b'_' && text[after..end].chars().next().is_some_and(char::is_alphanumeric);
        if run >= len && !inside_word {
            return Some(if first { j } else { after - len });
        }
        j = after;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Reading view would show: content without syntax markers.
    fn visible(text: &str) -> String {
        inline(text, 0..text.len())
            .iter()
            .filter(|s| !s.marker)
            .map(|s| &text[s.range.clone()])
            .collect()
    }

    fn span<'a>(spans: &'a [Span], text: &str, word: &str) -> &'a Span {
        spans
            .iter()
            .find(|s| !s.marker && &text[s.range.clone()] == word)
            .unwrap_or_else(|| panic!("no span '{word}' in {spans:#?}"))
    }

    fn kinds(text: &str) -> Vec<Kind> {
        parse(text).into_iter().map(|l| l.kind).collect()
    }

    #[test]
    fn markers_are_separated_from_content() {
        assert_eq!(visible("**a** *b* ~~c~~ ==d== `e` __f__ _g_"), "a b c d e f g");
        assert_eq!(visible("plain text"), "plain text");
    }

    #[test]
    fn styles_are_recorded_and_nest() {
        let text = "**a _b_** ~~c~~ ==d== `e`";
        let spans = inline(text, 0..text.len());
        let a = span(&spans, text, "a ");
        assert!(a.style.bold && !a.style.italic);
        assert_eq!(a.element, 0..9, "text inside ** belongs to the bold element");
        let b = span(&spans, text, "b");
        assert!(b.style.bold && b.style.italic);
        assert!(span(&spans, text, "c").style.strike);
        assert!(span(&spans, text, "d").style.highlight);
        assert!(span(&spans, text, "e").style.code);
    }

    #[test]
    fn triple_delimiters_close_two_spans() {
        for text in ["**a *b***", "*a **b***", "***b***"] {
            let spans = inline(text, 0..text.len());
            let b = span(&spans, text, "b");
            assert!(b.style.bold && b.style.italic, "{text}");
            assert!(!visible(text).contains('*'), "{text}");
        }
    }

    #[test]
    fn things_that_only_look_like_emphasis_stay_plain() {
        for text in ["some_var_name", "2 * 3 * 4", "**unclosed", "a = b == c", "~tilde~", "====="] {
            let spans = inline(text, 0..text.len());
            assert!(spans.iter().all(|s| !s.marker && s.style == Style::default()), "{text}");
            assert_eq!(visible(text), text);
        }
    }

    #[test]
    fn code_and_escapes_protect_delimiters() {
        let text = "`**x**` and \\*y\\*";
        assert_eq!(visible(text), "**x** and *y*");
        let spans = inline(text, 0..text.len());
        assert!(spans.iter().all(|s| !s.style.bold && !s.style.italic));
        assert_eq!(visible("*a `*` b*"), "a * b");
    }

    #[test]
    fn wikilinks_show_alias_and_point_at_the_note() {
        let text = "[[Note|alias]] [[Other#Part]] [[#here]]";
        assert_eq!(visible(text), "alias Other#Part #here");
        let spans = inline(text, 0..text.len());
        assert_eq!(span(&spans, text, "alias").link, Some(Link::Note("Note".into(), None)));
        assert_eq!(span(&spans, text, "alias").element, 0..14);
        let heading = |name: &str| Some(Place::Heading(name.into()));
        assert_eq!(span(&spans, text, "Other#Part").link, Some(Link::Note("Other".into(), heading("Part"))));
        assert_eq!(span(&spans, text, "#here").link, Some(Link::Note(String::new(), heading("here"))));
    }

    #[test]
    fn links_name_the_place_in_the_note_they_point_at() {
        let text = "[[Note#One#Two|x]] [[Note#^b-1]] [[#^here]] [[Note#]] [n](My%20Note.md#A%20part)";
        let spans = inline(text, 0..text.len());
        let links: Vec<_> = spans.iter().filter_map(|s| s.link.clone()).collect();
        let note = |name: &str, place| Link::Note(name.into(), place);
        assert_eq!(links[0], note("Note", Some(Place::Heading("One#Two".into()))));
        assert_eq!(links[1], note("Note", Some(Place::Block("b-1".into()))));
        assert_eq!(links[2], note("", Some(Place::Block("here".into()))));
        assert_eq!(links[3], note("Note", None), "nothing after the # is no place");
        assert_eq!(links[4], note("My Note", Some(Place::Heading("A part".into()))));
    }

    #[test]
    fn markdown_links_and_bare_urls() {
        let text = "[**site**](https://a.b/c) [n](My%20Note.md) see https://x.y/z.";
        assert_eq!(visible(text), "site n see https://x.y/z.");
        let spans = inline(text, 0..text.len());
        let site = span(&spans, text, "site");
        assert!(site.style.bold);
        assert_eq!(site.link, Some(Link::Url("https://a.b/c".into())));
        assert_eq!(span(&spans, text, "n").link, Some(Link::Note("My Note".into(), None)));
        assert_eq!(span(&spans, text, "https://x.y/z").link, Some(Link::Url("https://x.y/z".into())));
    }

    #[test]
    fn embeds_name_what_is_to_be_shown_in_their_place() {
        let text = "![[pic.png]] ![[Note#Part|shown]] ![[pic.png|200x100]] ![alt|64](a%20b.jpg) ![](c.gif) !plain ![[]]";
        assert_eq!(visible(text), "pic.png Note#Part|shown pic.png|200x100 alt|64 c.gif !plain ![[]]");
        let spans = inline(text, 0..text.len());
        let embeds: Vec<_> = spans.iter().filter_map(|s| s.link.clone()).collect();
        let embed = |target: &str, place, width| Link::Embed { target: target.into(), place, width };
        assert_eq!(embeds[0], embed("pic.png", None, None));
        assert_eq!(embeds[1], embed("Note", Some(Place::Heading("Part".into())), None));
        assert_eq!(embeds[2], embed("pic.png", None, Some(200)));
        assert_eq!(embeds[3], embed("a b.jpg", None, Some(64)));
        assert_eq!(embeds[4], embed("c.gif", None, None));
        assert_eq!(embeds.len(), 5);
    }

    #[test]
    fn an_embed_alone_on_its_line_is_a_block() {
        let alone = |text: &str| lone_embed(text, &parse(text)[0]).is_some();
        assert!(alone("![[pic.png]]") && alone("  ![[Note#Part]]  ") && alone("![a **b**](pic.png)"));
        assert!(!alone("see ![[pic.png]]") && !alone("![[a.png]] ![[b.png]]") && !alone("- ![[pic.png]]") && !alone("[[Note]]"));
    }

    #[test]
    fn footnotes_have_references_and_lines_that_define_them() {
        let text = "Said[^1] and done[^note-2], not [^ x] or [^].\n\n[^1]: The **first**.\n[^note-2]:no space\n[^bad id]: x";
        let lines = parse(text);
        let refs: Vec<_> = lines[0].spans.iter().filter_map(|s| s.link.clone()).collect();
        assert_eq!(refs, [Link::Footnote("1".into()), Link::Footnote("note-2".into())]);
        assert_eq!(lines[2].kind, Kind::Footnote("1".into()));
        assert_eq!(&text[lines[2].marker.clone()], "[^1]: ");
        assert!(lines[2].spans.iter().any(|s| s.style.bold));
        assert_eq!(lines[3].kind, Kind::Footnote("note-2".into()));
        assert_eq!(lines[4].kind, Kind::Paragraph);
    }

    #[test]
    fn a_mark_tag_is_a_highlight_in_the_colour_it_names() {
        let text = r##"a <mark style="background: #FF8800a6;">hot **bold**</mark> <mark>plain</mark> <mark style="background-color:#0f0">x</mark>"##;
        assert_eq!(visible(text), "a hot bold plain x");
        let spans = inline(text, 0..text.len());
        let hot = span(&spans, text, "hot ");
        assert!(hot.style.highlight);
        assert_eq!(hot.style.tint, Some([255, 136, 0, 166]));
        let bold = span(&spans, text, "bold");
        assert!(bold.style.bold && bold.style.tint == hot.style.tint, "the colour goes on through what is inside");
        assert_eq!((span(&spans, text, "plain").style.highlight, span(&spans, text, "plain").style.tint), (true, None));
        assert_eq!(span(&spans, text, "x").style.tint, Some([0, 255, 0, 255]));
        assert!(!span(&spans, text, "a ").style.highlight);
    }

    #[test]
    fn other_tags_and_unclosed_marks_stay_as_written() {
        for text in ["<marker>x</marker>", "<mark>never closed", "a < b", "<mark style=\"background: red\">x"] {
            assert_eq!(visible(text), text);
        }
        let named = r#"<mark style="background: red;">x</mark>"#;
        let spans = inline(named, 0..named.len());
        assert_eq!((span(&spans, named, "x").style.highlight, span(&spans, named, "x").style.tint), (true, None), "a colour by name is the usual yellow");
    }

    #[test]
    fn tags_need_a_space_before_and_a_non_digit() {
        let text = "#tag a#b #123 #2024-plans #nested/tag";
        let found: Vec<_> = inline(text, 0..text.len()).into_iter().filter_map(|s| s.link).collect();
        let tag = |t: &str| Link::Tag(t.into());
        assert_eq!(found, [tag("tag"), tag("2024-plans"), tag("nested/tag")]);
    }

    #[test]
    fn comments_are_marked() {
        let text = "a %%hidden%% b";
        let spans = inline(text, 0..text.len());
        assert!(span(&spans, text, "%%hidden%%").style.comment);
    }

    #[test]
    fn lines_are_classified() {
        let text = "---\ntags: [x]\n---\n# H1\n###### H6\n#tag\n\n- a\n  * b\n1. c\n- [ ] d\n- [x] e\n\
                    > q\n---\n```rust\n# not a heading\n```\n| a | b |\n|---|:-:|\n| 1 | 2 |\nend";
        use Kind::*;
        let got = kinds(text);
        let expected = [
            FrontmatterFence,
            Frontmatter,
            FrontmatterFence,
            Heading(1),
            Heading(6),
            Paragraph,
            Blank,
            Bullet,
            Bullet,
            Numbered,
            Task { done: false, checkbox: 0..0 },
            Task { done: true, checkbox: 0..0 },
            Quote { callout: None, tag: None },
            Rule,
            Fence { open: true },
            Code,
            Fence { open: false },
            TableRow { cells: vec![] },
            TableRule,
            TableRow { cells: vec![] },
            Paragraph,
        ];
        assert_eq!(got.len(), expected.len(), "{got:#?}");
        for (got, expected) in got.iter().zip(&expected) {
            assert_eq!(std::mem::discriminant(got), std::mem::discriminant(expected), "{got:?}");
        }
        assert_eq!(got[3], Heading(1));
        assert_eq!(got[4], Heading(6));
        assert!(matches!(got[10], Task { done: false, .. }) && matches!(got[11], Task { done: true, .. }));
    }

    #[test]
    fn block_markers_are_split_from_content() {
        let text = "## Title\n  - [x] done\n> > deep\n12. twelve";
        let lines = parse(text);
        let marker = |i: usize| &text[lines[i].marker.clone()];
        assert_eq!(marker(0), "## ");
        assert_eq!(marker(1), "- [x] ");
        assert_eq!(marker(2), "> > ");
        assert_eq!(marker(3), "12. ");
        let Kind::Task { checkbox, .. } = &lines[1].kind else { panic!() };
        assert_eq!(&text[checkbox.clone()], "[x]");
    }

    #[test]
    fn a_dashed_first_line_is_a_rule_unless_frontmatter_is_closed() {
        assert_eq!(kinds("---\ntext"), [Kind::Rule, Kind::Paragraph]);
        assert_eq!(kinds("---\n---")[0], Kind::FrontmatterFence);
    }

    #[test]
    fn callouts_carry_their_type_through_the_block() {
        let text = "> [!Warning]- Careful\n> body\n> [!note]\n> second\n\n> plain";
        let lines = parse(text);
        let Kind::Quote { callout, tag } = &lines[0].kind else { panic!() };
        assert_eq!(callout.as_deref(), Some("warning"));
        assert_eq!(&text[tag.clone().unwrap()], "[!Warning]- ");
        assert_eq!(&text[lines[0].marker.end..lines[0].range.end], "Careful");
        assert!(matches!(&lines[1].kind, Kind::Quote { callout: Some(c), tag: None } if c == "warning"));
        assert!(matches!(&lines[3].kind, Kind::Quote { callout: Some(c), tag: None } if c == "note"));
        assert!(matches!(&lines[5].kind, Kind::Quote { callout: None, tag: None }));
        // Two callouts in a row are two blocks.
        assert_eq!(lines[0].block, lines[1].block);
        assert_eq!(lines[2].block, lines[3].block);
        assert_ne!(lines[1].block, lines[2].block);
    }

    #[test]
    fn code_blocks_group_their_lines_and_stay_literal() {
        let text = "a\n```\n**x**\n```\n```\ny";
        let lines = parse(text);
        assert_eq!(&text[lines[1].block.clone()], "```\n**x**\n```");
        assert_eq!(lines[2].block, lines[1].block);
        assert!(lines[2].spans.is_empty());
        // An unclosed block runs to the end of the note.
        assert_eq!(&text[lines[4].block.clone()], "```\ny");
        assert_eq!(lines[0].block, lines[0].range);
    }

    #[test]
    fn inline_code_at_the_start_of_a_line_is_not_a_fence() {
        let text = "```a``` b\nc";
        assert_eq!(kinds(text), [Kind::Paragraph, Kind::Paragraph]);
        assert!(span(&parse(text)[0].spans, text, "a").style.code);
        // Only a backtick fence minds backticks after it.
        assert_eq!(kinds("~~~ `a`\nc"), [Kind::Fence { open: true }, Kind::Code]);
    }

    #[test]
    fn the_fence_names_the_language() {
        assert_eq!(fence_language("```rust"), "rust");
        assert_eq!(fence_language("  ~~~~ C++ title=\"x\""), "C++");
        assert_eq!(fence_language("```js{1,3}"), "js");
        assert_eq!(fence_language("```"), "");
    }

    #[test]
    fn table_cells_are_trimmed() {
        let text = "| a | **b** |\n| x | y";
        let lines = parse(text);
        let cells = |i: usize| -> Vec<&str> {
            let Kind::TableRow { cells } = &lines[i].kind else { panic!() };
            cells.iter().map(|c| &text[c.clone()]).collect()
        };
        assert_eq!(cells(0), ["a", "**b**"]);
        assert_eq!(cells(1), ["x", "y"]);
    }

    #[test]
    fn spans_cover_the_content_without_gaps() {
        let text = "# **T** [[a|b]]\n- [ ] ~~x~~ #t `c` \\* [l](u) https://e.f ==h== é *ü*\n> [!tip] _q_\r\n\n| a | *b* |";
        for line in parse(text) {
            let mut at = line.marker.end;
            for s in &line.spans {
                assert_eq!(s.range.start, at, "gap in {:?}", &text[line.range.clone()]);
                assert!(s.element.start <= s.range.start && s.range.end <= s.element.end);
                at = s.range.end;
            }
            if !line.spans.is_empty() {
                assert_eq!(at, line.range.end);
            }
            assert!(!text[line.range.clone()].contains(['\r', '\n']));
        }
    }
}
