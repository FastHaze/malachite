//! The settings dialog (Ctrl+,), and the app's dealings with the settings
//! file, `malachite.conf`.
//!
//! There are two such files. The app's own, `malachite.conf` in the user's
//! settings folder, is how the app looks, the same in every vault. The open
//! vault's, `.malachite/malachite.conf` inside it, is what is done with that
//! vault's notes as files, and each vault has its own (`ConfFile`).
//!
//! The dialog is the files laid out as things to click: a list of pages on
//! the left (Appearance, which is the app's file, then Files and links, which
//! is the vault's), and the settings of the page beside it, each with a line
//! about what it does. Nothing is kept in the dialog but which page it is on.
//! A change made in it is a line of a file rewritten (`conf.rs`), the file
//! read again for what it now says (`appearance.rs`, `files.rs`), and the
//! window restyled after that; so the dialog and a text editor are two ways
//! of doing the same thing, and what one does shows in the other.
//! The file is written once the mouse is let go, not for every step of a
//! slider, and read again whenever the window is come back to.

use std::path::{Path, PathBuf};

use eframe::egui::{self, pos2, vec2, Align, Color32, Layout, RichText, Stroke};

use crate::appearance::{self, Appearance, Palette, Scheme, PAINTS};
use crate::files::{self, Files, NewNotes, Relink};
use crate::{conf, folders, Action, App};

/// The pages of the dialog, in the order of its list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Page {
    #[default]
    Appearance,
    Files,
}

impl Page {
    const ALL: [Page; 2] = [Page::Appearance, Page::Files];

    fn name(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Files => "Files and links",
        }
    }
}

/// Whose settings a file holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Whose {
    /// The app's, about how it looks.
    App,
    /// A vault's, about its notes as files.
    Vault,
}

/// A settings file: where it is, its text as last read or written, whether
/// that text has changed here and is yet to be written, and what in it could
/// not be understood.
pub(crate) struct ConfFile {
    pub whose: Whose,
    /// None when there is nowhere to keep it, which tests make use of.
    pub path: Option<PathBuf>,
    pub text: String,
    pub unsaved: bool,
    pub problems: Vec<String>,
}

impl ConfFile {
    /// The file at `path` as it is on disk, said as the app now says it: if
    /// that is not how the file said it, it is to be written back.
    pub fn load(whose: Whose, path: Option<PathBuf>) -> Self {
        let on_disk = path.as_deref().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let text = Self::in_step(whose, &on_disk);
        Self { whose, path, unsaved: text != on_disk, text, problems: Vec::new() }
    }

    fn in_step(whose: Whose, text: &str) -> String {
        match whose {
            Whose::App => appearance::in_step(text),
            Whose::Vault => text.to_owned(),
        }
    }

    /// The file as it is first written: every setting it can have, commented out.
    fn template(&self) -> String {
        match self.whose {
            Whose::App => appearance::template(),
            Whose::Vault => files::vault_template(),
        }
    }

    /// What the file is called when something about it is told.
    pub fn name(&self) -> &'static str {
        match self.whose {
            Whose::App => appearance::FILE,
            Whose::Vault => files::VAULT_FILE,
        }
    }

    /// What its text says of how the app looks, keeping what is not understood.
    pub fn look(&mut self) -> Appearance {
        let (look, problems) = Appearance::read(&self.text);
        self.problems = appearance::said(problems);
        look
    }

    /// What its text says of notes as files, keeping what is not understood.
    pub fn files(&mut self) -> Files {
        let (files, problems) = Files::parse(&self.text);
        self.problems = appearance::said(problems);
        files
    }

    /// Take the file as it now is on disk, unless what it is to say has
    /// changed here. Says whether its text is another now.
    fn reload(&mut self) -> bool {
        if self.unsaved {
            return false;
        }
        let Some(path) = &self.path else { return false };
        // A file that is not there says nothing, which is every default.
        let on_disk = std::fs::read_to_string(path).unwrap_or_default();
        let text = Self::in_step(self.whose, &on_disk);
        if text == self.text {
            return false;
        }
        self.unsaved = text != on_disk;
        self.text = text;
        true
    }

    /// Set `key` in `section` to `value`, or comment it out at `default` if
    /// that is what it is. The first change of all starts the file, with every
    /// setting in it for the finding. Says whether the text is another now.
    fn set(&mut self, section: &str, key: &str, value: &str, default: &str) -> bool {
        if self.text.trim().is_empty() {
            self.text = self.template();
        }
        let text = match value == default {
            true => conf::unset(&self.text, section, key, default),
            false => conf::set(&self.text, section, key, value),
        };
        // A change of theme changes what the other lines have to show.
        let text = Self::in_step(self.whose, &text);
        if text == self.text {
            return false;
        }
        self.text = text;
        self.unsaved = true;
        true
    }

    /// Write the file, if what it is to say has changed here.
    fn write(&mut self) -> Result<(), String> {
        if !std::mem::take(&mut self.unsaved) {
            return Ok(());
        }
        let Some(path) = &self.path else { return Ok(()) };
        let folder = path.parent().map_or(Ok(()), std::fs::create_dir_all);
        folder.and_then(|()| std::fs::write(path, &self.text)).map_err(|e| format!("Could not write {}: {e}", path.display()))
    }
}

/// A folder of the vault to choose, from those in `folders`, or the one
/// that nothing names (the top of the vault, or none at all) under the name
/// `nothing`. A folder the file names that this vault does not have is still
/// shown, as such. Gives the one chosen, if one was.
fn folder_choice(ui: &mut egui::Ui, id: &str, current: &Path, folders: &[PathBuf], nothing: &str) -> Option<PathBuf> {
    let named = |folder: &Path| if folder.as_os_str().is_empty() { nothing.to_owned() } else { folders::display(folder) };
    let missing = !current.as_os_str().is_empty() && !folders.iter().any(|folder| folder == current);
    let shown = if missing { format!("{} (not here)", named(current)) } else { named(current) };
    let mut chosen = None;
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(CONTROLS - 20.0).show_ui(ui, |ui| {
        let empty = PathBuf::new();
        for folder in std::iter::once(&empty).chain(folders) {
            if ui.selectable_label(folder == current, named(folder)).clicked() {
                chosen = Some(folder.clone());
            }
        }
    });
    chosen
}

/// How wide the dialog is and how tall its page of settings, where the
/// window has the room; how wide its list of pages is; and how much of a
/// setting's row is kept for what changes it.
const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 420.0;
const PAGES: f32 = 120.0;
const CONTROLS: f32 = 290.0;

/// The room left at the right of the settings for the bar that scrolls them.
const SCROLL_BAR: i8 = 14;

/// How large a theme is shown in the dialog, with its name, and how much of
/// that is the picture of it.
const CARD: egui::Vec2 = vec2(64.0, 50.0);
const PICTURE: f32 = 33.0;

/// A theme to choose: a small picture of the window in its colours, in the
/// scheme that is showing, with its name under it.
fn theme_card(ui: &mut egui::Ui, theme: Palette, dark: bool, chosen: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let color = |index: usize| {
        let [r, g, b, a] = theme.colors(dark)[index];
        Color32::from_rgba_unmultiplied(r, g, b, a)
    };
    let painter = ui.painter();
    let picture = egui::Rect::from_min_size(rect.min, vec2(CARD.x, PICTURE));
    painter.rect_filled(picture, 4.0, color(appearance::PAGE));
    // The panel down the side; and on the page a line of writing, a link, and a quieter line.
    let panel = egui::Rect::from_min_max(picture.min, pos2(picture.left() + 18.0, picture.bottom()));
    painter.rect_filled(panel, egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, color(appearance::SIDEBAR));
    for (row, (index, length)) in [(appearance::TEXT, 32.0), (appearance::ACCENT, 20.0), (appearance::MUTED, 26.0)].into_iter().enumerate() {
        let (from, y) = (picture.left() + 25.0, picture.top() + 9.0 + 7.5 * row as f32);
        painter.hline(from..=from + length, y, Stroke::new(2.0, color(index)));
    }
    let visuals = ui.visuals();
    let (edge, ink) = match (chosen, response.hovered()) {
        (true, _) => (Stroke::new(2.0, visuals.hyperlink_color), visuals.strong_text_color()),
        (false, true) => (visuals.widgets.hovered.bg_stroke, visuals.strong_text_color()),
        (false, false) => (visuals.widgets.noninteractive.bg_stroke, visuals.text_color()),
    };
    painter.rect_stroke(picture, 4.0, edge, egui::StrokeKind::Outside);
    let font = egui::TextStyle::Small.resolve(ui.style());
    painter.text(pos2(rect.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, theme.name(), font, ink);
    response
}

/// One setting: its name with a line about it, and to the right of them
/// what changes it, which `control` lays out from the right.
fn setting(ui: &mut egui::Ui, name: &str, about: &str, control: impl FnOnce(&mut egui::Ui)) {
    let words = (ui.available_width() - CONTROLS).max(120.0);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(words, 0.0), Layout::top_down(Align::LEFT), |ui| {
            ui.set_width(words);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(name);
            ui.label(RichText::new(about).small().weak());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
    ui.add_space(5.0);
}

impl App {
    /// Take the settings files as they now are on disk, if they have changed
    /// there: they are files to be edited by hand, while the app is running too.
    pub(crate) fn reload_conf(&mut self) {
        let changed: Vec<&'static str> = [&mut self.conf, &mut self.vault_conf].into_iter().filter_map(|file| file.reload().then(|| file.name())).collect();
        if changed.is_empty() {
            return;
        }
        self.read_conf();
        if self.conf.problems.is_empty() && self.vault_conf.problems.is_empty() {
            self.info(format!("Read {} again", changed.join(" and ")));
        }
    }

    /// Go by what the files' texts say, and say what in them is not
    /// understood, if that is something new: a problem already told of is
    /// not told again each time something else is changed.
    pub(crate) fn read_conf(&mut self) {
        let before = (self.conf.problems.clone(), self.vault_conf.problems.clone());
        let look = self.conf.look();
        if look != self.look {
            self.look = look;
            self.restyle = true;
        }
        self.files = self.vault_conf.files();
        if (&self.conf.problems, &self.vault_conf.problems) != (&before.0, &before.1) {
            self.report_conf();
        }
    }

    /// Move what the app's own settings file says in `[files]` into the open
    /// vault's file, where it belongs now: a build of the app kept these in
    /// the app's file before vaults had files of their own. What the vault's
    /// file already sets stays as it is. Done when the app starts.
    pub(crate) fn take_over_files(&mut self) {
        let (entries, _) = conf::read(&self.conf.text);
        let moving: Vec<conf::Entry> = entries.into_iter().filter(|entry| entry.section == files::SECTION).collect();
        if moving.is_empty() {
            return;
        }
        let (set, _) = conf::read(&self.vault_conf.text);
        let mut moved = 0;
        for entry in &moving {
            let taken = set.iter().any(|already| already.section == files::SECTION && already.key == entry.key);
            // A default that is never the value, so that the line is written.
            if !taken && self.vault_conf.set(files::SECTION, &entry.key, &entry.value, "\0") {
                moved += 1;
            }
        }
        self.conf.text = conf::without(&self.conf.text, files::SECTION);
        self.conf.unsaved = true;
        self.read_conf();
        if self.conf.problems.is_empty() && self.vault_conf.problems.is_empty() {
            let what = if moved == 1 { "1 setting".to_owned() } else { format!("{moved} settings") };
            self.info(format!("Moved {what} of Files and links from {} into this vault's own {}", appearance::FILE, files::VAULT_FILE));
        }
    }

    /// Tell of the first thing in the settings files that is not understood.
    pub(crate) fn report_conf(&mut self) {
        let files = [&self.conf, &self.vault_conf];
        let Some((name, first)) = files.iter().find_map(|file| Some((file.name(), file.problems.first()?))) else { return };
        let more = match files.iter().map(|file| file.problems.len()).sum::<usize>() - 1 {
            0 => String::new(),
            more => format!(" (and {more} more)"),
        };
        self.fail(format!("{name}: {first}{more}"));
    }

    /// Change one setting, in its file's text and so in what the app goes by:
    /// the vault's file for `[files]`, the app's for the rest. A setting given
    /// the value it has when left alone is commented out again.
    pub(crate) fn set_option(&mut self, section: &str, key: &str, value: &str, default: &str) {
        let file = if section == files::SECTION { &mut self.vault_conf } else { &mut self.conf };
        if file.set(section, key, value, default) {
            self.read_conf();
        }
    }

    /// Write the settings files whose texts have changed here.
    pub(crate) fn write_conf(&mut self) {
        for written in [self.conf.write(), self.vault_conf.write()] {
            if let Err(e) = written {
                self.fail(e);
            }
        }
    }

    /// The file of one or the other's settings.
    fn conf_of(&mut self, whose: Whose) -> &mut ConfFile {
        match whose {
            Whose::App => &mut self.conf,
            Whose::Vault => &mut self.vault_conf,
        }
    }

    /// Open a settings file in a text editor, starting the file if there is none yet.
    fn edit_conf(&mut self, whose: Whose) {
        let file = self.conf_of(whose);
        let Some(path) = file.path.clone() else { return };
        if file.text.trim().is_empty() {
            file.text = file.template();
        }
        file.unsaved |= !path.is_file();
        self.write_conf();
        if let Err(e) = open_outside(&path, true) {
            self.fail(format!("Could not open {}: {e}", path.display()));
        }
    }

    pub(crate) fn settings_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.show_settings {
            return;
        }
        self.modal_drawn = true;
        // A small window gets a smaller dialog.
        let screen = ctx.screen_rect().size();
        let (width, height) = ((screen.x - 60.0).clamp(360.0, WIDTH), (screen.y - 190.0).clamp(140.0, HEIGHT));
        let mut close = false;
        // The page it is on is the only thing the dialog keeps, and it keeps
        // it with egui, for as long as the app is open.
        let page_id = egui::Id::new("settings page");
        let mut page = ctx.data(|data| data.get_temp::<Page>(page_id)).unwrap_or_default();
        let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.heading("Settings");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Close (Esc)").clicked();
                });
            });
            ui.separator();
            let columns = ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(PAGES, height), Layout::top_down_justified(Align::LEFT), |ui| {
                    ui.set_width(PAGES);
                    for each in Page::ALL {
                        let row = ui.selectable_label(page == each, each.name());
                        #[cfg(test)]
                        self.spots.insert(each.name(), row.rect);
                        if row.clicked() {
                            page = each;
                        }
                    }
                });
                ui.add_space(8.0);
                let line = ui.cursor().left() - 8.0;
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().max_height(height).auto_shrink(false).show(ui, |ui| {
                        // Clear of the scroll bar, which lies over the edge of what it scrolls.
                        let clear = egui::Margin { right: SCROLL_BAR, ..egui::Margin::ZERO };
                        egui::Frame::new().inner_margin(clear).show(ui, |ui| match page {
                            Page::Appearance => self.appearance_page(ui),
                            Page::Files => self.files_page(ui),
                        });
                    });
                });
                line
            });
            // Between the pages and the settings, as tall as they turned out.
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(columns.inner, columns.response.rect.y_range(), stroke);
            ui.separator();
            self.conf_footer(ui, if page == Page::Files { Whose::Vault } else { Whose::App });
        });
        ctx.data_mut(|data| data.insert_temp(page_id, page));
        if close || modal.should_close() {
            self.show_settings = false;
            // Back to the note, where the keyboard was.
            actions.push(Action::FocusEditor);
        }
    }

    /// The Appearance page: what `[appearance]` and the colours of the
    /// scheme now showing say, each as something to click. The themes are
    /// pictures of themselves.
    fn appearance_page(&mut self, ui: &mut egui::Ui) {
        use appearance::{colors, hex, number, SECTION};
        let dark = ui.visuals().dark_mode;
        let look = self.look.clone();

        setting(ui, "Theme", &format!("{} Every theme has a light scheme and a dark one.", look.palette.about()), |ui| {
            // Laid out from the right, so the last of them goes in first.
            for theme in Palette::ALL.into_iter().rev() {
                let card = theme_card(ui, theme, dark, look.palette == theme);
                #[cfg(test)]
                self.spots.insert(theme.name(), card.rect);
                if card.on_hover_text(theme.about()).clicked() {
                    self.set_option(SECTION, "theme", theme.word(), Palette::default().word());
                }
            }
        });
        // A colour the file sets is that colour in any theme, which a theme
        // just chosen can look the worse for: say so, with the way out.
        let own: Vec<usize> = (0..PAINTS.len()).filter(|index| look.is_set(dark, *index)).collect();
        if !own.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let which = match own.as_slice() {
                    [one] => format!("{} is", PAINTS[*one].name),
                    [one, other] => format!("{} and {} are", PAINTS[*one].name, PAINTS[*other].name),
                    many => format!("{} colours are", many.len()),
                };
                ui.label(RichText::new(format!("{which} set to a colour of your own, in this theme and any other.")).small().weak());
                // Whole, on the next line if it does not fit on this one:
                // a link broken over two lines reads as two.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let theirs = ui.link(RichText::new("Use the theme's").small());
                #[cfg(test)]
                self.spots.insert("Use the theme's", theirs.rect);
                if theirs.on_hover_text("Put every colour of this scheme back to the theme's own").clicked() {
                    for index in own {
                        let default = hex(look.themed(dark, index));
                        self.set_option(colors(dark), PAINTS[index].key, &default, &default);
                    }
                }
            });
            ui.add_space(5.0);
        }
        setting(ui, "Colour scheme", "Light or dark, or whichever of them the system is set to.", |ui| {
            for (scheme, name) in [(Scheme::Light, "Light"), (Scheme::Dark, "Dark"), (Scheme::System, "System")] {
                if ui.selectable_label(look.scheme == scheme, name).clicked() {
                    self.set_option(SECTION, "scheme", scheme.word(), Scheme::System.word());
                }
            }
        });
        setting(ui, "Text size", "Of a note's text, in points. Headings and code are sized from it.", |ui| {
            let mut size = look.text_size;
            if ui.add(egui::Slider::new(&mut size, appearance::TEXT_SIZES).step_by(1.0).fixed_decimals(0)).changed() {
                self.set_option(SECTION, "text_size", &number(size), &number(appearance::TEXT_SIZE));
            }
        });
        setting(ui, "Line width", "The widest a line of a note gets, in points. Unticked: as wide as there is room for.", |ui| {
            let mut limited = look.line_width > 0.0;
            let mut width = if limited { look.line_width } else { appearance::LINE_WIDTH };
            let drag = egui::DragValue::new(&mut width).range(appearance::LINE_WIDTHS).speed(5.0).max_decimals(0);
            let dragged = ui.add_enabled(limited, drag).changed();
            if ui.checkbox(&mut limited, "").changed() || dragged {
                let value = if limited { number(width) } else { "0".to_owned() };
                self.set_option(SECTION, "line_width", &value, &number(appearance::LINE_WIDTH));
            }
        });
        setting(ui, "Note title", "The note's name above its text, where it can be renamed. F2 renames it either way.", |ui| {
            let mut shown = look.show_title;
            if ui.checkbox(&mut shown, "").changed() {
                self.set_option(SECTION, "show_title", &shown.to_string(), "true");
            }
        });
        setting(ui, "Zoom", "Of the whole window: Ctrl with + and −. Remembered, though not a line of the file.", |ui| {
            self.zoom_control(ui);
        });

        ui.add_space(8.0);
        ui.strong(format!("Colours of the {} scheme", if dark { "dark" } else { "light" }));
        ui.label(RichText::new("They are the theme's until they are changed here. The other scheme has colours of its own: they are shown, and set, while it is the one in use.").small().weak());
        ui.add_space(6.0);
        for (index, paint) in PAINTS.iter().enumerate() {
            setting(ui, paint.name, paint.about, |ui| {
                let default = look.themed(dark, index);
                let mut color = look.rgba(dark, index);
                // Only the highlight lets what is behind it through.
                let picked = if index == appearance::HIGHLIGHT {
                    ui.color_edit_button_srgba_unmultiplied(&mut color).changed()
                } else {
                    let mut solid = [color[0], color[1], color[2]];
                    let picked = ui.color_edit_button_srgb(&mut solid).changed();
                    color = [solid[0], solid[1], solid[2], color[3]];
                    picked
                };
                let reset = egui::Button::new(RichText::new("Reset").small()).frame(false);
                if look.is_set(dark, index) && ui.add(reset).on_hover_text("Back to the theme's colour").clicked() {
                    self.set_option(colors(dark), paint.key, &hex(default), &hex(default));
                } else if picked {
                    self.set_option(colors(dark), paint.key, &hex(color), &hex(default));
                }
            });
        }
    }

    /// The Files and links page: what the open vault's `[files]` says.
    fn files_page(&mut self, ui: &mut egui::Ui) {
        use files::{written, SECTION};
        let chosen = self.files.clone();
        let defaults = Files::default();
        let vault_folders = self.folders.clone();
        ui.label(RichText::new(format!("These are this vault's own, kept in it: another vault has its own. Vault: {}", self.vault.root().display())).small().weak());
        ui.add_space(6.0);

        setting(ui, "New notes", "Where a note starts when it is made with Ctrl+N, or by following a link to a note that is not there yet: beside the note that is open or that has the link, at the top of the vault, or in a folder.", |ui| {
            for place in NewNotes::ALL.into_iter().rev() {
                let choice = ui.selectable_label(chosen.new_notes == place, place.name());
                #[cfg(test)]
                self.spots.insert(place.name(), choice.rect);
                if choice.clicked() {
                    self.set_option(SECTION, "new_notes", place.word(), defaults.new_notes.word());
                }
            }
        });
        if chosen.new_notes == NewNotes::Folder {
            setting(ui, "Folder for new notes", "Made when a note is first put in it, if it is not there.", |ui| {
                if let Some(folder) = folder_choice(ui, "new notes", &chosen.new_notes_folder, &vault_folders, "Top level") {
                    self.set_option(SECTION, "new_notes_folder", &written(&folder), "");
                }
            });
        }
        setting(ui, "Update links when renaming", "Whether renaming a note changes the links other notes have to it, so that they still lead there. Ask: each time there are some.", |ui| {
            for when in Relink::ALL.into_iter().rev() {
                let choice = ui.selectable_label(chosen.relink == when, when.name());
                #[cfg(test)]
                self.spots.insert(when.name(), choice.rect);
                if choice.clicked() {
                    self.set_option(SECTION, "update_links", when.word(), defaults.relink.word());
                }
            }
        });
        let bin = if malachite::SYSTEM_BIN { "" } else { " The system's bin cannot be reached on this system." };
        setting(ui, "Deleted notes and folders", &format!("Where they go: the vault's own .trash folder, the system's bin, or nowhere, deleted for good.{bin}"), |ui| {
            egui::ComboBox::from_id_salt("deleted").selected_text(files::discard_name(chosen.deleted)).width(CONTROLS - 20.0).show_ui(ui, |ui| {
                for how in files::DISCARDS {
                    let possible = how != malachite::Discard::System || malachite::SYSTEM_BIN;
                    if ui.add_enabled(possible, egui::SelectableLabel::new(chosen.deleted == how, files::discard_name(how))).clicked() {
                        self.set_option(SECTION, "deleted", files::discard_word(how), files::discard_word(defaults.deleted));
                    }
                }
            });
        });
        setting(ui, "Templates folder", "Its notes, and those of the folders in it, are the templates that Insert template offers.", |ui| {
            if let Some(folder) = folder_choice(ui, "templates", &chosen.templates, &vault_folders, "None") {
                self.set_option(SECTION, "templates", &written(&folder), &written(&defaults.templates));
            }
        });

        ui.add_space(8.0);
        ui.strong("Daily notes");
        ui.label(RichText::new("Today's note is opened from the button above the note list, or the command palette. It is made the first time.").small().weak());
        ui.add_space(6.0);
        setting(ui, "Folder", "Where a new daily note goes. One that has been moved elsewhere is still found.", |ui| {
            if let Some(folder) = folder_choice(ui, "daily folder", &chosen.daily_folder, &vault_folders, "Top level") {
                self.set_option(SECTION, "daily_folder", &written(&folder), "");
            }
        });
        let today = malachite::template::formatted(chrono::Local::now().naive_local(), &chosen.daily_format);
        let named = match malachite::daily_date(&today, &chosen.daily_format) {
            Some(_) => format!("Today's is called \"{today}\"."),
            None => format!("\"{today}\" does not say which day it is: the earlier and later daily notes cannot be found by it."),
        };
        setting(ui, "Name", &format!("In letters that stand for parts of a date: YYYY the year, MM the month, DD the day, dddd the weekday, MMMM the month's name. {named}"), |ui| {
            let mut format = chosen.daily_format.clone();
            let field = ui.add(egui::TextEdit::singleline(&mut format).desired_width(CONTROLS - 20.0));
            if field.changed() && !format.trim().is_empty() {
                self.set_option(SECTION, "daily_format", format.trim(), &defaults.daily_format);
            }
        });
        let templates: Vec<String> = self.templates().filter_map(|note| note.path.strip_prefix(self.vault.root()).ok()).map(|path| folders::display(&path.with_extension(""))).collect();
        setting(ui, "Template", "The note a new daily note starts as, filled in as a template is. None: it starts with its name as a heading.", |ui| {
            let missing = !chosen.daily_template.is_empty() && !templates.contains(&chosen.daily_template);
            let shown = match chosen.daily_template.as_str() {
                "" => "None".to_owned(),
                name if missing => format!("{name} (not here)"),
                name => name.to_owned(),
            };
            egui::ComboBox::from_id_salt("daily template").selected_text(shown).width(CONTROLS - 20.0).show_ui(ui, |ui| {
                for name in std::iter::once("").chain(templates.iter().map(String::as_str)) {
                    if ui.selectable_label(chosen.daily_template == name, if name.is_empty() { "None" } else { name }).clicked() {
                        self.set_option(SECTION, "daily_template", name, "");
                    }
                }
            });
        });
    }

    /// Under the settings of a page: where they are kept, and what in that
    /// file is not understood.
    fn conf_footer(&mut self, ui: &mut egui::Ui, whose: Whose) {
        const SHOWN: usize = 4;
        let file = self.conf_of(whose);
        let (problems, path) = (file.problems.clone(), file.path.clone());
        for problem in problems.iter().take(SHOWN) {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
        if problems.len() > SHOWN {
            ui.colored_label(ui.visuals().error_fg_color, format!("…and {} more", problems.len() - SHOWN));
        }
        let Some(path) = path else {
            ui.label(RichText::new("These last for as long as the app is open: it has no folder to keep its settings in.").small().weak());
            return;
        };
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hint = match whose {
                    Whose::App => "Every setting of this page is a line of this file. Change it in a text editor, and the app goes by it when you come back.",
                    Whose::Vault => "This vault's own settings file: every setting of this page is a line of it. Another vault has its own. Change it in a text editor, and the app goes by it when you come back.",
                };
                if ui.button("Open the file").on_hover_text(hint).clicked() {
                    self.edit_conf(whose);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let kept = RichText::new(format!("Kept in {}", path.display())).small().weak();
                    ui.add(egui::Label::new(kept).truncate()).on_hover_text(path.display().to_string());
                });
            });
        });
    }
}

/// Open a file in the program the system has for it, or with `text` in its
/// plain text editor.
pub(crate) fn open_outside(path: &Path, text: bool) -> std::io::Result<()> {
    let (program, arguments): (&str, &[&str]) = if cfg!(windows) {
        (if text { "notepad.exe" } else { "explorer.exe" }, &[])
    } else if cfg!(target_os = "macos") {
        ("open", if text { &["-t"] } else { &[] })
    } else {
        ("xdg-open", &[])
    };
    std::process::Command::new(program).args(arguments).arg(path).spawn().map(|_| ())
}
