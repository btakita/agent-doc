---
agent_doc_session: f8659a44-73f2-4885-a610-2750331ee9e5
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

How do I copy my JetBrains Idea settings to my current PyCharm settings?
I seem to have lost my PyCharm settings during an upgrade.

<!-- agent:exchange -->
### Copying IDEA Settings to PyCharm

Your system has:
- `~/.config/JetBrains/IntelliJIdea2025.3/` (rich — codestyles, colors, keymaps, options)
- `~/.config/JetBrains/PyCharm2025.3/` (sparse — lost settings)
- `~/.config/JetBrains/PyCharm2026.1/` (new — has some settings already)

**Two approaches:**

#### Option 1: Import via IDE (recommended)

In PyCharm 2026.1: **File > Manage IDE Settings > Import Settings...**
- Point it to `~/.config/JetBrains/IntelliJIdea2025.3/`
- Select what to import (keymaps, code style, colors, editor settings, etc.)
- Restart PyCharm

#### Option 2: Copy config dirs manually

The transferable directories (safe to copy from IDEA to PyCharm):

```bash
# Close PyCharm first!
SRC=~/.config/JetBrains/IntelliJIdea2025.3
DST=~/.config/JetBrains/PyCharm2026.1

# Keymaps, code style, editor colors
cp -r "$SRC/keymaps" "$DST/"
cp -r "$SRC/codestyles" "$DST/"
cp -r "$SRC/colors" "$DST/"

# Editor and IDE options (font, behavior, inspections, etc.)
# Be selective — some options are IDEA-specific
for f in editor.xml ide.general.xml editor.codeinsight.xml ui.lnf.xml; do
  [ -f "$SRC/options/$f" ] && cp "$SRC/options/$f" "$DST/options/"
done
```

**Don't copy:** `disabled_plugins.txt` (different plugin sets), `*.key`/`*.license` files, `jdbc-drivers/`, `bundled_plugins.txt`.

#### Option 3: JetBrains Settings Sync

If you have a JetBrains account: **Settings > Settings Sync > Enable**. This syncs across all JetBrains IDEs automatically. If it was enabled in IDEA, turning it on in PyCharm should pull the same settings.

Which approach do you want to try?

<!-- /agent:exchange -->
