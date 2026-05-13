# Plan: IPC Timeout Duplicate Patchback

## Bug Description

When `agent-doc finalize --stream` commits the response to git but the IPC notification to the plugin times out (2s), the plugin independently applies the patch from the fallback file, creating a duplicate response block in the working tree. Subsequent `write --commit` runs dedupe but does not cleanly remove the duplicate from HEAD.

## Reproduction

1. Run `agent-doc finalize <FILE> --stream --origin skill` with a response
2. IPC socket fails (timeout, plugin not listening)
3. Finalize falls back to file IPC (`patches/*.json`)
4. Git commit succeeds with single response
5. Plugin picks up the patch file and applies it again
6. Document now has two copies of the response
7. `write --commit` detects dedupe but result still has both copies in HEAD

## Evidence

- `tasks/root.md` session `1b08693f` on 2026-05-12
- Finalize log: `[write] IPC ack timeout (2s)`, `[write] IPC timeout — response saved as patch, awaiting plugin`
- Commit succeeded: `agent-doc(root): 2026-05-12 18:51:16`
- Document shows duplicate `### Re: business case enrichment + autoplay fix — opus-4-6` blocks (lines 123 and 149)

## Root Cause Analysis

Two independent write paths both succeed:
1. **Binary path:** finalize commits to git via hash_object+update-index
2. **Plugin path:** reads the fallback patch JSON and applies via CRDT/direct write

Neither path knows the other succeeded. The patch sentinel claim mechanism (`patch_id claimed`) is supposed to prevent double-apply, but timing allows both paths to complete.

## Fix Plan

### Phase 1: Prevent double-apply

1. After git commit in finalize, delete the fallback patch file immediately (not just write a sentinel)
2. Plugin should check git HEAD before applying a patch — if the response is already in HEAD, skip apply
3. Add a `committed_at` timestamp to the sentinel so the plugin can detect post-commit patches

### Phase 2: Robust dedupe in write --commit

1. `write --commit` dedupe should actually modify the file on disk, not just the staging area
2. After dedupe, verify the file content matches the staged content before committing
3. Add a session-check rule that detects duplicate response blocks and auto-repairs

### Phase 3: IPC reliability

1. Increase IPC timeout from 2s to 5s
2. Add retry with exponential backoff (2s, 4s)
3. If IPC ultimately fails, ensure the fallback path is the ONLY path (don't also commit from binary)
