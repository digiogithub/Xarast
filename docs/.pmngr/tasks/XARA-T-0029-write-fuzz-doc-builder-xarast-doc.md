---
id: XARA-T-0029
type: task
title: Write fuzz_doc_builder (xarast-doc)
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, doc]
created: 2026-09-23T10:33:14Z
updated: 2026-09-23T10:33:14Z
---

## Description
Arbitrary build scripts through `DocumentBuilder`: every node kind at every level, `push_scope`/`pop_scope` imbalance, live controllers/sources/generated, text outside stories, layers with and without active flags, colour definitions with forged/dangling parents and NaN components, attributes (incl. clip regions, dashes, indexed fills), and bitmap/colour keys forged with `slotmap::KeyData::from_ffi`. Outcome must be an error or a document with zero `validate()` errors; `BuildError::Inconsistent` is a finding. Closes the document-model.md TODO.
