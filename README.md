# r-clite

A minimal CLI text editor written in Rust.

## Quick Start

```sh
cargo build --release
./target/release/rcte myfile.txt
```

Or install it:

```sh
cargo install --path .
rcte myfile.txt
```

Open an empty unnamed buffer by omitting the filename:

```sh
rcte
```

## Key Bindings

### Navigation

| Key | Action |
|-----|--------|
| Arrow keys / `Ctrl+P` `Ctrl+N` `Ctrl+B` `Ctrl+F` | Move cursor up / down / left / right |
| `Home` | Beginning of line |
| `End` | End of line |
| `Page Up` / `Page Down` | Move one viewport height |

### Editing

| Key | Action |
|-----|--------|
| Printable characters | Insert at cursor |
| `Enter` | Insert newline / split line |
| `Tab` | Insert spaces (tab width) |
| `Backspace` | Delete character to the left |
| `Delete` | Delete character at cursor |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |

### File

| Key | Action |
|-----|--------|
| `Ctrl+O` | Open a file with the native system file picker |
| `Ctrl+S` | Save |
| `Ctrl+Shift+S` | Save As (prompt for filename) |
| `Ctrl+Q` | Quit (prompts if unsaved changes) |

If the current buffer has unsaved changes, press `Ctrl+O` once to confirm discarding them and again within a few seconds to show the file picker.

### View

| Key | Action |
|-----|--------|
| `Ctrl+F` | Find (case-insensitive, wraps around) |
| `Ctrl+L` | Toggle line numbers |

## Configuration

Create `~/.config/r-clite/config.toml` to customise the editor. Missing file or keys silently use defaults.

```toml
tab_width    = 4      # spaces per Tab key press (default: 4)
line_numbers = true   # show line numbers on startup (default: true)
theme        = "dark" # "dark" or "light" (default: "dark")
```

## Syntax Highlighting

`.rs` files are highlighted automatically (keywords, strings, comments, numbers, types). All other file types are displayed as plain text.

## LAN Collaboration (experimental)

```sh
# Build with collab support
cargo build --release --features collab

# Host a session (prints the port to use)
rcte --host myfile.txt

# Join from another terminal
rcte --join 192.168.1.10:12345
```

The host is both server and editor. `Ctrl+S` on the host saves to disk; guests cannot save. `Ctrl+Q` on a guest disconnects cleanly; `Ctrl+Q` on the host prompts before disconnecting all peers.

## License

MIT
