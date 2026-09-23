---
id: XARA-T-0175
type: task
title: "W9.2 remainder: text edit commands (T9.2.4) and story invariants (T9.2.5)"
status: backlog
parent: XARA-US-0045
author: mcp
labels: [phase-9, doc, text]
created: 2026-09-23T18:48:44Z
updated: 2026-09-23T18:48:44Z
---

## Description

The read side of W9.2 is done (`xarast_doc::StoryText`, `TextPos`/`TextCursor`, attribute bridge, importer). Still open, per `docs/phases/phase-09-text.md`:

- T9.2.4 edit commands: `InsertText`, `DeleteRange`, `SplitLine`, `MergeLines`, `SetTextAttr`, `InsertKern`, `SetStoryMode`, built on `StoryText::byte_of`/`pos_of`.
- T9.2.5 story invariants in `validate.rs`: every line ends with a break item or is the last; no empty story without one line; attribute ramps never straddle a line boundary incorrectly.
- T9.3.12 `format_story` writing lines back (wrap restructuring) once edits exist.

Coordinate with the phase-7 agent editing `xarast-doc` commands.

## Acceptance Criteria

- Commands undo byte-identically; invariants checked over the corpus.
