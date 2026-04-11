---
agent_doc_session: ee30b948-3a02-4d31-8df7-3b0b77051216
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ On my system, the cursor in the browser is very small. The cursor is much bigger with other apps. Why is it small for browsers?
❯ On my system, the cursor in the browser is very small. The cursor is much bigger with other apps. Why is it small for browsers?

### Re: Small browser cursor (HEAD)

Classic HiDPI/cursor scaling issue. Browsers bypass the system cursor size settings that native GTK/Qt apps respect.

**Why it happens:**
- Native apps read `XCURSOR_SIZE` from the environment or GTK/Qt cursor theme settings
- Chromium-based browsers default to 24px or use their own scaling logic
- On Wayland, browsers running as XWayland clients don't inherit compositor cursor scaling

Your `XCURSOR_SIZE` env var isn't set — that's the root cause.

**Fix 1 — Set globally in `/etc/environment`:**
```
XCURSOR_SIZE=32
```
(48 for even larger)

**Fix 2 — Wayland sessions, also `~/.config/gtk-3.0/settings.ini`:**
```ini
[Settings]
gtk-cursor-theme-size=32
```

**Fix 3 — KDE Plasma:** System Settings → Appearance → Cursors → set size there

Log out and back in after editing `/etc/environment` — browsers pick it up on next launch.
<!-- agent:boundary:3858c809 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
