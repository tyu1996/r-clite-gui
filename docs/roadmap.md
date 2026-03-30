# r-clite Feature Roadmap

Last updated: 2026-03-30

This document is the long-horizon reference for planned milestones.
Each milestone has its own spec in `docs/superpowers/specs/`.

---

## Milestone 1 — Fluid Text Movement
**Spec:** `docs/superpowers/specs/2026-03-30-milestone1-fluid-text-movement-design.md`
**Status:** Spec approved, pending implementation

Make everyday movement and line length comfortable.

- Word navigation: Ctrl/Option+Left/Right, Ctrl/Option+Backspace/Delete
- Soft word wrap: visual wrapping without modifying the buffer; togglable (Ctrl+Shift+W / Cmd+Shift+W); on by default
- Hard wrap / reflow paragraph: reflow current paragraph to `wrap_column` (Alt+Q / Option+Q)
- Config: `word_wrap`, `wrap_column`

---

## Milestone 2 — Text Manipulation
**Status:** Planned

Complete the find workflow and enable basic text substitution.

- Find & Replace: replace current match, replace all
- Case-sensitive / case-insensitive toggle in search
- Possibly: regex search (stretch goal)

---

## Milestone 3 — Multi-file Workflow
**Status:** Planned

Let users work with more than one file at a time.

- Tab bar with multiple open buffers (GUI)
- Ctrl+Tab / Cmd+T to cycle or open new tab
- Per-tab dirty flag and save prompt on close
- TUI: buffer list or split (TBD at spec time)

---

## Milestone 4 — Smart Editing
**Status:** Planned

Quality-of-life editing features that reduce manual work.

- Auto-indent: preserve indentation level on Enter
- Bracket / quote auto-pair: `(`, `[`, `{`, `"`, `'`
- Jump to line: Ctrl+G / Cmd+G

---

## Milestone 5 — Polish
**Status:** Planned

Surface information and improve daily comfort.

- Recent files list (File menu / quick open)
- Word count and character count in status bar
- Font size zoom in GUI (Ctrl+= / Ctrl+-)
- Expanded syntax highlighting beyond `.rs`
