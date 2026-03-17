---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

<!-- agent:status mode=replace -->
- [x] Phase 1
<!-- /agent:status -->

<!-- agent:exchange mode=append -->

### Re: Audit docs + compact exchange

**Exchange compacted:** 1695 → 14 lines. Old content archived to `.agent-doc/archives/`.

**Audit docs:** 4 files stale relative to `src/route.rs`. Updating CLAUDE.md, README.md, SPEC.md, and SKILL.md with the v0.17.15 routing/sync changes (running in background).

Updates cover:
- Route auto-start for new files (no lazy-claim)
- Sync pre-phase auto-start
- Poll-based Enter retry protocol
- `❯` prompt detection
- `auto_start()` now public
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e7cc7f7a85d39c090242bb3a72d53651075f5c7f9a1053e25d29aa36657310cc-20260317-060504.md`*

`Run Agent Doc` on tasks/briantakita.me.md ran...but the response was not written back to the document in the last run.

### Re: briantakita.me.md response not written

**Root cause: IPC false ACK.** The pane shows:
```
[write] IPC patch consumed by plugin — snapshot updated
```

The IPC path was used (not `--force-disk`). The JetBrains plugin consumed the patch file (deleted it as ACK) but didn't actually apply the content to the document. This is the same IPC bug we fixed earlier in v0.17.14 — but the **plugin hasn't been rebuilt** with the fix.

The fixed plugin source is at `src/agent-doc/editors/jetbrains/` with the `applyPatch() → Boolean` fix, but the installed plugin is still the old version. Need to rebuild and reinstall:

```bash
cd src/agent-doc/editors/jetbrains
./gradlew buildPlugin signPlugin
```

Then install the new zip via IntelliJ: Settings → Plugins → Install from Disk.

**Workaround:** The SKILL.md should use `--force-disk` until the plugin is fixed. Want me to add that flag to the skill's write commands?

**Also:** The `compact` command didn't update the snapshot (still 1708 lines vs 30 in the file). This is a separate bug to investigate.

Add automated tests to replicate the issue.
<!-- /agent:exchange -->
