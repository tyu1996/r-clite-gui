/// r-clite (rcte) — a minimal CLI text editor written in Rust.
///
/// This is the entry point. It handles only CLI argument parsing and
/// delegates all work to the editor module.
mod buffer;
mod editor;
mod keymap;
mod terminal;
mod ui;

#[cfg(feature = "collab")]
mod collab;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// rcte — a minimal CLI text editor.
#[derive(Parser)]
#[command(name = "rcte", version, about = "A minimal CLI text editor")]
struct Cli {
    /// File to open. Opens an empty unnamed buffer when omitted.
    file: Option<PathBuf>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("rcte: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let buf = match cli.file {
        Some(path) => buffer::Buffer::open(path)?,
        None => buffer::Buffer::new_empty(),
    };

    let mut ed = editor::Editor::new(buf)?;
    ed.run()
}
