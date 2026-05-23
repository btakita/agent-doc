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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-010515.md`*

Compacted content:
- Archived 1 response topic(s): post-exchange scratch comment cleanup
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Please fix. This has taken >50 turns and you still fail to fix this issue. FIX THE DANM ISSUE. Please add logging so you can detect when this issue occurs. I'm talking about the duplicate corrupt document & the duplicate prompt issue. It happened as I was typing this prompt. WTF are you doing? Not fixing the issue apparently. Just wasting tokens and time. #spec-test-build-install-commit-push
### Re: stale full-content IPC guard — gpt-5

Fixed and pushed.

The failure mode I found was at the binary-to-editor full-document handoff: a Compact Exchange/operator full-content replacement could still emit socket/file IPC after the source buffer used to compute that replacement was already stale on disk. Newer editor bridges were supposed to reject by `expected_content_hash`/`expected_content_len`, but the binary could still hand a stale whole-document payload to an older or non-compliant bridge. That matches the observed duplicate/corrupt document and duplicate prompt shape while typing.

I moved the guard into the binary before IPC emission. `try_ipc_full_content_with_mode` now verifies the caller-supplied source buffer against the current disk buffer before socket or file IPC is created. On stale source it refuses delivery, preserves the live document, avoids snapshot save, logs a typed FlowCore block event, and writes an ops-log record named `full_content_source_buffer_rejected` with path, source label, patch id, expected/current hashes, and expected/current byte lengths.

Changed paths:

- `src/agent-doc/src/write.rs`
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/PLUGIN-SPEC.md`

Verification:

- `cargo test late_fallback_patch_guard_tests` passed.
- `cargo test component_compact` passed.
- `make check` passed, including clippy, all default tests, version sync, and the ignored medium deterministic closeout simulator.
- `cargo fmt --check` passed.
- `git -C src/agent-doc diff --check` passed.
- `make build-release` passed.
- `make install` passed; installed `agent-doc 0.33.16` and `libagent_doc-0.33.16.so`.
- I initially gave Cargo two test filters in one command; Cargo rejected that invocation, then I reran the intended test groups separately and they passed.

CI/tmux review:

- Latest pushed root run: `26319684545` for `5cca83e06318a760478dee22ab7d00793789607a`.
- GitHub reported both `check` and `tmux-ci` jobs failed with empty `steps: []` after about two seconds, so this is an external CI-start failure per the runbook, not a code or tmux regression. Local verification above is the evidence for this cycle.

Commits pushed:

- `src/agent-doc`: `0046ee2a fix: guard stale full-content ipc`, pushed to `origin/20260422/codex-agent-backend`.
- parent repo: `5cca83e06 agent-doc: guard stale full-content ipc`, pushed to `origin/main`.

Escalation: none.

Confidence: high. The new regression tests cover both file-IPC and socket-IPC stale full-content delivery, including a fake non-compliant listener that would blindly apply `fullContent` if the binary emitted it.

<!-- agent:boundary:b4afc108 -->
<!-- /agent:exchange -->
###
<!--
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
