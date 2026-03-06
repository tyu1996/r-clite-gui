/// r-clite (rcte) — a minimal CLI text editor written in Rust.
///
/// This is the entry point. It handles only CLI argument parsing and
/// delegates all work to the editor module.
mod buffer;
mod config;
mod editor;
mod file_picker;
mod highlight;
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

    /// Open file and start hosting a collaborative session on a random TCP port.
    #[cfg(feature = "collab")]
    #[arg(long, value_name = "FILE", conflicts_with = "join")]
    host: Option<PathBuf>,

    /// Join a collaborative session at <host>:<port>.
    #[cfg(feature = "collab")]
    #[arg(long, value_name = "HOST:PORT", conflicts_with = "host")]
    join: Option<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("rcte: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let (cfg, cfg_warning) = config::Config::load();

    #[cfg(feature = "collab")]
    {
        if let Some(host_file) = cli.host {
            let buf = buffer::Buffer::open(host_file)?;
            let content: String = buf.rope.to_string();
            let username = whoami();

            let (port, collab_handle) =
                collab::server::start_server(content, username)?;

            eprintln!("rcte: hosting on port {}", port);

            let mut ed = editor::Editor::new(buf, cfg, Some(collab_handle))?;
            if let Some(warn) = cfg_warning {
                ed.set_startup_message(warn);
            }
            return ed.run();
        }

        if let Some(join_str) = cli.join {
            let addr = parse_addr(&join_str)?;
            let username = whoami();

            let collab_handle = collab::client::connect_client(addr, username)?;

            // The initial buffer content comes from the FullSync event.
            // Drain the event queue to get it.
            let initial_content = drain_initial_sync(&collab_handle);
            let buf = buffer::Buffer::from_content(initial_content);

            let mut ed = editor::Editor::new(buf, cfg, Some(collab_handle))?;
            if let Some(warn) = cfg_warning {
                ed.set_startup_message(warn);
            }
            return ed.run();
        }
    }

    let buf = match cli.file {
        Some(path) => buffer::Buffer::open(path)?,
        None => buffer::Buffer::new_empty(),
    };

    #[cfg(not(feature = "collab"))]
    let mut ed = editor::Editor::new(buf, cfg)?;
    #[cfg(feature = "collab")]
    let mut ed = editor::Editor::new(buf, cfg, None)?;

    if let Some(warn) = cfg_warning {
        ed.set_startup_message(warn);
    }

    ed.run()
}

/// Get the current username for display in collab sessions.
#[cfg(feature = "collab")]
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Parse a "host:port" string into a `SocketAddr`.
#[cfg(feature = "collab")]
fn parse_addr(s: &str) -> Result<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    s.to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", s, e))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve address '{}'", s))
}

/// Drain the event channel to retrieve the initial FullSync content.
/// Falls back to empty string if no sync arrives quickly.
#[cfg(feature = "collab")]
fn drain_initial_sync(handle: &collab::CollabHandle) -> String {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match handle.event_rx.try_recv() {
            Ok(collab::CollabEvent::FullSync { content, .. }) => return content,
            Ok(_) => {} // ignore other events during init
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if Instant::now() > deadline {
                    return String::new();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return String::new(),
        }
    }
}
