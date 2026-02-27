---
session: 22f64516-0a5f-4527-9037-9464d02cdf08
resume: null
agent: null
model: null
branch: null
---

In the slack linux client, Ctrl+Shift+u is bound to both Insert Hyperlink and Underline Selected Text.

Underline Selected Text wins in the hotkey conflict. How do have a hotkey to Insert Hyperlink?

## Assistant

According to [Slack's official shortcuts](https://slack.com/help/articles/201374536-Slack-keyboard-shortcuts), **Ctrl+Shift+U** is the Linux shortcut for "Hyperlink selected text." Underline (`Cmd+U`) is documented as Mac-only — there's no official Linux underline shortcut. Looks like a Slack bug where underline was bound to that combo on Linux too.

**Workarounds:**

1. **Ctrl+V with a link on clipboard** (recommended) — Select text, copy a URL to clipboard, then `Ctrl+V`. Slack hyperlinks the selected text instead of pasting over it. Official alternative per Slack docs.

2. **Slack markdown** — Type `<https://example.com|display text>` directly.

3. **Formatting toolbar** — Click the link icon in the message compose bar.

4. **System-level fix** — If `Ctrl+Shift+U` is also intercepted by IBus (Unicode code point input on GNOME/GTK), disable via `ibus-setup` or launch Slack with `GTK_IM_MODULE=`:
   ```sh
   GTK_IM_MODULE= slack
   ```

The **Ctrl+V** method is the most reliable workaround for the Slack-internal conflict.

## User

I start slack via dmenu. What is a good way to have the slack executable always have `GTK_IM_MODULE=` set for slack?

What is your response?

## Assistant

**Override the `.desktop` file** — dmenu reads XDG desktop entries, and `~/.local/share/applications/` takes precedence over `/usr/share/applications/`.

```sh
cp /usr/share/applications/slack.desktop ~/.local/share/applications/slack.desktop
```

Edit the `Exec=` line in the copy:

```diff
-Exec=/usr/bin/slack %U
+Exec=env GTK_IM_MODULE= /usr/bin/slack %U
```

That's it. dmenu sees the same "Slack" entry but launches it with the env var. Survives package updates (system file gets overwritten, your local override persists).

**Alternative: wrapper script** at `~/.local/bin/slack`:

```sh
#!/bin/sh
exec env GTK_IM_MODULE= /usr/bin/slack "$@"
```

But this depends on PATH ordering (`~/.local/bin` before `/usr/bin`). The `.desktop` file override is more reliable.

## User

Please write the wrapper script. The .desktop didn't seem to work.

## Assistant

Written and installed at `~/.local/bin/slack`:

```sh
#!/bin/sh
exec env GTK_IM_MODULE= /usr/bin/slack "$@"
```

Verified: `which slack` now resolves to `/home/brian/.local/bin/slack` (takes precedence over `/usr/bin/slack`). Restart Slack for it to take effect — next launch via dmenu will use the wrapper.

## User

The .local/bin/slack update did not work. Hotkey still captured.

## Assistant

Confirmed — this is a **Slack-internal keybinding conflict**, not a system-level intercept. `GTK_IM_MODULE` only fixes IBus/Unicode input; it can't resolve a conflict inside Slack's Electron app. Slack doesn't expose keybinding customization, so `Ctrl+Shift+U` can't be rebound.

**Best workaround: Ctrl+V**

1. Copy the URL to clipboard
2. Select the text you want to hyperlink
3. `Ctrl+V` — Slack detects URL + selected text → creates hyperlink

Or click the chain/link icon in the formatting toolbar (6th icon).

The wrapper script at `~/.local/bin/slack` can be removed — it doesn't help for this issue. This is a Slack bug worth reporting to Slack if it matters enough.

## User

search the internet for a bug report if any

## Assistant

**No existing bug report found.** Searched broadly — no one has publicly reported the `Ctrl+Shift+U` underline vs hyperlink conflict on Linux.

**Context from the search:**
- [Slack's docs](https://slack.com/help/articles/201374536-Slack-keyboard-shortcuts) list `Ctrl+Shift+U` as the Linux hyperlink shortcut. Underline is only documented for Mac (`Cmd+U`).
- Multiple sources say [Slack doesn't support underline](https://everythingwhat.com/can-you-underline-in-slack) at all — but your toolbar clearly shows a `U` icon. This suggests underline was added recently to the Linux client, creating the keybinding collision. It's likely a newer regression.
- The only related Linux reports are about the [system-level Ctrl+Shift+U unicode conflict on Fedora/GTK](https://discussion.fedoraproject.org/t/disabling-unicode-code-point-keystroke-ctrl-shift-u-on-fedora-35/78685) — a different issue.

**Slack has no public bug tracker.** Reports go to `feedback@slack.com`. This seems worth reporting since `Ctrl+Shift+U` is the *documented* hyperlink shortcut that they broke by adding underline to the same binding.

## User

Compose an email with the bug report. Include the screenshot I gave you.
