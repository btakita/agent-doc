# Plan: Fix `❯ ` prompt prefix stripping during finalize/write

## Bug

During `agent-doc finalize` or `write --commit`, user prompt lines with `❯ ` prefixes
can lose that prefix. The `repair` step in preflight catches and re-adds the prefix
post-hoc, but the stripping should not happen in the first place.

**Observed:** After finalize with "sidecar normalization diverged — falling back to
content_ours", the committed document had a user prompt line missing its `❯ ` prefix.
Preflight repair fixed it on the next cycle.

## Root cause

The write pipeline in `src/agent-doc/src/write.rs` has a sidecar normalization
verification step (line ~4584). When the plugin's IPC sidecar content doesn't preserve
`❯ ` prefixes, the binary falls back to `content_ours`. But `content_ours` is built
from the pre-normalization template patch output, which also lacks the prefixes.

The normalization targets (bare text without `❯ `) are extracted in
`extract_normalization_targets()` (write.rs ~2012) and baked into IPC patches via
`normalize_patch_content()` (write.rs ~5125). When the sidecar diverges and the fallback
fires, the normalization is lost because `content_ours` was captured before prefix
application.

## Fix approach

### Step 1: Apply normalization to `content_ours` before using it as fallback

In the sidecar fallback path (write.rs ~4599), run `normalize_user_prompts_in_exchange()`
on `content_ours` before using it as the effective snapshot. This ensures the fallback
content has the same `❯ ` prefixes that `normalize_patch_content()` already baked into
the IPC patches.

**File:** `src/agent-doc/src/write.rs` around line 4599
**Change:** Before returning `content_ours` as the fallback, apply the same normalization
that `normalize_patch_content()` uses, using the `normalize_prefix_lines` targets.

### Step 2: Add a test case

Add a test that:
1. Sets up a template document with a user prompt line (no `❯ ` prefix in raw content)
2. Runs the write pipeline with IPC sidecar divergence (mock the sidecar returning
   content without prefixes)
3. Verifies the `content_ours` fallback path still produces `❯ `-prefixed output

### Step 3: Verify repair is no longer needed for this case

After the fix, the repair step should not encounter stripped prefixes for the
finalize/write path. The repair code in `repair.rs` (~618-688) should remain as a
safety net for other edge cases.

## Files to modify

- `src/agent-doc/src/write.rs` — apply normalization to `content_ours` fallback
- `src/agent-doc/tests/` — add regression test for sidecar fallback prefix preservation
