//! Fonts and colours, for note text and for the interface around it. The
//! colours, and the size of a note's text, are what the settings file says
//! they are (`appearance.rs`): a theme, and what the file sets over it. The
//! fonts are not there to be chosen yet.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, vec2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, TextFormat, Vec2};
use malachite::highlight::Token;
use malachite::markdown::{Link, Style};

use crate::appearance::{self, Appearance, Scheme, BLUE, CYAN, GRAY, GREEN, ORANGE, PINK, PURPLE, RED, YELLOW};

/// The sizes of the text around the note. A note's own text has the size
/// the settings give it, and its headings, code and tags are sized relative
/// to that. Zooming scales all of them together.
pub mod size {
    /// Everything to click or read around the note: lists, buttons, panels, hints.
    pub const UI: f32 = 13.0;
    /// The status bar.
    pub const UI_SMALL: f32 = 12.0;
    /// Titles of dialogs.
    pub const UI_LARGE: f32 = 20.0;
}

/// Font families for note text, regular, bold, italic and bold italic.
/// Every family is registered even when its file is missing; it then falls
/// back to egui's built-in font.
const FACES: [&str; 4] = ["note", "note-bold", "note-italic", "note-bold-italic"];

/// The files of a font every system of its kind has, in the order of
/// `FACES`: Segoe UI on Windows, Arial on macOS, and on Linux DejaVu Sans
/// or Noto Sans, in the folders the larger distributions keep them in.
/// The first whose regular face is there is the one used.
fn face_files() -> Vec<[PathBuf; 4]> {
    let set = |dir: &Path, files: [&str; 4]| files.map(|file| dir.join(file));
    if cfg!(windows) {
        let fonts = std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts"));
        fonts.map(|dir| set(&dir, ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"])).into_iter().collect()
    } else if cfg!(target_os = "macos") {
        let dir = Path::new("/System/Library/Fonts/Supplemental");
        vec![set(dir, ["Arial.ttf", "Arial Bold.ttf", "Arial Italic.ttf", "Arial Bold Italic.ttf"])]
    } else {
        let dejavu = ["DejaVuSans.ttf", "DejaVuSans-Bold.ttf", "DejaVuSans-Oblique.ttf", "DejaVuSans-BoldOblique.ttf"];
        let noto = ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf", "NotoSans-Italic.ttf", "NotoSans-BoldItalic.ttf"];
        let dejavu_dirs = ["/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/dejavu-sans-fonts", "/usr/share/fonts/TTF", "/usr/share/fonts/dejavu"];
        let noto_dirs = ["/usr/share/fonts/truetype/noto", "/usr/share/fonts/noto", "/usr/share/fonts/google-noto"];
        let dejavu = dejavu_dirs.iter().map(|dir| set(Path::new(dir), dejavu));
        dejavu.chain(noto_dirs.iter().map(|dir| set(Path::new(dir), noto))).collect()
    }
}

/// Set up fonts and the look of the interface.
///
/// Notes are shown in the system's own sans-serif font (`face_files`) and
/// need real bold and italic faces, which egui's built-in font does not
/// have. The interface around the note uses the same font, so the two match.
/// Returns whether all faces were found.
pub fn install(ctx: &egui::Context) -> bool {
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let files = face_files().into_iter().find(|files| files[0].is_file());
    let mut found = 0;
    for (index, family) in FACES.into_iter().enumerate() {
        let mut chain = fallback.clone();
        if let Some(bytes) = files.as_ref().and_then(|files| std::fs::read(&files[index]).ok()) {
            fonts.font_data.insert(family.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            chain.insert(0, family.to_owned());
            found += 1;
        }
        fonts.families.insert(FontFamily::Name(family.into()), chain.clone());
        if family == "note" {
            fonts.families.insert(FontFamily::Proportional, chain);
        }
    }
    ctx.set_fonts(fonts);
    install_style(ctx);
    apply(ctx, &Appearance::default());
    found == FACES.len()
}

/// Sizes and spacing of the interface around the note.
fn install_style(ctx: &egui::Context) {
    // Zooming is the app's own (see `App::zoom`), so that it can be shown and remembered.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.all_styles_mut(|style| {
        use egui::TextStyle::{Body, Button, Heading, Monospace, Small};
        let sizes = [(Heading, size::UI_LARGE), (Body, size::UI), (Button, size::UI), (Small, size::UI_SMALL), (Monospace, size::UI)];
        for (text_style, size) in sizes {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
        style.spacing.item_spacing = vec2(8.0, 5.0);
        style.spacing.button_padding = vec2(8.0, 3.0);
        // Labels of the interface are not text to select; the note itself is.
        style.interaction.selectable_labels = false;
    });
}

/// A colour `part` of the way from `from` to `to`.
fn mix(from: Color32, to: Color32, part: f32) -> Color32 {
    let between = |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * part).round() as u8;
    Color32::from_rgb(between(from.r(), to.r()), between(from.g(), to.g()), between(from.b(), to.b()))
}

/// Make the window look as `look` says: light or dark, and in both of them
/// the colours of the interface around the note. To be called again whenever
/// the look changes.
///
/// A theme has no colours for buttons, fields and menus. They are made here
/// out of the two it has for the panels and for text: each is the panel's
/// colour taken some part of the way to the text's, or the text's taken some
/// way back. The parts are the ones that turn plain greys into the greys
/// such things have in any app, a pair of them for the dark scheme and the
/// light one; so a grey theme looks as it would have without this, and a
/// theme of another colour has that colour in everything.
pub fn apply(ctx: &egui::Context, look: &Appearance) {
    ctx.set_theme(match look.scheme {
        Scheme::System => egui::ThemePreference::System,
        Scheme::Dark => egui::ThemePreference::Dark,
        Scheme::Light => egui::ThemePreference::Light,
    });
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        let colors = Theme::of(look, dark, false);
        let (panel, text) = (colors.sidebar, colors.text);
        // Of the panel's colour, towards the text's; and of the text's, back towards the panel's.
        let ground = |in_dark: f32, in_light: f32| mix(panel, text, if dark { in_dark } else { in_light });
        let ink = |in_dark: f32, in_light: f32| mix(text, panel, if dark { in_dark } else { in_light });
        ctx.style_mut_of(theme, |style| {
            let visuals = &mut style.visuals;
            visuals.panel_fill = panel;
            // Dialogs and menus stand between the panels and the page.
            visuals.window_fill = mix(panel, colors.page, 0.5);
            visuals.window_stroke.color = colors.rule;
            // What is typed into: the page's own colour, or in the dark something deeper.
            visuals.extreme_bg_color = if dark { mix(colors.page, Color32::BLACK, 0.6) } else { colors.page };
            visuals.code_bg_color = colors.code_bg;
            visuals.hyperlink_color = colors.accent;
            visuals.warn_fg_color = colors.hue(ORANGE);
            visuals.error_fg_color = colors.hue(RED);
            // The open note in the list, and selected text, in the accent colour.
            visuals.selection.bg_fill = colors.accent.gamma_multiply(0.4);
            visuals.selection.stroke.color = colors.strong;

            let widgets = &mut visuals.widgets;
            // What is only read: labels, and the lines between things.
            widgets.noninteractive.bg_fill = panel;
            widgets.noninteractive.weak_bg_fill = panel;
            widgets.noninteractive.bg_stroke.color = colors.rule;
            widgets.noninteractive.fg_stroke.color = ink(0.43, 0.22);
            // A button as it stands, under the mouse, and held down.
            widgets.inactive.bg_fill = ground(0.12, 0.075);
            widgets.inactive.weak_bg_fill = ground(0.12, 0.075);
            widgets.inactive.fg_stroke.color = ink(0.21, 0.12);
            widgets.hovered.bg_fill = ground(0.18, 0.12);
            widgets.hovered.weak_bg_fill = ground(0.18, 0.12);
            widgets.hovered.bg_stroke.color = ground(0.62, 0.66);
            widgets.hovered.fg_stroke.color = mix(text, colors.strong, if dark { 0.6 } else { 1.0 });
            widgets.active.bg_fill = ground(0.095, 0.38);
            widgets.active.weak_bg_fill = ground(0.095, 0.38);
            widgets.active.bg_stroke.color = colors.strong;
            widgets.active.fg_stroke.color = colors.strong;
            // One with its menu open.
            widgets.open.bg_fill = ground(0.0, 0.12);
            widgets.open.weak_bg_fill = ground(0.04, 0.12);
            widgets.open.bg_stroke.color = ground(0.12, 0.4);
            widgets.open.fg_stroke.color = if dark { text } else { colors.strong };
        });
    }
}

pub struct Theme {
    /// The size of a note's text.
    pub size: f32,
    pub text: Color32,
    pub muted: Color32,
    /// Syntax characters, list bullets, checkbox borders.
    pub faint: Color32,
    /// Links, tags, checked boxes, quote bars.
    pub accent: Color32,
    pub code_bg: Color32,
    pub rule: Color32,
    /// Behind the note. The panels around it are a shade apart (`sidebar`),
    /// so the note reads as the page.
    pub page: Color32,
    sidebar: Color32,
    highlight: Color32,
    strong: Color32,
    check: Color32,
    /// The colours of code and of callouts, from red to gray.
    hues: [Color32; 9],
    /// Whether the bold and italic font faces are available.
    faces: bool,
    /// Tells this look from every other (see `key`).
    key: u64,
}

impl Theme {
    /// The look of the app when its settings say nothing.
    #[cfg(test)]
    pub fn new(dark: bool, faces: bool) -> Self {
        Self::of(&Appearance::default(), dark, faces)
    }

    /// The look that `look` describes, in its dark scheme or its light one.
    pub fn of(look: &Appearance, dark: bool, faces: bool) -> Self {
        let paint = |index| look.paint(dark, index);
        let mut theme = Self {
            size: look.text_size,
            text: paint(appearance::TEXT),
            muted: paint(appearance::MUTED),
            faint: paint(appearance::FAINT),
            accent: paint(appearance::ACCENT),
            code_bg: paint(appearance::CODE_BACKGROUND),
            rule: paint(appearance::RULE),
            page: paint(appearance::PAGE),
            sidebar: paint(appearance::SIDEBAR),
            highlight: paint(appearance::HIGHLIGHT),
            // The strongest text there is on this scheme, and a tick drawn in the page's own colour.
            strong: if dark { Color32::WHITE } else { Color32::BLACK },
            check: paint(appearance::PAGE),
            hues: std::array::from_fn(|hue| paint(RED + hue)),
            faces,
            key: 0,
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let colors = [theme.text, theme.muted, theme.faint, theme.accent, theme.code_bg, theme.rule, theme.page, theme.highlight];
        (theme.size.to_bits(), colors, theme.hues, dark, faces).hash(&mut hasher);
        theme.key = hasher.finish();
        theme
    }

    /// A number that is another one for another look: what is laid out for
    /// one look has to be laid out again for the next.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The heading scale: 2em for H1 down to 1.12em for H5 and H6.
    pub fn heading_size(&self, level: u8) -> f32 {
        const SCALE: [f32; 6] = [2.0, 1.6, 1.37, 1.25, 1.12, 1.12];
        self.size * SCALE[usize::from(level.clamp(1, 6)) - 1]
    }

    /// Format for content in `style`.
    pub fn format(&self, style: Style, size: f32, color: Color32) -> TextFormat {
        let family = match (self.faces, style.bold, style.italic) {
            (true, true, true) => "note-bold-italic",
            (true, true, false) => "note-bold",
            (true, false, true) => "note-italic",
            _ => "note",
        };
        let mut format = TextFormat {
            font_id: FontId::new(size, FontFamily::Name(family.into())),
            color,
            ..Default::default()
        };
        if style.code {
            format.font_id = FontId::new(size * 0.875, FontFamily::Monospace);
            format.background = self.code_bg;
        } else if !self.faces {
            // No real faces to switch to: slant the regular one, brighten bold text.
            format.italics = style.italic;
            if style.bold && color == self.text {
                format.color = self.strong;
            }
        }
        if style.strike {
            format.strikethrough = Stroke::new(1.0, format.color);
        }
        if style.highlight {
            // A highlight that names its colour has it exactly as written.
            format.background = style.tint.map_or(self.highlight, |[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        if style.comment {
            format.color = self.faint;
        }
        format
    }

    /// Syntax characters that are currently shown.
    pub fn syntax(&self, size: f32) -> TextFormat {
        self.format(Style::default(), size, self.faint)
    }

    /// Syntax characters that are hidden: too small to take up any room.
    pub fn hidden(&self) -> TextFormat {
        TextFormat {
            font_id: FontId::new(0.01, FontFamily::Name("note".into())),
            color: Color32::TRANSPARENT,
            ..Default::default()
        }
    }

    /// Characters that keep their width but are drawn over, like a list's `-`.
    pub fn invisible(&self, size: f32) -> TextFormat {
        self.format(Style::default(), size, Color32::TRANSPARENT)
    }

    /// Code blocks and other literal text.
    pub fn code(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size * 0.875, FontFamily::Monospace),
            color,
            ..Default::default()
        }
    }

    /// Restyle `format` as a link. Links to notes that do not exist yet are fainter.
    pub fn link(&self, mut format: TextFormat, link: &Link, resolved: bool) -> TextFormat {
        match link {
            Link::Tag(_) => {
                format.font_id.size *= 0.875;
                format.color = self.accent;
                format.background = self.accent.gamma_multiply(0.15);
            }
            // A footnote's mark is small, and in Reading view raised.
            Link::Footnote(_) => {
                format.font_id.size *= 0.75;
                format.color = self.accent;
            }
            Link::Note(..) | Link::Url(_) | Link::Embed { .. } => {
                format.color = self.accent.gamma_multiply(if resolved { 1.0 } else { 0.7 });
                let line = self.accent.gamma_multiply(if resolved { 1.0 } else { 0.35 });
                format.underline = Stroke::new(1.0, line);
            }
        }
        format
    }

    /// Code that is part of a block: `token` is its role, or `None` for
    /// code with no role.
    pub fn code_token(&self, token: Option<Token>) -> TextFormat {
        let color = match token {
            None | Some(Token::Punctuation) => self.muted,
            Some(Token::Comment) => self.faint,
            Some(Token::Function) => self.hue(YELLOW),
            Some(Token::Important) => self.hue(ORANGE),
            Some(Token::Keyword) => self.hue(PINK),
            Some(Token::Operator | Token::Tag) => self.hue(RED),
            Some(Token::Property) => self.hue(CYAN),
            Some(Token::String) => self.hue(GREEN),
            Some(Token::Value) => self.hue(PURPLE),
        };
        self.code(color)
    }

    /// One of the colours of code and callouts, by its place among the settings' colours.
    fn hue(&self, index: usize) -> Color32 {
        self.hues[index - RED]
    }

    /// Callouts are coloured by type; unknown types look like `note`.
    pub fn callout(&self, kind: &str) -> Color32 {
        self.hue(match kind {
            "abstract" | "summary" | "tldr" | "tip" | "hint" | "important" => CYAN,
            "success" | "check" | "done" => GREEN,
            "question" | "help" | "faq" | "warning" | "caution" | "attention" => ORANGE,
            "failure" | "fail" | "missing" | "danger" | "error" | "bug" => RED,
            "example" => PURPLE,
            "quote" | "cite" => GRAY,
            _ => BLUE,
        })
    }

    /// Fill behind a callout of the given colour.
    /// The fill behind a place the find bar has found, stronger for the current one.
    pub fn find_mark(&self, current: bool) -> Color32 {
        if current {
            Color32::from_rgba_unmultiplied(255, 140, 0, 150)
        } else {
            Color32::from_rgba_unmultiplied(255, 208, 0, 80)
        }
    }

    pub fn callout_fill(&self, color: Color32) -> Color32 {
        color.gamma_multiply(0.12)
    }

    pub fn checkbox_size(&self) -> Vec2 {
        Vec2::splat(self.size * 0.9)
    }

    pub fn paint_checkbox(&self, painter: &egui::Painter, center: Pos2, done: bool) {
        let rect = Rect::from_center_size(center, self.checkbox_size());
        if done {
            painter.rect_filled(rect, 3.0, self.accent);
            let at = |x: f32, y: f32| rect.min + rect.size() * vec2(x, y);
            let stroke = Stroke::new(1.6, self.check);
            painter.line_segment([at(0.24, 0.52), at(0.43, 0.70)], stroke);
            painter.line_segment([at(0.43, 0.70), at(0.77, 0.32)], stroke);
        } else {
            painter.rect_stroke(rect, 3.0, Stroke::new(1.0, self.faint), egui::StrokeKind::Inside);
        }
    }

    pub fn paint_bullet(&self, painter: &egui::Painter, center: Pos2) {
        painter.circle_filled(center, self.size * 0.16, self.faint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grey_theme_has_the_greys_the_interface_had_without_one() {
        let ctx = egui::Context::default();
        apply(&ctx, &Appearance::parse("[appearance]\ntheme = obsidian\n").0);
        // Label, button, its text; under the mouse, its edge and its text; held down; with its menu open.
        for (theme, greys) in [(egui::Theme::Dark, [140, 60, 180, 70, 150, 240, 55, 45]), (egui::Theme::Light, [80, 230, 60, 220, 105, 0, 165, 220])] {
            let widgets = ctx.style_of(theme).visuals.widgets.clone();
            let made = [
                widgets.noninteractive.fg_stroke.color,
                widgets.inactive.weak_bg_fill,
                widgets.inactive.fg_stroke.color,
                widgets.hovered.weak_bg_fill,
                widgets.hovered.bg_stroke.color,
                widgets.hovered.fg_stroke.color,
                widgets.active.weak_bg_fill,
                widgets.open.weak_bg_fill,
            ];
            for (color, grey) in made.into_iter().zip(greys) {
                let parts = [color.r(), color.g(), color.b()];
                assert!(parts.iter().all(|part| part.abs_diff(grey) <= 1), "{theme:?}: {parts:?} for {grey}");
            }
        }
    }

    #[test]
    fn a_theme_of_a_colour_has_that_colour_in_what_is_made_from_it() {
        let ctx = egui::Context::default();
        apply(&ctx, &Appearance::default());
        let visuals = ctx.style_of(egui::Theme::Dark).visuals.clone();
        for color in [visuals.panel_fill, visuals.window_fill, visuals.extreme_bg_color, visuals.widgets.inactive.weak_bg_fill, visuals.widgets.hovered.weak_bg_fill] {
            assert!(color.g() > color.r() && color.g() > color.b(), "{color:?} is a green");
        }
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.5), Color32::from_gray(128));
    }
}
