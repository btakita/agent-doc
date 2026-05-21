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

Added graph/orchestration next steps; top tsift backlog item: #orchdog.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-050641.md`*

Compacted content:
- Archived 4 response topic(s): next steps; dogfood #orchdog; simpler #orchdog options; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
<!-- agent:boundary:1df3b3ed -->
<!-- /agent:exchange -->
###



<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#gdbevid]
- do [#cmgr]
- do [#gdbidx]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.**
- [ ] [#gdbhard] [recommended] Harden graph-db operator docs/fixtures around the current feature set: refresh/status/doctor/evidence/drift/convex-sync replay paths, stale Convex repair commands, and real agent-loop session examples that can be regenerated in CI.
- [ ] [#gdbevid] [recommended] `tsift graph-db evidence <backlog-id>` returns 0 rows for backlog ids present in a session document but lacking projection-linked context (worker_context / source_handle / semantic / worker_result all empty). Observed 2026-05-21 dogfood: `evidence x63e` + `evidence v4v0` both empty despite both items being canonical backlog rows. Likely fix: cross-link from session document `agent:backlog` items into the graph projection when they're first observed, even before any worker run; OR document the expected empty-state and adjust the operator messaging to distinguish "no context yet" from "broken projection."
- [ ] [#cmgr] `tsift conflict-matrix` defaults to `fail_closed=true` when individual targets lack source-ownership evidence, even when the **inter-target** conflict score is `low` / `0` (no file/symbol/test overlap). This is conservative-correct but operators see a confusing combined verdict: cross-item conflict says "safe to parallel" while per-item fail_closed says "don't dispatch." Suggest: surface the two signals as orthogonal fields in the output envelope (`per_target_fail_closed: [...]` + `cross_target_parallel_safe: bool`) rather than a single `can_parallel` field, OR have `can_parallel=true` once cross-target overlap is `low` and downgrade the per-target signal to a soft warning.
- [ ] [#gdbidx] tsift session-share workspace has 41 stale files and zero cached summaries — `tsift summarize --extract` has not been run for this submodule. Either auto-run on `graph-db refresh` when staleness exceeds a threshold, or fail-closed the refresh with a clearer "index stale: run `tsift summarize`" actionable message rather than the current warning that ships alongside successful refresh.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
