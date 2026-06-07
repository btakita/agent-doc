# Plan: lazily-rs queued prompt patchback miss

## Problem

Live repro from `tasks/software/lazily-rs.md` on 2026-06-02:

```text
I added the prompt "Can `CellHandle::set(&ctx, value)` also apply to the multi-threaded Context?" to the queue during a previous turn. I did not see a response. If you completed it the response did not patchback into this document so it would be an #agent-doc-bug. If you did not process it already, proceed with the turn. If you did process it, write the response back into this document.
#spec-test-build-install-commit-push
```

The active queue also contained:

```text
preset #spec-test-build-install-commit-push
- Can `CellHandle::set(&ctx, value)` also apply to the multi-threaded Context?
```

Preflight correctly detected an unresolved prompt and an active free-text
auto-queue head, but `agent-doc plan` classified the cycle as
`execution_scope=plan_backlog_only` because the report included
`#agent-doc-bug`. That captured the bug report, but it also deferred the
actionable lazily-rs queue head without answering it in the same cycle.

From the document contract's point of view, a response does not exist unless it
crosses `finalize` / `write --commit`. A prior turn may have completed some work
or may have answered only in the console; either way the session document showed
no assistant response for this prompt.

## Desired Behavior

- A queued free-text prompt with a preset must not appear handled unless the
  session document contains the corresponding committed response or the queue
  head is explicitly left active with a deferral reason.
- A co-located `#agent-doc-bug` capture should not silently consume or bury the
  actionable queue head that triggered the report.
- If the agent already completed the work, the recovery path should be
  `agent-doc write --commit <file>` with a response body, not a console-only
  explanation or direct file patch.
- If no response body exists, the cycle should fail closed before reporting
  success and name the unresolved queue head.

## Related Work

- `tasks/agent-doc/plan-free-text-queue-owner-consume.md`
- `tasks/agent-doc/plan-response-missing-after-manual-closeout.md`
- `tasks/agent-doc/plan-empty-response-repair-write.md`
- `tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md`

This repro is narrower than those plans: the report mixes a missed response
repair request, an active free-text queue head, and a planning-only
`#agent-doc-bug` preset in one diff.

## Implementation Plan

1. Add a regression fixture for a template-mode document with:
   - a committed prior assistant response,
   - a new exchange-tail report containing `#agent-doc-bug`,
   - an active `agent:queue auto` free-text head with
     `preset #spec-test-build-install-commit-push`,
   - no assistant response after the new prompt.
2. Update `agent-doc plan` so `#agent-doc-bug` capture and actionable queue-head
   execution are represented separately. If the plan must choose
   `plan_backlog_only`, it should emit a deferral proof that names the exact
   queue head left for a later cycle.
3. Add a `session-check` guard for active free-text queue heads: after a cycle
   that saw the queue head, success requires one of:
   - the response heading/body for that head is committed,
   - the queue head is consumed through the binary-owned queue path,
   - the head remains active and the response states a concrete deferral reason.
4. Extend missed-response repair diagnostics so an already-completed but
   unpatched response points to `agent-doc write --commit <file>` and refuses an
   empty child-agent handoff.
5. Add queue-preset coverage proving `orchestrate --from-queue --plan` expands
   the preset while the normal cycle still preserves the unresolved queue head
   if execution is deferred by a bug-capture-only turn.

## Shared Primitive (cluster consolidation, 2026-06-02)

Auditing the lazily-rs queue cluster shows four backlog items all close on the
**same** missing mechanism — implementation-plan item 3 above. Build it once:

- `#lr-queue-response-miss` — a *seen* free-text queue head needs committed-
  response / consume / explicit-deferral proof. (Shares this exact plan file.)
- `#lr-queue-patchback-miss` (this plan) — same, plus the mixed `#agent-doc-bug`
  capture must not bury the runnable head.
- `#manual-queue-head-loss` — a free-text head lost *before* it was recorded as
  an active head (the adding cycle stalled).
- `#lr-queue-repair-adopt` — an adopted `write --commit` response must consume
  the *matching* free-text head atomically with the commit.

**Already shipped (do not re-implement):**
- Item 2 (plan deferral proof) → `plan_backlog_only_deferral_warning`
  (`src/plan.rs:422`, commit `4d0184e3`) names suppressed runnable directives.
- Item 4 (missed-response repair → `write --commit`, not empty child dispatch)
  → `run.rs:430` + the "empty response — nothing to write" path.

**The one remaining primitive — free-text queue-head provenance + proof:**
1. At preflight, record each active `agent:queue` head that is NOT a `do [#id]`
   directive: its normalized text, a content hash, and a source tag
   (user-authored vs generated/convergence). Persist alongside
   `cycle_state.active_queue_heads` (which today only carries `do [#id]` ids, so
   `check_queue_head_removal_guard` early-returns on free-text heads —
   `session_check.rs:2459`).
2. At closeout, for each recorded free-text head require one of: a committed
   `### Re:` response whose heading/body answers that head; a binary-owned
   consume of that exact head; or the head still present with a concrete
   deferral reason in the response. Else fail closed naming the head.
3. The same provenance record lets `write --commit` adoption consume the matching
   free-text head atomically (`#lr-queue-repair-adopt`) and lets the stall case
   restore a head that was never recorded (`#manual-queue-head-loss`).

Implementing 1–3 retires the open facet of all four items. `#lr-queue-response-
miss` and this plan are effectively the same work item.

## Tests

- `plan` regression: mixed `#agent-doc-bug` plus active preset queue head emits
  pending-add-to for the bug capture and explicit deferral proof for the queue
  head.
- `session-check` regression: a cycle cannot report clean success after seeing
  the free-text queue head unless response/consume/defer proof exists.
- `write --commit` regression: a supplied repair response attaches to the
  correct exchange boundary and can consume the matching free-text queue head
  without rerunning completed repo work.
- Negative regression: id-backed `do [#id]` queue heads still require the
  existing explicit done/gate/edit proof and are not consumed by broad heading
  matching.

## Exit Criteria

- The lazily-rs repro fails closed or leaves a clear queued deferral instead of
  silently losing the prompt.
- Missed-response recovery always crosses `finalize` / `write --commit`.
- `#agent-doc-bug` planning capture no longer masks the runnable queue prompt
  that motivated the bug report.
