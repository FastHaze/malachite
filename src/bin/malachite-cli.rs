//! `malachite-cli` — Malachite on the command line: a front end for the vault in
//! `lib.rs`. The app itself lives in `src/bin/malachite/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use malachite::search::Query;
use malachite::{tags, wikilinks, Vault};

#[derive(Parser)]
#[command(name = "malachite-cli", about = "Malachite on the command line: Markdown notes in a vault folder")]
struct Cli {
    /// Vault directory (created if missing)
    #[arg(long, short, default_value = "vault", global = true)]
    vault: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new note (title defaults to "Untitled", auto-numbered on conflict)
    New {
        title: Option<String>,
        /// Initial body text
        #[arg(long, short)]
        body: Option<String>,
        /// Tags stored in frontmatter
        #[arg(long, short, num_args = 1..)]
        tags: Vec<String>,
        /// Folder inside the vault to put the note in (made if missing)
        #[arg(long, short)]
        folder: Option<PathBuf>,
    },
    /// Open (or create) today's daily note
    Daily,
    /// List all notes, with the folder of those that are in one
    List,
    /// Move a note into a folder inside the vault ("." is the top level)
    Move { title: String, folder: PathBuf },
    /// List the folders in the vault
    Folders,
    /// Print a note
    Show { title: String },
    /// Append a line to a note
    Append { title: String, text: String },
    /// Rename a note and update [[links]] pointing to it
    Rename { from: String, to: String },
    /// Delete a note
    Delete { title: String },
    /// Search with a query: words, "a phrase", a OR b, -word,
    /// tag: file: path: content: line:( ) block:( ) section:( ) task:
    /// task-todo: task-done: match-case:
    Search { query: String },
    /// Show notes that link to a note
    Backlinks { title: String },
    /// List all tags, or notes with a given tag
    Tags { tag: Option<String> },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault = Vault::open(cli.vault)?;

    match cli.cmd {
        Cmd::New { title, body, tags, folder } => {
            let folder = top_level_if_dot(folder.unwrap_or_default());
            let n = vault.create_in(&folder, title.as_deref(), body.as_deref().unwrap_or(""), &tags)?;
            println!("created {}", n.path.display());
        }
        Cmd::Daily => {
            let (n, created) = vault.daily()?;
            let verb = if created { "created" } else { "exists " };
            println!("{verb} {}", n.path.display());
        }
        Cmd::List => {
            for n in vault.notes()? {
                println!("{}", slashed(&vault.folder_of(&n.path).join(&n.title)));
            }
        }
        Cmd::Move { title, folder } => {
            let note = vault.find(&title)?;
            let moved = vault.move_note(&note.path, &top_level_if_dot(folder))?;
            println!("moved {} -> {}", note.title, moved.display());
        }
        Cmd::Folders => {
            for folder in vault.folders()? {
                println!("{}", slashed(&folder));
            }
        }
        Cmd::Show { title } => print!("{}", vault.find(&title)?.content),
        Cmd::Append { title, text } => vault.append(&title, &text)?,
        Cmd::Rename { from, to } => {
            let (from, to, updated) = vault.rename(&from, &to)?;
            println!("renamed '{from}' -> '{to}' ({updated} notes updated)");
        }
        Cmd::Delete { title } => println!("deleted {}", vault.delete(&title)?.title),
        Cmd::Search { query } => {
            let query = Query::parse(&query);
            for n in vault.notes()?.iter().filter(|n| query.matches(n, vault.root())) {
                let lines = query.lines(n);
                // A note can match by its name or its tags alone.
                if lines.is_empty() {
                    println!("{}", n.title);
                }
                for found in lines {
                    println!("{}:{}: {}", n.title, found.line, n.content[found.range].trim());
                }
            }
        }
        Cmd::Backlinks { title } => {
            // A link may name the note by one of its aliases.
            let target = vault.find(&title)?;
            for n in vault.notes()?.iter().filter(|n| n.path != target.path) {
                if wikilinks(&n.content).iter().any(|l| target.is_called(l)) {
                    println!("{}", n.title);
                }
            }
        }
        Cmd::Tags { tag } => {
            let notes = vault.notes()?;
            let mut index: BTreeMap<String, Vec<&str>> = BTreeMap::new();
            for n in &notes {
                for t in tags(&n.content) {
                    index.entry(t).or_default().push(&n.title);
                }
            }
            match tag {
                Some(t) => {
                    for title in index.get(t.trim_start_matches('#')).into_iter().flatten() {
                        println!("{title}");
                    }
                }
                None => {
                    for (t, v) in &index {
                        println!("#{t} ({})", v.len());
                    }
                }
            }
        }
    }
    Ok(())
}

/// A path inside the vault as text, with `/` between its parts on every platform.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// On the command line "." names the top level of the vault, which the
/// library spells as an empty path.
fn top_level_if_dot(folder: PathBuf) -> PathBuf {
    if folder == Path::new(".") {
        PathBuf::new()
    } else {
        folder
    }
}
