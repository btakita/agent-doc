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

### Re: CRDT character interleaving fixed

**One-line fix** in `crdt.rs` line 136:

```rust
// Before (bug — could split formatting mid-line):
_ => mutual_prefix,

// After (fix — only advance to line boundaries):
_ => base_text.len(),
```

**Root cause:** The shared prefix advancement optimization (added in v0.17.2) could land on a shared formatting character like `*`. When the agent wrote `*Compacted...` and the user wrote `**Soft-bristle...`, the shared `*` was absorbed into the base, causing the CRDT to insert the agent's text between the user's `**` bold markers.

**Fix:** Only advance the base to a position after a `\n` character. If no suitable line boundary exists in the shared prefix, don't advance at all — keep the original base.

**Tests:** 2 regression tests. All 319 + 21 tests pass.

Ready to publish.

release
<!-- /agent:exchange -->
