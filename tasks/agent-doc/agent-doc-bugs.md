---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': '/goal do backlog items'
  '#gh-fix': 'fix then close'
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261001-134419.md`*

Compacted content:
- Archived 9 response topic(s): do [#refusalsteeringverbatim] + the lazily.md Stop-hook failure; #gh-fix GH #76 + fixing agent-doc's causes of the recycle churn; do [#stopreplaynoopretained]; 6 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): #advance-review; Drive Idea for #drivecapturedsplice; #coder7...

### Re: #gh-fix GH #77 (tmux shows 1 pane while the editor shows 2 documents) — opus-5.5 · 2026-10-01T10:05-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/77

**Fixed and closed.** Commit `52b43df8f` in agent-doc, JetBrains plugin 0.2.455. It is pushed and installed (`make install`, which also reloaded the plugin in IDE pid 2960722).

**Cause:** the JetBrains plugin works out which editor splits sit side by side from where each split appears on screen. A JetBrains Remote Dev backend never lays its splits out, so both report position (0,0).

- The plugin grouped them into **one** column.
- One column counts as "no layout detected", so the surface sent to the controller carried no columns.
- The controller then synced tmux to the focused document alone. The result was one pane whose document swapped on every switch.

That one cause explains both symptoms in the issue: the single pane and the focus that never moved.

**Fix:** two splits that are actually laid out can never share a position, so a shared position now means the geometry is unknown. Each split then stays its own column. When this happens, the plugin logs `[layout-detect] … share an origin; geometry unavailable` to idea.log at info level, so a live run can confirm the cause. The diagnosis started in the turn before the `/clear`, which left uncommitted plugin changes in the tree. I checked that work against the code path, extended it with the log line, and shipped it.

**Verification:**
- New regression test `LayoutDetectorTest … keeps unlaid-out splits apart`.
- JetBrains plugin suite: 603 tests, 0 failures.
- `make check`: exit 0, 10280 passed. The first run had 4 failures in `agent-doc-controller-io` dispatch tests. A second `make check` left over from before the `/clear` was running at the same time. Those tests pass alone, the crate passed twice in full parallel runs, and the rerun with no other suite running was all green.
- CI for the push is still running.

**Still to check (by you, on the Remote Dev host):** this is not confirmed live yet. Install plugin 0.2.455 there, then look for the `share an origin` line in idea.log and for `panes=2` in tmux. The issue comment says to reopen #77 if the line is missing and tmux still shows 1 pane.
<!-- agent:boundary:4df2c95c:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/77~~
- #gh-fix https://github.com/btakita/agent-doc/issues/78
- :pin: #release
- Look at all the old gh issue that I created. Close any of the issues that are no longer relevant. Anything relevant should be added in the backlog.
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice live proof (#activateinstalledjetbrai). LIVE RUN 2026-09-30 20:33Z (operator go-ahead): after harness fixes 01b1e30b7, the recipe drove IDEA end to end — edit one captured (editor_op_capture_proof operator_ops=37 shadow_replay=agreed, 20:33:01Z), canonical advanced independently via write --commit in owner pane %93 (20:33:02Z, editor acknowledged the target), edit two captured (operator_ops=36 agreed, 20:33:12Z); the final text holds every operator edit plus the advance. No text lost. The verifier still FAILS: its proving triple needs two source=captured-local-splice-recovery reads, and the plugin only takes that path (CrdtReplicaManager rebootstrapCanonicalAndForwardCapturedLocalEdit) when its replica is STALE against the canonical text. Healthy delivery reached the editor before edit two, so the recovery never had to run. DECISION NEEDED: (a) accept the healthy-path trail as the proof, extending verify-captured-splice-recovery with a capture/advance/capture triple, or (b) require the stale-replica race, making the recipe type edit two while the advance is still in flight. Aside: the scratch doc is under gitignored tmp/, so every advance ends commit_refused_ignored_path (write lands on disk, commit refused). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' reporting a ceiling in reach. Rechecked 2026-09-30 16:25 EDT: 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 158 releases (0.35.113..0.35.424), post-cutover mean 34.7 KiB, ~49,485 further releases fit; VERDICT headroom fine. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (github-actions bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
