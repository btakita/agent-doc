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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260610-165015.md`*

Compacted content:
- Archived 3 response topic(s): do #5b28; do #x9ds; #lvbremain
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#gng8] — gate-verify quoted-prose exclusion — fable-5

> **Queue prompt:**
>
> :round_pushpin: [#gng8]


Shipped. Root cause confirmed exactly as filed: scan_ops_log matched bare substrings against whole ops.log messages, and content-logging entries embed document text — every one of the 7 cross-session-reject hits in this project's ops.log is a queue_diff_active_prompt_differs line with the marker inside prompt_changes=["..."], so a gate could prove itself from its own backlog description.

**Fix (structural, no denylist):** embedded prose always arrives through {:?} debug formatting, which double-quotes it; genuine emissions ([claim] cross-session-reject pane_id=..., [ipc-socket] early_ack_pending ..., [s760] clear-decision ...) are plain unquoted message text. scan_ops_log now strips double-quoted spans (honoring \" escapes; an unterminated quote drops the rest of the line, fail safe) before matching proof/disproof markers. This generically covers queue_diff, route prompt={:?}, and session tail={:?} contamination without operators having to write longer predicates.

- agent-doc-core/src/gate_verify.rs — strip_quoted_spans + scan integration, module-doc prose-exclusion section, 4 new unit tests (quoted prose ignored for proof and disproof, structured marker still proves alongside prose mentions, escape/unterminated handling)
- agent-doc-orchestration/src/preflight.rs — run_gate_verify integration test: a queue_diff-style contaminated line leaves the gate pending, never auto-resolves
- agent-doc-orchestration/src/write.rs — comment marking the {:?} quoting at the queue_diff log site as load-bearing
- specs/07-closeout-commands.md — #optverify paragraph extended with the #gng8 prose-exclusion rule

**Verification:** make check green (exit 0 captured explicitly, 0 FAILED; remaining warnings are pre-existing in frontmatter.rs/template.rs test builds). Built + installed: cargo build --release, cargo install --path ., and agent-doc lib-install refreshed the JB cdylib — no .kt changed, so **no JB plugin reinstall is needed**. Submodule commit 7320a875 pushed to main; CI run 27292410729 is in progress (previous run green) — continuing on local evidence per the closeout rule. Superproject pointer bumped.

**Side finding:** preflight's stuck_captured_cycle warning this cycle was a false alarm — capture cycle-1781108818351 (the #lvbremain audit response) committed fine and was then compacted out of HEAD; its body is preserved in the 165015 archive. Do not run the suggested write --commit recovery. Filed as a new backlog item.

This fix also unblocks the auto-proof half of #ktw8/#lvbremain: structured markers now prove gates without prose false-positives, so operator-driven markers can be checked reliably next cycles.
<!-- agent:boundary:947604fb -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: [#gng8]~~
- :pushpin: monsterrodholders.md is having "IPC-drift hiccups". Do they relate to the next items or something new? I want to complete the full cut-over because the in-between state is causing too much churn. Just do it. Let me know if I should restart.
- :round_pushpin: [#ktw8]
- :round_pushpin: [#lvbremain]
- :pushpin: If I manually place an item onto the queue, automatically place a `:pushpin:` do it doesn't unexpectedly get deprioritized.
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
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

<!-- agent:backlog priority queue --> 
- [ ] [#8j86] agent-doc-bug (stuck_captured_cycle compaction false-alarm): preflight warns stuck_captured_cycle when a committed captured response was legitimately removed from HEAD by exchange compaction (body preserved under .agent-doc/archives/). The warning prescribes a write --commit recovery that would re-inject an archived response into the live exchange. Fix: before warning, check whether the capture body / response_sha256 appears in an archive for the doc hash (or a compaction landed after committed_at) and skip or downgrade to an informational line. Seen 2026-06-10 on agent-doc-bugs2.md capture cycle-1781108818351 right after compact.
- [ ] [#ktw8] operator live-verify (was gated review #nnrt / #s760d): destructive /clear path. On a throwaway doc with agent_doc_queue_context_reset set true, drive the exchange past the clear threshold; operator confirms ops.log shows a STRUCTURED [s760] clear-decision optIn=true ... clear=true line (not a prose mention) and /clear fires once at the boundary, no false fire below. Code shipped submodule 1edb20d2 (start.rs idle-queue watch + context_pct). Operator-drive (agent cannot fire a destructive /clear); auto-proof also blocked on the gate-verify prose false-positive bug. Converted from gated review per operator directive (no more gates; operator drives, agent checks logs).
- [ ] [#lvbremain] OPERATOR-DRIVE live-verify residual. Harness FIXED (#x9ds, submodule 9b22d576): the binary markers now reach ops.log as structured lines. Per-marker: #4wxr cross-session-reject DRIVEN+verified live (2 structured [claim] markers in ops.log); #9adk run_clear_coalesce is plugin-side (idea.log InvocationCoalescer), operator-verified via #lvbatch, NOT an ops.log marker; #saev early_ack_pending needs EARLY_ACK_ENABLED flipped (#saevon) + a live SOCKET-IPC drive (0 structured markers yet); visible_write_live_buffer_matches_disk needs a live editor buffer==disk write. Operator drives the remaining two; agent checks ops.log for STRUCTURED markers only (bare-substring scan false-positives on prose — see the gate-verify bug item).
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
