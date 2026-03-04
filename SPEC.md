# r-clite Specification

> **Binary name:** `rcte`
> **Version:** 0.1.0
> **Status:** Pre-implementation — stubs only

---

## 1. Project Overview

**r-clite** is a minimal CLI text editor written in Rust. It is invoked as
`rcte` in the terminal.

### 1.1 Philosophy

- **KISS** — Keep It Simple, Stupid. Every feature earns its place.
- Clean, readable code over clever code.
- Inspired by `kilo` / `hecto` in spirit, built from first principles with
  idiomatic Rust.

### 1.2 Goals

1. A fully working terminal text editor for real everyday use.
2. Teach idiomatic Rust through a meaningful, non-trivial project.
3. Extended feature: **collaborative real-time editing over LAN** (like a local
   Google Docs for the terminal).

### 1.3 Non-Goals

- Not a vim/emacs replacement (no modal editing, no extensible command set).
- No plugin system.
- No cloud sync.
- No GUI.

---

## 2. Platform Support

| Platform | Tier | Notes |
|----------|------|-------|
| Linux | **First-class** | Primary development target |
| macOS | **First-class** | Must work out of the box |
| Windows | **Best-effort** | Should compile and run; known gaps acceptable initially |

The terminal backend is **`crossterm`** — it is the only crate that handles all
three OSes correctly at the raw terminal level. `termion` is explicitly excluded
(Linux/macOS only).

---

## 3. Project Structure

```
r-clite/
├── Cargo.toml
├── Cargo.lock
├── SPEC.md
├── README.md
├── LICENSE
├── src/
│   ├── main.rs           ← entry point: arg parsing only, no business logic
│   ├── editor.rs         ← core editor state and event loop
│   ├── buffer.rs         ← text buffer (rope-backed) with undo/redo
│   ├── terminal.rs       ← raw terminal I/O abstraction over crossterm
│   ├── keymap.rs         ← key binding definitions and dispatch
│   ├── ui.rs             ← rendering: status bar, line numbers, viewport
│   └── collab/           ← (Milestone 4) LAN collaboration module
│       ├── mod.rs
│       ├── server.rs
│       └── client.rs
└── tests/
    └── buffer_tests.rs   ← unit tests for buffer logic
```

### 3.1 Module Rules

- `main.rs` contains **only** CLI argument parsing (via `clap`) and the call to
  start the editor. Zero business logic.
- Each module has one clear responsibility.
- If a module grows beyond ~300 lines, it is a signal to split it.
- All public APIs must have doc comments (`///`).

---

## 4. Dependencies

Every dependency must be justified. No dependency without a reason.

| Crate | Version | Purpose | Feature-gated? |
|-------|---------|---------|----------------|
| `crossterm` | 0.28 | Cross-platform raw terminal control (input events, cursor, colors) | No |
| `ropey` | 1.6 | Efficient rope data structure for the text buffer (handles large files, cheap mid-text edits) | No |
| `anyhow` | 1 | Ergonomic error handling with context chaining | No |
| `clap` | 4 (derive) | CLI argument parsing (`rcte <file>`, `--host`, `--join`) | No |
| `tokio` | 1 (full) | Async runtime for the collab networking layer | Yes (`collab`) |
| `serde` | 1 (derive) | Serialization for collab protocol messages | Yes (`collab`) |
| `serde_json` | 1 | JSON encoding/decoding for collab protocol | Yes (`collab`) |

### 4.1 Feature Flags

```toml
[features]
default = []
collab = ["dep:tokio", "dep:serde", "dep:serde_json"]
```

Milestones 1–3 build without the `collab` feature, keeping the binary lean.
Milestone 4 is enabled via `cargo build --features collab`.

### 4.2 Adding New Dependencies

Any new dependency must be documented in this section with:
- Crate name and version
- One-sentence justification
- Whether it is feature-gated

---

## 5. Milestones

Each milestone produces a **working, runnable editor**. The editor must never be
left in a broken state between milestones.

---

### 5.1 Milestone 1 — Bare Minimum Viable Editor

**Goal:** Open a file, move around, quit. Nothing else.

#### 5.1.1 Deliverables

1. **Raw mode lifecycle**
   - Enter raw terminal mode on launch using `crossterm::terminal::enable_raw_mode()`.
   - Restore terminal state on exit — even on panic. Implement a `RawModeGuard`
     struct whose `Drop` implementation calls `disable_raw_mode()` and flushes
     the alternate screen.

2. **File rendering**
   - Read the file into a `ropey::Rope`.
   - Render visible lines to the terminal, one per row.
   - Lines longer than the terminal width are truncated (horizontal scroll comes
     in Milestone 3).
   - Empty lines beyond the file content display a `~` character (à la vim).

3. **Keyboard input loop**
   - Use `crossterm::event::read()` in a blocking loop.
   - Dispatch key events through `keymap.rs` to editor commands.

4. **Cursor movement**
   | Key | Action |
   |-----|--------|
   | `↑` / `Ctrl+P` | Move cursor up one line |
   | `↓` / `Ctrl+N` | Move cursor down one line |
   | `←` / `Ctrl+B` | Move cursor left one character |
   | `→` / `Ctrl+F` | Move cursor right one character |
   | `Home` | Move cursor to beginning of line |
   | `End` | Move cursor to end of line |
   | `Page Up` | Move cursor up one viewport height |
   | `Page Down` | Move cursor down one viewport height |

   Cursor clamping rules:
   - The cursor column must never exceed the length of the current line.
   - When moving vertically to a shorter line, clamp the column to the end of
     that line but remember the "desired column" so moving back to a longer line
     restores the original column position.
   - The cursor row must never go below 0 or above `line_count - 1`.

5. **Viewport scrolling (vertical only)**
   - The viewport scrolls to keep the cursor visible.
   - Maintain a `scroll_offset` (top-left row of the viewport).
   - When the cursor moves above the viewport, `scroll_offset` decreases.
   - When the cursor moves below the viewport, `scroll_offset` increases.

6. **Status bar**
   - Rendered on the second-to-last terminal row.
   - Contents: `<filename> | Ln <line>, Col <col> | <dirty_flag>`
   - `<filename>` shows `[No Name]` for unnamed buffers.
   - `<dirty_flag>` shows `[modified]` when the buffer differs from disk,
     empty string otherwise.
   - The status bar is rendered in inverse video (swap foreground/background).

7. **Message bar**
   - Rendered on the last terminal row.
   - Used for transient messages (e.g., "Ctrl+Q to quit", prompts).
   - Messages auto-clear after 5 seconds.

8. **Quit**
   - `Ctrl+Q` quits the editor.
   - If the buffer has unsaved changes, display a warning in the message bar:
     `"WARNING: File has unsaved changes. Press Ctrl+Q again to quit."`.
   - A second `Ctrl+Q` within 3 seconds force-quits.

9. **CLI invocation**
   - `rcte <filename>` — open the given file.
   - `rcte` (no arguments) — open an empty unnamed buffer.
   - Invalid file paths produce a human-readable error and exit with code 1.

#### 5.1.2 Acceptance Criteria

- Can open any UTF-8 text file and display its contents.
- Cursor never goes out of bounds regardless of key sequence.
- Terminal is always restored on exit (normal exit, `Ctrl+Q`, and panic).
- `cargo test` passes.

---

### 5.2 Milestone 2 — Editing

**Goal:** Actually edit text and save it.

#### 5.2.1 Deliverables

1. **Character insertion**
   - Printable characters (including multi-byte UTF-8) are inserted at the
     cursor position.
   - After insertion the cursor advances one position to the right.

2. **Backspace**
   - Deletes the character to the left of the cursor.
   - At the beginning of a line: joins the current line with the previous line
     (cursor moves to the end of the previous line).
   - At position (0, 0): no-op.

3. **Delete**
   - Deletes the character at (under) the cursor.
   - At the end of a line: joins the next line onto the current line.
   - At the end of the last line: no-op.

4. **Enter / newline**
   - Splits the current line at the cursor position.
   - The cursor moves to column 0 of the new line below.

5. **Tab handling**
   - Insert spaces according to the configured tab width (default: 4).
   - No hard tab characters are inserted by default.

6. **Save**
   - `Ctrl+S` — save to the current file path.
     - If the buffer has no associated path (unnamed buffer), behave like
       "Save As" (prompt for a filename in the message bar).
   - `Ctrl+Shift+S` — always prompt for a filename (Save As).
     - If `Ctrl+Shift+S` is not reliably detectable on all terminals, fall back
       to an alternative binding documented in the status bar.
   - On successful save:
     - Clear the dirty flag.
     - Display `"<filename> written — <byte_count> bytes"` in the message bar.
   - On failure: display the error in the message bar; do not panic.

7. **Dirty flag**
   - The buffer is "dirty" if its content differs from the last saved/loaded
     state.
   - The status bar shows `[modified]` when dirty.
   - Saving clears the dirty flag.

8. **New file creation**
   - If `rcte newfile.txt` is invoked and `newfile.txt` does not exist, open an
     empty buffer with that filename. The file is created on disk only when the
     user saves.

#### 5.2.2 Acceptance Criteria

- Round-trip test: open a file, make edits, save, reopen — content is correct
  and identical.
- No panics on emoji, CJK characters, or other multi-byte UTF-8 input.
- Dirty flag correctly reflects buffer state at all times.
- `cargo test` passes.

---

### 5.3 Milestone 3 — Usability Essentials

**Goal:** Make the editor usable for real work.

#### 5.3.1 Deliverables

1. **Undo / Redo**
   - `Ctrl+Z` — undo the last edit operation.
   - `Ctrl+Y` — redo the last undone operation.
   - Implementation: a command stack (vector of operations).
   - Each operation records:
     - The edit type (insert / delete / split-line / join-line).
     - The position and text involved.
     - A "before" snapshot sufficient to reverse the operation.
   - Performing a new edit after an undo clears the redo stack.
   - Grouping: rapid consecutive character inserts at adjacent positions should
     be grouped into a single undo step (e.g., typing a word). A group boundary
     is created by:
     - A 1-second pause in typing.
     - A cursor movement.
     - A non-insert operation (delete, newline, etc.).

2. **Find (search)**
   - `Ctrl+F` opens a search prompt in the message bar.
   - The user types a query; matching is case-insensitive by default.
   - `Enter` or `n` — jump to the next match.
   - `N` (shift+n) — jump to the previous match.
   - `Esc` — cancel search and return cursor to its original position.
   - Search wraps around at end/beginning of file.
   - The current match is highlighted (inverse video or distinct color).

3. **Line numbers**
   - Displayed in a left gutter, right-aligned, with a one-space separator
     from text.
   - Gutter width adjusts to the number of digits needed (e.g., 3 columns for
     files up to 999 lines, 4 for up to 9999, etc.).
   - Toggled with `Ctrl+L`. Default: on.

4. **Horizontal scrolling**
   - Lines longer than the viewport are not wrapped.
   - The viewport scrolls horizontally to keep the cursor visible.
   - Maintain a `col_offset` analogous to the vertical `scroll_offset`.

5. **Syntax highlighting**
   - At minimum, highlight Rust (`.rs`) files.
   - Categories to highlight: keywords, strings, single-line comments (`//`),
     multi-line comments (`/* */`), numbers, types (capitalized identifiers).
   - Use a simple hand-rolled highlighter with regex or string matching. If this
     proves too complex or slow, `syntect` may be introduced as a dependency
     (must be documented here first).
   - Highlighting must not visibly lag on files under 10,000 lines.
   - Highlighting is file-extension-based: `.rs` → Rust rules; unknown
     extensions → no highlighting.

6. **Configuration file**
   - Path: `~/.config/r-clite/config.toml`
   - Parsed at startup. Missing file or missing keys use defaults silently.
   - Supported keys:

     | Key | Type | Default | Description |
     |-----|------|---------|-------------|
     | `tab_width` | integer | 4 | Number of spaces per tab |
     | `line_numbers` | boolean | true | Show line numbers on startup |
     | `theme` | string | `"dark"` | `"dark"` or `"light"` — controls status bar and highlight palette |

   - Invalid values produce a warning in the message bar on startup but do not
     prevent the editor from opening.

#### 5.3.2 Acceptance Criteria

- Undo/redo works correctly across all edit operations from Milestone 2,
  including multi-byte characters.
- Undo grouping collapses rapid typing into single steps.
- `Ctrl+F` find correctly wraps around end of file.
- Syntax highlighting does not visibly lag on files under 10,000 lines.
- Config file is read and applied; missing config is silently ignored.
- `cargo test` passes.

---

### 5.4 Milestone 4 — LAN Collaboration

**Goal:** Two or more users on the same local network can edit the same file
simultaneously, seeing each other's changes in near-real-time.

#### 5.4.1 Design Constraints

- **KISS first**: use a simple **server-authoritative operational transform
  (OT)** approach. Do not implement a CRDT unless OT proves insufficient.
- One peer acts as **host** (owns the file, runs a TCP server). Others are
  **guests** (connect as TCP clients).
- **LAN only** — no NAT traversal, no relay server.
- **Latency target:** remote changes appear on peers within 200 ms on a typical
  LAN.

#### 5.4.2 Protocol

**Transport:** TCP with length-prefixed JSON messages.

Each message is sent as:
```
[4 bytes: payload length as big-endian u32][payload: UTF-8 JSON]
```

**Message types:**

1. **Client → Server: Operation**
   ```json
   {
     "type": "op",
     "op": "insert" | "delete",
     "pos": <char_offset>,
     "text": "<string>",
     "rev": <revision_number>
   }
   ```
   - `pos`: character offset in the document (0-indexed).
   - `text`: for `insert`, the text to insert; for `delete`, the text that was
     deleted (used for conflict resolution and undo).
   - `rev`: the server revision number the client's operation is based on.

2. **Server → Client: Transformed Operation**
   ```json
   {
     "type": "op",
     "op": "insert" | "delete",
     "pos": <char_offset>,
     "text": "<string>",
     "rev": <new_revision_number>,
     "peer": "<username>"
   }
   ```
   - Broadcast to all clients (including the originator, as confirmation).
   - `rev` is the new canonical revision after applying this operation.
   - `peer` identifies the author for cursor display.

3. **Server → Client: Full Sync (on connect)**
   ```json
   {
     "type": "sync",
     "content": "<full_document_text>",
     "rev": <current_revision>,
     "peers": ["<username1>", "<username2>"]
   }
   ```

4. **Client → Server: Join**
   ```json
   {
     "type": "join",
     "username": "<string>"
   }
   ```

5. **Server → All Clients: Peer Update**
   ```json
   {
     "type": "peer_update",
     "peers": ["<username1>", "<username2>"],
     "event": "joined" | "left",
     "username": "<string>"
   }
   ```

6. **Client → Server: Cursor Position**
   ```json
   {
     "type": "cursor",
     "pos": <char_offset>
   }
   ```

7. **Server → All Clients: Remote Cursor**
   ```json
   {
     "type": "cursor",
     "peer": "<username>",
     "pos": <char_offset>
   }
   ```

#### 5.4.3 Operational Transform Rules

The server maintains:
- A canonical `Rope` buffer.
- A revision counter (starts at 0, increments with each applied operation).
- A log of the last N operations (for transforming late-arriving client ops).

When the server receives a client operation at revision `R`:
1. Collect all operations from the server log with revision > `R`.
2. Transform the client operation against each of those operations in order,
   using the standard OT transform functions:
   - **insert vs insert**: if positions conflict, the server's operation wins
     (its text stays in place; the client's position shifts right).
   - **insert vs delete**: adjust positions based on whether the insert falls
     within, before, or after the deleted range.
   - **delete vs insert**: adjust the delete range if the insert shifts text.
   - **delete vs delete**: handle overlapping deletes by shrinking or
     eliminating the later one.
3. Apply the transformed operation to the canonical buffer.
4. Increment the revision counter.
5. Broadcast the transformed operation (with the new revision) to all clients.

#### 5.4.4 Client-Side Behavior

- Local edits are applied **optimistically** to the local buffer immediately.
- The client also sends the operation to the server.
- When the server's confirmation (or a transformed version) arrives, the client
  reconciles:
  - If the confirmed op matches the local prediction, no action needed.
  - If the op was transformed (positions shifted), the client adjusts its local
    state to match the server's canonical version.
- Pending (unacknowledged) local operations are rebased against incoming remote
  operations.

#### 5.4.5 CLI Commands

| Command | Description |
|---------|-------------|
| `rcte --host <file>` | Open file and start hosting on a random available TCP port. Print the port to the status bar and stdout. |
| `rcte --join <host>:<port>` | Connect to a host as a guest. `<host>` is an IP address or hostname. |

- The host also edits locally — it is both server and client.
- `Ctrl+S` on the host saves the canonical buffer to disk. Guests cannot save
  (they see a message: `"Only the host can save."`).
- `Ctrl+Q` on a guest disconnects and exits. The host remains running.
- `Ctrl+Q` on the host prompts: `"Disconnect all peers and quit?"`.

#### 5.4.6 Collab UI

- **Status bar** shows: `[Host: <port>]` or `[Guest: <host>:<port>]` and
  `[<N> peers]`.
- **Remote cursors** are shown as colored block characters at the peer's cursor
  position.
  - Each peer gets a distinct color (cycle through a palette of 6–8 colors).
  - The peer's username (from `$USER` or `whoami`) is shown as a label next to
    the cursor on the same line (truncated if it would overlap text).
- **Connection status**: if the TCP connection drops, show `[disconnected]` in
  the status bar and attempt to reconnect every 2 seconds (up to 10 retries).

#### 5.4.7 Acceptance Criteria

- Two terminals on the same machine (localhost) can simultaneously edit a file.
- Two machines on the same LAN can simultaneously edit a file.
- No data loss when two users type at the same time in the same region.
- Disconnecting a guest does not crash the host.
- Disconnecting the host causes guests to show `[disconnected]` and stop
  sending operations.
- `cargo test --features collab` passes, including convergence tests.

---

## 6. Key Bindings Summary

| Key | Action | Milestone |
|-----|--------|-----------|
| `↑` / `Ctrl+P` | Cursor up | M1 |
| `↓` / `Ctrl+N` | Cursor down | M1 |
| `←` / `Ctrl+B` | Cursor left | M1 |
| `→` / `Ctrl+F` | Cursor right | M1 |
| `Home` | Beginning of line | M1 |
| `End` | End of line | M1 |
| `Page Up` | Page up | M1 |
| `Page Down` | Page down | M1 |
| `Ctrl+Q` | Quit (with unsaved-changes guard) | M1 |
| `Backspace` | Delete char left | M2 |
| `Delete` | Delete char right | M2 |
| `Enter` | Insert newline / split line | M2 |
| `Tab` | Insert spaces (tab width) | M2 |
| `Ctrl+S` | Save | M2 |
| `Ctrl+Shift+S` | Save As (if detectable) | M2 |
| `Ctrl+Z` | Undo | M3 |
| `Ctrl+Y` | Redo | M3 |
| `Ctrl+F` | Find | M3 |
| `Ctrl+L` | Toggle line numbers | M3 |

---

## 7. Error Handling Rules

1. Use `anyhow::Result` for all fallible functions that cross module boundaries.
2. Use `thiserror` for defining typed errors within a module when callers need
   to match on specific variants. Add `thiserror` as a dependency only when
   first needed.
3. **Never use `.unwrap()` or `.expect()` in production paths.** These are
   allowed only in:
   - Test code.
   - Truly unreachable branches, with a comment explaining why.
4. All I/O errors must surface a human-readable message to the status bar /
   message bar. The editor must not panic on I/O errors.
5. Startup errors (e.g., invalid file path) print to stderr and exit with
   code 1.

---

## 8. Testing Strategy

### 8.1 Unit Tests

Located in `tests/buffer_tests.rs` and as `#[cfg(test)]` modules within source
files where appropriate.

Coverage areas:
- **Buffer operations**: insert, delete, split-line, join-line at various
  positions including boundaries (start of file, end of file, empty file).
- **UTF-8 correctness**: insert/delete multi-byte characters (emoji, CJK,
  combining characters). Verify byte offsets vs. char offsets are handled
  correctly.
- **Undo/redo**: single operations, grouped operations, undo-then-new-edit
  clears redo stack.
- **Dirty flag**: verify transitions on edit, save, undo-to-clean-state.

### 8.2 Integration Tests

- A test helper that constructs an `Editor` in headless mode (no real terminal)
  and feeds key events programmatically.
- Assert on buffer content and cursor position after key sequences.
- Example scenarios:
  - Open file → type text → save → verify file on disk.
  - Open file → type text → undo → verify buffer matches original.
  - Open file → Ctrl+F → type query → verify cursor lands on match.

### 8.3 Collaboration Tests (Milestone 4)

- Use `tokio::test` to run a host and two clients in-process.
- Scenarios:
  - Two clients insert at different positions → verify convergence (both see
    the same final buffer).
  - Two clients insert at the same position → verify no data loss and both
    converge.
  - Client disconnects mid-edit → host and remaining client are unaffected.
  - Client reconnects and receives full sync.

---

## 9. Performance Considerations

- **Rope-backed buffer**: `ropey` provides O(log n) insert and delete, suitable
  for files of any size.
- **Rendering**: only re-render lines that have changed. Track dirty line ranges
  per frame to avoid full-screen redraws.
- **Syntax highlighting**: run the highlighter only on visible lines (viewport).
  Cache highlighted output and invalidate on edit.
- **Collab**: batch rapid operations (e.g., hold down a key) into fewer network
  messages. Send at most one operation message per 16 ms (~60 fps).

---

## 10. Future Considerations (Out of Scope)

These are explicitly **not planned** but noted for awareness:

- Multiple buffers / split panes.
- Mouse support.
- Clipboard integration (OS clipboard).
- Plugin/extension system.
- Syntax highlighting for languages beyond Rust.
- Configuration hot-reload.

These may be revisited after all four milestones are complete.
