---
tags: [overview, help]
---
# Overview

Everything Malachite can show, next to the syntax that produces it.
Switch views with the buttons at the top right, or Ctrl+E, to compare:
- **Live** — formatted while you type; syntax appears only around the cursor
- **Source** — formatted, with all syntax visible
- **Read** — the finished note

The block between the `---` lines at the very top is the note's properties.

## Text

**Bold** — `**Bold**` or `__Bold__`
*Italic* — `*Italic*` or `_Italic_`
***Bold italic*** — `***Bold italic***`
**Bold with *italic* inside** — `**Bold with *italic* inside**`
~~Strikethrough~~ — `~~Strikethrough~~`
==Highlight== — `==Highlight==`
<mark style="background: #08B94E66;">Highlight in a colour</mark> — `<mark style="background: #08B94E66;">Highlight in a colour</mark>`; right-click in the note to pick the colour instead of typing this
`Inline code` — `` `Inline code` ``
\*Not italic\* — `\*Not italic\*` (a backslash switches a symbol off)
A comment %%like this one%% — `%%like this one%%` (visible while editing, gone in Read)

## Headings

`# Heading 1` down to `###### Heading 6`: one `#` per level, then a space.

### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

## Links and tags

[[2026-10-12 - Markdown]] — `[[2026-10-12 - Markdown]]` opens that note
[[2026-10-12 - Markdown|Shown under another name]] — `[[2026-10-12 - Markdown|Shown under another name]]`
[[A note that does not exist yet]] — fainter; clicking it creates the note
[[Overview#Tables]] — `[[Overview#Tables]]` opens the note at that heading; `[[#Tables]]` is a heading of this note, and `[[Overview#Code#Tables]]` would be a heading under another
[[Overview#^tour]] — `[[Overview#^tour]]` goes to the block that ends in `^tour`, like this line ^tour
A note also answers to the names listed as `aliases: [Tour, Help]` in its properties: `[[Tour]]` would open it, and the quick switcher finds it by them
[The Rust website](https://www.rust-lang.org) — `[The Rust website](https://www.rust-lang.org)`
https://www.rust-lang.org — a bare address becomes a link too
#overview #help/markdown — a `#` and a word; clicking a tag filters the note list

## Embeds

`![[hills.png]]` on a line of its own shows the picture there; `![[hills.png|240]]` at that width; `![alt](Attachments/hills.png)` is the Markdown way to write it. Pictures are PNG, JPEG, GIF or BMP files anywhere in the vault.

![[hills.png|240]]

`![[Note]]` shows another note inside this one, `![[Note#Heading]]` what is under one of its headings, `![[Note#^id]]` one block. A click on a link inside goes there.

![[Meeting]]

More to try in [[Embeds and Footnotes]].

## Footnotes

A mark in the text[^1] and, anywhere in the note, a line that starts `[^1]: ` with what it says. In Read the marks become numbers and the footnotes are listed at the end; while editing, a click on a mark goes to its footnote.

[^1]: Like this one.

## Lists

- Bullet — `- Bullet` (or `*` or `+`)
  - Indent with two spaces or a tab to nest
    - And again
1. Numbered — `1. Numbered`
2. Second

## Tasks

- [ ] To do — `- [ ] To do`
- [x] Done — `- [x] Done`

Click a box to tick it, in Live or Read.

## Quotes

> A quote — `> A quote`
> It can run over several lines.

## Callouts

A quote whose first line is `> [!type] Title`. The type sets the colour.

> [!note] Note
> Also `info` and `todo`.

> [!tip] Tip
> Also `hint`, `important`, `abstract` and `summary`.

> [!success] Success
> Also `check` and `done`.

> [!warning] Warning
> Also `caution`, `attention`, `question`, `help` and `faq`.

> [!danger] Danger
> Also `error`, `bug`, `failure`, `fail` and `missing`.

> [!example] Example
> Purple.

> [!quote] Quote
> Also `cite`.

> [!tip]
> Without a title, the type is shown instead.

## Horizontal rule

Three dashes on a line of their own — `---` (also `***` or `___`):

---

## Code

Wrap a block in lines of three backticks. Name a language after the first three and the code is coloured by what each piece is.

```rust
// Comments are faint, keywords pink, strings green, numbers purple
fn main() {
    let total: u32 = [1, 2, 3].iter().sum();
    println!("**not bold** in here: {total}");
}
```

Typed like this:

~~~
```rust
fn main() {}
```
~~~

The same colours mean the same things in every language:

```python
def greet(name):
    """Say hello."""
    return f"Hello, {name}!" if name else None
```

```json
{ "name": "notes", "tags": ["a", "b"], "count": 3, "done": true }
```

Without a language, or with one the app does not know, the block stays plain:

```
fn main() { "nothing is coloured here" }
```

Known languages: Rust, Python, JavaScript, TypeScript, JSON, C, C++, C#, Java, Kotlin, Go, Swift, PHP, Ruby, Lua, Bash, PowerShell, SQL, HTML and XML, CSS, YAML, TOML and diff. Short names such as `rs`, `py`, `js`, `ts`, `sh` and `yml` work too.

## Tables

| View   | Editable   | Syntax visible    |
|--------|------------|-------------------|
| Live   | yes        | around the cursor |
| Source | yes        | **always**        |
| Read   | tasks only | never             |

Tables are drawn as a grid in Live and Read. In Live, put the cursor in one (a click on it does) and it is shown as typed, to edit.

## Around the note

- The four pictures at the top left are the tools of the list: a page with a plus for a new note, a folder with a plus for a new folder, a calendar for today's note, and two chevrons that fold every folder away
- The page with a plus, or Ctrl+N — a new note with its name selected: type a name, press Enter, start writing. It starts beside the open note, unless the settings say the top of the vault or a folder of your choosing. The folder with a plus makes a folder at the top of the vault, named the same way; a folder's right-click menu makes one inside it
- Ctrl+P — the command palette: type a few letters of what you want done, press Enter. Everything with a key below is in it, and so is much that has none
- Ctrl+O — the quick switcher: type part of a note's name and press Enter to open it. A name that matches nothing makes a new note
- In the list on the left every row starts with a picture of what it is: a folder (filled in while it is open), a page for a note, a calendar for a daily note, a blank page for a picture or other file. Click a file to open it in the program your system has for it
- Click a folder in the list to fold it away, and again to open it. The chevrons at the top left fold every folder at once, the folders inside others too; once the list shows only folded folders they point apart, and open them all again. "Collapse all folders" and "Expand all folders" in the command palette do the same, and "Collapse others" in a folder's right-click menu folds every folder but that one and those it is in. Each vault remembers which of its folders are folded, from one run of the app to the next
- The number at the right of a folder is how many notes are in it, counting the folders inside it
- Drag a note or a folder onto a folder to move it there; onto a note, to go beside it; below the list, to the top level. Links are by name, so they all still work. A note's right-click menu also has "Move to"
- The arrows at the top left, or Ctrl+Alt+Left and Ctrl+Alt+Right — back to the note you were in before, and forward again
- The calendar at the top left — today's daily note, ready to type in. "Open previous daily note" and "Open next daily note" in the command palette go to the daily notes either side of the one that is open. The settings choose the folder a new one goes in, how its name is written (`YYYY-MM-DD` to begin with) and a template it starts as
- The name at the top of the note renames it and updates the links that point to it; F2 jumps to it, Esc backs out. The settings can have it ask first, or leave the links as they are, and can hide the name; F2 still brings it back to rename
- The search box searches every note. Ctrl+Shift+F jumps to it, Enter opens the first match, Esc clears it. Under each note found are the lines that matched; click one to go to it
- Ctrl+F — find in this note, in any of its views, Read too: every match is marked, Enter goes to the next and Shift+Enter back, Esc closes. Ctrl+H adds a row to replace one match or all of them
- The panel on the right starts with this note's outline: its headings, nested by level. Click one to go to it, in any view. Below it are the note's links, backlinks and tags
- Point at a heading, or at a list item with items nested in it, and an arrow appears in the margin: click it to fold what is under the line away, and again to bring it back. This works in all three views, and what is folded in one is folded in the others. A folded line ends in "…", and opens by itself when the cursor moves into it or a search, the outline or a link leads there. The command palette has "Toggle fold on the current line", "Fold all headings and lists" and "Unfold all headings and lists". Folds last until the app is closed
- "Insert template" in the command palette — puts the text of a note from the `Templates` folder (another, if the settings say so) in at the cursor. In a template `{{title}}` becomes the note's name, `{{date}}` and `{{time}}` today's date and the time; `{{date:dddd, D MMMM YYYY}}` and `{{time:HH:mm}}` choose how they are written. [[Meeting]] is one to try
- The bar at the bottom counts the note's words and characters, or those of the selection
- The button at the left end of the bar above the note hides and shows the list, and the one at the top right, beside the window's own buttons, the panel on the right
- The window has no title bar. Its minimise, maximise and close buttons are at the top right corner; drag the window by any empty part of the bars along its top, double click there to fill the screen and again to go back, and drag an edge or a corner to resize it
- Notes save by themselves; Ctrl+S saves at once. The dot at the bottom right is hollow while a change is still to be saved
- **Delete** moves the note to the vault's `.trash` folder, or, as the settings say, to the Recycle Bin or nowhere at all
- Notes changed outside the app are picked up when you come back to its window
- Draw, beside Read in the bar above the note — draw over the note. It turns to Read and lays the tools out in a row between the bar and the note, which wraps onto a second row in a narrow window: a pen, a highlighter, a line and an arrow (hold Shift for steps of 45°), a curved arrow (drag it out straight, let go, then move the mouse to bend it, the middle of the curve following the mouse, and click to keep it there; Esc keeps it as it is; a line or an arrow is bent the same way if Alt is held as you let go), a rectangle and an ellipse (Shift for a square or a circle), text (click where it goes, type, and Esc or a click elsewhere to finish; click it again with the text tool to change it), numbered badges (each click puts the next number), an eraser that takes away whole marks, a laser pointer to point with (a red dot whose trail fades away and is never kept), and Select: click a mark or drag round some, then drag them to move them, pick a colour to recolour them, press Delete to take them away or Ctrl+] to bring them to the front, and double-click a text to change it. A line or an arrow selected on its own shows three handles: drag an end to move it, or the middle one to bend it (or bend it more, or the other way); double-click the middle one to make it straight again. Then the colours, and after them your own colour (click it to draw in it and to pick another, in the same picker as a highlight's, which offers it too), a slider for the size with the size you are drawing at beside it, and undo and redo (Ctrl+Z and Ctrl+Shift+Z while drawing). While drawing, keys choose the tools: P pen (hold Alt with it for a highlighter), H highlighter, L line, A arrow, R rectangle, O ellipse, T text, N badge, E eraser, V select; 1 to 8 the colours (recolouring what is selected), [ and ] step the size down or up to the next of three, C clears everything, Ctrl+A selects everything. They are letters again while a text or any other field is being written in. While drawing, a press on the note draws instead of following a link, and the wheel still scrolls. Press Esc, the button again, or turn to another view to stop. A mark belongs to the line under its middle, where it is drawn or moved to: it moves with that line when you write above it, stays when a task on it is ticked, hides while a fold hides the line, and comes back if the line does. Drawings are only shown in Read. They are kept beside the note, in `.malachite/ink` inside the vault, so the note's own text is left as it was, and they go with the note when it is renamed, moved or deleted
- **Graph** at the top right, or Ctrl+G — the whole vault as a ball of notes joined by their links, each bigger the more notes it is linked with, whichever way the links go. Drag to turn it, scroll to zoom, point at a note to see what it is linked to. Click a note to go in to it: only that note and the notes it is linked with are left, gathered round it, and the panel on the right then shows that note's outline, links, backlinks and tags, under its name, and they can be clicked as usual. That note is also opened behind the graph, so it is the one you write in when you put the graph away. Click the note again (or press Enter) to show it in place of the graph. Esc steps back out to the whole vault. Ctrl+F there finds a note by its name: type part of it and the graph shows only the notes called so (the first fifteen) and the notes linked with them, gathered together in the middle with the links between them. Enter goes to the first of the notes called so, then on to the next, and Shift+Enter back. Esc puts the bar away and brings every note back, with the graph still on the note it went to; a click on an empty part of the graph lets go of both
- The vault's name at the bottom left, beside a picture of a strongbox, or "Open another vault" in the command palette — switch to a vault you had open before, type the full path of a folder to open or create one there, or pick "Choose a folder…" to browse for it. The app starts in the vault it was last in
- Ctrl with + or - zooms the whole window, as does Ctrl with the scroll wheel; Ctrl+0 is 100%. The zoom is shown at the bottom right and is remembered
- Ctrl+, or the gear at the bottom right — settings, on two pages. **Appearance**: the theme, light or dark, the size of a note's text, how wide its lines may get, whether the note's name is shown above it, and every colour of both schemes. **Files and links**: where new notes start, whether renaming updates links, where deleted notes go, the templates folder, and the daily note's folder, name and template
- There are four themes, each with a light scheme and a dark one: **Gem**, the greens of malachite with a little gold; **Obsidian**, black glass and violet; **Aperture**, a laboratory's white panels and charcoal, with blue for the light scheme and orange for the dark; and **Cyberpunk**, neon yellow and deep teal with a green laser, teal on a pale yellow sky for the light scheme and yellow on the dark of the city for the dark. Pick one by its picture in the settings. A colour you have set yourself stays as you set it in every theme, until you put it back
- The settings are lines of text files, and **Open the file** in the settings shows the one of the page you are on. How the app looks is in `malachite.conf`, the same in every vault. **Files and links** are this vault's own, in `.malachite/malachite.conf` inside it, so each vault keeps its own and takes them with it wherever it is copied. Each line is `name = value`, with a comment above it saying what it is for (`theme = aperture`, say); change one in any text editor, and the app goes by it when you come back to its window

## Searching

Words in the search box are all looked for, in names and in text, capitals aside.

- `"buy milk"` — the phrase as written
- `milk OR bread` — either; `-milk` — notes without it; `( )` — groups
- `#overview` or `tag:overview` — notes with the tag, or one nested under it
- `file:plan` — in the note's name; `path:projects` — in its folders too; `content:plan` — in its text only
- `line:(buy milk)` — both on one line; `block:( )` — in one paragraph; `section:( )` — under one heading
- `task:call` — in a task; `task-todo:call` — in one still to do; `task-done:call` — in one that is done
- `match-case:Milk` — capitals as written

## Keys while writing

- Ctrl+B and Ctrl+I — bold and italic, for the selection or from the cursor on
- Ctrl+K — a link: the selection becomes its text, and the cursor waits where the address goes
- Ctrl+L — tick or untick a task; on any other line, make a task of it
- A right click in the note — highlight the selection: a row of colours to click, the colour you picked last, "Pick a colour…" for any other, and "Remove highlight". Inside a highlight the same menu changes its colour. Yellow is written `==like this==`; the other colours as a `<mark>` tag, which other Markdown apps show in colour too
- Ctrl+/ — turn the selection into a comment, or back
- Ctrl+D — delete the line
- `(`, `[`, `{`, `"` and a backtick bring their closing partner; typed over a selection they wrap it, as do `*`, `_`, `=` and `~`
- From the command palette: strikethrough, highlight, code, headings 1 to 6, moving a line up or down, and inserting a callout, code block, table or rule
- Enter in a list, a task list or a quote — starts the next item; on an empty item it ends the list
- Tab and Shift+Tab on a list item — nest it deeper, or bring it back out
- Ctrl+E — switch between editing and Read
- A click of the scroll wheel — autoscroll: move the mouse away from the mark to scroll, click to stop

Not shown by this app: math and diagrams, pictures from the web, WebP and SVG pictures, and embedded PDF, sound or video files.
