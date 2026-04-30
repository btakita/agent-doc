# Plan — icebox `do #id` closeout must resolve tracked work, not backlog-only work

## Problem

Completing a task that lives only in `agent:icebox` currently leaves the item open. The `do #id` closeout guard and the `--pending-done <id>` mutation path are both backlog-only, so an icebox item like `#tmv7` can be fully implemented and still survive the cycle unchanged.

## Confirmed root cause

- `session_check::detect_missing_pending_done_ids()` only scans `open_pending_ids()`, and `open_pending_ids()` only reads `agent:backlog`.
- `pending_cmd::done()` only resolves the canonical backlog/pending component.
- The write-layer pending normalization path also rewrites only backlog/pending content.
- The skill/runbook contract says `do #id` should not leave completed tracked work open, but that contract is currently true only for backlog items.

## Desired behavior

- `do #id` completion detection should treat open ids in both `agent:backlog` and `agent:icebox` as tracked work.
- `--pending-done <id>` should succeed when the id is in either live list surface.
- Completing an icebox item in a strict closeout should mark it done and let the existing closeout maintenance reap/archive it in the same persisted cycle, just like backlog items.

## Implementation steps

- [ ] Broaden the guard helper in `src/session_check.rs` from backlog-only open ids to tracked-work ids across backlog + icebox.
- [ ] Extend the done mutation path in `src/pending_cmd.rs` so it can find and mark an id in either backlog or icebox without rewriting the wrong component.
- [ ] Audit the write normalization path in `src/write.rs` for any backlog-only assumptions that would still drop or ignore done ids originating from icebox.
- [ ] Update the skill / runbook language so the `do #id` closeout rule explicitly covers `agent:icebox` items too.
- [ ] Add regression coverage for:
  - `session-check` / pre-commit pending-done detection on an icebox-only id
  - `--pending-done <id>` against an icebox item
  - full closeout reaping of a completed icebox item in the same cycle

## Verification

- [ ] Targeted Rust tests for the new guard + mutation helpers
- [ ] Relevant end-to-end `agent-doc` closeout test covering an icebox-only `do #id` cycle
- [ ] Full `cargo test` in `src/agent-doc`
