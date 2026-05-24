---
agent_doc_session: tsift-v0.1
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Completed: #gdbrefreshcache, #gdbtargetsnap, and #gdbpathprobe shipped in tsift c1c7668 and superproject f321eabef; GitHub Agent Doc CI is externally blocked before runner startup by the billing/spending-limit annotation.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260524-132906.md`*

Compacted content:
- Archived 2 response topic(s): graph-db backend gates; performance differences from last queue run
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps to improve sqlite graph performance?
### Re: sqlite graph performance next steps — gpt-5

Top active queue item: `#gdbrefreshfast`.
Top backlog item after this cycle: `#gdbpropscan`.

The current queue is already the right high-level priority order for SQLite graph performance:

1. `#gdbrefreshfast` - keep attacking cold/full refresh first. Cached refresh is now essentially free in the latest recorded comparison, but full projection still spends about `58.790s` in `source_graph_build` and `19.745s` in `projection_rows`, so backend switching should wait until this path is understood.
2. `#gdbedgescan` - tighten edge lookup and incident/property scan plans next. The standout current read-path cost is `edge_property_scan` at about `3.069s`, so I added `#gdbpropscan` as a focused backlog item to isolate that measurement instead of letting it hide inside the broader edge-scan work.
3. `#gdbhopcaps` - raise hop caps only after the synthetic deep-chain/high-fanout guards prove the normalized 64/128/256/512-hop timings stay bounded.
4. `#gdbfalkorgate` - keep FalkorDB behind the native promotion gate. The latest recorded numbers argue against claiming a backend win until projection loading, freshness/parity, lock behavior, and install cost are measured without replaying SQLite rows.
5. `#gdbvacproof` - keep this as review/gated operational evidence after the latency work, because compact/VACUUM proof affects file size, freelist, and tombstone policy more than the immediate query hot path.

Changed paths: `tasks/software/tsift.md` through `agent-doc finalize` only.
Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --from-exchange --plan`, `tsift status --fix`, `tsift --envelope context-pack`, `tsift --envelope search`, and targeted `rg`/`sed` reads of the tsift spec, done archive, and compacted exchange archive.
Verification: no code changed, so I did not run the project test suite; `agent-doc finalize` runs the document write/commit/session-check closeout.
Escalation: none.
Confidence: medium-high; this is based on the checked-in/archived performance records, not a fresh benchmark run.
❯ You read what was leftover in the queue. #agent-doc-bug
❯ The queue should have been cleared after the last run. #agent-doc-bug
### Re: inactive queue residue bug — gpt-5

You're right. I treated the stale `agent:queue` body as active priority context even though `queue_active: false`, and that let leftover queue state steer the `#next-steps` answer. The queue should either have been cleared after the prior queue run or treated as inert residue; it should not have been read as the current work order.

Plans created:

- Plan: `tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md`
- Plan: `tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md`

The first plan covers clearing stale non-auto queue residue after a queue drains. The second plan covers keeping inactive queue edits out of prompt targets and cross-document bug capture.

The queued `do [#gdbpropscan]` stays open. I did not execute it because this cycle was classified as `plan_backlog_only` for `#agent-doc-bug`, and the queue is inactive (`queue_active: false`).

Backlog items added to `tasks/agent-doc/agent-doc-bugs2.md`:

- [ ] [#qresidue] [recommended] Fix inactive `agent:queue` residue being read as active response context; implement `tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md`.
- [ ] [#qinactive] [recommended] Prevent edits inside inactive `agent:queue` blocks, such as `do [#gdbpropscan]`, from becoming prompt targets or cross-document `#agent-doc-bug` backlog captures; implement `tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md`.

Changed paths: `tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md` committed as `4120f4c52`; `tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md` committed as `e8adb5c`; `tasks/software/tsift.md` and `tasks/agent-doc/agent-doc-bugs2.md` through `agent-doc finalize`.
Commands used: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `agent-doc orchestrate --from-exchange --plan`, `tsift status --fix`, targeted `rg`/`sed` source reads, `agent-doc write --pending-only`, `git add`, and `git commit` for the non-session plan files.
Verification: no code changed; I did not run the project test suite. The plan files cite the specific regression tests to add when implementing the fix, and `agent-doc finalize` performs the session write/commit/session-check closeout.
Escalation: none.
Confidence: high on the diagnosis and plan boundaries; implementation still needs the planned code/test pass.
<!-- agent:boundary:9dde1cf3 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbrefreshfast]
- do [#gdbedgescan]
- do [#gdbhopcaps]
- do [#gdbfalkorgate]
- do [#gdbpropscan]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gdbpropscan] [recommended] Isolate the slow SQLite edge_property_scan path from #gdbedgescan: reproduce the latest roughly 3s fixture, capture EXPLAIN plus row-count/cardinality evidence, then prove any materialized edge-property index or scan-shape change with backend-eval and metric-digest before broader backend work.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
