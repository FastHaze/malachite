# Changelog

What changed in each version of Malachite. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions
follow [semantic versioning](https://semver.org): before 1.0, a new minor
version may change behaviour or the settings files, and a patch version only
fixes things.

## [Unreleased]

### Added
- The Cyberpunk theme: neon yellow and deep teal with a green laser, yellow
  on the dark of a city at night, and teal on a pale yellow sky by day.

## [0.3.0] - 2026-10-29

### Added
- Drawing over a note in Reading view: the Draw button beside the views in
  the bar above the note turns it on, and the tools take a row of their own
  between that bar and the note: a pen, a highlighter, lines and arrows
  (straight, or curved and bent by a handle), rectangles, ellipses, text,
  numbered badges, an eraser, a laser pointer whose trail fades away, a
  selection to move, recolour, delete or bring to front what is drawn,
  colours and one of your own, picked as a highlight's is, a slider for
  the size, and undo and redo, with a key for each (P, H, L, A, R, O, T, N,
  E, V, 1–8, [ and ], C to clear). The note is dimmed while drawing. A
  mark stays with the line under it, ticked or not, and folds away with it.
  Drawings are kept in `.malachite/ink` in the vault, never in the note, and
  follow the note when it is renamed, moved or deleted.

### Changed
- Long notes take less work to show: Reading view no longer reads the note
  again at every move of the mouse, and Live Preview and the find bar no
  longer work on matches and bullets out of sight.
- Find (Ctrl+F) and replace work in Reading view, marking the matches there and
  going from one to the next, instead of turning to the editing view.
- Choosing a note in the graph also opens it, behind the graph, so it is the
  note edited when the graph is put away, not the one open before.
- The vault's name in the status bar is the colour of the bar's other
  buttons (zoom and settings), not the dimmer colour of the note count.

## [0.2.0] - 2026-10-17

### Added
- A tool above the note list that collapses every folder, and expands them
  all again; "Collapse all folders" and "Expand all folders" in the command
  palette.
- "Collapse others" in a folder's right-click menu.
- Each folder in the note list shows how many notes are in it.
- Folders are moved by dragging them, as notes are.
- Folding in Reading view: headings and lists fold there as in the editor,
  with the same arrows, and what is folded in one view is folded in the
  others.
- Tables are drawn as grids in Live Preview, as in Reading view; putting the
  cursor in one shows it as typed.
- Pictures and other files that notes can embed are listed in the note list,
  in their folders; a click opens one in the system's program for it.
- A setting to hide the note's name above it (Appearance, "Note title").

### Changed
- Folded folders are remembered for each vault between runs.
- The vault's name in the status bar shows that it is a button: it lights up
  under the mouse and looks pressed while held down, and has a picture of a
  vault in front of it.
- The new-folder tool above the note list makes the folder at the top of the
  vault, whatever note is open.

### Fixed
- Ctrl+N puts the new note where the settings say, as the new-note tool does,
  not always beside the open note.
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

[Unreleased]: https://github.com/FastHaze/malachite/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/FastHaze/malachite/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/FastHaze/malachite/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/FastHaze/malachite/releases/tag/v0.1.0
