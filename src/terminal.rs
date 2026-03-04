// Low-level terminal I/O abstraction built on crossterm.
//
// Handles entering/exiting raw mode, reading input events, and
// writing to the terminal. Guarantees terminal state restoration on
// drop (even on panic).
