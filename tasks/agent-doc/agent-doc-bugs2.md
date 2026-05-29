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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260529-071750.md`*

Compacted content:
- Archived 3 response topic(s): do [#adoc-queue-ipc-drift]; do [#adoc-queue-ipc-buffer-divergence]; JB `Run Agent Doc` queued instead of dispatching (actor wasn't really busy)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
order the queue and backlog by priority

### Re: order the queue and backlog by priority — opus-4-7

Reordered both lists by severity (data corruption → workflow correctness → polish/coverage).

**Backlog top → bottom:**

1. `#prompt-duplicated-while-typing` — live editor input is being corrupted/duplicated as the user types. Active data-loss class on the user-facing side of the IPC seam.
2. `#duplicate-response-after-commit` — late IPC reposition re-inserts the already-committed response body in the working tree. Requires manual `Edit`-based dedup to recover. Data corruption on the agent side.
3. `#adoc-queue-ipc-buffer-divergence` — live IDE buffer diverges from the committed inactive snapshot. Blocker identified (IPC carries content only, not tag attrs / frontmatter); needs plugin protocol extension.
4. `#queue-strike-on-halt` — queue head wrongly struck on halt/refusal responses. Correctness bug that silently consumes scheduled work.
5. `#pending-add-append-not-prepend` — `--pending-add` violates the SKILL.md "front of list" contract. Workflow contract violation, manually recoverable.
6. `#next-steps-backlog-capture` — `#next-steps` recommendations don't auto-add to backlog. Automation enforcement gap (`session-check` doesn't enforce planning `expect_add`).
7. `#run-agent-doc-stale-busy-replay` — follow-up test coverage for an already-shipped fix. Lowest urgency.

**Queue top → bottom (active items, struck `#adoc-queue-ipc-drift` moved to bottom):**

1. `do [#duplicate-response-after-commit]`
2. `do [#adoc-queue-ipc-buffer-divergence]`
3. `do [#queue-strike-on-halt]`
4. `do [#pending-add-append-not-prepend]`
5. `do [#next-steps-backlog-capture]`

Note: `#prompt-duplicated-while-typing` and `#run-agent-doc-stale-busy-replay` are not in the queue — only in the backlog — so the queue's effective top is `#duplicate-response-after-commit` (the highest-priority item that is queued).

Top of backlog: `#prompt-duplicated-while-typing`.
Switch claude_model: opus to defer to Claude Code's definition of opus...which is currently Opus 4.8.
#spec-test-build-install-commit-push

### Re: Switch `claude_model: opus` to defer to Claude Code's opus (now Opus 4.8) — opus-4-8

Fixed. `opus` now resolves to **`claude-opus-4-8`** everywhere it matters.

**Root cause:** `CLAUDE_CODE_OPUS_MODEL` in `agent-doc-core/src/model_tier.rs` was pinned to `claude-opus-4-7`. That one constant is the single source of truth feeding three paths:

- the Claude Code `opus` alias (`claude_code_model_alias`) → launch `--model` injection in `start.rs`
- the built-in Claude Code **high** tier slot
- `### Re:` attribution via `resolve_agent_model` → `short_model_name` (`claude-opus-4-8` → `opus-4-8`)

So `claude_model: opus` was launching and attributing as 4-7 even though Claude Code's current opus is 4.8.

**Change (submodule `a2aea7b5`, on branch `20260422/codex-agent-backend`):**
- Bumped the constant `claude-opus-4-7` → `claude-opus-4-8` — propagates to alias, high tier, launch, and attribution from one line.
- Updated the lockstep tests: `model_tier.rs` (`resolve_builtin_claude_code`, `tier_from_model_name_builtin`, `parse_model_arg_*`, `canonical_model_name_expands_*`, `scan_model_switch_*`), `start.rs` (`model_injected_from_claude_model_frontmatter`), `preflight.rs` (`resolve_agent_model_expands_claude_code_opus_alias`).
- **Spec:** `SPEC.md` and `specs/02-document-format.md` "current Claude Code opus id" lines; added a `claude-opus-4-8` row to `runbooks/model-tier-gate.md`.

**Verification:**
- `make check` (clippy + full workspace tests) — green.
- `cargo install --path .` + `cargo build --release` — both binaries rebuilt (`.bin/agent-doc` → release, `~/.cargo/bin/agent-doc` → install).
- `agent-doc skill install --harness claude` — re-rendered installed skill/runbook surfaces.
- **End-to-end:** `agent-doc preflight` on this doc now reports `agent_model = opus-4-8` (was `opus-4-7`) from the unchanged `claude_model: opus` frontmatter. This response is attributed `— opus-4-8` accordingly.

**Committed + pushed:** submodule `a2aea7b5` (pushed to `origin/20260422/codex-agent-backend`); superproject `12fed421a` bumps the pointer + installed runbook (pushed to `origin/main`). Kept orthogonal install-regen churn (an unrelated `baseline-drift` catalog entry in the bundled SKILL.md, other regenerated runbooks) out of these commits.

Also folded in the queue/backlog priority reorder from the prior cycle — it had been left uncommitted by an IPC-proof miss on that finalize, so it commits with this cycle.

One follow-up filed to backlog (`#opus-alias-version-drift`, recommended): the constant still has to be hand-bumped per Claude Code opus release — worth deriving at runtime or gating in the release checklist.
<!-- agent:boundary:81caa660 -->
<!-- /agent:exchange -->
###
<!--
```
resync: stash pane %21 is registered in its own project root — skipping kill
resync: stash pane %2 is registered in its own project root — skipping kill
resync: stash pane %4 is registered in its own project root — skipping kill
resync: stash pane %20 is registered in its own project root — skipping kill
resync: stash pane %3 is registered in its own project root — skipping kill
[route] debounce OK — file idle for 0.5s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 233 for tasks/monsterrodholders.md is busy on pane %19; queued pending dispatch "complete [#nsbfrun]. #spec-test-commit-push" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
---
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

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#prompt-duplicated-while-typing]
- do [#duplicate-response-after-commit]
- do [#adoc-queue-ipc-buffer-divergence]
- do [#queue-strike-on-halt]
- do [#pending-add-append-not-prepend]
- do [#next-steps-backlog-capture]
- do [#run-agent-doc-stale-busy-replay]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#opus-alias-version-drift] [recommended] CLAUDE_CODE_OPUS_MODEL (agent-doc-core/src/model_tier.rs:50) is a hardcoded versioned constant that must be hand-bumped each time Claude Code redefines  (just moved 4-7 → 4-8). Consider deriving the current Claude Code opus id at runtime (e.g. probe the harness / env) so  attribution + launch never silently lag a release, or at minimum add a release-checklist gate. Low urgency: the constant is documented as the intended single source of truth.
- [ ] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing — in-progress prompt text appears twice in the buffer during/after typing, before any new agent-doc cycle. Distinct from #duplicate-response-after-commit (response body) and #adoc-queue-ipc-drift (queue/snapshot loop). Suspected: PatchWatcher.kt normalize+setText (lines 774/801) double-counts post-boundary keystrokes against the try_ipc() boundary-aware synthesized exchange patch payload; await_idle debounce narrows but does not close the race. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md (recommend Option A — single-source post-boundary user region + idempotent apply).
- [ ] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [ ] [#adoc-queue-ipc-buffer-divergence] Root cause #4 FIXED (submodule 729f7bc2): response-replay / stale-cycle hash matching now neutralizes the agent:queue component (tag + body) and the queue_active frontmatter, so queue-maintenance churn no longer wedges a committed cycle as stuck_captured_cycle. REMAINING #2 (live IDE buffer re-adds auto / diverges from committed inactive snapshot): blocker found — ipc_socket::send_patch carries component CONTENT only and cannot converge a queue opening-tag attribute (auto) or the queue_active frontmatter; try_ipc_full_content is disabled and rejects template docs, so no IPC seam exists yet. Needs a plugin protocol extension (PatchWatcher.kt + FFI) to converge queue tag + frontmatter to the editor buffer, plus a deterministic live-IPC-listener repro harness (SimWorld). Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [ ] [#queue-strike-on-halt] Queue head consumed on halt/refusal response. write.rs:1233-1242 + response_explicitly_targets_queue_head (write.rs:2481) treat any `### Re: <head-id> <suffix>` heading as completion signal even when body explicitly says "backlog left intact" + passes no --done/--pending-gate. Repro: tasks/agent-doc/open-code.md cycle 619af4d0b struck [#d97v] after halt response. Plan: tasks/agent-doc/plan-queue-strike-on-halt-response.md (recommend Option A — require explicit completion signal).
- [ ] [#pending-add-append-not-prepend] `--pending-add` appends new items below the previous CLI arg within the same finalize cycle, contradicting SKILL.md "Pending capture rule" ("Put new items at the beginning of agent:backlog"). Repro: tasks/agent-doc/open-code.md commit f94e7851c — agent added wave-4, wave-3, wave-2, wave-1 expecting wave-1 on top per SKILL.md; actual backlog was wave-4 on top (pure CLI-arg order). User flagged it as bug; agent had to issue --pending-reorder. Plan: tasks/agent-doc/plan-pending-add-prepend-semantics.md (recommend Option A — strict prepend, drop the implicit adjacency heuristic).
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
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
