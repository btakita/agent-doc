# Queue Auto-loop and Supervisor Handoff

Load this runbook when `preflight.queue_active` is true, when queue continuation
fields are present, or when dispatching queue work to subagents. The binary's
typed fields are authoritative; never re-derive drainability from queue prose.

## Continuation decision

After a successful committed closeout and an OK `session-check`, continue only
when the emitted contract says work is drainable:

- `queue_active == true`;
- `queue_continuation_required == true`;
- `queue_drainable_head_count > 0`;
- `queue_trigger` is `auto` or `persisted`;
- `queue_prompts` is non-empty; and
- `user_intent_prompt_changes` is empty.

`queue_continuation_required` and `queue_drainable_head_count` are the binary's
in-session drainability projection (`#cleardrainsignal`, `#qchurn`). Do not loop
when they say no work is drainable. A real exchange prompt preempts the queue;
managed-component bookkeeping and a proven turn-independent edit do not.

Read `ui_outcome` before describing a stop:

- `deferred_for_supervisor_drain` / `yield_to_supervisor_clear_and_continue`:
  end the turn for the supervisor and report the handoff, not an operator stall.
- `deferred_for_operator_proof` / `operator_proof_required`: report the required
  human proof and stop.
- `supervisor_drain_unavailable` / `drain_in_session_supervisor_unavailable`:
  drain the head in-session now.
- `no_drainable_work`: the queue is drained.

`queue_auto_clear_yield: true` is an intentional supervisor handoff. End after
the proven closeout; the ready supervisor resets context and dispatches the next
item. With auto-clear off or no ready supervisor, continuation remains true and
the current harness keeps draining. A context clear does not shrink the
document; only compaction reduces exchange accretion.

## Harness behavior

- **Claude Code:** when continuation is required, SUBMIT
  `/loop agent-doc <FILE>` as a real prompt with `ScheduleWakeup`. Do not invoke
  the `loop` Skill directly (`#loopskillnoadmit`); it would not run preflight or
  acquire the drain-owner lease. The submitted loop entry seals its own cycle
  contract (`#loopcontractseal`). The short-TTL `claude_loop` lease is acquired
  during admission and needs no manual `drain-claim` or release.
- **Codex:** its user-level `$CODEX_HOME/hooks.json` Stop hook drives
  continuation. With auto-clear and a ready supervisor, Stop yields so the
  supervisor restarts fresh and dispatches the next head.
- **OpenCode:** it has no `/loop` and no Stop hook. After a proven closeout with
  continuation true, immediately start the next binary-owned cycle and drain in
  the same turn until the binary emits a real stop condition or iteration cap.
- **Grok Build:** a `subagents` / `fan-out` queue does not drain inline. Dispatch
  every `queue_subagent_dispatch` entry immediately with native
  `spawn_subagent(background=true)`, one worker per item, then acknowledge each
  returned `subagent_id` with the entry's generation-fenced `start_command`.
  This dispatch is automatic on the first admitted cycle; it never waits for a
  follow-up operator prompt asking Grok to run the workers.

Both `agent_doc_queue_max_iterations` and
`AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` (default `50`) bound the loop.

## Never stall a drain

Do not defer a drainable item to a "fresh", "focused", or "fully verified"
cycle (`#drain-no-defer`). High session accretion is not a stop condition while
the queue is active. `[clean-session]` heads are drainable in place. Only a
genuinely operator-gated head, failed closeout, session-check interruption, or
lint-gate block stops in-session draining. Steering pending after a committed
response is next-cycle input, not a failed closeout (`#steerinterruptexit`).

The skip list is exhaustive after a proven editor delivery
(`#degraded-ipc-no-stall`). A timeout, missing Lazily visibility proof, or
`recovery=retry_without_disk_write` is not success: do not continue and do not
elect an attached-document disk write. The keyed worker retains the intent in
`state.db` and resumes after the registered endpoint recovers. Follow
`queue_continuation_guidance`; do not invent stop reasons.

## Subagent heads

The queue coordinator protocol is three-boundary and worker-fenced
(`#queueclaimworker`):

1. Run the dispatch entry's `claim_command`; `🟡` means reserved/dispatching.
2. Spawn the worker. Once the harness returns a concrete identity, run the
   entry's `start_command` with that identity and printed generation; `🚧` means
   worker-bound/running.
3. When that exact worker reports, run `queue report-complete` with the same
   generation and the complete worker response in `--response`; `☑️` means
   keep the live row because coordinator integration is still pending, while a
   wholly struck prompt tree is terminal and safe to delete. The marker is the
   worker-complete handoff/outbox. Integrate and persist the result in the same
   turn so closeout strikes the queue line and ends the claim.

Never infer running from a label, intended spawn, process poll, or heartbeat.
`queue claim --refresh --generation <N>` only extends an already-started attempt.
A generation binds to at most one worker and a worker to at most one live claim.
Use generation-matched `queue release` only for that attempt; add `--requeue` to
abandon it for a new worker. Expired attempts become visible `⚠️` lost state;
orphaned completion evidence is `🔄` and must be reconciled, not redispatched.

Claimed heads do not count toward continuation and must not be redispatched. If
all remaining heads are claimed or operator-gated, end quietly and wait for the
workers. The full dispatch, authorization-preamble, worktree, report, and
integration contract is in [respond.md](respond.md) (`#queuesubagents`).
