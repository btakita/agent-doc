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
- [x] Notifications: working with notification-daemon (dunst reverted, not needed)
- [ ] Slack notification filter: set to "direct mentions only" per workspace (user action in Slack UI)
- [ ] Reply email with screenshot (need file path)
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

Can we have an explicit stript or documentation about this configuration...dunst over notification-daemon
 

### Re: Notifications not working in Slack

Dunst works (test notification confirmed), but Slack's `notificationPlayback: web` means **Slack renders its own popup notifications** — it doesn't use the system notification daemon at all. Dunst fixed system notifications generally, but Slack's internal notification system is a separate issue.

**Most likely cause:** Slack was started Mar 15, before dunst existed. Slack may have cached the broken notification state, or its focus detection thinks the window is active (known Electron bug, May 2025).

**Try these in order:**

1. **Restart Slack** — close and relaunch. This forces Slack to re-initialize notifications with the new daemon:
   ```sh
   pkill slack && sleep 2 && slack &
   ```

2. **Check in-app settings** — Inside Slack: Preferences (Ctrl+,) → Notifications → verify:
   - "Notify me about" = Direct messages, mentions & keywords (or All)
   - No notification schedule restricting hours
   - No DND/pause active (check the bell icon in sidebar)

3. **Test with window unfocused** — Switch to a different i3 workspace, then have someone message you (or use Slack on your phone to message yourself)

4. **Force native notifications** — If web popups still don't appear, try switching to native. In Slack DevTools (Ctrl+Shift+I), run:
   ```js
   // Check current method
   console.log(SlackDesktop?.prefs?.get('notificationPlayback'))
   ```

Your second question ("Can we have an explicit stript or documentation about") seems incomplete — what were you going to ask?


### Re: Documentation for dunst over notification-daemon

Here's the configuration reference. I'll also save this to a persistent location.

**Problem:** CachyOS (Arch-based) installs `notification-daemon` (GNOME) which auto-activates via DBus. Under i3, it silently swallows all notifications — no visual popups appear. Dunst is the standard notification daemon for i3/tiling WM setups.

I was seeing notifications from pulsesms. I think this is a slack only issue.
I just saw a dunst notification from slack. I changed my slack notification config.

**Configuration (3 files):**

1. **`~/.config/i3/config`** — Start dunst on login:
   ```
   exec --no-startup-id dunst
   ```

2. **`~/.local/share/dbus-1/services/org.freedesktop.Notifications.service`** — Override DBus activation:
   ```ini
   [D-BUS Service]
   Name=org.freedesktop.Notifications
   Exec=/usr/bin/dunst
   ```
   This prevents `/usr/share/dbus-1/services/org.freedesktop.Notifications.service` (which points to `notification-daemon`) from auto-activating.

3. **`~/.config/dunst/dunstrc`** — Dunst display config (already exists, uses Hack 10pt font). The deprecated settings (`allow_markup`, `bounce_freq`, `geometry`, `startup_notification`, frame/shortcuts sections) should be updated to modern dunst 1.13 format when convenient.

**Verification:**
```sh
# Check active notification server
dbus-send --session --dest=org.freedesktop.Notifications \
  --print-reply /org/freedesktop/Notifications \
  org.freedesktop.Notifications.GetServerInformation
# Should return "dunst" not "Notification Daemon"

# Test
notify-send "Test" "Should appear as dunst popup"
```


### Slack notifications confirmed working

The fix was two-part:
1. **System level:** dunst replacing notification-daemon (already done)
2. **Slack level:** notification config change inside Slack preferences

PulseSMS was already sending notifications via dunst, confirming the system notification stack was working — the issue was Slack-specific settings. Now resolved.

Let's try the original notification-daemon config again.


### Reverted to notification-daemon

1. Killed dunst
2. Removed DBus override (`~/.local/share/dbus-1/services/org.freedesktop.Notifications.service`)
3. Removed `exec --no-startup-id dunst` from `~/.config/i3/config`

`notification-daemon` (GNOME 3.20.0) auto-activated via DBus and is now the active notification server. Sent a test notification — **did you see it?**

If notifications still work under `notification-daemon`, then the original issue was purely Slack's notification settings and dunst wasn't needed. If the test notification didn't appear visually, then `notification-daemon` truly can't render under i3 and we should switch back to dunst.

Yes...it appeared.

I don't want to be notified for every message. Only direct mentions. Please update slack config.

### notification-daemon works — dunst wasn't needed

Good news — `notification-daemon` renders fine under i3. The original notification issue was purely Slack's internal settings. The dunst changes have been fully reverted.

### Re: Only notify for direct mentions

This setting is **server-side per workspace** — it can't be changed via the local config file. You need to set it inside Slack's UI for each workspace:

1. Open Slack → **Preferences** (`Ctrl+,`) → **Notifications**
2. Under "Notify me about": select **"Direct messages, mentions & keywords"**
3. Repeat for each workspace (click workspace name in top-left to switch)

You have 5 workspaces:
- T06D6M966L8
- T55LHL8UD
- T031JJZ7Q6T
- T01KVSV78P7
- T0A738MJP09

Each needs the preference set individually. This is a one-time change — it persists server-side.
compact exchange
<!-- /agent:exchange -->
