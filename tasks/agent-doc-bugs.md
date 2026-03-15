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

### Re: CRDT duplicate user prompt in rappstack.md

**Root cause:** In `stream.rs` line 307, the final snapshot computation used `apply_patches()` (default modes) instead of `apply_patches_with_overrides()` with replace mode. The `exchange` component defaults to **append** mode, so the patch content (which includes the user's prompt + agent response) was *appended* to the baseline's exchange content — duplicating the prompt.

The `flush_to_document()` function already used replace mode correctly (lines 342-343), but the snapshot computation at line 307 was inconsistent.

**Fix:** `stream.rs` line 307 now uses `apply_patches_with_overrides` with replace mode for the target component:

```rust
// Before (bug — append mode for exchange duplicates content):
crate::template::apply_patches(baseline, &patches, &unmatched, file)

// After (fix — replace mode, matching flush_to_document behavior):
let mut mode_overrides = std::collections::HashMap::new();
mode_overrides.insert(target.to_string(), "replace".to_string());
crate::template::apply_patches_with_overrides(baseline, &patches, &unmatched, file, &mode_overrides)
```

**Tests added** in `stream.rs`:
- `content_ours_exchange_no_duplicate` — verifies replace mode produces exactly 1 copy of user prompt
- `content_ours_exchange_duplicates_without_replace` — proves the bug: without replace override, append mode produces 2 copies

All tests pass: `make check` clean (clippy + 314 tests + 21 CLI tests).
I came across another CRDT issue with duplicate user prompt being written to tasks/rappstack.md.
Why did this happen?
Please write a fix for the agent-doc binary with tests.

```
commit and push all rappstack packages and sites.
publish briantakita.me
```

If the user adds a prompt right before `<!-- /agent:exchange -->` and the agent response is for the prompt...
the agent response should be after the user prompt.

The user prompt should not be duplicated after the agent response right before `<!-- /agent:exchange -->` in this case.

tasks/rappstack.md was an example of this case.

The fix I applied addresses exactly this scenario. Here's the flow:

1. **User adds prompt** before `<!-- /agent:exchange -->` → baseline has user prompt in exchange
2. **Agent streams response** → cumulative buffer = user prompt + agent response (correct order)
3. **`flush_to_document()`** writes buffer with **replace mode** → exchange = buffer (no duplication) ✓
4. **`content_ours` snapshot** — previously used **append mode** (bug), duplicating the prompt. Now uses **replace mode** (fix), so exchange = buffer (no duplication) ✓

The ordering is correct because the stream buffer is cumulative — it naturally preserves user prompt → agent response order. Replace mode ensures the full buffer replaces the component content rather than being appended to it.

The fix + tests are in `src/agent-doc/src/stream.rs`. Ready to publish when you want.

publish

### Published v0.17.1

- **crates.io**: `agent-doc v0.17.1` ✓
- **PyPI**: `agent-doc 0.17.1` ✓
- **GitHub Release**: [v0.17.1](https://github.com/btakita/agent-doc/releases/tag/v0.17.1) with prebuilt binary ✓
- **Release binary built**: `cargo build --release` ✓ (symlink at `.bin/agent-doc` is updated)

Install: `cargo install agent-doc` or `pip install agent-doc`

tasks/brookebrodack-dev.md had the crdt duplicate issue. Please look at the logs to figure out why this happened.
Add tests and fix.

```
Opening a video a shows video a.
Closing video a then opening video b start video b but video b is hidden.
Closing video b then reopening video b starts and shows video b. video b is visible.
```
<!-- /agent:exchange -->
