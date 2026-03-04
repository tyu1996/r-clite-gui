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

fn main() {
    // TODO: Parse CLI arguments with clap and start the editor.
}
