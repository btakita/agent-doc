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
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260930-035337.md`*

Compacted content:
- Archived 14 response topic(s): Fix fpe.md issue; Fix fpe.md issue (paste landed; lint-gate fix); do [#syntheticstrike]; 11 more
- Prior summary/context: prior compacted content: Archived 31 response topic(s): infra.md "owned by pane %12, invocation from pane %3"; api.md...
<!-- agent:boundary:d3c2a789:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
- Fix https://github.com/btakita/agent-doc/issues/60
- Fix https://github.com/btakita/agent-doc/issues/61
- Fix https://github.com/btakita/agent-doc/issues/62
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog -->
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [agent-doc][operator-verify] Run the captured-splice live proof to completion (#activateinstalledjetbrai). BLOCKER RESOLVED 2026-09-29: the canonical advance was refused while the captured edit was unsaved in the editor; #retainedadvanceadmit (54452093d) admits a new cycle when the authority already holds the retained write and only its disk projection is pending (closeout still waits for disk). Recipe re-verified offline: 'scripts/xdotool-live-verify.sh captured-splice --dry-run' now prints every step and exits 0 (42fe78bd2 stopped it dying on the capture-receipt wait). Harness fixes from 095d2af54 stand (XTEST typing, absolute-path focus guard, tmp/live-repro scratch, fresh doc-scoped receipts). REMAINING: the live run injects real keystrokes into the operator's desktop, so it runs only on the operator's go-ahead ('run the captured-splice live proof'). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' exiting non-zero. Rechecked 2026-09-29 21:5x EDT: exit 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 152 releases (0.35.113..0.35.418), ~49,630 further post-cutover releases fit. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (acknowledgement bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
