---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
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
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260520-234158.md`*

Compacted content:
- Archived 4 response topic(s): graph semantic features; semantic similarity queue catchup; semantic graph extraction queue catchup; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Can tsift be used to assist planning agents to work on a different parts of the codebase in parallel, so there are no merge conflicts?
### Re: parallel planning with tsift — gpt-5

Yes, but I would frame it as conflict-reduction and conflict-screening, not a guarantee that Git will never produce a merge conflict.

`tsift` already has the right primitives for this: AST symbol extraction, call/import graph edges, per-submodule indexes, `communities`, `impact`, `diff-digest`, `context-pack`, and `graph-db evidence` packets with `worker_context` / `source_handle` rows. That is enough to let a planner turn backlog items or prompt targets into bounded write scopes before dispatching workers.

A practical workflow would be:

1. Refresh the graph/index: `tsift status --fix` or `tsift graph-db --path . refresh --json`.
2. For each proposed task, collect evidence: `tsift graph-db --path . evidence <backlog-id> --json` or `tsift --envelope context-pack <doc-or-task> --budget normal`.
3. Build a conflict matrix from the returned files, symbols, routes, tests, and neighboring graph nodes.
4. Assign workers only when their write scopes are disjoint enough: separate submodules are lowest risk, separate files are usually safe, separate symbols in the same file are medium risk, shared routes/config/tests/core helpers should be serialized or owned by one worker.
5. Put the chosen ownership directly into each worker prompt: owned files/symbols, read-only context, forbidden files, expected tests, and handoff handles to expand instead of rereading the world.
6. Before merge, run `tsift diff-digest --cached .` and `tsift impact .` to catch accidental overlap and shared test fallout.

The important limit is that `tsift` can identify likely overlap from source structure and session/backlog graph evidence, but it cannot prove semantic independence. Two changes can touch different files and still conflict through generated code, public contracts, migrations, config, tests, or formatting churn. So the right planner rule is: use `tsift` to make parallel work explicit and auditable, then fail closed when the graph shows shared ownership or when the final diffs overlap.
<!-- agent:boundary:93a38c16 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#atvp] Unblock crates.io publication for tsift by replacing the upstream git-only sift dependency, publishing a compatible upstream crate, or vendoring a small stable search adapter; rerun cargo publish --dry-run before enabling TSIFT_ENABLE_CRATES_PUBLISH.
- [ ] [#g6kf] Confirm the v0.1.42 GitHub release completes after the macOS x86_64 Actions runner leaves the queue; if it remains stuck, remove or replace that matrix target and retag a fixed release.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
