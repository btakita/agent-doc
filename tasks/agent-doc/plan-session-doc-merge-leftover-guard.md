# Plan: Session doc raw three-way merge markers survive closeout

**Pending:** `#efsmrgmarkers` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft
**Bug class:** write / repair / IPC fallback / CRDT merge
**Severity:** high - commits a visibly corrupted session document

## Problem

On 2026-06-15, `tasks/professional/equityfundingsource.md` contained raw three-way merge marker text inside `agent:exchange` after an agent-doc closeout. The corrupted section surrounded the `### Re: conversation/system architecture review - gpt-5` response and included the marker strings `<<<<<<< agent-response`, `||||||| original`, `=======`, and `>>>>>>> your-edits`.

That shape should never survive a response-bearing write. A structured session document may CRDT-merge independent edits, or it may fail closed with a precise recovery message, but it must not commit raw conflict-marker syntax into the live exchange.

## Evidence

- The visible corruption was reported against `tasks/professional/equityfundingsource.md`.
- A subsequent `agent-doc preflight tasks/professional/equityfundingsource.md` repaired and committed the document as parent-repo commit `0eb0b2032` (`agent-doc(equityfundingsource): 2026-06-15 15:48:26`).
- That preflight also printed related symptoms:
  - stale route-owned supervisor serving the document
  - previous cycle stuck at `preflight_started`
  - `ipc_proof_insufficient ... source=file_ipc ... invariant=no_ack ... recovery=direct_write_fallback`
  - `postcommit_worktree_check match=false ... auto-reconciled to HEAD (#pcwc)`
- The repair commit removed the raw marker block from the EFS document, so the remaining work is a root-cause fix and regression, not another manual cleanup.
- While capturing this plan, `tasks/agent-doc/agent-doc-bugs2.md` also contained a committed partial scaffold around `### Re: EFS compact summary replay - gpt-5`: prompt-prefixed `agent-response` / `original` / separator marker lines plus a raw `your-edits` closing marker. That local scaffold was normalized in this plan-capture cleanup, and git history around commit `7a581a30d7` can seed a second regression.

## Goal

Ensure every response-bearing write, finalize, repair, and postcommit reconciliation path either:

- produces a valid template document without raw three-way conflict markers, or
- fails closed before commit with enough context to recover automatically or manually.

## Desired Behavior

- Raw marker strings such as `<<<<<<< agent-response`, `||||||| original`, `=======`, and `>>>>>>> your-edits` must be rejected or repaired before a session document is committed.
- The guard should be scoped to actual merge-marker structure, not ordinary prose that merely mentions marker strings inline.
- If a CRDT/file-IPC fallback cannot prove a clean merge, it should leave a recoverable pending patch or fail with a diagnostic instead of writing conflict text into `agent:exchange`.
- `agent-doc repair` should recognize this corruption signature and either remove the abandoned conflict scaffolding deterministically or report the exact block that needs operator intervention.

## Implementation Steps

- [ ] Reconstruct the EFS failure path from available artifacts:
  - `.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json` if still present
  - the preflight/recovery logs around cycle `cycle-1781552725167`
  - commit `0eb0b2032` and the pre-repair diff from git reflog/snapshot artifacts if available
- [ ] Identify which write surface allowed marker text through:
  - `finalize` response patchback
  - file-IPC fallback with no ACK
  - CRDT merge normalization
  - repair/preflight recovery
  - postcommit working-tree reconciliation (`#pcwc`)
- [ ] Add a structural post-merge invariant that scans the final template before commit for raw three-way marker blocks inside managed components.
- [ ] Decide the policy by context:
  - fail closed for ambiguous marker blocks
  - auto-repair only for a proven abandoned scaffold where exactly one side is empty and the assistant response side is intact
- [ ] Add `agent-doc repair` coverage for the EFS-style abandoned scaffold so an already-corrupted session can be recovered without hand editing.
- [ ] Add regression tests for:
  - clean CRDT/file-IPC fallback with no marker text
  - EFS-style `agent-response` / `original` / `your-edits` scaffold inside `agent:exchange`
  - literal marker strings mentioned in prose/list text, which must not be treated as a conflict block
  - postcommit reconciliation attempting to adopt a working tree that contains marker scaffolding
- [ ] Update closeout/repair specs once the fail-closed vs auto-repair boundary is settled.

## Files Likely To Change

- `src/agent-doc/agent-doc-orchestration/src/write.rs`
- `src/agent-doc/agent-doc-orchestration/src/repair.rs`
- `src/agent-doc/agent-doc-orchestration/src/merge.rs`
- `src/agent-doc/agent-doc-orchestration/src/template.rs`
- `src/agent-doc/agent-doc-orchestration/src/git.rs`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- A response-bearing closeout cannot commit raw three-way marker blocks into a managed session component.
- The EFS corruption shape is covered by a deterministic regression.
- `agent-doc repair` can recover or precisely report this marker-block corruption.
- Full `make check` passes in `src/agent-doc`.
