//! Reading view: the note fully rendered, with no Markdown syntax in sight.
//!
//! Line breaks are kept as they are written, where strict Markdown would
//! join the lines of a paragraph: every source line is its own visual line
//! and a blank line separates paragraphs.
//!
//! What a note embeds is drawn where it is embedded: a picture as the
//! picture, a note (or the heading or block of one) as that note rendered in
//! a box, by this same code a level deeper. Footnotes are numbered in the
//! order they are referred to and listed at the end.
//!
//! A note folds here as it does in the editor, by the same folds: what a
//! folded heading or list item hides is not drawn, and the line ends in
//! dots. Where the arrows go is handed back for the editor's code to draw
//! and click, so that both views fold alike. A note embedded in another is
//! drawn whole.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::text::LayoutJob;
use eframe::egui::text_selection::LabelSelectionState;
use eframe::egui::{self, vec2, Color32, CursorIcon, Galley, Margin, Rect, Sense, Stroke, TextFormat};
use malachite::highlight::highlight;
use malachite::markdown::{self, Kind, Line, Link, Span, Style};
use malachite::outline::{self, Fold};

use crate::embed::{fit, Embeds};
use crate::live::{self, char_at, resolved, Hit};
use crate::theme::Theme;

/// How many notes deep embeds are followed: a note that embeds itself, or
/// two that embed each other, stop here.
const DEPTH: usize = 3;

/// What came of drawing a note in Reading view.
pub struct Read {
    /// The link or checkbox that was clicked.
    pub hit: Option<Hit>,
    /// Each line drawn that folds: where its arrow goes, the line, and
    /// whether it is folded, as `live::Preview::arrows` has them.
    pub arrows: Vec<(Rect, Range<usize>, bool)>,
    /// The folded lines that hid the place `goto` named, which have been
    /// drawn open to show it and are to stay open.
    pub unfolded: Vec<String>,
}

/// Draw `text` rendered, with the lines in `folded` folded (the text of
/// each line, as `App::folds` keeps them). `goto` is a place in the note, as
/// a byte offset, to bring to the top of the view: the heading a link or the
/// outline leads to.
pub fn show(
    ui: &mut egui::Ui,
    text: &str,
    theme: &Theme,
    exists: &dyn Fn(&str) -> bool,
    goto: Option<usize>,
    folded: &HashSet<String>,
    embeds: &mut Embeds,
) -> Read {
    let mut reader = Reader::new(ui, text, theme, exists, Some(folded), embeds);
    reader.blocks(ui, goto);
    Read { hit: reader.hit, arrows: reader.arrows, unfolded: reader.unfolded }
}

/// Draw what `embed` embeds, as Reading view shows it, for the editor to put
/// under the line that embeds it. Returns the link in it that was clicked.
pub fn embed(ui: &mut egui::Ui, embed: &Link, theme: &Theme, exists: &dyn Fn(&str) -> bool, embeds: &mut Embeds) -> Option<Hit> {
    let mut reader = Reader::new(ui, "", theme, exists, None, embeds);
    reader.embed(ui, embed);
    reader.hit
}

/// Draw `text`, the lines of one table, as Reading view draws a table.
pub fn table(ui: &mut egui::Ui, text: &str, theme: &Theme, exists: &dyn Fn(&str) -> bool, embeds: &mut Embeds) -> Option<Hit> {
    let lines = markdown::parse(text);
    let mut reader = Reader::new(ui, text, theme, exists, None, embeds);
    reader.table(ui, &lines);
    reader.hit
}

struct Reader<'a, 'e> {
    text: &'a str,
    theme: &'a Theme,
    exists: &'a dyn Fn(&str) -> bool,
    /// Height of one row of body text.
    row: f32,
    hit: Option<Hit>,
    embeds: &'a mut Embeds<'e>,
    /// How many notes deep in embeds this text is.
    depth: usize,
    /// The ids of the footnotes of this text, in the order of their numbers.
    footnotes: Vec<String>,
    /// The lines of this text that are folded, if it is the note itself and
    /// not something the note embeds, which does not fold.
    folded: Option<&'a HashSet<String>>,
    /// Folded lines that are drawn open, because what they hide was to be gone to.
    unfolded: Vec<String>,
    /// Where the arrows of the lines that fold go, as `Read` hands them on.
    arrows: Vec<(Rect, Range<usize>, bool)>,
}

/// Where a run of text was put: its first row and its last, which are one
/// row if it did not wrap.
struct Placed {
    first: Rect,
    last: Rect,
}

impl<'a, 'e> Reader<'a, 'e> {
    fn new(
        ui: &mut egui::Ui,
        text: &'a str,
        theme: &'a Theme,
        exists: &'a dyn Fn(&str) -> bool,
        folded: Option<&'a HashSet<String>>,
        embeds: &'a mut Embeds<'e>,
    ) -> Self {
        ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
        let body = theme.format(Style::default(), theme.size, theme.text);
        let row = ui.fonts(|f| f.row_height(&body.font_id));
        let (unfolded, arrows) = (Vec::new(), Vec::new());
        Self { text, theme, exists, row, hit: None, embeds, depth: 0, footnotes: Vec::new(), folded, unfolded, arrows }
    }
}

impl Reader<'_, '_> {
    /// Draw the text, block after block.
    fn blocks(&mut self, ui: &mut egui::Ui, goto: Option<usize>) {
        let (theme, row) = (self.theme, self.row);
        let lines = markdown::parse(self.text);
        // Footnotes get their numbers in the order they are first referred
        // to; one that nothing refers to, or that is not defined, has none.
        let defined = |id: &str| lines.iter().any(|line| matches!(&line.kind, Kind::Footnote(defined) if defined == id));
        for span in lines.iter().flat_map(|line| &line.spans) {
            if let Some(Link::Footnote(id)) = &span.link {
                if defined(id) && !self.footnotes.contains(id) {
                    self.footnotes.push(id.clone());
                }
            }
        }

        // What folds. A place to go to that a folded line hides is shown by
        // opening that fold, as the editor does; but not a footnote listed
        // at the end, which shows there whatever is folded. Footnotes keep
        // their numbers, and their place in that list, while what refers to
        // them is folded away.
        let folds = match self.folded {
            Some(_) => outline::folds_in(self.text, &lines),
            None => Vec::new(),
        };
        let listed = |at: usize| {
            let listed_here = |line: &&Line| matches!(&line.kind, Kind::Footnote(id) if self.footnotes.contains(id));
            lines.iter().filter(listed_here).any(|line| (line.range.start..=line.range.end).contains(&at))
        };
        if let Some(at) = goto.filter(|at| !listed(*at)) {
            let hides = |fold: &&Fold| self.is_folded(fold) && fold.hidden.start < at && at <= fold.hidden.end;
            self.unfolded = folds.iter().filter(hides).map(|fold| self.text[fold.line.clone()].to_owned()).collect();
        }
        let mut next_fold = 0;
        // Where what the last folded line drawn hides comes to an end.
        let mut hidden_to = None;

        let mut i = 0;
        // Whether the previous thing drawn was a paragraph gap (or nothing at all).
        let mut gap = true;
        while i < lines.len() {
            let line = &lines[i];
            // A line at a time, as the editor hides them: a fold can end
            // inside a block, which then shows from there on.
            if hidden_to.is_some_and(|end| line.range.end <= end) {
                i += 1;
                continue;
            }
            let block = &lines[i..i + lines[i..].iter().take_while(|l| l.block == line.block).count()];
            i += block.len();
            if goto.is_some_and(|at| (line.block.start..=line.block.end).contains(&at)) {
                ui.scroll_to_cursor(Some(egui::Align::TOP));
            }
            while folds.get(next_fold).is_some_and(|fold| fold.line.start < line.range.start) {
                next_fold += 1;
            }
            let fold = folds.get(next_fold).filter(|fold| fold.line == line.range);
            let folded = fold.is_some_and(|fold| self.is_folded(fold));
            if folded {
                hidden_to = fold.map(|fold| fold.hidden.end);
            }

            match &line.kind {
                Kind::Blank => {
                    if !gap {
                        ui.add_space(theme.size * 0.8);
                    }
                    gap = true;
                    continue;
                }
                Kind::Heading(level) => {
                    if !gap {
                        ui.add_space(theme.size * 0.4);
                    }
                    let left = ui.cursor().left();
                    let bold = Style { bold: true, ..Style::default() };
                    let placed = self.rich(ui, &line.spans, theme.heading_size(*level), bold, theme.text, None);
                    if let Some(fold) = fold {
                        self.foldable(ui, fold, folded, left, &placed);
                    }
                }
                Kind::Paragraph => self.paragraph(ui, line),
                // Listed at the end, if anything refers to it.
                Kind::Footnote(id) if self.footnotes.contains(id) => continue,
                Kind::Footnote(_) => self.plain(ui, &self.text[line.range.clone()], theme.format(Style::default(), theme.size, theme.muted)),
                Kind::Bullet | Kind::Numbered | Kind::Task { .. } => {
                    let (left, placed) = self.list_item(ui, line);
                    if let Some(fold) = fold {
                        self.foldable(ui, fold, folded, left, &placed);
                    }
                }
                Kind::Quote { callout, .. } => self.quote(ui, block, callout.as_deref()),
                Kind::Rule => {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::hover());
                    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, theme.rule));
                }
                Kind::Fence { .. } | Kind::Code => self.code_block(ui, block),
                Kind::FrontmatterFence | Kind::Frontmatter => self.properties(ui, block),
                Kind::TableRow { .. } | Kind::TableRule => self.table(ui, block),
            }
            gap = false;
        }

        if self.footnotes.is_empty() {
            return;
        }
        ui.add_space(theme.size);
        let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), row / 2.0), Sense::hover());
        ui.painter().hline(rule.x_range(), rule.center().y, Stroke::new(1.0, theme.rule));
        let small = theme.size * 0.875;
        for (number, id) in self.footnotes.clone().iter().enumerate() {
            let Some(line) = lines.iter().find(|line| matches!(&line.kind, Kind::Footnote(defined) if defined == id)) else { continue };
            if goto.is_some_and(|at| (line.range.start..=line.range.end).contains(&at)) {
                ui.scroll_to_cursor(Some(egui::Align::Center));
            }
            ui.horizontal_top(|ui| {
                self.plain(ui, &format!("{}.", number + 1), theme.format(Style::default(), small, theme.faint));
                self.rich(ui, &line.spans, small, Style::default(), theme.muted, None);
            });
        }
    }

    /// Whether `fold` is folded, here and now.
    fn is_folded(&self, fold: &Fold) -> bool {
        let line = &self.text[fold.line.clone()];
        self.folded.is_some_and(|folded| folded.contains(line)) && !self.unfolded.iter().any(|open| open == line)
    }

    /// A line that folds, drawn from `left` and `placed` as it is: its arrow
    /// goes in the margin beside its first row, and if it is folded, dots
    /// at its end say that there is more.
    fn foldable(&mut self, ui: &egui::Ui, fold: &Fold, folded: bool, left: f32, placed: &Placed) {
        self.arrows.push((live::arrow_beside(left, placed.first.y_range()), fold.line.clone(), folded));
        if folded {
            live::paint_folded(ui.painter(), placed.last.right_center(), self.theme);
        }
    }

    /// Draw what an embed names: the picture, or the note in a box of its own.
    fn embed(&mut self, ui: &mut egui::Ui, embed: &Link) {
        let Link::Embed { target, place, width } = embed else { return };
        let theme = self.theme;
        let quiet = theme.format(Style { italic: true, ..Style::default() }, theme.size * 0.875, theme.faint);
        if malachite::is_attachment(target) {
            match self.embeds.picture(ui.ctx(), target) {
                Some((texture, size)) => {
                    let size = fit(size, *width, ui.available_width());
                    ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(texture, size)));
                }
                None if malachite::is_picture(target) => self.plain(ui, &format!("{target} is not in this vault, or cannot be read"), quiet),
                None => self.plain(ui, &format!("{target}: files of this kind are not shown"), quiet),
            }
            return;
        }
        if self.depth >= DEPTH {
            return self.plain(ui, &format!("{target} (embedded too deep to show)"), quiet);
        }
        let Some((title, text)) = self.embeds.note(target, place.as_ref()) else {
            return self.plain(ui, &format!("{target} is not in this vault, or has no such part"), quiet);
        };
        let frame = egui::Frame::new().inner_margin(Margin { left: 14, top: 4, bottom: 4, right: 0 });
        let shown = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let bold = Style { bold: true, ..Style::default() };
            self.plain(ui, &title, theme.format(bold, theme.size * 0.875, theme.muted));
            let mut inner = Reader {
                text: &text,
                theme,
                exists: self.exists,
                row: self.row,
                hit: None,
                embeds: &mut *self.embeds,
                depth: self.depth + 1,
                footnotes: Vec::new(),
                folded: None,
                unfolded: Vec::new(),
                arrows: Vec::new(),
            };
            inner.blocks(ui, None);
            inner.hit
        });
        let bar = Stroke::new(2.0, theme.accent);
        ui.painter().vline(shown.response.rect.left() + 1.0, shown.response.rect.y_range(), bar);
        match shown.inner {
            // A link into "this note" means the note it is written in.
            Some(Hit::Link(Link::Note(name, place))) if name.is_empty() => self.hit = Some(Hit::Link(Link::Note(target.clone(), place))),
            Some(hit @ Hit::Link(_)) => self.hit = Some(hit),
            // A box ticked here would be ticked in the text of another note.
            Some(Hit::Checkbox(_)) | None => {}
        }
    }

    /// Draw inline content without its syntax, wrapped at `width`, or at
    /// the width there is room for if none is given. Links in it are clickable.
    fn rich(&mut self, ui: &mut egui::Ui, spans: &[Span], size: f32, base: Style, color: Color32, width: Option<f32>) -> Placed {
        let (mut job, links) = self.compose(spans, size, base, color);
        job.wrap.max_width = width.unwrap_or_else(|| ui.available_width());

        let galley = ui.fonts(|f| f.layout_job(job));
        let (rect, response) = self.place(ui, galley.clone());

        // Set after `place`, so the hand wins over the text cursor it shows.
        let hovered = response
            .hover_pos()
            .and_then(|pos| char_at(&galley, pos - rect.min))
            .and_then(|index| links.iter().find(|(range, _)| range.contains(&index)));
        if let Some((_, link)) = hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if response.clicked() {
                self.hit = Some(Hit::Link((*link).clone()));
            }
        }
        let first = galley.rows.first().map_or(rect, |row| row.rect.translate(rect.min.to_vec2()));
        let last = galley.rows.last().map_or(rect, |row| row.rect.translate(rect.min.to_vec2()));
        Placed { first, last }
    }

    /// Inline content without its syntax, ready to be laid out, and the
    /// links in it by their character range in what will show.
    fn compose<'s>(&self, spans: &'s [Span], size: f32, base: Style, color: Color32) -> (LayoutJob, Vec<(Range<usize>, &'s Link)>) {
        let theme = self.theme;
        let mut job = LayoutJob::default();
        let mut links: Vec<(Range<usize>, &Link)> = Vec::new();
        let mut chars = 0;
        for span in spans.iter().filter(|s| !s.marker && !s.style.comment) {
            let mut piece = std::borrow::Cow::Borrowed(&self.text[span.range.clone()]);
            let mut style = span.style;
            style.bold |= base.bold;
            style.strike |= base.strike;
            let mut format = theme.format(style, size, color);
            if let Some(link) = &span.link {
                format = theme.link(format, link, resolved(link, self.exists));
                // A footnote shows as its number, raised.
                let number = match link {
                    Link::Footnote(id) => self.footnotes.iter().position(|known| known == id),
                    _ => None,
                };
                if let Some(number) = number {
                    piece = std::borrow::Cow::Owned((number + 1).to_string());
                    format.valign = egui::Align::TOP;
                }
            }
            let len = piece.chars().count();
            if let Some(link) = &span.link {
                links.push((chars..chars + len, link));
            }
            job.append(&piece, 0.0, format);
            chars += len;
        }
        if job.sections.is_empty() {
            // Nothing visible still takes up a row.
            job.append("", 0.0, theme.format(base, size, color));
        }
        (job, links)
    }

    /// Draw literal text in one format.
    fn plain(&self, ui: &mut egui::Ui, text: &str, format: TextFormat) {
        self.draw(ui, LayoutJob::single_section(text.to_owned(), format));
    }

    /// Draw laid-out text that nothing in can be clicked.
    fn draw(&self, ui: &mut egui::Ui, mut job: LayoutJob) {
        job.wrap.max_width = ui.available_width();
        let galley = ui.fonts(|f| f.layout_job(job));
        self.place(ui, galley);
    }

    /// Put laid-out text into the ui so that it can be selected with the mouse
    /// and copied, the way egui's own labels can. A selection may run across
    /// several pieces of text: they are visited top to bottom every frame, on
    /// screen or not, so it also survives scrolling.
    fn place(&self, ui: &mut egui::Ui, galley: Arc<Galley>) -> (egui::Rect, egui::Response) {
        // Clicks and drags select; Tab should not stop at every paragraph.
        let mut sense = Sense::click_and_drag();
        sense -= Sense::FOCUSABLE;
        let (rect, response) = ui.allocate_exact_size(galley.size(), sense);
        LabelSelectionState::label_text_selection(ui, &response, rect.min, galley, self.theme.text, Stroke::NONE);
        (rect, response)
    }

    /// A line of text. What it embeds is drawn where it stands, with the
    /// text before it above and the text after it below.
    fn paragraph(&mut self, ui: &mut egui::Ui, line: &Line) {
        let (theme, spans) = (self.theme, &line.spans);
        let embedded = |span: &Span| matches!(span.link, Some(Link::Embed { .. }));
        let elements: Vec<&Range<usize>> = spans.iter().filter(|span| embedded(span)).map(|span| &span.element).collect();
        if elements.is_empty() {
            self.rich(ui, spans, theme.size, Style::default(), theme.text, None);
            return;
        }
        // Text between embeds is drawn only if there is any to see.
        let says_something = |run: &[Span]| run.iter().any(|span| !span.marker && !self.text[span.range.clone()].trim().is_empty());
        let (mut from, mut at) = (0, 0);
        while at < spans.len() {
            if !elements.contains(&&spans[at].element) {
                at += 1;
                continue;
            }
            if says_something(&spans[from..at]) {
                self.rich(ui, &spans[from..at], theme.size, Style::default(), theme.text, None);
            }
            let element = &spans[at].element;
            let end = at + spans[at..].iter().take_while(|span| span.element == *element).count();
            if let Some(link) = spans[at..end].iter().find_map(|span| span.link.as_ref()) {
                self.embed(ui, link);
            }
            (from, at) = (end, end);
        }
        if says_something(&spans[from..]) {
            self.rich(ui, &spans[from..], theme.size, Style::default(), theme.text, None);
        }
    }

    /// A list item, with its bullet, number or checkbox. Returns where the
    /// bullet starts, and where the item's text was put.
    fn list_item(&mut self, ui: &mut egui::Ui, line: &Line) -> (f32, Placed) {
        let theme = self.theme;
        // Nesting follows the indentation: a tab or four spaces per level.
        let indent: usize = self.text[line.range.start..line.marker.start]
            .chars()
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        let (mut style, mut color) = (Style::default(), theme.text);
        ui.horizontal_top(|ui| {
            ui.add_space(indent as f32 * theme.size * 0.45);
            let left = ui.cursor().left();
            let slot = vec2(theme.size * 1.1, self.row);
            match &line.kind {
                Kind::Task { done, checkbox } => {
                    let (rect, response) = ui.allocate_exact_size(slot, Sense::click());
                    theme.paint_checkbox(ui.painter(), rect.center(), *done);
                    if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.hit = Some(Hit::Checkbox(checkbox.clone()));
                    }
                    if *done {
                        style.strike = true;
                        color = theme.muted;
                    }
                }
                Kind::Numbered => {
                    let number = self.text[line.marker.clone()].trim_end();
                    self.plain(ui, number, theme.format(Style::default(), theme.size, theme.faint));
                }
                _ => {
                    let (rect, _) = ui.allocate_exact_size(slot, Sense::hover());
                    theme.paint_bullet(ui.painter(), rect.center());
                }
            }
            (left, self.rich(ui, &line.spans, theme.size, style, color, None))
        })
        .inner
    }

    /// A quote gets an accent bar; a callout its tinted box and title.
    fn quote(&mut self, ui: &mut egui::Ui, block: &[Line], callout: Option<&str>) {
        let theme = self.theme;
        let tint = callout.map(|kind| theme.callout(kind));
        let frame = match tint {
            Some(tint) => egui::Frame::new()
                .fill(theme.callout_fill(tint))
                .corner_radius(4.0)
                .inner_margin(Margin::symmetric(12, 8)),
            None => egui::Frame::new().inner_margin(Margin { left: 14, ..Margin::ZERO }),
        };
        let response = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for line in block {
                    match (&line.kind, tint, callout) {
                        (Kind::Quote { tag: Some(_), .. }, Some(tint), Some(kind)) => {
                            let bold = Style { bold: true, ..Style::default() };
                            if line.spans.is_empty() {
                                // No title: the type's name is shown.
                                let mut rest = kind.chars();
                                let first = rest.next().into_iter().flat_map(char::to_uppercase);
                                let name: String = first.chain(rest).collect();
                                self.plain(ui, &name, theme.format(bold, theme.size, tint));
                            } else {
                                self.rich(ui, &line.spans, theme.size, bold, tint, None);
                            }
                        }
                        _ => self.paragraph(ui, line),
                    }
                }
            })
            .response;
        if tint.is_none() {
            let bar = Stroke::new(2.0, theme.accent);
            ui.painter().vline(response.rect.left() + 1.0, response.rect.y_range(), bar);
        }
    }

    /// A code block, coloured by the language named on its opening fence.
    fn code_block(&mut self, ui: &mut egui::Ui, block: &[Line]) {
        let theme = self.theme;
        let language = block.first().map_or("", |fence| markdown::fence_language(&self.text[fence.range.clone()]));
        let mut code = block.iter().filter(|l| l.kind == Kind::Code);
        let range = code.next().map(|first| first.range.start..code.last().unwrap_or(first).range.end);
        let code = range.map_or(String::new(), |r| self.text[r].replace('\r', ""));

        let mut job = LayoutJob::default();
        let mut at = 0;
        for (range, token) in highlight(language, &code) {
            job.append(&code[at..range.start], 0.0, theme.code_token(None));
            job.append(&code[range.clone()], 0.0, theme.code_token(Some(token)));
            at = range.end;
        }
        job.append(&code[at..], 0.0, theme.code_token(None));

        egui::Frame::new().fill(theme.code_bg).corner_radius(4.0).inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.draw(ui, job);
        });
    }

    /// Frontmatter, shown as a list of the note's properties.
    fn properties(&mut self, ui: &mut egui::Ui, block: &[Line]) {
        let theme = self.theme;
        let border = Stroke::new(1.0, theme.rule);
        egui::Frame::new().stroke(border).corner_radius(4.0).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let bold = Style { bold: true, ..Style::default() };
            self.plain(ui, "Properties", theme.format(bold, theme.size * 0.875, theme.muted));
            for line in block.iter().filter(|l| l.kind == Kind::Frontmatter) {
                self.plain(ui, &self.text[line.range.clone()], theme.code(theme.muted));
            }
        });
    }

    fn table(&mut self, ui: &mut egui::Ui, block: &[Line]) {
        // Rows only form a table when a `|---|` line follows the header.
        if !matches!(block.get(1).map(|l| &l.kind), Some(Kind::TableRule)) {
            for line in block {
                self.paragraph(ui, line);
            }
            return;
        }
        let theme = self.theme;
        let rows: Vec<(Style, Vec<Vec<Span>>)> = block
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let Kind::TableRow { cells } = &line.kind else { return None };
                let style = Style { bold: index == 0, ..Style::default() };
                Some((style, cells.iter().map(|cell| markdown::inline(self.text, cell.clone())).collect()))
            })
            .collect();

        // How wide each column would like to be: as wide as its widest cell
        // written on one line.
        let mut wanted: Vec<f32> = Vec::new();
        for (style, cells) in &rows {
            for (column, spans) in cells.iter().enumerate() {
                let (job, _) = self.compose(spans, theme.size, *style, theme.text);
                let width = ui.fonts(|f| f.layout_job(job)).size().x;
                match wanted.get_mut(column) {
                    Some(widest) => *widest = widest.max(width),
                    None => wanted.push(width),
                }
            }
        }
        let room = ui.available_width() - TABLE_SPACING.x * wanted.len().saturating_sub(1) as f32;
        let widths = fit_columns(&wanted, room);

        egui::Grid::new(("table", block[0].range.start)).striped(true).min_col_width(0.0).spacing(TABLE_SPACING).show(ui, |ui| {
            for (style, cells) in &rows {
                for (spans, width) in cells.iter().zip(&widths) {
                    self.rich(ui, spans, theme.size, *style, theme.text, Some(*width));
                }
                ui.end_row();
            }
        });
    }
}

/// The space between the cells of a table.
const TABLE_SPACING: egui::Vec2 = vec2(24.0, 6.0);

/// The narrowest a column of a table is squeezed to.
const MIN_COLUMN: f32 = 48.0;

/// Widths for the columns of a table that has `room` across, given the
/// width each would take with its cells on one line. A table that fits is
/// left as it is. Otherwise, as a browser does it, the narrow columns keep
/// their width and the wide ones share what is left, their text wrapping, so
/// that no cell runs out of the page.
fn fit_columns(wanted: &[f32], room: f32) -> Vec<f32> {
    let mut widths = wanted.to_vec();
    // The columns still to be given a width, and the room left for them.
    let mut open: Vec<usize> = (0..wanted.len()).collect();
    let mut left = room;
    while !open.is_empty() {
        let share = (left / open.len() as f32).max(MIN_COLUMN);
        // Columns that ask for no more than an equal share get what they ask.
        let (fitting, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&column| wanted[column] <= share);
        if fitting.is_empty() {
            for column in wide {
                widths[column] = share;
            }
            break;
        }
        left -= fitting.iter().map(|&column| wanted[column]).sum::<f32>();
        open = wide;
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::fit_columns;

    #[test]
    fn a_table_that_fits_keeps_its_columns() {
        assert_eq!(fit_columns(&[100.0, 200.0, 50.0], 400.0), [100.0, 200.0, 50.0]);
        assert!(fit_columns(&[], 400.0).is_empty());
    }

    #[test]
    fn wide_columns_share_what_the_narrow_ones_leave() {
        assert_eq!(fit_columns(&[60.0, 900.0, 500.0], 460.0), [60.0, 200.0, 200.0]);
        assert_eq!(fit_columns(&[300.0, 900.0], 500.0), [250.0, 250.0], "a column wider than its share gives way too");
        let widths = fit_columns(&[60.0, 900.0, 180.0], 460.0);
        assert_eq!(widths, [60.0, 220.0, 180.0]);
        assert!(widths.iter().sum::<f32>() <= 460.0);
    }

    #[test]
    fn no_column_is_squeezed_to_nothing() {
        assert!(fit_columns(&[500.0; 20], 300.0).iter().all(|width| *width == 48.0));
    }
}
