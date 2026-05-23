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

Logged three fresh log-triage follow-ups. No open backlog items.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-004214.md`*

Compacted content:
- Archived 1 response topic(s): response-quoted scratch comment cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ When I ran agent-doc on tsift.md, the html comment below the agent:exchange element was immediately deleted. #spec-test-build-install-commit-push
### Re: post-exchange scratch comment cleanup — gpt-5

Fixed the `tsift.md` scratch-comment deletion path.

The root cause was that duplicate-prompt cleanup treated every post-exchange HTML comment line matching an exchange prompt as owned cleanup residue. In the `tsift.md` case, the prompt line was already parked in the HTML comment before the response cycle, so closeout answered the prompt and then emptied the original scratch comment. Cleanup is now baseline-aware: closeout, preflight, route, and IPC snapshot cleanup preserve duplicate-looking comment lines that were already present in the pre-response baseline or HEAD, while still scrubbing same-cycle duplicate residue. Editor-visible normalization now preserves ordinary post-exchange comments because it has no baseline ownership proof, and the duplicate-residue guard now ignores ordinary HTML comment bodies instead of treating them as ambiguous Markdown residue.

Changed paths:
- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/route/tests.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/specs/04-diff-computation.md`
- `src/agent-doc/specs/07-closeout-commands.md`

Verification:
- `cargo fmt --check`
- `cargo test prompt_html_comment`
- `cargo test duplicate_prompt_comment`
- `cargo test mixed_prompt_comment`
- `cargo test duplicate_prompt_residue`
- `make check`
- `cargo build --locked`
- `cargo install --path . --locked`
- `agent-doc --version` -> `agent-doc 0.33.16`
- CI/tmux review: latest agent-doc CI run `26318495699` succeeded, including `Run make tmux-ci`.

Commit / push:
- `src/agent-doc` `b30b12c1` (`fix: preserve post-exchange scratch comments`) pushed to `origin/20260422/codex-agent-backend`.
- root `agent-loop` `f2b09ecba` (`chore: pin agent-doc scratch comment fix`) pushed to `origin/main`.

Escalation: none.

Confidence: high. The regression coverage includes the exact baseline-owned `tsift.md` shape, same-cycle duplicate cleanup, route/preflight cleanup, editor-visible normalization, and duplicate Markdown residue rejection.
<!-- agent:boundary:b6b272f4 -->
<!-- /agent:exchange -->
###
<!--
Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.
---
#spec-test-build-install-commit-push
-->

<!-- agent:queue -->
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
