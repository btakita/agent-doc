# Plan — content_ours fallback strips ❯ prefix from user prompts

**Pending:** `#pfxcours` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

When the write pipeline falls back to `content_ours` (sidecar normalization divergence), the `❯ ` prompt prefix is stripped from user prompt lines in the committed document.

Reproduction from May 7, 2026 on `tasks/velvet-thunder/dev.md`:

1. User edits document to add `❯ Please rename the buildparty-investor-demo directory to velvet-thunder`
2. Preflight detects the addition (annotated as `[user+]`)
3. Agent processes the request, then runs `agent-doc write --commit` with `--stream`
4. Write pipeline logs: `[write] sidecar normalization diverged — falling back to content_ours (5926 bytes)`
5. Committed document contains `Please rename the buildparty-investor-demo directory to velvet-thunder` — prefix `❯ ` is gone

The `content_ours` fallback path reconstructs document content from the write pipeline's internal representation, which apparently does not preserve the `❯ ` prompt prefix on user-added lines that precede the new agent response.

## Root Cause Investigation

1. Trace the `content_ours` fallback path in `write.rs` (or equivalent) — identify where the document content is reconstructed
2. Check whether `content_ours` strips `❯ ` prefixes from lines classified as "user" content during reconstruction
3. Compare with the IPC (sidecar) path — does the sidecar path preserve `❯ ` prefixes correctly?
4. Check if the prefix is stripped before or after the content_ours serialization

## Fix

- The `content_ours` path must preserve `❯ ` prefixes on user prompt lines exactly as they appear in the working-tree document
- If the fallback reconstructs content from parsed components, the prompt prefix must be part of the canonical representation
- Add a regression test: write with `content_ours` fallback on a document containing `❯ ` prefixed user prompts and assert the prefix survives

## Related

- `plan-prompt-prefix-stripped-after-closeout.md` — similar symptom (prefix strip) but different trigger (closeout adoption path vs content_ours fallback)
- `plan-sidecar-prefix-normalization-loss.md` — prefix loss in sidecar normalization
- `#pfxleak2` — content_ours prefix leak + commit churn (related but inverse direction — prefix appearing where it shouldn't)
