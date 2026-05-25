# Plan: agent-doc cycle does not recognize response after manual prefix-strip recovery

**Bug ID:** `#adoc-prefix-strip-uncommitted`
**Bug class:** snapshot/baseline recovery
**Related:** [#adoc-jb-cache-cancel-stuck-cycle] (precondition), [#stuckcycledup], [#stuckcycleorphan], [#stuckcyclerepair]
**Severity:** medium — defeats the binary-owned closeout contract for the recovery path; forces bare `git commit` and leaves cycle state stale.

## Problem

When the JB-cache-conflict bug (`#adoc-jb-cache-cancel-stuck-cycle`) wedges a cycle, the user's recovery sometimes finds the response body in the working tree with **prompt-prefix markers (`❯ `) incorrectly applied to the agent's own response paragraphs** — an artifact of an IPC patch being applied with malformed marker handling. The user runs a manual `sed` (or equivalent) to strip the stray `❯ ` markers from the agent's response paragraphs.

After the strip:

1. The response body is structurally correct in the working tree (no stray prefix markers).
2. The user runs `agent-doc write --commit <FILE>`.
3. Either:
   - `Error: empty response — nothing to write` — the binary does not see the cleaned response as recoverable.
   - `Error: captured response baseline no longer matches current document` — the captured snapshot is out of sync with the post-strip file.
   - Or it succeeds in committing, but the next `agent-doc session-check` reports `INTERRUPTED: found likely direct response patchback without agent-doc cycle: <heading> (bare prompt target missing ❯ : <body>)` because the heuristic flags the response body paragraphs.
4. Recovery: the user falls back to `git commit -m "..."` directly. The response ships, but the agent-doc cycle state never closes cleanly — the next preflight sees the cycle as still open or recoverable, and may emit stale warnings for several subsequent cycles.

This was observed multiple times in this session — for example after commits `006f6c95d` and `a824e0e85`, where session-check kept flagging the committed response as a direct patchback even though the cycle reached `phase=committed, last_event=commit_success`.

## Root cause hypotheses

1. **The captured response sidecar holds the pre-strip body (with stray `❯ `).** After the manual strip, the live file no longer matches the captured body byte-for-byte, so the `captured response body is not present in the staged snapshot` guard fires and refuses the closeout.
2. **`session_check`'s `likely_direct_response_patchback` heuristic does not whitelist responses that were repaired by a prior `write --commit` recovery.** It flags any response body whose paragraphs start with non-`❯ ` text as "bare prompt target missing ❯ " regardless of whether the cycle is already `committed`.
3. **There is no `agent-doc repair --resync-capture` command** that lets the user say "the working-tree response is canonical; re-capture the snapshot/baseline against it and mark the cycle committed."

## Repro

1. Reproduce `#adoc-jb-cache-cancel-stuck-cycle` to wedge a cycle with malformed prefix markers.
2. Run `sed -i 's/^❯ \(Submodule\|All three\|Implemented\)/\1/' <FILE>` to strip the stray markers from agent response paragraphs.
3. Run `agent-doc write --commit <FILE>`. Observe `empty response — nothing to write` or `baseline no longer matches`.
4. Run `git commit -m "manual recovery"` to ship the response.
5. Run `agent-doc session-check <FILE>`. Observe `INTERRUPTED: found likely direct response patchback ...` despite HEAD containing the response.

## Plan

### Phase 1: Detection — distinguish post-strip recovery from a real direct patchback

**Files:** `src/session_check.rs`, `src/repair.rs`

1. When the cycle phase is `committed` and `last_event` is `commit_success` AND the working-tree response body differs from the captured body only by removal of `❯ ` prefix markers, treat as `recovered_via_prefix_strip` instead of `likely_direct_response_patchback`.
2. The `bare prompt target missing ❯ ` heuristic should not fire for response body paragraphs once the cycle has reached `committed`.

### Phase 2: Recovery command

**Files:** `src/repair.rs`, `src/main.rs`

1. Add `agent-doc repair --resync-capture <FILE>` that:
   - Reads the current file content.
   - Replaces the captured response body with the post-strip content.
   - Re-saves the snapshot.
   - Re-runs the commit boundary.
2. Surface this command in the error message of `agent-doc write --commit` when the captured body no longer matches the file but the structural difference is prefix-marker stripping.

### Phase 3: Tighten the captured response contract

**Files:** `src/capture.rs`, `src/write.rs`

1. Hash the captured body with the same normalization the file-IPC apply path uses (boundary markers stripped, prefix markers normalized). The `captured response body is not present in the staged snapshot` guard should compare normalized hashes.
2. This eliminates the false negative when the user's manual strip produces a structurally equivalent body.

### Phase 4: Plug the upstream bug

The whole chain only fires when `#adoc-jb-cache-cancel-stuck-cycle` produces malformed prefix markers in the first place. Phase 2/3 of that plan eliminate the upstream cause. Phase 1/2 of this plan are the defense-in-depth for any future regression.

### Phase 5: Regression coverage

1. Unit test: `session_check` recognises the post-strip recovery pattern and does not surface `likely_direct_response_patchback`.
2. Integration test: simulate the post-strip working tree and assert `agent-doc repair --resync-capture` reconciles cleanly.
3. SimWorld scenario for the full pipeline.

## Testing

- Manual: follow the Repro steps; assert `agent-doc session-check` returns OK after Phase 1 + 2 land.
- `make check` clean.
- New unit + integration tests pass.
