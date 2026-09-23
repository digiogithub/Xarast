---
id: XARA-T-0041
type: task
title: "Portal file dialogs: rfd reports failures as cancel and spawns zenity; exit hangs on an open dialog"
status: done
priority: high
parent: XARA-US-0012
author: mcp
labels: [shell, defect]
created: 2026-09-23T12:09:31Z
updated: 2026-09-23T12:09:31Z
---

## Description
Measured in an isolated GNOME 46 session (gnome-shell --headless, xdg-desktop-portal + -gtk on a private bus):
- rfd 0.17's portal backend maps every portal failure to "cancelled" and then spawns `zenity` (observed with an unreachable bus: `Using zenity fallback`, a zenity dialog appeared). `PortalEvent::Failed` was unreachable.
- Escape in the GTK portal dialog answers response code 2 ("other"), not 1.
- Dropping `PortalService` with a dialog open joined the services thread: the process stayed up until a human answered, then served the next queued request.
- `request_ids_are_unique_and_increasing` posted real requests: on a desktop with a session bus `cargo test` would open dialogs on the developer's screen.

## Acceptance Criteria
- File dialogs through `ashpd` FileChooser; dismissal (1 or 2) = Cancelled, anything else = Failed with a reason.
- Quit with a dialog open exits promptly; queued requests are discarded.
- Tests never reach a live portal.

## Notes
Fixed in 354c59c. Evidence: open (typed path with a space), save (path with space and `#`), 3 open/Escape cycles, unreachable bus → Failed with reason and no zenity, quit with dialog open → exit, dialog closed by the portal.
