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

### Flush Cycle

```
USER EDITS                    AGENT STREAM                    DOCUMENT
    │                              │                              │
    │  ① file save                 │                              │
    ├─────────────────────────────►│                              │
    │  (watch: immediate,          │                              │
    │   no debounce)               │                              │
    │                              │  ② read + diff + prompt      │
    │                              ├─────────────────────────────►│
    │                              │                              │
    │                              │  ③ send to Claude (stream)   │
    │                              │  ┌──────────────────────┐    │
    │                              │  │ Timer: every 200ms   │    │
    │                              │  │                      │    │
    │   ④ user keeps editing       │  │  ⑤ flush:            │    │
    ├──────────────────────────────┼──┤   read file (has     │    │
    │                              │  │   user edits!)       │    │
    │                              │  │                      │    │
    │                              │  │   CRDT 3-way merge:  │    │
    │                              │  │   base = baseline    │    │
    │                              │  │   ours = agent text  │    │
    │                              │  │   theirs = file now  │    │
    │                              │  │        ↓             │    │
    │                              │  │   merged = agent +   ├───►│ ⑥ atomic
    │                              │  │           user edits │    │   write
    │                              │  │                      │    │
    │                              │  │  (repeat every 200ms)│    │
    │                              │  └──────────────────────┘    │
    │                              │                              │
    │                              │  ⑦ stream complete           │
    │                              │  final flush + snapshot      │
    │                              ├─────────────────────────────►│
    │                              │                              │
    │  ⑧ next edit                 │                              │
    ├─────────────────────────────►│  ⑨ new cycle (immediate)     │
    │                              │  diff sees only new user     │
    │                              │  content (agent output       │
    │                              │  already in snapshot)        │
```

### CRDT Merge at Flush (Step ⑤)

Each 200ms flush reads the current file from disk, which may contain user edits made since the last flush. The CRDT 3-way merge combines:

- **Baseline**: document state saved before the agent started (step ②)
- **Ours**: cumulative agent response (replace into target component)
- **Theirs**: current file on disk (contains any concurrent user edits)

The merged result preserves both the agent's streaming output in the target component and user edits elsewhere in the document.

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

All write-back paths converge through `merge_contents_crdt()` before reaching the CRDT layer:

```
                                  crdt::merge()
                                       ▲
                                       │
                              merge::merge_contents_crdt()
                                       ▲
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                   │
            write.rs             write.rs              stream.rs
          (run_stream)     (apply_stream_           (stream_loop
           --stream)        from_string)             final save)
                    │                  │                   │
                    ▼                  ▼                   ▼
              agent-doc          agent-doc            agent-doc
              write --stream     recover              stream
```

- **`agent-doc write --stream`**: The SKILL-level write-back path. Used when Claude Code's `/agent-doc` skill writes a response to the document.
- **`agent-doc recover`**: Replays orphaned stream responses from `.agent-doc/pending/`. Used when a previous cycle was interrupted by context compaction.
- **`agent-doc stream`**: The real-time streaming path. Timer-based flush loop writes cumulative agent output to the document every 200ms.

All three converge through `merge_contents_crdt()` which handles CRDT state loading, merging, and persistence.

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
