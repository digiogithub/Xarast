---
id: XARA-T-0021
type: task
title: "Fix: DocumentBuilder::finish returns Inconsistent on dangling colour refs and on sourceless controllers at the depth limit (fuzz_doc_builder)"
status: done
parent: XARA-US-0014
author: mcp
labels: [fuzz, doc]
created: 2026-09-23T10:24:10Z
updated: 2026-09-23T10:24:10Z
---

## Description
Two ways `finish` failed its own "valid or nothing" guarantee:
1. A layer/guideline/attribute referencing a colour that was never defined: the builder keeps dangling colour refs on purpose (they resolve through the table fallback), but `validate()` called every missing resource an error.
2. A live controller with no source at the depth limit: the repair's `attach` of an empty source was refused as too deep and the error discarded.

## Fix
1. `validate_document` reports a dangling *colour* reference as a warning; bitmap/dash/arrow stay errors (286682e).
2. A controller that cannot get a source is dropped with a `Repaired` diagnostic (1cc7984).

Tests: `a_dangling_colour_reference_is_kept_and_is_only_a_warning`, `a_sourceless_controller_at_the_depth_limit_is_dropped` (both verified to fail before).
