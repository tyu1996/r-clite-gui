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

## Key Bindings

| Key | Action |
|-----|--------|
| Arrow keys | Move cursor |
| `Ctrl+S` | Save |
| `Ctrl+Q` | Quit |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+F` | Find |
| `Ctrl+L` | Toggle line numbers |

## LAN Collaboration (experimental)

```sh
# Build with collab support
cargo build --release --features collab

# Host a session
rcte --host myfile.txt

# Join from another terminal
rcte --join 192.168.1.10:12345
```

## License

MIT
