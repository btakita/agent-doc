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

Please draw a diagram from the write, diff, agent-doc stack, back to the document. Show what the crdt is doing.

### Stream Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                        DOCUMENT (file on disk)                  │
│                     tasks/stream-test.md                        │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  <!-- agent:exchange -->                                │    │
│  │  [user text] + [agent text growing in real-time]        │    │
│  │  <!-- /agent:exchange -->                               │    │
│  └─────────────────────────────────────────────────────────┘    │
└──────────┬──────────────────────────────────────┬───────────────┘
           │ READ                                  ▲ WRITE
           ▼                                       │
┌──────────────────┐                    ┌──────────────────────┐
│   1. SNAPSHOT    │                    │  5. ATOMIC WRITE     │
│                  │                    │                      │
│ .agent-doc/      │                    │ flock → write tmp    │
│ snapshots/       │                    │ → rename → unlock    │
│ <hash>.md        │                    └──────────┬───────────┘
└────────┬─────────┘                               ▲
         │                                         │
         ▼                                         │
┌──────────────────┐                    ┌──────────────────────┐
│   2. DIFF        │                    │  4. CRDT MERGE       │
│                  │                    │                      │
│ strip comments   │                    │ file_doc (YDoc)      │
│ compare snapshot │                    │   ← current file     │
│ vs current file  │                    │ agent_doc (YDoc)     │
│ → changed text   │                    │   ← buffered chunks  │
│                  │                    │                      │
└────────┬─────────┘                    │ merge_updates():     │
         │                              │   encode(agent_doc)  │
         ▼                              │   → apply to         │
┌──────────────────┐                    │     file_doc         │
│   3. AGENT       │                    │   → serialize to     │
│                  │                    │     markdown          │
│ claude -p        │                    │                      │
│ --output-format  │──── chunks ────►   │ User edits file      │
│  stream-json     │   (every ~50ms)    │ concurrently?        │
│                  │                    │ → No conflict.        │
│ StreamChunk {    │                    │   CRDT positions are  │
│   text: "...",   │──► buffer ────►    │   stable across both  │
│   is_final: bool │   (Arc<Mutex>)     │   writers.            │
│ }                │                    └──────────────────────┘
└──────────────────┘                               ▲
                                                   │
                                        ┌──────────────────────┐
                                        │  TIMER THREAD        │
                                        │  (every 2000ms)      │
                                        │                      │
                                        │  drain buffer →      │
                                        │  trigger step 4      │
                                        └──────────────────────┘

CRDT State Persistence:
  .agent-doc/crdt/<hash>.yrs
  ├── Saved after each stream completes
  ├── Loaded on next stream for continuity
  └── Compacted via `agent-doc compact` (GC tombstones)
```

**What the CRDT does at each tick:**
1. **Read** current file → `YDoc` (`file_doc`)
2. **Apply** buffered agent chunks → separate `YDoc` (`agent_doc`)
3. **Merge** — encode `agent_doc` as binary update, apply to `file_doc`. Yjs CRDTs use unique client IDs + logical clocks, so agent and user insertions never collide.
4. **Serialize** merged `YDoc` → markdown
5. **Write** atomically (flock + tmp + rename)

The user's editor and the agent are both "clients" in the CRDT sense. Each has a unique ID. Concurrent inserts at the same position resolve deterministically without data loss.

Is the poll time always 20s or will the buffer flush when there is new content before 20s?

### Poll timing

The default interval is **2 seconds** (2000ms), not 20s. The buffer flushes **strictly on the timer tick** — there is no early flush when new content arrives.

The timer thread uses `recv_timeout(timer_interval)` — blocks for `interval` ms, then checks if the buffer changed (`text != last_written`). If yes, flush. If not, sleep again. No notification channel from chunk consumer to timer — purely time-based polling.

**Configurable via frontmatter:**
```yaml
agent_doc_stream:
  interval: 500   # flush every 500ms for faster feedback
```

**Tradeoff:** Lower interval = faster editor updates but more file I/O (flock + read + CRDT merge + atomic write per tick).

**Could it flush early?** Yes — you could add a `Condvar` or `mpsc::channel` that the chunk consumer signals when the buffer changes, waking the timer thread immediately. That would give sub-second latency without the cost of frequent polling. Currently not implemented — the 2s default is a pragmatic choice.

Can we flush for every newline completed?

### Newline-triggered flush

Yes — flush on newline boundaries instead of (or in addition to) time-based polling.

**Implementation:** In the chunk consumer loop, after writing to the buffer, check if a new `\n` appeared. If so, signal the timer thread via a second `mpsc::channel`:

```rust
// Chunk consumer (main thread):
let had_lines = buf.matches('\n').count();
*buf = chunk.text.clone();
if buf.matches('\n').count() > had_lines {
    let _ = newline_tx.send(());  // wake timer
}
```

Timer thread changes from `done_rx.recv_timeout(interval)` to checking both the done channel and the newline channel, waking on whichever fires first.

**Tradeoff:** Gives line-by-line streaming in the editor (~1-3 flushes/sec matching Claude's output rate). I/O cost bounded by agent speed, not a fixed timer.

**Simpler alternative:** Set `interval: 200` (200ms). Catches most newlines within 200ms. Less code, nearly the same UX. The newline-trigger is more precise but requires more plumbing.

**Recommendation:** Start with the simpler `interval: 200` default, add newline-triggered flush as a follow-up if the latency isn't good enough. Both can coexist — the newline signal acts as an early wake, the timer acts as a fallback.

Sure...let's start with 200ms. Immediately build the binary and run 

### Default interval → 200ms

Done. Changed default from 2000ms to 200ms in three places:

1. `src/agent-doc/src/main.rs:258` — CLI `--interval` default value
2. `src/agent-doc/src/frontmatter.rs:8` — `StreamConfig` doc comment
3. `src/agent-doc/CLAUDE.md` — documentation

`make check` passes (281 tests, 0 failures). The frontmatter `interval` still overrides the default, so existing documents with `interval: 2000` won't change behavior.

Sure...let's start with 200ms. Immediately build the binary and run it.
Will this document now use the 200ms interval?

### Binary built & installed

Binary installed to `~/.cargo/bin/agent-doc`.

Also fixed a pre-existing build error: `find_project_root` was private in `snapshot.rs` but called from `watch.rs`. Made it `pub`.

**Will this document use 200ms?** No — the `/agent-doc` skill runs inside your current Claude Code session. It writes back once at the end via `agent-doc write --stream`, not via the timer loop. The 200ms interval only applies to `agent-doc stream tasks/stream-test.md` (which spawns its own Claude CLI process with the write-back loop).

**To test 200ms streaming:** Run this from a terminal:
```bash
agent-doc stream tasks/stream-test.md
```

Is it currently running at a 200ms interval?
I expect to see more updates coming from the claude code session.

### Not streaming right now

**No — there is no 200ms loop running right now.** You're using `/agent-doc` (the Claude Code skill), which writes the complete response once at the end. No streaming loop involved.

To get real-time 200ms streaming:
```bash
agent-doc stream tasks/stream-test.md
```
That spawns its own Claude CLI process with the 200ms timer loop.

| Approach | How it works | Update frequency |
|----------|-------------|-----------------|
| `/agent-doc` (skill) | Write once at end | 1 write per submit |
| `agent-doc stream` | Timer-based CRDT loop | Every 200ms |
<!-- /agent:exchange -->
