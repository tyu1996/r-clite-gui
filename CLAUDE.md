# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Build with LAN collaboration feature
cargo build --release --features collab

# Run
cargo run -- myfile.txt

# Run tests
cargo test

# Run tests (including collab)
cargo test --features collab

# Run tests for a specific module
cargo test --lib buffer

# Run a single test by name
cargo test empty_buffer_has_one_line

# Lint
cargo clippy

# Format
cargo fmt
```

## Architecture

r-clite (`rcte`) is a minimal terminal text editor. The binary name is `rcte`.

**Module responsibilities:**

- `main.rs` — CLI arg parsing via `clap`, constructs `Buffer` and `Editor`, calls `ed.run()`
- `buffer.rs` — `Buffer` wraps a `ropey::Rope` for UTF-8-correct text storage; tracks dirty flag and file path; exposes line-oriented API (`line_count`, `line`, `line_len`); `raw_insert`/`raw_delete` for OT
- `editor.rs` — `Editor` owns `Buffer`, `Ui`, and `RawModeGuard`; drives the event loop: render → read key → dispatch `Command` → update state; polls collab events each frame
- `keymap.rs` — maps raw `crossterm::KeyEvent` to a `Command` enum; all key bindings live here
- `ui.rs` — `Ui` is stateless beyond terminal dimensions; renders viewport (text lines + `~` beyond EOF), status bar (inverse video), and message bar each frame; `RenderState` holds cursor, scroll, search_match, and (behind `collab` feature) collab_status and peer_cursors
- `terminal.rs` — `RawModeGuard` RAII type that enters raw mode + alternate screen on construction and restores terminal state on drop (even on panic)
- `highlight.rs` — syntax highlighting for `.rs` files only; `highlight_line(line, ext, in_block_comment, theme)` returns `Vec<Span>` and updated block-comment state; theme is `"dark"` or `"light"`
- `config.rs` — loads `~/.config/r-clite/config.toml` at startup; silently ignores missing file; keys: `tab_width` (int), `line_numbers` (bool), `theme` (`"dark"`|`"light"`)
- `collab/` — feature-gated (`--features collab`) TCP-based LAN collaboration using server-authoritative OT; depends on `tokio`, `serde`, `serde_json`

**Data flow:** `Editor::run` loop → `Ui::render` (reads from `Buffer`) → `event::read()` → `keymap::map` → `Editor` mutation methods → next frame

**Feature flag:** `collab` is off by default. It gates `tokio`/`serde`/`serde_json` dependencies and the `src/collab/` module.

**Collab internals:** Host runs a TCP server (`collab/server.rs`) and gets a `CollabHandle`; guests connect via `collab/client.rs`. `CollabHandle` exposes an `event_rx` channel delivering `CollabEvent` (Edit, FullSync, PeersChanged, PeerCursor, ConnectionStatus). OT is handled by `transform_pos` in `collab/mod.rs`.
