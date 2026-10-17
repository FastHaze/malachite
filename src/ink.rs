//! Drawings over a note: strokes of a pen or a highlighter, lines, arrows,
//! rectangles, ellipses, text and numbered badges, made in Reading view and
//! kept beside the note, in a file of their own under the vault's
//! `.malachite/ink` folder (`Vault::ink_file`), so that the note itself stays
//! plain Markdown for any other program.
//!
//! A note is not a picture: its lines move when text above them is written or
//! taken away. So a mark is drawn from a line of the note, its *anchor*, and
//! its points are offsets from where that line is drawn: from the left of the
//! column of text and the top of the line. The anchor is kept as the line's
//! text, as folds are, and which of the lines with that text it is (`nth`,
//! counting from 0), so that edits elsewhere need no bookkeeping. A mark whose
//! line can no longer be found is not drawn, but it is kept in the file all the
//! same: rewording a line back brings its drawing back.
//!
//! Every kind of mark is a `Stroke`: what it was made with, a colour, a width
//! and its points, which say what each kind needs (`Tool`). The file is lines
//! of text, one mark to a line under the `anchor` line it is drawn from, with
//! the text of a text or a badge after a `"`:
//!
//! ```text
//! anchor 0 ## A heading
//! pen #e93147ff 2 10,4 12.5,6.1 15,9
//! highlighter #ffd00060 10 0,8 120,8
//! arrow #e93147ff 4 0,0 80,20 30,-10
//! text #e93147ff 20 10,30 "Two\nlines
//! ```
//!
//! Reading it never fails: a line that is not understood is skipped, so a
//! kind of mark that a later version adds is passed over by this one.
//!
//! A pen's or a highlighter's points are what the pointer went through. They
//! are drawn smoothed (`path`): every five points become a cubic curve whose
//! end is moved to the middle of its neighbours, so one curve runs into the
//! next without a corner. Shapes are made from a drag (`shape`), as ScreenInk
//! makes them: Shift keeps a line to steps of 45° and a rectangle or ellipse
//! square. A line or an arrow is curved by a third point it bends towards,
//! put there by its middle handle (`move_handle`).

/// What a mark was made with, and so what its points and width are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    /// The points the pointer went through; the width is the line's.
    Pen,
    /// The same, wide and seen through, to go over text.
    Highlighter,
    /// From the first point to the second.
    Line,
    /// From the first point to the second, where its head is, bending towards
    /// a third if there is one.
    Arrow,
    /// Between two opposite corners.
    Rectangle,
    /// In the box between two opposite corners.
    Ellipse,
    /// The text, from its top left at the point; the width is its size.
    Text,
    /// A disc with the text in it, centred on the point; the width is its radius.
    Badge,
}

impl Tool {
    const ALL: [Tool; 8] = [Tool::Pen, Tool::Highlighter, Tool::Line, Tool::Arrow, Tool::Rectangle, Tool::Ellipse, Tool::Text, Tool::Badge];

    fn word(self) -> &'static str {
        match self {
            Tool::Pen => "pen",
            Tool::Highlighter => "highlighter",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Text => "text",
            Tool::Badge => "badge",
        }
    }

    /// Whether marks of this kind say something in words.
    pub fn has_text(self) -> bool {
        matches!(self, Tool::Text | Tool::Badge)
    }
}

/// One mark over the note, of any kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// The text of the line the mark is drawn from.
    pub anchor: String,
    /// Which of the lines with that text, counting from 0.
    pub nth: usize,
    pub tool: Tool,
    /// Red, green, blue and opacity.
    pub color: [u8; 4],
    pub width: f32,
    /// From the left of the text and the top of the line.
    pub points: Vec<[f32; 2]>,
    /// What a text says, or a badge's number; empty for the other kinds.
    pub text: String,
}

/// The note's lines: where each starts, and its text without the line break.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = 0;
    text.split('\n').map(move |line| {
        let at = start;
        start += line.len() + 1;
        (at, line.strip_suffix('\r').unwrap_or(line))
    })
}

/// The anchor of the line of `text` that byte `at` is in: its text, and how
/// many lines before it are the same (`same_line`).
pub fn anchor_at(text: &str, at: usize) -> (String, usize) {
    let (start, line) = lines(text).take_while(|(start, _)| *start <= at).last().unwrap_or((0, ""));
    let key = unticked(line);
    let nth = lines(text).take_while(|(before, _)| *before < start).filter(|(_, other)| unticked(other) == key).count();
    (line.to_owned(), nth)
}

/// Where in `text` the line a stroke is anchored to starts, if it is still there.
pub fn line_of(text: &str, anchor: &str, nth: usize) -> Option<usize> {
    let key = unticked(anchor);
    lines(text).filter(|(_, line)| unticked(line) == key).nth(nth).map(|(at, _)| at)
}

/// A line with its task's box, if it has one, left empty: ticking a task
/// changes its line, but it is the same line to draw on.
fn unticked(line: &str) -> std::borrow::Cow<'_, str> {
    let body = line.trim_start();
    let indent = line.len() - body.len();
    let marker = if body.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !body[digits..].starts_with(['.', ')']) {
            return line.into();
        }
        digits + 1
    };
    let Some(inside) = body[marker..].strip_prefix(" [") else { return line.into() };
    let mut chars = inside.chars();
    match (chars.next(), chars.next()) {
        (Some(state), Some(']')) if state != ' ' => format!("{}{} [ ]{}", &line[..indent], &body[..marker], &inside[state.len_utf8() + 1..]).into(),
        _ => line.into(),
    }
}

/// The strokes written in `file`.
pub fn read(file: &str) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    let mut anchor = None;
    for line in file.lines() {
        if let Some(rest) = line.strip_prefix("anchor ") {
            anchor = rest.split_once(' ').and_then(|(nth, text)| Some((text.to_owned(), nth.parse().ok()?)));
            continue;
        }
        let Some((text, nth)) = &anchor else { continue };
        if let Some(stroke) = stroke(line, text, *nth) {
            strokes.push(stroke);
        }
    }
    strokes
}

/// One stroke's line of the file, under the anchor `text`, `nth`.
fn stroke(line: &str, text: &str, nth: usize) -> Option<Stroke> {
    let (line, said) = match line.split_once('"') {
        Some((line, said)) => (line, unescape(said)),
        None => (line, String::new()),
    };
    let mut words = line.split_whitespace();
    let word = words.next()?;
    let tool = Tool::ALL.into_iter().find(|tool| tool.word() == word)?;
    let hex = words.next()?.strip_prefix('#').filter(|hex| hex.len() == 8 && hex.is_ascii())?;
    let mut color = [0; 4];
    for (i, channel) in color.iter_mut().enumerate() {
        *channel = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    let width = words.next()?.parse().ok().filter(|width: &f32| width.is_finite() && *width > 0.0)?;
    let point = |word: &str| {
        let (x, y) = word.split_once(',')?;
        let (x, y): (f32, f32) = (x.parse().ok()?, y.parse().ok()?);
        (x.is_finite() && y.is_finite()).then_some([x, y])
    };
    let points = words.map(point).collect::<Option<Vec<_>>>()?;
    let enough = match tool {
        Tool::Pen | Tool::Highlighter | Tool::Text | Tool::Badge => 1,
        Tool::Line | Tool::Arrow | Tool::Rectangle | Tool::Ellipse => 2,
    };
    if points.len() < enough || (tool.has_text() && said.is_empty()) {
        return None;
    }
    Some(Stroke { anchor: text.to_owned(), nth, tool, color, width, points, text: said })
}

/// The file that keeps `strokes`.
pub fn write(strokes: &[Stroke]) -> String {
    let mut file = String::from("# Drawings over the note this file is named for, as Malachite keeps them.\n");
    let mut last: Option<(&str, usize)> = None;
    for stroke in strokes {
        if last != Some((&stroke.anchor, stroke.nth)) {
            file.push_str(&format!("anchor {} {}\n", stroke.nth, stroke.anchor));
            last = Some((&stroke.anchor, stroke.nth));
        }
        let [r, g, b, a] = stroke.color;
        file.push_str(&format!("{} #{r:02x}{g:02x}{b:02x}{a:02x} {}", stroke.tool.word(), tenths(stroke.width)));
        for [x, y] in &stroke.points {
            file.push_str(&format!(" {},{}", tenths(*x), tenths(*y)));
        }
        if !stroke.text.is_empty() {
            file.push_str(" \"");
            file.push_str(&escape(&stroke.text));
        }
        file.push('\n');
    }
    file
}

/// A text on one line of the file: line breaks as `\n`, and so `\` as `\\`.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

/// A number to a tenth of a point, which is finer than any pointer.
fn tenths(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// The points a shape made with `tool` by a drag from `start` to `end` is
/// kept as, or `None` if the drag is too short to mean one. `constrain`
/// (Shift) keeps a line or an arrow to steps of 45° and a rectangle or an
/// ellipse square. A line or an arrow is made straight, and bent afterwards
/// (`move_handle`).
pub fn shape(tool: Tool, start: [f32; 2], end: [f32; 2], constrain: bool) -> Option<Vec<[f32; 2]>> {
    match tool {
        Tool::Line | Tool::Arrow => {
            let end = if constrain { snap_angle(start, end) } else { end };
            if distance(start, end) < 3.0 {
                return None;
            }
            Some(vec![start, end])
        }
        Tool::Rectangle | Tool::Ellipse => {
            let end = if constrain {
                let side = (end[0] - start[0]).abs().max((end[1] - start[1]).abs());
                [start[0] + side.copysign(end[0] - start[0]), start[1] + side.copysign(end[1] - start[1])]
            } else {
                end
            };
            ((end[0] - start[0]).abs() >= 3.0 || (end[1] - start[1]).abs() >= 3.0).then(|| vec![start, end])
        }
        _ => None,
    }
}

/// `end` turned about `start` to the nearest step of 45°, as far away.
fn snap_angle(start: [f32; 2], end: [f32; 2]) -> [f32; 2] {
    let step = std::f32::consts::FRAC_PI_4;
    let angle = ((end[1] - start[1]).atan2(end[0] - start[0]) / step).round() * step;
    let length = distance(start, end);
    // Straight across or up is exactly so, not a hair off.
    let exact = |v: f32| if v.abs() < 1e-4 { 0.0 } else { v };
    [start[0] + exact(length * angle.cos()), start[1] + exact(length * angle.sin())]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
}

/// The line a mark is drawn along, from its anchor: a pen's or highlighter's
/// points smoothed, a line's ends, an arrow's shaft, a rectangle's or an
/// ellipse's outline going round. A text or a badge is its point.
pub fn outline(stroke: &Stroke) -> Vec<[f32; 2]> {
    let points = &stroke.points;
    match stroke.tool {
        Tool::Pen | Tool::Highlighter => path(points),
        Tool::Line => match points.get(2) {
            Some(&bend) => quadratic(points[0], bend, points[1]),
            None => points[..2].to_vec(),
        },
        Tool::Arrow => shaft(points, stroke.width),
        Tool::Rectangle => {
            let ([x0, y0], [x1, y1]) = (points[0], points[1]);
            vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]
        }
        Tool::Ellipse => {
            let centre = middle(points[0], points[1]);
            let (rx, ry) = ((points[1][0] - points[0][0]).abs() / 2.0, (points[1][1] - points[0][1]).abs() / 2.0);
            (0..=48)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / 48.0;
                    [centre[0] + rx * angle.cos(), centre[1] + ry * angle.sin()]
                })
                .collect()
        }
        Tool::Text | Tool::Badge => points[..1].to_vec(),
    }
}

/// How long an arrow's head is, for a line of `width`.
fn head_length(width: f32) -> f32 {
    (width * 4.0).max(14.0)
}

/// An arrow's head: its tip, and the two corners behind it.
pub fn arrow_head(stroke: &Stroke) -> Option<[[f32; 2]; 3]> {
    if stroke.tool != Tool::Arrow {
        return None;
    }
    let (tip, direction) = arrival(&stroke.points);
    let length = head_length(stroke.width);
    let base = [tip[0] - direction[0] * length, tip[1] - direction[1] * length];
    let (nx, ny) = (-direction[1] * length * 0.55, direction[0] * length * 0.55);
    Some([tip, [base[0] + nx, base[1] + ny], [base[0] - nx, base[1] - ny]])
}

/// Where an arrow ends, and which way it is going as it gets there.
fn arrival(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    let tip = points[1];
    let from = points.get(2).copied().unwrap_or(points[0]);
    let length = distance(from, tip);
    let direction = if length > 0.0 { [(tip[0] - from[0]) / length, (tip[1] - from[1]) / length] } else { [1.0, 0.0] };
    (tip, direction)
}

/// An arrow's shaft, which ends inside its head so that the line's round end
/// does not poke out past the head's sides; none if the arrow is all head.
fn shaft(points: &[[f32; 2]], width: f32) -> Vec<[f32; 2]> {
    let (tip, direction) = arrival(points);
    let length = head_length(width);
    if distance(points[0], tip) <= length {
        return Vec::new();
    }
    let end = [tip[0] - direction[0] * length * 0.6, tip[1] - direction[1] * length * 0.6];
    match points.get(2) {
        None => vec![points[0], end],
        Some(&bend) => quadratic(points[0], bend, end),
    }
}

/// Points along the curve from `start` to `end` that bends towards `bend`.
fn quadratic(start: [f32; 2], bend: [f32; 2], end: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let at = |k: usize| u * u * start[k] + 2.0 * u * t * bend[k] + t * t * end[k];
            [at(0), at(1)]
        })
        .collect()
}

/// What a line or an arrow can be held by to change it: its start, its end,
/// and the middle of it, which is on the curve, not at the point it bends to.
pub fn handles(stroke: &Stroke) -> Option<[[f32; 2]; 3]> {
    if !matches!(stroke.tool, Tool::Line | Tool::Arrow) {
        return None;
    }
    let (a, b) = (stroke.points[0], stroke.points[1]);
    let middle = match stroke.points.get(2) {
        Some(c) => [0.25 * a[0] + 0.5 * c[0] + 0.25 * b[0], 0.25 * a[1] + 0.5 * c[1] + 0.25 * b[1]],
        None => middle(a, b),
    };
    Some([a, b, middle])
}

/// Move a line's or an arrow's handle `which` (as `handles` gives them) to
/// `to`. The middle one bends it so that its curve goes through `to`, and
/// brought back to halfway between the ends, straightens it.
pub fn move_handle(stroke: &mut Stroke, which: usize, to: [f32; 2]) {
    match which {
        0 | 1 => stroke.points[which] = to,
        _ => {
            let (a, b) = (stroke.points[0], stroke.points[1]);
            let straight = middle(a, b);
            stroke.points.truncate(2);
            if distance(to, straight) >= 3.0 {
                stroke.points.push([2.0 * to[0] - straight[0], 2.0 * to[1] - straight[1]]);
            }
        }
    }
}

/// The number the next badge over a note with `strokes` is given: one more
/// than the highest, so that undoing a badge gives its number back.
pub fn next_badge(strokes: &[Stroke]) -> u32 {
    strokes.iter().filter(|stroke| stroke.tool == Tool::Badge).filter_map(|stroke| stroke.text.parse::<u32>().ok()).max().unwrap_or(0) + 1
}

/// The line a stroke through `points` is drawn along: the points smoothed
/// into curves, each curve given as a few points on it.
pub fn path(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out = points.first().map(|first| vec![*first]).unwrap_or_default();
    for [_, c1, c2, end] in curves(points) {
        let start = *out.last().unwrap();
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let u = 1.0 - t;
            let at = |i: usize| u * u * u * start[i] + 3.0 * u * u * t * c1[i] + 3.0 * u * t * t * c2[i] + t * t * t * end[i];
            out.push([at(0), at(1)]);
        }
    }
    out
}

/// How many points each curve of a path is drawn through.
const STEPS: usize = 6;

/// The cubic curves through `points`: start, two control points and end.
fn curves(points: &[[f32; 2]]) -> Vec<[[f32; 2]; 4]> {
    let mut curves = Vec::new();
    let mut buffer: Vec<[f32; 2]> = Vec::new();
    for point in points {
        if buffer.last() == Some(point) {
            continue;
        }
        buffer.push(*point);
        if buffer.len() == 5 {
            // The end moves to halfway between its neighbours, so that this
            // curve and the next leave it in the same direction.
            buffer[3] = middle(buffer[2], buffer[4]);
            curves.push([buffer[0], buffer[1], buffer[2], buffer[3]]);
            buffer = vec![buffer[3], buffer[4]];
        }
    }
    // What is left over after the last whole curve.
    let along = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    match buffer[..] {
        [a, b] => curves.push([a, along(a, b, 1.0 / 3.0), along(a, b, 2.0 / 3.0), b]),
        // A curve through three points, as a cubic.
        [a, q, b] => curves.push([a, along(a, q, 2.0 / 3.0), along(b, q, 2.0 / 3.0), b]),
        [a, b, c, d] => curves.push([a, b, c, d]),
        _ => {}
    }
    curves
}

fn middle(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// Whether the line `path` runs through comes within `reach` of `point`.
pub fn near(path: &[[f32; 2]], point: [f32; 2], reach: f32) -> bool {
    let distance = |a: [f32; 2], b: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx * dx + dy * dy;
        let t = if length == 0.0 { 0.0 } else { (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length).clamp(0.0, 1.0) };
        ((point[0] - a[0] - t * dx).powi(2) + (point[1] - a[1] - t * dy).powi(2)).sqrt()
    };
    match path {
        [] => false,
        [only] => distance(*only, *only) <= reach,
        _ => path.windows(2).any(|pair| distance(pair[0], pair[1]) <= reach),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen(anchor: &str, nth: usize, points: Vec<[f32; 2]>) -> Stroke {
        Stroke { anchor: anchor.into(), nth, tool: Tool::Pen, color: [233, 49, 71, 255], width: 2.0, points, text: String::new() }
    }

    #[test]
    fn strokes_come_back_as_they_were_written() {
        let strokes = vec![
            pen("## A heading", 0, vec![[10.0, 4.0], [12.5, 6.1]]),
            pen("## A heading", 0, vec![[1.0, 1.0]]),
            Stroke { tool: Tool::Highlighter, color: [255, 208, 0, 96], width: 10.0, ..pen("- an item  ", 2, vec![[-3.0, 8.0], [120.0, 8.0]]) },
        ];
        let file = write(&strokes);
        assert_eq!(file.matches("anchor ").count(), 2, "{file}");
        assert_eq!(read(&file), strokes);
    }

    #[test]
    fn every_kind_of_mark_comes_back_with_its_text() {
        let strokes = vec![
            Stroke { tool: Tool::Line, ..pen("Text", 0, vec![[0.0, 0.0], [10.0, 5.0]]) },
            Stroke { tool: Tool::Arrow, ..pen("Text", 0, vec![[0.0, 0.0], [80.0, 20.0], [30.0, -10.0]]) },
            Stroke { tool: Tool::Rectangle, ..pen("Text", 0, vec![[0.0, 0.0], [10.0, 5.0]]) },
            Stroke { tool: Tool::Ellipse, ..pen("Text", 0, vec![[0.0, 0.0], [10.0, 5.0]]) },
            Stroke { tool: Tool::Text, width: 20.0, text: "Two \"quoted\"\nlines, a \\ and a \\n".into(), ..pen("Text", 0, vec![[10.0, 30.0]]) },
            Stroke { tool: Tool::Badge, width: 13.0, text: "12".into(), ..pen("Text", 0, vec![[5.0, 5.0]]) },
        ];
        let file = write(&strokes);
        assert_eq!(file.lines().count(), 8, "a text stays on one line of the file:\n{file}");
        assert_eq!(read(&file), strokes);
    }

    #[test]
    fn a_line_that_is_not_understood_is_skipped() {
        let file = "pen #ff0000ff 2 1,1\nanchor 0 Text\npen #ff0000ff 2 1,1\npen red 2 1,1\nbrush #ff0000ff 2 1,1\npen #ff0000ff 2 1,x\npen #ff0000ff 0 1,1\nline #ff0000ff 2 1,1\ntext #ff0000ff 2 1,1\ntext #ff0000ff 2 1,1 \"\n";
        let strokes = read(file);
        assert_eq!(strokes, vec![Stroke { color: [255, 0, 0, 255], ..pen("Text", 0, vec![[1.0, 1.0]]) }]);
    }

    #[test]
    fn a_stroke_follows_its_line_when_text_is_written_above_it() {
        let text = "# Title\n\nSome text\n";
        let (anchor, nth) = anchor_at(text, 10);
        assert_eq!((anchor.as_str(), nth), ("Some text", 0));
        let edited = format!("A new first line\n{text}");
        assert_eq!(line_of(&edited, &anchor, nth), Some(edited.find("Some").unwrap()));
        assert_eq!(line_of("# Title\n\nOther text\n", &anchor, nth), None);
    }

    #[test]
    fn lines_with_the_same_text_are_told_apart_by_their_order() {
        let text = "- [ ] item\r\nbetween\r\n- [ ] item\r\n";
        let second = text.rfind("- [ ]").unwrap();
        let (anchor, nth) = anchor_at(text, second);
        assert_eq!((anchor.as_str(), nth), ("- [ ] item", 1));
        assert_eq!(line_of(text, &anchor, nth), Some(second));
        assert_eq!(line_of(text, &anchor, 0), Some(0));
    }

    #[test]
    fn a_task_is_the_same_line_ticked_or_not() {
        let (anchor, nth) = anchor_at("- [ ] milk\n  2. [ ] eggs\n", 12);
        assert_eq!((anchor.as_str(), nth), ("  2. [ ] eggs", 0));
        assert_eq!(line_of("- [x] milk\n  2. [x] eggs\n", &anchor, nth), Some(11));
        assert_eq!(line_of("- [ ] milk\n  2. [/] eggs\n", "  2. [x] eggs", 0), Some(11), "and back, in any state");
        assert_eq!(line_of("- [x] other\n", "- [ ] milk", 0), None, "but its words are its own");
        // Two tasks alike, one ticked, are still told apart by their order.
        assert_eq!(line_of("- [x] a\n- [ ] a\n", "- [ ] a", 1), Some(8));
        assert_eq!(line_of("[x] not a task\n", "[ ] not a task", 0), None);
    }

    #[test]
    fn a_path_starts_and_ends_where_the_pointer_did() {
        let points: Vec<[f32; 2]> = (0..12).map(|i| [i as f32 * 5.0, (i % 3) as f32]).collect();
        let path = path(&points);
        assert_eq!(path.first(), points.first());
        assert_eq!(path.last(), points.last());
        assert!(path.len() > points.len());
        assert_eq!(super::path(&[[3.0, 4.0]]), vec![[3.0, 4.0]]);
    }

    #[test]
    fn a_point_is_near_a_path_within_reach_of_it() {
        let path = [[0.0, 0.0], [10.0, 0.0]];
        assert!(near(&path, [5.0, 2.0], 3.0));
        assert!(!near(&path, [5.0, 4.0], 3.0));
        assert!(!near(&path, [14.0, 0.0], 3.0));
        assert!(near(&[[1.0, 1.0]], [2.0, 1.0], 1.5));
    }

    #[test]
    fn shift_keeps_lines_to_steps_of_45_degrees_and_boxes_square() {
        assert_eq!(shape(Tool::Line, [0.0, 0.0], [100.0, 3.0], true).unwrap()[1][1], 0.0);
        let diagonal = shape(Tool::Line, [0.0, 0.0], [50.0, 47.0], true).unwrap()[1];
        assert!((diagonal[0] - diagonal[1]).abs() < 1e-3, "{diagonal:?}");
        assert_eq!(shape(Tool::Rectangle, [10.0, 10.0], [40.0, 0.0], true).unwrap(), vec![[10.0, 10.0], [40.0, -20.0]]);
        assert_eq!(shape(Tool::Ellipse, [0.0, 0.0], [30.0, 10.0], false).unwrap(), vec![[0.0, 0.0], [30.0, 10.0]]);
        assert_eq!(shape(Tool::Arrow, [0.0, 0.0], [1.0, 1.0], false), None, "a click is not an arrow");
    }

    #[test]
    fn an_arrow_has_its_head_at_its_end_pointing_the_way_it_arrives() {
        let straight = Stroke { tool: Tool::Arrow, width: 2.0, ..pen("Text", 0, vec![[0.0, 0.0], [100.0, 0.0]]) };
        let [tip, a, b] = arrow_head(&straight).unwrap();
        assert_eq!(tip, [100.0, 0.0]);
        assert!(a[0] < 100.0 && a[0] == b[0] && a[1] == -b[1], "{a:?} {b:?}");
        let shaft = outline(&straight);
        assert_eq!(shaft.first(), Some(&[0.0, 0.0]));
        assert!(shaft.last().unwrap()[0] < 100.0, "the shaft stops inside the head");
        let curved = Stroke { points: vec![[0.0, 0.0], [100.0, 0.0], [50.0, -40.0]], ..straight.clone() };
        assert!(outline(&curved).iter().any(|p| p[1] < -10.0), "a curved shaft bends");
        assert!(arrow_head(&curved).unwrap()[1][1] != arrow_head(&curved).unwrap()[2][1]);
        let short = Stroke { points: vec![[0.0, 0.0], [5.0, 0.0]], ..straight };
        assert!(outline(&short).is_empty(), "all head");
    }

    #[test]
    fn a_line_is_bent_through_its_middle_handle_and_straightened_back() {
        let mut line = Stroke { tool: Tool::Line, ..pen("Text", 0, vec![[0.0, 0.0], [100.0, 0.0]]) };
        assert_eq!(handles(&line).unwrap()[2], [50.0, 0.0]);
        move_handle(&mut line, 2, [50.0, -30.0]);
        assert_eq!(line.points.len(), 3);
        assert_eq!(handles(&line).unwrap()[2], [50.0, -30.0], "the curve goes through the handle");
        assert!(outline(&line).iter().any(|p| (p[1] + 30.0).abs() < 0.5));
        move_handle(&mut line, 1, [100.0, 40.0]);
        assert_eq!(line.points[1], [100.0, 40.0], "an end moves, and the bend stays");
        assert_eq!(line.points.len(), 3);
        move_handle(&mut line, 2, [50.0, 21.0]);
        assert_eq!(line.points.len(), 2, "back to the middle, it is straight");
        assert!(handles(&pen("Text", 0, vec![[0.0, 0.0]])).is_none());
    }

    #[test]
    fn badges_are_numbered_on_from_the_highest() {
        let badge = |n: &str| Stroke { tool: Tool::Badge, text: n.into(), ..pen("Text", 0, vec![[0.0, 0.0]]) };
        assert_eq!(next_badge(&[]), 1);
        assert_eq!(next_badge(&[badge("1"), badge("3"), pen("Text", 0, vec![[0.0, 0.0]])]), 4);
    }
}
