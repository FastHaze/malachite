//! Everything the app can be told to do, by name: the list behind the
//! keyboard shortcuts and the command palette.
//!
//! The ids, names and hotkeys are the ones these commands commonly have, so
//! what the fingers know already works here. A command either edits the note's text, in which
//! case it is one of the functions in `malachite::editing` and needs the editor
//! to act on, or it does something to the app.

use std::ops::Range;

use anyhow::Result;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use malachite::editing::{self, Edit};

use crate::picker::{Kind, Picker};
use crate::{App, Confirm, Focus, Mode};

/// Changes the note's text, given the text and the selection in it.
pub(crate) type EditFn = fn(&str, Range<usize>) -> Option<Edit>;

#[derive(Clone, Copy)]
pub(crate) enum Run {
    Edit(EditFn),
    App(fn(&mut App) -> Result<()>),
}

pub(crate) struct Command {
    pub id: &'static str,
    /// What the command palette calls it.
    pub name: &'static str,
    pub hotkey: Option<KeyboardShortcut>,
    pub run: Run,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CTRL_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

const fn on(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const fn edit(id: &'static str, name: &'static str, hotkey: Option<KeyboardShortcut>, run: EditFn) -> Command {
    Command { id, name, hotkey, run: Run::Edit(run) }
}

const fn app(id: &'static str, name: &'static str, hotkey: Option<KeyboardShortcut>, run: fn(&mut App) -> Result<()>) -> Command {
    Command { id, name, hotkey, run: Run::App(run) }
}

/// A drawing tool chosen by its key, as ScreenInk has them.
macro_rules! tool {
    ($id:literal, $name:literal, $key:ident, $use:ident) => {
        app($id, $name, on(Modifiers::NONE, Key::$key), |app| {
            app.choose_tool(crate::ink::Use::$use);
            Ok(())
        })
    };
}

/// A drawing colour chosen by its number, from 1.
macro_rules! colour {
    ($place:literal, $id:literal, $name:literal, $key:ident) => {
        app($id, $name, on(Modifiers::NONE, Key::$key), |app| {
            app.pick_colour($place);
            Ok(())
        })
    };
}

pub(crate) const COMMANDS: &[Command] = &[
    edit("editor:toggle-bold", "Toggle bold", on(CTRL, Key::B), |t, s| Some(editing::toggle_wrap(t, s, "**"))),
    edit("editor:toggle-italics", "Toggle italic", on(CTRL, Key::I), |t, s| Some(editing::toggle_wrap(t, s, "*"))),
    edit("editor:toggle-strikethrough", "Toggle strikethrough", None, |t, s| Some(editing::toggle_wrap(t, s, "~~"))),
    edit("editor:toggle-highlight", "Toggle highlight", None, |t, s| Some(editing::toggle_wrap(t, s, "=="))),
    edit("editor:toggle-code", "Toggle code", None, |t, s| Some(editing::toggle_wrap(t, s, "`"))),
    edit("editor:toggle-comments", "Toggle comment", on(CTRL, Key::Slash), |t, s| Some(editing::toggle_wrap(t, s, "%%"))),
    edit("editor:insert-link", "Insert Markdown link", on(CTRL, Key::K), |t, s| Some(editing::insert_link(t, s))),
    edit("editor:toggle-checklist-status", "Toggle checkbox status", on(CTRL, Key::L), |t, s| editing::toggle_task(t, s.start)),
    edit("editor:delete-paragraph", "Delete paragraph", on(CTRL, Key::D), |t, s| Some(editing::delete_paragraph(t, s))),
    edit("editor:swap-line-up", "Move line up", None, |t, s| editing::swap_line(t, s, true)),
    edit("editor:swap-line-down", "Move line down", None, |t, s| editing::swap_line(t, s, false)),
    edit("editor:set-heading-0", "Remove heading", None, |t, s| editing::set_heading(t, s.start, 0)),
    edit("editor:set-heading-1", "Set as heading 1", None, |t, s| editing::set_heading(t, s.start, 1)),
    edit("editor:set-heading-2", "Set as heading 2", None, |t, s| editing::set_heading(t, s.start, 2)),
    edit("editor:set-heading-3", "Set as heading 3", None, |t, s| editing::set_heading(t, s.start, 3)),
    edit("editor:set-heading-4", "Set as heading 4", None, |t, s| editing::set_heading(t, s.start, 4)),
    edit("editor:set-heading-5", "Set as heading 5", None, |t, s| editing::set_heading(t, s.start, 5)),
    edit("editor:set-heading-6", "Set as heading 6", None, |t, s| editing::set_heading(t, s.start, 6)),
    edit("editor:insert-callout", "Insert callout", None, |t, s| Some(editing::insert_callout(t, s.end))),
    edit("editor:insert-codeblock", "Insert code block", None, |t, s| Some(editing::insert_code_block(t, s.end))),
    edit("editor:insert-table", "Insert table", None, |t, s| Some(editing::insert_table(t, s.end))),
    edit("editor:insert-horizontal-rule", "Insert horizontal rule", None, |t, s| Some(editing::insert_rule(t, s.end))),
    app("command-palette:open", "Open command palette", on(CTRL, Key::P), |app| {
        app.picker = Some(Picker::new(Kind::Commands));
        Ok(())
    }),
    app("switcher:open", "Open quick switcher", on(CTRL, Key::O), |app| {
        app.picker = Some(Picker::new(Kind::Notes));
        Ok(())
    }),
    app("global-search:open", "Search in all files", on(CTRL_SHIFT, Key::F), |app| {
        app.show_list = true;
        app.focus = Some(Focus::Search);
        Ok(())
    }),
    app("app:open-vault", "Open another vault", None, |app| {
        app.picker = Some(Picker::new(Kind::Vaults));
        Ok(())
    }),
    app("app:open-settings", "Open settings", on(CTRL, Key::Comma), |app| {
        app.show_settings = true;
        Ok(())
    }),
    app("editor:open-search", "Search current file", on(CTRL, Key::F), |app| {
        // In the graph there is no file showing: there it looks for a note.
        if app.show_graph {
            app.open_graph_find();
        } else {
            app.open_find(false);
        }
        Ok(())
    }),
    app("editor:open-search-replace", "Search & replace in current file", on(CTRL, Key::H), |app| {
        app.open_find(true);
        Ok(())
    }),
    app("graph:open", "Open graph view", on(CTRL, Key::G), |app| {
        app.toggle_graph();
        Ok(())
    }),
    app("file-explorer:new-file", "Create new note", on(CTRL, Key::N), |app| app.create_in(&app.new_note_folder(), None)),
    app("file-explorer:new-folder", "Create new folder", None, |app| app.new_folder(&app.current_folder())),
    app("file-explorer:collapse-all", "Collapse all folders", None, |app| {
        app.fold_folders(true);
        Ok(())
    }),
    app("file-explorer:expand-all", "Expand all folders", None, |app| {
        app.fold_folders(false);
        Ok(())
    }),
    app("daily-notes", "Open today's daily note", None, App::daily),
    app("daily-notes:goto-prev", "Open previous daily note", None, |app| app.daily_step(false)),
    app("daily-notes:goto-next", "Open next daily note", None, |app| app.daily_step(true)),
    app("insert-template", "Insert template", None, |app| {
        if app.open.is_none() {
            anyhow::bail!("open a note to put a template in");
        }
        if app.templates().next().is_none() {
            match app.files.templates.as_os_str().is_empty() {
                true => anyhow::bail!("there are no templates: no folder is set for them in the settings (Ctrl+,)"),
                false => anyhow::bail!("there are no templates: they are the notes in the folder {}", crate::folders::display(&app.files.templates)),
            }
        }
        app.picker = Some(Picker::new(Kind::Templates));
        Ok(())
    }),
    app("editor:toggle-fold", "Toggle fold on the current line", None, |app| {
        app.toggle_fold_at_caret();
        Ok(())
    }),
    app("editor:fold-all", "Fold all headings and lists", None, |app| {
        app.fold_all(true);
        Ok(())
    }),
    app("editor:unfold-all", "Unfold all headings and lists", None, |app| {
        app.fold_all(false);
        Ok(())
    }),
    app("workspace:edit-file-title", "Rename file", on(Modifiers::NONE, Key::F2), |app| {
        app.focus = app.open.is_some().then_some(Focus::Title);
        Ok(())
    }),
    app("app:delete-file", "Delete current file", None, |app| {
        app.confirm = app.open_path.clone().map(Confirm::Note);
        Ok(())
    }),
    app("editor:save-file", "Save current file", on(CTRL, Key::S), |app| {
        app.save();
        Ok(())
    }),
    app("app:go-back", "Navigate back", on(CTRL_ALT, Key::ArrowLeft), |app| {
        app.travel(true);
        Ok(())
    }),
    app("app:go-forward", "Navigate forward", on(CTRL_ALT, Key::ArrowRight), |app| {
        app.travel(false);
        Ok(())
    }),
    app("ink:toggle-drawing", "Toggle drawing over the note", None, |app| {
        app.toggle_drawing();
        Ok(())
    }),
    app("ink:undo", "Undo drawing", on(CTRL, Key::Z), |app| {
        app.undo_ink(true);
        Ok(())
    }),
    app("ink:redo", "Redo drawing", on(CTRL_SHIFT, Key::Z), |app| {
        app.undo_ink(false);
        Ok(())
    }),
    app("ink:stop-drawing", "Stop drawing", on(Modifiers::NONE, Key::Escape), |app| {
        app.escape_drawing();
        Ok(())
    }),
    app("ink:erase-drawings", "Erase all drawings over the note", on(Modifiers::NONE, Key::C), |app| {
        app.erase_drawings();
        Ok(())
    }),
    tool!("ink:pen", "Drawing: pen", P, Pen),
    tool!("ink:highlighter", "Drawing: highlighter", H, Highlighter),
    tool!("ink:line", "Drawing: line", L, Line),
    tool!("ink:arrow", "Drawing: arrow", A, Arrow),
    tool!("ink:rectangle", "Drawing: rectangle", R, Rectangle),
    tool!("ink:ellipse", "Drawing: ellipse", O, Ellipse),
    tool!("ink:text", "Drawing: text", T, Text),
    tool!("ink:badge", "Drawing: numbered badge", N, Badge),
    tool!("ink:eraser", "Drawing: eraser", E, Eraser),
    tool!("ink:select", "Drawing: select", V, Select),
    colour!(0, "ink:colour-1", "Drawing: colour 1", Num1),
    colour!(1, "ink:colour-2", "Drawing: colour 2", Num2),
    colour!(2, "ink:colour-3", "Drawing: colour 3", Num3),
    colour!(3, "ink:colour-4", "Drawing: colour 4", Num4),
    colour!(4, "ink:colour-5", "Drawing: colour 5", Num5),
    colour!(5, "ink:colour-6", "Drawing: colour 6", Num6),
    colour!(6, "ink:colour-7", "Drawing: colour 7", Num7),
    colour!(7, "ink:colour-8", "Drawing: colour 8", Num8),
    app("ink:thinner", "Drawing: smaller", on(Modifiers::NONE, Key::OpenBracket), |app| {
        // To the next of the three sizes down, from wherever the slider is.
        app.ink.width = (app.ink.width.ceil() - 1.0).max(0.0);
        Ok(())
    }),
    app("ink:thicker", "Drawing: bigger", on(Modifiers::NONE, Key::CloseBracket), |app| {
        app.ink.width = (app.ink.width.floor() + 1.0).min(2.0);
        Ok(())
    }),
    app("ink:select-all", "Select all drawings", on(CTRL, Key::A), |app| {
        app.select_all_ink();
        Ok(())
    }),
    app("ink:delete-selection", "Delete the selected drawings", on(Modifiers::NONE, Key::Delete), |app| {
        app.delete_selected_ink();
        Ok(())
    }),
    app("ink:bring-to-front", "Bring the selected drawings to front", on(CTRL, Key::CloseBracket), |app| {
        app.bring_ink_to_front();
        Ok(())
    }),
    app("markdown:toggle-preview", "Toggle reading view", on(CTRL, Key::E), |app| {
        app.toggle_reading();
        Ok(())
    }),
    app("editor:toggle-source", "Toggle Live Preview/Source mode", None, |app| {
        app.set_mode(if app.edit_mode == Mode::Source { Mode::Live } else { Mode::Source });
        Ok(())
    }),
    app("app:toggle-left-sidebar", "Toggle left sidebar", None, |app| {
        app.show_list = !app.show_list;
        Ok(())
    }),
    app("app:toggle-right-sidebar", "Toggle right sidebar", None, |app| {
        app.show_info = !app.show_info;
        Ok(())
    }),
];

/// Whether the keys of `command` are only its while drawing over a note:
/// the rest of the time they are the editor's, or the find bar's, or the
/// letters they are.
pub(crate) fn only_while_drawing(command: &Command) -> bool {
    command.id.starts_with("ink:") && command.id != "ink:toggle-drawing"
}

/// The command whose hotkey has just been pressed, among those `wanted`. The
/// key press is used up, so nothing drawn later reacts to it as well.
///
/// The modifiers have to match exactly: AltGr reaches the app as Ctrl+Alt,
/// and typing `ł` with it must not tick a task the way Ctrl+L does.
pub(crate) fn pressed(ctx: &egui::Context, wanted: impl Fn(&Command) -> bool) -> Option<&'static Command> {
    ctx.input_mut(|input| {
        let (at, command) = input.events.iter().enumerate().find_map(|(at, event)| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return None };
            let hit = |c: &&Command| c.hotkey.is_some_and(|h| h.logical_key == *key && modifiers.matches_exact(h.modifiers));
            COMMANDS.iter().find(|c| hit(c) && wanted(c)).map(|command| (at, command))
        })?;
        input.events.remove(at);
        Some(command)
    })
}
