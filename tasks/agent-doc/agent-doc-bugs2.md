---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

FlowCore refactor is in maintenance mode; top backlog item: #ipcd.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260521-042016.md`*

Compacted content:
- Archived 3 response topic(s): #gxct/#wpkm tsift graph orchestration contracts; closeout repair; #dxtr/#wrcx tsift dispatch-trace closeout audit
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: lower-agent orchestration — gpt-5

Implemented the lower-agent orchestration MVP in `src/agent-doc`.

What changed:
- `agent-doc plan` now emits deterministic lower-agent routing fields: dispatch candidacy, task class, risk, parallelism, model tier, context budgets, write scope, required proof, dispatch mode, and tsift context commands/fallback policy.
- Added `agent_doc_dispatch` frontmatter support.
- Added `agent-doc jobs create/list/status/collect` with `agent-doc-job-packet-v1` markdown packets, optional operation docs, tsift context sidecars, and `agent-doc-worker-result-v1` result collection.
- Added `runbooks/manual-job-packets.md`, updated orchestration specs, README, SPEC, command autocomplete metadata, and changelog.

Verification:
- `cargo test jobs::tests`
- `cargo test build_plan_emits_lower_agent_routing_fields`
- `make check`
- `cargo install --path . --force`
- Installed-binary smoke: `agent-doc --version`, `agent-doc jobs --help`, and `agent-doc plan tasks/agent-doc/agent-doc-bugs2.md`
- Latest GitHub Actions CI run `26205142943` was green, including `Run make tmux-ci`.

Commits:
- `src/agent-doc`: `7c3063e feat: add lower-agent job packets`
- superproject: `ab919fa7b chore: update agent-doc submodule`

The concurrently added `#rvel` backlog item was already committed in the session document history and remains open.
<!-- agent:boundary:73aff3f8 -->
<!-- /agent:exchange -->
###
<!--
The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push
-->


<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#y9kk]
- do [#v44k]
- do [#ffsm]
- do [#rafe]
- do [#ek86]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#rvel] [recommended] Add `agent:review` document element for the review-then-done lifecycle (Rob-review rule in session-share). Move `[/]` (gated) items out of `agent:backlog` into a dedicated `<!-- agent:review -->` block; retarget `--pending-gate` to move items there; teach `--done` to accept both sources with new `review_done_guard` frontmatter switch (default off). Auto-migrate legacy `[/]` items via `agent-doc migrate`. Use case: session-share `tasks/root.md` has `#vtvc`/`#vtas`/`#vtmgr`/`#wdmgr`/`#lk2a`/`#vtbs`/`#qew8` floating as `[/]` in the backlog today, conflating queued work with review-pending. Plan: tasks/agent-doc/plan-agent-review-element.md
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
