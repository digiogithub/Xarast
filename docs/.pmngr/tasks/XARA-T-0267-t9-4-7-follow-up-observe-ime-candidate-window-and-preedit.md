---
id: XARA-T-0267
type: task
title: T9.4.7 follow-up — observe IME candidate window and preedit on GNOME and a wlroots compositor
status: backlog
parent: XARA-US-0047
author: mcp
labels: [phase-9, text, shell]
created: 2026-09-24T07:50:52Z
updated: 2026-09-24T07:50:52Z
---

## Description
XARA-T-0224 wired IME composition into the text tool and tested it headlessly only (synthetic `winit::event::Ime` through `translate_ime`, `Viewer::ime_request`). The acceptance criterion "candidate window under the caret on GNOME and one wlroots compositor" needs a real session.

## Acceptance Criteria
- On GNOME (IBus, text-input-v3) and one wlroots compositor (e.g. sway + fcitx5): IME on exactly while a text caret is up; the candidate window opens under the caret at 1x and 1.5x; preedit is drawn in the story and underlined; commit types; Esc/cancel leaves nothing.
- Check whether any stack sends a key event with text *and* an `Ime::Commit` for one keystroke (it would double the character); if so, add a guard in `Viewer::handle`.
- Record the per-compositor observations in `docs/memory/ui.md` decision 41.

## Notes
Run in the isolated compositor setup described in `ui.md` ("How the compositor experiments are run"), never on the maintainer's desktop.
