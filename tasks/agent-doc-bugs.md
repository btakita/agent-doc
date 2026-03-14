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
### Re: CRDT duplicate data bug

**Root cause:** In `src/agent-doc/src/write.rs`, three functions (`run_stream`, `run_ipc`, `run_ipc_fallback`) save CRDT state incorrectly after a merge. They rebuild the CRDT state from `content_ours` (the agent's response without user edits) instead of using the merged `crdt_state` returned by `merge_contents_crdt()`.

This means:
1. Agent writes response → user edits concurrently → CRDT merge produces correct merged text
2. But the saved CRDT state only knows about the agent's text (missing user edits)
3. Next cycle: merge sees user edits as *new* insertions relative to the stale base → duplicates them

**Fix applied** — 3 locations in `write.rs` (lines ~227, ~374, ~415):
```rust
// Before (bug):
let crdt_ours = crate::crdt::CrdtDoc::from_text(&content_ours);
snapshot::save_crdt(file, &crdt_ours.encode_state())?;

// After (fix):
snapshot::save_crdt(file, &crdt_state)?;
```

**Tests added** in `merge.rs`:
- `crdt_state_includes_user_edits_no_duplicates` — simulates 2 merge cycles; verifies user edit from cycle 1 isn't duplicated in cycle 2
- `crdt_multi_flush_no_duplicates` — simulates streaming checkpoints with concurrent user edits across multiple flushes

All tests pass: `make check` clean (clippy + 317 tests + 21 CLI tests).

I came across another CRDT issue with duplicate user prompt being written to tasks/rappstack.md.
Why did this happen?
Please write a fix for the agent-doc binary with tests.

```
commit and push all rappstack packages and sites.
publish briantakita.me
```
<!-- /agent:exchange -->
