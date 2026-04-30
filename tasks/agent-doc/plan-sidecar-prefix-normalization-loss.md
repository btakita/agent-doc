# Sidecar `❯ ` Prefix Normalization Loss

| Field | Value |
|---|---|
| **Source** | `tasks/claudescore-3.md` session (2026-04-30) |
| **Bug class** | Same as `#splpend` — sidecar divergence fallback drops binary-side mutations |
| **Severity** | Medium — silent uncommitted diff; can cause phantom prefix diffs on next cycle |

## Problem

When `verify_sidecar_normalization` detects that the plugin's ack_content is missing `❯ ` prefixes on user prompt lines, the fallback to `content_ours` correctly preserves the prefix in the **snapshot** (used for commit staging) but does NOT reconcile the **working tree file**. The post-commit `try_ipc_reposition_boundary` only sends `boundary_id` — it does not resend `normalize_prefix_lines`.

## Reproduction

Observable in `finalize --stream` log:
```
[write] sidecar normalization diverged — falling back to content_ours (N bytes)
[write] snapshot from content_ours (N bytes)
[commit] skipping working-tree boundary reposition — IPC listener active
[commit] IPC reposition boundary signal sent
```

After commit, the working tree may show:
```diff
-Add expected scores + the calibration cycle to the plan.
+❯ Add expected scores + the calibration cycle to the plan.
```

as an uncommitted diff (or the reverse: HEAD has `❯ ` but working tree doesn't).

## Root Cause Chain

1. Binary applies `normalize_user_prompts_in_exchange_safe()` to `content_ours` → adds `❯ ` prefix (write.rs:3243)
2. Binary sends `normalize_prefix_lines` in socket IPC payload (write.rs:4069-4077)
3. Plugin applies patches + `normalize_prefix_lines` but writes ack_content before (or without) prefix normalization
4. `verify_sidecar_normalization` checks ack_content → missing `❯ ` → returns false (write.rs:2494-2511)
5. Fallback: `content_ours` used for snapshot only, not working tree (write.rs:4136-4148)
6. `stage_snapshot_for_commit` stages from snapshot via `hash-object + update-index` → committed content has `❯ ` (git.rs:2134-2161)
7. Post-commit `try_ipc_reposition_boundary` sends only `boundary_id`, not `normalize_prefix_lines` (write.rs:4416-4457)
8. Working tree retains plugin's version — may lack `❯ `

## Fix Options

### Option A: Resend normalize_prefix_lines in post-commit IPC (recommended)

Extend `try_ipc_reposition_boundary` to also send `normalize_prefix_lines` when the write cycle recorded a sidecar divergence. This lets the plugin re-apply the prefixes after commit, keeping the working tree in sync with HEAD.

**Files:** write.rs (`try_ipc_reposition_boundary`), git.rs (pass divergence state to post-commit)

### Option B: Write working tree directly on divergence

When `content_ours` fallback is triggered, also write `content_ours` to the working tree file (not just the snapshot). Risk: races with user typing in the IDE (same concern that led to IPC-only working tree updates — bug #xbs3).

**Files:** write.rs (divergence fallback path at line 4136)

### Option C: Fix plugin-side ack_content timing

Ensure the plugin writes ack_content AFTER applying `normalize_prefix_lines`, not just after applying patches. This would prevent the divergence from occurring in the first place.

**Files:** Plugin (claudescore-plugin or VS Code extension) — IPC handler

## Implementation Steps

- [ ] **Step 1** — Add integration test: verify that after a sidecar divergence fallback, the working tree file contains `❯ ` prefixes matching the committed blob. Currently untested.

- [ ] **Step 2** — Implement Option A: thread `normalize_prefix_lines` through to `try_ipc_reposition_boundary` when a divergence was recorded in the write cycle. Add a `divergence_recorded` flag or pass the lines directly.

- [ ] **Step 3** — Investigate Option C: audit the plugin's IPC handler to determine if ack_content is written before `normalize_prefix_lines` application. If so, fix the ordering. This prevents the divergence rather than recovering from it.

## Relationship to #splpend

Same bug class: `content_ours` fallback recovers snapshot but not working tree. #splpend is about pending mutations; this bug is about `❯ ` prefix canonicalization. Both would be fixed by Option B (writing `content_ours` to working tree on divergence), but Option A is safer for the prefix case because it's narrower and doesn't risk IDE keystroke loss.
