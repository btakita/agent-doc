---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Context automatically compacted before the next turn because session-accretion heuristics tripped.

Active backlog:
❯ - [ ] [#sshprelude] [recommended] Resumed Codex streaming turns with `required_ssh_targets` can get stuck on stale SSH state if the thread emits assistant prelude text before the failing SSH command. Add fresh-retry recovery for that sequence without leaking stale prelude text into the final response. Plan: `tasks/agent-doc/plan-codex-required-ssh-retry-after-prelude.md`
- [ ] [#noopcap] [recommended] Collapse repeated commit_noop closeout churn into a compact or restart handoff instead of appending another Re: section.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- 3 more active item(s)

Icebox:
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the

❯ *1 earlier topic(s) archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260506-054256.md`*

### Re: bounded response-context pack for warn-level sessions — gpt-5

Implemented `#ctxpack`.

Warn/block accreting sessions no longer have to resend the full exchange tail on resumed `run`, `stream`, or sequential `orchestrate` steps when there is still live prompt-bearing work. Those prompt builders now switch to a bounded response-context pack that carries the active prompt targets, current `### Session Summary`, backlog head, and available component names. I also added a focused spec note for orchestration plus regression tests for the shared helper and each prompt builder.

Verification: `cargo test`, `cargo build`, and `cargo install --path . --force` all passed in `src/agent-doc`.

Commits:
- `src/agent-doc`: `971cad1` (`feat: bound warn-level session prompt context`)
- `agent-loop`: `113c52eca` (`chore: update agent-doc prompt context pack`)
❯ /home/brian/work/btakita/agent-loop/tasks/software/tsift.md was compacted. The rate of turns should not be a factor. The auto-compaction is too aggressive. I personally have worked with longer documents. The sqlite archive cache does mitigate the problem, but it may make sense to keep the document uncompacted but only send the diff with recent content as context.
❯ > this document closed 11 cycles in the last 30 minutes
### Re: compact trigger vs bounded recent context — gpt-5

`tasks/software/tsift.md` was not compacted just because it was long. The current `preflight` path auto-compacts whenever `session_accretion.auto_compact_policy()` returns a policy, and that policy treats warn-level closeout churn as compact-worthy in addition to raw exchange size. In the compacting turn, the triggering reason was the recent no-op closeout signal, not document length by itself.

The `this document closed ... cycles` line is generated prompt context from `session_accretion.rs`, and `prompt_context::build_document_section()` injects the first accretion reason into the bounded prompt pack. So that text was not something the user or harness literally "Entered"; it was synthesized diagnostic context that got surfaced in the binding prompt.

❯ I agree with the policy concern. With `#ctxpack` in place, the better default is to keep the visible document intact for cycle-count-only warn states and send bounded diff plus prompt targets, session summary, and backlog head to the harness. `#ratecmp` is now in backlog to tighten auto-compact so it is reserved for true exchange growth or repeated closeout churn rather than session rate alone.
### Re: icebox normalization — gpt-5

Marked the historical icebox entries closed so `session-check` no longer treats them as shadow-open backlog items outside the live backlog.

<!-- agent:boundary:a2316633 -->
<!-- /agent:exchange -->
###

<!--
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#cmpclr] Compaction does not clear the session-accretion guard: `preflight` / `plan` can keep failing closed on the same recent-cycle blocker immediately after a successful `agent-doc compact --commit`, trapping the document in a compact-then-block loop. Plan: `tasks/agent-doc/plan-compact-does-not-clear-accretion-guard.md`
- [ ] [#ratecmp] [recommended] Rework session-accretion auto-compact so recent cycle count alone does not compact otherwise healthy docs; prefer bounded diff/recent-context packs and reserve auto-compact for exchange growth or closeout churn.
- [ ] [#sshprelude] [recommended] Resumed Codex streaming turns with `required_ssh_targets` can get stuck on stale SSH state if the thread emits assistant prelude text before the failing SSH command. Add fresh-retry recovery for that sequence without leaking stale prelude text into the final response. Plan: `tasks/agent-doc/plan-codex-required-ssh-retry-after-prelude.md`
- [ ] [#noopcap] [recommended] Collapse repeated commit_noop closeout churn into a compact or restart handoff instead of appending another Re: section.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
