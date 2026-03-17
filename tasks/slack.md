---
agent_doc_session: 22f64516-0a5f-4527-9037-9464d02cdf08
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# Slack Linux Issues

## Summary

**Ctrl+Shift+U hotkey conflict** — Documented as "Insert Hyperlink" but intercepted by IBus unicode input mode on Linux. GTK_IM_MODULE and XMODIFIERS env vars don't fix it (Electron handles IM independently). Bug reported to feedback@slack.com (Feb 27, 2026). Workaround: copy URL to clipboard, select text, Ctrl+V.

**Notifications not working** — Root cause: `notification-daemon` (GNOME) running instead of `dunst` under i3. The GNOME daemon silently swallows notifications. Fix: kill notification-daemon, start dunst, add `exec --no-startup-id dunst` to i3 config.

**Pending:** Reply email with screenshot attachment (corky 0.12.1). Screenshot file location unknown — needs user to provide path.

<!-- agent:input mode=replace -->
<!-- /agent:input -->

<!-- agent:status mode=replace -->
- [x] Ctrl+Shift+U bug: reported to Slack
- [x] Notifications: root cause identified (notification-daemon vs dunst)
- [ ] Reply email with screenshot (need file path)
- [ ] Verify dunst fix resolves notifications
<!-- /agent:status -->

<!-- agent:exchange mode=append -->

### 2026-03-17 — Conversion to template format

Compacted inline conversation (17 exchanges) into summary. Key decisions preserved above.

**Notification diagnosis results:**
- `notification-daemon` PID 2692 running, dunst installed but not started
- i3 WM, X11 (`:0`), Slack 4.47.69
- `notify-send` succeeds (DBus works) but GNOME daemon renders nothing under i3
- Slack config: `notificationPlayback: web`, `notificationMethod: null`, `muteSounds: false`

**Next steps:** Provide screenshot path for corky reply. Kill notification-daemon, start dunst, verify Slack notifications work.
go
<!-- /agent:exchange -->
