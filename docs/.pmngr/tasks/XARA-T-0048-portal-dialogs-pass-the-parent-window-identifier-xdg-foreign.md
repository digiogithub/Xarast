---
id: XARA-T-0048
type: task
title: "Portal dialogs: pass the parent window identifier (xdg-foreign)"
status: backlog
priority: low
parent: XARA-US-0012
author: mcp
labels: [shell]
created: 2026-09-23T12:09:55Z
updated: 2026-09-23T12:09:55Z
---

## Description
File dialogs are requested with no parent window, so on Wayland they are independent toplevels (not transient/modal to the Xarast window, and subject to focus-stealing prevention). ashpd's `WindowIdentifier` from the raw window handle exports the surface through xdg-foreign; the identifier must stay alive for the dialog's lifetime and the services thread has no window today.

## Acceptance Criteria
- The portal dialog is attached to the Xarast window on GNOME and KDE.
