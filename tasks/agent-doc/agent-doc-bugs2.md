---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260518-174346.md`*

Compacted content:
- Archived 1 response topic(s): OpenCode arrow-key logging
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#hrnsmis]. spec-test-build-install-commit-push
Done.

- Outcome: Implemented the harness mismatch guard for `agent-doc preflight`. It now emits a structured `harness_mismatch` warning when frontmatter `agent:` differs from the detected Claude Code, Codex, or OpenCode harness, while continuing with active-harness attribution and closeout behavior.
- Verified: `cargo test harness`; `cargo check`; `cargo test preflight_output_includes_warnings`; `make check TEST_THREADS=8`; `cargo install --path .`; smoke-tested `CLAUDE_CODE=1 agent-doc preflight` on an `agent: codex` doc; reviewed latest GitHub CI run `25773430221` as successful with no tmux failure surfaced.
- Committed: `f9b3af8c fix: warn on harness mismatch during preflight`
- Pushed: yes, to `origin/20260422/codex-agent-backend`
- Notes: Left pre-existing uncommitted changes in `VERSIONS.md`, `src/template.rs`, and `src/write.rs` untouched.
ccdo [#hrnsmis]. spec-test-build-install-commit-push
<!-- agent:boundary:978c40fa -->
<!-- /agent:exchange -->
###
<!--
Use TEST_THREADS=8. spec-test-build-install-commit-push
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#rspdigest] Direct `agent-doc <FILE>` preserves progress commentary in template patchbacks — captured Codex response can be `progress prose + patch:exchange`; direct run/write applies unmatched prose into the exchange instead of stripping or digesting it like the Codex Stop replay guard. Plan: tasks/agent-doc/plan-direct-run-progress-commentary-digest.md
- [ ] [#crossdocpend] Cross-document --pending-add targets wrong file — skill adds to current document backlog instead of preset-specified target file. Plan: tasks/agent-doc/plan-cross-doc-pending-add.md
- [ ] [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- [ ] [#spfxnorm] Sidecar ❯ prefix normalization loss — content_ours fallback recovers snapshot but not working tree. Plan: tasks/agent-doc/plan-sidecar-prefix-normalization-loss.md
- [ ] [#oobpmt] OOB prompt absorption — preflight commit step absorbs user-content changes (new prompts in status) before diff runs, causing `no_changes: true`. Fix: use pre-commit snapshot as diff baseline, not post-commit snapshot.
- [ ] [#staleflt] Stale preflight_started cycle recovery — auto-close empty preflight cycles (no capture, phase=PreflightStarted) on next preflight invocation instead of failing closed. Add time-based staleness check.
- [ ] [#busychk] Pane busy-check before sync eviction — check for open agent-doc cycles before killing panes during layout reconciliation. Skip eviction if busy, log warning.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death doesn't lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- [ ] [#pfxleak2] content_ours prefix leak + commit churn — plan: tasks/agent-doc/plan-content-ours-prefix-and-commit-churn.md
- [ ] [#bppfxstrip2] content_ours strips ❯ from multi-line user prompts during finalize — plan: tasks/agent-doc/plan-prefix-strip-on-user-prompts.md
- [ ] [#lastpfx] Last prompt missing `❯ ` prefix — final user prompt can persist as bare text at the exchange tail even though prompt prefix normalization should make prompt ownership explicit. Plan: tasks/agent-doc/plan-last-prompt-prefix-loss.md
- [ ] [#y9kk] [recommended] Phase 1 for lower-agent orchestration: define job packet + worker result schemas, add the manual job-packet runbook, and include examples for search, test triage, small patch, and spec update jobs. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#v44k] [recommended] Phase 2 for lower-agent orchestration: add tsift JSON contracts for context-pack, source-read, diff-digest, test-digest, index-status, stable handles, freshness checks, and token estimates. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ffsm] [recommended] Phase 3 for lower-agent orchestration: extend agent-doc plan with task_class, dispatch_candidate, model tier, context budget, write scope, required proof, and tsift missing/stale fallback behavior. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#rafe] [recommended] Phase 4 for lower-agent orchestration: implement agent-doc jobs create/list/status/collect plus optional operation-doc generation and closeout links. Plan: tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md
- [ ] [#ek86] Implement the lower-agent orchestration MVP from tasks/agent-doc/plan-lower-agent-orchestration-token-savings.md, starting with manual job packets plus tsift context packets.
- [ ] [#fwjw] Define the agent-doc taxonomy for agent docs, job packets, operation docs, runbooks, and plans.
- [ ] [#0fcx] Specify operation docs/opdocs as retained audit artifacts and how they relate to job packets.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
