# Plan — fix the recurring post-commit working-tree corruption

**Symptom (observed all of 2026-06-19 in `agent-doc-bugs2.md`):** every `finalize`
commits the CORRECT response to HEAD, then the working tree is mangled
post-commit — the new `### Re:` response is dropped from its correct exchange
block and fragments are spliced into an older response block. Recovery each cycle
was a manual `git checkout HEAD -- FILE` + `reset --from-current`.

## Root cause (two layers)

**Layer A — no disk fallback when the editor refresh fails (`#pcwcfailfix`).**
`git.rs::emit_postcommit_worktree_check` detects `match=false` +
`postcommit_worktree_lost_committed_content` (HEAD content dropped from the tree,
no new user work → HEAD authoritative). In the `#pcwcdiskfree` branch (a JB editor
listener is active) it **skips the disk write** and calls
`send_postcommit_editor_refresh` to push HEAD content into the live buffer, then
`return`s. But `send_postcommit_editor_refresh` swallows both failure outcomes —
`Ok(false)` (no ack) and `Err` (ack `status=error`) — as "non-fatal" and does
nothing else. Observed this session: `postcommit editor buffer refresh failed ...
IPC ack status error`. Result: disk write skipped AND editor refresh failed →
the corrupted working tree persists on disk and the mangled editor buffer is
unchanged, so both re-seed `content_ours` on the next cycle = perpetual loop.

**Layer B — `live_prompt_drift_after_preflight` adopts a mangled `content_ours`
every cycle (`#driftrootfix`, the `#semmerge`/`#smconv` converge family).** The
ack-content sidecar / editor CRDT buffer presented to the finalize guard has the
new response spliced into the wrong block. `guard_ipc_snapshot_adoption_against_live_prompt_drift`
adopts it for the snapshot (candidate → HEAD, content_ours → snapshot), so the
snapshot/editor buffer carry the corruption forward. This is the deeper converge
bug; Layer A stops the *persistence* of the corruption, Layer B stops it being
*produced*.

## Phases

### Phase 1 — `#pcwcfailfix` (high-confidence, contained; do first)
Make `send_postcommit_editor_refresh` return a success signal (`bool`/enum:
`Sent` vs `Failed/NoAck`). In the `#pcwcdiskfree` listener-active branch of
`emit_postcommit_worktree_check`, when the refresh does NOT succeed, fall back to
the authoritative HEAD **disk write** (`std::fs::write(file, &head_doc)`) — the
editor refused/failed to converge, so a one-time File-Cache-Conflict risk is
strictly better than a corrupted working tree that re-seeds the drift loop
forever. Log the fallback (`postcommit_worktree_auto_reconciled ...
transport=disk_after_failed_editor_refresh`). Keep the disk-skip ONLY when the
refresh is acked (`Ok(true)`).
- SimWorld coverage: listener-active + `lost_committed_content` + refresh returns
  NoAck/Err → assert the working tree is restored to HEAD on disk.
- Live-supervisor-critical: touches the live commit/postcommit path → clean
  focused session with `make check` + `make tmux-ci`.

### Phase 2 — `#driftrootfix` (deeper converge root-cause)
Root-cause why the editor/ack-content `content_ours` buffer has the new response
spliced into the wrong exchange block (the CRDT merge inverting/misplacing the
`### Re:` heading-prose turn under a live-buffer race). Make the converge place
the new response turn at the boundary position, not merged into a prior block.
Overlaps `#hap7`/`#gf5n`/`#semmerge`; coordinate with those review items. Needs
the focused converge-core session (clean context, careful reproduction).

## Verification
- Phase 1: deterministic SimWorld test for the failed-refresh disk fallback;
  `make check` green; one live finalize cycle shows the working tree matching HEAD
  with zero `postcommit_worktree_check match=false` persisting.
- Phase 2: a live edit-during-finalize cycle produces a `content_ours` that
  places the new turn correctly (no splice), `make check` green.
