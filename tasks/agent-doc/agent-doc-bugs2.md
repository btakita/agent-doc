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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260608-164619.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do #ipc-crdt-response-drift — opus-4-8

> **Queue prompt:**
>
> do [#ipc-crdt-response-drift]


Worked the bug from the merge layer down. Findings + what landed:

**Reproduced and fixed at the CRDT merge layer.** I built deterministic reproductions of the reported drift directly against `agent_doc_core::crdt::merge`:
- A **stale/divergent `theirs`** (a stale editor live-buffer — preflight literally logged "live buffer convergence send failed" this session — or a concurrent foreign supervisor) that has *lost* a committed `### Re:` block makes the line-diff merge honor that foreign deletion and strip the committed response. That is exactly the reported "`### Re:` header missing / prior response lost" symptom. **Fixed** in `src/agent-doc` `c9d276b9`: `merge()` captures committed response headings from the original base, and `guard_committed_responses()` restores `ours` when a committed heading was dropped and `theirs` introduced no genuinely-new non-transient content. A negative test proves it never clobbers real new user input.

**Narrowed the rest.** The core merge is **robust for every `theirs ≥ base` shape** I could construct — foreign tail append, and a concurrent multi-line foreign block at the *same* boundary insertion point both merge correctly (no reversal, no hoist, header intact). So the remaining **reversed-lines / hoisted-above-prior** symptom under `theirs > base` (the production repro had `theirs=6386 > base=5797`) is **not** in `crdt::merge`. It lives in the layer that produces/applies the merged text: IPC node-patch application (`ipc_socket.rs`), boundary-aware template append, and the visible-buffer guard / stream write-back — plus write-back serialization across the multiple supervisors that were observed racing the superproject.

**Tests / verification:** 3 new regression tests in `crdt.rs`; `make check` green (**4010 passed, 0 failed**); latest CI run green.

**Not done — gated.** The merge fix is **not released** (no `cargo install` / `build --release` this turn): making an unvalidated merge-semantics change the live binary would jump the manual-testing gate, and the plan explicitly forbids editing the live write path while *this* document is mid-drift. The remaining IPC-layer fix needs a **clean quiescent session** to reproduce live — this session was actively racy (session drift across 2 tmux sessions, 10 foreign-owned docs, degraded IPC listener). I gated `#ipc-crdt-response-drift` with that exact unblock condition rather than overclaim it done.

### Re: false positives — fix this + install — opus-4-8

> **Queue prompt:**
>
> "the plan explicitly forbids editing the live write path while *this* document is mid-drift" I did not edit the document. You are getting false positives. fix this + install


You're right, and thanks for the correction — those were false positives, not your edits. Found and fixed the root cause, installed.

**Root cause.** The visible-buffer guard (`guard_visible_write_idle_and_current` → `live_buffer_diverges_from_content`) fired whenever the editor's live-buffer sidecar digest differed from current disk — with **no staleness check**. The editor plugin stamps that sidecar with its buffer on every modification. A mismatch against disk has two causes:
1. the editor holds **genuine unsaved edits** ahead of disk (must protect), or
2. a **concurrent foreign writer** (another supervisor, or agent-doc's own machinery) changed disk *after* the editor last reported — so the digest merely **lags** a disk change the editor never made. **Not a user edit.**

The guard conflated the two and failed closed on case 2 — exactly what hit you ("I did not edit the document").

**Fix** (`src/agent-doc` `51b9ed40`, `debounce.rs`): `live_buffer_diverges_from_content` now compares the sidecar's `timestamp_ms` against the on-disk file mtime. When disk was modified clearly later than the editor's last report (past a 250ms skew margin), the digest describes superseded content and **cannot** represent unsaved edits against current disk, so it's suppressed. Genuine unsaved edits (editor newer than disk) are still protected — proven by a paired test. This also retires the old manual `rm` stale-sidecar recovery.

**Tests / verification:** 2 new regression tests + the existing one green; `make check` green (**4012 passed, 0 failed**).

**Installed** as you asked: `cargo build --release` + `cargo install --path .` + `agent-doc lib-install` (binary, `.bin` release target, and the JB-plugin FFI `.so` — all 0.34.0). This very closeout runs on the fixed binary.

This is the visible-buffer slice of `#ipc-crdt-response-drift` (now fixed + installed, alongside the earlier merge-layer fix `c9d276b9`). Still gated for the remaining reversed/hoisted symptom under `theirs>base` and cross-supervisor write-back serialization, which need a clean-session live repro.
<!-- agent:boundary:4fba1a98 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- We need to use the AST + document model to understand where the document changes occurred and who made the changes. So if you made a change, even uncommitted, that should not cause drift. If I made a change in an area that does not effect the current turn, such as adding text into a comment, another queue item, etc, the drift should just me added to the model + sent to the durable sqlite db...but the turn should not be affected.
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
1. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED + INSTALLED: (1) merge layer c9d276b9 — crdt::merge protects committed ### Re: history from a stale/divergent theirs; (2) visible-buffer layer 51b9ed40 — live_buffer_diverges_from_content suppresses a stale live-buffer sidecar (disk modified after the editor last reported = lagging foreign disk write, not a user edit), killing the false-positive 'visible editor buffer differs' finalize block. Both shipped via cargo install + build --release + agent-doc lib-install (0.34.0). REMAINING (still gated, needs clean-session live repro): reversed-lines/hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs), and write-back serialization across concurrent supervisors. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
2. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
