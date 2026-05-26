# Plan: JB File Cache Conflict Accept Replays Stale Response Patch

**Bug ID:** `#jbccacceptdup`
**Bug class:** editor IPC / closeout idempotency
**Related:** `#adoc-jb-cache-cancel-stuck-cycle`, `#latefallbackloop`, `#stuckcycledup`, `tasks/agent-doc/plan-late-fallback-patch-redirties-committed-doc.md`

## Problem

On 2026-05-26, after `tasks/software/tsift.md` had already reached a successful
agent-doc closeout and root push for `#gsqlwrite`, the user accepted a JetBrains
File Cache Conflict dialog overnight. The document then became dirty again with
a second copy of the same response block:

- existing committed response: `### Re: #gsqlwrite - gpt-5`
- replayed working-tree response: `### Re: #gsqlwrite - gpt-5 (HEAD)`
- `agent-doc session-check tasks/software/tsift.md` reported a likely direct
  response patchback even though the response was already present in `HEAD`.

This differs from the known cancel-path bug: the user accepted the conflict, and
the stale editor/cache payload replayed after the successful commit boundary.

## Desired Behavior

Accepting a File Cache Conflict after an agent-doc cycle has already reached
`commit_success` must be idempotent. The plugin and binary should recognize that
the response body is already in the committed document and must not append or
replay a second response block.

## Fix Plan

1. Capture a deterministic repro fixture from the `tasks/software/tsift.md`
   incident: committed response in `HEAD`, late accepted editor/cache payload,
   duplicate response in working tree, and `session-check` direct-patchback
   warning.
2. Extend the JetBrains/file-IPC apply path so an accepted conflict revalidates
   against current disk and current `HEAD` immediately before mutation.
3. Reject or no-op any patch whose response body and target heading already
   exist in the committed exchange for the same document and cycle.
4. Add logging with patch id, cycle id, boundary id, document path, and reason
   such as `jb_cache_accept_duplicate_already_committed`.
5. Teach `session-check` to classify this accepted-conflict replay separately
   from a true direct manual patchback and to recommend the safe dedupe/commit
   recovery path.
6. Add SimWorld/editor regression coverage for the accept branch:
   successful closeout -> stale File Cache Conflict accept -> no duplicate
   response, clean session-check.

## Verification

- SimWorld scenario for accepted conflict replay.
- JetBrains plugin unit/integration test for accepted File Cache Conflict after
  commit success.
- `cargo test` for agent-doc.
- Manual repro in IntelliJ with a committed session document and a delayed
  accepted conflict.
