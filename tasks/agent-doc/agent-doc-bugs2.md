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

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-133319.md`*

Compacted content:
- Archived 1 response topic(s): full-document IPC duplicate prompt corruption
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ agent-doc on this document writing this prompt & tsift.md had the duplicate content corrupt full-document IPC issue. Please narrow the the conditions to fix this category of bug entirely. #spec-test-build-install-commit-push

❯ The duplicate prompt issue also happened. The duplicate prompt happened after the duplicate content corrupt document...This often happens and can cycle.

❯ This issue is a critical issue. Whatever drives the full-document IPC and the duplicate prompt needs to be significantly narrowed. I would rather not even have it at all. It is only caused problems.
❯ #spec-test-build-install-commit-push
### Re: full-document IPC duplicate prompt corruption — gpt-5

Implemented and pushed the full-document IPC scope fix in `src/agent-doc`.

Changed paths:
- `src/agent-doc/src/write.rs`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/specs/07-closeout-commands.md`

What changed:
- `fullContent` IPC now rejects explicit template documents and any document with parsed `agent:*` component markers before socket/file payload emission, logging `full_content_ipc_scope_rejected scope=<scope>`.
- Template normalization-only editor IPC now sends narrow normalization data such as `normalize_prefix_lines` without whole-document redelivery.
- Template/component IPC dedupe repair now uses guarded disk repair when no narrow repair patch exists, instead of re-sending the corrupted whole document to the editor.
- The functional spec and closeout-command invariants now document the narrowed full-document IPC contract.

Verification:
- `cargo fmt`
- `cargo test template_normalization_only_file_ipc_omits_full_content -- --nocapture`
- `cargo test template_ipc_dedupe_repair_uses_disk_not_full_content_redelivery -- --nocapture`
- `cargo test full_content -- --nocapture`
- `cargo test --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `make check`
- `cargo install --path .`
- `agent-doc --version` -> `agent-doc 0.33.16`
- GitHub Actions CI run `26363212302` passed: `Run make check` and `Run make tmux-ci` were both green. The only annotation was the upstream Node.js 20 Actions deprecation warning.

Commit and push:
- Commit: `8346238 fix: narrow full-content IPC scope`
- Pushed: `origin/20260422/codex-agent-backend`

Escalation: none. Confidence: high.
<!-- agent:boundary:2861e342 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#1mm8]
- do [#hg0n]
- do [#te1d]
- do [#pmz2]
- do [#f4xr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#qresidue] [recommended] Fix inactive agent:queue residue being read as active response context; implement tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md.
- [ ] [#qinactive] [recommended] Prevent edits inside inactive agent:queue blocks, such as do [#gdbpropscan], from becoming prompt targets or cross-document #agent-doc-bug backlog captures; implement tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
