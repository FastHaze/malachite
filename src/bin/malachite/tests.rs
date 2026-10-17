//! Tests that drive the whole interface without a window: frames are drawn
//! into a bare egui context and the mouse and keyboard are simulated events.

use super::*;
use std::fs;

/// An app on a fresh vault in the system temp dir, removed when dropped,
/// with a windowless egui context to draw it in.
struct TempApp(App, egui::Context);

impl TempApp {
    /// A vault with these notes, given as (title, text). A title may have
    /// folders in front of it: "Projects/Plan" is the note Plan in Projects.
    fn new(name: &str, notes: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("malachite-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let vault = Vault::open(&dir).unwrap();
        for (title, body) in notes {
            let (folder, title) = title.rsplit_once('/').unwrap_or(("", title));
            vault.create_in(Path::new(folder), Some(title), body, &[]).unwrap();
        }
        Self::start(dir, None)
    }

    /// Start the app on the vault in `dir`, as launching the program does.
    /// Settings are only kept between runs when given a file for them.
    fn start(dir: PathBuf, settings: Option<PathBuf>) -> Self {
        let ctx = egui::Context::default();
        let faces = theme::install(&ctx);
        let app = App::new(dir, faces, settings).unwrap();
        ctx.set_zoom_factor(app.zoom);
        Self(app, ctx)
    }

    /// Delete the open note, as answering yes in the dialog does.
    fn trash_open(&mut self) {
        let path = self.0.open_path.clone().expect("a note is open");
        self.0.apply(Action::Trash(path));
    }

    /// Bring up the delete dialog for the open note, as its Delete button does.
    fn ask_to_delete_open(&mut self) {
        self.0.confirm = self.0.open_path.clone().map(Confirm::Note);
    }

    fn type_text(&mut self, text: &str) {
        self.0.buffer.push_str(text);
        self.0.dirty = true;
    }

    fn on_disk(&self, title: &str) -> String {
        self.0.vault.find(title).unwrap().content
    }

    /// Draw one frame without a window.
    fn frame(&mut self) {
        self.frame_with(Vec::new());
    }

    fn frame_with(&mut self, events: Vec<egui::Event>) -> egui::PlatformOutput {
        self.frame_full(events).platform_output
    }

    /// Draw a frame and return everything it produced, shapes included.
    fn frame_full(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.frame_in(events, false)
    }

    /// Draw a frame in a window that fills the screen, or one that does not.
    fn frame_in(&mut self, events: Vec<egui::Event>, maximized: bool) -> egui::FullOutput {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0));
        let mut input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        input.viewports.entry(input.viewport_id).or_default().maximized = Some(maximized);
        let app = &mut self.0;
        // As eframe does before every frame.
        eframe::App::raw_input_hook(app, &self.1, &mut input);
        self.1.run(input, |ctx| app.ui(ctx))
    }

    /// Move the mouse to `pos` and report which cursor it gets there.
    fn cursor_at(&mut self, pos: egui::Pos2) -> egui::CursorIcon {
        self.frame_with(vec![egui::Event::PointerMoved(pos)]);
        self.frame_with(Vec::new()).cursor_icon
    }

    /// A point on the first row of the note's text in Reading view: scanning
    /// down the middle of the text column from below the note's name, the
    /// first place the mouse becomes a text cursor.
    fn text_row(&mut self) -> egui::Pos2 {
        self.frame();
        let top = self.0.content_top.ceil() as usize + 6;
        let column = (top..top + 300).step_by(4).map(|y| egui::pos2(450.0, y as f32));
        let edge = column.into_iter().find(|pos| self.cursor_at(*pos) == egui::CursorIcon::Text).expect("no text found");
        // The cursor already changes a few points before the text; step onto the row itself.
        edge + egui::vec2(0.0, 8.0)
    }

    /// Move the mouse to `pos` and click.
    fn click(&mut self, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(vec![egui::Event::PointerMoved(pos)]);
        self.frame_with(vec![button(true)]);
        self.frame_with(vec![button(false)]);
        self.frame();
    }

    /// Where the editor drew the first clickable thing matching `wanted`.
    fn hot_spot(&mut self, wanted: impl Fn(&Hit) -> bool) -> Option<egui::Pos2> {
        // Two frames, so the layout has settled.
        self.frame();
        self.frame();
        self.0.hot_spots.iter().find(|(_, hit)| wanted(hit)).map(|(pos, _)| *pos)
    }
}

impl Drop for TempApp {
    /// Remove the vault the app ends in, if it is a folder a test made for
    /// itself: one of those named `malachite-test-…` in the system temp dir.
    /// An app can be led to another vault, and wherever that is, it is not
    /// this code's to delete.
    fn drop(&mut self) {
        let root = self.0.vault.root();
        let made_here = root.file_name().is_some_and(|name| name.to_string_lossy().starts_with("malachite-test-"));
        if made_here && root.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = fs::remove_dir_all(root);
        }
    }
}

#[test]
fn opens_first_note_and_saves_edits() {
    let mut t = TempApp::new("save", &[("A", "one"), ("B", "two")]);
    assert_eq!(t.0.open.as_deref(), Some("A"));
    t.type_text(" more");
    t.0.save();
    assert!(!t.0.dirty);
    assert_eq!(t.on_disk("A"), "one more");
}

#[test]
fn switching_notes_saves_the_previous_one() {
    let mut t = TempApp::new("switch", &[("A", "one"), ("B", "two")]);
    t.type_text("!");
    t.0.apply(Action::Open("B".into()));
    assert_eq!(t.0.buffer, "two");
    assert_eq!(t.on_disk("A"), "one!");
}

#[test]
fn autosave_runs_once_the_delay_has_passed() {
    let mut t = TempApp::new("autosave", &[("A", "one")]);
    t.type_text("!");
    t.0.last_edit = Instant::now();
    t.frame();
    assert!(t.0.dirty, "should wait for the autosave delay");
    t.0.last_edit = Instant::now() - AUTOSAVE_DELAY;
    t.frame();
    assert_eq!(t.on_disk("A"), "one!");
}

#[test]
fn new_notes_are_untitled_and_numbered() {
    let mut t = TempApp::new("new", &[]);
    assert!(t.0.open.is_none());
    t.0.apply(Action::New);
    t.0.apply(Action::New);
    assert_eq!(t.0.open.as_deref(), Some("Untitled 1"));
    assert_eq!(t.0.notes.len(), 2);
}

#[test]
fn following_a_link_opens_or_creates_the_target() {
    let mut t = TempApp::new("follow", &[("A", "[[B]] [[C]]"), ("B", "b")]);
    t.0.apply(Action::Follow("b".into(), None));
    assert_eq!(t.0.open.as_deref(), Some("B"));
    t.0.apply(Action::Follow("C".into(), None));
    assert_eq!(t.0.open.as_deref(), Some("C"));
    assert_eq!(t.0.notes.len(), 3);
}

#[test]
fn renaming_keeps_unsaved_text_and_updates_links() {
    let mut t = TempApp::new("rename", &[("A", "text"), ("B", "see [[A]]")]);
    t.type_text(" edited");
    t.0.title_buf = "Z".into();
    t.0.apply(Action::Rename);
    assert_eq!(t.0.open.as_deref(), Some("Z"));
    assert_eq!(t.0.buffer, "text edited");
    assert_eq!(t.on_disk("B"), "see [[Z]]");
    assert!(!t.0.failed());
}

#[test]
fn renaming_onto_an_existing_note_is_refused() {
    let mut t = TempApp::new("rename-clash", &[("A", "a"), ("B", "b")]);
    t.0.title_buf = "B".into();
    t.0.apply(Action::Rename);
    assert!(t.0.failed());
    assert_eq!((t.0.open.as_deref(), t.0.title_buf.as_str()), (Some("A"), "A"));
    assert_eq!(t.on_disk("B"), "b");
}

#[test]
fn trash_saves_then_moves_the_note_away() {
    let mut t = TempApp::new("trash", &[("A", "one")]);
    t.type_text("!");
    t.trash_open();
    assert!(t.0.open.is_none() && t.0.notes.is_empty());
    let trashed = t.0.vault.root().join(".trash").join("A.md");
    assert_eq!(fs::read_to_string(trashed).unwrap(), "one!");
}

#[test]
fn every_screen_draws_without_a_window() {
    let mut t = TempApp::new("draw", &[("A", "[[B]] #tag"), ("B", "[[A]]")]);
    t.frame();
    t.ask_to_delete_open();
    t.frame();
    t.0.filter = "#tag".into();
    t.frame();
    let mut empty = TempApp::new("draw-empty", &[]);
    empty.frame();
}

/// One of everything the renderer knows, with non-ASCII text.
const RICH: &str = "---\ntags: [demo]\n---\n# Заголовок **bold**\n\nText with *italic*, ~~strike~~, ==mark==, `code` \
    and [[B|a link]], [[Missing]], [site](https://example.com) #tag %%note%%\n\n\
    - bullet é\n  - nested\n1. first\n- [ ] todo\n- [x] done\n\n> quote\n> more\n\n\
    > [!warning] Careful\n> body\n\n> [!tip]\n> untitled\n\n---\n\n```rust\nlet x = 1;\n```\n\n\
    | a | b |\n|---|---|\n| 1 | **2** |\n\n## \n";

#[test]
fn a_rich_note_draws_in_every_mode() {
    let mut t = TempApp::new("modes", &[("A", RICH), ("B", "")]);
    for mode in [Mode::Live, Mode::Source, Mode::Reading] {
        t.0.set_mode(mode);
        t.frame();
        t.frame();
    }
    let mut empty = TempApp::new("modes-empty", &[("A", "")]);
    empty.frame();
    assert_eq!(empty.0.buffer, "", "drawing must not change the note");
    assert_eq!(t.0.buffer, RICH);
}

#[test]
fn ctrl_e_switches_between_editing_and_reading() {
    let mut t = TempApp::new("toggle", &[("A", "x")]);
    t.0.set_mode(Mode::Source);
    t.0.toggle_reading();
    assert!(t.0.mode == Mode::Reading);
    t.0.toggle_reading();
    assert!(t.0.mode == Mode::Source, "returns to the editing mode in use before");
}

#[test]
fn clicking_a_checkbox_in_live_preview_ticks_it() {
    let mut t = TempApp::new("click-task", &[("A", "intro\n- [ ] task\n")]);
    let is_checkbox = |hit: &Hit| matches!(hit, Hit::Checkbox(_));
    let pos = t.hot_spot(is_checkbox).expect("the task has a checkbox");
    t.click(pos);
    assert_eq!(t.0.buffer, "intro\n- [x] task\n");
    assert!(t.0.dirty);
    // The click must not have put the text cursor into the `[x]`, which
    // would turn the checkbox back into editable syntax.
    assert!(t.hot_spot(is_checkbox).is_some());
    t.click(pos);
    assert_eq!(t.0.buffer, "intro\n- [ ] task\n");
}

#[test]
fn clicking_a_link_in_live_preview_follows_it() {
    let mut t = TempApp::new("click-link", &[("A", "go to [[B|the other note]] now"), ("B", "b")]);
    let pos = t.hot_spot(|hit| *hit == Hit::Link(Link::Note("B".into(), None))).expect("the link is clickable");
    t.click(pos);
    assert_eq!(t.0.open.as_deref(), Some("B"));
}

#[test]
fn clicking_plain_text_in_live_preview_does_nothing_special() {
    let mut t = TempApp::new("click-text", &[("A", "- [ ] task and [[B]]\n\nplain words here\n"), ("B", "b")]);
    let checkbox = t.hot_spot(|hit| matches!(hit, Hit::Checkbox(_))).unwrap();
    // Two rows further down, over the plain paragraph.
    t.click(checkbox + egui::vec2(20.0, 45.0));
    assert_eq!(t.0.open.as_deref(), Some("A"));
    assert!(!t.0.dirty && t.0.buffer.contains("- [ ] task"));
}

#[test]
fn text_in_reading_view_can_be_selected_and_copied() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda ".repeat(6);
    let mut t = TempApp::new("select", &[("A", &words)]);
    t.0.set_mode(Mode::Reading);
    let from = t.text_row();
    let to = from + egui::vec2(150.0, 0.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    // Drag along the row, then Ctrl+C.
    t.frame_with(vec![button(from, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(to)]);
    t.frame_with(vec![button(to, false)]);
    let output = t.frame_with(vec![egui::Event::Copy]);
    let copied = output.commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.trim().to_owned()),
        _ => None,
    });
    let copied = copied.expect("Ctrl+C should copy the selected text");
    assert!(copied.len() >= 8 && words.contains(&copied), "copied {copied:?}");
    assert!(!t.0.dirty && t.0.buffer == words, "selecting must not edit the note");
}

#[test]
fn a_selection_in_reading_view_can_span_paragraphs() {
    let text = "first paragraph with plenty of words in it to fill the row all the way across\n\n\
                second paragraph with plenty of words in it to fill the row all the way across";
    let mut t = TempApp::new("select-two", &[("A", text)]);
    t.0.set_mode(Mode::Reading);
    let from = t.text_row();
    // One row and a paragraph gap further down, and a little to the right.
    let to = from + egui::vec2(60.0, 45.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    t.frame_with(vec![button(from, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(to)]);
    t.frame_with(vec![button(to, false)]);
    let output = t.frame_with(vec![egui::Event::Copy]);
    let copied = output.commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let copied = copied.expect("Ctrl+C should copy the selected text");
    let (first, second) = copied.split_once('\n').expect("two paragraphs, so a line break");
    assert!(!first.trim().is_empty() && text.lines().next().unwrap().ends_with(first.trim()), "{copied:?}");
    assert!(!second.trim().is_empty() && text.lines().last().unwrap().starts_with(second.trim()), "{copied:?}");
}

/// A press or release of a mouse button at `pos`.
fn button(button: egui::PointerButton, pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
}

/// A note far longer than the window.
fn long_note() -> String {
    (1..=300).map(|n| format!("Line {n} of a long note with a [[B]] link in it\n")).collect()
}

#[test]
fn a_click_of_the_wheel_scrolls_towards_the_mouse_until_the_next_click() {
    use egui::PointerButton::{Middle, Primary};
    for mode in [Mode::Live, Mode::Source, Mode::Reading] {
        let mut t = TempApp::new("wheel-click", &[("A", &long_note()), ("B", "b")]);
        t.0.set_mode(mode);
        t.frame();
        t.frame();
        let anchor = egui::pos2(450.0, 300.0);
        t.frame_with(vec![egui::Event::PointerMoved(anchor)]);
        t.frame_with(vec![button(Middle, anchor, true)]);
        t.frame_with(vec![button(Middle, anchor, false)]);
        assert!(t.0.autoscroll.is_some(), "a quick click leaves autoscroll on");
        for _ in 0..5 {
            t.frame();
        }
        assert_eq!(t.0.scroll_offset, 0.0, "nothing moves while the mouse rests on the anchor");
        let drawn = t.frame_full(Vec::new()).shapes;
        let is_anchor = |s: &egui::epaint::ClippedShape| matches!(&s.shape, egui::Shape::Circle(c) if c.center == anchor && c.radius > 10.0);
        assert!(drawn.iter().any(is_anchor), "the anchor mark is drawn where the wheel was pressed");

        // Below the anchor the note scrolls down, and faster further away.
        t.frame_with(vec![egui::Event::PointerMoved(anchor + egui::vec2(0.0, 60.0))]);
        for _ in 0..10 {
            t.frame();
        }
        let near = t.0.scroll_offset;
        t.frame_with(vec![egui::Event::PointerMoved(anchor + egui::vec2(0.0, 200.0))]);
        for _ in 0..10 {
            t.frame();
        }
        let far = t.0.scroll_offset;
        assert!(near > 0.0 && far - near > 3.0 * near, "near {near}, far {far}");

        // Above the anchor it scrolls back up.
        t.frame_with(vec![egui::Event::PointerMoved(anchor - egui::vec2(0.0, 120.0))]);
        for _ in 0..10 {
            t.frame();
        }
        assert!(t.0.scroll_offset < far);

        // Any click ends it, and that click does nothing else.
        let at = egui::pos2(450.0, 320.0);
        t.frame_with(vec![egui::Event::PointerMoved(at)]);
        t.frame_with(vec![button(Primary, at, true)]);
        t.frame_with(vec![button(Primary, at, false)]);
        assert!(t.0.autoscroll.is_none());
        let rest = t.0.scroll_offset;
        t.frame_with(vec![egui::Event::PointerMoved(at + egui::vec2(0.0, 200.0))]);
        t.frame();
        assert_eq!(t.0.scroll_offset, rest, "no scrolling once it is off");
        assert_eq!(t.0.open.as_deref(), Some("A"), "the click did not follow a link");
        assert!(!t.0.dirty);
    }
}

#[test]
fn dragging_with_the_wheel_held_scrolls_only_while_it_is_held() {
    use egui::PointerButton::Middle;
    let mut t = TempApp::new("wheel-drag", &[("A", &long_note()), ("B", "b")]);
    t.0.set_mode(Mode::Reading);
    t.frame();
    t.frame();
    let anchor = egui::pos2(450.0, 250.0);
    let below = anchor + egui::vec2(0.0, 150.0);
    t.frame_with(vec![egui::Event::PointerMoved(anchor)]);
    t.frame_with(vec![button(Middle, anchor, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(below)]);
    for _ in 0..10 {
        t.frame();
    }
    assert!(t.0.scroll_offset > 0.0);
    t.frame_with(vec![button(Middle, below, false)]);
    assert!(t.0.autoscroll.is_none(), "letting go after a drag ends it");
    // And nothing got selected by dragging over the text.
    let output = t.frame_with(vec![egui::Event::Copy]);
    assert!(!output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(_))));
}

#[test]
fn a_key_press_or_the_wheel_ends_autoscroll() {
    use egui::PointerButton::Middle;
    let anchor = egui::pos2(450.0, 250.0);
    let key = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let turn = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -1.0),
        modifiers: egui::Modifiers::NONE,
    };
    for interruption in [key, turn] {
        let mut t = TempApp::new("wheel-stop", &[("A", &long_note())]);
        t.frame();
        t.frame_with(vec![egui::Event::PointerMoved(anchor)]);
        t.frame_with(vec![button(Middle, anchor, true)]);
        t.frame_with(vec![button(Middle, anchor, false)]);
        assert!(t.0.autoscroll.is_some());
        t.frame_with(vec![interruption]);
        assert!(t.0.autoscroll.is_none());
    }
}

#[test]
fn a_wheel_click_in_the_editor_leaves_the_text_cursor_alone() {
    use egui::PointerButton::Middle;
    let mut t = TempApp::new("wheel-caret", &[("A", "see [[B|the other note]] here\n\nmore"), ("B", "b")]);
    let is_link = |hit: &Hit| matches!(hit, Hit::Link(_));
    let link = t.hot_spot(is_link).expect("a hidden link");
    t.frame_with(vec![egui::Event::PointerMoved(link)]);
    t.frame_with(vec![button(Middle, link, true)]);
    t.frame_with(vec![button(Middle, link, false)]);
    assert!(t.0.autoscroll.is_some());
    assert_eq!(t.0.open.as_deref(), Some("A"), "a wheel click is not a click on the link");
    // Had the cursor moved into the link, its syntax would be showing and
    // it would no longer be a click target.
    assert!(t.hot_spot(is_link).is_some());
}

#[test]
fn links_in_reading_view_still_follow_on_click() {
    let text = "[[B|the other note]] and enough more words to carry this line right across the column";
    let mut t = TempApp::new("read-link", &[("A", text), ("B", "b")]);
    t.0.set_mode(Mode::Reading);
    let row = t.text_row();
    // The link is at the start of the row: scan along it for the hand cursor.
    let along = (230..450).step_by(4).map(|x| egui::pos2(x as f32, row.y));
    let link = along.into_iter().find(|pos| t.cursor_at(*pos) == egui::CursorIcon::PointingHand);
    t.click(link.expect("the link shows a hand cursor"));
    assert_eq!(t.0.open.as_deref(), Some("B"));
}

#[test]
fn clicks_from_reading_view_are_handled() {
    let mut t = TempApp::new("hits", &[("A", "- [ ] task #tag [[New note]]")]);
    let ctx = t.1.clone();
    let mut actions = Vec::new();
    t.0.on_hit(&ctx, Hit::Checkbox(2..5), &mut actions);
    assert!(t.0.buffer.starts_with("- [x] task") && t.0.dirty);
    t.0.on_hit(&ctx, Hit::Link(Link::Tag("tag".into())), &mut actions);
    t.0.on_hit(&ctx, Hit::Link(Link::Note("New note".into(), None)), &mut actions);
    t.0.on_hit(&ctx, Hit::Link(Link::Note(String::new(), None)), &mut actions);
    // A stale range from a note that has since changed is ignored.
    t.0.on_hit(&ctx, Hit::Checkbox(900..903), &mut actions);
    assert_eq!(actions.len(), 2);
    for action in actions {
        t.0.apply(action);
    }
    assert_eq!(t.0.filter, "#tag");
    assert_eq!(t.0.open.as_deref(), Some("New note"));
    assert!(t.on_disk("A").starts_with("- [x] task"), "the tick was saved on the way out");
}

/// A key going down, with nothing held.
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// A key going down with Ctrl held.
fn ctrl(key: egui::Key) -> egui::Event {
    let modifiers = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn typed(text: &str) -> egui::Event {
    egui::Event::Text(text.to_owned())
}

impl TempApp {
    fn titles(&self) -> Vec<&str> {
        self.0.notes.iter().map(|n| n.title.as_str()).collect()
    }

    /// Put the keyboard in the note, as Enter in the title field does.
    fn focus_editor(&mut self) {
        self.0.apply(Action::FocusEditor);
        self.frame();
        self.frame();
    }

    /// Select characters `from..to` of the note in the editor.
    fn select(&mut self, from: usize, to: usize) {
        use egui::text::{CCursor, CCursorRange};
        let id = self.0.editor_id.expect("the editor has been drawn");
        let mut state = egui::TextEdit::load_state(&self.1, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
        state.store(&self.1, id);
        self.frame();
    }
}

#[test]
fn notes_are_listed_the_way_a_person_would_order_them() {
    let names = ["Untitled 10", "untitled 2", "Untitled", "banana", "Apple", "2026-10-12", "2026-9-30", "Untitled 1"];
    let notes: Vec<(&str, &str)> = names.iter().map(|name| (*name, "")).collect();
    let t = TempApp::new("order", &notes);
    assert_eq!(t.titles(), ["2026-9-30", "2026-10-12", "Apple", "banana", "Untitled", "Untitled 1", "untitled 2", "Untitled 10"]);
    assert_eq!(t.0.open.as_deref(), Some("2026-9-30"));
}

#[test]
fn a_new_note_is_named_then_written_without_touching_the_mouse() {
    let mut t = TempApp::new("flow", &[("A", "a")]);
    t.0.set_mode(Mode::Reading);
    t.frame();
    t.0.apply(Action::New);
    assert!(t.0.mode == Mode::Live, "a new note opens ready to edit");
    t.frame();
    t.frame();
    t.frame_with(vec![typed("Shopping")]);
    assert_eq!(t.0.title_buf, "Shopping", "typing replaces the placeholder name, which was selected");
    t.frame_with(vec![key(egui::Key::Enter)]);
    t.frame();
    assert_eq!(t.0.open.as_deref(), Some("Shopping"));
    assert_eq!(t.titles(), ["A", "Shopping"]);
    t.frame_with(vec![typed("milk")]);
    assert_eq!(t.0.buffer, "milk", "Enter moved the keyboard on into the note");
    t.0.save();
    assert_eq!(t.on_disk("Shopping"), "milk");
}

#[test]
fn a_hidden_title_comes_out_to_be_renamed_and_goes_again() {
    let mut t = TempApp::new("no-title", &[("A", "a")]);
    t.frame();
    let shown = t.0.content_top;
    t.0.look.show_title = false;
    t.frame();
    assert!(t.0.content_top < shown, "the note's text starts higher without its name");
    t.frame_with(vec![key(egui::Key::F2)]);
    t.frame();
    t.frame();
    t.frame_with(vec![typed("B")]);
    assert_eq!(t.0.title_buf, "B");
    t.frame_with(vec![key(egui::Key::Enter)]);
    t.frame();
    assert_eq!(t.titles(), ["B"]);
    assert!(t.0.content_top < shown, "and once renamed it is hidden again");
}

#[test]
fn live_preview_draws_a_table_as_a_grid_and_measures_it() {
    let table = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
    let mut t = TempApp::new("live-table", &[("T", &format!("{table}\nafter"))]);
    t.0.set_mode(Mode::Live);
    t.frame();
    t.frame();
    let height = *t.0.table_heights.get(table).expect("the grid was drawn and measured");
    assert!(height > 30.0, "three rows of it: {height}");
}

#[test]
fn escape_in_the_title_backs_out_of_a_rename() {
    let mut t = TempApp::new("rename-esc", &[("A", "a")]);
    t.frame();
    t.0.focus = Some(Focus::Title);
    t.frame();
    t.frame();
    t.frame_with(vec![typed("Oops")]);
    assert_eq!(t.0.title_buf, "Oops");
    t.frame_with(vec![key(egui::Key::Escape)]);
    t.frame();
    assert_eq!((t.0.title_buf.as_str(), t.0.open.as_deref()), ("A", Some("A")));
    assert_eq!(t.titles(), ["A"]);
}

#[test]
fn the_search_box_works_from_the_keyboard() {
    let mut t = TempApp::new("search", &[("Apple", "red fruit"), ("Banana", "yellow"), ("Cherry", "red too")]);
    t.0.show_list = false;
    t.frame();
    t.frame_with(vec![ctrl_with(egui::Key::F, egui::Modifiers::SHIFT)]);
    assert!(t.0.show_list, "Ctrl+Shift+F brings the note list back if it was hidden");
    t.frame();
    t.frame_with(vec![typed("yell")]);
    assert_eq!(t.0.filter, "yell");
    t.frame_with(vec![key(egui::Key::Enter)]);
    t.frame();
    assert_eq!(t.0.open.as_deref(), Some("Banana"), "Enter opens the first match");
    t.frame_with(vec![typed("!")]);
    assert_eq!(t.0.buffer, "yellow!", "and puts the keyboard at the end of the note");

    // Esc clears the search.
    t.frame_with(vec![ctrl_with(egui::Key::F, egui::Modifiers::SHIFT)]);
    t.frame();
    t.frame_with(vec![key(egui::Key::Escape)]);
    t.frame();
    assert_eq!(t.0.filter, "");
}

#[test]
fn enter_and_tab_keep_a_list_going() {
    let mut t = TempApp::new("list-keys", &[("A", "- one")]);
    t.frame();
    t.focus_editor();
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.buffer, "- one\n- ");
    t.frame_with(vec![typed("two")]);
    assert_eq!(t.0.buffer, "- one\n- two", "the caret is after the new bullet");
    t.frame_with(vec![key(egui::Key::Tab)]);
    assert_eq!(t.0.buffer, "- one\n\t- two");
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.buffer, "- one\n\t- two\n\t- ");
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.buffer, "- one\n\t- two\n", "Enter on an empty item ends the list");
    assert!(t.0.dirty);
    t.frame_with(vec![typed("after")]);
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.buffer, "- one\n\t- two\nafter\n", "outside a list Enter is just a new line");
}

#[test]
fn ctrl_b_and_ctrl_i_format_the_selection() {
    let mut t = TempApp::new("format-keys", &[("A", "a plain word here")]);
    t.frame();
    t.focus_editor();
    t.select(8, 12);
    t.frame_with(vec![ctrl(egui::Key::B)]);
    assert_eq!(t.0.buffer, "a plain **word** here");
    t.frame_with(vec![ctrl(egui::Key::I)]);
    assert_eq!(t.0.buffer, "a plain ***word*** here");
    t.frame_with(vec![ctrl(egui::Key::B)]);
    assert_eq!(t.0.buffer, "a plain *word* here", "the word stays selected, so the keys toggle");
    t.frame_with(vec![ctrl(egui::Key::I)]);
    assert_eq!(t.0.buffer, "a plain word here");
    // In Reading view there is nothing to format.
    t.0.set_mode(Mode::Reading);
    t.frame();
    t.frame_with(vec![ctrl(egui::Key::B)]);
    assert_eq!(t.0.buffer, "a plain word here");
}

#[test]
fn changes_made_outside_the_app_show_up_when_the_window_is_back_in_front() {
    let mut t = TempApp::new("refresh", &[("A", "old"), ("B", "b")]);
    t.frame();
    let root = t.0.vault.root().to_path_buf();
    let back_in_front = || vec![egui::Event::WindowFocused(true)];

    fs::write(root.join("A.md"), "new from outside").unwrap();
    fs::write(root.join("C.md"), "c").unwrap();
    t.frame_with(back_in_front());
    assert_eq!(t.0.buffer, "new from outside");
    assert_eq!(t.titles(), ["A", "B", "C"]);

    // Unsaved text here is never thrown away for what is on disk.
    t.type_text("!");
    fs::write(root.join("A.md"), "something else").unwrap();
    t.frame_with(back_in_front());
    assert_eq!(t.0.buffer, "new from outside!");

    // A note deleted elsewhere is closed, not left open as a ghost.
    t.0.save();
    fs::remove_file(root.join("A.md")).unwrap();
    t.frame_with(back_in_front());
    assert_eq!(t.0.open, None);
    assert_eq!(t.titles(), ["B", "C"]);
    assert!(!t.0.failed());
}

#[test]
fn deleting_a_note_moves_on_to_its_neighbour() {
    let mut t = TempApp::new("trash-next", &[("A", "a"), ("B", "b"), ("C", "c")]);
    t.0.apply(Action::Open("B".into()));
    t.trash_open();
    assert_eq!(t.0.open.as_deref(), Some("C"), "the note that took its place");
    t.trash_open();
    assert_eq!(t.0.open.as_deref(), Some("A"), "or the last one, at the end of the list");
}

#[test]
fn the_delete_dialog_answers_to_escape_and_enter() {
    let mut t = TempApp::new("delete-keys", &[("A", "a"), ("B", "b")]);
    t.frame();
    t.ask_to_delete_open();
    t.frame();
    t.frame_with(vec![key(egui::Key::Escape)]);
    t.frame();
    assert!(t.0.confirm.is_none());
    assert_eq!(t.titles(), ["A", "B"], "Esc cancels");

    t.ask_to_delete_open();
    t.frame();
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.titles(), ["B"], "Enter confirms");
    assert!(t.0.confirm.is_none());
}

#[test]
fn the_enter_that_brings_a_dialog_up_does_not_answer_it() {
    // A button that has the keyboard is pressed with Enter, and what it
    // opens is drawn in that same frame, with that Enter still to be seen.
    let mut t = TempApp::new("enter-twice", &[("A", "a"), ("B", "b")]);
    t.frame();
    t.ask_to_delete_open();
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert!(t.0.confirm.is_some());
    assert_eq!(t.titles(), ["A", "B"], "nothing is deleted by a question that has not been seen");
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.titles(), ["B"], "the next Enter is an answer");

    // The same for a list: here the first row would be the system's folder
    // dialog, where there is one (on Linux there is not, nor any other row).
    t.0.picker = Some(picker::Picker::new(picker::Kind::Vaults));
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert!(t.0.picker.is_some() && !t.0.browse, "the list is up, and nothing in it taken");
    if !picker::CAN_BROWSE {
        return;
    }
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert!(t.0.picker.is_none() && t.0.browse, "now it is, and the dialog is asked for");
    assert_eq!(t.0.vault.root(), t.0.recent[0], "which only the real window ever shows");
}

#[test]
fn messages_leave_the_status_bar_after_a_while() {
    let mut t = TempApp::new("message", &[("A", "a")]);
    t.0.info("Renamed");
    t.frame();
    assert!(t.0.message.is_some());
    t.0.message.as_mut().unwrap().since -= MESSAGE_TIME + Duration::from_secs(1);
    t.frame();
    assert!(t.0.message.is_none());
}

#[test]
fn the_window_draws_with_its_side_panels_hidden() {
    let mut t = TempApp::new("panels", &[("A", RICH), ("B", "#tag")]);
    t.0.show_list = false;
    t.0.show_info = false;
    t.frame();
    t.frame();
    // Asking for a tag's notes brings the list back, since that is where they show.
    t.0.apply(Action::Filter("#tag".into()));
    assert!(t.0.show_list);
    t.frame();
}

/// A notch of the scroll wheel with Ctrl held; positive is away from the user.
fn ctrl_wheel(notches: f32) -> egui::Event {
    let modifiers = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, notches), modifiers }
}

/// The app's zoom as a whole percentage.
fn percent(t: &TempApp) -> i32 {
    (t.0.zoom * 100.0).round() as i32
}

#[test]
fn the_window_zooms_with_the_keyboard_and_the_wheel() {
    let mut t = TempApp::new("zoom", &[("A", RICH)]);
    t.frame();
    assert_eq!(percent(&t), 100);
    t.frame_with(vec![ctrl(egui::Key::Equals)]);
    t.frame_with(vec![ctrl(egui::Key::Plus)]);
    assert_eq!(percent(&t), 120);
    t.frame();
    assert_eq!(t.1.zoom_factor(), t.0.zoom, "the whole window is drawn at the app's zoom");
    t.frame_with(vec![ctrl(egui::Key::Minus)]);
    assert_eq!(percent(&t), 110);
    t.frame_with(vec![ctrl_wheel(1.0)]);
    assert_eq!(percent(&t), 120, "a notch of the wheel is one step");
    t.frame_with(vec![ctrl_wheel(-1.0), ctrl_wheel(-1.0), ctrl_wheel(-1.0)]);
    assert_eq!(percent(&t), 90);
    t.frame_with(vec![ctrl(egui::Key::Num0)]);
    assert_eq!(percent(&t), 100);

    // A pinch on a touchpad zooms smoothly, and the keys then go to the next round figure.
    t.frame_with(vec![egui::Event::Zoom(1.04)]);
    assert_eq!(percent(&t), 104);
    t.frame_with(vec![ctrl(egui::Key::Equals)]);
    assert_eq!(percent(&t), 110);

    // The note still draws, in every view, zoomed right out and right in.
    for (key, limit) in [(egui::Key::Minus, 50), (egui::Key::Equals, 250)] {
        for _ in 0..30 {
            t.frame_with(vec![ctrl(key)]);
        }
        assert_eq!(percent(&t), limit, "zoom stops at its limit");
        for mode in [Mode::Live, Mode::Source, Mode::Reading] {
            t.0.set_mode(mode);
            t.frame();
            t.frame();
        }
    }
    assert_eq!(t.0.buffer, RICH, "zooming never touches the note");
}

#[test]
fn the_wheel_without_ctrl_scrolls_and_does_not_zoom() {
    let mut t = TempApp::new("no-zoom", &[("A", &long_note())]);
    t.frame();
    t.frame_with(vec![egui::Event::PointerMoved(egui::pos2(450.0, 300.0))]);
    let turn = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -3.0),
        modifiers: egui::Modifiers::NONE,
    };
    t.frame_with(vec![turn]);
    for _ in 0..20 {
        t.frame();
    }
    assert_eq!(percent(&t), 100);
    assert!(t.0.scroll_offset > 0.0);
}

#[test]
fn zoom_panels_and_editing_mode_are_remembered_for_the_next_run() {
    let dir = std::env::temp_dir().join(format!("malachite-test-{}-remember", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    Vault::open(&dir).unwrap().create(Some("A"), "a", &[]).unwrap();
    let settings = dir.join(".settings").join("settings.txt");

    let mut t = TempApp::start(dir.clone(), Some(settings.clone()));
    t.frame();
    assert!(!settings.exists(), "nothing is written until something changes");
    t.frame_with(vec![ctrl(egui::Key::Equals), ctrl(egui::Key::Equals), ctrl(egui::Key::Equals)]);
    t.0.show_info = false;
    t.0.set_mode(Mode::Source);
    t.0.set_mode(Mode::Reading);
    t.frame();
    assert!(!t.0.failed());
    // A second run of the program, on the same vault and settings file.

    let mut again = TempApp::start(dir, Some(settings));
    assert_eq!(percent(&again), 130);
    assert!(again.0.show_list && !again.0.show_info);
    assert!(again.0.mode == Mode::Source, "the editing mode comes back; Reading view is not sticky");
    again.frame();
    assert_eq!(again.1.zoom_factor(), again.0.zoom);
}

#[test]
fn the_name_of_a_note_is_always_one_line() {
    let mut t = TempApp::new("title-lines", &[("A", "a")]);
    t.frame();
    t.0.focus = Some(Focus::Title);
    t.frame();
    t.frame();
    t.frame_with(vec![egui::Event::Paste("Two\r\nlines".into())]);
    assert_eq!(t.0.title_buf, "Twolines", "pasted line breaks are dropped");
    t.frame_with(vec![key(egui::Key::Enter)]);
    t.frame();
    assert_eq!(t.0.open.as_deref(), Some("Twolines"));
}

impl TempApp {
    /// What the note list shows, top to bottom.
    fn listed(&mut self) -> Vec<String> {
        self.frame();
        self.frame();
        self.0.list_rows.iter().map(|(label, _)| label.clone()).collect()
    }

    /// The middle of the list row that says `label`.
    fn row(&mut self, label: &str) -> egui::Pos2 {
        self.frame();
        self.frame();
        let found = self.0.list_rows.iter().find(|(text, _)| text == label);
        found.unwrap_or_else(|| panic!("no row '{label}' in the list")).1.center()
    }

    /// Press on one point, drag to another and let go there.
    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        use egui::PointerButton::Primary;
        self.frame_with(vec![egui::Event::PointerMoved(from)]);
        self.frame_with(vec![button(Primary, from, true)]);
        // Far enough for it to count as a drag, not a click.
        self.frame_with(vec![egui::Event::PointerMoved(from + egui::vec2(3.0, 10.0))]);
        self.frame_with(vec![egui::Event::PointerMoved(to)]);
        self.frame();
        self.frame_with(vec![button(Primary, to, false)]);
        self.frame();
    }

    /// The folder the note titled `title` is in, with `/` between its parts.
    fn folder_of(&self, title: &str) -> String {
        let note = self.0.notes.iter().find(|n| n.title == title).unwrap_or_else(|| panic!("no note '{title}'"));
        folders::display(&self.0.vault.folder_of(&note.path))
    }
}

#[test]
fn the_list_is_a_tree_with_folders_first() {
    let notes = [("Zebra", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", ""), ("Archive/Old", ""), ("apple", "")];
    let mut t = TempApp::new("tree", &notes);
    t.0.vault.create_folder(Path::new("Projects"), "Empty").unwrap();
    t.0.reload().unwrap();
    assert_eq!(t.listed(), ["Archive", "Old", "Projects", "2026", "Goals", "Empty", "Plan", "apple", "Zebra"]);
    assert_eq!(t.0.open.as_deref(), Some("Old"), "the first note in the list opens at the start");
    assert_eq!(t.titles(), ["Old", "Goals", "Plan", "apple", "Zebra"]);
}

#[test]
fn a_click_on_a_folder_folds_it_and_opening_a_note_inside_unfolds_it() {
    let mut t = TempApp::new("fold", &[("Top", ""), ("Projects/Plan", "p"), ("Projects/2026/Goals", "")]);
    t.0.apply(Action::Open("Top".into()));
    let folder = t.row("Projects");
    t.click(folder);
    assert_eq!(t.listed(), ["Projects", "Top"], "everything inside is tucked away");
    t.click(folder);
    assert_eq!(t.listed(), ["Projects", "2026", "Goals", "Plan", "Top"]);

    // A link can lead into a folded folder; the list then shows where you are.
    t.click(folder);
    t.0.apply(Action::Open("Goals".into()));
    assert_eq!(t.listed(), ["Projects", "2026", "Goals", "Plan", "Top"]);
}

#[test]
fn a_click_on_a_note_in_the_list_opens_it() {
    let mut t = TempApp::new("list-click", &[("A", "a"), ("Folder/B", "b")]);
    let b = t.row("B");
    t.click(b);
    assert_eq!((t.0.open.as_deref(), t.0.buffer.as_str()), (Some("B"), "b"));
}

#[test]
fn new_notes_and_folders_start_beside_the_open_note() {
    let mut t = TempApp::new("new-in", &[("Top", ""), ("Projects/Plan", "see [[Later]]")]);
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::New);
    assert_eq!(t.folder_of("Untitled"), "Projects");
    // Following a link to a note that does not exist makes it there too.
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::Follow("Later".into(), None));
    assert_eq!(t.folder_of("Later"), "Projects");
    // A folder's own menu says where, whatever is open.
    t.0.apply(Action::NewIn(PathBuf::from("Archive")));
    assert_eq!(t.folder_of("Untitled 1"), "Archive");
    // The daily note stays at the top level.
    t.0.apply(Action::Daily);
    assert_eq!(t.folder_of(t.0.open.clone().unwrap().as_str()), "");
}

#[test]
fn ctrl_n_puts_the_note_where_the_settings_say() {
    let mut t = TempApp::new("ctrl-n", &[("Projects/Plan", "")]);
    t.0.apply(Action::Open("Plan".into()));
    t.0.files.new_notes = files::NewNotes::Top;
    t.keys(vec![ctrl(egui::Key::N)]);
    assert_eq!(t.folder_of("Untitled"), "");
}

#[test]
fn pictures_are_listed_in_their_folder_after_its_notes() {
    let mut t = TempApp::new("files-listed", &[("Top", ""), ("Attachments/About", "")]);
    std::fs::write(t.0.vault.root().join("Attachments").join("Cat.png"), "x").unwrap();
    t.0.reload().unwrap();
    t.frame();
    assert_eq!(t.listed(), ["Attachments", "About", "Cat.png", "Top"]);
    assert!(t.0.rows.iter().any(|row| matches!(row, Row::Folder { notes: 1, .. })), "a picture is not counted as a note");
}

#[test]
fn a_new_folder_is_named_straight_away() {
    let mut t = TempApp::new("new-folder", &[("Projects/Plan", "")]);
    t.0.filter = "plan".into();
    t.0.apply(Action::NewFolder(t.0.current_folder()));
    assert_eq!(t.0.filter, "", "the search is cleared so the tree, and the new folder in it, shows");
    t.frame();
    t.frame();
    t.frame_with(vec![typed("Research")]);
    t.frame_with(vec![key(egui::Key::Enter)]);
    t.frame();
    assert_eq!(t.listed(), ["Projects", "Research", "Plan"]);
    assert!(t.0.vault.root().join("Projects").join("Research").is_dir());
    assert!(t.0.renaming.is_none() && !t.0.failed());

    // Esc while naming keeps the name it was given.
    t.0.apply(Action::NewFolder(PathBuf::new()));
    t.frame();
    t.frame();
    t.frame_with(vec![typed("Never mind")]);
    t.frame_with(vec![key(egui::Key::Escape)]);
    t.frame();
    assert_eq!(t.listed(), ["New folder", "Projects", "Research", "Plan"]);
}

#[test]
fn dragging_a_note_onto_a_folder_moves_it_there() {
    let mut t = TempApp::new("drag", &[("Inbox", "i"), ("Loose", "l"), ("Projects/Plan", "p")]);
    let (from, to) = (t.row("Inbox"), t.row("Projects"));
    t.drag(from, to);
    assert_eq!(t.folder_of("Inbox"), "Projects");
    assert_eq!(t.listed(), ["Projects", "Inbox", "Plan", "Loose"]);
    assert!(!t.0.failed());

    // Onto a note: it joins that note's folder.
    let (from, to) = (t.row("Loose"), t.row("Plan"));
    t.drag(from, to);
    assert_eq!(t.folder_of("Loose"), "Projects");

    // Below the list: back to the top level.
    let from = t.row("Plan");
    t.drag(from, egui::pos2(from.x, 600.0));
    assert_eq!(t.folder_of("Plan"), "");
    assert_eq!(t.listed(), ["Projects", "Inbox", "Loose", "Plan"]);
}

#[test]
fn a_plain_click_does_not_move_anything() {
    let mut t = TempApp::new("click-no-move", &[("Inbox", ""), ("Projects/Plan", "")]);
    let (inbox, folder) = (t.row("Inbox"), t.row("Projects"));
    t.click(inbox);
    t.click(folder);
    t.click(folder);
    assert_eq!((t.folder_of("Inbox"), t.folder_of("Plan")), (String::new(), "Projects".to_owned()));
}

#[test]
fn moving_the_open_note_keeps_it_open_with_its_unsaved_text() {
    let mut t = TempApp::new("move-open", &[("A", "one"), ("B", "b")]);
    t.type_text(" two");
    let a = t.0.open_path.clone().unwrap();
    t.0.apply(Action::Move { note: a.clone(), folder: PathBuf::from("Deep/Down") });
    assert_eq!((t.0.open.as_deref(), t.0.buffer.as_str()), (Some("A"), "one two"));
    assert_eq!(t.folder_of("A"), "Deep/Down");
    assert!(!a.exists());
    assert_eq!(t.on_disk("A"), "one two", "it was saved on the way");
    // And it goes on saving to where it is now.
    t.type_text(" three");
    t.0.save();
    assert_eq!(t.on_disk("A"), "one two three");
    assert_eq!(t.listed(), ["Deep", "Down", "A", "B"]);
}

#[test]
fn a_move_onto_a_namesake_is_refused() {
    let mut t = TempApp::new("move-clash", &[("A/Same", "in a")]);
    fs::create_dir_all(t.0.vault.root().join("B")).unwrap();
    fs::write(t.0.vault.root().join("B").join("Same.md"), "in b").unwrap();
    t.0.reload().unwrap();
    let a = t.0.open_path.clone().unwrap();
    t.0.apply(Action::Move { note: a.clone(), folder: PathBuf::from("B") });
    assert!(t.0.failed());
    assert!(a.exists());
    assert_eq!(fs::read_to_string(t.0.vault.root().join("B").join("Same.md")).unwrap(), "in b");
}

#[test]
fn two_notes_with_one_title_in_different_folders_stay_separate() {
    let mut t = TempApp::new("namesakes", &[("A/Same", "in a")]);
    let b = t.0.vault.root().join("B").join("Same.md");
    fs::create_dir_all(b.parent().unwrap()).unwrap();
    fs::write(&b, "in b").unwrap();
    t.0.reload().unwrap();
    t.0.apply(Action::OpenAt(b.clone()));
    assert_eq!(t.0.buffer, "in b");
    t.type_text("!");
    t.0.save();
    assert_eq!(fs::read_to_string(&b).unwrap(), "in b!");
    assert_eq!(fs::read_to_string(t.0.vault.root().join("A").join("Same.md")).unwrap(), "in a");
    // Renaming and deleting act on the one that is open, too.
    t.0.title_buf = "Other".into();
    t.0.apply(Action::Rename);
    assert_eq!(t.folder_of("Other"), "B");
    assert_eq!(t.folder_of("Same"), "A");
    t.trash_open();
    assert_eq!(t.titles(), ["Same"]);
}

#[test]
fn renaming_a_folder_takes_the_open_note_with_it() {
    let mut t = TempApp::new("rename-folder", &[("Projects/2026/Goals", "g"), ("Top", "")]);
    t.0.apply(Action::Open("Goals".into()));
    t.type_text("!");
    t.0.collapsed.insert(PathBuf::from("Projects/2026"));
    t.0.apply(Action::RenameFolder(PathBuf::from("Projects"), "Work".into()));
    assert!(!t.0.failed());
    assert_eq!(t.folder_of("Goals"), "Work/2026");
    assert_eq!((t.0.open.as_deref(), t.0.buffer.as_str()), (Some("Goals"), "g!"));
    t.type_text("?");
    t.0.save();
    assert_eq!(t.on_disk("Goals"), "g!?", "saving goes to the note's new place");
    assert_eq!(t.listed(), ["Work", "2026", "Top"], "the folder inside stays folded");

    // A name that is taken, or no name at all, changes nothing.
    t.0.vault.create_folder(Path::new(""), "Taken").unwrap();
    t.0.apply(Action::RenameFolder(PathBuf::from("Work"), "Taken".into()));
    assert!(t.0.failed());
    t.0.apply(Action::RenameFolder(PathBuf::from("Work"), "  ".into()));
    assert_eq!(t.folder_of("Goals"), "Work/2026");
}

#[test]
fn deleting_a_folder_asks_first_and_takes_its_notes_to_the_trash() {
    let mut t = TempApp::new("trash-folder", &[("Projects/Plan", "p"), ("Projects/2026/Goals", "g"), ("Top", "t")]);
    t.0.apply(Action::Open("Goals".into()));
    t.type_text("!");
    t.0.confirm = Some(Confirm::Folder(PathBuf::from("Projects")));
    t.frame();
    assert_eq!(t.0.notes_in(Path::new("Projects")), 2);
    t.frame_with(vec![key(egui::Key::Escape)]);
    t.frame();
    assert!(t.0.confirm.is_none());
    assert_eq!(t.titles(), ["Goals", "Plan", "Top"], "Esc cancels");

    t.0.confirm = Some(Confirm::Folder(PathBuf::from("Projects")));
    t.frame();
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.titles(), ["Top"]);
    assert_eq!(t.0.open.as_deref(), Some("Top"), "the open note went with the folder");
    let trashed = t.0.vault.root().join(".trash").join("Projects").join("2026").join("Goals.md");
    assert_eq!(fs::read_to_string(trashed).unwrap(), "g!", "with its last edit saved");
    assert_eq!(t.listed(), ["Top"]);
}

#[test]
fn deleting_another_note_from_the_list_leaves_the_open_one_alone() {
    let mut t = TempApp::new("trash-other", &[("A", "a"), ("B", "b")]);
    t.type_text("!");
    let b = t.0.notes.iter().find(|n| n.title == "B").unwrap().path.clone();
    t.0.confirm = Some(Confirm::Note(b));
    t.frame();
    t.frame_with(vec![key(egui::Key::Enter)]);
    assert_eq!(t.titles(), ["A"]);
    assert_eq!((t.0.open.as_deref(), t.0.buffer.as_str(), t.0.dirty), (Some("A"), "a!", true));
}

#[test]
fn a_search_lists_matches_from_every_folder() {
    let mut t = TempApp::new("search-folders", &[("Top", "apple"), ("Projects/Plan", "apple pie"), ("Projects/Other", "pear")]);
    t.0.collapsed.insert(PathBuf::from("Projects"));
    t.0.filter = "apple".into();
    assert_eq!(t.listed(), ["Plan", "apple pie", "Top", "apple"], "folded folders are searched too, and folders themselves are not listed");
    let plan = t.row("Plan");
    t.click(plan);
    assert_eq!(t.0.open.as_deref(), Some("Plan"));
}

#[test]
fn the_app_starts_on_the_note_that_was_open_last_time() {
    let dir = std::env::temp_dir().join(format!("malachite-test-{}-last-note", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let vault = Vault::open(&dir).unwrap();
    vault.create_in(Path::new("Archive"), Some("Old"), "", &[]).unwrap();
    vault.create_in(Path::new("Projects/2026"), Some("Plan"), "p", &[]).unwrap();
    let settings = dir.join(".settings").join("settings.txt");

    let mut t = TempApp::start(dir.clone(), Some(settings.clone()));
    assert_eq!(t.0.open.as_deref(), Some("Old"), "with nothing remembered, the first note in the list");
    t.0.apply(Action::Open("Plan".into()));
    t.frame();

    let mut again = TempApp::start(dir.clone(), Some(settings.clone()));
    assert_eq!((again.0.open.as_deref(), again.0.buffer.as_str()), (Some("Plan"), "p"));
    assert_eq!(again.folder_of("Plan"), "Projects/2026");

    // What was open in another vault says nothing about this one.
    fs::write(&settings, "vault = C:\\some\\other\\vault\nnote = Projects/2026/Plan.md\n").unwrap();
    let other_vault = TempApp::start(dir.clone(), Some(settings.clone()));
    assert_eq!(other_vault.0.open.as_deref(), Some("Old"));

    // And if the note has gone since, it is the first note again.
    again.trash_open();
    fs::write(&settings, format!("vault = {}\nnote = Projects/2026/Plan.md\n", dir.display())).unwrap();
    let gone = TempApp::start(dir, Some(settings));
    assert_eq!(gone.0.open.as_deref(), Some("Old"));
}

/// A key going down with these modifiers held as well as Ctrl.
fn ctrl_with(key: egui::Key, also: egui::Modifiers) -> egui::Event {
    let modifiers = egui::Modifiers { ctrl: true, command: true, ..also };
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

impl TempApp {
    /// Type into whatever has the keyboard, one frame per event.
    fn keys(&mut self, events: Vec<egui::Event>) {
        for event in events {
            self.frame_with(vec![event]);
        }
        self.frame();
    }
}

#[test]
fn every_command_has_its_own_id_and_hotkey() {
    use commands::COMMANDS;
    let ctx = egui::Context::default();
    for (index, command) in COMMANDS.iter().enumerate() {
        for other in &COMMANDS[index + 1..] {
            assert_ne!(command.id, other.id);
            assert_ne!(command.name, other.name);
            if let (Some(a), Some(b)) = (command.hotkey, other.hotkey) {
                assert_ne!(a, b, "{} and {} share {}", command.id, other.id, ctx.format_shortcut(&a));
            }
        }
    }
}

#[test]
fn the_hotkeys_of_the_editing_commands_write_markdown() {
    let mut t = TempApp::new("edit-keys", &[("A", "one\ntwo\nthree")]);
    t.frame();
    t.focus_editor();
    t.select(4, 7);
    t.frame_with(vec![ctrl(egui::Key::K)]);
    assert_eq!(t.0.buffer, "one\n[two]()\nthree");
    t.frame_with(vec![typed("x.md")]);
    assert_eq!(t.0.buffer, "one\n[two](x.md)\nthree", "the caret waits where the address goes");
    t.frame_with(vec![ctrl(egui::Key::D)]);
    assert_eq!(t.0.buffer, "one\nthree");
    t.frame_with(vec![ctrl(egui::Key::L)]);
    assert_eq!(t.0.buffer, "one\n- [ ] three");
    t.frame_with(vec![ctrl(egui::Key::L)]);
    assert_eq!(t.0.buffer, "one\n- [x] three");
    t.select(0, 3);
    t.frame_with(vec![ctrl(egui::Key::Slash)]);
    assert_eq!(t.0.buffer, "%%one%%\n- [x] three");
    assert!(t.0.dirty);
}

#[test]
fn a_hotkey_needs_exactly_its_modifiers() {
    let mut t = TempApp::new("altgr", &[("A", "- [ ] task")]);
    t.frame();
    t.focus_editor();
    // AltGr is Ctrl+Alt to the app: it types a letter, it is not Ctrl+L.
    t.frame_with(vec![ctrl_with(egui::Key::L, egui::Modifiers::ALT)]);
    t.frame_with(vec![ctrl_with(egui::Key::N, egui::Modifiers::SHIFT)]);
    assert_eq!((t.0.buffer.as_str(), t.0.notes.len()), ("- [ ] task", 1));
}

#[test]
fn brackets_and_quotes_close_themselves_while_typing() {
    let mut t = TempApp::new("pairs", &[("A", "")]);
    t.frame();
    t.focus_editor();
    t.keys(vec![typed("["), typed("[")]);
    assert_eq!(t.0.buffer, "[[]]");
    t.keys(vec![typed("B"), typed("]"), typed("]"), typed(" ok")]);
    assert_eq!(t.0.buffer, "[[B]] ok", "the closing brackets are stepped over, not doubled");
    t.select(6, 8);
    t.keys(vec![typed("*"), typed("*")]);
    assert_eq!(t.0.buffer, "[[B]] **ok**", "a marker typed over a selection wraps it");
    assert!(t.0.dirty);
}

#[test]
fn the_command_palette_runs_the_command_that_is_picked() {
    let mut t = TempApp::new("palette", &[("A", "a plain word here")]);
    t.frame();
    t.focus_editor();
    t.select(8, 12);
    t.keys(vec![ctrl(egui::Key::P)]);
    assert!(t.0.picker.is_some());
    assert!(t.0.picked_from.contains(&"Toggle bold".to_owned()) && t.0.picked_from.contains(&"Create new note".to_owned()));
    assert!(!t.0.picked_from.contains(&"Open command palette".to_owned()), "it is open already");
    t.keys(vec![typed("strike")]);
    assert_eq!(t.0.picked_from, ["Toggle strikethrough"]);
    t.keys(vec![key(egui::Key::Enter)]);
    assert!(t.0.picker.is_none());
    // The note gets the keyboard back a frame after the palette has gone.
    t.frame();
    assert_eq!(t.0.buffer, "a plain ~~word~~ here", "the command acted on what was selected in the note");
    t.keys(vec![typed("!")]);
    assert_eq!(t.0.buffer, "a plain ~~!~~ here", "and the keyboard is back in the note");
}

#[test]
fn the_palette_moves_with_the_arrows_and_closes_on_escape() {
    let mut t = TempApp::new("palette-keys", &[("A", "text")]);
    t.frame();
    t.keys(vec![ctrl(egui::Key::P), typed("toggle sidebar")]);
    assert_eq!(t.0.picked_from, ["Toggle left sidebar", "Toggle right sidebar"]);
    t.keys(vec![key(egui::Key::ArrowDown), key(egui::Key::ArrowDown), key(egui::Key::Enter)]);
    assert!(t.0.show_list && !t.0.show_info, "the second row was the one taken");

    t.keys(vec![ctrl(egui::Key::P), typed("heading")]);
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.picker.is_none());
    assert_eq!(t.0.buffer, "text");
    // While it is up the keyboard is its own.
    t.keys(vec![ctrl(egui::Key::P), ctrl(egui::Key::N)]);
    assert_eq!(t.0.notes.len(), 1);
}

#[test]
fn the_highlighted_row_of_a_menu_is_a_fill_with_no_line_around_it() {
    let mut t = TempApp::new("picker-look", &[("A", "text"), ("B", "")]);
    t.frame();
    let selection = t.1.style().visuals.selection;
    // The boxes a frame draws that are filled as a selection is, and those
    // outlined as one. A menu fades in, so it is given a moment first.
    let boxes = |t: &mut TempApp| {
        for _ in 0..20 {
            t.frame();
        }
        let shapes = t.frame_full(Vec::new()).shapes;
        let rects = shapes.iter().filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => Some((rect.fill == selection.bg_fill, !rect.stroke.is_empty() && rect.stroke.color == selection.stroke.color)),
            _ => None,
        });
        rects.fold((0, 0), |(filled, outlined), (fill, outline)| (filled + usize::from(fill), outlined + usize::from(outline)))
    };
    let (filled, outlined) = boxes(&mut t);
    for hotkey in [egui::Key::O, egui::Key::P] {
        t.keys(vec![ctrl(hotkey)]);
        assert!(t.0.picker.is_some() && t.0.picked_from.len() > 1);
        assert_eq!(boxes(&mut t), (filled + 1, outlined), "one more fill, behind the row Enter would take, and no outline");
        t.keys(vec![key(egui::Key::ArrowDown)]);
        assert_eq!(boxes(&mut t), (filled + 1, outlined), "and so for the row below it");
        t.keys(vec![key(egui::Key::Escape)]);
    }
}

#[test]
fn the_palette_offers_no_editing_in_reading_view() {
    let mut t = TempApp::new("palette-reading", &[("A", "text")]);
    t.0.set_mode(Mode::Reading);
    t.frame();
    t.keys(vec![ctrl(egui::Key::P)]);
    assert!(!t.0.picked_from.contains(&"Toggle bold".to_owned()));
    t.keys(vec![typed("reading"), key(egui::Key::Enter)]);
    assert!(t.0.mode == Mode::Live);
}

#[test]
fn the_quick_switcher_opens_a_note_by_name() {
    let mut t = TempApp::new("switcher", &[("Apple", "a"), ("Projects/Banana", "b"), ("Cherry", "c")]);
    t.frame();
    t.keys(vec![ctrl(egui::Key::O)]);
    assert_eq!(t.0.picked_from, ["Banana", "Apple", "Cherry"], "every note, in the order of the list");
    t.keys(vec![typed("an")]);
    assert_eq!(t.0.picked_from, ["Banana"]);
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Banana"));
    t.keys(vec![typed("!")]);
    assert_eq!(t.0.buffer, "b!", "ready to write in it");

    t.keys(vec![ctrl(egui::Key::O), typed("chy")]);
    assert_eq!(t.0.picked_from, ["Cherry"], "letters in order are enough");
}

#[test]
fn the_quick_switcher_creates_the_note_it_cannot_find() {
    let mut t = TempApp::new("switcher-new", &[("Projects/Plan", "p")]);
    t.frame();
    t.keys(vec![ctrl(egui::Key::O), typed("Ideas"), key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Ideas"));
    assert_eq!(t.folder_of("Ideas"), "Projects", "beside the note that was open");
    assert!(!t.0.failed());
}

#[test]
fn back_and_forward_retrace_the_notes_that_were_open() {
    let alt = egui::Modifiers::ALT;
    let mut t = TempApp::new("history", &[("A", "a"), ("B", "b"), ("C", "c")]);
    t.frame();
    t.0.apply(Action::Open("B".into()));
    t.0.apply(Action::Open("C".into()));
    t.type_text("!");
    t.keys(vec![ctrl_with(egui::Key::ArrowLeft, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("B"));
    assert_eq!(t.on_disk("C"), "c!", "leaving a note saves it");
    t.keys(vec![ctrl_with(egui::Key::ArrowLeft, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("A"));
    t.keys(vec![ctrl_with(egui::Key::ArrowLeft, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("A"), "there is nothing before the first");
    t.keys(vec![ctrl_with(egui::Key::ArrowRight, alt), ctrl_with(egui::Key::ArrowRight, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("C"));

    // Opening a note from here starts a new way forward.
    t.keys(vec![ctrl_with(egui::Key::ArrowLeft, alt)]);
    t.0.apply(Action::Open("A".into()));
    t.keys(vec![ctrl_with(egui::Key::ArrowRight, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("A"));
    t.keys(vec![ctrl_with(egui::Key::ArrowLeft, alt)]);
    assert_eq!(t.0.open.as_deref(), Some("B"));
}

#[test]
fn going_back_passes_over_notes_that_are_gone_or_renamed() {
    let mut t = TempApp::new("history-gone", &[("A", "a"), ("B", "b"), ("C", "c")]);
    t.0.apply(Action::Open("B".into()));
    t.0.apply(Action::Open("C".into()));
    let b = t.0.notes[1].path.clone();
    t.0.apply(Action::Trash(b));
    t.0.title_buf = "Z".into();
    t.0.apply(Action::Rename);
    assert_eq!(t.0.open.as_deref(), Some("Z"));
    t.0.travel(true);
    assert_eq!(t.0.open.as_deref(), Some("A"), "B is in the trash, and Z's old name is not a place");
    t.0.travel(true);
    assert_eq!(t.0.open.as_deref(), Some("A"));
}

#[test]
fn the_arrows_above_the_note_can_be_drawn() {
    let mut t = TempApp::new("arrows", &[("A", "a"), ("B", "b")]);
    t.0.apply(Action::Open("B".into()));
    t.frame();
    let font = egui::TextStyle::Button.resolve(&t.1.style());
    for arrow in [panels::BACK, panels::FORWARD] {
        assert!(t.1.fonts(|fonts| fonts.has_glyphs(&font, arrow)), "no glyph for {arrow}");
    }
}

#[test]
fn f2_renames_the_note_from_the_keyboard() {
    let mut t = TempApp::new("f2", &[("A", "a")]);
    t.frame();
    t.focus_editor();
    t.keys(vec![key(egui::Key::F2), typed("Renamed"), key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Renamed"));
    assert_eq!(t.titles(), ["Renamed"]);
}

/// A folder in the system temp dir for a second vault, empty to begin with.
/// The caller removes it.
fn spare_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("malachite-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn opening_another_vault_leaves_the_first_one_saved_and_behind() {
    let other = spare_folder("vault-other");
    Vault::open(&other).unwrap().create(Some("Elsewhere"), "far away", &[]).unwrap();

    let mut t = TempApp::new("vault-first", &[("A", "a"), ("Projects/B", "b")]);
    let first = t.0.vault.root().to_path_buf();
    t.0.apply(Action::Open("B".into()));
    t.type_text("!");
    t.0.filter = "b".into();
    t.0.apply(Action::OpenVault(other.clone()));

    assert!(!t.0.failed());
    assert_eq!(fs::read_to_string(first.join("Projects").join("B.md")).unwrap(), "b!", "what was unsaved is saved first");
    assert_eq!(t.0.vault.root(), other);
    assert_eq!((t.titles(), t.0.open.as_deref(), t.0.buffer.as_str()), (vec!["Elsewhere"], Some("Elsewhere"), "far away"));
    assert!(t.0.back.is_empty() && t.0.filter.is_empty(), "nothing of the old vault carries over");
    t.frame();
    t.0.travel(true);
    assert_eq!(t.0.open.as_deref(), Some("Elsewhere"), "there is no way back into the other vault's notes");

    // And back again, which is no different.
    t.0.apply(Action::OpenVault(first.clone()));
    assert_eq!(t.titles(), ["B", "A"]);
    assert_eq!(t.0.recent, [first, other.clone()]);
    let _ = fs::remove_dir_all(other);
}

#[test]
fn the_vault_switcher_lists_the_vaults_that_were_open_before() {
    let other = spare_folder("vault-listed");
    let gone = spare_folder("vault-gone");
    let mut t = TempApp::new("vault-switcher", &[("A", "a")]);
    let first = t.0.vault.root().to_path_buf();
    t.0.apply(Action::OpenVault(gone.clone()));
    t.0.apply(Action::OpenVault(other.clone()));
    fs::remove_dir_all(&gone).unwrap();
    t.frame();

    t.keys(vec![ctrl(egui::Key::P), typed("another vault"), key(egui::Key::Enter)]);
    let listed = |t: &TempApp| t.0.picked_from.iter().filter(|row| !row.starts_with("Choose")).cloned().collect::<Vec<_>>();
    assert_eq!(listed(&t), [picker::vault_name(&first)], "not the open one, nor one whose folder has gone");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!((t.0.vault.root(), t.titles()), (first.as_path(), vec!["A"]));
    let _ = fs::remove_dir_all(other);
}

#[test]
fn the_vaults_name_in_the_status_bar_answers_the_mouse_as_a_button_does() {
    let mut t = TempApp::new("vault-button", &[("A", "a")]);
    let name = picker::vault_name(t.0.vault.root());
    let at = t.spot("Vault");
    let width = t.0.spots["Vault"].width();
    let visuals = t.1.style().visuals.clone();
    // The ground under the name and the colour of its letters, in a frame
    // with these things happening.
    let looks = |t: &mut TempApp, events| {
        let (mut ground, mut ink) = (egui::Color32::TRANSPARENT, None);
        for clipped in t.frame_full(events).shapes {
            match clipped.shape {
                egui::Shape::Rect(rect) if rect.rect.contains(at) && rect.rect.width() < width + 4.0 => ground = rect.fill,
                egui::Shape::Text(text) if text.galley.text().trim_start() == name => ink = Some(text.fallback_color),
                _ => {}
            }
        }
        (ground, ink.expect("the vault's name is written"))
    };

    let widgets = &visuals.widgets;
    assert_eq!(looks(&mut t, Vec::new()), (egui::Color32::TRANSPARENT, visuals.weak_text_color()), "flat and quiet while left alone");
    looks(&mut t, vec![egui::Event::PointerMoved(at)]);
    assert_eq!(looks(&mut t, Vec::new()), (widgets.hovered.weak_bg_fill, widgets.hovered.fg_stroke.color), "lit up under the mouse");
    looks(&mut t, vec![button(egui::PointerButton::Primary, at, true)]);
    assert_eq!(looks(&mut t, Vec::new()), (widgets.active.weak_bg_fill, widgets.active.fg_stroke.color), "pressed in while held down");
    looks(&mut t, vec![button(egui::PointerButton::Primary, at, false)]);
    assert!(t.0.picker.is_some(), "and a click brings up the list of vaults");
}

#[test]
fn a_typed_path_opens_that_folder_as_a_vault_or_makes_it() {
    let new = spare_folder("vault-typed");
    let mut t = TempApp::new("vault-typing", &[("A", "a")]);
    let first = t.0.vault.root().to_path_buf();
    t.frame();
    t.0.picker = Some(picker::Picker::new(picker::Kind::Vaults));
    t.keys(vec![typed("some folder")]);
    assert!(t.0.picked_from.iter().all(|row| row.starts_with("Choose")), "half a path is not a place");
    assert!(!first.join("some folder").exists());

    t.0.picker = Some(picker::Picker::new(picker::Kind::Vaults));
    t.keys(vec![typed(&new.display().to_string()), key(egui::Key::Enter)]);
    assert!(!t.0.failed());
    assert_eq!(t.0.vault.root(), new);
    assert!(new.is_dir() && t.0.notes.is_empty() && t.0.open.is_none(), "a new vault starts empty");
    t.0.apply(Action::New);
    assert!(new.join("Untitled.md").is_file());
    // The app cleans up the vault it ends in; the one it started in is left to us.
    let _ = fs::remove_dir_all(first);
}

#[test]
fn the_vault_that_was_open_is_the_one_the_app_starts_in() {
    let first = spare_folder("vault-start-a");
    let second = spare_folder("vault-start-b");
    let settings = first.join(".settings").join("settings.txt");
    Vault::open(&first).unwrap().create(Some("One"), "", &[]).unwrap();
    Vault::open(&second).unwrap().create(Some("Two"), "", &[]).unwrap();

    let mut t = TempApp::start(first.clone(), Some(settings.clone()));
    t.0.apply(Action::OpenVault(second.clone()));
    t.frame();
    let saved = Settings::load(&settings);
    assert_eq!(saved.recent, [second.display().to_string(), first.display().to_string()]);
    assert_eq!(starting_vault(None, &saved), second);
    assert_eq!(starting_vault(Some(first.clone()), &saved), first, "a vault named on the command line wins");

    let again = TempApp::start(starting_vault(None, &saved), Some(settings.clone()));
    assert_eq!((again.0.open.as_deref(), again.0.recent.len()), (Some("Two"), 2));
    drop(again);

    // The settings went with the first vault, and the second has been removed by now.
    assert_eq!(starting_vault(None, &saved), PathBuf::from("vault"), "a vault that is gone is not gone back to");
    let _ = fs::remove_dir_all(first);
}

/// A key going down with Shift held.
fn shift(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT }
}

impl TempApp {
    /// Which match the find bar is on, and how many there are.
    fn found(&self) -> (usize, usize) {
        (self.0.find.as_ref().expect("the find bar is up").current, self.0.marks.len())
    }

    /// What is selected in the editor.
    fn selected(&self) -> &str {
        &self.0.buffer[self.0.selection.clone().expect("the editor has a caret")]
    }
}

#[test]
fn the_search_box_understands_the_operators_of_a_query() {
    let notes = [
        ("Shopping", "- [ ] milk\n- [x] bread\n#home"),
        ("Projects/Plan", "milk the project\nfor bread money #work/urgent"),
        ("Projects/Notes", "nothing much"),
    ];
    let mut t = TempApp::new("operators", &notes);
    let mut found = |query: &str| {
        t.0.filter = query.into();
        t.0.search().iter().map(|found| t.0.notes[found.note].title.clone()).collect::<Vec<_>>()
    };
    assert_eq!(found("milk bread"), ["Plan", "Shopping"]);
    assert_eq!(found("line:(milk bread)"), [] as [&str; 0]);
    assert_eq!(found("task-done:bread"), ["Shopping"]);
    assert_eq!(found("path:projects -milk"), ["Notes"]);
    assert_eq!(found("tag:work OR #home"), ["Plan", "Shopping"]);
    assert_eq!(found("file:plan"), ["Plan"]);
    assert_eq!(found("\"the project\""), ["Plan"]);
}

#[test]
fn a_search_is_done_again_when_the_notes_change() {
    let mut t = TempApp::new("search-fresh", &[("A", "apple"), ("B", "pear")]);
    t.0.filter = "apple".into();
    assert_eq!(t.listed(), ["A", "apple"]);
    t.0.apply(Action::Open("B".into()));
    t.type_text(" and apple");
    t.0.save();
    assert_eq!(t.listed(), ["A", "apple", "B", "pear and apple"]);
    t.0.apply(Action::New);
    t.0.title_buf = "Apple pie".into();
    t.0.apply(Action::Rename);
    assert_eq!(t.listed().len(), 5, "a name counts too, with no line to show for it");
}

#[test]
fn a_click_on_a_found_line_goes_to_it_in_the_note() {
    let mut text = long_note();
    text.push_str("\nthe needle is here\n");
    let mut t = TempApp::new("search-jump", &[("A", "short"), ("Hay", &text)]);
    t.0.filter = "needle".into();
    assert_eq!(t.listed(), ["Hay", "the needle is here"]);
    let line = t.row("the needle is here");
    t.click(line);
    t.frame();
    assert_eq!(t.0.open.as_deref(), Some("Hay"));
    assert_eq!(t.selected(), "needle");
    assert!(t.0.scroll_offset > 0.0, "the line has been scrolled into view");
    t.frame_with(vec![typed("pin")]);
    assert!(t.0.buffer.contains("the pin is here"), "the keyboard is in the note, on what was found");
}

#[test]
fn ctrl_f_finds_text_in_the_note_and_enter_goes_round_the_matches() {
    let mut t = TempApp::new("find", &[("A", "one two One\nthree ONE two")]);
    t.frame();
    t.focus_editor();
    t.keys(vec![ctrl(egui::Key::F), typed("one")]);
    assert_eq!(t.found(), (0, 3));
    assert_eq!(t.selected(), "one");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!((t.found(), t.selected()), ((1, 3), "One"));
    t.keys(vec![key(egui::Key::Enter), key(egui::Key::Enter)]);
    assert_eq!(t.found(), (0, 3), "after the last match comes the first");
    t.keys(vec![shift(egui::Key::Enter)]);
    assert_eq!((t.found(), t.selected()), ((2, 3), "ONE"));
    assert_eq!(t.0.buffer, "one two One\nthree ONE two", "none of this has typed anything into the note");

    // With capitals mattering there is one match, and it is the current one.
    t.0.find.as_mut().unwrap().case = true;
    t.keys(vec![typed("s")]);
    assert_eq!(t.found(), (0, 0));
    t.0.find.as_mut().unwrap().query = "ONE".into();
    t.frame();
    assert_eq!(t.found(), (0, 1));
}

#[test]
fn escape_closes_the_find_bar_and_leaves_the_match_selected() {
    let mut t = TempApp::new("find-esc", &[("A", "alpha beta gamma")]);
    t.frame();
    t.focus_editor();
    t.keys(vec![ctrl(egui::Key::F), typed("beta"), key(egui::Key::Escape)]);
    assert!(t.0.find.is_none() && t.0.marks.is_empty());
    t.keys(vec![typed("BETA")]);
    assert_eq!(t.0.buffer, "alpha BETA gamma", "typing goes into the note, over what was found");
}

#[test]
fn the_find_bar_starts_with_what_is_selected_and_from_where_the_caret_is() {
    let mut t = TempApp::new("find-selected", &[("A", "ab cd ab cd ab")]);
    t.frame();
    t.focus_editor();
    t.select(6, 8);
    t.keys(vec![ctrl(egui::Key::F)]);
    assert_eq!(t.0.find.as_ref().unwrap().query, "ab");
    assert_eq!(t.found(), (1, 3), "the match that was selected, not the first in the note");
}

#[test]
fn a_far_match_is_scrolled_into_view() {
    let mut text = long_note();
    text.push_str("\nneedle\n");
    let mut t = TempApp::new("find-scroll", &[("A", &text)]);
    t.frame();
    t.focus_editor();
    t.select(0, 0);
    t.keys(vec![ctrl(egui::Key::F), typed("needle")]);
    t.frame();
    assert_eq!(t.found(), (0, 1));
    assert!(t.0.scroll_offset > 0.0);
}

#[test]
fn replace_swaps_one_match_or_all_of_them() {
    let mut t = TempApp::new("replace", &[("A", "a cat, a Cat, a cat")]);
    t.frame();
    t.focus_editor();
    t.select(0, 0);
    t.keys(vec![ctrl(egui::Key::H), typed("cat")]);
    assert_eq!(t.0.find.as_ref().unwrap().replace.as_deref(), Some(""), "Ctrl+H shows the replace row");
    t.0.find.as_mut().unwrap().replace = Some("cat cat".into());
    t.0.replace_one();
    t.frame();
    assert_eq!(t.0.buffer, "a cat cat, a Cat, a cat");
    assert_eq!((t.found(), t.selected()), ((2, 4), "Cat"), "on to the next, not back into what was put in");
    assert!(t.0.dirty);

    t.0.find.as_mut().unwrap().replace = Some("dog".into());
    t.0.replace_all();
    t.frame();
    assert_eq!(t.0.buffer, "a dog dog, a dog, a dog");
    assert_eq!(t.found().1, 0);
    t.0.save();
    assert_eq!(t.on_disk("A"), "a dog dog, a dog, a dog");
}

#[test]
fn enter_in_the_replace_field_replaces_the_match() {
    let mut t = TempApp::new("replace-enter", &[("A", "x y x")]);
    t.frame();
    t.focus_editor();
    t.select(0, 0);
    t.keys(vec![ctrl(egui::Key::H), typed("x")]);
    t.0.find.as_mut().unwrap().replace = Some("z".into());
    t.1.memory_mut(|memory| memory.request_focus(egui::Id::new("replace")));
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.buffer, "z y x");
}

#[test]
fn find_in_reading_view_goes_to_the_editing_view() {
    let mut t = TempApp::new("find-reading", &[("A", "some text")]);
    t.0.set_mode(Mode::Source);
    t.0.set_mode(Mode::Reading);
    t.frame();
    t.keys(vec![ctrl(egui::Key::F), typed("text")]);
    assert!(t.0.mode == Mode::Source, "the editing view that was last used");
    assert_eq!(t.found(), (0, 1));
    // And going back to reading puts the bar away.
    t.keys(vec![ctrl(egui::Key::E)]);
    assert!(t.0.find.is_none());
}

#[test]
fn the_find_bar_draws_with_everything_showing() {
    let mut t = TempApp::new("find-draw", &[("A", "a long line that wraps ".repeat(30).as_str()), ("B", "")]);
    t.frame();
    t.keys(vec![ctrl(egui::Key::H), typed("line that wraps a long")]);
    assert_eq!(t.found().1, 29, "matches that run over the end of a row are marked too");
    t.0.apply(Action::Open("B".into()));
    t.frame();
    t.frame();
    assert_eq!(t.found(), (0, 0), "the bar stays up from note to note");
    let font = egui::TextStyle::Button.resolve(&t.1.style());
    for arrow in [find::PREVIOUS, find::NEXT] {
        assert!(t.1.fonts(|fonts| fonts.has_glyphs(&font, arrow)), "no glyph for {arrow}");
    }
}

#[test]
fn the_note_list_can_be_as_narrow_as_it_is_allowed_to_be() {
    let mut t = TempApp::new("narrow-list", &[("A", "a")]);
    t.frame();
    t.frame();
    let panel = |t: &TempApp| egui::containers::panel::PanelState::load(&t.1, egui::Id::new("notes")).unwrap().rect;
    // Drag its edge far past the narrowest it goes, and keep the mouse down.
    let edge = egui::pos2(panel(&t).right(), 300.0);
    let far = egui::pos2(20.0, 300.0);
    t.frame_with(vec![egui::Event::PointerMoved(edge)]);
    t.frame_with(vec![button(egui::PointerButton::Primary, edge, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(far)]);
    t.frame();
    // What it draws must fit, or the note beside it starts further right than
    // the list ends, with a strip of bare window between them.
    assert_eq!(panel(&t).width(), 170.0);
    t.frame_with(vec![button(egui::PointerButton::Primary, far, false)]);
}

#[test]
fn the_links_panel_can_be_as_narrow_as_it_is_allowed_to_be() {
    let mut t = TempApp::new("narrow-info", &[("A", "[[A long link to somewhere]] #a-rather-long-tag #another/long-tag"), ("B", "[[A]]")]);
    t.frame();
    t.frame();
    let panel = |t: &TempApp| egui::containers::panel::PanelState::load(&t.1, egui::Id::new("info")).unwrap().rect;
    let edge = egui::pos2(panel(&t).left(), 300.0);
    let far = egui::pos2(1090.0, 300.0);
    t.frame_with(vec![egui::Event::PointerMoved(edge)]);
    t.frame_with(vec![button(egui::PointerButton::Primary, edge, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(far)]);
    t.frame();
    assert_eq!(panel(&t).width(), 150.0);
    t.frame_with(vec![button(egui::PointerButton::Primary, far, false)]);
}

impl TempApp {
    /// The graph, once it has been drawn.
    fn graph(&self) -> &graph::GraphView {
        self.0.graph.as_ref().expect("the graph has been drawn")
    }

    /// Open the graph and let it finish arriving: the notes appearing, the camera spinning in.
    fn open_graph(&mut self) {
        self.frame_with(vec![ctrl(egui::Key::G)]);
        for _ in 0..320 {
            self.frame();
        }
    }

    /// The first of these notes that is not lost in the graph's fog.
    fn in_the_clear<'a>(&self, titles: &[&'a str]) -> &'a str {
        let clear = |title: &&str| !self.graph().fogged[self.0.notes.iter().position(|note| note.title == **title).unwrap()];
        titles.iter().copied().find(|title| clear(title)).expect("all of them are in the fog")
    }

    /// Where the graph drew the note with this title.
    fn node(&self, title: &str) -> (usize, egui::Pos2) {
        let index = self.0.notes.iter().position(|note| note.title == title).expect("no such note");
        (index, self.graph().seen[index].expect("the note is in view"))
    }
}

#[test]
fn ctrl_g_shows_the_graph_and_takes_it_away_again() {
    let mut t = TempApp::new("graph-toggle", &[("A", "[[B]] [[C]]"), ("B", "[[A]]"), ("C", ""), ("D", "")]);
    t.frame();
    t.frame_with(vec![ctrl(egui::Key::G)]);
    assert!(t.0.show_graph);
    t.frame();
    assert_eq!((t.graph().seen.len(), t.graph().links()), (4, 2), "every note is a node, and a link each way is one line");
    t.frame_with(vec![ctrl(egui::Key::G)]);
    assert!(!t.0.show_graph);
    t.frame();
    t.frame_with(vec![ctrl(egui::Key::G)]);
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(!t.0.show_graph, "Esc closes it too");
    assert_eq!(t.0.open.as_deref(), Some("A"), "and the note that was open is still open");
}

#[test]
fn a_click_on_a_note_in_the_graph_focuses_it_and_a_second_opens_it() {
    let mut t = TempApp::new("graph-click", &[("A", "[[B]] [[C]]"), ("B", ""), ("C", ""), ("D", "")]);
    t.frame();
    t.open_graph();
    let title = t.in_the_clear(&["B", "C", "D"]);
    let (c, at) = t.node(title);
    assert_eq!(t.cursor_at(at), egui::CursorIcon::PointingHand);
    assert_eq!(t.graph().hovered, Some(c));
    let far = t.graph().camera.distance;
    t.click(at);
    assert_eq!(t.graph().focused, Some(c));
    assert_eq!(t.0.open.as_deref(), Some("A"), "the first click only looks");
    for _ in 0..120 {
        t.frame();
    }
    assert!(t.graph().camera.distance < far, "the camera has gone in to the note");
    let (_, at) = t.node(title);
    assert!(at.distance(egui::pos2(565.0, 360.0)) < 60.0, "which is now in the middle of the view, not at {at:?}");
    t.click(at);
    assert_eq!(t.0.open.as_deref(), Some(title));
    assert!(!t.0.show_graph, "the note takes the graph's place");
    t.frame();
    t.frame_with(vec![typed("!")]);
    assert_eq!(t.0.buffer, "!", "ready to be written in");
}

#[test]
fn a_note_clicked_in_the_graph_is_shown_with_its_neighbours_alone() {
    let mut t = TempApp::new("graph-local", &[("A", "[[B]] [[C]]"), ("B", ""), ("C", ""), ("D", "[[E]]"), ("E", "")]);
    t.frame();
    t.open_graph();
    let title = t.in_the_clear(&["A", "B", "C"]);
    let (_, at) = t.node(title);
    t.click(at);
    for _ in 0..120 {
        t.frame();
    }
    let shown = |t: &TempApp, title: &str| t.graph().shown[t.place(title).unwrap()];
    for near in ["A", "B", "C"].iter().filter(|near| title == "A" || **near == "A" || **near == title) {
        assert!(shown(&t, near) > 0.99, "{near} is drawn with {title}");
    }
    assert!(shown(&t, "D") < 0.01 && shown(&t, "E") < 0.01, "what it is not linked with is not");
    // Stepping back out brings every note back.
    t.keys(vec![key(egui::Key::Escape)]);
    for _ in 0..60 {
        t.frame();
    }
    assert!(t.graph().focused.is_none() && t.graph().shown.iter().all(|&shown| shown > 0.99));
}

#[test]
fn the_keyboard_opens_the_focused_note_and_steps_back_out() {
    let mut t = TempApp::new("graph-keys", &[("A", "[[B]]"), ("B", ""), ("C", "")]);
    t.frame();
    t.open_graph();
    let title = t.in_the_clear(&["B", "C", "A"]);
    let (b, at) = t.node(title);
    t.click(at);
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.show_graph && t.graph().focused.is_none(), "the first Esc lets go of the note");
    for _ in 0..90 {
        t.frame();
    }
    let (_, at) = t.node(title);
    t.click(at);
    assert_eq!(t.graph().focused, Some(b));
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some(title));
    // A click on nothing lets go as well.
    t.open_graph();
    assert_eq!(t.graph().focused, Some(b), "the graph comes back as it was left");
    t.click(egui::pos2(320.0, 110.0));
    assert!(t.graph().focused.is_none());
}

impl TempApp {
    /// What the side panel lists: its headings, and its links and backlinks.
    fn panel(&self) -> (Vec<&str>, Vec<&str>) {
        fn said(rows: &[(String, egui::Rect)]) -> Vec<&str> {
            rows.iter().map(|(text, _)| text.as_str()).collect()
        }
        (said(&self.0.outline_rows), said(&self.0.link_rows))
    }
}

#[test]
fn the_side_panel_shows_the_note_the_graph_has_in_focus() {
    let other = "[[A]] [[Later]]\n# Of another";
    let mut t = TempApp::new("graph-panel", &[("A", "# Of A\n[[B]] [[C]] [[D]]"), ("B", other), ("C", other), ("D", other)]);
    t.frame();
    t.open_graph();
    // The open note's links, and then the notes that link to it.
    let of_a = (vec!["Of A"], vec!["B", "C", "D", "B", "C", "D"]);
    assert_eq!(t.panel(), of_a, "with nothing in focus it is the open note's");
    assert_eq!(t.0.info_name, None, "which the bar above already names");
    let title = t.in_the_clear(&["B", "C", "D"]);
    let (_, at) = t.node(title);
    t.click(at);
    assert_eq!(t.0.open.as_deref(), Some("A"), "the click only looks");
    assert_eq!(t.0.info_name.as_deref(), Some(title), "and the panel is headed by the note looked at");
    assert_eq!(t.panel(), (vec!["Of another"], vec!["A", "Later (new)", "A"]), "its headings, its links, and what links to it");
    // Letting go of the note gives the panel back to the open one.
    t.keys(vec![key(egui::Key::Escape)]);
    assert_eq!((t.panel(), t.0.info_name.as_deref()), (of_a.clone(), None));
    // So does leaving the graph with a note still in focus.
    for _ in 0..90 {
        t.frame();
    }
    let (_, at) = t.node(title);
    t.click(at);
    assert_eq!(t.0.info_name.as_deref(), Some(title));
    t.keys(vec![ctrl(egui::Key::G)]);
    assert_eq!((t.panel(), t.0.info_name.as_deref()), (of_a.clone(), None));
}

#[test]
fn a_link_in_the_panel_goes_to_its_note_in_the_graph_without_opening_it() {
    let mut t = TempApp::new("graph-panel-link", &[("A", "[[B]] [[C]]"), ("B", "[[D]]"), ("C", ""), ("D", "")]);
    t.frame();
    t.open_graph();
    let link_row = |t: &TempApp, title: &str| t.0.link_rows.iter().find(|(text, _)| text == title).expect("a row for the note").1.center();
    let row = link_row(&t, "B");
    t.click(row);
    t.frame();
    assert_eq!(t.0.info_name.as_deref(), Some("B"), "the graph has B in focus");
    assert!(t.0.show_graph && t.0.open.as_deref() == Some("A"), "and nothing has been opened");
    // From there on, both ways: a link of B, and a note linking to it.
    let row = link_row(&t, "D");
    t.click(row);
    t.frame();
    assert_eq!(t.0.info_name.as_deref(), Some("D"));
    let row = link_row(&t, "B");
    t.click(row);
    t.frame();
    assert_eq!(t.0.info_name.as_deref(), Some("B"), "a backlink goes there too");
    assert!(t.0.show_graph && t.0.open.as_deref() == Some("A"));
    // It is the click in the graph that opens the note.
    for _ in 0..120 {
        t.frame();
    }
    let (_, at) = t.node("B");
    t.click(at);
    assert_eq!(t.0.open.as_deref(), Some("B"));
    assert!(!t.0.show_graph);
    // Away from the graph a link opens its note, as before.
    let row = link_row(&t, "D");
    t.click(row);
    assert_eq!(t.0.open.as_deref(), Some("D"));
}

#[test]
fn the_panel_of_the_open_note_keeps_what_is_not_saved_yet() {
    let mut t = TempApp::new("graph-panel-open", &[("A", "[[B]]"), ("B", "[[A]]")]);
    t.frame();
    t.type_text("\n# Just written");
    t.open_graph();
    let (_, at) = t.node("A");
    t.click(at);
    assert_eq!(t.0.info_name.as_deref(), Some("A"));
    assert_eq!(t.panel().0, ["Just written"]);
}

#[test]
fn a_heading_in_the_panel_takes_the_graph_away_and_goes_there() {
    let (text, far) = note_with_a_far_heading();
    let mut t = TempApp::new("graph-outline", &[("A", &format!("[[B]] [[C]] [[D]]\n{text}")), ("B", &text), ("C", &text), ("D", &text)]);
    t.frame();
    t.open_graph();
    // Of the open note: it comes back in the graph's place.
    let row = t.outline_row("Far away");
    t.click(row);
    t.frame();
    assert!(!t.0.show_graph && t.0.open.as_deref() == Some("A"));
    let in_a = far + "[[B]] [[C]] [[D]]\n".len();
    assert_eq!(t.0.selection, Some(in_a..in_a), "with the caret on the heading");
    assert!(t.0.back.is_empty(), "no note was left for it");
    // Of the note in focus: that one is opened.
    t.open_graph();
    let title = t.in_the_clear(&["B", "C", "D"]);
    let (_, at) = t.node(title);
    t.click(at);
    let row = t.outline_row("Far away");
    t.click(row);
    t.frame();
    assert!(!t.0.show_graph && t.0.open.as_deref() == Some(title));
    assert!(t.0.scroll_offset > 0.0, "the heading has been scrolled to");
    assert_eq!(t.0.selection, Some(far..far));
    t.keys(vec![typed("!")]);
    assert!(t.0.buffer.contains("\n!## Far"), "with the keyboard in the note");
}

#[test]
fn a_note_made_from_the_panel_starts_beside_the_note_in_focus() {
    let other = "[[Later]]";
    let mut t = TempApp::new("graph-new", &[("A", "[[B]] [[C]] [[D]]"), ("Deep/B", other), ("Deep/C", other), ("Deep/D", other)]);
    t.frame();
    t.open_graph();
    let title = t.in_the_clear(&["B", "C", "D"]);
    let (_, at) = t.node(title);
    t.click(at);
    let row = t.0.link_rows.iter().find(|(text, _)| text == "Later (new)").expect("the link to a note that is not there").1.center();
    t.click(row);
    assert_eq!(t.0.open.as_deref(), Some("Later"));
    assert_eq!(t.folder_of("Later"), "Deep", "beside the note that links to it, not the one that was open");
    assert!(!t.0.show_graph);
}

#[test]
fn the_names_in_the_graph_grow_evenly_as_it_is_zoomed() {
    // The note that is watched has no links, and so the smallest name: one
    // with room to grow before it is as large as names get.
    let mut t = TempApp::new("graph-zoom", &[("A long name", ""), ("B", "[[C]] [[D]]"), ("C", ""), ("D", "")]);
    t.frame();
    t.open_graph();
    // How wide the open note's name is drawn in the graph, where names are in capitals and
    // in the monospace font, and the size it was laid out at.
    let name = |t: &mut TempApp, events| {
        let shapes = t.frame_full(events).shapes;
        shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == "A LONG NAME" => {
                let font = &text.galley.job.sections[0].format.font_id;
                (font.family == egui::FontFamily::Monospace).then(|| (text.galley.rect.width(), font.size))
            }
            _ => None,
        })
    };
    let (_, at) = t.node("A long name");
    let mut seen = vec![name(&mut t, vec![egui::Event::PointerMoved(at)]).expect("the open note is named")];
    for frame in 0..90 {
        // A few notches of the wheel, and the rest of the time to coast in.
        let wheel = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, 1.0), modifiers: egui::Modifiers::NONE };
        seen.extend(name(&mut t, if frame < 6 { vec![wheel] } else { Vec::new() }));
    }
    let mut widths: Vec<f32> = seen.iter().map(|(width, _)| *width).collect();
    assert!(widths[widths.len() - 1] > widths[0] * 1.3, "zooming in makes the name larger: {widths:?}");
    let even = |pair: &[f32]| pair[1] > pair[0] * 0.999 && pair[1] < pair[0] * 1.04;
    assert!(widths.windows(2).all(even), "a little at a time: never a jump, nor a step back, from one size it is laid out at to the next: {widths:?}");
    widths.dedup();
    let mut laid_out: Vec<f32> = seen.iter().map(|(_, size)| *size).collect();
    laid_out.dedup();
    assert!(laid_out.len() > 1 && widths.len() > 10 * laid_out.len(), "through many sizes, laid out at few: {} and {laid_out:?}", widths.len());
}

#[test]
fn dragging_the_graph_turns_it_and_is_not_a_click() {
    let mut t = TempApp::new("graph-drag", &[("A", "[[B]]"), ("B", ""), ("C", "")]);
    t.frame();
    t.open_graph();
    let (_, from) = t.node(t.in_the_clear(&["A", "B", "C"]));
    let before = t.graph().camera;
    let to = from + egui::vec2(140.0, 40.0);
    t.frame_with(vec![egui::Event::PointerMoved(from)]);
    t.frame_with(vec![button(egui::PointerButton::Primary, from, true)]);
    t.frame_with(vec![egui::Event::PointerMoved(from + egui::vec2(70.0, 20.0))]);
    t.frame_with(vec![egui::Event::PointerMoved(to)]);
    t.frame_with(vec![button(egui::PointerButton::Primary, to, false)]);
    t.frame();
    let after = t.graph().camera;
    assert!(after.azimuth < before.azimuth - 0.2 && after.polar < before.polar, "{before:?} to {after:?}");
    assert!(t.graph().focused.is_none(), "though the drag began on a note");
    assert_eq!(after.distance, before.distance);
}

#[test]
fn the_graph_follows_the_notes() {
    let mut t = TempApp::new("graph-follows", &[("A", ""), ("B", ""), ("C", "")]);
    t.frame();
    t.open_graph();
    assert_eq!(t.graph().links(), 0);
    let title = t.in_the_clear(&["B", "C"]);
    let (b, at) = t.node(title);
    t.click(at);
    let camera = t.graph().camera;

    // A link written in another program, and a note added.
    fs::write(t.0.vault.root().join("A.md"), "[[C]]").unwrap();
    fs::write(t.0.vault.root().join("Aa.md"), "").unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    t.frame();
    assert_eq!((t.graph().seen.len(), t.graph().links()), (4, 1));
    assert_eq!(t.graph().focused, Some(b + 1), "the focus stays on its note, which is one further down the list now");
    let moved = t.graph().camera;
    assert!((moved.azimuth - camera.azimuth).abs() < 0.2 && (moved.distance - camera.distance).abs() < 40.0, "and the camera where it was");
}

#[test]
fn the_graph_draws_whatever_the_vault_holds() {
    for notes in [&[][..], &[("Alone", "")][..], &[("A", "[[B]]"), ("B", "[[A]]")][..]] {
        let mut t = TempApp::new("graph-odd", notes);
        t.frame();
        t.frame_with(vec![ctrl(egui::Key::G)]);
        for _ in 0..30 {
            t.frame();
        }
        assert!(t.0.show_graph && !t.0.failed());
        assert_eq!(t.graph().seen.len(), notes.len());
    }
}

#[test]
fn notes_at_the_back_of_the_graph_are_lost_in_fog() {
    let notes: Vec<(String, String)> = (0..40).map(|index| (format!("N{index}"), format!("[[N{}]]", (index * 7 + 1) % 40))).collect();
    let notes: Vec<(&str, &str)> = notes.iter().map(|(title, text)| (title.as_str(), text.as_str())).collect();
    let mut t = TempApp::new("graph-fog", &notes);
    t.frame();
    t.open_graph();
    let fogged: Vec<usize> = (0..40).filter(|&node| t.graph().fogged[node]).collect();
    assert!(!fogged.is_empty() && fogged.len() < 30, "some of the notes, not all: {}", fogged.len());
    // What cannot be seen cannot be pointed at.
    let hidden = t.graph().seen[fogged[0]].unwrap();
    t.cursor_at(hidden);
    assert_ne!(t.graph().hovered, Some(fogged[0]));
    // A neighbour of the note the mouse is on is brought out of the fog with it.
    let clear = (0..40).find(|&node| !t.graph().fogged[node] && fogged.contains(&((node * 7 + 1) % 40)));
    if let Some(node) = clear {
        let at = t.graph().seen[node].unwrap();
        t.cursor_at(at);
        for _ in 0..60 {
            t.frame();
        }
        assert_eq!(t.graph().hovered, Some(node));
        assert!(!t.graph().fogged[(node * 7 + 1) % 40]);
    }
}

impl TempApp {
    /// Where the note with this title is among the notes, which is its
    /// number in the graph.
    fn place(&self, title: &str) -> Option<usize> {
        self.0.notes.iter().position(|note| note.title == title)
    }
}

#[test]
fn ctrl_f_in_the_graph_looks_for_a_note_by_its_name_and_goes_to_it() {
    let notes = [("A", "[[Design Brief]] [[Design Notes]] [[Other]] [[Redesign]]"), ("Design Brief", ""), ("Design Notes", ""), ("Other", ""), ("Redesign", "")];
    let mut t = TempApp::new("graph-find", &notes);
    t.frame();
    t.open_graph();
    t.keys(vec![ctrl(egui::Key::F)]);
    assert!(t.0.show_graph && t.0.graph_find.is_some() && t.0.find.is_none(), "the graph stays, with a bar of its own");
    t.keys(vec![typed("design")]);
    assert_eq!(t.graph().focused, None, "typing shows what is found and goes to none of it");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Design Brief"), "Enter goes to the first of the names that start so");
    assert_eq!(t.0.info_name.as_deref(), Some("Design Brief"), "and the side panel is that note's");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Design Notes"));
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Redesign"), "a name with it inside comes after those that start with it");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Design Brief"), "and round again");
    t.keys(vec![shift(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Redesign"));
    assert!(t.0.show_graph && t.0.open.as_deref() == Some("A"), "Enter in the bar opens nothing");
    // The arrows beside the field do the same with the mouse.
    let (before, after) = (t.spot("Note before"), t.spot("Next note"));
    t.click(after);
    assert_eq!(t.graph().focused, t.place("Design Brief"));
    t.click(before);
    assert_eq!(t.graph().focused, t.place("Redesign"));
    // A click on an arrow has taken the keyboard from the field; Ctrl+F gives it back.
    t.keys(vec![ctrl(egui::Key::F), key(egui::Key::End)]);
    // Typing on narrows it down, and lets go of a note it no longer finds.
    t.keys(vec![typed(" b")]);
    assert_eq!(t.graph().focused, None);
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place("Design Brief"));
    // Esc puts the bar away and does nothing else; after that the keys are the graph's.
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.graph_find.is_none() && t.0.show_graph);
    assert_eq!(t.graph().focused, t.place("Design Brief"), "the graph is still on the note it went to");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Design Brief"));
    assert!(!t.0.show_graph);
}

#[test]
fn the_graph_shows_only_the_notes_found_and_their_neighbours_gathered_together() {
    let notes = [("A", "[[Design Brief]] [[Design Notes]] [[Other]] [[Redesign]]"), ("Design Brief", "[[Design Notes]]"), ("Design Notes", ""), ("Other", ""), ("Redesign", "")];
    let mut t = TempApp::new("graph-find-only", &notes);
    t.frame();
    t.open_graph();
    let found = ["Design Brief", "Design Notes", "Redesign"].map(|title| t.place(title).unwrap());
    let (a, other) = (t.place("A").unwrap(), t.place("Other").unwrap());
    let resting: Vec<_> = found.iter().map(|&node| t.graph().at[node]).collect();
    t.keys(vec![ctrl(egui::Key::F), typed("design")]);
    for _ in 0..120 {
        t.frame();
    }
    let graph = t.graph();
    assert!(graph.shown[other] < 0.01, "the notes not found are gone: {:?}", graph.shown);
    assert!(found.iter().all(|&node| graph.shown[node] > 0.99));
    assert!(graph.shown[a] > 0.99, "but not those linked with them");
    // They are gathered about the middle, nearer each other than where they rest.
    let spread = |at: &[malachite::graph::Vec3]| at.iter().map(|a| at.iter().map(|b| a.distance(*b)).fold(0.0, f32::max)).fold(0.0, f32::max);
    let now: Vec<_> = found.iter().map(|&node| graph.at[node]).collect();
    let middle = now.iter().fold(malachite::graph::Vec3::ZERO, |sum, &at| sum + at) * (1.0 / 3.0);
    assert!(middle.length() < 15.0, "about the middle: {middle:?}");
    assert!(spread(&now) < spread(&resting), "{now:?} against {resting:?}");
    // What is not drawn cannot be pointed at.
    let gone = graph.seen[other];
    if let Some(gone) = gone {
        t.cursor_at(gone);
        assert_ne!(t.graph().hovered, Some(other));
    }
    // Esc puts the bar away, and everything comes back where it was.
    t.keys(vec![key(egui::Key::Escape)]);
    for _ in 0..300 {
        t.frame();
    }
    let graph = t.graph();
    assert!(graph.shown.iter().all(|&shown| shown > 0.99));
    let back: Vec<_> = found.iter().map(|&node| graph.at[node]).collect();
    assert!(back.iter().zip(&resting).all(|(back, rest)| back.distance(*rest) < 12.0), "{back:?} against {resting:?}");
}

#[test]
fn a_click_on_nothing_while_searching_lets_go_of_the_note_and_the_search() {
    let notes = [("A", "[[Design Brief]] [[Design Notes]]"), ("Design Brief", ""), ("Design Notes", "")];
    let mut t = TempApp::new("graph-find-click", &notes);
    t.frame();
    t.open_graph();
    t.keys(vec![ctrl(egui::Key::F), typed("design"), key(egui::Key::Enter)]);
    for _ in 0..120 {
        t.frame();
    }
    assert_eq!(t.graph().focused, t.place("Design Brief"));
    // A corner of the graph, far from the notes gathered in the middle.
    let corner = egui::pos2(t.spot("Next note").x, 600.0);
    t.click(corner);
    assert!(t.0.graph_find.is_none() && t.0.show_graph, "the bar is put away");
    assert_eq!(t.graph().focused, None);
}

#[test]
fn a_name_no_note_has_leaves_the_graph_where_it_was() {
    let mut t = TempApp::new("graph-find-none", &[("A", "[[B]] [[C]]"), ("B", ""), ("C", "")]);
    t.frame();
    t.open_graph();
    let title = t.in_the_clear(&["B", "C"]);
    let (node, at) = t.node(title);
    t.click(at);
    t.keys(vec![ctrl(egui::Key::F), typed("zebra")]);
    assert_eq!(t.graph().focused, Some(node));
    // The bar takes one Esc, the note the next, the graph the one after.
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.graph_find.is_none());
    assert_eq!(t.graph().focused, Some(node));
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.show_graph && t.graph().focused.is_none());
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(!t.0.show_graph);
}

#[test]
fn the_bar_of_the_graph_goes_away_with_the_graph_and_finds_notes_in_the_fog() {
    let notes: Vec<(String, String)> = (0..40).map(|index| (format!("N{index}"), format!("[[N{}]]", (index * 7 + 1) % 40))).collect();
    let notes: Vec<(&str, &str)> = notes.iter().map(|(title, text)| (title.as_str(), text.as_str())).collect();
    let mut t = TempApp::new("graph-find-fog", &notes);
    t.frame();
    t.open_graph();
    // A note the mouse cannot reach, and whose name is the start of no other.
    let hidden = (10..40).find(|&node| t.graph().fogged[t.place(&format!("N{node}")).unwrap()]).expect("a note in the fog");
    let title = format!("N{hidden}");
    t.keys(vec![ctrl(egui::Key::F), typed(&title)]);
    for _ in 0..120 {
        t.frame();
    }
    assert!(!t.graph().fogged[t.place(&title).unwrap()], "it has been brought out to be seen");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.graph().focused, t.place(&title));
    // Ctrl+G takes the graph away, and the bar with it: Ctrl+F is the note's again.
    t.keys(vec![ctrl(egui::Key::G)]);
    assert!(!t.0.show_graph && t.0.graph_find.is_none());
    t.keys(vec![ctrl(egui::Key::F)]);
    assert!(t.0.find.is_some() && t.0.graph_find.is_none());
}

impl TempApp {
    /// Do what picking this command in the palette does.
    fn run(&mut self, id: &str) {
        let command = commands::COMMANDS.iter().find(|command| command.id == id).expect("no such command");
        self.0.apply(Action::Run(command));
        self.frame();
        self.frame();
    }

    /// The middle of the outline's row for this heading.
    fn outline_row(&mut self, title: &str) -> egui::Pos2 {
        self.frame();
        let found = self.0.outline_rows.iter().find(|(text, _)| text == title);
        found.unwrap_or_else(|| panic!("no '{title}' in the outline")).1.center()
    }

    /// The middle of the fold arrow beside this line of the note.
    fn fold_arrow(&mut self, line: &str) -> egui::Pos2 {
        self.frame();
        self.frame();
        let found = self.0.fold_spots.iter().find(|(_, text)| text == line);
        found.unwrap_or_else(|| panic!("'{line}' does not fold")).0
    }

    /// Whether this line of the open note is folded.
    fn folded(&self, line: &str) -> bool {
        let folds = self.0.open_path.as_ref().and_then(|file| self.0.folds.get(file));
        folds.is_some_and(|folds| folds.contains(line))
    }
}

/// A note with a heading a long way down it, and where that heading is.
fn note_with_a_far_heading() -> (String, usize) {
    let filler = "a line of filler\n".repeat(80);
    let text = format!("# Top\n{filler}## Far **away**\nmore ^blk\n{filler}");
    let far = text.find("## Far").unwrap();
    (text, far)
}

#[test]
fn the_outline_lists_the_headings_and_a_click_goes_to_one() {
    let (text, far) = note_with_a_far_heading();
    let mut t = TempApp::new("outline", &[("A", &text)]);
    t.frame();
    let listed: Vec<&str> = t.0.outline_rows.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(listed, ["Top", "Far away"], "as they read, without their syntax");
    let row = t.outline_row("Far away");
    t.click(row);
    t.frame();
    assert!(t.0.scroll_offset > 0.0, "the heading has been scrolled to");
    assert_eq!(t.0.selection, Some(far..far), "and the caret is on it");
    t.keys(vec![typed("!")]);
    assert!(t.0.buffer.contains("\n!## Far"), "with the keyboard in the note");
}

#[test]
fn the_outline_scrolls_reading_view_as_well() {
    let (text, _) = note_with_a_far_heading();
    let mut t = TempApp::new("outline-reading", &[("A", &text)]);
    t.0.set_mode(Mode::Reading);
    let row = t.outline_row("Far away");
    t.click(row);
    t.frame();
    assert!(t.0.mode == Mode::Reading && t.0.scroll_offset > 0.0);
    assert_eq!(t.0.buffer, text);
}

#[test]
fn a_link_to_a_heading_or_a_block_opens_the_note_there() {
    let (text, far) = note_with_a_far_heading();
    let mut t = TempApp::new("places", &[("A", "[[B#far away]] [[B#^blk]]"), ("B", &text)]);
    t.frame();
    t.0.apply(Action::Follow("B".into(), Some(Place::Heading("far away".into()))));
    t.frame();
    t.frame();
    assert_eq!(t.0.open.as_deref(), Some("B"));
    assert!(t.0.scroll_offset > 0.0);
    assert_eq!(t.0.selection, Some(far..far));

    t.0.apply(Action::Open("A".into()));
    t.0.apply(Action::Follow("B".into(), Some(Place::Block("blk".into()))));
    t.frame();
    t.frame();
    let block = text.find("more ^blk").unwrap();
    assert_eq!(t.0.selection, Some(block..block));
}

#[test]
fn a_link_with_no_note_in_it_points_into_the_open_note() {
    let (text, far) = note_with_a_far_heading();
    let mut t = TempApp::new("places-here", &[("A", &text)]);
    t.frame();
    let ctx = egui::Context::default();
    let mut actions = Vec::new();
    let here = |name: &str| Hit::Link(Link::Note(String::new(), Some(Place::Heading(name.into()))));
    t.0.on_hit(&ctx, here("Far away"), &mut actions);
    t.frame();
    t.frame();
    assert_eq!(t.0.selection, Some(far..far));
    t.0.on_hit(&ctx, here("Nowhere"), &mut actions);
    assert!(actions.is_empty() && t.0.notes.len() == 1, "nothing is opened or made");
    assert!(t.0.message.as_ref().is_some_and(|message| message.text.contains("no heading")));
}

#[test]
fn a_link_to_a_heading_of_a_note_that_is_not_there_makes_the_note() {
    let mut t = TempApp::new("places-new", &[("A", "[[New#Part]]")]);
    t.0.apply(Action::Follow("New".into(), Some(Place::Heading("Part".into()))));
    assert_eq!(t.0.open.as_deref(), Some("New"));
    assert!(!t.0.failed());
}

const FOLDING: &str = "# A\none\ntwo\nthree\n# B\nend";

#[test]
fn the_arrow_beside_a_heading_folds_what_is_under_it() {
    let mut t = TempApp::new("fold-heading", &[("A", FOLDING)]);
    let below = t.fold_arrow("# B").y;
    let arrow = t.fold_arrow("# A");
    t.click(arrow);
    assert!(t.folded("# A") && !t.folded("# B"));
    let closed = t.fold_arrow("# B").y;
    assert!(closed < below - 30.0, "what follows has moved up: from {below} to {closed}");
    t.click(arrow);
    assert!(!t.folded("# A"));
    assert_eq!(t.fold_arrow("# B").y, below, "and back down");
    assert!(t.0.buffer == FOLDING && !t.0.dirty, "folding does not touch the note");
}

#[test]
fn list_items_fold_the_items_nested_in_them() {
    let mut t = TempApp::new("fold-list", &[("A", "- a\n  - b\n  - c\n- d\n  - e")]);
    let below = t.fold_arrow("- d").y;
    let arrow = t.fold_arrow("- a");
    t.click(arrow);
    assert!(t.folded("- a"));
    assert!(t.fold_arrow("- d").y < below - 20.0);
}

#[test]
fn the_caret_never_stays_inside_a_fold() {
    let mut t = TempApp::new("fold-caret", &[("A", FOLDING)]);
    t.frame();
    t.focus_editor();
    t.select(5, 5);
    t.run("editor:fold-all");
    assert!(t.folded("# A") && t.folded("# B"));
    assert_eq!(t.0.selection, Some(3..3), "it has moved out, to the end of the heading");
    // Moving into what is folded opens it, as typing there would have to.
    t.keys(vec![key(egui::Key::ArrowRight)]);
    assert!(!t.folded("# A") && t.folded("# B"));
    // So does being sent there: by a search, the find bar or a link.
    let end = FOLDING.len();
    t.0.apply(Action::Jump(end - 3..end));
    t.frame();
    t.frame();
    assert!(!t.folded("# B"));
}

#[test]
fn folds_are_worked_from_the_command_palette() {
    let mut t = TempApp::new("fold-commands", &[("A", FOLDING)]);
    t.frame();
    t.focus_editor();
    t.select(1, 1);
    t.keys(vec![ctrl(egui::Key::P), typed("toggle fold"), key(egui::Key::Enter)]);
    t.frame();
    assert!(t.folded("# A") && !t.folded("# B"));
    t.run("editor:toggle-fold");
    assert!(!t.folded("# A"));
    t.select(5, 5);
    t.run("editor:toggle-fold");
    assert!(!t.folded("# A"), "a line with nothing under it does not fold");
    t.run("editor:fold-all");
    t.run("editor:unfold-all");
    assert!(!t.folded("# A") && !t.folded("# B"));
}

#[test]
fn a_fold_stays_with_its_line_while_the_note_is_edited_elsewhere() {
    let mut t = TempApp::new("fold-edit", &[("A", FOLDING)]);
    t.frame();
    t.focus_editor();
    t.select(0, 0);
    t.0.fold_all(true);
    t.keys(vec![typed("new first line"), key(egui::Key::Enter)]);
    assert!(t.0.buffer.starts_with("new first line\n# A\n"));
    assert!(t.folded("# A") && t.folded("# B"));
    let mut modes = TempApp::new("fold-modes", &[("A", FOLDING)]);
    modes.0.fold_all(true);
    for mode in [Mode::Source, Mode::Reading, Mode::Live] {
        modes.0.set_mode(mode);
        modes.frame();
        modes.frame();
    }
    assert_eq!(modes.0.buffer, FOLDING);
}

impl TempApp {
    /// Each piece of text a frame draws, as it reads. Reading view draws
    /// every line of a note as a piece of its own.
    fn texts_drawn(&mut self) -> Vec<String> {
        let shapes = self.frame_full(Vec::new()).shapes;
        let text = |shape: &egui::epaint::ClippedShape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        };
        shapes.iter().filter_map(text).collect()
    }
}

/// Whether `shapes` hold a fold arrow drawn at `spot`, the middle of where one goes.
fn arrow_drawn(shapes: &[egui::epaint::ClippedShape], spot: egui::Pos2) -> bool {
    let arrow = |shape: &egui::epaint::ClippedShape| match &shape.shape {
        egui::Shape::Path(path) => path.closed && !path.points.is_empty() && path.points.iter().all(|point| point.distance(spot) < 6.0),
        _ => false,
    };
    shapes.iter().any(arrow)
}

#[test]
fn a_fold_closed_in_live_preview_stays_closed_in_reading_view() {
    let mut t = TempApp::new("fold-to-reading", &[("A", FOLDING)]);
    t.0.set_mode(Mode::Reading);
    let open = t.fold_arrow("# B").y;
    assert!(t.texts_drawn().contains(&"two".to_owned()));
    t.keys(vec![ctrl(egui::Key::E)]);
    assert!(t.0.mode == Mode::Live);
    let arrow = t.fold_arrow("# A");
    t.click(arrow);
    assert!(t.folded("# A"));
    t.keys(vec![ctrl(egui::Key::E)]);
    assert!(t.0.mode == Mode::Reading && t.folded("# A"));
    let closed = t.fold_arrow("# B").y;
    assert!(closed < open - 30.0, "what follows has moved up: from {open} to {closed}");
    let drawn = t.texts_drawn();
    assert!(!drawn.iter().any(|text| ["one", "two", "three"].contains(&text.as_str())), "what is under # A is not drawn: {drawn:?}");
    assert!(drawn.contains(&"…".to_owned()), "and the heading ends in dots");
}

#[test]
fn clicking_a_fold_arrow_in_reading_view_folds_it() {
    for (text, line, after) in [(FOLDING, "# A", "# B"), ("- a\n  - b\n  - c\n- d\n  - e", "- a", "- d")] {
        let mut t = TempApp::new("fold-reading", &[("A", text)]);
        t.0.set_mode(Mode::Reading);
        let below = t.fold_arrow(after).y;
        let arrow = t.fold_arrow(line);
        t.click(arrow);
        assert!(t.folded(line) && !t.folded(after), "{line}");
        let closed = t.fold_arrow(after).y;
        assert!(closed < below - 20.0, "what follows {line} has moved up: from {below} to {closed}");
        t.click(arrow);
        assert!(!t.folded(line));
        assert_eq!(t.fold_arrow(after).y, below, "and back down");
        assert!(t.0.buffer == text && !t.0.dirty, "folding does not touch the note");
        assert!(t.0.mode == Mode::Reading);
    }
}

#[test]
fn a_fold_closed_in_reading_view_stays_closed_in_live_preview() {
    let mut t = TempApp::new("fold-from-reading", &[("A", FOLDING)]);
    let open = t.fold_arrow("# B").y;
    // With the caret in what is about to be folded.
    t.focus_editor();
    t.select(5, 5);
    t.keys(vec![ctrl(egui::Key::E)]);
    assert!(t.0.mode == Mode::Reading);
    let arrow = t.fold_arrow("# A");
    t.click(arrow);
    assert!(t.folded("# A"));
    t.keys(vec![ctrl(egui::Key::E)]);
    assert!(t.0.mode == Mode::Live && t.folded("# A"));
    let closed = t.fold_arrow("# B").y;
    assert!(closed < open - 30.0, "what follows has moved up: from {open} to {closed}");
    // The caret has been moved out of the fold, as folding in the editor
    // moves it, so taking up the editor again leaves the fold closed.
    t.focus_editor();
    assert!(t.folded("# A"));
    assert_eq!(t.0.selection, Some(3..3));
}

#[test]
fn reading_view_shows_an_arrow_beside_a_folded_line_and_the_line_under_the_mouse() {
    let mut t = TempApp::new("fold-reading-arrows", &[("A", FOLDING)]);
    t.0.set_mode(Mode::Reading);
    let (a, b) = (t.fold_arrow("# A"), t.fold_arrow("# B"));
    let shapes = t.frame_full(Vec::new()).shapes;
    assert!(!arrow_drawn(&shapes, a) && !arrow_drawn(&shapes, b), "none while the mouse is not over the note");
    let shapes = t.frame_full(vec![egui::Event::PointerMoved(b + egui::vec2(200.0, 0.0))]).shapes;
    assert!(arrow_drawn(&shapes, b) && !arrow_drawn(&shapes, a), "one beside the line the mouse is on");

    t.click(a);
    let b = t.fold_arrow("# B");
    let shapes = t.frame_full(vec![egui::Event::PointerMoved(b + egui::vec2(200.0, 0.0))]).shapes;
    assert!(arrow_drawn(&shapes, a) && arrow_drawn(&shapes, b));
    let shapes = t.frame_full(vec![egui::Event::PointerMoved(b + egui::vec2(200.0, 100.0))]).shapes;
    assert!(arrow_drawn(&shapes, a) && !arrow_drawn(&shapes, b), "the folded line keeps its arrow");
}

#[test]
fn going_to_a_place_in_reading_view_opens_the_fold_it_is_in() {
    let (text, _) = note_with_a_far_heading();
    let mut t = TempApp::new("fold-reading-jump", &[("A", &text)]);
    t.0.set_mode(Mode::Reading);
    let arrow = t.fold_arrow("# Top");
    t.click(arrow);
    assert!(t.folded("# Top"));
    let row = t.outline_row("Far away");
    t.click(row);
    t.frame();
    assert!(!t.folded("# Top"), "the fold the heading was in has opened");
    assert!(t.0.mode == Mode::Reading && t.0.scroll_offset > 0.0, "and the heading has been scrolled to");

    // A footnote is listed at the end of the note whatever is folded, so
    // going to one opens nothing.
    let text = "# A\nsee[^1]\n# B\nmore\n\n[^1]: the note";
    let mut t = TempApp::new("fold-reading-footnote", &[("A", text)]);
    t.0.set_mode(Mode::Reading);
    let arrow = t.fold_arrow("# B");
    t.click(arrow);
    let ctx = t.1.clone();
    t.0.on_hit(&ctx, Hit::Link(Link::Footnote("1".into())), &mut Vec::new());
    t.frame();
    t.frame();
    assert!(t.folded("# B"));
    assert!(t.texts_drawn().contains(&"the note".to_owned()));
    // Anything else under the folded line opens it.
    let more = text.find("more").unwrap();
    t.0.apply(Action::Jump(more..more + 4));
    t.frame();
    t.frame();
    assert!(!t.folded("# B"));
}

#[test]
fn the_status_bar_counts_words_and_characters() {
    let mut t = TempApp::new("counts", &[("A", "one two\nthré")]);
    t.frame();
    assert_eq!(t.0.counted, "3 words · 12 characters");
    t.focus_editor();
    t.select(0, 7);
    t.frame();
    assert_eq!(t.0.counted, "2 words · 7 characters", "of the selection, while there is one");
}

#[test]
fn a_template_is_inserted_at_the_caret_with_its_blanks_filled_in() {
    let mut t = TempApp::new("template", &[("Templates/Meeting", "## {{title}} in {{date:YYYY}}\n"), ("Templates/Other", "o"), ("Day", "x")]);
    t.0.apply(Action::Open("Day".into()));
    t.frame();
    t.focus_editor();
    t.keys(vec![ctrl(egui::Key::P), typed("insert template"), key(egui::Key::Enter)]);
    assert_eq!(t.0.picked_from, ["Meeting", "Other"], "the notes in the Templates folder, and no others");
    t.keys(vec![typed("meet"), key(egui::Key::Enter)]);
    t.frame();
    let year = chrono::Local::now().format("%Y");
    assert_eq!(t.0.buffer, format!("x## Day in {year}\n"));
    assert!(t.0.dirty && t.0.picker.is_none());
    t.keys(vec![typed("!")]);
    assert!(t.0.buffer.ends_with("\n!"), "the caret is after what was put in");
}

#[test]
fn inserting_a_template_needs_templates_and_a_note() {
    let mut t = TempApp::new("template-none", &[("A", "x")]);
    t.run("insert-template");
    assert!(t.0.failed() && t.0.picker.is_none());
    let mut empty = TempApp::new("template-no-note", &[]);
    empty.run("insert-template");
    assert!(empty.0.failed());
}

#[test]
fn a_note_answers_to_its_aliases() {
    let mut t = TempApp::new("aliases", &[("A", "[[LN]]"), ("Long name", "---\naliases: [LN, Elle]\n---\nbody")]);
    t.frame();
    t.0.apply(Action::Follow("ln".into(), None));
    assert_eq!((t.0.open.as_deref(), t.0.notes.len()), (Some("Long name"), 2), "opened, not made anew");
    t.0.apply(Action::Open("A".into()));
    t.keys(vec![ctrl(egui::Key::O), typed("elle")]);
    assert_eq!(t.0.picked_from, ["Elle"]);
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Long name"));
}

#[test]
fn daily_notes_lead_to_the_one_before_and_the_one_after() {
    let mut t = TempApp::new("daily-step", &[("2026-03-01", ""), ("2026-03-05", ""), ("2026-03-09", ""), ("Plan", "")]);
    t.0.apply(Action::Open("2026-03-05".into()));
    t.run("daily-notes:goto-prev");
    assert_eq!(t.0.open.as_deref(), Some("2026-03-01"));
    t.run("daily-notes:goto-prev");
    assert_eq!(t.0.open.as_deref(), Some("2026-03-01"), "there is none before it");
    assert!(t.0.message.is_some() && !t.0.failed());
    t.run("daily-notes:goto-next");
    t.run("daily-notes:goto-next");
    assert_eq!(t.0.open.as_deref(), Some("2026-03-09"));
    // From any other note, it is the days around today.
    t.0.apply(Action::Open("Plan".into()));
    t.run("daily-notes:goto-prev");
    assert_eq!(t.0.open.as_deref(), Some("2026-03-09"));
}

impl TempApp {
    /// Right-click the note where its text starts, which brings up its menu.
    fn open_note_menu(&mut self) {
        let pos = egui::pos2(450.0, self.0.content_top + 12.0);
        self.frame_with(vec![egui::Event::PointerMoved(pos)]);
        self.frame_with(vec![button(egui::PointerButton::Secondary, pos, true)]);
        self.frame_with(vec![button(egui::PointerButton::Secondary, pos, false)]);
        self.frame();
        self.frame();
    }

    /// Click a choice of the note's right-click menu, which is up.
    fn pick_from_menu(&mut self, name: &str) {
        let found = self.0.menu_spots.iter().find(|(text, _)| text == name);
        let spot = found.unwrap_or_else(|| panic!("no '{name}' in the menu")).1;
        self.click(spot);
        self.frame();
    }
}

#[test]
fn a_right_click_offers_colours_to_highlight_the_selection_in() {
    let mut t = TempApp::new("highlight-menu", &[("A", "a plain word here")]);
    t.frame();
    t.focus_editor();
    t.select(8, 12);
    t.open_note_menu();
    let offered: Vec<&str> = t.0.menu_spots.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(offered, ["Yellow", "Red", "Orange", "Green", "Cyan", "Blue", "Purple", "Your colour", "Remove highlight"]);
    assert_eq!(t.0.selection, Some(8..12), "the right click has not moved the selection");
    t.pick_from_menu("Red");
    assert_eq!(t.0.buffer, "a plain <mark style=\"background: #E9314766;\">word</mark> here");
    assert!(t.0.dirty && t.0.menu_spots.is_empty(), "and the menu has gone");

    // From inside the highlight, another colour changes it; yellow is the plain highlight.
    t.open_note_menu();
    t.pick_from_menu("Yellow");
    assert_eq!(t.0.buffer, "a plain ==word== here");
    t.open_note_menu();
    t.pick_from_menu("Your colour");
    assert_eq!(t.0.buffer, "a plain <mark style=\"background: #D5398466;\">word</mark> here");
    t.open_note_menu();
    t.pick_from_menu("Remove highlight");
    assert_eq!(t.0.buffer, "a plain word here");
    t.keys(vec![typed("!")]);
    assert_eq!(t.0.buffer, "a plain ! here", "the word is still selected, with the keyboard in the note");
}

#[test]
fn removing_a_highlight_that_is_not_there_only_says_so() {
    let mut t = TempApp::new("highlight-none", &[("A", "plain")]);
    t.frame();
    t.focus_editor();
    t.select(1, 3);
    t.open_note_menu();
    t.pick_from_menu("Remove highlight");
    assert!(t.0.buffer == "plain" && !t.0.dirty);
    assert!(t.0.message.is_some() && !t.0.failed());
}

#[test]
fn coloured_highlights_draw_in_every_mode() {
    let text = "a <mark style=\"background: #E9314766;\">red **bold**</mark> and <mark>plain</mark> and ==yellow==\n";
    let mut t = TempApp::new("highlight-modes", &[("A", text)]);
    for mode in [Mode::Live, Mode::Source, Mode::Reading] {
        t.0.set_mode(mode);
        t.frame();
        t.frame();
    }
    assert_eq!(t.0.buffer, text);
}

#[test]
fn the_colour_picked_for_highlights_is_remembered_for_the_next_run() {
    let dir = std::env::temp_dir().join(format!("malachite-test-{}-highlight-kept", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let settings = dir.join("settings.txt");
    let vault = dir.join("vault");
    let mut t = TempApp::start(vault.clone(), Some(settings.clone()));
    t.0.highlight = [1, 2, 3];
    t.frame();
    drop(t);
    fs::create_dir_all(&vault).unwrap();
    let again = TempApp::start(vault, Some(settings));
    assert_eq!(again.0.highlight, [1, 2, 3]);
    drop(again);
    let _ = fs::remove_dir_all(&dir);
}

impl TempApp {
    /// Put a picture of this size in the vault, as a PNG file.
    fn add_picture(&mut self, name: &str, width: u32, height: u32) {
        let path = self.0.vault.root().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(width, height, image::Rgba([200, 40, 40, 255])).save(path).unwrap();
    }

    /// How far down the note this line of its text is drawn, by the fold
    /// arrow it has: a way to see what is above it grow and shrink.
    fn line_top(&mut self, line: &str) -> f32 {
        self.fold_arrow(line).y
    }
}

/// A note that embeds `what` on a line of its own, above a heading that can
/// be found on screen.
fn embedding(what: &str) -> String {
    format!("intro\n{what}\n# After\ntext")
}

#[test]
fn a_picture_is_shown_under_the_line_that_embeds_it() {
    let mut t = TempApp::new("picture", &[("A", &embedding("![[pic.png]]")), ("B", &embedding("nothing here"))]);
    t.add_picture("img/pic.png", 300, 120);
    t.0.apply(Action::Open("B".into()));
    let plain = t.line_top("# After");
    t.0.apply(Action::Open("A".into()));
    let with_picture = t.line_top("# After");
    assert!((with_picture - plain - 120.0).abs() < 24.0, "room for the picture's 120: {plain} to {with_picture}");
    assert_eq!(t.0.buffer, embedding("![[pic.png]]"));
    assert!(!t.0.dirty);
}

#[test]
fn a_picture_is_drawn_at_the_width_its_embed_asks_for() {
    let mut t = TempApp::new("picture-width", &[("A", &embedding("![[pic.png]]")), ("B", &embedding("![[pic.png|150]]")), ("C", &embedding("![x|150](pic.png)"))]);
    t.add_picture("pic.png", 300, 120);
    let full = t.line_top("# After");
    for note in ["B", "C"] {
        t.0.apply(Action::Open(note.into()));
        let half = t.line_top("# After");
        assert!((full - half - 60.0).abs() < 4.0, "{note}: half as wide is half as tall, {full} to {half}");
    }
}

#[test]
fn a_picture_that_is_missing_is_left_as_its_text() {
    let mut t = TempApp::new("picture-missing", &[("A", &embedding("![[nowhere.png]]")), ("B", &embedding("nothing here"))]);
    let missing = t.line_top("# After");
    t.0.apply(Action::Open("B".into()));
    assert_eq!(t.line_top("# After"), missing);
    // A file that is not a picture after all is no worse.
    fs::write(t.0.vault.root().join("nowhere.png"), "not a picture").unwrap();
    t.0.refresh_from_disk();
    t.0.apply(Action::Open("A".into()));
    assert_eq!(t.line_top("# After"), missing);
    for mode in [Mode::Reading, Mode::Source] {
        t.0.set_mode(mode);
        t.frame();
        t.frame();
    }
}

#[test]
fn a_picture_changed_on_disk_is_read_again() {
    let mut t = TempApp::new("picture-changed", &[("A", &embedding("![[pic.png]]"))]);
    t.add_picture("pic.png", 300, 60);
    let short = t.line_top("# After");
    t.add_picture("pic.png", 300, 180);
    // Coming back to the window is when files are looked at again.
    t.0.refresh_from_disk();
    let tall = t.line_top("# After");
    assert!((tall - short - 120.0).abs() < 4.0, "{short} to {tall}");
}

impl TempApp {
    /// Where the editor's caret is drawn, from its top to its bottom, and
    /// where the pictures in the note are: looked for over a second of
    /// frames, as the caret is only there for half of every second.
    fn caret_and_pictures(&mut self) -> (Option<[egui::Pos2; 2]>, Vec<egui::Rect>) {
        let color = self.1.style().visuals.text_cursor.stroke.color;
        let mut found = (None, Vec::new());
        for _ in 0..60 {
            let (mut caret, mut pictures) = (None, Vec::new());
            for clipped in self.frame_full(Vec::new()).shapes {
                match clipped.shape {
                    egui::Shape::LineSegment { points, stroke } if stroke.color == color && stroke.width > 0.0 => caret = Some(points),
                    egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => pictures.push(mesh.calc_bounds()),
                    _ => {}
                }
            }
            if caret.is_some() {
                return (caret, pictures);
            }
            found = (caret, pictures);
        }
        found
    }
}

#[test]
fn the_caret_on_the_line_of_a_picture_stays_beside_the_text_of_that_line() {
    let text = "intro
![[pic.png]]
```
code
```";
    let mut t = TempApp::new("picture-caret", &[("A", text)]);
    t.add_picture("pic.png", 300, 120);
    t.focus_editor();
    t.select(2, 2);
    let (plain, _) = t.caret_and_pictures();
    let [top, bottom] = plain.expect("a caret on an ordinary line");
    let ordinary = bottom.y - top.y;
    assert!(ordinary > 16.0 && ordinary < 40.0, "as tall as a line of text: {ordinary}");

    // The row of the picture's line is the line and the picture under it.
    let end = text.find("]]").unwrap() + 2;
    t.select(end, end);
    let (caret, pictures) = t.caret_and_pictures();
    let ([top, bottom], [picture]) = (caret.expect("a caret on the picture's line"), &pictures[..]) else {
        panic!("one picture, not {pictures:?}")
    };
    assert!(bottom.y < picture.top(), "the caret ends at {} above the picture at {}", bottom.y, picture.top());
    assert!((bottom.y - top.y - ordinary).abs() < 4.0, "and is as tall as any: {} for {ordinary}", bottom.y - top.y);
    assert!(top.x > picture.left() + 20.0, "after the line's text, which is showing");

    // Under the picture the caret is whole, with nothing painted over its top.
    let under = text.find("```").unwrap();
    t.select(under, under);
    let (caret, pictures) = t.caret_and_pictures();
    let [top, bottom] = caret.expect("a caret under the picture");
    assert!(top.y > pictures[0].bottom() && bottom.y - top.y > 16.0, "{top:?} to {bottom:?} under {:?}", pictures[0]);
}

#[test]
fn the_caret_shows_only_while_the_editor_has_the_keyboard_and_blinks() {
    let mut t = TempApp::new("caret", &[("A", "some text")]);
    t.frame();
    assert!(t.caret_and_pictures().0.is_none(), "nothing to type into yet");
    t.focus_editor();
    assert!(t.caret_and_pictures().0.is_some());
    let color = t.1.style().visuals.text_cursor.stroke.color;
    let is_caret = |s: &egui::epaint::ClippedShape| matches!(s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == color && stroke.width > 0.0);
    let carets = |t: &mut TempApp, events| t.frame_full(events).shapes.iter().filter(|s| is_caret(s)).count();
    let shown: Vec<usize> = (0..70).map(|_| carets(&mut t, Vec::new())).collect();
    assert!(shown.contains(&0) && shown.contains(&1), "it comes and goes: {shown:?}");
    assert!(shown.iter().all(|carets| *carets <= 1), "and is drawn once, not by the text field as well: {shown:?}");
    // Typing brings it back at once, and it stays while the typing goes on.
    while carets(&mut t, Vec::new()) == 1 {}
    assert_eq!(carets(&mut t, vec![typed("x")]), 1);
    assert_eq!(t.0.buffer, "some textx");
}

#[test]
fn an_embedded_note_is_shown_in_the_note_that_embeds_it() {
    let other = "---\ntags: [x]\n---\n# Other\none\ntwo\nthree\nfour\n## Part\nonly this\n";
    let mut t = TempApp::new("embed-note", &[("A", &embedding("![[Other]]")), ("B", &embedding("![[Other#Part]]")), ("C", &embedding("nothing here")), ("Other", other)]);
    t.0.apply(Action::Open("C".into()));
    let plain = t.line_top("# After");
    t.0.apply(Action::Open("A".into()));
    // Its height is measured as it is drawn: give that a few frames.
    for _ in 0..4 {
        t.frame();
    }
    let whole = t.line_top("# After");
    t.0.apply(Action::Open("B".into()));
    for _ in 0..4 {
        t.frame();
    }
    let part = t.line_top("# After");
    assert!(whole > plain + 120.0, "the whole note takes room: {plain} to {whole}");
    assert!(part > plain + 30.0 && part < whole - 60.0, "one heading of it takes less: {part}");
    assert!(!t.0.dirty);
}

#[test]
fn embeds_and_footnotes_draw_in_every_mode() {
    let text = "Text[^1] and more[^two] and none[^3].\n\n![[pic.png]]\nsee ![[pic.png|40]] inline\n![[B]]\n![[B#Part]]\n![[B#^blk]]\n\
                ![[A]]\n![[Missing]]\n![[scan.pdf]]\n\n[^1]: The first, with [[B]].\n[^two]: The second.\n[^unused]: Nobody refers to this.\n";
    let mut t = TempApp::new("embed-modes", &[("A", text), ("B", "# B\nb ^blk\n## Part\n- [ ] task\n![[A]]\n| a | b |\n|---|---|\n| 1 | 2 |\n")]);
    t.add_picture("pic.png", 64, 64);
    for mode in [Mode::Live, Mode::Source, Mode::Reading] {
        t.0.set_mode(mode);
        for _ in 0..4 {
            t.frame();
        }
    }
    assert_eq!(t.0.buffer, text, "notes that embed each other stop somewhere");
    assert!(!t.0.failed());
}

#[test]
fn a_footnote_mark_leads_to_the_footnote() {
    let text = format!("Said[^why].\n{}\n[^why]: Because.\n", "filler\n".repeat(80));
    let mut t = TempApp::new("footnote", &[("A", &text)]);
    t.frame();
    let ctx = egui::Context::default();
    let mut actions = Vec::new();
    t.0.on_hit(&ctx, Hit::Link(Link::Footnote("why".into())), &mut actions);
    t.frame();
    t.frame();
    let defined = text.find("Because").unwrap();
    assert_eq!(t.0.selection, Some(defined..defined));
    assert!(t.0.scroll_offset > 0.0);
    t.0.on_hit(&ctx, Hit::Link(Link::Footnote("nowhere".into())), &mut actions);
    assert!(t.0.message.as_ref().is_some_and(|message| message.text.contains("no footnote")));
}

#[test]
fn a_click_on_an_embed_link_opens_the_note_and_leaves_pictures_alone() {
    let mut t = TempApp::new("embed-hit", &[("A", "![[B#Part]]"), ("B", "# B\n## Part\nx")]);
    let ctx = egui::Context::default();
    let mut actions = Vec::new();
    let embed = |target: &str| Hit::Link(Link::Embed { target: target.into(), place: None, width: None });
    t.0.on_hit(&ctx, embed("pic.png"), &mut actions);
    assert!(actions.is_empty());
    t.0.on_hit(&ctx, embed("B"), &mut actions);
    assert!(matches!(&actions[..], [Action::Follow(title, None)] if title == "B"));
    // An embedded note counts as a link to it.
    t.frame();
    assert_eq!(t.0.index().links[0], ["B"]);
}

impl TempApp {
    /// An app that keeps its settings, in a folder inside its vault, and
    /// where the settings file of that folder is.
    fn with_settings(name: &str) -> (Self, PathBuf) {
        let dir = std::env::temp_dir().join(format!("malachite-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Vault::open(&dir).unwrap().create(Some("A"), &embedding("nothing here"), &[]).unwrap();
        let folder = dir.join(".settings");
        (Self::start(dir, Some(folder.join("settings.txt"))), folder.join(appearance::FILE))
    }
}

#[test]
fn the_settings_dialog_opens_on_its_hotkey_and_from_the_palette() {
    let mut t = TempApp::new("settings-dialog", &[("A", "text")]);
    t.focus_editor();
    t.keys(vec![ctrl(egui::Key::Comma)]);
    assert!(t.0.show_settings);
    // While it is up the keyboard is its own.
    t.keys(vec![ctrl(egui::Key::N), typed("x")]);
    assert_eq!((t.0.notes.len(), t.0.buffer.as_str()), (1, "text"));
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(!t.0.show_settings);
    // And the note has it back.
    t.keys(vec![typed("!")]);
    assert_eq!(t.0.buffer, "text!");

    t.keys(vec![ctrl(egui::Key::P), typed("open settings"), key(egui::Key::Enter)]);
    assert!(t.0.show_settings && t.0.picker.is_none());
    // It draws whatever the settings are, in both schemes of every theme.
    for theme in appearance::Palette::ALL {
        t.0.set_option(appearance::SECTION, "theme", theme.word(), "gem");
        for scheme in ["light", "dark"] {
            t.0.set_option(appearance::SECTION, "scheme", scheme, "system");
            t.0.set_option(appearance::colors(scheme == "dark"), "highlight", "#00ff0080", "#ffd00038");
            t.frame();
            t.frame();
            assert_eq!(t.0.look.palette, theme);
        }
    }
    assert!(!t.0.failed());
}

#[test]
fn a_theme_chosen_in_the_dialog_is_the_colour_of_everything() {
    use appearance::{Palette, ACCENT, SIDEBAR};
    let (mut t, conf) = TempApp::with_settings("themes");
    t.0.set_option(appearance::SECTION, "scheme", "dark", "system");
    t.0.show_settings = true;
    for _ in 0..25 {
        t.frame();
    }
    let color = |theme: Palette, index: usize| {
        let [r, g, b, _] = theme.colors(true)[index];
        egui::Color32::from_rgb(r, g, b)
    };
    assert_eq!(t.0.look.palette, Palette::Gem, "the app's own, until another is chosen");
    assert_eq!(t.1.style().visuals.panel_fill, color(Palette::Gem, SIDEBAR));
    for theme in [Palette::Aperture, Palette::Obsidian, Palette::Gem] {
        let card = t.spot(theme.name());
        t.click(card);
        assert_eq!(t.0.look.palette, theme);
        let visuals = t.1.style().visuals.clone();
        assert_eq!((visuals.panel_fill, visuals.hyperlink_color), (color(theme, SIDEBAR), color(theme, ACCENT)), "{}", theme.name());
        assert_eq!(t.0.theme(&t.1).accent, color(theme, ACCENT), "in the note as around it");
        // A button is the panel's colour, a little towards the text's: no grey of its own.
        let (panel, button) = (visuals.panel_fill, visuals.widgets.inactive.weak_bg_fill);
        assert!(button != panel && button.g().abs_diff(panel.g()) < 40, "{}: {button:?} on {panel:?}", theme.name());
        assert_eq!(button.g() > button.r(), panel.g() > panel.r(), "{}: of the hue of the panel", theme.name());
        let written = fs::read_to_string(&conf).unwrap();
        let line = if theme == Palette::Gem { "\n# theme = gem\n".to_owned() } else { format!("\ntheme = {}\n", theme.word()) };
        assert!(written.contains(&line), "{}: {written}", theme.name());
        assert_eq!(written.matches(&format!("show the {} theme's.", theme.name())).count(), 2);
        assert!(written.contains(&format!("# accent = {}", appearance::hex(theme.colors(true)[ACCENT]))), "the file shows the theme's colours");
    }
    assert_eq!(fs::read_to_string(&conf).unwrap(), appearance::template().replace("# scheme = system", "scheme = dark"), "and back at the first, the file is as it began");

    // A colour of one's own holds in any theme, and the dialog says so and offers the way back.
    assert!(t.spot("Gem").x > 0.0 && !t.0.spots.contains_key("Use the theme's"));
    t.0.set_option(appearance::colors(true), "accent", "#ff0000", "#3aa96f");
    let card = t.spot("Aperture");
    t.click(card);
    assert_eq!(t.0.theme(&t.1).accent, egui::Color32::RED);
    let theirs = t.spot("Use the theme's");
    t.click(theirs);
    assert_eq!(t.0.theme(&t.1).accent, color(Palette::Aperture, ACCENT));
    assert!(fs::read_to_string(&conf).unwrap().contains("\n# accent = #ff9a1f\n"));
    assert!(!t.0.failed());
}

#[test]
fn a_settings_file_from_before_there_were_themes_is_taken_over() {
    let (t, conf) = TempApp::with_settings("conf-before-themes");
    let dir = t.0.vault.root().to_path_buf();
    drop(t);
    Vault::open(&dir).unwrap().create(Some("A"), "text", &[]).unwrap();
    fs::create_dir_all(conf.parent().unwrap()).unwrap();
    let old = "[appearance]\n# Light or dark. `system` goes by what the system is set to.\ntheme = light\n\n# The colours of the light scheme, as #RRGGBB.\n[colors.light]\n# Behind the note\n# page = #ffffff\n# The text of a note\ntext = #101010\n";
    fs::write(&conf, old).unwrap();
    let mut t = TempApp::start(dir, Some(conf.with_file_name("settings.txt")));
    t.frame();
    assert!(!t.1.style().visuals.dark_mode, "light, as it said in the words it had for it");
    assert_eq!(t.0.look.palette, appearance::Palette::Gem);
    assert_eq!(t.0.theme(&t.1).text, egui::Color32::from_rgb(16, 16, 16), "with the colour it set");
    let new = fs::read_to_string(&conf).unwrap();
    assert!(new.contains("\n# theme = gem\n") && new.contains("\nscheme = light\n") && !new.contains("\ntheme = light"), "{new}");
    assert!(new.contains("\n# page = #fbfdfc\n") && new.contains("\ntext = #101010\n"), "{new}");
    assert!(!t.0.failed());
}

#[test]
fn a_setting_that_is_changed_is_a_line_of_the_settings_file() {
    let (mut t, conf) = TempApp::with_settings("conf-written");
    t.frame();
    assert!(!conf.exists(), "nothing is written until something is set");
    t.0.set_option(appearance::SECTION, "text_size", "20", "16");
    t.frame();
    let written = fs::read_to_string(&conf).unwrap();
    assert!(written.contains("\ntext_size = 20\n"), "{written}");
    assert!(written.contains("# theme = gem") && written.contains("# scheme = system") && written.contains("[colors.light]"), "among every setting there is");
    assert_eq!(t.0.theme(&t.1).size, 20.0);

    // What is added to the file by hand stays in it.
    fs::write(&conf, format!("# Mine.\n{written}")).unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    t.0.set_option(appearance::colors(true), "accent", "#ff0000", "#3aa96f");
    t.frame();
    let written = fs::read_to_string(&conf).unwrap();
    assert!(written.starts_with("# Mine.\n") && written.contains("\ntext_size = 20\n") && written.contains("\naccent = #ff0000\n"));
    assert_eq!(t.0.theme(&t.1).accent, egui::Color32::RED);

    // Back at what they are when left alone, they are comments again.
    t.0.set_option(appearance::SECTION, "text_size", "16", "16");
    t.0.set_option(appearance::colors(true), "accent", "#3aa96f", "#3aa96f");
    t.frame();
    assert_eq!(fs::read_to_string(&conf).unwrap(), format!("# Mine.\n{}", appearance::template()));
    assert!(!t.0.failed());
}

#[test]
fn the_settings_file_is_gone_by_from_the_start_and_when_the_window_is_come_back_to() {
    let (t, conf) = TempApp::with_settings("conf-read");
    let dir = t.0.vault.root().to_path_buf();
    drop(t);
    // Written by hand before the app is started.
    Vault::open(&dir).unwrap().create(Some("A"), "text", &[]).unwrap();
    fs::create_dir_all(conf.parent().unwrap()).unwrap();
    fs::write(&conf, "[appearance]\ntheme = obsidian\nscheme = light\ntext_size = 18\n\n[colors.light]\naccent = #ff0000\n").unwrap();
    let mut t = TempApp::start(dir, Some(conf.with_file_name("settings.txt")));
    t.frame();
    assert!(!t.1.style().visuals.dark_mode, "light, as the file says");
    let theme = t.0.theme(&t.1);
    assert_eq!((theme.size, theme.accent), (18.0, egui::Color32::RED));
    assert_eq!(t.1.style().visuals.hyperlink_color, egui::Color32::RED, "the interface around the note follows");
    assert_eq!(theme.page, egui::Color32::WHITE, "and what the file does not set is the theme's");

    // And edited while it runs. A mistake is said, and the rest still counts.
    let by_hand = "[appearance]\nscheme = dark\ntext_size = huge\n[colors.dark]\npage = #000000\n";
    fs::write(&conf, by_hand).unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    t.frame();
    assert!(t.0.message.as_ref().is_some_and(|message| message.error && message.text.contains("Line 3")), "the line that is wrong is named");
    assert!(t.1.style().visuals.dark_mode);
    let theme = t.0.theme(&t.1);
    assert_eq!((theme.size, theme.page), (16.0, egui::Color32::BLACK));
    assert_eq!(t.0.look.palette, appearance::Palette::Gem, "a theme no longer named is the first again");
    assert_eq!(fs::read_to_string(&conf).unwrap(), by_hand, "reading the file does not rewrite it");
}

#[test]
fn the_size_of_the_text_and_the_width_of_its_lines_are_what_the_settings_say() {
    let mut t = TempApp::new("look", &[("A", &embedding("nothing here"))]);
    (t.0.show_list, t.0.show_info) = (false, false);
    let before = t.line_top("# After");
    assert_eq!(t.0.column_width, appearance::LINE_WIDTH);
    // What is already laid out is laid out again for the new size.
    t.0.set_option(appearance::SECTION, "text_size", "32", "16");
    let after = t.line_top("# After");
    assert!(after > before + 20.0, "lines twice as tall put the heading lower: {before} to {after}");

    t.0.set_option(appearance::SECTION, "line_width", "400", "700");
    t.frame();
    assert_eq!(t.0.column_width, 400.0);
    t.0.set_option(appearance::SECTION, "line_width", "0", "700");
    t.frame();
    assert!(t.0.column_width > 1000.0, "no limit is the width of the window: {}", t.0.column_width);
    // Reading view goes by the same.
    t.0.set_mode(Mode::Reading);
    t.frame();
    assert!(t.0.column_width > 1000.0 && !t.0.failed());
}

impl TempApp {
    /// The middle of one of the buttons that are pictures, by what it is for.
    fn spot(&mut self, name: &str) -> egui::Pos2 {
        self.0.spots.clear();
        self.frame();
        self.0.spots.get(name).unwrap_or_else(|| panic!("no '{name}' button was drawn")).center()
    }

    /// What the app asks of its window as these things happen, a frame for
    /// each and one more, in a window that fills the screen or one that does
    /// not. Its title is left out: that is asked for whenever a note opens.
    fn asks(&mut self, events: Vec<egui::Event>, maximized: bool) -> Vec<egui::ViewportCommand> {
        let frames = events.into_iter().map(|event| vec![event]).chain([Vec::new()]);
        let asked = frames.flat_map(|events| self.frame_in(events, maximized).viewport_output.into_values().flat_map(|window| window.commands).collect::<Vec<_>>());
        asked.filter(|command| !matches!(command, egui::ViewportCommand::Title(_))).collect()
    }
}

/// The mouse going to `pos` and clicking there.
fn click_at(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![egui::Event::PointerMoved(pos), button(egui::PointerButton::Primary, pos, true), button(egui::PointerButton::Primary, pos, false)]
}

#[test]
fn the_tools_above_the_list_are_pictures_that_make_a_note_a_folder_and_today() {
    let mut t = TempApp::new("tools", &[("Projects/Plan", "")]);
    let note = t.spot("New note");
    t.click(note);
    assert_eq!(t.0.open.as_deref(), Some("Untitled"));
    assert_eq!(t.folder_of("Untitled"), "Projects", "beside the note that was open");
    let folder = t.spot("New folder");
    t.click(folder);
    t.keys(vec![typed("Research"), key(egui::Key::Enter)]);
    assert!(t.0.vault.root().join("Research").is_dir(), "the tool makes a folder at the top, whatever is open");
    let today = t.spot("Today");
    t.click(today);
    let open = t.0.open.clone().expect("today's note is open");
    assert!(malachite::daily_date(&open, malachite::DAILY_FORMAT).is_some(), "{open} is a day");
    assert!(!t.0.failed());
}

#[test]
fn the_last_tool_above_the_list_folds_every_folder_and_then_unfolds_them_all() {
    let mut t = TempApp::new("fold-all", &[("Top", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", ""), ("Archive/Old", "")]);
    t.0.apply(Action::Open("Top".into()));
    let everything = ["Archive", "Old", "Projects", "2026", "Goals", "Plan", "Top"];
    assert_eq!(t.listed(), everything);
    let collapse = t.spot("Collapse all");
    t.click(collapse);
    assert_eq!(t.listed(), ["Archive", "Projects", "Top"]);

    // The folders inside were folded as well.
    let projects = t.row("Projects");
    t.click(projects);
    assert_eq!(t.listed(), ["Archive", "Projects", "2026", "Plan", "Top"]);
    // A folder showing is open, so the tool folds again rather than unfolding.
    let collapse = t.spot("Collapse all");
    t.click(collapse);
    assert_eq!(t.listed(), ["Archive", "Projects", "Top"]);
    let expand = t.spot("Expand all");
    assert_eq!(expand, collapse, "the same button, now with its other picture");
    t.click(expand);
    assert_eq!(t.listed(), everything);
    assert!(!t.0.failed());
}

#[test]
fn the_fold_tool_goes_by_what_the_list_shows() {
    let mut t = TempApp::new("fold-all-shown", &[("Top", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", "")]);
    t.0.apply(Action::Open("Top".into()));
    let projects = t.row("Projects");
    t.click(projects);
    assert_eq!(t.listed(), ["Projects", "Top"]);
    // 2026 is still open, but out of sight: there is nothing left showing to fold.
    let expand = t.spot("Expand all");
    t.click(expand);
    assert_eq!(t.listed(), ["Projects", "2026", "Goals", "Plan", "Top"]);

    // Without folders there is nothing for it to do.
    let mut flat = TempApp::new("fold-all-flat", &[("A", ""), ("B", "")]);
    let collapse = flat.spot("Collapse all");
    flat.click(collapse);
    assert!(flat.0.collapsed.is_empty() && !flat.0.failed());
}

#[test]
fn folders_collapse_and_expand_from_the_command_palette() {
    let mut t = TempApp::new("fold-all-palette", &[("Top", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", "")]);
    t.0.apply(Action::Open("Top".into()));
    t.0.show_list = false;
    t.frame();
    t.keys(vec![ctrl(egui::Key::P), typed("collapse all")]);
    assert_eq!(t.0.picked_from, ["Collapse all folders"]);
    t.keys(vec![key(egui::Key::Enter)]);
    assert!(t.0.show_list, "the list comes out to show what was done to it");
    assert_eq!(t.listed(), ["Projects", "Top"]);
    t.keys(vec![ctrl(egui::Key::P), typed("expand all"), key(egui::Key::Enter)]);
    assert_eq!(t.listed(), ["Projects", "2026", "Goals", "Plan", "Top"]);
}

impl TempApp {
    /// Every piece of text a frame draws, and where.
    fn texts(&mut self) -> Vec<(String, egui::Rect)> {
        let shapes = self.frame_full(Vec::new()).shapes;
        let text = |clipped: &egui::epaint::ClippedShape| match &clipped.shape {
            egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.galley.rect.translate(text.pos.to_vec2()))),
            _ => None,
        };
        shapes.iter().filter_map(text).collect()
    }

    /// What else is written on the row of the list that says `label`.
    fn beside(&mut self, label: &str) -> Vec<String> {
        self.frame();
        let texts = self.texts();
        let found = self.0.list_rows.iter().find(|(text, _)| text == label);
        let row = found.unwrap_or_else(|| panic!("no row '{label}' in the list")).1;
        texts.into_iter().filter(|(text, at)| row.contains(at.center()) && text != label).map(|(text, _)| text).collect()
    }

    /// Right-click the row of the list that says `label` and pick `choice`
    /// from the menu that brings up.
    fn pick_from_row_menu(&mut self, label: &str, choice: &str) {
        let pos = self.row(label);
        self.frame_with(vec![egui::Event::PointerMoved(pos)]);
        self.frame_with(vec![button(egui::PointerButton::Secondary, pos, true)]);
        self.frame_with(vec![button(egui::PointerButton::Secondary, pos, false)]);
        self.frame();
        let found = self.texts().into_iter().find(|(text, _)| text == choice);
        let spot = found.unwrap_or_else(|| panic!("no '{choice}' in the menu of '{label}'")).1.center();
        self.click(spot);
    }
}

#[test]
fn collapse_others_folds_every_folder_but_the_one_picked_and_those_it_is_in() {
    let notes = [("Top", ""), ("Archive/Old", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", ""), ("Projects/2026/Deep/Down", "")];
    let mut t = TempApp::new("fold-others", &notes);
    t.0.apply(Action::Open("Top".into()));
    assert_eq!(t.listed(), ["Archive", "Old", "Projects", "2026", "Deep", "Down", "Goals", "Plan", "Top"]);
    t.pick_from_row_menu("2026", "Collapse others");
    assert_eq!(t.listed(), ["Archive", "Projects", "2026", "Deep", "Goals", "Plan", "Top"], "the folders inside it are folded too");
    // A folded folder opens for it.
    t.pick_from_row_menu("Archive", "Collapse others");
    assert_eq!(t.listed(), ["Archive", "Old", "Projects", "Top"]);
    assert!(!t.0.failed());
}

#[test]
fn a_folder_shows_how_many_notes_are_in_it() {
    let notes = [("Top", ""), ("Projects/Plan", ""), ("Projects/2026/Goals", ""), ("Projects/2026/More", "")];
    let mut t = TempApp::new("folder-counts", &notes);
    t.0.vault.create_folder(Path::new(""), "Empty").unwrap();
    t.0.reload().unwrap();
    assert_eq!(t.beside("Projects"), ["3"], "counting those in the folders inside it");
    assert_eq!(t.beside("2026"), ["2"]);
    assert!(t.beside("Empty").is_empty(), "nothing for none");
    assert!(t.beside("Plan").is_empty(), "and notes have no count");

    // Folded or not, and as notes come and go.
    let projects = t.row("Projects");
    t.click(projects);
    assert_eq!(t.beside("Projects"), ["3"]);
    t.0.apply(Action::NewIn(PathBuf::from("Projects")));
    assert_eq!(t.beside("Projects"), ["4"]);
}

#[test]
fn dragging_a_folder_moves_it_with_everything_in_it() {
    let notes = [("Inbox", "i"), ("Archive/Old", "o"), ("Projects/Plan", "p"), ("Projects/2026/Goals", "g")];
    let mut t = TempApp::new("drag-folder", &notes);
    t.0.apply(Action::Open("Plan".into()));
    t.type_text("!");
    t.0.collapsed.insert(PathBuf::from("Projects/2026"));
    let (from, to) = (t.row("Projects"), t.row("Archive"));
    t.drag(from, to);
    assert!(!t.0.failed());
    assert_eq!((t.folder_of("Plan"), t.folder_of("Goals")), ("Archive/Projects".to_owned(), "Archive/Projects/2026".to_owned()));
    assert_eq!((t.0.open.as_deref(), t.0.buffer.as_str()), (Some("Plan"), "p!"), "the open note goes with it");
    assert_eq!(t.on_disk("Plan"), "p!", "saved on the way");
    assert_eq!(t.listed(), ["Archive", "Projects", "2026", "Plan", "Old", "Inbox"], "the folder inside stays folded");

    // Onto a note, into that note's folder; below the list, to the top level.
    let (from, to) = (t.row("2026"), t.row("Inbox"));
    t.drag(from, to);
    assert_eq!(t.folder_of("Goals"), "2026");
    let from = t.row("Projects");
    t.drag(from, egui::pos2(from.x, 600.0));
    assert_eq!(t.folder_of("Plan"), "Projects");
    assert!(!t.0.failed());

    // Let go of on itself, it stays; into a folder inside it, it cannot go.
    let projects = t.row("Projects");
    t.drag(projects, projects);
    assert!(!t.0.failed());
    t.0.vault.create_folder(Path::new("Projects"), "Inner").unwrap();
    t.0.reload().unwrap();
    let (from, to) = (t.row("Projects"), t.row("Inner"));
    t.drag(from, to);
    assert!(t.0.failed());
    assert_eq!(t.folder_of("Plan"), "Projects");
}

#[test]
fn folded_folders_are_remembered_for_each_vault_between_runs() {
    let make = |name: &str, notes: &[&str]| {
        let dir = std::env::temp_dir().join(format!("malachite-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let vault = Vault::open(&dir).unwrap();
        for note in notes {
            let (folder, title) = note.rsplit_once('/').unwrap_or(("", note));
            vault.create_in(Path::new(folder), Some(title), "", &[]).unwrap();
        }
        dir
    };
    let here = make("fold-remembered", &["Archive/Old", "Projects/Plan", "Top"]);
    let other = make("fold-remembered-other", &["Work/Task", "Loose"]);
    let settings = here.join(".settings").join("settings.txt");

    let mut t = TempApp::start(here.clone(), Some(settings.clone()));
    assert_eq!(t.0.open.as_deref(), Some("Old"));
    let archive = t.row("Archive");
    t.click(archive);
    let folded = ["Archive", "Projects", "Plan", "Top"];
    assert_eq!(t.listed(), folded);
    t.0.apply(Action::OpenVault(other.clone()));
    let work = t.row("Work");
    t.click(work);
    assert_eq!(t.listed(), ["Work", "Loose"]);
    t.0.apply(Action::OpenVault(here.clone()));
    assert_eq!(t.listed(), folded, "each vault has its own");
    assert!(fs::read_to_string(&settings).unwrap().contains("\nfolded = Archive\n"));

    // And so they are the next time the app starts, the open note's folder too.
    let mut again = TempApp::start(here.clone(), Some(settings.clone()));
    assert_eq!(again.0.open.as_deref(), Some("Old"));
    assert_eq!(again.listed(), folded);
    again.0.apply(Action::OpenVault(other.clone()));
    assert_eq!(again.listed(), ["Work", "Loose"]);

    // A folder that has gone since is let go of.
    fs::write(&settings, format!("recent = {}\nfolded = Gone\nfolded = Projects\n", here.display())).unwrap();
    let last = TempApp::start(here.clone(), Some(settings));
    assert_eq!(last.0.folded_here(), ["Projects"]);
}

#[test]
fn the_window_has_its_three_buttons_at_its_top_right_corner() {
    use egui::ViewportCommand::{Close, Maximized, Minimized};
    let mut t = TempApp::new("window-buttons", &[("A", "# Of A")]);
    let close = t.spot("Close");
    let corner = t.0.spots["Close"].right_top();
    assert_eq!(corner, egui::pos2(1100.0, 0.0), "flush against the corner");
    assert_eq!(t.asks(click_at(close), false), [Close]);
    let minimise = t.spot("Minimise");
    assert_eq!(t.asks(click_at(minimise), false), [Minimized(true)]);
    let maximise = t.spot("Maximise");
    assert_eq!(t.asks(click_at(maximise), false), [Maximized(true)]);
    // In a window that fills the screen the same button puts it back.
    t.frame_in(Vec::new(), true);
    assert!(t.0.spots.contains_key("Restore"));
    assert_eq!(t.asks(click_at(maximise), true), [Maximized(false)]);
}

#[test]
fn the_corner_of_the_window_is_the_same_with_the_right_panel_and_without() {
    let mut t = TempApp::new("window-corner", &[("A", "# Of A")]);
    let places = |t: &mut TempApp| {
        t.spot("Close");
        ["Close", "Maximise", "Minimise", "Right panel"].map(|name| t.0.spots[name])
    };
    let with_panel = places(&mut t);
    // The button beside the window's own folds the panel away, and is still
    // there afterwards to bring it back.
    let toggle = with_panel[3].center();
    t.click(toggle);
    assert!(!t.0.show_info);
    assert_eq!(places(&mut t), with_panel, "the bar above the note has the corner now");
    t.click(toggle);
    assert!(t.0.show_info);
    // Without a note there is no panel, and still a bar to hold them.
    t.trash_open();
    assert_eq!(t.0.open, None);
    assert_eq!(places(&mut t), with_panel);
    let close = t.spot("Close");
    assert_eq!(t.asks(click_at(close), false), [egui::ViewportCommand::Close]);
}

#[test]
fn the_bars_along_the_top_move_the_window_and_a_double_click_fills_the_screen() {
    use egui::ViewportCommand::{Maximized, StartDrag};
    let mut t = TempApp::new("window-bars", &[("A", "# Of A")]);
    t.frame();
    // Beside the tools of the list, on the name in the bar above the note,
    // and beside the buttons at the top of the right-hand panel.
    for bar in [egui::pos2(200.0, 19.0), egui::pos2(520.0, 19.0), egui::pos2(915.0, 19.0)] {
        let dragged = vec![egui::Event::PointerMoved(bar), button(egui::PointerButton::Primary, bar, true), egui::Event::PointerMoved(bar + egui::vec2(30.0, 12.0))];
        assert_eq!(t.asks(dragged, false), [StartDrag], "at {bar:?}");
        t.frame_with(vec![button(egui::PointerButton::Primary, bar + egui::vec2(30.0, 12.0), false)]);
        let twice = [click_at(bar), click_at(bar)].concat();
        assert_eq!(t.asks(twice.clone(), false), [Maximized(true)], "at {bar:?}");
        // Time for the next click to be a first one again.
        for _ in 0..40 {
            t.frame();
        }
        assert_eq!(t.asks(twice, true), [Maximized(false)], "at {bar:?}");
        for _ in 0..40 {
            t.frame();
        }
    }
    // A button in a bar is still a button.
    let toggle = t.spot("Note list");
    assert_eq!(t.asks(click_at(toggle), false), []);
    assert!(!t.0.show_list);
    // And the note is not a bar.
    let text = egui::pos2(520.0, 300.0);
    let dragged = vec![egui::Event::PointerMoved(text), button(egui::PointerButton::Primary, text, true), egui::Event::PointerMoved(text + egui::vec2(30.0, 12.0))];
    assert_eq!(t.asks(dragged, false), []);
}

#[test]
fn an_edge_of_the_window_resizes_it_and_presses_nothing_else() {
    use egui::viewport::ResizeDirection::{East, NorthEast, South};
    use egui::ViewportCommand::{BeginResize, Close};
    let mut t = TempApp::new("window-edges", &[("A", "# Of A")]);
    t.frame();
    // The right edge runs down the close button: it is the edge that is taken hold of.
    let on_close = egui::pos2(1098.0, 20.0);
    assert_eq!(t.asks(click_at(on_close), false), [BeginResize(East)]);
    assert_eq!(t.asks(click_at(egui::pos2(1098.0, 2.0)), false), [BeginResize(NorthEast)]);
    assert_eq!(t.asks(click_at(egui::pos2(500.0, 698.0)), false), [BeginResize(South)]);
    assert_eq!(t.cursor_at(egui::pos2(1098.0, 300.0)), egui::CursorIcon::ResizeHorizontal);
    assert_eq!(t.cursor_at(egui::pos2(1098.0, 698.0)), egui::CursorIcon::ResizeNwSe);
    // A window that fills the screen has no edges to hold, and the button reaches the corner.
    assert_eq!(t.asks(click_at(on_close), true), [Close]);
    assert_eq!(t.asks(click_at(egui::pos2(1099.0, 0.5)), true), [Close]);
}

#[test]
fn the_files_page_is_a_page_of_the_dialog_and_a_section_of_the_file() {
    let (mut t, conf) = TempApp::with_settings("files-page");
    t.0.show_settings = true;
    // Until the dialog has come to rest in the middle of the window.
    for _ in 0..25 {
        t.frame();
    }
    assert!(!t.0.spots.contains_key("Top level"), "it opens on Appearance");
    let page = t.spot("Files and links");
    t.click(page);
    let top = t.spot("Top level");
    t.click(top);
    assert_eq!(t.0.files.new_notes, files::NewNotes::Top);
    let ask = t.spot("Ask");
    t.click(ask);
    assert_eq!(t.0.files.relink, files::Relink::Ask);
    t.frame();
    // Into the vault's own file, not the app's.
    let in_vault = files::vault_file(t.0.vault.root());
    let written = fs::read_to_string(&in_vault).unwrap();
    assert!(written.starts_with("# This vault's settings") && written.contains("\nnew_notes = top\n") && written.contains("\nupdate_links = ask\n"), "{written}");
    assert!(!conf.exists(), "the app's file has nothing to say yet");
    assert_eq!(t.titles(), ["A"], "and the vault's file is not a note");
    // It draws whatever the settings are.
    for (key, value) in [("new_notes", "folder"), ("new_notes_folder", "Not/Here"), ("deleted", "delete"), ("templates", ""), ("daily_format", "dddd"), ("daily_template", "Gone")] {
        t.0.set_option(files::SECTION, key, value, "");
        t.frame();
    }
    // The page is kept while the dialog is put away and brought back.
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(!t.0.show_settings);
    t.0.show_settings = true;
    t.0.spots.clear();
    t.frame();
    t.frame();
    assert!(t.0.spots.contains_key("Top level"));
    assert!(!t.0.failed());
    // A mistake in either file is told by its line, each in its own.
    fs::create_dir_all(conf.parent().unwrap()).unwrap();
    fs::write(&conf, "[files]\ndeleted = vault\n[appearance]\ntext_size = big\n").unwrap();
    fs::write(&in_vault, "[files]\ndeleted = shredder\n").unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    assert_eq!(t.0.conf.problems, ["Line 2: `deleted` is a vault's setting now: it is kept in each vault, in .malachite/malachite.conf", "Line 4: `big` is not a number"]);
    assert_eq!(t.0.vault_conf.problems, ["Line 2: `shredder` is not where deleted notes go: it is vault, system or delete"]);
    assert!(t.0.message.as_ref().is_some_and(|message| message.text.starts_with("malachite.conf: Line 2") && message.text.ends_with("(and 2 more)")));
}

#[test]
fn new_notes_start_where_the_settings_say() {
    let mut t = TempApp::new("new-notes", &[("Projects/Plan", "[[Later]] [[Sooner]]")]);
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::New);
    assert_eq!(t.folder_of("Untitled"), "Projects", "beside the open note, as before");
    t.0.set_option(files::SECTION, "new_notes", "top", "beside");
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::New);
    assert_eq!(t.folder_of("Untitled 1"), "");
    t.0.set_option(files::SECTION, "new_notes", "folder", "beside");
    t.0.set_option(files::SECTION, "new_notes_folder", "Inbox/New", "");
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::New);
    assert_eq!(t.folder_of("Untitled 2"), "Inbox/New", "made for it");
    // A link to a note that is not there yet makes it there too.
    t.0.apply(Action::Open("Plan".into()));
    t.0.apply(Action::Follow("Later".into(), None));
    assert_eq!(t.folder_of("Later"), "Inbox/New");
    // So does one in the panel beside the graph.
    t.0.apply(Action::Open("Plan".into()));
    t.frame();
    t.open_graph();
    let (_, at) = t.node("Plan");
    t.click(at);
    let row = t.0.link_rows.iter().find(|(text, _)| text == "Sooner (new)").expect("the link to a note that is not there").1.center();
    t.click(row);
    assert_eq!(t.folder_of("Sooner"), "Inbox/New");
    assert!(!t.0.failed());
}

#[test]
fn renaming_asks_before_updating_links_when_told_to() {
    let mut t = TempApp::new("rename-ask", &[("A", "see [[B]]"), ("B", "b"), ("C", "c")]);
    t.0.set_option(files::SECTION, "update_links", "ask", "always");
    let rename = |t: &mut TempApp, from: &str, to: &str| {
        t.0.apply(Action::Open(from.into()));
        t.0.title_buf = to.into();
        t.0.apply(Action::Rename);
        t.frame();
        t.frame();
    };
    // Called off: nothing changes, and the name is the note's again.
    rename(&mut t, "B", "Bee");
    assert!(matches!(t.0.confirm, Some(Confirm::Rename { linking: 1, .. })));
    assert_eq!(t.0.open.as_deref(), Some("B"), "not renamed while it asks");
    t.keys(vec![key(egui::Key::Escape)]);
    assert!(t.0.confirm.is_none());
    assert_eq!((t.0.open.as_deref(), t.0.title_buf.as_str()), (Some("B"), "B"));
    assert_eq!(t.titles(), ["A", "B", "C"]);
    // Enter updates them.
    rename(&mut t, "B", "Bee");
    t.keys(vec![key(egui::Key::Enter)]);
    assert_eq!(t.0.open.as_deref(), Some("Bee"));
    assert_eq!(t.on_disk("A"), "see [[Bee]]");
    // The other answer leaves them.
    rename(&mut t, "Bee", "Bea");
    let leave = t.spot("Don't update");
    t.click(leave);
    assert_eq!(t.0.open.as_deref(), Some("Bea"));
    assert_eq!(t.on_disk("A"), "see [[Bee]]");
    // With nothing linking to it, there is nothing to ask.
    rename(&mut t, "C", "See");
    assert!(t.0.confirm.is_none());
    assert_eq!(t.0.open.as_deref(), Some("See"));
    // And never is never.
    t.0.set_option(files::SECTION, "update_links", "never", "always");
    fs::write(t.0.vault.root().join("A.md"), "see [[Bea]]").unwrap();
    t.0.reload().unwrap();
    rename(&mut t, "Bea", "Bo");
    assert!(t.0.confirm.is_none());
    assert_eq!((t.0.open.as_deref(), t.on_disk("A").as_str()), (Some("Bo"), "see [[Bea]]"));
    assert!(!t.0.failed());
}

#[test]
fn deleting_can_be_for_good() {
    let mut t = TempApp::new("delete-for-good", &[("A", "a"), ("Folder/B", "b"), ("C", "c")]);
    t.0.set_option(files::SECTION, "deleted", "delete", "vault");
    let a = t.0.notes.iter().find(|note| note.title == "A").unwrap().path.clone();
    t.0.apply(Action::Trash(a.clone()));
    assert!(!a.exists() && !t.0.vault.root().join(".trash").exists());
    assert!(t.0.message.as_ref().is_some_and(|message| message.text == "Deleted A for good"));
    t.0.apply(Action::TrashFolder(PathBuf::from("Folder")));
    assert!(!t.0.vault.root().join("Folder").exists());
    assert_eq!(t.titles(), ["C"]);
    t.0.set_option(files::SECTION, "deleted", "vault", "vault");
    let c = t.0.notes[0].path.clone();
    t.0.apply(Action::Trash(c));
    assert!(t.0.vault.root().join(".trash").join("C.md").exists(), "and back to the vault's .trash");
    assert!(!t.0.failed());
}

#[test]
fn templates_and_daily_notes_follow_the_settings() {
    let mut t = TempApp::new("daily-settings", &[("Meta/Bits/Day", "# {{title}}\nA {{date:dddd}}."), ("Templates/Old", "x"), ("A", "a")]);
    let names = |t: &TempApp| t.0.templates().map(|note| note.title.clone()).collect::<Vec<_>>();
    assert_eq!(names(&t), ["Old"]);
    t.0.set_option(files::SECTION, "templates", "Meta/Bits", "Templates");
    assert_eq!(names(&t), ["Day"]);
    t.0.set_option(files::SECTION, "daily_folder", "Journal", "");
    t.0.set_option(files::SECTION, "daily_format", "D MMMM YYYY", "YYYY-MM-DD");
    t.0.set_option(files::SECTION, "daily_template", "Meta/Bits/Day", "");
    let now = chrono::Local::now().naive_local();
    let today = malachite::template::formatted(now, "D MMMM YYYY");
    t.0.apply(Action::Daily);
    assert_eq!(t.0.open.as_deref(), Some(today.as_str()));
    assert_eq!(t.folder_of(&today), "Journal");
    assert_eq!(t.0.buffer, format!("# {today}\nA {}.", malachite::template::formatted(now, "dddd")));
    // The days before and after are found by the same format.
    let yesterday = malachite::template::formatted(now - chrono::Duration::days(1), "D MMMM YYYY");
    t.0.vault.create_in(Path::new("Journal"), Some(&yesterday), "", &[]).unwrap();
    t.0.reload().unwrap();
    t.0.daily_step(false).unwrap();
    assert_eq!(t.0.open.as_deref(), Some(yesterday.as_str()));
    // A template that is not there is said, and nothing is made.
    t.0.set_option(files::SECTION, "daily_template", "Meta/Gone", "");
    fs::remove_file(t.0.vault.root().join("Journal").join(format!("{today}.md"))).unwrap();
    t.0.reload().unwrap();
    t.0.apply(Action::Daily);
    assert!(t.0.failed());
    assert!(!t.0.vault.root().join("Journal").join(format!("{today}.md")).exists());
}

#[test]
fn renaming_a_folder_the_settings_name_changes_them_to_its_new_name() {
    let notes = [("Meta/Bits/Day", "# {{title}}"), ("Life/Journal/Old", "o"), ("Lifelong/Plan", "p")];
    let mut t = TempApp::new("rename-folder-settings", &notes);
    let file = files::vault_file(t.0.vault.root());
    let before = "[files]\n# Mine, by hand.\ntemplates = Meta/Bits\ndaily_folder = Life/Journal\ndaily_template = Meta/Bits/Day\nnew_notes = folder\nnew_notes_folder = Lifelong\n";
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, before).unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    assert_eq!(t.0.files.templates, Path::new("Meta/Bits"));

    // The templates folder itself, and a folder the daily notes' folder is in.
    t.0.apply(Action::RenameFolder(PathBuf::from("Meta/Bits"), "Pieces".into()));
    t.0.apply(Action::RenameFolder(PathBuf::from("Life"), "Days".into()));
    t.frame();
    assert!(!t.0.failed());
    let named = &t.0.files;
    assert_eq!((named.templates.as_path(), named.daily_template.as_str()), (Path::new("Meta/Pieces"), "Meta/Pieces/Day"));
    assert_eq!((named.daily_folder.as_path(), named.new_notes_folder.as_path()), (Path::new("Days/Journal"), Path::new("Lifelong")), "a folder whose name only starts the same stays");
    let after = before.replace("Meta/Bits", "Meta/Pieces").replace("Life/Journal", "Days/Journal");
    assert_eq!(fs::read_to_string(&file).unwrap(), after, "those lines change, and nothing else");

    // And they are where the app goes on looking.
    assert_eq!(t.0.templates().map(|note| note.title.as_str()).collect::<Vec<_>>(), ["Day"]);
    let today = malachite::template::formatted(chrono::Local::now().naive_local(), "YYYY-MM-DD");
    t.0.apply(Action::Daily);
    assert_eq!((t.folder_of(&today), t.0.buffer.clone()), ("Days/Journal".to_owned(), format!("# {today}")));
    assert!(!t.0.failed());
}

#[test]
fn moving_a_folder_the_settings_name_changes_them_to_where_it_is_now() {
    let notes = [("Templates/Day", "# {{title}}"), ("Life/Journal/Old", "o"), ("Archive/Done", "d")];
    let mut t = TempApp::new("move-folder-settings", &notes);
    let file = files::vault_file(t.0.vault.root());
    // Left alone, the templates are in Templates: moved, the file is started to say where.
    t.0.apply(Action::MoveFolder { folder: PathBuf::from("Templates"), into: PathBuf::from("Archive") });
    t.frame();
    assert!(!t.0.failed());
    assert_eq!(t.0.files.templates, Path::new("Archive/Templates"));
    assert_eq!(fs::read_to_string(&file).unwrap(), files::vault_template().replace("# templates = Templates", "templates = Archive/Templates"));

    // A folder the daily notes' folder is in.
    t.0.set_option(files::SECTION, "daily_folder", "Life/Journal", "");
    t.0.set_option(files::SECTION, "daily_template", "Archive/Templates/Day", "");
    t.frame();
    let before = fs::read_to_string(&file).unwrap();
    t.0.apply(Action::MoveFolder { folder: PathBuf::from("Life"), into: PathBuf::from("Archive") });
    t.frame();
    assert_eq!(t.0.files.daily_folder, Path::new("Archive/Life/Journal"));
    assert_eq!(fs::read_to_string(&file).unwrap(), before.replace("daily_folder = Life/Journal", "daily_folder = Archive/Life/Journal"));

    // Back at the top, the templates folder is where it is when left alone,
    // and its line is a comment again.
    let before = fs::read_to_string(&file).unwrap();
    t.0.apply(Action::MoveFolder { folder: PathBuf::from("Archive/Templates"), into: PathBuf::new() });
    t.frame();
    assert_eq!((t.0.files.templates.as_path(), t.0.files.daily_template.as_str()), (Path::new("Templates"), "Templates/Day"));
    let after = before.replace("\ntemplates = Archive/Templates\n", "\n# templates = Templates\n").replace("Archive/Templates/Day", "Templates/Day");
    assert_eq!(fs::read_to_string(&file).unwrap(), after);

    let today = malachite::template::formatted(chrono::Local::now().naive_local(), "YYYY-MM-DD");
    t.0.apply(Action::Daily);
    assert_eq!((t.folder_of(&today), t.0.buffer.clone()), ("Archive/Life/Journal".to_owned(), format!("# {today}")));
    assert!(!t.0.failed());
}

#[test]
fn each_vault_has_its_own_files_settings() {
    let (mut t, conf) = TempApp::with_settings("vault-settings");
    t.0.set_option(appearance::SECTION, "text_size", "20", "16");
    t.0.set_option(files::SECTION, "templates", "Meta", "Templates");
    t.0.set_option(files::SECTION, "update_links", "never", "always");
    t.frame();
    let first = t.0.vault.root().to_path_buf();
    assert!(fs::read_to_string(&conf).unwrap().contains("\ntext_size = 20\n"));
    assert!(fs::read_to_string(files::vault_file(&first)).unwrap().contains("\ntemplates = Meta\n"));

    // Another vault starts at the defaults, with the same look.
    let other = std::env::temp_dir().join(format!("malachite-test-{}-vault-settings-other", std::process::id()));
    let _ = fs::remove_dir_all(&other);
    t.0.apply(Action::OpenVault(other.clone()));
    assert_eq!(t.0.files, files::Files::default());
    assert_eq!(t.0.look.text_size, 20.0);
    t.0.set_option(files::SECTION, "new_notes", "top", "beside");
    t.frame();
    assert!(fs::read_to_string(files::vault_file(&other)).unwrap().contains("\nnew_notes = top\n"));

    // And going back, the first vault's are as they were left.
    t.0.apply(Action::OpenVault(first.clone()));
    assert_eq!((t.0.files.templates.as_path(), t.0.files.relink, t.0.files.new_notes), (Path::new("Meta"), files::Relink::Never, files::NewNotes::Beside));
    // Edited by hand while the app is away, they are read again on coming back.
    fs::write(files::vault_file(&first), "[files]\ndeleted = delete\n").unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    assert_eq!((t.0.files.deleted, t.0.files.relink), (malachite::Discard::Delete, files::Relink::Always));
    assert!(!t.0.failed());
    let _ = fs::remove_dir_all(&other);
}

#[test]
fn files_settings_kept_in_the_app_file_by_an_earlier_build_move_into_the_vault() {
    let (t, conf) = TempApp::with_settings("take-over-files");
    let dir = t.0.vault.root().to_path_buf();
    drop(t);
    Vault::open(&dir).unwrap().create(Some("A"), "a", &[]).unwrap();
    fs::create_dir_all(conf.parent().unwrap()).unwrap();
    let mut old = appearance::template().replace("# scheme = system", "scheme = dark");
    old.push_str("\n[files]\n# Where a new note starts.\nnew_notes = top\n# The folder.\ndaily_folder = 00-Daily\ntemplates = Mine\n");
    fs::write(&conf, &old).unwrap();
    fs::create_dir_all(files::vault_file(&dir).parent().unwrap()).unwrap();
    fs::write(files::vault_file(&dir), "[files]\ntemplates = Ours\n").unwrap();

    let mut t = TempApp::start(dir.clone(), Some(conf.with_file_name("settings.txt")));
    t.frame();
    assert_eq!((t.0.files.new_notes, t.0.files.daily_folder.as_path()), (files::NewNotes::Top, Path::new("00-Daily")));
    assert_eq!(t.0.files.templates, Path::new("Ours"), "what the vault already said stays");
    assert!(t.0.conf.problems.is_empty() && !t.0.failed());
    assert!(t.0.message.as_ref().is_some_and(|message| message.text.starts_with("Moved 2 settings of Files and links")));
    assert_eq!(fs::read_to_string(&conf).unwrap(), appearance::template().replace("# scheme = system", "scheme = dark"), "the app's file is as the app writes it now");
    let in_vault = fs::read_to_string(files::vault_file(&dir)).unwrap();
    assert!(in_vault.contains("\nnew_notes = top") && in_vault.contains("\ndaily_folder = 00-Daily") && in_vault.contains("templates = Ours"), "{in_vault}");
}

#[test]
fn a_problem_with_the_settings_is_told_once_not_at_every_change() {
    let (mut t, conf) = TempApp::with_settings("problems-once");
    fs::create_dir_all(conf.parent().unwrap()).unwrap();
    fs::write(&conf, "[appearance]\ntext_size = big\n").unwrap();
    t.frame_with(vec![egui::Event::WindowFocused(true)]);
    assert!(t.0.failed(), "told when it is found");
    t.0.message = None;
    t.0.set_option(appearance::SECTION, "scheme", "dark", "system");
    t.0.set_option(files::SECTION, "new_notes", "top", "beside");
    assert!(t.0.message.is_none(), "not again for every other change");
    t.0.set_option(appearance::SECTION, "text_size", "18", "16");
    assert!(t.0.conf.problems.is_empty());
}

#[test]
fn the_list_of_the_palette_grows_back_after_a_search_has_narrowed_it() {
    let notes: Vec<String> = (0..30).map(|index| format!("Note {index:02}")).collect();
    let notes: Vec<(&str, &str)> = notes.iter().map(|title| (title.as_str(), "")).collect();
    let mut t = TempApp::new("picker-height", &notes);
    t.focus_editor();
    let height = |t: &mut TempApp| {
        t.frame();
        t.frame();
        t.1.memory(|memory| memory.area_rect(egui::Id::new("picker"))).expect("the list is up").height()
    };
    t.keys(vec![ctrl(egui::Key::O)]);
    let full = height(&mut t);
    assert!(full > 300.0, "a full list scrolls at its full height: {full}");
    t.keys(vec![typed("Note 03")]);
    let narrowed = height(&mut t);
    assert!(narrowed < 100.0, "one row is a short list: {narrowed}");
    t.keys(vec![ctrl(egui::Key::A), key(egui::Key::Backspace)]);
    assert_eq!(height(&mut t), full, "and every row a full one again");
    // Nor does the next list start where the last was left.
    t.keys(vec![typed("Note 03"), key(egui::Key::Escape)]);
    t.keys(vec![ctrl(egui::Key::P)]);
    assert!(height(&mut t) > 300.0);
}

#[test]
fn ctrl_backspace_in_the_note_takes_one_word_in_any_script() {
    let text = "# Нотатки\n\nПерший рядок.\nДругий рядок тут";
    for mode in [Mode::Live, Mode::Source] {
        let mut t = TempApp::new("ctrl-backspace", &[("A", text)]);
        t.0.set_mode(mode);
        t.focus_editor();
        let end = text.chars().count();
        t.select(end, end);
        t.keys(vec![ctrl(egui::Key::Backspace)]);
        assert_eq!(t.0.buffer, "# Нотатки\n\nПерший рядок.\nДругий рядок ", "in {}", if mode == Mode::Live { "Live Preview" } else { "Source mode" });
        t.keys(vec![ctrl(egui::Key::Backspace), ctrl(egui::Key::Backspace)]);
        assert_eq!(t.0.buffer, "# Нотатки\n\nПерший рядок.\n", "and never past its line");
        t.keys(vec![ctrl(egui::Key::Backspace)]);
        assert_eq!(t.0.buffer, "# Нотатки\n\nПерший рядок.", "then the line break");
        // Ctrl+Delete the other way.
        t.select(0, 0);
        t.keys(vec![ctrl(egui::Key::Delete)]);
        assert_eq!(t.0.buffer, " Нотатки\n\nПерший рядок.");
        // A selection is deleted as it is.
        t.select(0, 8);
        t.keys(vec![ctrl(egui::Key::Backspace)]);
        assert_eq!(t.0.buffer, "\n\nПерший рядок.");
    }
}

#[test]
fn the_graph_is_drawn_less_often_while_it_only_drifts_and_not_at_all_at_rest() {
    let mut t = TempApp::new("graph-pace", &[("A", "[[B]] [[C]]"), ("B", ""), ("C", "")]);
    t.frame();
    t.open_graph();
    // The pace it asks for, and that it asks egui for a frame no sooner, or
    // for none at all.
    let pace = |t: &mut TempApp, events: Vec<egui::Event>, in_front: bool| {
        let output = t.frame_full(events);
        let delay = output.viewport_output.values().map(|viewport| viewport.repaint_delay).min().unwrap();
        let now = t.1.input(|input| input.time);
        let pace = t.graph().frames_per_second(now, in_front);
        // egui takes the time a frame takes to draw off the delay asked for.
        let drawing = t.1.input(|input| input.predicted_dt);
        // Frames in a test are drawn with the window in front.
        match pace {
            Some(pace) if in_front => assert!((delay.as_secs_f32() + drawing - 1.0 / pace).abs() < 1e-3, "{delay:?} for {pace} a second"),
            None => assert_eq!(delay, std::time::Duration::MAX, "at rest it asks for nothing"),
            Some(_) => {}
        }
        pace
    };
    // Opened and left alone, past its spin, it only drifts.
    assert_eq!(pace(&mut t, Vec::new(), true), Some(30.0));
    assert_eq!(pace(&mut t, Vec::new(), false), Some(15.0), "slower again behind another window");
    // The mouse brings it back to full pace at once, and for a while after.
    assert_eq!(pace(&mut t, vec![egui::Event::PointerMoved(egui::pos2(500.0, 300.0))], true), Some(60.0));
    for _ in 0..120 {
        t.frame();
    }
    assert_eq!(pace(&mut t, Vec::new(), true), Some(60.0), "two seconds on, things may still be coming to rest");
    for _ in 0..120 {
        t.frame();
    }
    assert_eq!(pace(&mut t, Vec::new(), true), Some(30.0));
    // So does a flight to a note.
    let (_, at) = t.node("A");
    t.click(at);
    assert_eq!(pace(&mut t, Vec::new(), false), Some(60.0), "busy, even behind another window");

    // Left alone for long enough, it slows to a stop and is no longer drawn.
    for _ in 0..(12 * 60) {
        t.frame();
    }
    assert_eq!(pace(&mut t, Vec::new(), true), None);
    let still = t.graph().at.clone();
    let camera = t.graph().camera.azimuth;
    for _ in 0..60 {
        t.frame();
    }
    assert_eq!((&t.graph().at, t.graph().camera.azimuth), (&still, camera), "nothing moves at rest");
    // Touched again, it picks up from where it stopped, without a jump.
    assert_eq!(pace(&mut t, vec![egui::Event::PointerMoved(egui::pos2(520.0, 310.0))], true), Some(60.0));
    let moved = |a: &[malachite::graph::Vec3], b: &[malachite::graph::Vec3]| a.iter().zip(b).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
    let first = t.graph().at.clone();
    assert!(moved(&still, &first) < 0.5, "a step of a frame, not a jump: {}", moved(&still, &first));
    for _ in 0..120 {
        t.frame();
    }
    assert!(moved(&first, &t.graph().at) > 0.01, "and drifts again");
    assert!(!t.0.failed());
}
