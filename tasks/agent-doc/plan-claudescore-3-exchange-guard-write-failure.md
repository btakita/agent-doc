# Plan: claudescore-3 exchange-guard write failure

## Problem

`agent-doc` can successfully:

1. detect and commit the new user prompt,
2. generate the assistant reply,
3. persist the reply payload into `.agent-doc/pending`,
4. and then fail during the template write/CRDT merge with:

`closing marker <!-- /agent:exchange --> without matching open`

That leaves the live session document missing the just-generated reply even though the tool has already committed the prompt-bearing turn and reported enough success to require manual recovery from `.agent-doc/pending`.

The May 1, 2026 `src/session-share/tasks/claudescore-3.md` reruns are the reference repro.

## Why this is separate from the older closeout bugs

- This is not only "patchback left uncommitted". The reply may never land in the live file at all.
- This is not only "boundary stale after answered turn". The writer itself rejects the merged template structure before closeout can even prove boundary correctness.
- The same repro family also exposed a UTF-8 indexing panic in component parsing against `❯`, so the write/repair path needs explicit hardening around both transcript structure and byte/char indexing.

## Desired behavior

Once `agent-doc` has generated a reply for a template document, it should do exactly one of these:

- write the reply into `agent:exchange`, preserve a valid template structure, and continue closeout, or
- fail closed before committing success, with resumable state and a deterministic repair path that can finish the same turn without manual document surgery.

Manual patching from `.agent-doc/pending` should not be required.

## Investigation targets

- [ ] Reconstruct the exact write chain for the `claudescore-3.md` repro:
  - snapshot commit
  - pre-response snapshot
  - boundary insertion
  - CRDT merge inputs (`base`, `ours`, `theirs`)
  - template parse/guard check
- [ ] Identify why the merged text can end up with a closing `agent:exchange` marker that the parser no longer considers matched.
- [ ] Trace whether the failure is caused by:
  - duplicated or reordered boundary insertion
  - merge-base drift after the prompt commit
  - component parser assumptions about marker positions
  - stale snapshot/pre-response state
  - mixed byte indexing vs char indexing in component slicing
- [ ] Reproduce the UTF-8 parsing panic deterministically with `❯` prompt markers and verify whether the same component/span code is also responsible for the unmatched-close error.
- [ ] Define the invariant for template writes:
  - `agent:exchange` must still parse after every binary-owned transformation
  - the response that reached `.agent-doc/pending` must either land in the live file or keep the cycle in a recoverable fail-closed state
- [ ] Decide where to harden first:
  - parser/span math in `component.rs`
  - CRDT merge post-processing / normalization
  - boundary insertion ordering
  - repair path adoption of pending responses when the response is already known-good
- [ ] Add a deterministic recovery path for this exact state:
  - if `.agent-doc/pending` contains a response for the active cycle and the only failure is post-merge template invalidity, `repair` should be able to finish the same turn automatically after canonicalizing the template
- [ ] Tighten success classification so the tool cannot report a completed/committed turn while the response is still stranded only in `.agent-doc/pending`.

## Regression coverage

- [ ] A template doc with ASCII prompt markers routes, writes, and closes normally through the current `claudescore-3` path.
- [ ] The same shape with `❯` prompt markers does not panic in parser/repair/write flows.
- [ ] A CRDT merge that previously produced the unmatched-close error now either:
  - writes a valid template successfully, or
  - fails closed with a precise resumable diagnostic and no false success.
- [ ] `agent-doc repair` can consume the pending response for this repro without manual patching.
- [ ] Submodule-owned session docs still commit against the correct owning root after the write fix lands.

## Likely code surfaces

- `src/agent-doc/src/component.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/crdt.rs`
- `src/agent-doc/src/session_check.rs`

## Exit criteria

- The `claudescore-3.md` rerun no longer requires manual recovery from `.agent-doc/pending`.
- UTF-8 prompt markers do not panic the parser.
- The writer preserves a valid `agent:exchange` component across commit + reply write + CRDT merge.
- The tool does not report success until the reply is either visible in the live file and committed, or the cycle remains explicitly fail-closed and resumable.
