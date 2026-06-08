---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260608-161626.md`*

Compacted content:
- Archived 6 response topic(s): churn from repeated committed-cycle finalize; active continuation closeout; previous few turns issues; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: Codex `Run Agent Doc` ignores exchange-only prompts on a fresh session — opus-4-8

Triaged as a bug and planned (not yet fixed — this doc's `#agent-doc-bug` convention is plan + backlog, and the queue here is inactive so I did not run the attached `#spec-test-build-install-commit-push` preset).

**What I found.** Two independent signals drive `route` dispatch, and only the queue one is robust on a fresh session:

- **Queue write** activates via a snapshot-independent signal (`fm.queue_active` at `route.rs:2121` plus preflight queue-trigger resolution). It dispatches even before any cycle snapshot exists — which is why typing into `agent:queue` starts the turn.
- **Exchange write** is detected through `pending_prompt_bearing_context_for_route` (`route.rs:7727`) → `first_unstarted_prompt_bearing_change` (`session_check.rs:4649`). That function has two gates that hide a fresh-session exchange prompt: it returns `None` when no snapshot exists (`session_check.rs:4652`), and it strips `agent:queue` content before diffing (`session_check.rs:4707`). On a brand-new session there is no committed cycle snapshot (or the snapshot already equals the file), so the exchange tail prompt is never seen as "unstarted" → nothing happens.

So the asymmetry is real and structural: the queue path has a fresh-session activation signal, the exchange path doesn't.

**Plan:** [`tasks/agent-doc/plan-codex-exchange-prompt-no-dispatch.md`](plan-codex-exchange-prompt-no-dispatch.md) — repro, root-cause trace, the snapshot-absent fallback fix, and route + session_check test coverage.

**Backlog:** added `#codex-exchange-prompt-no-dispatch` at the top of the backlog.

Say the word (or `do #codex-exchange-prompt-no-dispatch`) and I'll implement + test + build + commit per the preset.

### Re: do #codex-exchange-prompt-no-dispatch — opus-4-8

Fixed, tested, built, installed, committed, and pushed.

**Root cause.** `first_unstarted_prompt_bearing_change` (`session_check.rs:4649`) returned `None` whenever no cycle snapshot existed. On a fresh session that's exactly the state: a freshly typed exchange tail prompt has no snapshot to diff against, so the exchange path saw nothing and `Run Agent Doc` did nothing. The queue path activates snapshot-independently (route queue activation re-saves the snapshot), so the same text in `agent:queue` started a turn — the asymmetry you hit.

**Fix.** When no snapshot exists, the detector now falls back to the committed `HEAD` blob (`git::show_head`), then to an empty baseline for untracked docs. A freshly typed exchange tail prompt is therefore detected and routed. The fallback stays exchange-scoped — queue components are still stripped from this diff and keep their own activation path.

**Verification.**
- `cargo test -p agent-doc-orchestration first_unstarted_prompt_bearing_change` — 9 passed, including two new tests: `..._detects_fresh_exchange_prompt_without_snapshot` (HEAD fallback) and `..._ignores_fresh_queue_only_write_without_snapshot` (regression guard that the fallback stays exchange-scoped).
- `make check` — clippy `-D warnings` clean, full suite green.
- `cargo build --release` + `cargo install --path .` — both binaries refreshed (`.bin/agent-doc` → release, tmux → `~/.cargo/bin`), still `0.34.0`.

**Shipped.**
- `src/agent-doc` `agent-doc-orchestration/src/session_check.rs` (fix + tests) and `SPEC.md` (new rule) — committed `4f963e53`, pushed to `btakita/agent-doc` `main`.
- Superproject: pointer bump + plan file committed locally; pushing now.

**Live-verify note.** Logic is covered by deterministic tests, but the end-to-end JB **Run Agent Doc** → fresh Codex pane path can't be exercised from this Claude session. Worth a one-off live confirmation next time you start a fresh Codex session: type into `agent:exchange`, hit Run Agent Doc, and confirm the turn now starts.
<!-- agent:boundary:28c2a0d8 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- do #codex-exchange-prompt-no-dispatch
---
The agent-doc monsterrodholders.md has drift issues. We recently refactored to a control pane process. Please fix. Add Claude Code Simworld tests.
```
● The IPC/CRDT drift garbled my response (reversed lines, missing header) — the known drift situation. Let me
  reconstruct the Exchange cleanly, then reset sidecars from the corrected file. First, the exact current bytes:

  Read 1 file (ctrl+o to expand)

● The corruption is clear — lines 37–43 are my response fragments inserted reversed above the old HEAD response. Let me
  reconstruct cleanly with two edits.
```
---
<!-- /agent:queue -->
###
<!--
`'/clear' is disabled while a task is in progress
---
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority --> 
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
