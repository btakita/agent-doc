# Plan — baseline invalidation when session document path changes (git mv)

**Pending:** `#pathmove` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

When a session document's file path changes mid-cycle (e.g., `git mv tasks/buildparty-investor-demo/ tasks/velvet-thunder/`), the preflight baseline becomes inaccessible because its hash is keyed to the old path. This forces the agent to fall back from `finalize` (which requires `--baseline-file`) to `write --commit`, which uses the `content_ours` fallback path and introduces secondary bugs (prefix stripping, potential content loss).

Reproduction from May 7, 2026:

1. Preflight saves baseline at hash `fbdde1a9...` (keyed to `tasks/buildparty-investor-demo/dev.md`)
2. Agent performs `git mv tasks/buildparty-investor-demo tasks/velvet-thunder`
3. `agent-doc finalize tasks/velvet-thunder/dev.md --baseline-file .../fbdde1a9...md` fails: `Error: failed to read baseline file — No such file or directory`
4. Agent falls back to `agent-doc write --commit` without baseline, triggering `content_ours` fallback

The baseline file was never moved because agent-doc's internal state (baselines, snapshots, cycle state) is keyed by a content hash of the file path. When the path changes, the hash changes, and all prior state becomes orphaned.

## Root Cause

- Baseline/snapshot/state file naming uses a hash derived from the document file path
- No mechanism exists to migrate state when the document path changes
- The `finalize` command has no fallback for missing baselines

## Fix Options

### Option A: Path-migration command
Add `agent-doc migrate-path <old-path> <new-path>` that rehashes and moves all state files (baselines, snapshots, cycle state, archives) from the old path hash to the new path hash.

### Option B: Finalize fallback
When `--baseline-file` is missing, `finalize` should fall back to snapshot-based comparison (similar to `write --commit`) rather than failing hard. This doesn't fix the orphaned state but prevents the cascade.

### Option C: Content-hash keying
Key state files by content hash of the document rather than path hash, so renames don't invalidate state. Requires migration of existing state.

**Recommended:** Option A (explicit migration command) as the primary fix, with Option B (finalize fallback) as defense-in-depth. The agent skill instructions should document that `git mv` of a session document requires `agent-doc migrate-path` before `finalize`.

## Tests

- Rename a document via `git mv`, then verify `finalize` still succeeds
- Verify `migrate-path` moves all state files correctly
- Verify orphaned state is cleaned up
