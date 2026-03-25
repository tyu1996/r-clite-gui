extern crate r_clite;

use std::path::PathBuf;

use clap::Parser;
use r_clite::gui;

#[derive(Parser)]
#[command(name = "rcte-gui", version, about = "A minimal GUI text editor")]
struct Cli {
    /// Optional file to open on startup.
    file: Option<PathBuf>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("rcte-gui: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    gui::launch(cli.file)
}
