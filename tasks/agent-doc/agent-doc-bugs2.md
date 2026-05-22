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

Added auto-DAG next steps; top backlog item: #autodag.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260522-002414.md`*

Compacted content:
- Archived 1 response topic(s): duplicate prompt corruption review
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ How can we "This does not mathematically eliminate duplicate prompt text from arbitrary manual Markdown editing"?
❯ #spec-test-build-install-commit-push
### Re: duplicate prompt residue — gpt-5

The way to make this rigorous is to stop treating arbitrary Markdown duplicates as something the binary may guess-delete. Safe, known residue gets normalized; anything else becomes a fail-closed state before route or closeout can claim success.

I implemented that boundary. Post-exchange HTML comment duplicates are still scrubbed while preserving the comment shell, but after that scrub the route path, editor-visible normalization, final template reconciliation, and IPC snapshot persistence now reject duplicate or near-duplicate prompt residue that remains in freeform post-exchange Markdown outside tracked components. The write path logs this through the typed document-mutation reason `duplicate_prompt_residue`, so it is audited by the FlowCore guard budget instead of being another untracked string check.

Changed paths in `src/agent-doc`:
- `SPEC.md`, `specs/07-closeout-commands.md`, `VERSIONS.md`
- `src/template.rs`, `src/write.rs`, `src/route.rs`, `src/route/tests.rs`
- `src/flow/document_mutation.rs`, `src/sim_world.rs`, `tests/test_cli.rs`

Verification:
- Focused regressions for template, route, and IPC duplicate Markdown residue passed.
- `cargo test duplicate_prompt --manifest-path src/agent-doc/Cargo.toml` passed.
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml` and `git -C src/agent-doc diff --check` passed.
- `make check` in `src/agent-doc` passed: clippy, 2245 default tests, integration tests, and deterministic closeout simulation.
- `make install` passed; `agent-doc --version` reports `agent-doc 0.33.16`.
- Latest upstream CI run `26257411546` was green, including `Run make tmux-ci`.
- Local `make tmux-ci` passed: 228 live tmux tests.

Commit / push:
- `src/agent-doc`: `43cbb79c` (`fix: fail closed on duplicate prompt markdown residue`) pushed to `origin/20260422/codex-agent-backend`.
- Root repo: `74a0f7c82` (`agent-doc: fail closed on duplicate prompt residue`) pushed to `origin/main`.

I left unrelated dirty files in the root workspace untouched; only the `src/agent-doc` gitlink was committed in the root repo.
<!-- agent:boundary:fb1929e4 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#autodag]
- do [#dagdispatch]
- do [#dagrecover]
- do [#daglogs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#autodag] [recommended] Add an agent-doc auto-DAG planner for queued do/compound directives: parse explicit depends-on text, backlog ids, graph/conflict evidence, and job packets into a topological schedule; fail closed on cycles, missing ids, stale graph evidence, or ambiguous ownership before dispatch.
- [ ] [#dagdispatch] [recommended] Execute auto-DAG batches through agent-doc orchestrate/jobs: run parallel-safe antichain batches, collect worker_result proof, unblock downstream nodes only after required proof/session-check passes, and stop deterministically on failed/gated nodes without launching dependents.
- [ ] [#dagrecover] [recommended] Add auto-DAG persistence/recovery semantics: write operation docs/job packets with schedule id, node state, attempt count, replay commands, and repair commands so interrupted DAG runs can resume idempotently after compact/finalize/restart.
- [ ] [#daglogs] [recommended] Use session-review guardrails as scheduler input: classify prompt-budget/cache-resend/restart-loop/noop-closeout log families before auto-DAG start and gate large or looping runs into compact/restart/fixture-fix steps.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
