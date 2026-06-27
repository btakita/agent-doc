# Plan: #closeoutstall - proven-live-editor closeout stalls direct Codex turn

**Status:** plan only - do not implement yet.
**Filed:** 2026-06-27 from `tasks/professional/equityfundingsource.md`.
**Backlog:** `#closeoutstall` in `tasks/agent-doc/agent-doc-bugs2.md`.

## Symptom

A direct Codex `agent-doc` turn on `tasks/professional/equityfundingsource.md`
stalled during closeout after the response was composed but before the session
document could be committed.

Observed sequence:

1. `agent-doc preflight` reported a stale snapshot/CRDT sidecar mismatch after
   compaction cleanup.
2. `agent-doc reset --from-current --preserve-session <file>` realigned sidecars.
3. `agent-doc write --commit --template --stream --origin skill <file>` failed
   twice:
   - socket IPC returned `ack status error`;
   - file IPC queued `.agent-doc/patches/<doc-hash>.json`;
   - no editor ACK arrived within the 2 second proof window;
   - write failed with `recovery=retry_without_disk_write`.
4. `agent-doc repair --apply-recovery <file>` said the document was clean.
5. `agent-doc doctor <file>` classified closeout as blocked:
   - `closeout_commit`: recoverable, because snapshot and HEAD disagreed;
   - `editor_convergence`: operator-gated, with an editor convergence failure
     marker in ops.log.
6. `agent-doc verify-op-capture <file>` succeeded, so the binary had live editor
   evidence and correctly refused an automatic disk write.
7. The Codex turn ended with the assistant response only in the queued patch
   payload, not durably appended to the session document or committed.

Net result: the agent obeyed the editor-buffer safety rule, but the workflow had
no first-class blocked-closeout handoff. The operator experienced this as "the
turn stalled."

## Root Cause Hypothesis

The write path has a safe fail-closed branch for a proven live editor:

- no ACK means do not write behind the editor;
- keep the file-IPC patch for retry;
- require operator/editor convergence before commit.

That is the right data-safety decision, but the direct Codex harness lacks a
terminal state that is both safe and actionable. It treats the failure as a
normal tool failure the agent must reason about manually. Once the model stops,
the durable session state is ambiguous:

- cycle/session-check says closeout is interrupted;
- `repair --apply-recovery` may report "clean";
- the response body is hidden in `.agent-doc/patches`;
- the target document does not carry an operator-visible handoff item;
- the Codex stop/final gate did not keep the turn in a recovery loop or surface a
  machine-readable operator action.

## Relationship To Existing Work

- `#fullboundary` gates supervisor queue-item dispatch on prior editor
  convergence and records loud force-disk playback. This bug is not the
  inter-item dispatch boundary; it is the *current turn's direct closeout* when
  a proven live editor does not ACK.
- `#kcb5` handles the editor-less CLI case where socket-connectable does not
  mean editor-present. This bug is the opposite: editor op-capture is proven, so
  automatic disk fallback must remain guarded.
- `#supselfheal` and stale-binary recycle work do not apply here; the inspected
  controller and supervisor matched the installed binary.

## Fix Direction

Add a first-class operator-gated closeout state for proven-live-editor no-ACK
instead of leaving direct Codex turns to hand-roll a blocked final response.

1. Introduce `CloseoutBlocked::EditorConvergenceRequired`.
   - Inputs: live editor endpoint/op-capture proven, response patch queued,
     missing ACK/no ACK after bounded retry, and no safe auto-recovery.
   - Persist: cycle id, patch id, patch path, doc path, snapshot/HEAD hashes,
     actor generation, supervisor pid/freshness, and the exact recovery command.
   - Do not mark the cycle as clean/committed.

2. Make `session-check` and `doctor` consume that typed state.
   - Report one canonical diagnostic, not a mix of "committed but snapshot
     differs" plus "repair clean".
   - Include copyable recovery actions:
     `save/resolve the live editor buffer, then run agent-doc write --commit`,
     or `agent-doc write --commit --force-disk` only when the operator explicitly
     accepts clobber risk.

3. Teach Codex/direct Stop-hook handling about the blocked state.
   - If the latest assistant response failed only because
     `EditorConvergenceRequired`, do not clear tracking as if the turn closed.
   - Surface a concise operator-gated closeout message and keep the pending
     response/patch metadata attached for retry.
   - Avoid an infinite self-retry loop while the editor has not converged.

4. Add an explicit retry command or recovery subcommand.
   - Candidate: `agent-doc closeout retry <file>` or extend
     `agent-doc repair --apply-recovery <file>` to recognize the typed blocked
     state and replay the queued response only after editor convergence proof.
   - The command should be idempotent: if the patch is already applied in the
     editor, commit it; if not, requeue/poll; if the editor is still blocking,
     return the same typed operator-gated diagnostic.

5. Keep force-disk loud and explicit.
   - Do not auto force-disk when live editor proof exists.
   - If the operator uses force-disk, record the existing loud playback artifact
     style from `#fullboundary`: patch id, hashes, ACK history, and why the
     operator override was used.

## Tests

- Unit: classify proven-editor no-ACK as
  `EditorConvergenceRequired`, distinct from editor-less `#kcb5`.
- Integration/SimWorld: direct Codex closeout with live editor proof and no ACK
  leaves a typed blocked state, preserves the response patch, and makes
  `session-check` fail with the canonical diagnostic.
- Integration: after the editor applies/saves the queued patch, the retry
  command commits the response and clears the blocked state.
- Hook test: Codex Stop hook does not report a successful closeout or clear
  tracking while `EditorConvergenceRequired` is active.
- Regression: proven live editor still prevents silent direct disk writes; only
  explicit `--force-disk` bypasses it, with loud playback.

## Acceptance

- A direct Codex `agent-doc` turn can no longer end in an ambiguous stall with
  the response only discoverable in `.agent-doc/patches`.
- The operator sees one typed, actionable blocked-closeout state.
- Retrying after resolving/saving the editor buffer commits the response through
  the normal binary-owned path.
- No regression of the live-editor safety invariant: no automatic write behind a
  proven live editor buffer.
