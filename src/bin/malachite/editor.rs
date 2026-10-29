//! The middle of the window: the open note, as an editor or as Reading view.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::time::Instant;

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, Key, Modifiers};
use malachite::editing::{self, Edit, Highlighter};
use malachite::outline;

use malachite::markdown::Link;

use crate::autoscroll::Autoscroll;
use crate::embed::{fit, Embeds};
use crate::commands::{self, EditFn, Run};
use crate::live::{self, Hit, Offsets};
use crate::theme::Theme;
use crate::{reading, Action, App, Focus, Mode};

/// The colours the note's right-click menu offers for a highlight. The
/// first is the yellow of a plain `==highlight==`, the rest six colours
/// to go with it, in the order of the rainbow.
pub(crate) const HIGHLIGHTS: [(&str, Highlighter, [u8; 3]); 7] = [
    ("Yellow", Highlighter::Yellow, [255, 208, 0]),
    ("Red", Highlighter::Color([233, 49, 71]), [233, 49, 71]),
    ("Orange", Highlighter::Color([236, 117, 0]), [236, 117, 0]),
    ("Green", Highlighter::Color([8, 185, 78]), [8, 185, 78]),
    ("Cyan", Highlighter::Color([0, 191, 188]), [0, 191, 188]),
    ("Blue", Highlighter::Color([8, 109, 221]), [8, 109, 221]),
    ("Purple", Highlighter::Color([120, 82, 238]), [120, 82, 238]),
];

/// How tall an embedded note is taken to be before it has been drawn once.
const EMBED_GUESS: f32 = 60.0;

/// How far past the top and the bottom of its row the caret reaches: what
/// egui gives the caret of a text field.
const CARET_REACH: f32 = 1.5;

/// Space kept free beside the text column, so block backgrounds do not run
/// under the scroll bar when the window is narrow.
const GUTTER: f32 = 12.0;

impl App {
    pub(crate) fn editor(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let theme = self.theme(ctx);
        // Drawings are only made in Reading view.
        if self.mode != Mode::Reading || self.open.is_none() {
            self.ink.on = false;
        }
        // The note sits on its own shade, a page among the panels.
        let page = egui::Frame::central_panel(&ctx.style()).fill(theme.page);
        egui::CentralPanel::default().frame(page).show(ctx, |ui| {
            let (Some(open), Some(file)) = (self.open.clone(), self.open_path.clone()) else {
                self.autoscroll = None;
                self.pending_edit = None;
                self.pending_insert = None;
                self.find = None;
                self.jump = None;
                self.caret = None;
                self.header(ui, None, actions);
                ui.separator();
                ui.centered_and_justified(|ui| {
                    ui.weak("No note open. Pick one from the list, press Ctrl+O to find one, or Ctrl+N to start one.");
                });
                return;
            };
            self.header(ui, Some(&open), actions);
            ui.separator();
            self.find_bar(ui, actions);
            // Laid out above the note, not over it, so it never covers the panels.
            if self.drawing() {
                self.tool_strip(ui, &theme);
            }

            // What a link can call a note: its title, or one of its aliases.
            let index = self.index();
            // An empty title is a `[[#Heading]]` link into the note itself.
            let exists = |title: &str| title.is_empty() || index.names.contains(&title.to_lowercase());

            // A click of the scroll wheel over the note starts autoscroll.
            let view = ui.available_rect_before_wrap();
            self.over_note = ui.rect_contains_pointer(view);
            let glide = Autoscroll::update(&mut self.autoscroll, ui, view, &mut self.wheel);

            // Each note keeps its own place: by file, since titles can repeat across folders.
            let scroll = egui::ScrollArea::vertical().id_salt(&file).auto_shrink(false);
            let scrolled = scroll.show(ui, |ui| {
                if glide != 0.0 {
                    // A positive delta moves the content down, which is scrolling up.
                    ui.scroll_with_delta_animation(egui::vec2(0.0, -glide), egui::style::ScrollAnimation::none());
                }
                let area = ui.available_rect_before_wrap();
                // The "readable line length": the column of text is kept
                // to the width the settings give it, and centred.
                let limit = if self.look.line_width > 0.0 { self.look.line_width } else { f32::INFINITY };
                let width = (area.width() - 2.0 * GUTTER).min(limit).max(GUTTER);
                #[cfg(test)]
                {
                    self.column_width = width;
                }
                let left = (area.center().x - width / 2.0).floor();
                let column = egui::Rect::from_min_size(egui::pos2(left, area.top()), egui::vec2(width, area.height()));
                ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                    ui.set_width(width);
                    ui.add_space(GUTTER);
                    // A hidden title still comes out to be renamed (F2, a new note).
                    let renaming = self.focus == Some(Focus::Title) || ui.memory(|m| m.has_focus(egui::Id::new("title")));
                    if self.look.show_title || renaming {
                        self.title(ui, &open, &theme, actions);
                        ui.add_space(GUTTER / 2.0);
                    }
                    #[cfg(test)]
                    {
                        self.content_top = ui.cursor().top();
                    }
                    // The editor fills what is left of the window below the name.
                    let rest = area.bottom() - ui.cursor().top();
                    match self.mode {
                        Mode::Reading => {
                            // There is nothing to edit in a finished note,
                            // and no caret to send anywhere: a place to go
                            // to is only scrolled to, and opened if it is
                            // folded away. A caret that folding has moved
                            // out of a fold waits for the editor.
                            self.pending_edit = None;
                            self.pending_insert = None;
                            let goto = self.jump.take().map(|range| range.start);
                            let note = self.digest();
                            let nothing = HashSet::new();
                            let folded = self.folds.get(&file).unwrap_or(&nothing);
                            let mut embeds = Embeds { pictures: &mut self.pictures, vault: &self.vault, notes: &self.notes, version: self.version, scanned: self.scanned };
                            // Kept for the highlighters' strokes, which go under the text.
                            let under = ui.painter().add(egui::Shape::Noop);
                            let marks = self.marks.iter().filter(|range| range.end <= self.buffer.len()).enumerate();
                            let marks: Vec<_> = marks.map(|(index, range)| (range.clone(), theme.find_mark(index == self.mark))).collect();
                            let read = reading::show(ui, &note, &theme, &exists, goto, folded, &marks, &mut embeds);
                            if let Some(folded) = self.folds.get_mut(&file) {
                                folded.retain(|line| !read.unfolded.contains(line));
                            }
                            let id = ui.make_persistent_id(("reading", &file));
                            self.fold_arrows(ui, id, read.arrows, &theme);
                            // Over the text, so that while drawing a press on it draws.
                            self.ink_layer(ui, &file, &read.blocks, &theme, under);
                            read.hit.filter(|_| !self.drawing())
                        }
                        mode => self.edit(ui, &file, mode == Mode::Source, rest, &theme, &exists),
                    }
                })
                .inner
            });
            #[cfg(test)]
            {
                self.scroll_offset = scrolled.state.offset.y;
            }
            if let Some(hit) = scrolled.inner {
                self.on_hit(ctx, hit, actions);
            }
        });
    }

    /// The Markdown editor, in Live Preview or Source mode. Returns the link
    /// or checkbox that was clicked, if any.
    fn edit(
        &mut self,
        ui: &mut egui::Ui,
        file: &Path,
        source: bool,
        min_height: f32,
        theme: &Theme,
        exists: &dyn Fn(&str) -> bool,
    ) -> Option<Hit> {
        // Keyed by the note's file so cursor and undo history never leak between notes.
        let id = ui.make_persistent_id(("editor", file));
        #[cfg(test)]
        {
            self.editor_id = Some(id);
        }
        if self.focus == Some(Focus::Editor) {
            ui.memory_mut(|m| m.request_focus(id));
            // A note that has not been edited yet starts with the caret at its
            // end, where writing carries on, not in front of its first line.
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            if state.cursor.char_range().is_none() {
                let end = CCursor::new(self.buffer.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), id);
            }
        }
        // Somewhere to go: a match of the find bar, a line a search found, or
        // the heading that a link or the outline names.
        let arrival = self.jump.take().filter(|range| self.buffer.get(range.clone()).is_some());
        // Or only somewhere for the caret to be, without going to look at it.
        let parked = self.caret.take().filter(|at| self.buffer.is_char_boundary(*at)).map(|at| at..at);
        if let Some(range) = arrival.as_ref().or(parked.as_ref()) {
            let offsets = Offsets::new(&self.buffer);
            let at = |byte: usize| CCursor::new(offsets.char_at(byte));
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(at(range.start), at(range.end))));
            state.store(ui.ctx(), id);
        }
        if let Some(run) = self.pending_edit.take_if(|_| !self.under_modal) {
            // A command from the palette, which had the keyboard until now.
            ui.memory_mut(|m| m.request_focus(id));
            self.run_edit(ui, id, run);
        }
        if let Some(text) = self.pending_insert.take_if(|_| !self.under_modal) {
            // A template, picked in a list that had the keyboard until now.
            ui.memory_mut(|m| m.request_focus(id));
            let selection = self.selection(ui, id).unwrap_or(self.buffer.len()..self.buffer.len());
            self.set_text(ui, id, editing::insert_text(&self.buffer, selection, &text));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        if focused {
            self.typing_keys(ui, id);
        }

        // The preview is laid out around the cursor as it was after the last frame.
        let before = egui::TextEdit::load_state(ui.ctx(), id).and_then(|state| state.cursor.char_range());
        let selection = before.filter(|_| focused).map(|range| {
            let [start, end] = range.sorted();
            start.index..end.index
        });
        let offsets = Offsets::new(&self.buffer);
        let cursor = selection.as_ref().map(|s| offsets.byte_at(s.start)..offsets.byte_at(s.end));
        self.selection = before.map(|range| {
            let [start, end] = range.sorted();
            offsets.byte_at(start.index)..offsets.byte_at(end.index)
        });

        // What the caret is in, or is being sent to, cannot stay folded away.
        for touched in [arrival.as_ref(), cursor.as_ref()].into_iter().flatten() {
            self.unfold_around(file, touched);
        }
        // What the note's embeds will need room for. An embedded note is as
        // tall as it was last drawn, or a guess the first time.
        let digest = self.digest();
        let mut sizes = std::collections::HashMap::new();
        for embed in &digest.embeds {
            let Link::Embed { target, place, .. } = embed else { continue };
            let mut embeds = Embeds { pictures: &mut self.pictures, vault: &self.vault, notes: &self.notes, version: self.version, scanned: self.scanned };
            if malachite::is_attachment(target) {
                if let Some((_, size)) = embeds.picture(ui.ctx(), target) {
                    sizes.insert(embed.clone(), [size.x, size.y]);
                }
            } else if embeds.note(target, place.as_ref()).is_some() {
                sizes.insert(embed.clone(), [0.0, self.embed_heights.get(embed).copied().unwrap_or(EMBED_GUESS)]);
            }
        }
        let sizes = std::rc::Rc::new(sizes);
        let tables = std::rc::Rc::new(self.table_heights.clone());

        let nothing = HashSet::new();
        let folded = self.folds.get(file).unwrap_or(&nothing);

        let mut preview = None;
        let (kept, notes) = (&mut self.layout, self.version);
        // The text the cursor was taken in; a key the field has just taken changes it.
        let typed_into = self.buffer.clone();
        let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let blocks = live::Blocks { width: wrap_width, sizes: sizes.clone(), tables: tables.clone() };
            let cursor = cursor.as_ref().map(|c| if text == typed_into { c.clone() } else { live::follow(&typed_into, text, c) });
            let (mut job, built) = live::build_kept(kept, text, cursor.as_ref(), source, theme, exists, notes, folded, &blocks);
            job.wrap.max_width = wrap_width;
            preview = Some(built);
            ui.fonts(|f| f.layout_job(job))
        };
        // Reserved first so block backgrounds end up underneath the text.
        let background = ui.painter().add(egui::Shape::Noop);
        // The text field draws its caret from a little above its row to a
        // little below it, and the row of a line that embeds something goes
        // on down past the line's text, for as far as what is embedded. So
        // the field is shown without a caret, and the caret is drawn further
        // on, where it can be kept to the text.
        let caret_stroke = std::mem::take(&mut ui.visuals_mut().text_cursor.stroke);
        let out = egui::TextEdit::multiline(&mut self.buffer)
            .id(id)
            .frame(false)
            // Flush with the column, so the text lines up with the note's name
            // above it and sits where Reading view puts it.
            .margin(egui::Margin::ZERO)
            .lock_focus(true)
            .desired_width(f32::INFINITY)
            .min_size(egui::vec2(0.0, min_height))
            .layouter(&mut layouter)
            .show(ui);
        ui.visuals_mut().text_cursor.stroke = caret_stroke;
        let preview = preview?;

        // egui moves the caret for a press of any button. A right click is
        // for the menu, which acts on what is selected: leave that as it was.
        if out.response.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
            let mut state = out.state.clone();
            state.cursor.set_char_range(before);
            state.store(ui.ctx(), id);
        }
        #[cfg(test)]
        self.menu_spots.clear();
        out.response.context_menu(|ui| self.highlight_menu(ui, id));

        if out.response.changed() {
            self.dirty = true;
            self.last_edit = Instant::now();
            // The panels were drawn before this, from the text as it was.
            ui.ctx().request_repaint();
        }
        let offsets = Offsets::new(&self.buffer);
        let width = out.response.rect.x_range();
        let marks: Vec<_> = self.marks.iter().filter(|range| range.end <= self.buffer.len()).cloned().enumerate().collect();
        let marks: Vec<_> = marks.into_iter().map(|(index, range)| (range, theme.find_mark(index == self.mark))).collect();
        preview.paint(ui.painter(), background, &out.galley, out.galley_pos, width, &offsets, theme, &marks);
        if let Some(range) = arrival {
            let caret = out.galley.pos_from_ccursor(CCursor::new(offsets.char_at(range.start)));
            // Something found is shown among what is around it; a place that
            // was asked for by name, a heading, goes to the top to be read from.
            let align = if range.is_empty() { egui::Align::TOP } else { egui::Align::Center };
            ui.scroll_to_rect(caret.translate(out.galley_pos.to_vec2()), Some(align));
        }
        #[cfg(test)]
        {
            self.hot_spots = preview.hot_spots(&out.galley, out.galley_pos, &offsets);
        }

        // What the note embeds, in the room kept for it under its line.
        let rooms = preview.embeds(&out.galley, out.galley_pos, &offsets);
        let caret = self.caret_rect(ui, id, &out, before, &rooms);
        let mut embedded_hit = None;
        for live::Room { corner, height, row, text_end, shown } in rooms {
            let room = egui::Rect::from_min_size(corner, egui::vec2(width.span(), height));
            if !ui.is_rect_visible(room) {
                continue;
            }
            // The room is part of the line's row to the text field, which
            // has drawn its selection down the whole of that row, and on past
            // the end of the line by half the row's height, its way of
            // showing a selected line break. Only the line's text is
            // selected: paint the rest clean again, right across the page.
            let across = ui.clip_rect().x_range();
            let under = egui::Rect::from_x_y_ranges(across, corner.y..=row.max);
            ui.painter().rect_filled(under, 0.0, theme.page);
            // Beside the text, a line break is given the width it has on any other line.
            let beside = egui::Rect::from_x_y_ranges(text_end + theme.size / 2.0..=across.max, row.min..=corner.y);
            if beside.is_positive() {
                ui.painter().rect_filled(beside, 0.0, theme.page);
            }
            let mut embeds = Embeds { pictures: &mut self.pictures, vault: &self.vault, notes: &self.notes, version: self.version, scanned: self.scanned };
            let embed = match shown {
                live::Shown::Embed(embed) => embed,
                live::Shown::Table(range) => {
                    let Some(table) = self.buffer.get(range) else { continue };
                    let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(room).id_salt(("table", corner.y as i64)));
                    inside.set_clip_rect(room.intersect(ui.clip_rect()));
                    // A click on the grid goes through to the text, and shows the table's lines.
                    inside.style_mut().interaction.selectable_labels = false;
                    embedded_hit = reading::table(&mut inside, table, theme, exists, &mut embeds).or(embedded_hit);
                    let used = inside.min_rect().height();
                    if (used - height).abs() > 0.5 {
                        self.table_heights.insert(table.to_owned(), used);
                        ui.ctx().request_repaint();
                    }
                    continue;
                }
            };
            let Link::Embed { target, width: wanted, .. } = &embed else { continue };
            if malachite::is_attachment(target) {
                if let Some((texture, size)) = embeds.picture(ui.ctx(), target) {
                    let shown = egui::Rect::from_min_size(corner, fit(size, *wanted, room.width()));
                    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture, shown, whole, egui::Color32::WHITE);
                }
                continue;
            }
            // A note: drawn as Reading view draws it, and measured, so that
            // the room is right the next time.
            let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(room).id_salt(("embed", corner.y as i64)));
            inside.set_clip_rect(room.intersect(ui.clip_rect()));
            embedded_hit = reading::embed(&mut inside, &embed, theme, exists, &mut embeds).or(embedded_hit);
            let used = inside.min_rect().height();
            if (used - height).abs() > 0.5 {
                self.embed_heights.insert(embed, used);
                ui.ctx().request_repaint();
            }
        }
        // Over what has just been painted clean.
        if let Some(caret) = caret {
            let painter = ui.painter_at(out.text_clip_rect.expand(1.0));
            let since = ui.input(|i| i.time) - self.caret_since;
            egui::text_selection::visuals::paint_text_cursor(ui, &painter, caret, since);
        }
        if embedded_hit.is_some() {
            return embedded_hit;
        }

        self.fold_arrows(ui, id, preview.arrows(&out.galley, out.galley_pos, &offsets), theme);

        // If the cursor moved, what is hidden and shown changes: draw again.
        if out.cursor_range.map(|range| range.as_sorted_char_range()) != selection {
            ui.ctx().request_repaint();
        }

        let pointer = out.response.hover_pos()?;
        let hit = preview.hit(&out.galley, out.galley_pos, &offsets, pointer).cloned();
        if hit.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // Only a press on a link or checkbox is more than ordinary editing.
        let hit = hit.filter(|_| ui.input(|i| i.pointer.primary_pressed()))?;
        // The press also moved the text cursor here, which would un-hide the
        // syntax under the pointer. Put the cursor back where it was.
        let mut state = out.state;
        state.cursor.set_char_range(before);
        state.store(ui.ctx(), id);
        if !focused {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        ui.ctx().stop_dragging();
        Some(hit)
    }

    /// The arrows that fold, in the editor and in Reading view alike: beside
    /// a folded line always, and beside the line the mouse is on. `arrows`
    /// says where each line that folds has its arrow, and whether it is
    /// folded; a click on one folds its line or unfolds it. They are drawn
    /// after the note, so that a click on one is theirs, and their ids are
    /// made from `id`, the view's.
    fn fold_arrows(&mut self, ui: &egui::Ui, id: egui::Id, arrows: Vec<(egui::Rect, Range<usize>, bool)>, theme: &Theme) {
        #[cfg(test)]
        {
            self.fold_spots = arrows.iter().map(|(rect, line, _)| (rect.center(), self.buffer[line.clone()].to_owned())).collect();
        }
        let mouse = ui.input(|i| i.pointer.hover_pos()).filter(|pos| ui.clip_rect().contains(*pos));
        let mut toggled = None;
        for (rect, line, folded) in arrows {
            let pointed = mouse.is_some_and(|pos| rect.y_range().contains(pos.y));
            if !(folded || pointed) || !ui.is_rect_visible(rect) {
                continue;
            }
            let arrow = ui.interact(rect, id.with(("fold", line.start)), egui::Sense::click());
            live::paint_arrow(ui.painter(), rect, folded, if arrow.hovered() { theme.text } else { theme.faint });
            let hint = if folded { "Unfold" } else { "Fold" };
            if arrow.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint).clicked() {
                toggled = Some(line);
            }
        }
        if let Some(line) = toggled {
            self.toggle_fold(line);
        }
    }

    /// Where the caret is to be drawn, if the editor has one to show: from a
    /// little above its row to a little below it, as a text field draws its
    /// own, but on a line with something embedded under it only as far down
    /// as the line's text goes. Also notes when the caret was last moved,
    /// which is what its blinking is timed from.
    fn caret_rect(
        &mut self,
        ui: &egui::Ui,
        id: egui::Id,
        out: &egui::text_edit::TextEditOutput,
        before: Option<CCursorRange>,
        rooms: &[live::Room],
    ) -> Option<egui::Rect> {
        let response = &out.response;
        let touched = response.changed() || response.gained_focus() || response.is_pointer_button_down_on();
        if touched || out.state.cursor.char_range() != before {
            self.caret_since = ui.input(|i| i.time);
        }
        // Only in a window that can be typed in, as with any text field.
        if !ui.memory(|m| m.has_focus(id)) || !ui.input(|i| i.focused) {
            return None;
        }
        let range = out.state.cursor.range(&out.galley)?;
        let row = out.galley.pos_from_cursor(&range.primary).translate(out.galley_pos.to_vec2());
        let bottom = match rooms.iter().find(|room| room.row.contains(row.center().y)) {
            // The caret has only just come to this line, whose text is not
            // showing yet: it will be in the next frame.
            Some(room) if room.corner.y <= row.top() => return None,
            // Clear of what is embedded, and about as tall as on other lines.
            Some(room) => room.corner.y - 2.0 * CARET_REACH,
            None => row.bottom() + CARET_REACH,
        };
        Some(egui::Rect::from_x_y_ranges(row.x_range(), row.top() - CARET_REACH..=bottom))
    }

    /// Keys that write Markdown for you: the hotkeys of the editing commands
    /// (Ctrl+B for bold and the rest), brackets and quotes that close
    /// themselves, Enter carrying a list on, Tab and Shift+Tab nesting and
    /// un-nesting a list item. They are taken before the text field sees them.
    fn typing_keys(&mut self, ui: &egui::Ui, id: egui::Id) {
        if let Some(command) = commands::pressed(ui.ctx(), |command| matches!(command.run, Run::Edit(_))) {
            let Run::Edit(run) = command.run else { return };
            return self.run_edit(ui, id, run);
        }
        let Some(selection) = self.selection(ui, id) else { return };
        let text = self.buffer.as_str();

        let edit = ui.input_mut(|i| {
            // One typed character on its own: more than that is a paste or
            // an input method, which know what they want.
            let mut typed = i.events.iter().enumerate().filter_map(|(at, event)| match event {
                egui::Event::Text(text) => Some((at, text.as_str())),
                _ => None,
            });
            if let (Some((at, typed)), None) = (typed.next(), typed.next()) {
                let mut chars = typed.chars();
                if let (Some(typed), None) = (chars.next(), chars.next()) {
                    if let Some(edit) = editing::pair(text, selection.clone(), typed) {
                        i.events.remove(at);
                        return Some(edit);
                    }
                }
            }
            if !selection.is_empty() {
                return None;
            }
            // These keys only do something special in a list; anywhere else
            // they are left for the text field to handle as usual.
            if i.key_pressed(Key::Enter) && i.modifiers.is_none() {
                let edit = editing::continue_list(text, selection.start)?;
                i.consume_key(Modifiers::NONE, Key::Enter);
                return Some(edit);
            }
            // A word at a time: the text field's own idea of a word is Latin
            // letters only, and so it took any other script, line breaks and
            // all, back to the last of them, which could be the whole note.
            // By the modifiers the key came with, as the text field goes by.
            let by_word = |modifiers: &Modifiers| (modifiers.ctrl || modifiers.alt) && !modifiers.shift && !modifiers.mac_cmd;
            for (key, forward) in [(Key::Backspace, false), (Key::Delete, true)] {
                let word = |event: &egui::Event| matches!(event, egui::Event::Key { key: pressed, pressed: true, modifiers, .. } if *pressed == key && by_word(modifiers));
                if i.events.iter().any(word) {
                    let edit = editing::delete_word(text, selection.start, forward)?;
                    i.events.retain(|event| !word(event));
                    return Some(edit);
                }
            }
            if i.key_pressed(Key::Tab) && !i.modifiers.command && !i.modifiers.alt {
                let outdent = i.modifiers.shift;
                let edit = editing::indent_item(text, selection.start, outdent)?;
                i.consume_key(if outdent { Modifiers::SHIFT } else { Modifiers::NONE }, Key::Tab);
                return Some(edit);
            }
            None
        });
        if let Some(edit) = edit {
            self.set_text(ui, id, edit);
        }
    }

    /// The selection in the editor, in bytes of the note, if it has ever had one.
    fn selection(&self, ui: &egui::Ui, id: egui::Id) -> Option<std::ops::Range<usize>> {
        let range = egui::TextEdit::load_state(ui.ctx(), id)?.cursor.char_range()?;
        let offsets = Offsets::new(&self.buffer);
        let [start, end] = range.sorted();
        Some(offsets.byte_at(start.index)..offsets.byte_at(end.index))
    }

    /// Carry out an editing command on the selection. A note that has not
    /// been clicked in yet has its caret at the end.
    fn run_edit(&mut self, ui: &egui::Ui, id: egui::Id, run: EditFn) {
        let selection = self.selection(ui, id).unwrap_or(self.buffer.len()..self.buffer.len());
        if let Some(edit) = run(&self.buffer, selection) {
            self.set_text(ui, id, edit);
        }
    }

    /// The note's right-click menu: highlight what is selected in one of a
    /// few colours or in a colour of one's own, or take the highlight off.
    /// A row of colours with a custom one at the end is the usual way to
    /// let a colour be chosen.
    fn highlight_menu(&mut self, ui: &mut egui::Ui, id: egui::Id) {
        let mut picked = None;
        ui.weak("Highlight");
        ui.horizontal(|ui| {
            for (name, to, color) in HIGHLIGHTS {
                if self.swatch(ui, name, color) {
                    picked = Some(to);
                }
            }
            // The colour picked last time, ready to use again.
            if self.swatch(ui, "Your colour", self.highlight) {
                picked = Some(Highlighter::Color(self.highlight));
            }
        });
        ui.menu_button("Pick a colour…", |ui| {
            let [r, g, b] = self.highlight;
            let mut color = egui::Color32::from_rgb(r, g, b);
            egui::color_picker::color_picker_color32(ui, &mut color, egui::color_picker::Alpha::Opaque);
            self.highlight = [color.r(), color.g(), color.b()];
            if ui.button("Highlight in this colour").clicked() {
                picked = Some(Highlighter::Color(self.highlight));
            }
        });
        ui.separator();
        let remove = ui.button("Remove highlight");
        #[cfg(test)]
        self.menu_spots.push(("Remove highlight".to_owned(), remove.rect.center()));
        if remove.clicked() {
            picked = Some(Highlighter::Off);
        }

        let Some(to) = picked else { return };
        let selection = self.selection(ui, id).unwrap_or(self.buffer.len()..self.buffer.len());
        match editing::set_highlight(&self.buffer, selection, to) {
            Some(edit) => self.set_text(ui, id, edit),
            None if to == Highlighter::Off => self.info("There is no highlight here to remove"),
            None => self.info("A highlight cannot run over several lines"),
        }
        // Back to the note, with what was highlighted still selected.
        ui.memory_mut(|m| m.request_focus(id));
        ui.close_menu();
    }

    /// A square of `color` to click in the highlight menu. Whether it was clicked.
    fn swatch(&mut self, ui: &mut egui::Ui, name: &str, [r, g, b]: [u8; 3]) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
        #[cfg(test)]
        self.menu_spots.push((name.to_owned(), rect.center()));
        let square = if response.hovered() { rect } else { rect.shrink(2.0) };
        ui.painter().rect_filled(square, 4.0, egui::Color32::from_rgb(r, g, b));
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(name).clicked()
    }

    /// Fold what is under `line` of the open note, a heading or a list item,
    /// or bring it back if it is folded.
    ///
    /// A fold is remembered by what its line says, not by where it is: text
    /// typed anywhere else in the note leaves it alone, and so does moving
    /// the line. Rewording the line opens it.
    pub(crate) fn toggle_fold(&mut self, line: Range<usize>) {
        let (Some(file), Some(key)) = (self.open_path.clone(), self.buffer.get(line).map(str::to_owned)) else { return };
        let folded = self.folds.entry(file).or_default();
        if !folded.remove(&key) {
            folded.insert(key);
            self.leave_folds();
        }
    }

    /// Fold or unfold the line the caret is on, if it is one that folds.
    pub(crate) fn toggle_fold_at_caret(&mut self) {
        let Some(caret) = self.selection.as_ref().map(|selection| selection.start) else { return };
        let on_line = |fold: &outline::Fold| fold.line.start <= caret && caret <= fold.line.end;
        if let Some(fold) = outline::folds(&self.buffer).into_iter().find(on_line) {
            self.toggle_fold(fold.line);
        }
    }

    /// Fold every heading and list of the open note, or unfold them all.
    pub(crate) fn fold_all(&mut self, fold: bool) {
        let Some(file) = self.open_path.clone() else { return };
        if fold {
            let lines = outline::folds(&self.buffer).into_iter().map(|fold| self.buffer[fold.line].to_owned());
            self.folds.insert(file, lines.collect());
            self.leave_folds();
        } else {
            self.folds.remove(&file);
        }
    }

    /// The closed folds of the open note, outermost first.
    fn closed_folds(&self, file: &Path) -> Vec<outline::Fold> {
        let Some(folded) = self.folds.get(file).filter(|folded| !folded.is_empty()) else { return Vec::new() };
        outline::folds(&self.buffer).into_iter().filter(|fold| folded.contains(&self.buffer[fold.line.clone()])).collect()
    }

    /// A caret inside what has just been folded would open it again at once:
    /// it moves to the end of the line that is left showing.
    fn leave_folds(&mut self) {
        let (Some(file), Some(selection)) = (self.open_path.clone(), self.selection.clone()) else { return };
        let around = |fold: &outline::Fold| selection.end > fold.hidden.start && selection.start <= fold.hidden.end;
        if let Some(fold) = self.closed_folds(&file).into_iter().find(around) {
            self.caret = Some(fold.line.end);
        }
    }

    /// Open the folds that hide any of `range`, where the caret is or is going.
    fn unfold_around(&mut self, file: &Path, range: &Range<usize>) {
        let inside = |fold: &outline::Fold| range.end > fold.hidden.start && range.start <= fold.hidden.end;
        let opened: Vec<String> = self.closed_folds(file).into_iter().filter(inside).map(|fold| self.buffer[fold.line].to_owned()).collect();
        if let Some(folded) = self.folds.get_mut(file) {
            folded.retain(|line| !opened.contains(line));
        }
    }

    /// Replace the note's text and selection with the outcome of an edit.
    fn set_text(&mut self, ui: &egui::Ui, id: egui::Id, edit: Edit) {
        let offsets = Offsets::new(&edit.text);
        let at = |byte: usize| CCursor::new(offsets.char_at(byte));
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(at(edit.selection.start), at(edit.selection.end))));
        state.store(ui.ctx(), id);
        self.buffer = edit.text;
        self.dirty = true;
        self.last_edit = Instant::now();
    }
}
