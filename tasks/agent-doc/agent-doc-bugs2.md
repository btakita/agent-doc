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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260529-060029.md`*

Compacted content:
- Archived 4 response topic(s): do [#h9g0]; do [#zeb5]; do [#already-applied-divergence]; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#adoc-queue-ipc-drift] — opus-4-7

Landed a discrete fix for the most visible symptom of the drift loop and split the deeper divergence into its own tracked item.

#### What shipped this cycle

**`inactive_queue_residue` is now a per-*edit* signal, not a per-preflight nag.** The warning previously fired purely from the static "inactive + non-empty queue" state, so the live tail an `item_modified` halt retains (and syncs into the snapshot) re-warned on *every* subsequent edit-free preflight — exactly the "recurring on every preflight with no user edit" symptom in the plan. It now fires only when the inactive queue body actually changed vs the snapshot this cycle (`inactive_queue_changed_vs_snapshot`, normalized through `queue::parse`+`render`).

- `agent-doc-orchestration/src/preflight.rs` — gate + helper
- Tests: `preflight_does_not_reflag_stable_inactive_queue_as_residue`, `preflight_flags_inactive_queue_when_changed_this_cycle`
- Spec: `specs/07-orchestration-commands.md` § Post-commit queue consumption

A first attempt — suppressing the warning whenever *live prompts* remained — was **wrong**: it broke the legitimate case where a user adds a `do [#id]` to an inactive queue and must be told it won't run (`test_preflight_warns_but_does_not_target_inactive_queue_edit`). The two states are indistinguishable in steady state; "changed vs snapshot this cycle" is the correct discriminator, since the bug's defining trait is *no user edit*.

**Incidental test-isolation fix.** `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` failed whenever `make check` ran from inside a Claude Code session: `detect_harness()` checks Claude/OpenCode markers before Codex, so an inherited `CLAUDECODE` short-circuited the codex recursive-deadlock guard. The test now clears competing harness markers. Required for a green suite from the agent-doc session harness.

`make check` green (exit 0, 0 failures); both binaries rebuilt + installed; submodule `b3dd9598` + superproject `21356aada` pushed; latest CI run green.

#### Why the umbrella item is reaped but the bug isn't fully closed

This item's **observable loop symptoms** are now both capped: duplicate-growth (prior `dedup_live_prompts` mitigation) and the recurring residue warning (this cycle). The genuinely-remaining work is the **IPC-buffer divergence itself** (#2) and **`stuck_captured_cycle` decoupling** (#4) — both require a deterministic live-IPC-listener repro harness that can't be modeled without simulating the IDE buffer. I split that into a new backlog item `[#adoc-queue-ipc-buffer-divergence]` (top of backlog) rather than re-spin this umbrella, so the next queue item proceeds and the hard part stays explicitly tracked.

The root fix for #2: halt + queue-maintenance currently write the deactivated queue shape to disk, but a live IDE buffer overwrites it on its next flush — the change must go *through the IPC listener* so the editor buffer converges.
<!-- agent:boundary:ec6b3799 -->
<!-- /agent:exchange -->
###
<!--
JB `Run Agent Doc` error
```
resync: stash pane %19 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 13.7s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 35 for tasks/professional/equityfundingsource.md is busy on pane %3; queued pending dispatch "re [#2z8r]: How can we allow custom addresses?" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
---
JB `Run Agent Doc` on tsift.md error:
```
Session is still running for tasks/software/tsift.md. Pane %21 is busy (agent-doc). Wait for the turn to finish, then retry Clear Session Context. Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn. Latest pane output: 22.4K (11%) ctrl+p commands
```
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#adoc-queue-ipc-drift]~
- do [#duplicate-response-after-commit]
- do [#pending-add-append-not-prepend]
- do [#queue-strike-on-halt]
- do [#next-steps-backlog-capture]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#adoc-queue-ipc-buffer-divergence] Durable fix for the remaining #adoc-queue-ipc-drift root causes #2 + #4. #2: the IPC working buffer re-adds `auto` / diverges from the committed inactive snapshot because halt + queue-maintenance writes land on disk while the live IDE buffer wins on its next flush — push the queue-shape change (strip auto, clear queue_active, deduped body) through the IPC listener so the editor buffer converges instead of only writing the file. #4: decouple stuck_captured_cycle from queue-component churn. Needs a deterministic live-IPC-listener repro harness (SimWorld) first. Plan: tasks/agent-doc/plan-queue-ipc-drift.md § Still open.
- [ ] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [ ] [#pending-add-append-not-prepend] `--pending-add` appends new items below the previous CLI arg within the same finalize cycle, contradicting SKILL.md "Pending capture rule" ("Put new items at the beginning of agent:backlog"). Repro: tasks/agent-doc/open-code.md commit f94e7851c — agent added wave-4, wave-3, wave-2, wave-1 expecting wave-1 on top per SKILL.md; actual backlog was wave-4 on top (pure CLI-arg order). User flagged it as bug; agent had to issue --pending-reorder. Plan: tasks/agent-doc/plan-pending-add-prepend-semantics.md (recommend Option A — strict prepend, drop the implicit adjacency heuristic).
- [ ] [#queue-strike-on-halt] Queue head consumed on halt/refusal response. write.rs:1233-1242 + response_explicitly_targets_queue_head (write.rs:2481) treat any `### Re: <head-id> <suffix>` heading as completion signal even when body explicitly says "backlog left intact" + passes no --done/--pending-gate. Repro: tasks/agent-doc/open-code.md cycle 619af4d0b struck [#d97v] after halt response. Plan: tasks/agent-doc/plan-queue-strike-on-halt-response.md (recommend Option A — require explicit completion signal).
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
