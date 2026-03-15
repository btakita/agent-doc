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

### Re: CRDT corruption in lazily-rs.md

**Root cause:** `run_stream()` in `write.rs` used the stored CRDT state from the *previous* cycle as the merge base. When the user edits the exchange section between cycles, this stored state is stale — it doesn't match the actual content either side diverged from.

When the agent replaces the exchange content (replace mode patch) and the user inserts text in the middle of the old content, the stale base causes `compute_edit_ops` (character-level diffs) to produce overlapping delete+insert operations. The Yrs CRDT interleaves these character-level insertions, splitting words like `decisions` → `de` + [user text] + `cisions`.

**Fix applied** (`write.rs:220-227`): Now uses a fresh CRDT state built from the baseline instead of loading stored state. The baseline is the exact content both sides diverged from, giving clean diffs:
- ours diff: agent's replace patch
- theirs diff: user's additions

**Tests added** (`crdt.rs`):
- `merge_replace_vs_append_no_interleaving` — reproduces the lazily-rs pattern (user inserts in middle of content that agent replaces)
- `merge_replace_vs_append_with_baseline_base` — verifies baseline-based merge gives clean results

All 346 tests pass.

**lazily-rs.md:** The existing corruption needs manual cleanup — the CRDT fix prevents future occurrences but doesn't retroactively repair the document.

I had as issue with agent-doc run and cwd:
```
❯ /agent-doc tasks/youtube/youtube.md

● Bash(agent-doc recover tasks/youtube/youtube.md 2>&1)
  ⎿  Error: Exit code 1
     Error: file not found: tasks/youtube/youtube.md

● Read 1 file (ctrl+o to expand)

● The file tasks/youtube/youtube.md doesn't exist. Would you like me to create it, or did you mean a different path?
```

I tried to run this file but the autoclaim did not show up in the tmux pane
<!-- /agent:exchange -->
