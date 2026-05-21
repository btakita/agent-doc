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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260521-044005.md`*

Compacted content:
- Archived 1 response topic(s): do #aorp
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Update #next-steps in agent-doc-bugs2.md with latest tsift features.
❯ Add #next-steps to tsift.md for agent orchestration and graphdb features.
### Re: next steps — gpt-5

Captured the requested `#next-steps` backlog updates.

For `tasks/agent-doc/agent-doc-bugs2.md`, I added top item `#aggtp` for an agent-doc graph-backed orchestration acceptance gate using the latest tsift evidence, conflict-matrix, dispatch-trace, worker-result, and job-packet contracts.

For `tasks/software/tsift.md`, I added `#orchdog` and `#gdbhard`. The top tsift backlog item is `#orchdog`; the top agent-doc-bugs2 backlog item is `#aggtp`.

No code or test files changed in this cycle.
❯ How do I dogfood [#orchdog] in Claude Code and in Codex?
### Re: dogfood #orchdog — gpt-5

Use the same acceptance shape in both harnesses, but invoke `agent-doc` differently.

Claude Code path:

1. In `tasks/software/tsift.md`, add an exchange prompt such as:

```markdown
do [#orchdog] by running the graph-backed orchestration dogfood path in Claude Code: refresh graph-db, gather evidence, conflict-matrix, dispatch-trace, create/collect job packets, compare worker_prompt_packet and worker_result closure fields, and record any tsift fixture gaps. #spec-test-build-install-commit-push
```

2. From Claude Code, run:

```text
/agent-doc tasks/software/tsift.md
```

3. The Claude Code agent should use the slash-command/Skill path, then still close through `agent-doc finalize ... --stream` or `agent-doc write --commit ...` if repairing.

Codex path:

1. Do not use a leading slash. Send a normal Codex prompt:

```text
agent-doc tasks/software/tsift.md do [#orchdog] by running the graph-backed orchestration dogfood path in Codex: refresh graph-db, gather evidence, conflict-matrix, dispatch-trace, create/collect job packets, compare worker_prompt_packet and worker_result closure fields, and record any tsift fixture gaps. #spec-test-build-install-commit-push
```

2. Codex should run `agent-doc preflight`, `agent-doc plan`, execute the repo work, then run `agent-doc finalize ... --stream` and `agent-doc session-check tasks/software/tsift.md`.

For both harnesses, the core evidence commands to compare are:

```bash
tsift graph-db --path . refresh --json
tsift graph-db --path . doctor --json
tsift graph-db --path . evidence orchdog --depth 3 --limit 8 --json
tsift conflict-matrix --path tasks/software/tsift.md orchdog --json
tsift dispatch-trace --path tasks/software/tsift.md orchdog --format json
tsift dispatch-trace --path tasks/software/tsift.md orchdog --format html
agent-doc jobs create tasks/software/tsift.md --operation-doc --audit
agent-doc jobs list tasks/software/tsift.md
agent-doc jobs status tasks/software/tsift.md
agent-doc jobs collect tasks/software/tsift.md --json
```

The dogfood comparison should check whether Claude Code and Codex produce the same contract-grade fields: `packet_id`, `projection_hash`, replay/repair commands, owned files/symbols, forbidden files, expected tests, `worker_feedback`, worker result status, touched files/tests, and follow-up ids. Any missing or unstable field becomes the tsift-side fixture gap to add under `#orchdog` rather than an agent-doc routing bug by default.

No repo files needed to change for this answer.
<!-- agent:boundary:9e4a9ffb -->
<!-- /agent:exchange -->
###



<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#dtrc]
- do [#gafe]
- do [#wfcl]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps.
- [ ] [#gdbhard] [recommended] Harden graph-db operator docs/fixtures around the current feature set: refresh/status/doctor/evidence/drift/convex-sync replay paths, stale Convex repair commands, and real agent-loop session examples that can be regenerated in CI.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
