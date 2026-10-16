# Changelog

What changed in each version of Malachite. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions
follow [semantic versioning](https://semver.org): before 1.0, a new minor
version may change behaviour or the settings files, and a patch version only
fixes things.

## [Unreleased]

### Added
- A tool above the note list that collapses every folder, and expands them
  all again; "Collapse all folders" and "Expand all folders" in the command
  palette.
- "Collapse others" in a folder's right-click menu.
- Each folder in the note list shows how many notes are in it.
- Folders are moved by dragging them, as notes are.

### Changed
- Folded folders are remembered for each vault between runs.
- The vault's name in the status bar shows that it is a button: it lights up
  under the mouse and looks pressed while held down.

### Fixed
- Renaming or moving a folder that the vault's settings name, or one with
  such a folder in it, changes those settings to where it is now: the
  templates folder, the daily notes' folder and template, and the folder for
  new notes no longer point to where it was.

## [0.1.0] - 2026-10-15

The first public release.

### Notes and vaults
- A vault is a folder of Markdown files; notes are found anywhere in it by
  their names, and `aliases` in a note's properties give it more.
- Create, rename, move and delete notes and folders. Renaming updates the
  `[[links]]` to a note. Deleted notes go to the vault's `.trash`, the system's
  bin, or are deleted for good, as the vault's settings say.
- Daily notes, named in a date format of your choice, and templates with
  `{{title}}`, `{{date}}` and `{{time}}`.
- Open other vaults, and switch between the ones used before.

### Editing and reading
- Live Preview, Source mode and Reading view.
- Wikilinks to notes, headings and blocks, embedded pictures and notes,
  footnotes, callouts, tables, tasks, tags, properties, comments, and
  highlights in any colour.
- Code blocks highlighted in over twenty languages.
- Folding of headings and lists.
- Typing aids: lists continue on Enter, brackets and markers pair up,
  Ctrl+Backspace deletes by words in any script.
- Find and replace in a note, and search across the vault with operators.
- Command palette, quick switcher, back and forward.
- Outline, links, backlinks and tags in the side panel.

### Graph
- A 3D graph of the vault's links, with a camera to turn and zoom, names that
  give way to one another, and fog over the far side.
- Clicking a note shows only it and the notes it is linked with, gathered
  round it.
- Ctrl+F finds notes by name and shows them with their neighbours; Enter goes
  from one to the next.

### Look and settings
- Themes Gem, Obsidian and Aperture, each light and dark, and every colour
  open to change.
- A settings dialog with Appearance and Files and links pages, writing plain
  files that can be edited by hand as well.
- A window without the system's title bar, moved, resized and snapped as any
  other.

### Builds
- Windows, macOS (Apple silicon and Intel) and Linux.
- `malachite-cli` for the terminal.

[Unreleased]: https://github.com/FastHaze/malachite/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/FastHaze/malachite/releases/tag/v0.1.0
