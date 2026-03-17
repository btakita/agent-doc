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
- [x] Notifications: fixed and verified (dunst running, i3 config, DBus override)
- [ ] Reply email with screenshot (need file path)
- [ ] Confirm Slack notifications work (user test)
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

### Notification fix applied

1. Killed `notification-daemon` (PID 2692)
2. Started `dunst` (PID 935511) — test notification sent successfully
3. Added `exec --no-startup-id dunst` to `~/.config/i3/config` (line 39, after nm-applet)
4. Created DBus override at `~/.local/share/dbus-1/services/org.freedesktop.Notifications.service` pointing to `/usr/bin/dunst` — prevents `notification-daemon` from auto-activating

**Dunst config warnings** (cosmetic, non-blocking): deprecated `allow_markup`, `bounce_freq`, `geometry`, `startup_notification`, `frame`, `shortcuts` settings in dunstrc. Consider updating `~/.config/dunst/dunstrc` to modern format.

**Did you see the test notification?** If yes, Slack notifications should now work. Try sending yourself a DM from another device with Slack minimized.

I saw the test notification. But I don't have notifications from Slack.

Is my desktop environment now setup to use dunst?


### Re: Is my desktop environment now setup to use dunst?

**Yes — fully configured.** Three layers of persistence:

| Layer | Status | What it does |
|-------|--------|-------------|
| **Running process** | dunst PID 935511, DBus reports `dunst` v1.13.1 as notification server | Immediate fix |
| **i3 config** | `exec --no-startup-id dunst` in `~/.config/i3/config` | Survives i3 restart/reload |
| **DBus service override** | `~/.local/share/dbus-1/services/org.freedesktop.Notifications.service` → `/usr/bin/dunst` | Prevents `notification-daemon` from auto-activating on-demand |

Dunst will start on every i3 session and claim the notification DBus name before `notification-daemon` can.

Can we have an explicit stript or
 
<!-- /agent:exchange -->
