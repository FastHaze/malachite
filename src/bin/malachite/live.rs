//! Live Preview, the default editing mode: the editor shows formatted
//! text, and a construct's Markdown syntax only while the cursor touches it.
//!
//! egui's text editor draws exactly the characters in the buffer, so syntax
//! cannot be removed from the display. Instead each character gets a format:
//! hidden syntax is laid out at a size too small to occupy any width, and
//! markers that are replaced with a drawing (bullets, checkboxes, quote
//! bars, rules) stay in place but transparent, with the drawing painted on top.
//!
//! Folding uses the first of those tricks for whole lines: what a folded
//! heading or list item hides is laid out too small to have any height, line
//! breaks included, so the lines after it close up.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use eframe::egui::text::{CCursor, LayoutJob, LayoutSection};
use eframe::egui::{self, pos2, Color32, Galley, Pos2, Rangef, Rect, Shape, Stroke, TextFormat, Vec2};
use malachite::highlight::{highlight, Token};
use malachite::markdown::{self, Kind, Link, Style};
use malachite::outline;

use crate::embed::fit;
use crate::theme::Theme;

/// The room for the line of syntax above an embed, while the cursor is on
/// it, as a part of the text size; and the space left under an embed.
const EMBED_SYNTAX_ROW: f32 = 1.6;
const EMBED_GAP: f32 = 6.0;

/// What the embeds of a note need room for under their lines.
#[derive(Clone, Default, PartialEq)]
pub struct Blocks {
    /// How wide the text column is.
    pub width: f32,
    /// The size of what each embed shows: a picture's width and height in
    /// pixels, or for an embedded note no width and the height it was last
    /// drawn at. An embed that is not here is left as its text.
    pub sizes: Rc<HashMap<Link, [f32; 2]>>,
    /// How tall each table was last drawn, by its text.
    pub tables: Rc<HashMap<String, f32>>,
}

/// What is drawn in the room kept under a line.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    Embed(Link),
    /// The table whose lines are these bytes of the note.
    Table(Range<usize>),
}

impl Blocks {
    /// How tall the room for this embed has to be.
    fn height(&self, embed: &Link) -> Option<f32> {
        let [wide, tall] = *self.sizes.get(embed)?;
        let Link::Embed { width, .. } = embed else { return None };
        Some(if wide > 0.0 { fit(egui::vec2(wide, tall), *width, self.width).y } else { tall })
    }
}

/// The room kept for an embed, as it has been laid out on screen.
pub struct Room {
    /// The top left corner of the room, and how tall it is.
    pub corner: Pos2,
    pub height: f32,
    /// From the top to the bottom of the row of text the room is part of:
    /// the line's own text first, if it is showing, then the room.
    pub row: Rangef,
    /// Where the line's text ends.
    pub text_end: f32,
    pub shown: Shown,
}

/// Room kept under a line for what it embeds.
struct Embedded {
    /// Where the line starts, and where it ends.
    at: usize,
    end: usize,
    /// How far under the top of the line's row the room starts.
    below: f32,
    height: f32,
    shown: Shown,
}

/// Something in the note that reacts to a click.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// A task's `[ ]`, by its byte range in the note.
    Checkbox(Range<usize>),
    Link(Link),
}

enum Decoration {
    /// A dot over a list's `-`.
    Bullet(usize),
    Checkbox { range: Range<usize>, done: bool },
    /// A bar over a quote's `>`, as tall as the (possibly wrapped) line.
    QuoteBar { at: usize, line: Range<usize> },
    /// A horizontal line through the row holding `---`.
    Rule(usize),
    /// A fill behind a code block or callout.
    Background { block: Range<usize>, color: Color32 },
    /// Dots after the line that ends at this offset, whose fold is closed.
    Folded(usize),
}

/// A line that can be folded, for the arrow beside it.
struct Arrow {
    /// Where the line's text starts, after any indentation.
    at: usize,
    line: Range<usize>,
    folded: bool,
}

/// What is drawn over and clicked in the editor, beyond its text.
pub struct Preview {
    decorations: Vec<Decoration>,
    /// Clickable ranges. Only constructs whose syntax is hidden: once the
    /// cursor is in one, a click there is ordinary editing.
    targets: Vec<(Range<usize>, Hit)>,
    /// The lines in view that fold.
    arrows: Vec<Arrow>,
    embeds: Vec<Embedded>,
}

/// Where the table that starts at `lines[index]` ends, if one does: a row
/// with a `|---|` line under it, and every table line that follows.
fn table_end(lines: &[markdown::Line], index: usize) -> Option<usize> {
    let is_table = |line: &markdown::Line| matches!(line.kind, Kind::TableRow { .. } | Kind::TableRule);
    let starts = matches!(lines[index].kind, Kind::TableRow { .. })
        && matches!(lines.get(index + 1).map(|l| &l.kind), Some(Kind::TableRule))
        && (index == 0 || !is_table(&lines[index - 1]));
    starts.then(|| lines[index..].iter().take_while(|l| is_table(l)).last().map_or(lines[index].range.end, |l| l.range.end))
}

/// Lay out `text` for the editor.
///
/// `cursor` is the selection as a byte range, or `None` while the editor has
/// no focus. `source` is Source mode: formatted, all syntax shown.
/// `folded` holds the lines whose folds are closed, as they are written.
#[cfg(test)]
pub fn build(
    text: &str,
    cursor: Option<&Range<usize>>,
    source: bool,
    theme: &Theme,
    exists: &dyn Fn(&str) -> bool,
    folded: &HashSet<String>,
) -> (LayoutJob, Preview) {
    build_with(text, cursor, source, theme, exists, folded, &Blocks::default())
}

/// As [`build`], with room under the lines that are an embed and nothing
/// else for what they embed, which the editor draws there.
///
/// The text field can only show the note's own characters, so the room is
/// made out of them: one of the line's characters is given a line height as
/// tall as the embed, and the line's text is set at the top of that row.
/// While the cursor is elsewhere the syntax is hidden and the row is all
/// embed; with the cursor on the line the syntax shows above it.
pub fn build_with(
    text: &str,
    cursor: Option<&Range<usize>>,
    source: bool,
    theme: &Theme,
    exists: &dyn Fn(&str) -> bool,
    folded: &HashSet<String>,
    blocks: &Blocks,
) -> (LayoutJob, Preview) {
    let touches =
        |r: &Range<usize>| source || cursor.is_some_and(|c| c.start <= r.end && c.end >= r.start);
    let body = theme.format(Style::default(), theme.size, theme.text);
    let mut out = Sections { job: LayoutJob { text: text.to_owned(), ..Default::default() }, at: 0, filler: body };
    let mut decorations = Vec::new();
    let mut targets = Vec::new();

    // Syntax-highlighting tokens of the code block being laid out, and how
    // many of them the lines so far have used up.
    let mut tokens: Vec<(Range<usize>, Token)> = Vec::new();
    let mut next_token = 0;

    let lines = markdown::parse(text);
    let folds = outline::folds_in(text, &lines);
    let mut next_fold = 0;
    let mut arrows = Vec::new();
    let mut embeds = Vec::new();
    // Where the closed fold that the lines being passed are in ends.
    let mut hidden_to = None;

    for (index, line) in lines.iter().enumerate() {
        if hidden_to.is_some_and(|end| line.range.end <= end) {
            continue;
        }
        while folds.get(next_fold).is_some_and(|fold| fold.line.start < line.range.start) {
            next_fold += 1;
        }
        // A table the cursor is not in is drawn as a grid, as Reading view
        // draws it: its first line holds a row open for it, and the rest are
        // hidden as a closed fold hides its lines.
        if let Some(end) = table_end(&lines, index).filter(|_| !source) {
            let table = line.range.start..end;
            if !touches(&table) {
                let rows = lines[index..].iter().take_while(|l| matches!(l.kind, Kind::TableRow { .. })).count().max(1);
                let guess = rows as f32 * (theme.size * 1.4 + 6.0);
                let height = blocks.tables.get(&text[table.clone()]).copied().unwrap_or(guess);
                out.push(line.range.clone(), TextFormat { line_height: Some(height + EMBED_GAP), ..theme.hidden() });
                let hidden = end + usize::from(text.as_bytes().get(end) == Some(&b'\r'));
                out.push(line.range.end..hidden, theme.hidden());
                embeds.push(Embedded { at: line.range.start, end: line.range.end, below: 0.0, height, shown: Shown::Table(table) });
                hidden_to = Some(end);
                continue;
            }
        }
        let active = touches(&line.range);
        let embed = markdown::lone_embed(text, line).filter(|_| !source);
        let embed = embed.and_then(|embed| Some((embed, blocks.height(embed)?)));
        if let Some((embed, height)) = embed.filter(|_| !active) {
            // All embed: the line's characters only hold the row open.
            let room = TextFormat { line_height: Some(height + EMBED_GAP), ..theme.hidden() };
            out.push(line.range.clone(), room);
            embeds.push(Embedded { at: line.range.start, end: line.range.end, below: 0.0, height, shown: Shown::Embed(embed.clone()) });
            continue;
        }
        // The first of the sections this line is about to be given.
        let first_section = out.job.sections.len();
        // A list marker turns back into text only when the cursor is inside
        // it, not when it sits at the start of the item's text.
        let in_marker = touches(&(line.marker.start..line.marker.end.saturating_sub(1)));
        let mut size = theme.size;
        let mut style = Style::default();
        let mut color = theme.text;

        match &line.kind {
            Kind::Blank | Kind::Paragraph => {}
            // A table the cursor is in (or any, in Source mode) is literal, in
            // the monospace font so hand-aligned columns line up.
            Kind::TableRow { .. } => {
                out.push(line.range.clone(), theme.code(theme.text));
                continue;
            }
            Kind::TableRule => {
                out.push(line.range.clone(), theme.code(theme.faint));
                continue;
            }
            Kind::Heading(level) => {
                size = theme.heading_size(*level);
                style.bold = true;
                // An empty heading keeps its `#`, or its row would have no height.
                let shown = active || line.marker.end == line.range.end;
                out.push(line.marker.clone(), if shown { theme.syntax(size) } else { theme.hidden() });
            }
            Kind::Bullet if in_marker => out.push(line.marker.clone(), theme.syntax(size)),
            Kind::Bullet => {
                out.push(line.marker.clone(), theme.invisible(size));
                decorations.push(Decoration::Bullet(line.marker.start));
            }
            Kind::Numbered | Kind::Footnote(_) => out.push(line.marker.clone(), theme.syntax(size)),
            Kind::Task { done, checkbox } => {
                if in_marker {
                    out.push(line.marker.clone(), theme.syntax(size));
                } else {
                    // The `- ` vanishes so the box sits where a bullet would.
                    out.push(line.marker.start..checkbox.start, theme.hidden());
                    out.push(checkbox.start..line.marker.end, theme.invisible(size));
                    decorations.push(Decoration::Checkbox { range: checkbox.clone(), done: *done });
                    targets.push((checkbox.clone(), Hit::Checkbox(checkbox.clone())));
                }
                if *done {
                    style.strike = true;
                    color = theme.muted;
                }
            }
            Kind::Quote { callout, tag } => {
                let tint = callout.as_deref().map(|kind| theme.callout(kind));
                let bars = line.marker.start..tag.as_ref().map_or(line.marker.end, |t| t.start);
                if active {
                    out.push(bars, theme.syntax(size));
                } else {
                    // A callout is a tinted box; a plain quote gets a bar per `>`.
                    if tint.is_none() {
                        for at in bars.clone().filter(|&i| text.as_bytes()[i] == b'>') {
                            decorations.push(Decoration::QuoteBar { at, line: line.range.clone() });
                        }
                    }
                    out.push(bars, theme.invisible(size));
                }
                if let (Some(tag), Some(tint)) = (tag, tint) {
                    let fill = theme.callout_fill(tint);
                    decorations.push(Decoration::Background { block: line.block.clone(), color: fill });
                    style.bold = true;
                    color = tint;
                    if active {
                        out.push(tag.clone(), theme.syntax(size));
                    } else if tag.end == line.range.end {
                        // No title: the type is shown instead, so leave
                        // that word of `[!type]` visible.
                        let name_end = tag.start + text[tag.clone()].find(']').unwrap_or(2);
                        out.push(tag.start..tag.start + 2, theme.hidden());
                        out.push(tag.start + 2..name_end, theme.format(style, size, color));
                        out.push(name_end..tag.end, theme.hidden());
                    } else {
                        out.push(tag.clone(), theme.hidden());
                    }
                }
            }
            Kind::Rule if active => out.push(line.range.clone(), theme.syntax(size)),
            Kind::Rule => {
                out.push(line.range.clone(), theme.invisible(size));
                decorations.push(Decoration::Rule(line.range.start));
            }
            Kind::Fence { open } => {
                if *open {
                    let block = line.block.clone();
                    decorations.push(Decoration::Background { block, color: theme.code_bg });
                    // The word after the fence names the language to colour the code by.
                    let language = markdown::fence_language(&text[line.range.clone()]);
                    let mut code = lines[index + 1..].iter().take_while(|l| l.kind == Kind::Code);
                    let code = code.next().map(|first| first.range.start..code.last().unwrap_or(first).range.end);
                    tokens = code.map_or_else(Vec::new, |code| {
                        let shift = |(r, token): (Range<usize>, Token)| (code.start + r.start..code.start + r.end, token);
                        highlight(language, &text[code.clone()]).into_iter().map(shift).collect()
                    });
                    next_token = 0;
                }
                // Fences show while the cursor is anywhere in the block; otherwise
                // their rows stay as the block's top and bottom padding.
                let shown = touches(&line.block);
                let color = if shown { theme.faint } else { Color32::TRANSPARENT };
                out.push(line.range.clone(), theme.code(color));
            }
            Kind::Code => {
                // A token can span lines (a block comment); each line takes its part.
                let mut at = line.range.start;
                while let Some((range, token)) = tokens.get(next_token) {
                    if range.start >= line.range.end {
                        break;
                    }
                    let part = range.start.max(at)..range.end.min(line.range.end);
                    if !part.is_empty() {
                        out.push(at..part.start, theme.code_token(None));
                        out.push(part.clone(), theme.code_token(Some(*token)));
                        at = part.end;
                    }
                    if range.end > line.range.end {
                        break;
                    }
                    next_token += 1;
                }
                out.push(at..line.range.end, theme.code_token(None));
            }
            Kind::FrontmatterFence => out.push(line.range.clone(), theme.code(theme.faint)),
            Kind::Frontmatter => out.push(line.range.clone(), theme.code(theme.muted)),
        }

        for span in &line.spans {
            let touched = touches(&span.element);
            let format = if span.marker {
                if touched {
                    theme.syntax(size)
                } else {
                    theme.hidden()
                }
            } else {
                let mut merged = span.style;
                merged.bold |= style.bold;
                merged.strike |= style.strike;
                let mut format = theme.format(merged, size, color);
                if let Some(link) = &span.link {
                    format = theme.link(format, link, resolved(link, exists));
                    if !touched {
                        targets.push((span.range.clone(), Hit::Link(link.clone())));
                    }
                }
                format
            };
            out.push(span.range.clone(), format);
        }

        if let Some((embed, height)) = embed {
            // The cursor is on the line: its syntax at the top of a row
            // that goes on down far enough for the embed.
            let below = theme.size * EMBED_SYNTAX_ROW;
            // Not the line break before it, which is among these sections
            // too but belongs to the row above.
            let own = out.job.sections[first_section..].iter_mut().filter(|section| section.byte_range.start >= line.range.start);
            for (index, section) in own.enumerate() {
                section.format.valign = egui::Align::TOP;
                if index == 0 {
                    section.format.line_height = Some(below + height + EMBED_GAP);
                }
            }
            embeds.push(Embedded { at: line.range.start, end: line.range.end, below, height, shown: Shown::Embed(embed.clone()) });
        }

        if let Some(fold) = folds.get(next_fold).filter(|fold| fold.line == line.range) {
            let closed = folded.contains(&text[fold.line.clone()]);
            arrows.push(Arrow { at: line.marker.start, line: fold.line.clone(), folded: closed });
            if closed {
                // A `\r` left at the end of the last line would keep its row's height.
                let end = fold.hidden.end + usize::from(text.as_bytes().get(fold.hidden.end) == Some(&b'\r'));
                out.push(fold.hidden.start..end, theme.hidden());
                decorations.push(Decoration::Folded(fold.line.end));
                hidden_to = Some(fold.hidden.end);
            }
        }
    }
    (out.finish(), Preview { decorations, targets, arrows, embeds })
}

/// A layout that has been built, with everything it was built from. The
/// editor asks for its layout on every frame, and most frames nothing it
/// depends on has changed: the mouse moved, or something else was redrawn.
pub struct Kept {
    text: String,
    cursor: Option<Range<usize>>,
    source: bool,
    /// The look it was laid out in (`Theme::key`).
    look: u64,
    /// The `App::version` of the notes that `exists` answered from.
    notes: u64,
    folded: HashSet<String>,
    blocks: Blocks,
    job: LayoutJob,
    preview: Rc<Preview>,
}

/// As [`build`], but handing back the layout in `kept` when it was built
/// from these same things, and keeping the one built otherwise. `notes` says
/// which state of the vault `exists` speaks for.
#[allow(clippy::too_many_arguments)]
pub fn build_kept(
    kept: &mut Option<Kept>,
    text: &str,
    cursor: Option<&Range<usize>>,
    source: bool,
    theme: &Theme,
    exists: &dyn Fn(&str) -> bool,
    notes: u64,
    folded: &HashSet<String>,
    blocks: &Blocks,
) -> (LayoutJob, Rc<Preview>) {
    let look = theme.key();
    let same = |k: &&Kept| {
        let same_view = k.cursor.as_ref() == cursor && k.source == source && k.look == look && k.blocks == *blocks;
        same_view && k.notes == notes && k.folded == *folded && k.text == text
    };
    if let Some(kept) = kept.as_ref().filter(same) {
        return (kept.job.clone(), kept.preview.clone());
    }
    let (job, preview) = build_with(text, cursor, source, theme, exists, folded, blocks);
    let preview = Rc::new(preview);
    *kept = Some(Kept {
        text: text.to_owned(),
        cursor: cursor.cloned(),
        source,
        look,
        notes,
        folded: folded.clone(),
        blocks: blocks.clone(),
        job: job.clone(),
        preview: preview.clone(),
    });
    (job, preview)
}

/// Where a caret taken in `old` is in `new`, one edit later. The text field
/// lays out the text it has just changed with the caret as it was before the
/// change, which after a Backspace at the end of a line is the start of the
/// next one: that line then showed its markers for a frame. An edit starts
/// at the caret, so whatever it changed is taken to start no earlier, and
/// the caret goes to the end of what it put in.
pub fn follow(old: &str, new: &str, caret: &Range<usize>) -> Range<usize> {
    let head = &old.as_bytes()[..caret.start.min(old.len())];
    let mut start = head.iter().zip(new.as_bytes()).take_while(|(a, b)| a == b).count();
    while !new.is_char_boundary(start) {
        start -= 1;
    }
    let room = old.len().min(new.len()) - start;
    let tail = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    let mut end = new.len() - tail;
    while !new.is_char_boundary(end) {
        end += 1;
    }
    end..end
}

/// Whether a link leads somewhere that exists. Only note links can dangle.
pub fn resolved(link: &Link, exists: &dyn Fn(&str) -> bool) -> bool {
    match link {
        Link::Note(title, _) => exists(title),
        // Whether a file is there is only found out when it is to be shown.
        Link::Embed { target, .. } => malachite::is_attachment(target) || exists(target),
        Link::Url(_) | Link::Tag(_) | Link::Footnote(_) => true,
    }
}

/// Collects format sections in order, giving anything skipped (line breaks,
/// indentation, unformatted lines) the body format.
struct Sections {
    job: LayoutJob,
    at: usize,
    filler: TextFormat,
}

impl Sections {
    fn push(&mut self, range: Range<usize>, format: TextFormat) {
        debug_assert!(range.start >= self.at, "sections must be pushed in order");
        self.section(self.at..range.start, self.filler.clone());
        self.section(range, format);
    }

    fn section(&mut self, byte_range: Range<usize>, format: TextFormat) {
        if !byte_range.is_empty() {
            self.at = byte_range.end;
            self.job.sections.push(LayoutSection { leading_space: 0.0, byte_range, format });
        }
    }

    fn finish(mut self) -> LayoutJob {
        self.section(self.at..self.job.text.len(), self.filler.clone());
        if self.job.sections.is_empty() {
            // Even an empty note needs a section to give its one row a height.
            let format = self.filler.clone();
            self.job.sections.push(LayoutSection { leading_space: 0.0, byte_range: 0..0, format });
        }
        self.job
    }
}

impl Preview {
    /// Draw everything that is shown in place of syntax. `background` is a
    /// shape reserved before the editor was drawn, so fills end up under the
    /// text. `marks` are parts of the text to fill in behind, each in its colour.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        painter: &egui::Painter,
        background: egui::layers::ShapeIdx,
        galley: &Galley,
        origin: Pos2,
        width: Rangef,
        offsets: &Offsets,
        theme: &Theme,
        marks: &[(Range<usize>, Color32)],
    ) {
        // Where the character at a byte offset starts, as a zero-width rect of its row's height.
        let edge = |byte: usize| {
            galley.pos_from_ccursor(CCursor::new(offsets.char_at(byte))).translate(origin.to_vec2())
        };
        // Middle of the characters in `range`, which sit on one row.
        let middle = |range: Range<usize>| {
            let (a, b) = (edge(range.start), edge(range.end));
            let right = if b.top() == a.top() { b.left() } else { a.left() + theme.size / 2.0 };
            pos2((a.left() + right) / 2.0, a.center().y)
        };

        let mut fills = Vec::new();
        for decoration in &self.decorations {
            match decoration {
                Decoration::Bullet(at) => theme.paint_bullet(painter, middle(*at..at + 1)),
                Decoration::Checkbox { range, done } => {
                    theme.paint_checkbox(painter, middle(range.clone()), *done);
                }
                Decoration::QuoteBar { at, line } => {
                    let (top, bottom) = (edge(*at), edge(line.end));
                    let stroke = Stroke::new(2.0, theme.accent);
                    painter.vline(top.left() + 1.0, top.top()..=bottom.bottom(), stroke);
                }
                Decoration::Rule(at) => {
                    painter.hline(width, edge(*at).center().y, Stroke::new(1.0, theme.rule));
                }
                Decoration::Background { block, color } => {
                    let (top, bottom) = (edge(block.start), edge(block.end));
                    let rect = Rect::from_x_y_ranges(width.expand(8.0), top.top()..=bottom.bottom());
                    fills.push(Shape::rect_filled(rect, 4.0, *color));
                }
                Decoration::Folded(at) => paint_folded(painter, edge(*at).right_center(), theme),
            }
        }
        for (range, color) in marks {
            let (start, end) = (edge(range.start), edge(range.end));
            let mut fill = |x: std::ops::RangeInclusive<f32>, y: Rangef| {
                fills.push(Shape::rect_filled(Rect::from_x_y_ranges(x, y), 2.0, *color));
            };
            if start.top() == end.top() {
                fill(start.left()..=end.left(), start.y_range());
            } else {
                // It runs on over a line break: to the end of its first row,
                // across any rows between, and from the start of its last.
                fill(start.left()..=width.max, start.y_range());
                if end.top() > start.bottom() {
                    fill(width.min..=width.max, Rangef::new(start.bottom(), end.top()));
                }
                fill(width.min..=end.left(), end.y_range());
            }
        }
        painter.set(background, Shape::Vec(fills));
    }

    /// The clickable thing under `pos`, if any.
    pub fn hit(&self, galley: &Galley, origin: Pos2, offsets: &Offsets, pos: Pos2) -> Option<&Hit> {
        let byte = offsets.byte_at(char_at(galley, pos - origin)?);
        self.targets.iter().find(|(range, _)| range.contains(&byte)).map(|(_, hit)| hit)
    }

    /// Where the arrow that folds each foldable line goes: in the margin
    /// beside the start of the line, as tall as the line's first row. With
    /// each comes the line it folds and whether that fold is closed.
    pub fn arrows(&self, galley: &Galley, origin: Pos2, offsets: &Offsets) -> Vec<(Rect, Range<usize>, bool)> {
        let place = |arrow: &Arrow| {
            let start = galley.pos_from_ccursor(CCursor::new(offsets.char_at(arrow.at))).translate(origin.to_vec2());
            (arrow_beside(start.left(), start.y_range()), arrow.line.clone(), arrow.folded)
        };
        self.arrows.iter().map(place).collect()
    }

    /// Where the room kept for each embed is, with what is embedded.
    pub fn embeds(&self, galley: &Galley, origin: Pos2, offsets: &Offsets) -> Vec<Room> {
        let place = |kept: &Embedded| {
            let edge = |byte: usize| galley.pos_from_ccursor(CCursor::new(offsets.char_at(byte))).translate(origin.to_vec2());
            let row = edge(kept.at);
            Room {
                corner: pos2(origin.x, row.top() + kept.below),
                height: kept.height,
                row: Rangef::new(row.top(), row.bottom()),
                text_end: edge(kept.end).right(),
                shown: kept.shown.clone(),
            }
        };
        self.embeds.iter().map(place).collect()
    }

    /// Centre of every clickable range, for driving the editor in tests.
    #[cfg(test)]
    pub fn hot_spots(&self, galley: &Galley, origin: Pos2, offsets: &Offsets) -> Vec<(Pos2, Hit)> {
        self.targets
            .iter()
            .map(|(range, hit)| {
                let at = |byte: usize| galley.pos_from_ccursor(CCursor::new(offsets.char_at(byte)));
                let (a, b) = (at(range.start), at(range.start + 1));
                (origin + pos2((a.left() + b.left()) / 2.0, a.center().y).to_vec2(), hit.clone())
            })
            .collect()
    }
}

/// How much of the margin beside a line its fold arrow takes.
const ARROW_ROOM: f32 = 16.0;

/// Where the arrow that folds a line goes: in the margin before `left`,
/// where the line starts, as tall as `row`, the line's first row. Reading
/// view puts its arrows in the same place.
pub fn arrow_beside(left: f32, row: Rangef) -> Rect {
    Rect::from_x_y_ranges(left - ARROW_ROOM..=left - 2.0, row)
}

/// Draw the dots that end a folded line, which ends at `end`.
pub fn paint_folded(painter: &egui::Painter, end: Pos2, theme: &Theme) {
    let dots = pos2(end.x + 6.0, end.y);
    painter.text(dots, egui::Align2::LEFT_CENTER, "…", egui::FontId::proportional(theme.size), theme.faint);
}

/// Draw a fold arrow in `rect`: pointing at the line while its fold is
/// closed, and down at what is under it while that is showing.
pub fn paint_arrow(painter: &egui::Painter, rect: Rect, folded: bool, color: Color32) {
    let (c, r) = (rect.center(), 3.5);
    let points = if folded {
        vec![pos2(c.x - r * 0.6, c.y - r), pos2(c.x + r * 0.8, c.y), pos2(c.x - r * 0.6, c.y + r)]
    } else {
        vec![pos2(c.x - r, c.y - r * 0.6), pos2(c.x + r, c.y - r * 0.6), pos2(c.x, c.y + r * 0.8)]
    };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Index of the character drawn at `pos` (relative to the galley), if `pos`
/// is on a character rather than in the space around the text.
pub fn char_at(galley: &Galley, pos: Vec2) -> Option<usize> {
    let mut index = 0;
    for row in &galley.rows {
        if row.rect.y_range().contains(pos.y) {
            let glyph = row.glyphs.iter().position(|g| (g.pos.x..g.max_x()).contains(&pos.x))?;
            return Some(index + glyph);
        }
        index += row.glyphs.len() + usize::from(row.ends_with_newline);
    }
    None
}

/// Converts between byte offsets (what the parser uses) and character
/// indices (what egui's cursor uses).
pub struct Offsets {
    /// Byte offset of every character; empty for ASCII text, where the two agree.
    starts: Vec<usize>,
    len: usize,
}

impl Offsets {
    pub fn new(text: &str) -> Self {
        let starts = if text.is_ascii() { Vec::new() } else { text.char_indices().map(|(i, _)| i).collect() };
        Self { starts, len: text.len() }
    }

    pub fn char_at(&self, byte: usize) -> usize {
        if self.starts.is_empty() {
            byte.min(self.len)
        } else {
            self.starts.partition_point(|&start| start < byte)
        }
    }

    pub fn byte_at(&self, char_index: usize) -> usize {
        if self.starts.is_empty() {
            char_index.min(self.len)
        } else {
            self.starts.get(char_index).copied().unwrap_or(self.len)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_follows_an_edit_made_at_it() {
        // Backspace at the end of an item: the caret stays on that line.
        assert_eq!(follow("- a\n- b", "- \n- b", &(3..3)), 2..2);
        // Delete before a run of the same letter, and typing into one.
        assert_eq!(follow("aab", "ab", &(1..1)), 1..1);
        assert_eq!(follow("aa", "aaa", &(1..1)), 2..2);
        // A selection typed over, in letters wider than a byte.
        assert_eq!(follow("ая б", "аx б", &(2..4)), 3..3);
    }

    fn build_at(text: &str, cursor: Option<usize>) -> (LayoutJob, Preview) {
        let cursor = cursor.map(|c| c..c);
        build(text, cursor.as_ref(), false, &Theme::new(true, false), &|title| title == "Real", &HashSet::new())
    }

    /// The format given to the character at byte offset `at`.
    fn format(job: &LayoutJob, at: usize) -> &TextFormat {
        &job.sections.iter().find(|s| s.byte_range.contains(&at)).expect("no section").format
    }

    fn hidden(job: &LayoutJob, at: usize) -> bool {
        format(job, at).font_id.size < 1.0
    }

    #[test]
    fn sections_cover_the_whole_note_in_order() {
        let text = "---\na: b\n---\n# T\n\n- x\n  1. y\r\n- [x] **z** [[Real|r]] #t\n> q\n> [!tip]\n> c\n---\n```\ncode\n```\n| a |\né ==ü== `c`\n```py\ns = '''é\r\nü''' # é\r\n\r\nx\n```\n```rust\n";
        for cursor in [None, Some(0), Some(20), Some(text.len())] {
            for (source, fold) in [(false, false), (true, false), (false, true)] {
                let cursor = cursor.map(|c| c..c);
                let folded: HashSet<String> = ["# T", "- x"].iter().filter(|_| fold).map(|line| line.to_string()).collect();
                let (job, _) = build(text, cursor.as_ref(), source, &Theme::new(false, true), &|_| true, &folded);
                let mut at = 0;
                for section in &job.sections {
                    assert_eq!(section.byte_range.start, at);
                    assert!(text.is_char_boundary(section.byte_range.end));
                    at = section.byte_range.end;
                }
                assert_eq!(at, text.len());
            }
        }
        assert_eq!(build_at("", None).0.sections.len(), 1);
    }

    #[test]
    fn inline_syntax_shows_only_while_the_cursor_touches_it() {
        let text = "a **bold** b";
        let (job, _) = build_at(text, Some(0));
        assert!(hidden(&job, 2) && hidden(&job, 8), "cursor elsewhere on the line");
        assert!(!hidden(&job, 4));
        for cursor in [2, 5, 10] {
            let (job, _) = build_at(text, Some(cursor));
            assert!(!hidden(&job, 2) && !hidden(&job, 8), "cursor at {cursor}");
        }
        let (job, _) = build_at(text, None);
        assert!(hidden(&job, 2), "no focus, nothing revealed");
    }

    #[test]
    fn heading_syntax_shows_while_the_cursor_is_on_its_line() {
        let text = "## Title\nbody";
        let theme = Theme::new(true, false);
        let (job, _) = build_at(text, Some(10));
        assert!(hidden(&job, 0));
        assert_eq!(format(&job, 3).font_id.size, theme.heading_size(2));
        assert_eq!(format(&job, 10).font_id.size, theme.size);
        let (job, _) = build_at(text, Some(8));
        assert!(!hidden(&job, 0));
        // An empty heading never hides its marker.
        assert!(!hidden(&build_at("# \nx", Some(3)).0, 0));
    }

    #[test]
    fn source_mode_shows_everything() {
        let text = "# T **b** [[Real]]";
        let (job, preview) = build(text, None, true, &Theme::new(true, false), &|_| true, &HashSet::new());
        assert!((0..text.len()).all(|i| !hidden(&job, i)));
        assert!(preview.targets.is_empty() && preview.decorations.is_empty());
    }

    #[test]
    fn list_markers_become_drawings_until_the_cursor_enters_them() {
        let text = "- item\n- [ ] task";
        let (job, preview) = build_at(text, Some(2));
        assert_eq!(format(&job, 0).color, Color32::TRANSPARENT, "cursor at the item's text");
        assert!(matches!(preview.decorations[0], Decoration::Bullet(0)));
        assert!(matches!(&preview.decorations[1], Decoration::Checkbox { done: false, range } if *range == (9..12)));
        let (job, preview) = build_at(text, Some(1));
        assert_ne!(format(&job, 0).color, Color32::TRANSPARENT, "cursor inside the marker");
        assert!(!preview.decorations.iter().any(|d| matches!(d, Decoration::Bullet(_))));
    }

    #[test]
    fn hidden_links_and_checkboxes_are_click_targets() {
        let text = "- [ ] see [[Real|it]] and [[Missing]] #tag";
        let (job, preview) = build_at(text, None);
        let hits: Vec<_> = preview.targets.iter().map(|(_, hit)| hit.clone()).collect();
        let note = |title: &str| Hit::Link(Link::Note(title.into(), None));
        assert_eq!(hits, [Hit::Checkbox(2..5), note("Real"), note("Missing"), Hit::Link(Link::Tag("tag".into()))]);
        assert_eq!(&text[preview.targets[1].0.clone()], "it");
        assert!(hidden(&job, 10) && hidden(&job, 14), "[[Real| is hidden");
        // A link to a note that does not exist is fainter.
        assert_ne!(format(&job, 17).color, format(&job, 28).color);
        // With the cursor inside the link it is text to edit, not a target.
        let (_, preview) = build_at(text, Some(18));
        assert!(!preview.targets.iter().any(|(_, hit)| *hit == note("Real")));
    }

    #[test]
    fn a_closed_fold_takes_no_room_and_leaves_its_line_showing() {
        let text = "# A\r\none\r\n\r\n## B\r\ntwo\r\n# C\r\n- x\r\n  - y\r\nend";
        let folded = HashSet::from(["# A".to_owned(), "## B".to_owned(), "- x".to_owned()]);
        let (job, preview) = build(text, None, false, &Theme::new(true, false), &|_| true, &folded);
        let at = |snippet: &str| text.find(snippet).unwrap();
        assert!(!hidden(&job, at("A")) && !hidden(&job, at("C")) && !hidden(&job, at("x")) && !hidden(&job, at("end")));
        // Line breaks and all, or the rows would still be there, empty.
        assert!((at("A") + 1..at("# C") - 1).all(|i| hidden(&job, i)), "everything under A");
        assert!(!hidden(&job, at("# C") - 1), "but not the break that starts the next line");
        assert!((at("x") + 1..at("end") - 1).all(|i| hidden(&job, i)), "the item nested in x");
        // B is folded too, but it is out of sight inside A: no arrow for it.
        let arrows: Vec<_> = preview.arrows.iter().map(|arrow| (&text[arrow.line.clone()], arrow.folded)).collect();
        assert_eq!(arrows, [("# A", true), ("# C", false), ("- x", true)]);
        let dots = preview.decorations.iter().filter(|d| matches!(d, Decoration::Folded(_))).count();
        assert_eq!(dots, 2);
    }

    #[test]
    fn lines_that_fold_have_an_arrow_whether_folded_or_not() {
        let (_, preview) = build_at("# A\ntext\n- x\n- y\n  - z\n# Empty", None);
        let arrows: Vec<_> = preview.arrows.iter().map(|arrow| (arrow.at, arrow.folded)).collect();
        assert_eq!(arrows, [(0, false), (13, false)]);
    }

    #[test]
    fn a_layout_is_kept_until_something_it_was_built_from_changes() {
        let theme = Theme::new(true, false);
        let (mut kept, none) = (None, HashSet::new());
        let built = std::cell::Cell::new(0);
        // `exists` is only asked while a layout is being built.
        let exists = |_: &str| {
            built.set(built.get() + 1);
            true
        };
        let mut layout = |text: &str, cursor: usize, notes: u64, folded: &HashSet<String>| {
            let cursor = cursor..cursor;
            build_kept(&mut kept, text, Some(&cursor), false, &theme, &exists, notes, folded, &Blocks::default()).0
        };
        let first = layout("[[B]]\n# A\nx", 0, 1, &none);
        assert_eq!(layout("[[B]]\n# A\nx", 0, 1, &none), first);
        assert_eq!(built.get(), 1, "the second time it was handed back as it was");
        layout("[[C]]\n# A\nx", 0, 1, &none);
        layout("[[C]]\n# A\nx", 2, 1, &none);
        layout("[[C]]\n# A\nx", 2, 2, &none);
        layout("[[C]]\n# A\nx", 2, 2, &HashSet::from(["# A".to_owned()]));
        assert_eq!(built.get(), 5, "other text, another cursor, other notes, another fold: built again each time");
    }

    #[test]
    fn a_line_that_is_an_embed_keeps_room_under_it_for_what_it_shows() {
        let theme = Theme::new(true, false);
        let text = "before\n![[pic.png|100]]\nsee ![[pic.png]] here\n![[Unknown]]";
        let picture = Link::Embed { target: "pic.png".into(), place: None, width: Some(100) };
        let blocks = Blocks { width: 500.0, sizes: Rc::new(HashMap::from([(picture.clone(), [400.0, 200.0])])), ..Blocks::default() };
        let build = |cursor: Option<usize>, source| {
            let cursor = cursor.map(|c| c..c);
            build_with(text, cursor.as_ref(), source, &theme, &|_| true, &HashSet::new(), &blocks)
        };
        // Away from the line: the syntax is hidden and the row is as tall as the picture at the width asked for.
        let (job, preview) = build(Some(0), false);
        assert!(hidden(&job, 8) && hidden(&job, 20));
        assert_eq!(format(&job, 8).line_height, Some(50.0 + EMBED_GAP));
        assert_eq!(preview.embeds.len(), 1, "an embed in a sentence, or one with nothing to show, is left as text");
        assert_eq!((preview.embeds[0].at, preview.embeds[0].below, preview.embeds[0].height), (7, 0.0, 50.0));
        assert_eq!(preview.embeds[0].shown, Shown::Embed(picture));
        // On the line: the syntax shows, at the top of a row with the picture's room below it.
        let (job, preview) = build(Some(10), false);
        assert!(!hidden(&job, 8));
        assert_eq!(format(&job, 8).line_height, Some(theme.size * EMBED_SYNTAX_ROW + 50.0 + EMBED_GAP));
        assert!(preview.embeds[0].below > 0.0);
        // Source mode shows the note as it is written.
        assert!(build(Some(0), true).1.embeds.is_empty());
    }

    #[test]
    fn a_table_away_from_the_cursor_is_one_row_held_open_for_its_grid() {
        let theme = Theme::new(true, false);
        let text = "before\n| a | b |\n|---|---|\n| 1 | 2 |\nafter";
        let table = 7..text.len() - 6;
        let blocks = Blocks { tables: Rc::new(HashMap::from([(text[table.clone()].to_owned(), 80.0)])), ..Blocks::default() };
        let build = |cursor: usize, source| build_with(text, Some(&(cursor..cursor)), source, &theme, &|_| true, &HashSet::new(), &blocks);
        let (job, preview) = build(0, false);
        assert_eq!(format(&job, 8).line_height, Some(80.0 + EMBED_GAP), "as tall as the grid was drawn");
        assert!(hidden(&job, 8) && hidden(&job, 20) && hidden(&job, 30), "every line of it hidden");
        assert!(!hidden(&job, text.len() - 2), "and the text after it not");
        assert_eq!(preview.embeds.len(), 1);
        assert_eq!(preview.embeds[0].shown, Shown::Table(table));
        // The cursor in any line of it, or Source mode, shows the lines as written.
        assert!(build(30, false).1.embeds.is_empty() && !hidden(&build(30, false).0, 8));
        assert!(build(0, true).1.embeds.is_empty());
    }

    #[test]
    fn code_fences_show_while_the_cursor_is_in_the_block() {
        let text = "```\ncode\n```\nafter";
        let (job, preview) = build_at(text, Some(text.len()));
        assert_eq!(format(&job, 0).color, Color32::TRANSPARENT);
        assert!(matches!(&preview.decorations[0], Decoration::Background { block, .. } if *block == (0..12)));
        assert_ne!(format(&build_at(text, Some(5)).0, 0).color, Color32::TRANSPARENT);
    }

    #[test]
    fn code_is_coloured_by_the_language_on_the_fence() {
        let theme = Theme::new(true, false);
        let text = "```rust\nfn x() { /* é\nb */ }\r\n```\n```\nfn y\n```\n```nonsense\nfn z\n```";
        let role = |token| theme.code_token(Some(token)).color;
        let plain = theme.code_token(None).color;
        for cursor in [None, Some(9)] {
            let (job, _) = build_at(text, cursor);
            let color = |snippet: &str| format(&job, text.find(snippet).unwrap()).color;
            assert_eq!(color("fn x"), role(Token::Keyword));
            assert_eq!(color("x()"), role(Token::Function));
            assert_eq!(color("/* é"), role(Token::Comment));
            assert_eq!(color("b */"), role(Token::Comment), "a comment keeps its colour on its second line");
            assert_eq!(color("}"), role(Token::Punctuation));
            assert_eq!(color("fn y"), plain, "no language, no colours");
            assert_eq!(color("fn z"), plain, "unknown language, no colours");
        }
        assert_ne!(role(Token::Keyword), plain);
    }

    #[test]
    fn offsets_convert_between_bytes_and_characters() {
        let text = "aé-ю!";
        let offsets = Offsets::new(text);
        for (chars, (byte, _)) in text.char_indices().enumerate() {
            assert_eq!(offsets.char_at(byte), chars);
            assert_eq!(offsets.byte_at(chars), byte);
        }
        assert_eq!(offsets.char_at(text.len()), 5);
        assert_eq!(offsets.byte_at(5), text.len());
        let ascii = Offsets::new("abc");
        assert_eq!((ascii.char_at(2), ascii.byte_at(3), ascii.byte_at(9)), (2, 3, 3));
    }
}
