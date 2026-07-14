# Reactive Stream Mode

Stream-mode documents (`agent_doc_mode: stream`) use reactive file-watching instead of the standard 500ms debounce. This eliminates latency between user edits and agent responses by leveraging CRDT merge for conflict-free concurrent writes.

## Architecture

### Debounced vs Reactive Mode

| Mode | Trigger | Debounce | Concurrency |
|------|---------|----------|-------------|
| **Debounced** (append/template) | File change | 500ms | 3-way merge |
| **Reactive** (stream) | File change | None | CRDT merge |

In debounced mode, the watch daemon waits 500ms after the last file change before processing. This prevents triggering on intermediate auto-save states but adds latency.

In reactive mode, the watch daemon processes file changes immediately. CRDT merge handles concurrent edits — if the user edits the document while the agent is streaming a response, both sets of changes are preserved via conflict-free merge at each 200ms flush interval.

### Final Write Cycle

```
USER EDITS                    AGENT STREAM                    DOCUMENT
│                              │                              │
│  ① file save                 │                              │
├─────────────────────────────►│  ② read + immutable baseline │
│                              │  ③ send to backend           │
│  ④ user keeps editing        │  ⑤ buffer partial chunks     │
├──────────────────────────────┤     + recovery sidecars only │
│                              │                              │
│                              │  ⑥ final chunk               │
│                              │  ⑦ validate complete payload │
│                              │  ⑧ one CRDT merge + closeout │
│                              ├─────────────────────────────►│
│                              │                              │
│  ⑨ next edit                 │  ⑩ next cycle                │
├─────────────────────────────►│                              │
```

### CRDT Merge at Final Write (Step ⑧)

Only the complete final response is merged. The transaction combines:

- **Baseline**: immutable document state saved before generation started.
- **Ours**: the complete validated assistant response.
- **Theirs**: current editor authority, including concurrent user edits.

The merged result preserves concurrent user edits while response placement,
queue/backlog changes, snapshot publication, and commit cross one closeout boundary.
Timer ticks never merge a cumulative response prefix into the document.

### Loop Prevention

Reactive mode still applies the same loop prevention as debounced mode:

1. **Convergence detection**: If the content hash matches the previous submit, skip processing
2. **Cycle counter**: Hard cap at `max_cycles` (default 3) agent-triggered cycles per file
3. **Agent-change detection**: Changes within 3× debounce window of last submit are treated as agent-triggered

Stream flushes write to the file, which triggers file-change events. The convergence detection and cycle counter prevent these from re-triggering the agent.

## Implementation

The watch daemon (`watch.rs`) tracks reactive paths via a `HashSet<PathBuf>`:

- `discover_entries()` marks stream-mode documents as reactive
- Stream-mode paths are added to both `watched_files` (for file-change events) and `stream_states` (for tmux capture polling)
- In the debounce check, reactive paths use `Duration::ZERO` instead of the configured debounce
- Every watched document keeps the last observed markdown projection in memory. On each file change, the daemon diffs the previous and current agent-component overlay and emits a `document_node_events` ops-log payload with `{component, node_key, op, item_id, before_index, after_index, before, after, previous_node_key, next_node_key}` for each changed item.
- Event `op` values are `insert`, `remove`, `replace`, `strike`, `unstrike`, and `move`. These node-keyed records are the realtime handoff for follow-up features such as backlog `:inbox_tray:` enqueue without relying on text-line matching.
- All other loop prevention mechanisms apply unchanged

## Configuration

Reactive mode is automatic — any document with `agent_doc_mode: stream` in its frontmatter gets reactive file-watching. No additional configuration is needed.

```yaml
---
agent_doc_mode: stream
agent_doc_stream:
  interval: 200
  target: exchange
---
```

## Truncation Detection

Reactive mode includes truncation detection (`wait_for_stable_content()` in `diff.rs`) as a secondary safety net. If the last added line looks like an incomplete sentence (mid-word, no terminal punctuation), the system rechecks the file every 200ms (up to 25 times / 5 seconds) before processing.

Fast-path bypasses ensure zero latency for common inputs:
1. Empty lines
2. Structural markers (`/`, `#`, `` ``` ``, `<!--`)
3. Single alphanumeric characters (choice selections: A, B, 1, y, n)
4. Single words ≥ 2 characters (commands: go, ok, release)
5. Lines ending with terminal punctuation

Only genuinely suspicious fragments trigger the recheck delay.

## Merge Call Path Diagram

All write-back paths build an overlay-aware merge base, then converge through `merge_contents_crdt()` before reaching the CRDT layer:

```
                         snapshot::crdt_merge_base_state()
                                       ▲
                                       │
                                  crdt::merge()
                                       ▲
                                       │
                              merge::merge_contents_crdt()
                                       ▲
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                   │
finalize.rs          write.rs              stream.rs
(finalize_stream) (apply_stream_           (stream_loop
final only)       from_string)             final save)
                    │                  │                   │
                    ▼                  ▼                   ▼
agent-doc          agent-doc            agent-doc
finalize --stream  repair               stream
```

- **`agent-doc finalize --stream`**: The skill-level final write-back path. It receives one complete response and owns document, queue/backlog, snapshot, and commit closeout.
- **`agent-doc repair`** (legacy alias: `recover`): Exceptional crash/restart recovery for orphaned final captures or stale cycle state; it is not part of healthy streaming.
- **`agent-doc stream`**: The real-time generation path. It buffers chunks and writes recovery-only partial checkpoints, then writes the complete final output once. Timer ticks never publish cumulative prefixes into the document.

All three prefer the structured `.overlay.yrs` markdown projection as the merge base when it matches the active cycle baseline. If the overlay sidecar is missing, corrupt, or stale, the merge falls back to the explicit baseline text and logs the fallback reason before calling `merge_contents_crdt()`.

## Truncation Detection

The `looks_truncated()` function in `diff.rs` uses a cascade of fast-path checks to determine whether the last added line is a complete thought or a mid-sentence fragment:

```
Input line
    │
    ├── empty/whitespace? ──── YES → not truncated
    │
    ├── starts with / # ``` <!-- ? ── YES → not truncated (structural)
    │
    ├── single alphanumeric char? ── YES → not truncated (choice: A,B,1,y,n)
    │
    ├── single word ≥ 2 chars? ── YES → not truncated (command: go, ok)
    │
    ├── ends with terminal punctuation? ── YES → not truncated
    │   (. ! ? : ; ) ] " ' ` * - > |)
    │
    └── OTHERWISE → potentially truncated
        │
        └── recheck chain: 200ms × 25 = 5s max
            ├── content changed → recheck again
            └── content stable → proceed with diff
```

Fast-path bypasses ensure zero latency for common inputs — only genuinely suspicious fragments (mid-sentence, no terminal punctuation) trigger the recheck delay via `wait_for_stable_content()`.
