---
agent_doc_session: 22f64516-0a5f-4527-9037-9464d02cdf08
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

# Slack Linux Issues

## Summary

**Ctrl+Shift+U hotkey conflict** — Documented as "Insert Hyperlink" but intercepted by IBus unicode input mode on Linux. GTK_IM_MODULE and XMODIFIERS env vars don't fix it (Electron handles IM independently). Bug reported to feedback@slack.com (Feb 27, 2026). Workaround: copy URL to clipboard, select text, Ctrl+V.

**Notifications** — Fixed by changing Slack's in-app notification preferences (Preferences → Notifications). `notification-daemon` (GNOME) works fine under i3 — dunst was tested but not needed. User wants "Direct messages, mentions & keywords" only (server-side per-workspace setting in Slack UI).

<!-- agent:input mode=replace -->
<!-- /agent:input -->

<!-- agent:status mode=replace -->
- [x] Ctrl+Shift+U bug: reported to Slack
- [x] Notifications: fixed via Slack in-app settings (notification-daemon works fine)
- [ ] Slack notification filter: set to "direct mentions only" per workspace (user action in Slack UI)
- [ ] Reply email with screenshot (need file path from user)
<!-- /agent:status -->

<!-- agent:exchange mode=append -->

### 2026-03-17 — Exchange compacted

**Session history:** 17 inline exchanges compacted to template format, then 8 template cycles covering notification diagnosis.

**Key findings:**
- `notification-daemon` (GNOME 3.20.0) renders notifications correctly under i3 + X11
- Slack uses `notificationPlayback: web` (renders own popups, independent of system daemon)
- The notification issue was Slack's internal preferences, not the system notification stack
- PulseSMS notifications worked throughout, confirming system notifications were fine
- Dunst was installed, configured, tested, then fully reverted (not needed)

**Remaining items:**
- Set "Direct messages, mentions & keywords" in Slack Preferences → Notifications for each of 5 workspaces
- Reply to Slack bug report email with screenshot attachment (corky 0.12.1) — screenshot file path needed

<!-- /agent:exchange -->
