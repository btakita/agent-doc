---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

In tasks/briantakita.me.md, the CRDT created duplicate data. Please add tests to replicate. Fix bug.
```
Why were the videos not public to begin with?
Cork

Why were the videos not public to begin with?
Corky should always publish public videos...unless explicitly to private.
```

<!-- agent:exchange -->

### Re: robert-ward.md CRDT corruption

**Severe character-level interleaving.** The CRDT merge algorithm interleaved the user's typing with the agent's streaming output character-by-character, producing garbled text like:
- `*C*Compacmpacted. CContent archived to` 
- `**Sot-bristle brush nly**` instead of `**Soft-bristle brush only**`

**Timeline:**
1. `e6c6488` (22:36) — User changed frontmatter. Clean.
2. `0774ba3` (22:37) — Agent response. Clean.
3. `4fc584b` (22:40) — Corruption. User typed questions while agent streamed response. CRDT merged at character level.

**Root cause:** The CRDT flush-to-file step failed during concurrent edits. The CRDT `.yrs` state and snapshot have the **correct** merged content, but the on-disk file received corrupted output. The character-level interleaving suggests the operational transform or diff-patch step broke when resolving concurrent insertions at overlapping positions.

**No content permanently lost** — the clean version is in the snapshot at `.agent-doc/snapshots/6d80de6e...md`.

**The "externally modified" issue:** The JetBrains plugin claimed the file but agent-doc's flush wrote to disk directly, bypassing the editor's file watcher. This triggers IDEA's "file modified externally" dialog.

This is a deeper CRDT bug than the previous fixes. The flush-to-document path needs to be reworked to properly handle concurrent character-level edits during streaming. Want me to investigate and fix?

Yes
<!-- /agent:exchange -->
