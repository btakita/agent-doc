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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Completed #caud: duplicate-prompt and prompt-prefix normalization audit committed in e17074f71.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-040655.md`*

Compacted content:
- Archived 1 response topic(s): auto queue continuation
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: #caud — gpt-5

Completed the audit and classified the duplicate-prompt / prompt-prefix paths in `tasks/agent-doc/audit-duplicate-prompt-prefix-normalization.md`.

What changed:
- Added a classification table covering canonical prompt-prefix invariants, sidecar/IPC repair fallbacks, duplicate-prompt cleanup fallbacks, logging hooks, and the obsolete force-commit workaround candidate.
- Repaired the auto queue so the completed audit prompt is no longer at the head.

Verification:
- `make -C src/agent-doc check` passed: clippy, default Rust tests, integration tests, and `closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget`.
- CI/tmux review: latest CI run `26322998714` is in progress with `Run make tmux-ci` running; previous completed CI run `26321931940` had `Run make tmux-ci` successful.
- `make -C src/agent-doc install` completed and reinstalled `agent-doc 0.33.16` plus `libagent_doc-0.33.16.so`.

Commit / push:
- Committed the non-session audit file as `e17074f71` (`agent-doc: audit duplicate prompt repairs`).
- The session repair write is being committed through `agent-doc write --commit`; push comes after this repair commit passes session-check.
<!-- agent:boundary:27f69d30 -->
<!-- /agent:exchange -->
###
<!--
"separate the decisions"
---
html comment fix
---
Please fix. This has taken >50 turns and you still fail to fix this issue. FIX THE DANM ISSUE. Please add logging so you can detect when this issue occurs. I'm talking about the duplicate corrupt document & the duplicate prompt issue. It happened as I was typing this prompt. WTF are you doing? Not fixing the issue apparently. Just wasting tokens and time. #spec-test-build-install-commit-push
---
Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.
---
#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- do [#cuni]
- do [#cspe]
- do [#ctes]
- do [#crem]
- do [#cobs]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#cspe] [recommended] Update src/agent-doc/editors/PLUGIN-SPEC.md and src/agent-doc/editors/SPEC.md to describe the new snapshot-only repair versus editor-redelivery contract, removing stale language that says every normalization fallback is sent back through fullContent.
- [ ] [#ctes] [recommended] Rewrite tests that encode old over-eager fullContent redelivery behavior so they assert the new typed decisions, skip-redelivery branch, narrow-repair branch, and last-resort fullContent branch.
- [ ] [#crem] [recommended] Remove redundant workaround branches and compatibility shims made unnecessary by the typed repair decision, keeping only guards that still protect committed snapshots, active typing, or externally visible editor state.
- [ ] [#cobs] [recommended] Add temporary observability cleanup criteria: once the new repair/redelivery logs prove stable across local runs and tmux/CI, reduce noisy transition logs while preserving high-signal corruption diagnostics.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
