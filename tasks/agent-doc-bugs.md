---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:exchange -->

### Re: Notification + why fallback exists

**Why the disk fallback exists:** The IPC path was added in v0.17.5 as an *optimization* — the disk write was the original (only) write path. IPC was layered on top as "try IPC first, fall back to disk if no plugin." The fallback ensures agent-doc works without the IDE plugin installed (terminal-only workflows, CI, etc.).

**The problem:** The fallback is too eager. When the plugin IS installed but slow to ACK (editor busy, indexing, etc.), it falls back to disk write within 2s and overwrites editor buffer changes.

**Better design:**

1. **No plugin installed** (`.agent-doc/patches/` doesn't exist) → disk write only (current behavior, correct)
2. **Plugin installed, ACK received** → IPC write (current behavior, correct)
3. **Plugin installed, ACK timeout** → NEW behavior:
   - Leave patch file in place
   - Print warning to stderr: `[write] IPC timeout — response saved to patch file, awaiting plugin`
   - Print to Claude session console (visible to user)
   - **Don't fall back to disk write**

**Notification:** The stderr warning is already visible in the Claude session terminal. For the editor, the plugin could show a notification balloon when it finds an unprocessed patch file older than 5s. This gives the user two signals: terminal warning + editor notification.

Can we prompt a option to do an external write after 5s?
Will this be a 3 way merge or a CRDT?

**Retry/force option:** Add `--force-disk` flag to `agent-doc write` for when the user explicitly wants to bypass IPC:
```bash
echo "response" | agent-doc write <file> --stream --force-disk
```

This keeps the default safe (no data loss) while giving an escape hatch.

Shall I implement this? It's changes to `write.rs` (IPC fallback logic) + the plugin (stale patch detection).
<!-- /agent:exchange -->
