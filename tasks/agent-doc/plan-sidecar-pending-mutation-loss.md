# Plan: Sidecar fallback drops pending mutations

## Problem

When `finalize --stream` is called with `--pending-add` flags, pending mutations are applied to the on-disk file before the IPC stream path runs. But `content_ours` is computed from the baseline (pre-mutation state). When the sidecar normalization verification fails and the binary falls back to `content_ours`, the pending mutations are lost because `content_ours` lacks them.

**Repro:** `finalize --stream --pending-add "id=foo text"` when the IPC sidecar normalization diverges (e.g., sidecar strips `❯ ` prefixes). The exchange response survives (sidecar applies it independently) but backlog items disappear.

**Observed:** `tasks/claudescore-3.md` on Apr 29, 2026 — 13 `--pending-add` flags passed, committed response present, all 13 backlog items missing from HEAD. Reproduced twice with identical `sidecar normalization diverged — falling back to content_ours` message.

**Workaround:** `agent-doc write --commit --pending-only` (bypasses IPC stream path).

## Root Cause

In `src/write.rs`:

1. **Pending mutations applied to disk** (lines ~519-563): `pending_cmd::add_many()` modifies the file before `run_stream()`.

2. **`content_ours` computed from baseline** (lines ~3176-3185): Built from `baseline + patches` only. Does not include the pending mutations already on disk.

3. **Sidecar normalization check** (lines ~4130-4157 socket, ~4500-4532 file): When `verify_sidecar_normalization()` fails, falls back to `content_ours`.

4. **`splice_pending_component()` exists** (lines ~5000-5051) and transfers `agent:backlog` / `agent:pending` from one document version to another. But it's only called on the **IPC timeout path** (line ~3410), NOT on the sidecar normalization divergence fallback path.

## Fix

### Step 1: Splice pending into fallback

In both the socket IPC path (around line 4136) and the file IPC path (around line 4530), when falling back to `content_ours` after sidecar normalization divergence, call `splice_pending_component(content_ours, current_on_disk_content)` before using it as the snapshot. This transfers the pending mutations from the on-disk file into the fallback version.

The current on-disk content still has the pending mutations from step 1 (they were written before IPC started).

### Step 2: Add regression test

Add a test that:
1. Starts with a document that has an `agent:backlog` component
2. Calls `finalize --stream --pending-add "..."` 
3. Simulates sidecar normalization divergence (plugin writes ack_content_sidecar without proper `❯ ` prefixes)
4. Verifies that the committed document contains the pending items

### Step 3: Audit other fallback paths

Check whether the IPC file path and any other fallback scenarios also need the splice. The timeout path already calls `splice_pending_component`, so it's only the normalization divergence path that's missing it.

## Scope

- `src/agent-doc/src/write.rs` — add `splice_pending_component` call to both normalization-divergence fallback branches
- Test file for regression coverage
