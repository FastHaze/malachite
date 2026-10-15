//! Find and replace in the open note (Ctrl+F and Ctrl+H), in a bar above it.
//!
//! Every place the text is found is marked in the note, and one of them is
//! the current one: selected in the editor and scrolled into view. Enter goes
//! on to the next and Shift+Enter back, round and round. Esc closes the bar
//! and leaves the keyboard in the note with the current match selected.
//!
//! It works on the note's source, so it is for the two editing views; asked
//! for in Reading view, it changes to the editing view first.
//!
//! In the graph there is no text to look through, and Ctrl+F looks for a
//! note instead, by its name (`GraphFind`): the same bar, with the same keys.
//! What it finds is all the graph draws while it is up, gathered together
//! (`GraphView::search`), and Enter gives each of them in turn the graph's
//! focus, as a click on it would.

use std::ops::Range;
use std::time::Instant;

use eframe::egui::{self, Key, Modifiers};
use malachite::search::occurrences;

use crate::panels::take_keyboard;
use crate::picker::rank;
use crate::{Action, App, Focus, Mode};

/// The arrows on the buttons for the match before and the match after.
pub(crate) const PREVIOUS: &str = "⏶";
pub(crate) const NEXT: &str = "⏷";

#[derive(Default)]
pub(crate) struct Find {
    pub(crate) query: String,
    /// What to put in its place, while the replace row is showing.
    pub(crate) replace: Option<String>,
    /// Whether capitals have to match.
    pub(crate) case: bool,
    /// Which of the matches is the current one.
    pub(crate) current: usize,
    /// The current match is to be chosen again, as the first one from the
    /// caret on: after opening the bar, and when what is looked for changes.
    seek: bool,
}

/// The most notes the graph's bar finds, as on the site: the graph is to
/// show a few notes gathered together, not most of the vault.
const MOST_FOUND: usize = 15;

/// Looking for a note in the graph by its name.
#[derive(Default)]
pub(crate) struct GraphFind {
    pub(crate) query: String,
    /// Which of the notes found has been gone to, if one has yet.
    pub(crate) current: Option<usize>,
    /// The notes found, the likeliest first, by their place in `App::notes`;
    /// how many names matched, of which those are the first; and what they
    /// were found for, the name and the `App::version`, so that the notes
    /// are not gone through again for every frame the graph draws.
    found: Vec<usize>,
    matched: usize,
    found_for: Option<(String, u64)>,
}

impl GraphFind {
    /// The notes found, the likeliest first, by their place in `App::notes`.
    pub(crate) fn found(&self) -> &[usize] {
        &self.found
    }
}

impl App {
    /// Bring up the bar that looks for a note in the graph, or go back to it
    /// if it is up.
    pub(crate) fn open_graph_find(&mut self) {
        self.graph_find.get_or_insert_with(GraphFind::default);
        self.focus = Some(Focus::Find);
    }

    /// Draw the bar that looks for a note in the graph, if it is up, and say
    /// which note the graph is to go to now, by its place in `App::notes`:
    /// one of those found, when it has been asked for.
    ///
    /// A name is found by how it starts, or failing that by what is in it.
    /// Typing goes to no note: the graph shows the notes found, all of them.
    /// Enter goes to the first and then on round them, as it goes round the
    /// matches in a note, and Shift+Enter back. Esc puts the bar away, and
    /// the graph shows every note again, still on the note it has gone to,
    /// where Enter opens it.
    pub(crate) fn graph_find_bar(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let undisturbed = self.picker.is_none() && self.confirm.is_none() && !self.show_settings;
        let find = self.graph_find.as_mut()?;
        // Esc is the bar's while it is up, and nobody's after it: the graph
        // would let go of its note for the same press.
        if undisturbed && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.graph_find = None;
            return None;
        }
        let id = egui::Id::new("graph find");
        // Enter is taken before the field sees it, or it would end the typing in it.
        let focused = ui.memory(|memory| memory.has_focus(id));
        let (mut back, mut on) = ui.input_mut(|input| {
            let back = focused && input.consume_key(Modifiers::SHIFT, Key::Enter);
            (back, focused && !back && input.consume_key(Modifiers::NONE, Key::Enter))
        });
        let (mut go, mut close) = (false, false);
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut find.query).id(id).hint_text("Find a note in the graph").desired_width(220.0).show(ui);
            if self.focus == Some(Focus::Find) {
                take_keyboard(ui, field, id, &find.query);
            }
            let sought = (find.query.clone(), self.version);
            if find.found_for.as_ref() != Some(&sought) {
                // By how directly the name matches, then in the order of the list.
                let named = |note: usize| rank(&sought.0, &self.notes[note].title).filter(|rank| *rank < 2 && !sought.0.trim().is_empty());
                let mut found: Vec<(u8, usize)> = (0..self.notes.len()).filter_map(|note| Some((named(note)?, note))).collect();
                found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| crate::natural_cmp(&self.notes[a.1].title, &self.notes[b.1].title)));
                // A new name has gone to none of what it finds yet; the same
                // name over changed notes stays where it was, if it can.
                let renamed = find.found_for.as_ref().map(|found_for| &found_for.0) != Some(&sought.0);
                find.matched = found.len();
                find.found = found.into_iter().take(MOST_FOUND).map(|(_, note)| note).collect();
                if renamed {
                    find.current = None;
                }
                find.found_for = Some(sought);
            }
            let count = find.found.len();
            let (before, after) = (ui.button(PREVIOUS), ui.button(NEXT));
            #[cfg(test)]
            self.spots.extend([("Note before", before.rect), ("Next note", after.rect)]);
            back |= before.on_hover_text("The note before (Shift+Enter)").clicked();
            on |= after.on_hover_text("The next note (Enter)").clicked();
            if count > 0 && (back || on) {
                find.current = Some(match (find.current, back) {
                    (None, false) => 0,
                    (None, true) => count - 1,
                    (Some(current), back) => (current.min(count - 1) + if back { count - 1 } else { 1 }) % count,
                });
                go = true;
            }
            let more = if find.matched > count { format!(", the first of {}", find.matched) } else { String::new() };
            match (find.query.trim().is_empty(), count, find.current) {
                (true, _, _) => {}
                (false, 0, _) => {
                    ui.colored_label(ui.visuals().error_fg_color, "No notes");
                }
                (false, 1, None) => {
                    ui.weak("1 note");
                }
                (false, count, None) => {
                    ui.weak(format!("{count} notes{more}"));
                }
                (false, count, Some(current)) => {
                    ui.weak(format!("{} of {count}{more}", current.min(count - 1) + 1));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Close (Esc)").clicked();
            });
        });
        ui.separator();
        let sought = find.current.and_then(|current| find.found.get(current)).copied().filter(|_| go);
        if close {
            self.graph_find = None;
        }
        sought
    }

    /// Bring up the find bar, or go back to it if it is up, with the replace
    /// row as well if that is asked for. A word or phrase that is selected in
    /// the note is what it starts out looking for.
    pub(crate) fn open_find(&mut self, replace: bool) {
        if self.open.is_none() {
            return;
        }
        if self.mode == Mode::Reading {
            self.mode = self.edit_mode;
        }
        self.show_graph = false;
        let selected = self.selection.clone().and_then(|selection| self.buffer.get(selection));
        let selected = selected.filter(|text| !text.is_empty() && !text.contains('\n')).map(str::to_owned);
        let find = self.find.get_or_insert_with(Find::default);
        if let Some(selected) = selected {
            find.query = selected;
        }
        if replace && find.replace.is_none() {
            find.replace = Some(String::new());
        }
        find.seek = true;
        self.focus = Some(Focus::Find);
    }

    /// Where in the note the text being looked for is.
    pub(crate) fn find_matches(&self) -> Vec<Range<usize>> {
        self.find.as_ref().map_or_else(Vec::new, |find| occurrences(&self.buffer, &find.query, find.case))
    }

    /// Make the match after the current one current, or with `back` the one
    /// before it. After the last comes the first again.
    pub(crate) fn find_step(&mut self, back: bool) {
        let matches = self.find_matches();
        let Some(find) = self.find.as_mut().filter(|_| !matches.is_empty()) else { return };
        let step = if back { matches.len() - 1 } else { 1 };
        find.current = (find.current.min(matches.len() - 1) + step) % matches.len();
        self.jump = Some(matches[find.current].clone());
    }

    /// Replace the current match, and go on to the next one after it.
    pub(crate) fn replace_one(&mut self) {
        let matches = self.find_matches();
        let Some(find) = self.find.as_ref() else { return };
        let (Some(replacement), Some(range)) = (find.replace.clone(), matches.get(find.current).cloned()) else { return };
        self.buffer.replace_range(range.clone(), &replacement);
        self.edited();
        // From the end of what was put in: the new text may contain the old.
        self.find_from(range.start + replacement.len());
    }

    /// Replace every match in the note.
    pub(crate) fn replace_all(&mut self) {
        let matches = self.find_matches();
        let Some(replacement) = self.find.as_ref().and_then(|find| find.replace.clone()) else { return };
        if matches.is_empty() {
            return;
        }
        let mut text = String::with_capacity(self.buffer.len());
        let mut at = 0;
        for range in &matches {
            text.push_str(&self.buffer[at..range.start]);
            text.push_str(&replacement);
            at = range.end;
        }
        text.push_str(&self.buffer[at..]);
        self.buffer = text;
        self.edited();
        self.info(match matches.len() {
            1 => "Replaced 1 match".to_owned(),
            n => format!("Replaced {n} matches"),
        });
    }

    /// Make the first match at or after `caret` the current one, or the first
    /// of all if there is none after it, and show it.
    fn find_from(&mut self, caret: usize) {
        let matches = self.find_matches();
        let Some(find) = self.find.as_mut() else { return };
        find.current = matches.iter().position(|range| range.start >= caret).unwrap_or(0);
        self.jump = matches.get(find.current).cloned();
    }

    fn edited(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// Draw the bar, and work out what is to be marked in the note below it.
    pub(crate) fn find_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(find) = self.find.as_mut() else {
            self.marks.clear();
            return;
        };
        if std::mem::take(&mut find.seek) {
            let caret = self.selection.as_ref().map_or(0, |selection| selection.start);
            self.find_from(caret);
        }
        let count = self.find_matches().len();
        let (find_id, replace_id) = (egui::Id::new("find"), egui::Id::new("replace"));

        // Enter is taken before the fields see it, or it would end the typing in them.
        let enter = |ui: &egui::Ui, id| {
            let focused = ui.memory(|memory| memory.has_focus(id));
            ui.input_mut(|input| {
                let back = focused && input.consume_key(Modifiers::SHIFT, Key::Enter);
                (back, focused && !back && input.consume_key(Modifiers::NONE, Key::Enter))
            })
        };
        let (mut back, mut on) = enter(ui, find_id);
        let (_, mut replace) = enter(ui, replace_id);
        let (mut replace_all, mut close) = (false, false);

        let Some(find) = self.find.as_mut() else { return };
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut find.query).id(find_id).hint_text("Find in this note").desired_width(220.0).show(ui);
            find.seek |= field.response.changed();
            if self.focus == Some(Focus::Find) {
                take_keyboard(ui, field, find_id, &find.query);
            }
            back |= ui.button(PREVIOUS).on_hover_text("The match before (Shift+Enter)").clicked();
            on |= ui.button(NEXT).on_hover_text("The next match (Enter)").clicked();
            if ui.selectable_label(find.case, "Aa").on_hover_text("Match capitals").clicked() {
                find.case = !find.case;
                find.seek = true;
            }
            if ui.selectable_label(find.replace.is_some(), "Replace").on_hover_text("Show or hide the replace row (Ctrl+H)").clicked() {
                find.replace = if find.replace.is_some() { None } else { Some(String::new()) };
            }
            match (find.query.is_empty(), count) {
                (true, _) => {}
                (false, 0) => {
                    ui.colored_label(ui.visuals().error_fg_color, "No matches");
                }
                (false, count) => {
                    ui.weak(format!("{} of {count}", find.current.min(count - 1) + 1));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Close (Esc)").clicked();
            });
        });
        if let Some(replacement) = find.replace.as_mut() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(replacement).id(replace_id).hint_text("Replace with").desired_width(220.0));
                replace |= ui.add_enabled(count > 0, egui::Button::new("Replace")).on_hover_text("Replace this match (Enter)").clicked();
                replace_all = ui.add_enabled(count > 0, egui::Button::new("Replace all")).clicked();
            });
        }
        ui.separator();

        if replace {
            self.replace_one();
        } else if replace_all {
            self.replace_all();
        } else if back || on {
            self.find_step(back);
        }
        // Esc is the bar's while it is up, unless something on top of it wants it.
        let escape = self.picker.is_none() && self.confirm.is_none() && ui.input(|input| input.key_pressed(Key::Escape));
        if close || escape {
            self.find = None;
            self.marks.clear();
            actions.push(Action::FocusEditor);
            return;
        }
        self.marks = self.find_matches();
        self.mark = self.find.as_ref().map_or(0, |find| find.current);
    }
}
