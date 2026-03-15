---
agent_doc_session: e71ca19b-6167-4a51-a5e0-e4a4937b54bd
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

When I call `agent-doc run` with an inline prompt at the bottom of the `<!-- /agent:exchange -->`...the user prompt is duplicated in front of the generated response...and after...at the original position just before the `<!-- /agent:exchange -->`.

This just happened in tasks/rappstack.md. I deleted the duplicate prompt right before `<!-- /agent:exchange -->`:

> R2 custom domain. Switch the environment variable as well.
> Rename the R2 buckets to match the custom domain.

The frontmatter has:

```
agent_doc_format: template
agent_doc_write: crdt
```

<!-- agent:exchange -->

### Root Cause: `flush_to_document()` ignores baseline

In `stream.rs`, `flush_to_document()` takes a `_baseline` parameter but **ignores it**. Instead, it reads the current file state and applies the patch directly:

```rust
fn flush_to_document(
    file: &Path,
    text: &str,
    target: &str,
    _baseline: &str,  // <-- IGNORED
) -> Result<()> {
    // ...
    let content_current = std::fs::read_to_string(file)?;  // has user's prompt
    let content_patched = template::apply_patches_with_overrides(
        &content_current, &patches, ...  // patch also has user's prompt
    )?;
}
```

**The duplication path:**

1. User adds prompt at bottom of `<!-- agent:exchange -->` component
2. `agent-doc run` computes diff (includes user prompt), sends to agent
3. Agent's response includes/echoes the user's prompt text
4. `flush_to_document()` reads current file (user prompt is in exchange component)
5. Patch block (agent response with user prompt) is applied to current file in **replace mode**
6. Result: user prompt appears twice — once from the agent's response, once preserved at its original position

**Why only in CRDT/template mode:** Non-CRDT writes use 3-way merge (baseline/response/current) which would detect the overlap. Stream mode uses direct patch application without baseline comparison.

**Fix approach:** In `flush_to_document()`, when the file has diverged from baseline, use CRDT merge (like `write::run_stream()` does) instead of direct patch application. This is a 1-file fix in `stream.rs`.

Implement. Add to spec.

### Fix Implemented

**Root cause (refined):** Not in `flush_to_document()` (stream path) — the bug is in `write::run_stream()` (the `agent-doc write --stream` path used by the skill). The `exchange` component defaults to **append** mode, so when the agent's patch content is appended to the baseline's exchange content, the user's prompt appears twice: once from the baseline and once from the agent's response.

**Fix:** In `write::run_stream()`, force **replace** mode for the `exchange` component via `apply_patches_with_overrides()`. The `--stream` path receives the complete intended exchange content (not a delta), so append would duplicate content already in the baseline.

**Changes:**
- `src/write.rs` — `run_stream()`: switched from `apply_patches()` to `apply_patches_with_overrides()` with replace mode for exchange
- `SPEC.md` — added §7.21 documenting `agent-doc write` command behavior including the exchange replace-mode override

**Tests:** 333 pass (312 unit + 21 CLI), 0 failures. The existing `flush_cumulative_does_not_duplicate` test confirms no regression.
Implement. Add to spec.
Add automated tests
<!-- /agent:exchange -->
