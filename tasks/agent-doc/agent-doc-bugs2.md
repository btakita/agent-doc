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
Implemented #rdec and advanced the auto queue. Top backlog item after closeout should be #eprf.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-030654.md`*

Compacted content:
- Archived 1 response topic(s): auto queue continuation after finalize
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
### Re: auto queue continuation — gpt-5

Implemented `#eprf`, `#rlog`, `#rtst`, and `#nfix`.

What changed:
- The IPC repair decision now carries prompt-prefix normalization targets and logs structured sidecar fallback records with patch id, bad/fallback hashes and lengths, prefix counts, and duplicate-prompt counts.
- Prefix-only sidecar fallback now attempts a narrow normalize/reposition patch against the fresh rejected editor buffer first, with full-content redelivery kept as the stale-checked last resort.
- Repair/redelivery skip and proof logs now preserve source patch ids and expected/current buffer hashes so stale editor buffers are not overwritten.
- Added deterministic unit and SimWorld coverage for narrow normalization repair, stale bad-state skip, and ops-log evidence; updated the closeout/editor specs and version notes.

Commits:
- `src/agent-doc`: `12b1ac51 fix: narrow sidecar normalization redelivery`
- parent gitlink: `e1d5d2b7c chore: advance agent-doc submodule`

Verification:
- `cargo test normalization_fallback_`
- `cargo test normalization_repair_sim_uses_narrow_patch_for_prefix_only_divergence`
- `cargo test ipc_repair_decision_`
- `make check`
- `make install`
- Latest CI/tmux evidence: GitHub CI run `26321931940` completed successfully, including `make check` and `make tmux-ci`.

Push will run after this session-document finalize, per the closeout rule that keeps the session document on the binary-owned commit path.
<!-- agent:boundary:2218b3b2 -->
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
- ~do [#dvre]~
- ~do [#rdec]~
- do [#eprf]
- do [#rlog]
- do [#rtst]
- do [#nfix]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#caud] [recommended] After the repair/redelivery split lands, audit the duplicate-prompt and prefix-normalization fixes added during the failed attempts and classify each as invariant, fallback, logging, or obsolete workaround.
- [ ] [#cuni] [recommended] Consolidate duplicate-prompt cleanup into one canonical repair/invariant path instead of scattered cleanup helpers for post-exchange comments, prompt lines against before-content, prefix variants, and duplicate response blocks.
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
