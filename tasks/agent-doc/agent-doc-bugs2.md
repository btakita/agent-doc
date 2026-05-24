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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260524-172749.md`*

Compacted content:
- Archived 5 response topic(s): duplicate IPC/prompt corruption; fix elaboration; documentation index and process diagram; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The full-document IPC corruption occurred in equityfundingsource.md. I noticed that only the Codex sessions have this issue. root.md using a Claude Code session does not have this issue. Is the Codex path different for this behavior? If yes, can we just standardize on the Claude Code path across all agent sessions?
❯ #spec-test-build-install-commit-push
### Re: Codex closeout path — gpt-5

Yes. The Codex path is different at invocation, routing, and backend capture: Codex uses a plain `agent-doc <FILE>` prompt, Codex hooks, Codex JSONL parsing, and Codex-specific resume/capability behavior. It should not standardize on the Claude Code slash command or Skill-tool path because that path is not available to Codex.

The part that should be standardized is the response persistence path. Once any harness has final assistant text, it must go through the same binary-owned `finalize` / `write --commit` closeout, component-patch write pipeline, snapshot commit, and `session-check` boundary. I updated the specs to make that explicit and added a regression test that locks the invariant down.

Changed paths:
- `src/agent-doc/specs/codex-support.md`
- `src/agent-doc/specs/05-agent-backend.md`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/tests/test_cli.rs`

Verification:
- `cargo test test_codex_shared_closeout_spec_invariants` passed.
- `make check` passed from the workspace root, including `instruction-files`, `src/agent-doc`, and `src/corky`.
- Latest GitHub Actions CI run `26367673444` is green; its `Run make tmux-ci` step also succeeded.
- `make install` passed for `src/agent-doc` and installed `agent-doc v0.33.16` plus `libagent_doc-0.33.16.so` locally.
- `cargo run --quiet -- audit-docs` is blocked by pre-existing root `AGENTS.md` managed-surface drift from the tsift code-navigation block. I preserved that user/worktree drift and added a recommended backlog item for it.

Commit:
- `src/agent-doc`: `8a5058bf spec: clarify codex closeout boundary`
- parent repo: `794dd6c15 chore: update agent-doc submodule`

Push:
- Queued after this session-document closeout so the response commit is included, per the agent-doc commit ordering rule.

Escalation: none required.
Confidence: high for the spec/test change; the remaining audit-docs issue is isolated to the pre-existing managed instruction-surface drift described above.
<!-- agent:boundary:fbb346d8 -->
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
- [ ] [#e86b] [recommended] Resolve pre-existing root AGENTS.md managed instruction-surface drift from the tsift code-navigation block so  can pass.
- [ ] [#rpatch] Fix response patchback recovery so stale-cycle closeout cannot report already committed while a valid visible response patchback remains uncommitted; implement tasks/agent-doc/plan-response-patchback-uncommitted.md.
- [ ] [#qresidue] [recommended] Fix inactive agent:queue residue being read as active response context; implement tasks/agent-doc/plan-drained-non-auto-queue-cleanup.md.
- [ ] [#qinactive] [recommended] Prevent edits inside inactive agent:queue blocks, such as do [#gdbpropscan], from becoming prompt targets or cross-document #agent-doc-bug backlog captures; implement tasks/agent-doc/plan-inactive-queue-residue-prompt-target.md.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
