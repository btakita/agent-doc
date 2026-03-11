---
agent_doc_session: 17c67299-0385-48ea-b3f9-8dd08aa08d22
tmux_session: '23'
agent_doc_mode: stream
---



# Stream Mode Test

<!-- agent:status -->
Testing stream mode with watch daemon (Option B)
<!-- /agent:status -->

<!-- agent:exchange -->
Hello from stream-test! This document is in stream mode.

The watch daemon should detect this document and poll the tmux pane for output.

To test:
1. Claim this document: `agent-doc claim tasks/stream-test.md`
2. The watch daemon auto-starts and begins polling
3. Run `/agent-doc tasks/stream-test.md` to trigger a response
4. Watch the document update in your editor in near-real-time

tell me how the agent-doc stream works

### How agent-doc stream works

**`agent-doc stream <FILE>`** spawns its own Claude CLI process and writes agent output back to the document in real-time using CRDT-based conflict-free merge.

**The flow:**
1. Validates document mode is `stream`, reads `StreamConfig` from frontmatter
2. Computes diff (same as template mode — snapshot-based, comment-stripped)
3. Builds prompt requesting patch-block format, spawns `claude -p --output-format stream-json`
4. Timer thread (default 2s) flushes accumulated text: `flock → read → CRDT merge → atomic write → unlock`
5. On completion: saves CRDT state (`.agent-doc/crdt/<hash>.yrs`), updates snapshot, optional git commit

**Key difference from template mode:** Template mode writes once at the end (or at checkpoints). Stream mode writes continuously every N ms, so text appears in your editor in near-real-time.

**CRDT merge vs 3-way merge:** CRDT (`yrs`/Yjs) tracks character-level insertions/deletions — the user can edit the document while the agent writes without conflicts. 3-way merge (template mode) works for sequential edits but can conflict during truly concurrent writes.

**To test the actual streaming pipeline:** Run `agent-doc stream tasks/stream-test.md` from a terminal. The `/agent-doc` skill runs inside the current Claude session and doesn't use the streaming write-back loop.
<!-- /agent:exchange -->
