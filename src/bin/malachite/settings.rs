//! What the app remembers between runs: the zoom level, which side panels
//! are showing, the editing mode, and the vaults that have been open. A few
//! `key = value` lines in a text file in the user's settings folder; a
//! missing or damaged file just means the defaults.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// How many vaults are remembered for going back to.
pub const RECENT_VAULTS: usize = 10;

/// How far the window can be zoomed out and in.
pub const ZOOM_RANGE: RangeInclusive<f32> = 0.5..=2.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Scale of everything in the window; 1.0 is 100%.
    pub zoom: f32,
    pub show_list: bool,
    pub show_info: bool,
    /// Edit in Source mode rather than Live Preview.
    pub source_mode: bool,
    /// The vault that was open last, and the note that was open in it (its
    /// file, relative to the vault, with `/` between folders), so that the
    /// app starts where it was left. Both are empty when there is nothing to
    /// go back to.
    pub vault: String,
    pub note: String,
    /// The vaults that have been open, the latest first: one `recent` line each.
    pub recent: Vec<String>,
    /// The colour last picked for a highlight of one's own, as red, green
    /// and blue; written as six hex digits.
    pub highlight: [u8; 3],
}

/// The custom highlight colour before any has been picked: a pink.
pub const HIGHLIGHT: [u8; 3] = [213, 57, 132];

/// A colour from its six hex digits.
fn color(hex: &str) -> Option<[u8; 3]> {
    let pair = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    (hex.len() == 6).then_some(())?;
    Some([pair(0)?, pair(2)?, pair(4)?])
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            show_list: true,
            show_info: true,
            source_mode: false,
            vault: String::new(),
            note: String::new(),
            recent: Vec::new(),
            highlight: HIGHLIGHT,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "zoom" => settings.zoom = value.parse().map_or(settings.zoom, clamp_zoom),
                "show_list" => settings.show_list = value.parse().unwrap_or(settings.show_list),
                "show_info" => settings.show_info = value.parse().unwrap_or(settings.show_info),
                "source_mode" => settings.source_mode = value.parse().unwrap_or(settings.source_mode),
                "vault" => settings.vault = value.to_owned(),
                "note" => settings.note = value.to_owned(),
                "highlight" => settings.highlight = color(value).unwrap_or(settings.highlight),
                "recent" if !value.is_empty() && settings.recent.len() < RECENT_VAULTS => settings.recent.push(value.to_owned()),
                _ => {}
            }
        }
        settings
    }

    pub fn write(&self) -> String {
        let mut text = format!(
            "zoom = {:.2}\nshow_list = {}\nshow_info = {}\nsource_mode = {}\nvault = {}\nnote = {}\n",
            self.zoom, self.show_list, self.show_info, self.source_mode, self.vault, self.note
        );
        let [r, g, b] = self.highlight;
        text.push_str(&format!("highlight = {r:02x}{g:02x}{b:02x}\n"));
        for vault in &self.recent {
            text.push_str(&format!("recent = {vault}\n"));
        }
        text
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|text| Self::parse(&text)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, self.write())
    }
}

/// A zoom level the window can actually have.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end())
    } else {
        1.0
    }
}

/// Where the settings file is: `malachite\settings.txt` in the user's
/// application-data folder (`%APPDATA%` on Windows, `~/.config` elsewhere).
/// The `MALACHITE_SETTINGS` environment variable names another file to use.
pub fn path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MALACHITE_SETTINGS") {
        return Some(path.into());
    }
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let folder = var("APPDATA").or_else(|| var("XDG_CONFIG_HOME")).or_else(|| Some(var("HOME")?.join(".config")))?;
    Some(folder.join("malachite").join("settings.txt"))
}

/// The folders the app kept its settings in under the names it has had
/// before, the latest first, and what its settings file was called in each.
const FORMER: [(&str, Option<&str>); 2] = [("smaragd", Some("smaragd.conf")), ("notes-gui", None)];

/// Start the settings folder off with what the app had under an earlier
/// name, the first time it runs under this one: `settings` is where its
/// settings are to be, and nothing is done if that folder is already there.
/// The earlier folder is left as it is.
///
/// The app does this itself, on starting, because only the app is sure to
/// be looking at the same folders as the app: a program started from inside
/// a packaged one can be shown a private copy of the user's settings folder.
pub fn inherit(settings: &Path) {
    let Some((folder, home)) = settings.parent().and_then(|folder| Some((folder, folder.parent()?))) else { return };
    if folder.exists() {
        return;
    }
    let Some((former, conf)) = FORMER.iter().map(|(name, conf)| (home.join(name), conf)).find(|(former, _)| former.join("settings.txt").is_file()) else { return };
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let _ = std::fs::copy(former.join("settings.txt"), settings);
    if let Some(text) = conf.and_then(|conf| std::fs::read_to_string(former.join(conf)).ok()) {
        // Its own name is in the comment the file opens with.
        let _ = std::fs::write(folder.join(crate::appearance::FILE), text.replace("Smaragd", "Malachite"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_of_an_earlier_name_are_taken_over_once() {
        let home = std::env::temp_dir().join(format!("malachite-settings-{}-inherit", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let settings = home.join("malachite").join("settings.txt");
        inherit(&settings);
        assert!(!home.exists(), "with nothing to take over, nothing is made");

        // The latest of the earlier names is the one that counts.
        for (name, zoom) in [("notes-gui", "0.80"), ("smaragd", "1.30")] {
            std::fs::create_dir_all(home.join(name)).unwrap();
            std::fs::write(home.join(name).join("settings.txt"), format!("zoom = {zoom}
")).unwrap();
        }
        std::fs::write(home.join("smaragd").join("smaragd.conf"), "# Smaragd's settings.
[appearance]
theme = light
").unwrap();
        inherit(&settings);
        assert_eq!(Settings::load(&settings).zoom, 1.3);
        let conf = std::fs::read_to_string(settings.with_file_name(crate::appearance::FILE)).unwrap();
        assert_eq!(conf, "# Malachite's settings.
[appearance]
theme = light
");
        assert!(home.join("smaragd").join("settings.txt").is_file(), "what was there is left there");

        // Once there are settings under this name, the earlier ones are of no account.
        std::fs::write(home.join("smaragd").join("settings.txt"), "zoom = 2.00
").unwrap();
        inherit(&settings);
        assert_eq!(Settings::load(&settings).zoom, 1.3);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn settings_survive_being_written_and_read_back() {
        let settings = Settings {
            zoom: 1.3,
            show_list: false,
            show_info: true,
            source_mode: true,
            vault: r"C:\Users\me\My notes = here".into(),
            note: "Projects/2026/Plan = draft.md".into(),
            recent: vec![r"C:\Users\me\My notes = here".into(), r"D:\Work".into()],
            highlight: [1, 171, 255],
        };
        assert_eq!(Settings::parse(&settings.write()), settings);
        assert_eq!(Settings::parse(&Settings::default().write()), Settings::default());
    }

    #[test]
    fn a_damaged_file_falls_back_to_defaults_line_by_line() {
        let settings = Settings::parse("zoom = huge\nshow_list=false\nnonsense\nunknown = 1\n zoom = 9 ");
        assert_eq!(settings, Settings { zoom: 2.5, show_list: false, ..Settings::default() });
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("zoom = NaN").zoom, 1.0);
        assert_eq!(Settings::parse("zoom = 0.1").zoom, 0.5);
        assert_eq!(Settings::parse("highlight = red\nhighlight = 12345").highlight, HIGHLIGHT);
    }

    #[test]
    fn only_so_many_vaults_are_remembered() {
        let lines: String = (0..RECENT_VAULTS + 5).map(|n| format!("recent = vault {n}\n")).collect();
        let recent = Settings::parse(&format!("recent =\n{lines}")).recent;
        assert_eq!(recent.len(), RECENT_VAULTS);
        assert_eq!(recent[0], "vault 0", "the first lines are the latest vaults");
    }

    #[test]
    fn saving_makes_the_folder_it_needs() {
        let folder = std::env::temp_dir().join(format!("malachite-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join("deeper").join("settings.txt");
        assert_eq!(Settings::load(&path), Settings::default(), "no file yet");
        let settings = Settings { zoom: 0.8, ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
