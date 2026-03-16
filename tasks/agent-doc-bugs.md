---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Keep/replace history + CRDT-only writes

**Keep vs replace:** This is already handled by the component mode in `components.toml`:
- `mode = "append"` → keeps history (each response appends below)
- `mode = "replace"` → replaces (current behavior for status)

The user can switch per-document by editing `components.toml` or per-component inline. No new code needed — just convention.

**For history --restore:** Already prepends (keeps both old and current). To replace instead, could add `--replace` flag that replaces exchange content with the old version.

**External edit losing typed content — CRDT-only writes:**

This is the critical bug. When `agent-doc write` falls back to direct disk write (no plugin ACK), it overwrites the file including any unsaved editor buffer changes. The IPC path (plugin) uses Document API which preserves cursor and undo stack, but the disk fallback doesn't.

**Fix:** When the plugin is available (`.agent-doc/patches/` exists), **never fall back to disk write**. Instead:
1. Write the IPC patch file
2. Wait for ACK (current 2s timeout)
3. If ACK timeout: **leave the patch file** and print a warning, don't overwrite the disk file
4. The plugin will apply it when it next checks

This ensures user edits in the editor are never lost by a disk overwrite. The tradeoff: if the plugin is truly dead, the response stays in the patch file until manually applied. But that's better than losing user work.

Can we have a notification when this happens? Either in the editor or in the claude session.
The user should have an option to try CRDT again or do the external write.

Should I implement this IPC-priority fix? It's a change to `write.rs` — the `try_ipc` fallback logic.

Why did we attempt the fallback logic?
<!-- /agent:exchange -->
