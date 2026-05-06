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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260506-035514.md`*

Active backlog:
- [ ] [#autocmp] [recommended] Promote session-accretion warn/block into an automatic compact policy: auto-run partial exchange compaction or rewrite the exchange to a fresh session summary before another full turn.
- [ ] [#ctxpack] [recommended] Add a bounded response-context pack for warn-level sessions so the harness receives prompt targets, session summary, and backlog head instead of the full exchange tail.
- [ ] [#noopcap] [recommended] Collapse repeated commit_noop closeout churn into a compact or restart handoff instead of appending another Re: section.
- 4 more active item(s)

Icebox:
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the
❯ Please plan for a sqlite session archive for fast lookup of compacted turns. Please ensure that the db does not grow unbounted. Perhaps we should treat this as a LRU cache?
### Re: sqlite archive plan — gpt-5
Plan: `tasks/agent-doc/plan-sqlite-compacted-turn-archive.md`

I split this from the broader `.agent-doc/state.db` authority discussion. The plan treats sqlite as a bounded, rebuildable retrieval index over compacted markdown archives, not as the new source of truth for live session content.

On the growth question: I would not use pure LRU as the only rule. The better default is a hybrid cache policy: pin archives tied to current documents, active backlog ids, or recent lookups; bias search toward recency; prune only cold unpinned rows when size/age caps are exceeded; and keep the markdown archives themselves as the durable history that the sqlite index can rebuild from.

I added the follow-up to backlog with the same plan path so the next implementation or review turn can open the exact artifact directly.
❯ do [#sqlarcidx]. spec-test-build-install-commit-push
### Re: sqlite compacted-turn archive rollout — gpt-5

Implemented `#sqlarcidx` from `tasks/agent-doc/plan-sqlite-compacted-turn-archive.md`.

Added a derived sqlite archive index at `.agent-doc/archive-index.db`, new `agent-doc archive-index` and `agent-doc archive-search` commands, and compact-time best-effort indexing for new archives. Archive markdown remains canonical; the DB rebuilds from `.agent-doc/archives/*.md`.

Verification: `cargo test`, `cargo build`, and `cargo install --path . --force` all passed in `src/agent-doc`.

Commits:
- `src/agent-doc`: `b8a439b` (`feat: add sqlite archive index for compacted turns`)
- `agent-loop`: `209c60e87` (`chore: update agent-doc archive index rollout`)
❯ do [#autocmp]. spec-test-build-install-commit-push
### Re: automatic compact policy for accreting sessions — gpt-5

Implemented `#autocmp`.

`agent-doc preflight` now auto-compacts churn-heavy template exchanges before another large turn when session-accretion signals trip, while preserving any live unresolved prompt tail in the visible file and keeping the snapshot on the compacted committed baseline. The partial compact path now keeps prompt tails that were sitting after the binary-owned boundary, and the session-accretion policy only auto-compacts warn/block states tied to exchange/closeout churn rather than restart-only incidents.

Verification: `cargo test`, `cargo build`, and `cargo install --path . --force` all passed in `src/agent-doc`.

Commits:
- `src/agent-doc`: `7789330` (`feat: auto-compact accreting sessions`)
- `agent-loop`: `80f440ff0` (`chore: update agent-doc accretion auto-compact`)
<!-- agent:boundary:70d38601 -->
<!-- /agent:exchange -->
###

<!--
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#ctxpack] [recommended] Add a bounded response-context pack for warn-level sessions so the harness receives prompt targets, session summary, and backlog head instead of the full exchange tail.
- [ ] [#noopcap] [recommended] Collapse repeated commit_noop closeout churn into a compact or restart handoff instead of appending another Re: section.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
  `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the
  `session_end` metadata closeout fix in
  `startup_miss.rs` and the routed-dispatch binding hardening in
  `route.rs`. Final child closed:
  `#tmuxcrash-rn2d`.

<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
