# Plan — hash-prefixed `--pending-done` closeout normalization

**Status:** Completed
**Date:** 2026-05-05
**Historical trigger:** `#9aep` in `tasks/software/tsift.md`

## Problem

`agent-doc`'s closeout guards and cycle-state bookkeeping already treated
pending ids canonically, but the actual pending mutation path still compared
raw strings.

That left one concrete failure mode:

- a response correctly attempted `--pending-done '#9aep'`
- `cycle_state` normalized that to `9aep`
- `pending_cmd::done()` / `pending::op_done()` still looked for the literal
  `#9aep`
- closeout failed with `id not found in backlog/icebox: #9aep`

The result was a false mismatch between the guard layer and the mutation layer:
the binary already knew which backlog item the response meant, but the pending
write path refused to mark it done.

## Resolution

Normalize pending ids at the mutation boundary the same way the rest of the
closeout path already does:

- trim surrounding whitespace
- strip one optional leading `#`
- lowercase before lookup

Applied that normalization across the id-based pending operations in
`src/agent-doc/src/pending.rs`, and normalized the component lookup used by
`pending_cmd::done()` in `src/agent-doc/src/pending_cmd.rs`.

## Verification

- `cargo test -p agent-doc pending::tests::op_done_accepts_hash_prefixed_id`
- `cargo test -p agent-doc write_pending_done_accepts_hash_prefixed_id --test pending_integration`
- `cargo test -p agent-doc finalize_accepts_hash_prefixed_pending_done_id --test finalize_integration`

## Follow-up

No open follow-up is required for this specific bug after the normalization
patch and regression coverage landed.
