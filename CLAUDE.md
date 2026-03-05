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
- `buffer.rs` — `Buffer` wraps a `ropey::Rope` for UTF-8-correct text storage; tracks dirty flag and file path; exposes line-oriented API (`line_count`, `line`, `line_len`)
- `editor.rs` — `Editor` owns `Buffer`, `Ui`, and `RawModeGuard`; drives the event loop: render → read key → dispatch `Command` → update state
- `keymap.rs` — maps raw `crossterm::KeyEvent` to a `Command` enum; all key bindings live here
- `ui.rs` — `Ui` is stateless beyond terminal dimensions; renders viewport (text lines + `~` beyond EOF), status bar (inverse video), and message bar each frame
- `terminal.rs` — `RawModeGuard` RAII type that enters raw mode + alternate screen on construction and restores terminal state on drop (even on panic)
- `collab/` — feature-gated (`--features collab`) TCP-based LAN collaboration using server-authoritative OT; depends on `tokio`, `serde`, `serde_json`

**Data flow:** `Editor::run` loop → `Ui::render` (reads from `Buffer`) → `event::read()` → `keymap::map` → `Editor` mutation methods → next frame

**Feature flag:** `collab` is off by default. It gates `tokio`/`serde`/`serde_json` dependencies and the `src/collab/` module.
