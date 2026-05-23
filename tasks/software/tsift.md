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
queue_active: true
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260523-222948.md`*

Compacted content:
- Archived 2 response topic(s): sqlite graph performance next steps; gdbfalkorreal location
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: graph backend cache queue — gpt-5

Completed the active graph backend queue and pushed it.

- `#gdbprepcache`: added disk-backed conflict-matrix preparation and graph/evidence reuse under `.tsift/conflict-matrix-cache`, with report statuses for `memory_hit`, `disk_hit`, and `computed`. The conformance test now verifies a second CLI invocation hits the disk prep cache and target-scoped graph evidence cache.
- `#gdbcurbase` and `#gdbfullreg`: refreshed the real agent-loop graph/index state and appended four benchmark reports to `fixtures/graph-db-performance-history.json`: three bounded FalkorDB candidate samples and one `--full-projection` sample.
- `#gdbfalkorreal`: kept FalkorDB behind `backend-eval`; the new real benchmark still shows the prototype Falkor path regressing on full real graph scans, so this is evidence for holding the gate rather than promotion.

Benchmark notes: `metric-digest` showed conflict-matrix preparation improved about 99%+ on the real agent-loop sample, including a 99.999% per-1k-graph-rows prep-rate improvement. The all-candidate/four-target run stayed too expensive after more than five minutes, so I used repeated single-target Falkor samples plus the full-projection sample for practical evidence. `tsift status --fix --json` still reported the overall workspace index as stale only because the configured `FDE-Project-Interview` scope path is missing; the tsift scope was refreshed.

Verification and release path:

- Local: `cargo check`, focused graph tests, full `make check`, and `cargo install --path .` passed; installed `tsift --version` is `tsift 0.1.42`.
- CI: tsift run `26345952453` passed both `clippy + test` and live Convex acceptance. The superproject `Agent Doc` run `26345957563` failed before any job steps were allocated for both `check` and `tmux-ci`, so I classified that as an external CI-start blocker rather than a red tmux regression.
- Commits pushed: `btakita/tsift@dd4a3fc` and superproject `btakita/agent-loop@4459e752a`.
<!-- agent:boundary:27ba4f5c -->
<!-- /agent:exchange -->
###
<!--
#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#gdbcurbase]~
- ~do [#gdbprepcache]~
- ~do [#gdbfullreg]~
- ~do [#gdbfalkorreal]~
- Please run a benchmark on the performance improvements for this queue run
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
