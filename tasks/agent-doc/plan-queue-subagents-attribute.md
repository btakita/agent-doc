# Plan: `subagents` / `fan-out` attribute on `agent:queue` (planattributeauto)

## Goal

`<!-- agent:queue preset="#spec-test-build-install-commit-push" subagents go -->`
dispatches every drainable queue head to its own background subagent. The
operator no longer has to prefix each line with `#subagent:`. The in-session
agent acts as the coordinator. It claims each head, dispatches it, merges the
result, and closes the item when the subagent reports back.

## What exists today (2026-10-03)

- Per-item intent: `agent_doc_queue::subagent_intent` recognises
  `#subagent(s)` / `#sub-agent(s)`, whether or not the document registers the
  tag. `PresetContext::classify` (`agent-doc-document-realtime/src/midturn_steering.rs`)
  classifies a line as `Subagent` in three cases: the line has a literal tag,
  an item preset asks for subagents, or the queue-level `preset=` asks for them.
- `subagent_dispatch_heads` lists only lines that are **new since the previous
  cycle's steering seed**. Preflight puts those lines in `queue_subagent_dispatch`
  and leaves them out of `selected_queue_prompts`.
- Claims (`agent-doc queue claim`, `agent-doc-queue-io/src/queue_claim.rs`):
  selection, drainability, and the Stop-hook re-entry all skip a claimed head.
  The default TTL is 2h.
- In practice: on 2026-10-03, `tasks/software/tsift.md` prefixed its queue lines
  with `#subagent:`. It worked. Each head was claimed and got its own worktree
  under `~/work/btakita/worktrees/`.

## Weaknesses of the per-item prefix (why we need a queue attribute)

1. **Dispatch is offered only once.** One cycle lists the head in
   `queue_subagent_dispatch`. If that cycle does not claim it (the model skips
   it, the turn is interrupted, or closeout fails), the next seed already
   contains the line. From then on the line drains **inline** in queue order,
   and nothing reports this.
2. **The claim TTL can expire while the subagent is still working.** If a
   subagent runs longer than 2h, its claim lapses. The head becomes selectable
   inline, and the work runs twice.
3. **`preset=` takes a single value.** `preset="#subagents"` cannot be combined
   with a pipeline preset such as `#spec-test-build-install-commit-push`, so the
   operator has to tag every line instead.
4. **Nothing limits concurrency or repo conflicts.** Every tagged head is
   dispatched at once. Two heads that touch the same repo each get their own
   worktree, but merge order, `Cargo.lock`/`VERSIONS.md` conflicts, and
   `make install` (which recycles supervisors) are left to the coordinator with
   no rules.
5. **Harness gap.** Only Claude Code has an in-session `Agent` tool. Codex,
   OpenCode, and Grok have no equivalent path.

## Design

Attribute: `subagents`, with `fan-out` as an alias. It works as a bare flag or
with a value: `subagents=N` sets the maximum number of concurrent claims
(default 3). It is valid only on the queue; add it to `QUEUE_ONLY_COMPONENT_ATTRS`.

- **Eligibility from current state.** While the attribute is present, *every*
  live, unclaimed, drainable head can be dispatched, not just new ones. A
  missed claim is offered again on the next cycle, which fixes weakness 1.
- **Precedence.** A line opts out with an `[inline]` tag or an
  `[operator-verify]` tag. Ordering directives (`after A do B`,
  `run these in order`) keep dependents in order: a head becomes eligible only
  after its predecessors have closed.
- **Concurrency cap.** Preflight emits at most `N - active_claims` heads in
  `queue_subagent_dispatch`, in queue order. Heads past the cap stay in the
  queue, but inline selection skips them so they wait for the coordinator.
  `queue_continuation_required` stays `false` while every remaining head is
  either claimed or waiting for a slot, so the turn ends quietly.
- **Claim heartbeat.** `agent-doc queue claim --refresh` extends a claim's TTL.
  The coordinator refreshes it as long as the subagent task is alive. Under
  `subagents`, an expired claim makes the head eligible for dispatch again. It
  never drains inline, so the work is not duplicated.
- **Integration is the coordinator's job.** Subagents commit to a branch in a
  worktree outside agent-loop
  (`feedback_agent_worktrees_outside_ide_project_inotify`). They do **not** run
  `make install` or a release. When a subagent reports back, the coordinator:
  1. rebases or cherry-picks the branch onto main;
  2. runs the preset's verification once;
  3. runs `make install` once per integrated batch;
  4. pushes;
  5. releases the claim.

  The next cycle then answers the head and runs `--done` for it.
- **Repo conflicts.** Two eligible heads with the same repo `write_scope` (taken
  from `agent-doc plan` for each item) are both dispatched to separate worktrees,
  but the coordinator integrates them one at a time. A head that `plan` marks
  `parallelizable: false` runs alone.
- **Harnesses without an `Agent` tool.** Codex, OpenCode, and Grok fall back to
  `agent-doc orchestrate --mode parallel` over the eligible heads, under the
  same claims and cap. If neither path is available, preflight emits a
  `subagents_unsupported_harness` warning and the heads drain inline.

## Phases

1. **qsubattr1: parse and classify.** Add `subagents` / `fan-out` (with
   optional `=N`) to the queue attributes and to `QUEUE_ONLY_COMPONENT_ATTRS`,
   with typo and misplacement warnings. `PresetContext` treats the attribute as
   queue-scope subagent intent; no `preset=` value is needed. Tests: every
   spelling classifies correctly, and the `[inline]` and `[operator-verify]`
   opt-outs work.
2. **qsubattr2: eligibility from current state, plus the cap.** Under the
   attribute, `pending_subagent_dispatch_for_content` drops the
   "new since seed" filter and applies the cap and ordering dependencies.
   Drainability and continuation treat capped and claimed heads as not
   drainable inline. SimWorld tests: a missed claim is offered again, the cap
   is respected, and `after A do B` holds B back.
3. **qsubattr3: claim refresh and expiry.** Add `queue claim --refresh`. Under
   `subagents`, an expired claim makes the head eligible for dispatch again
   instead of draining it inline. Test: a TTL expiry does not cause inline
   duplication.
4. **qsubattr4: coordinator contract in the skill.** Update SKILL.md and the
   respond/planning-dispatch runbooks for every harness. They cover how to
   claim, dispatch, and refresh; the integration order; one `make install`
   per batch; releasing then closing; and the `orchestrate --mode parallel`
   fallback for harnesses without an `Agent` tool.
5. **qsubattr5: dogfood.** Switch `tasks/software/tsift.md` from per-line
   `#subagent:` prefixes to `subagents=3` on the queue marker. Use
   `agent-doc queue claims` and ops.log to verify that each head was dispatched
   exactly once and that none drained inline.
