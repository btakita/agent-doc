# Plan — prevent partial reap of completed multi-line backlog items

**Pending:** `#mlreap` in `tasks/agent-doc/agent-doc-bugs2.md`

- [ ] Reconstruct the exact repro where a user manually flips a tracked backlog parent line to `[x]`, preflight reaps that first line, and leftover continuation text remains in `agent:backlog`.
- [ ] Use the `tasks/software/tsift.md` `#9aep` / `#a7e3` drift as the primary specimen: it contains a completed parent item plus malformed flush-left transcript spill (`Commands:`, `Touched files:`, diff text) that survived after the parent line was removed.
- [ ] Trace the parser/reap path in `src/agent-doc/src/pending.rs`:
  - `PendingLayout::parse`
  - `parse_item_line`
  - `is_indented_continuation_line`
  - `reap` / `reap_with_items`
  - any later render/repair/history-replay path that can preserve or resurrect orphan continuation text
- [ ] Define the invariant: once a tracked backlog item is considered complete for reap, agent-doc must either
  - reap the entire logical item block in the same persisted cycle, or
  - fail closed with actionable normalization guidance before mutating the backlog.
- [ ] Decide how to treat malformed flush-left continuation spill from earlier patchback/transcript corruption. Partial reap that deletes only the tracked parent line while leaving orphan prose in `agent:backlog` is not acceptable.
- [ ] Preserve the existing good path for valid indented continuation blocks and nested subtasks. The fix must not regress the documented multi-line backlog contract for properly indented children.
- [ ] Inspect preflight/repair/session-check history-replay behavior so same-cycle manual `[x]` handling cannot remove the parent line and then let stale local/editor text reappear as detached backlog prose.
- [ ] Add regression coverage for:
  - a valid indented multi-line backlog item marked `[x]` and fully reaped
  - the malformed flush-left continuation repro that currently leaves orphan lines behind
  - the chosen fix path: either full-block cleanup or fail-closed normalization guidance
  - archive/history behavior when the completed multi-line item is reaped
- [ ] Update pending-system / runbook documentation only after the code-level invariant and regressions are in place.
- [ ] Patch the concrete outcome back into `tasks/agent-doc/agent-doc-bugs2.md` and reduce `#mlreap` only after targeted verification passes.
