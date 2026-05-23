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

Completed #nextstale, #nextrunmiss, #nextswallow, and #nextorder2. No open backlog items.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-132638.md`*

Compacted content:
- Archived 1 response topic(s): #next-steps log review
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: #next-steps prompt regressions — gpt-5

Completed the four queued `#next-steps` regression items.

Implementation:
- `preflight.prompt_presets_requested` now uses the prompt-contract reference scan, so inline preset references like `Please analyze failed orders and bot traffic. #next-steps` are surfaced as requested presets while YAML `prompt_presets:` definition lines are ignored.
- Added deterministic coverage for stale preflight-started `#next-steps` prompt drift, post-compact `#next-steps` follow-up prompts, embedded natural-language `#next-steps` plan classification/backlog capture, and ordered multi-item `#next-steps` backlog closeout/status behavior.
- Updated `SPEC.md` with the inline preset request and ordered `#next-steps` backlog/status invariants.

Verification:
- `cargo test requested_prompt_presets -- --test-threads=2`
- `cargo test next_steps -- --test-threads=2`
- `make check`
- `make build-release`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`
- Latest GitHub CI run `26324125156` on `20260422/codex-agent-backend` was successful, including `Run make tmux-ci`.

Commits prepared before this document closeout:
- `src/agent-doc`: `17f2ebd` `fix: keep inline next-steps prompts actionable`
- parent gitlink: `6329e095d` `agent-doc: keep inline next-steps prompts actionable`
<!-- agent:boundary:82ef6eea -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do `#nextstale`
- do `#nextrunmiss`
- do `#nextswallow`
- do `#nextorder2`
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
