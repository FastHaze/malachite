//! How the app looks, as the settings file says it: which theme, its light
//! scheme or its dark one, the size of a note's text, how wide its lines may
//! get, and any colour of either scheme that is to be other than the theme's.
//!
//! A theme is a set of colours with a name (`Palette`): the same eighteen
//! (`PAINTS`) for a light scheme and for a dark one. The colours of the
//! interface around the note are not among them; they are worked out from
//! these (`theme::apply`), so a theme is these two rows of colours and
//! nothing more.
//!
//! The file (`conf.rs` has its format) is the one place these are kept. It
//! is the app's, the same whichever vault is open; what a vault keeps for
//! itself is in a file of its own (`files.rs`). It can be edited by hand,
//! and the settings dialog is another way of editing it: a change made there is a line rewritten here. A setting the file does
//! not have is at its default, and a value that cannot be understood is
//! passed over, and reported, rather than stopping the app from starting.

use std::ops::RangeInclusive;

use eframe::egui::Color32;

use crate::{conf, files};

/// The name of the settings file, which is kept beside `settings.txt`.
pub const FILE: &str = "malachite.conf";

/// The sections of the file that are about looks.
pub const SECTION: &str = "appearance";
const COLORS: [&str; 2] = ["colors.light", "colors.dark"];

/// The sizes a note's text can have, and the size it has unless told otherwise.
pub const TEXT_SIZES: RangeInclusive<f32> = 8.0..=40.0;
pub const TEXT_SIZE: f32 = 16.0;

/// How wide a note's lines can be told to get, and the width they stop at
/// unless told otherwise. No width at all, written `0`, is as wide as the window.
pub const LINE_WIDTHS: RangeInclusive<f32> = 300.0..=2000.0;
pub const LINE_WIDTH: f32 = 700.0;

/// Whether the window is light or dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scheme {
    /// As the system is.
    System,
    Dark,
    Light,
}

impl Scheme {
    /// The word for it in the file.
    pub fn word(self) -> &'static str {
        match self {
            Scheme::System => "system",
            Scheme::Dark => "dark",
            Scheme::Light => "light",
        }
    }

    /// The scheme the file means by `word`, if it is one.
    fn named(word: &str) -> Option<Self> {
        match word.trim().to_lowercase().as_str() {
            "system" | "auto" => Some(Scheme::System),
            "dark" => Some(Scheme::Dark),
            "light" => Some(Scheme::Light),
            _ => None,
        }
    }
}

/// A theme: every colour of the app, for a light scheme and for a dark one.
/// The file and the dialog call it a theme. Here it is the palette that a
/// look starts from, `theme::Theme` being the look once it is made up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Palette {
    /// The greens of malachite, the stone the app is named for and its icon
    /// is cut from, with the dull gold such a stone is set in.
    #[default]
    Gem,
    /// The greys of black volcanic glass, and violet: the colours the app
    /// had before it had themes.
    Obsidian,
    /// A testing laboratory: white panels over charcoal, and the colours of
    /// a pair of portals, the blue one in the light scheme and the orange
    /// one in the dark, each with the other for its highlight.
    Aperture,
}

/// The themes there are, as a problem with the file names them.
const THEMES: &str = "gem, obsidian or aperture";

impl Palette {
    /// Every theme, in the order the dialog shows them.
    pub const ALL: [Palette; 3] = [Palette::Gem, Palette::Obsidian, Palette::Aperture];

    /// The word for it in the file.
    pub fn word(self) -> &'static str {
        match self {
            Palette::Gem => "gem",
            Palette::Obsidian => "obsidian",
            Palette::Aperture => "aperture",
        }
    }

    /// What the dialog calls it, and what it says of it.
    pub fn name(self) -> &'static str {
        match self {
            Palette::Gem => "Gem",
            Palette::Obsidian => "Obsidian",
            Palette::Aperture => "Aperture",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Palette::Gem => "Malachite green on the dark of the stone, with a little gold.",
            Palette::Obsidian => "Black glass and violet.",
            Palette::Aperture => "A laboratory's white panels and charcoal, with a portal of each colour: blue by day, orange by night.",
        }
    }

    /// The theme the file means by `word`, if it is one.
    fn named(word: &str) -> Option<Self> {
        match word.trim().to_lowercase().replace(['-', ' '], "_").as_str() {
            "gem" | "malachite" => Some(Palette::Gem),
            "obsidian" => Some(Palette::Obsidian),
            "aperture" | "aperture_science" => Some(Palette::Aperture),
            _ => None,
        }
    }

    /// Its colours, of the light scheme or of the dark one, in the order of `PAINTS`.
    pub fn colors(self, dark: bool) -> &'static [[u8; 4]; PAINTS.len()] {
        let schemes = match self {
            Palette::Gem => &GEM,
            Palette::Obsidian => &OBSIDIAN,
            Palette::Aperture => &APERTURE,
        };
        &schemes[usize::from(dark)]
    }
}

/// A colour by its six digits, as it is written everywhere else.
const fn rgb(hex: u32) -> [u8; 4] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255]
}

/// A colour that lets what is behind it through: `opacity` of 255 is none of it.
const fn clear(hex: u32, opacity: u8) -> [u8; 4] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, opacity]
}

/// The colours of a theme: its light scheme, then its dark one, each in the
/// order of `PAINTS`. A row reads: the accent; the three weights of text;
/// the page, the panels, the ground of code, the lines; the highlight; and
/// the nine colours of code and callouts, red to gray.
type Colors = [[[u8; 4]; PAINTS.len()]; 2];

/// The greens are the icon's, between its deepest facet and its brightest,
/// and the grounds are those greens taken nearly to black, or nearly to
/// white. Gold is what malachite is set in, and blue is azurite, the stone
/// it is found with.
static GEM: Colors = [
    [
        rgb(0x167a52),
        rgb(0x16241d), rgb(0x486054), rgb(0x8fa598),
        rgb(0xfbfdfc), rgb(0xeef5f1), rgb(0xe6f0ea), rgb(0xcbddd2),
        clear(0xcfa941, 97),
        rgb(0xd23a4c), rgb(0xc56a14), rgb(0xb08d06), rgb(0x1a4e27), rgb(0x0b9a94), rgb(0x2a63c8), rgb(0x7653d6), rgb(0xc5417f), rgb(0x86998e),
    ],
    [
        rgb(0x3aa96f),
        rgb(0xd8e5dd), rgb(0xa5b9ad), rgb(0x5d7769),
        rgb(0x101a16), rgb(0x16241e), rgb(0x1b2c25), rgb(0x2a4136),
        clear(0xad9449, 56),
        rgb(0xf2646e), rgb(0xe9a25a), rgb(0xe3d47a), rgb(0x8fd694), rgb(0x56d4cf), rgb(0x5aa2f0), rgb(0xb39df2), rgb(0xf29bc6), rgb(0x8ea398),
    ],
];

/// Plain greys, a shade apart, and one violet for everything that is marked out.
static OBSIDIAN: Colors = [
    [
        rgb(0x7852ee),
        rgb(0x222222), rgb(0x5c5c5c), rgb(0xa0a0a0),
        rgb(0xffffff), rgb(0xf6f6f6), rgb(0xf2f2f2), rgb(0xd2d2d2),
        clear(0xffd000, 56),
        rgb(0xe93147), rgb(0xec7500), rgb(0xe0ac00), rgb(0x08b94e), rgb(0x00bfbc), rgb(0x086ddd), rgb(0x7852ee), rgb(0xd53984), rgb(0x9e9e9e),
    ],
    [
        rgb(0xa882ff),
        rgb(0xdadada), rgb(0xb3b3b3), rgb(0x6e6e6e),
        rgb(0x1e1e1e), rgb(0x262626), rgb(0x2a2a2a), rgb(0x3c3c3c),
        clear(0xffd000, 56),
        rgb(0xfb464c), rgb(0xe9973f), rgb(0xe0de71), rgb(0x44cf6e), rgb(0x53dfdd), rgb(0x027aff), rgb(0xa882ff), rgb(0xfa99cd), rgb(0x9e9e9e),
    ],
];

/// The light scheme is the test chamber, the dark one what is behind its
/// panels. A portal's colour is the accent and the other portal's the
/// highlight, the other way about in the other scheme: orange cannot be read
/// on white, nor that blue on charcoal. The rest are things met there: the
/// red of a laser, the yellow of a warning stripe, the pink of a heart on a
/// cube.
static APERTURE: Colors = [
    [
        rgb(0x0872c4),
        rgb(0x22272b), rgb(0x56606a), rgb(0x9aa3ab),
        rgb(0xfcfcfb), rgb(0xeceeef), rgb(0xe4e7e9), rgb(0xc8cdd1),
        clear(0xff853f, 128),
        rgb(0xd63a2f), rgb(0xd96c00), rgb(0xb38b00), rgb(0x3f9a35), rgb(0x0a96a6), rgb(0x0872c4), rgb(0x7a56d3), rgb(0xd4467b), rgb(0x8a9299),
    ],
    [
        rgb(0xff9a1f),
        rgb(0xe3e6e8), rgb(0xa9b0b7), rgb(0x69727a),
        rgb(0x1a1c1e), rgb(0x222528), rgb(0x292d31), rgb(0x3a4046),
        clear(0x00a2ff, 72),
        rgb(0xff5c50), rgb(0xff9a1f), rgb(0xf2c94c), rgb(0x7ad468), rgb(0x4fd3de), rgb(0x27a7ff), rgb(0xb48df2), rgb(0xf48fb1), rgb(0x8b949c),
    ],
];

/// One of the colours of a scheme.
pub struct Paint {
    /// Its name in the file.
    pub key: &'static str,
    /// What the dialog calls it, and what it says it is for.
    pub name: &'static str,
    pub about: &'static str,
}

const fn paint(key: &'static str, name: &'static str, about: &'static str) -> Paint {
    Paint { key, name, about }
}

/// Every colour that can be set, in the order the dialog and the file list
/// them, and the order of a theme's rows. The constants below are their
/// places in this list.
pub const PAINTS: [Paint; 18] = [
    paint("accent", "Accent", "Links, tags, ticked boxes, the open note in the list, selected text"),
    paint("text", "Text", "The text of a note"),
    paint("muted", "Quiet text", "Code, properties, tasks that are done"),
    paint("faint", "Faint text", "Markdown syntax, bullets, comments"),
    paint("page", "Page", "Behind the note"),
    paint("sidebar", "Panels", "Behind the panels around the note"),
    paint("code_background", "Code background", "Behind code"),
    paint("rule", "Lines", "Rules, and the borders of tables and properties"),
    paint("highlight", "Highlight", "Behind ==highlighted== text; the last two digits are how strong"),
    paint("red", "Red", "In code: operators and tags. Callouts: failure, danger, bug"),
    paint("orange", "Orange", "In code: what is marked important. Callouts: question, warning"),
    paint("yellow", "Yellow", "In code: the names of functions"),
    paint("green", "Green", "In code: strings. Callouts: success"),
    paint("cyan", "Cyan", "In code: properties. Callouts: tip, abstract, important"),
    paint("blue", "Blue", "Callouts: note, info, todo, and any other kind"),
    paint("purple", "Purple", "In code: values. Callouts: example"),
    paint("pink", "Pink", "In code: keywords"),
    paint("gray", "Gray", "Callouts: quote"),
];

pub const ACCENT: usize = 0;
pub const TEXT: usize = 1;
pub const MUTED: usize = 2;
pub const FAINT: usize = 3;
pub const PAGE: usize = 4;
pub const SIDEBAR: usize = 5;
pub const CODE_BACKGROUND: usize = 6;
pub const RULE: usize = 7;
pub const HIGHLIGHT: usize = 8;
pub const RED: usize = 9;
pub const ORANGE: usize = 10;
pub const YELLOW: usize = 11;
pub const GREEN: usize = 12;
pub const CYAN: usize = 13;
pub const BLUE: usize = 14;
pub const PURPLE: usize = 15;
pub const PINK: usize = 16;
pub const GRAY: usize = 17;

/// The section of the file that holds the colours of the dark scheme, or of
/// the light one.
pub fn colors(dark: bool) -> &'static str {
    COLORS[usize::from(dark)]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    /// The theme, which is where every colour comes from that the file does
    /// not set itself.
    pub palette: Palette,
    pub scheme: Scheme,
    /// The size of a note's text, in points.
    pub text_size: f32,
    /// How wide a note's lines may get, in points, or 0 for no limit.
    pub line_width: f32,
    /// The colours the file sets, of the light scheme and of the dark one,
    /// in the order of `PAINTS`. They hold in whichever theme.
    set: [[Option<[u8; 4]>; PAINTS.len()]; 2],
}

impl Default for Appearance {
    fn default() -> Self {
        Self { palette: Palette::default(), scheme: Scheme::System, text_size: TEXT_SIZE, line_width: LINE_WIDTH, set: [[None; PAINTS.len()]; 2] }
    }
}

impl Appearance {
    /// What the settings file says about looks. With it comes whatever in
    /// the file could not be made sense of, a line to a problem, for the
    /// person whose file it is.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let (look, problems) = Self::read(text);
        (look, said(problems))
    }

    /// The same, with each problem by the number of the line it is on, and
    /// without a word about the sections that are other modules' to read.
    pub fn read(text: &str) -> (Self, Vec<(usize, String)>) {
        let mut look = Self::default();
        let (entries, stray) = conf::read(text);
        let mut problems: Vec<(usize, String)> = stray.into_iter().map(|line| (line, "is not a setting (`name = value`)".to_owned())).collect();
        for conf::Entry { line, section, key, value } in entries {
            // Both spellings of the word are understood.
            let section = section.replace("colours", "colors");
            let problem = match section.as_str() {
                SECTION => look.option(&key, &value).err(),
                _ => match COLORS.iter().position(|colors| *colors == section) {
                    Some(scheme) => match (PAINTS.iter().position(|paint| paint.key == key), color(&value)) {
                        (Some(paint), Some(color)) => {
                            look.set[scheme][paint] = Some(color);
                            None
                        }
                        (None, _) => Some(format!("there is no colour called `{key}`")),
                        (_, None) => Some(format!("`{value}` is not a colour: write it as #RRGGBB")),
                    },
                    None if section == files::SECTION => Some(format!("`{key}` is a vault's setting now: it is kept in each vault, in {}", files::VAULT_FILE)),
                    None if section.is_empty() => Some(format!("`{key}` is not under a [section]")),
                    None => Some(format!("there is no section [{section}]")),
                },
            };
            problems.extend(problem.map(|problem| (line, problem)));
        }
        (look, problems)
    }

    /// Take one setting of the `[appearance]` section, or say what is wrong with it.
    fn option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let number = |range: &RangeInclusive<f32>| match value.parse::<f32>() {
            Ok(number) if number.is_finite() => Ok(number.clamp(*range.start(), *range.end())),
            _ => Err(format!("`{value}` is not a number")),
        };
        match key {
            "theme" => match (Palette::named(value), Scheme::named(value)) {
                (Some(palette), _) => self.palette = palette,
                // Before there were themes, light or dark was said here: a
                // file that still says it so is understood (see `in_step`).
                (None, Some(scheme)) => self.scheme = scheme,
                (None, None) => return Err(format!("`{value}` is not a theme: it is {THEMES}")),
            },
            "scheme" => self.scheme = Scheme::named(value).ok_or_else(|| format!("`{value}` is not a scheme: it is system, dark or light"))?,
            "text_size" => self.text_size = number(&TEXT_SIZES)?,
            // Nothing, or less than nothing, is no limit.
            "line_width" => self.line_width = if value.parse::<f32>().is_ok_and(|width| width <= 0.0) { 0.0 } else { number(&LINE_WIDTHS)? },
            _ => return Err(format!("there is no setting called `{key}`")),
        }
        Ok(())
    }

    /// The colour in this place of `PAINTS`, in the dark scheme or the light one.
    pub fn paint(&self, dark: bool, index: usize) -> Color32 {
        let [r, g, b, a] = self.rgba(dark, index);
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    /// The same, as red, green, blue and opacity.
    pub fn rgba(&self, dark: bool, index: usize) -> [u8; 4] {
        self.set[usize::from(dark)][index].unwrap_or(self.themed(dark, index))
    }

    /// The colour the theme has in this place: what it is when the file does
    /// not set it.
    pub fn themed(&self, dark: bool, index: usize) -> [u8; 4] {
        self.palette.colors(dark)[index]
    }

    /// Whether the file sets this colour, to something other than the theme's.
    pub fn is_set(&self, dark: bool, index: usize) -> bool {
        self.set[usize::from(dark)][index].is_some_and(|color| color != self.themed(dark, index))
    }
}

/// Problems with the file as they are told: in the order of their lines, each
/// with its line's number.
pub fn said(mut problems: Vec<(usize, String)>) -> Vec<String> {
    problems.sort();
    problems.into_iter().map(|(line, problem)| format!("Line {line}: {problem}")).collect()
}

/// A colour as it is written in the file: `#RRGGBB`, or `#RRGGBBAA` for one
/// that lets what is behind it through. The `#` may be left off.
pub fn color(written: &str) -> Option<[u8; 4]> {
    let hex = written.trim().trim_start_matches('#');
    let pair = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    match hex.len() {
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

/// How a colour is written in the file.
pub fn hex([r, g, b, a]: [u8; 4]) -> String {
    match a {
        255 => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
    }
}

/// How a size or a width is written in the file: without a fraction it does not have.
pub fn number(value: f32) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// What the file says above the settings of the theme and of its scheme.
const THEME_ABOUT: &str = "# The theme, which is the colours of everything: gem, obsidian or aperture.";
const SCHEME_ABOUT: &str = "# Which of a theme's two schemes, light or dark. `system` goes by what the system is set to.";

/// What the file says above the colours of a scheme. The last of these lines
/// names the theme whose colours the commented lines under it show.
fn colors_about(dark: bool, shown: Palette) -> [String; 3] {
    let scheme = if dark { "dark" } else { "light" };
    [
        format!("# The colours of the {scheme} scheme, as #RRGGBB. One that is set here is used in"),
        "# place of the theme's own, whichever the theme.".to_owned(),
        shown_about(shown),
    ]
}

fn shown_about(shown: Palette) -> String {
    format!("# The lines that are commented out show the {} theme's.", shown.name())
}

/// The settings file as it is first written: every setting there is, each
/// under a line saying what it is, and each commented out at its default, so
/// that the file says nothing until a line of it is changed.
pub fn template() -> String {
    let theme = Palette::default();
    let mut text = String::from(
        "# Malachite's settings.
#
# This file is yours to edit. Malachite reads it when it starts and whenever
# its window is come back to, and the settings dialog (Ctrl+,) writes what is
# changed there into it.
#
# A line is `name = value`. A line that starts with `#` is a comment. Every
# setting is here, commented out, with the value it has when left alone:
# take the `#` off a line and change its value.

[appearance]
",
    );
    text.push_str(&format!("{THEME_ABOUT}\n# theme = {}\n{SCHEME_ABOUT}\n# scheme = system\n", theme.word()));
    text.push_str("# The size of a note's text, in points.\n");
    text.push_str(&format!("# text_size = {}\n", number(TEXT_SIZE)));
    text.push_str("# How wide a line of a note may get, in points. 0 is as wide as the window.\n");
    text.push_str(&format!("# line_width = {}\n", number(LINE_WIDTH)));
    for dark in [true, false] {
        text.push_str(&format!("\n{}\n[{}]\n", colors_about(dark, theme).join("\n"), colors(dark)));
        for (paint, color) in PAINTS.iter().zip(theme.colors(dark)) {
            text.push_str(&format!("# {}\n# {} = {}\n", paint.about, paint.key, hex(*color)));
        }
    }
    text
}

/// The file's text, brought in step with the app and with itself. Nothing it
/// says is changed by this, only how it is said, and what its comments show.
///
/// A file from before there were themes says light or dark under the name
/// `theme`; that is `scheme` now, and the lines the app wrote about it are
/// made to say so. And the lines that have a colour commented out show what
/// the colour is when left alone, which is the theme's, so they are made to
/// show the colours of the theme the file now names. A colour that has been
/// commented out by hand, at a value no theme has, is nobody's to rewrite.
pub fn in_step(text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_owned();
    }
    let text = renamed(text);
    let (look, _) = Appearance::parse(&text);
    let text = conf::recomment(&text, |section, key, value| {
        let dark = COLORS.iter().position(|colors| *colors == section.replace("colours", "colors"))? == 1;
        let index = PAINTS.iter().position(|paint| paint.key == key)?;
        let shown = color(value)?;
        let of_a_theme = Palette::ALL.iter().any(|theme| theme.colors(dark)[index] == shown);
        (of_a_theme && shown != look.themed(dark, index)).then(|| hex(look.themed(dark, index)))
    });
    let others = Palette::ALL.into_iter().filter(|theme| *theme != look.palette);
    others.fold(text, |text, other| text.replace(&shown_about(other), &shown_about(look.palette)))
}

/// `text` with what a file from before there were themes says of light and
/// dark, said as it is now. The colours such a file shows commented out are
/// those of the one look there was then.
fn renamed(text: &str) -> String {
    const WAS_ABOUT: &str = "# Light or dark. `system` goes by what the system is set to.";
    const WAS_COLORS: [&str; 2] = ["# The colours of the light scheme, as #RRGGBB.", "# The colours of the dark scheme, as #RRGGBB."];
    let line_break = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut section = String::new();
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let said = line.trim();
        if let Some(name) = said.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = name.trim().to_lowercase();
        }
        if said == WAS_ABOUT {
            lines.extend([THEME_ABOUT.to_owned(), format!("# theme = {}", Palette::default().word()), SCHEME_ABOUT.to_owned()]);
        } else if let Some(dark) = WAS_COLORS.iter().position(|was| *was == said) {
            lines.extend(colors_about(dark == 1, Palette::Obsidian));
        } else if let Some(scheme) = was_scheme(said).filter(|_| section == SECTION) {
            lines.push(scheme);
        } else {
            lines.push(line.to_owned());
        }
    }
    let mut text = lines.join(line_break);
    text.push_str(line_break);
    text
}

/// If `line` sets `theme` to light or dark, or has that commented out, the
/// same line with `scheme` for its name.
fn was_scheme(line: &str) -> Option<String> {
    let (mark, setting) = match line.strip_prefix('#') {
        Some(setting) => ("# ", setting.trim()),
        None => ("", line),
    };
    let (key, value) = setting.split_once('=')?;
    let scheme = Scheme::named(value).filter(|_| key.trim().eq_ignore_ascii_case("theme"))?;
    Some(format!("{mark}scheme = {}", scheme.word()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_says_nothing_is_the_defaults() {
        assert_eq!(Appearance::parse(""), (Appearance::default(), Vec::new()));
        assert_eq!(Appearance::parse(&template()), (Appearance::default(), Vec::new()), "the file as first written sets nothing");
        let look = Appearance::default();
        assert_eq!(look.palette, Palette::Gem);
        assert_eq!(look.paint(true, ACCENT), Color32::from_rgb(0x3a, 0xa9, 0x6f));
        assert_eq!(look.rgba(false, PAGE), [0xfb, 0xfd, 0xfc, 255]);
        assert!(!look.is_set(true, ACCENT));
    }

    #[test]
    fn the_file_sets_what_it_names() {
        let text = "[Appearance]\ntheme = Obsidian\nscheme = Light\ntext-size = 18\nline_width = 0\n[colours.dark]\naccent = #ff0000\npage = 101010\n[colors.light]\nhighlight = #00ff0080\n";
        let (look, problems) = Appearance::parse(text);
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!((look.palette, look.scheme, look.text_size, look.line_width), (Palette::Obsidian, Scheme::Light, 18.0, 0.0));
        assert_eq!(look.paint(true, ACCENT), Color32::from_rgb(255, 0, 0));
        assert_eq!(look.rgba(true, PAGE), [16, 16, 16, 255]);
        assert_eq!(look.rgba(false, HIGHLIGHT), [0, 255, 0, 128]);
        assert!(look.is_set(true, ACCENT) && !look.is_set(false, ACCENT), "each scheme has its own colours");
        assert_eq!(look.paint(false, ACCENT), Color32::from_rgb(120, 82, 238), "and what is not set is the theme's");
    }

    #[test]
    fn a_theme_is_every_colour_of_both_schemes_and_a_set_colour_outlasts_it() {
        for theme in Palette::ALL {
            assert_eq!(Palette::named(theme.word()), Some(theme));
            let (look, problems) = Appearance::parse(&format!("[appearance]\ntheme = {}\n[colors.dark]\naccent = #010203\n", theme.word()));
            assert!(problems.is_empty());
            assert_eq!(look.palette, theme);
            assert_eq!(look.rgba(true, ACCENT), [1, 2, 3, 255], "{}: the file's own colour holds in any theme", theme.name());
            assert_eq!(look.rgba(false, ACCENT), theme.colors(false)[ACCENT]);
            for dark in [false, true] {
                // The page is what the text is read on, in both schemes.
                let light = |index: usize| theme.colors(dark)[index][..3].iter().map(|part| u32::from(*part)).sum::<u32>();
                assert_eq!(light(PAGE) < light(TEXT), dark, "{} is dark where it says it is", theme.name());
                assert_eq!(light(PAGE) < light(SIDEBAR), dark, "{}: the panels are a shade off the page, towards the text", theme.name());
            }
        }
        assert_eq!(Palette::named("Aperture Science"), Some(Palette::Aperture));
        assert_eq!(Palette::named("sepia"), None);
    }

    #[test]
    fn what_cannot_be_understood_is_reported_and_passed_over() {
        let text = "stray = 1\n[appearance]\ntheme = sepia\ntext_size = big\nfont = Arial\ntext_size = 200\nnonsense\n[colors.dark]\naccent = purple\nbackground = #000000\n[editor]\ntabs = 4\n[appearance]\nscheme = dusk\n";
        let (look, problems) = Appearance::parse(text);
        assert_eq!(
            problems,
            [
                "Line 1: `stray` is not under a [section]",
                "Line 3: `sepia` is not a theme: it is gem, obsidian or aperture",
                "Line 4: `big` is not a number",
                "Line 5: there is no setting called `font`",
                "Line 7: is not a setting (`name = value`)",
                "Line 9: `purple` is not a colour: write it as #RRGGBB",
                "Line 10: there is no colour called `background`",
                "Line 12: there is no section [editor]",
                "Line 14: `dusk` is not a scheme: it is system, dark or light",
            ]
        );
        assert_eq!(look, Appearance { text_size: 40.0, ..Appearance::default() }, "a size out of range is brought into it");
    }

    #[test]
    fn values_are_written_as_they_are_read() {
        for written in ["#a882ff", "#ffd00038"] {
            assert_eq!(hex(color(written).unwrap()), written);
        }
        assert_eq!(color("A882FF"), Some([168, 130, 255, 255]));
        assert_eq!((color("#fff"), color("#gggggg"), color("")), (None, None, None));
        assert_eq!((number(16.0), number(17.5), number(699.96)), ("16".to_owned(), "17.5".to_owned(), "700".to_owned()));
    }

    #[test]
    fn a_setting_changed_in_the_file_as_first_written_is_read_back() {
        let text = conf::set(&template(), SECTION, "text_size", "20");
        let text = conf::set(&text, colors(true), "accent", "#00ff00");
        let (look, problems) = Appearance::parse(&text);
        assert!(problems.is_empty());
        assert_eq!(look.text_size, 20.0);
        assert_eq!(look.paint(true, ACCENT), Color32::from_rgb(0, 255, 0));
        // Each went onto the line that had it commented out, under its description.
        assert_eq!(text.lines().count(), template().lines().count());
        assert!(text.contains("# The size of a note's text, in points.\ntext_size = 20\n"));
        // And back to its default, the file is as it was.
        let back = conf::unset(&conf::unset(&text, SECTION, "text_size", "16"), colors(true), "accent", "#3aa96f");
        assert_eq!(back, template());
        assert_eq!(in_step(&template()), template(), "which is a file in step with itself");
    }

    #[test]
    fn the_commented_colours_of_the_file_follow_its_theme() {
        let text = conf::set(&template(), SECTION, "theme", "aperture");
        let text = conf::set(&text, colors(true), "page", "#000000");
        // One the person has commented out themselves, at a value of their own.
        let text = text.replace("# rule = #2a4136", "# rule = #123456");
        let stepped = in_step(&text);
        assert!(stepped.contains("# accent = #ff9a1f") && stepped.contains("# accent = #0872c4"), "both schemes show the new theme's");
        assert!(!stepped.contains("#3aa96f"));
        assert!(stepped.contains("\npage = #000000\n"), "what is set stays set");
        assert!(stepped.contains("# rule = #123456"), "and what is nobody's is left alone");
        assert_eq!(stepped.matches("show the Aperture theme's.").count(), 2);
        assert_eq!(stepped.lines().count(), text.lines().count());
        assert_eq!(Appearance::parse(&stepped), Appearance::parse(&text), "it says what it said");
        assert_eq!(in_step(&stepped), stepped);
        // Back to the first theme, the lines are the first file's again.
        let back = conf::unset(&in_step(&conf::unset(&stepped, SECTION, "theme", "gem")), colors(true), "page", "#101a16");
        assert_eq!(back, template().replace("# rule = #2a4136", "# rule = #123456"));
    }

    #[test]
    fn a_file_from_before_there_were_themes_is_brought_up_to_date() {
        let old = "# Mine.\n[appearance]\n# Light or dark. `system` goes by what the system is set to.\ntheme = dark\n# The size of a note's text, in points.\n# text_size = 16\n\n# The colours of the dark scheme, as #RRGGBB.\n[colors.dark]\n# Behind the note\n# page = #1e1e1e\n# Links\naccent = #3aa96f\n";
        let (before, problems) = Appearance::parse(old);
        assert!(problems.is_empty(), "it is still understood as it stands");
        assert_eq!((before.palette, before.scheme), (Palette::Gem, Scheme::Dark));
        let new = in_step(old);
        assert_eq!(
            new,
            "# Mine.\n[appearance]\n# The theme, which is the colours of everything: gem, obsidian or aperture.\n# theme = gem\n# Which of a theme's two schemes, light or dark. `system` goes by what the system is set to.\nscheme = dark\n# The size of a note's text, in points.\n# text_size = 16\n\n# The colours of the dark scheme, as #RRGGBB. One that is set here is used in\n# place of the theme's own, whichever the theme.\n# The lines that are commented out show the Gem theme's.\n[colors.dark]\n# Behind the note\n# page = #101a16\n# Links\naccent = #3aa96f\n"
        );
        assert_eq!(Appearance::parse(&new), (before, Vec::new()));
        assert_eq!(in_step(&new), new, "once is enough");
        // The line as the app first wrote it, never changed.
        assert!(in_step("[appearance]\n# theme = system\n").contains("# scheme = system"));
        assert_eq!(in_step("[appearance]\ntheme = aperture\n"), "[appearance]\ntheme = aperture\n", "a theme is a theme");
        assert_eq!(in_step("[a]\r\nkey = 1\r\n"), "[a]\r\nkey = 1\r\n");
        assert_eq!(in_step(""), "");
    }
}
