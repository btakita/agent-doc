---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 322cd20d-d440-49dc-9da0-c9fd53bfabbb
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
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260929-230356.md`*

Compacted content:
- Archived 31 response topic(s): infra.md "owned by pane %12, invocation from pane %3"; api.md reverted by queue updates; #pastetriggeradmit; 28 more
- Prior summary/context: Fix api.md issue ``` • Failed (exit 1) agent-doc write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/ta...
<!-- agent:boundary:d3c2a789:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#editorauth2]: ensure plugin parity~~
- ~~do [#editorauth3]~~
- ~~Fix multiple `agent-doc: awaiting response` prompts in lazily.md: ![img_2.png](img_2.png)~~
- 🚧 📌 do [#layoutobservehandoff]
- 📌 do [#syntheticstrike]
- 📌 do [#focuscrossrootverify]
- 📌 do [#retainedadvanceadmit]
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#frontmattersessionflap] [agent-doc] agent-doc-bugs.md frontmatter `claude:` session id changed three times in ~30 minutes on 2026-09-29 (72fd5a16 -> 500fe7a4 -> edbb61e8) with no new Claude session; each rewrite is post-commit drift that makes a later close refuse (commit_document_via_controller_error 22:44:30Z) and start_session_id_record_failed appears around it. Find the writer (session-id capture on /loop re-entry or hook), make it write only on a real session change, and add regression coverage.
- [ ] [#editorauth2] [agent-doc][jetbrains][vscode] Plugin registration never adopts canonical over a buffer that differs, in BOTH editor plugins (operator: ensure plugin parity) (plan: tasks/agent-doc/plan-editor-authority-ladder.md). Remove every ApplyCanonical-over-differing-buffer outcome in JetBrains and VS Code, preferably in the shared FFI registration path so both plugins get one behaviour; registration publishes the editor state and then merges forward. The controller side landed in #editorauth3 (4d9e78578): on editor_replica_reregister the plugin must re-register WITH its retained state vector (resumeState) so the controller's cold-relay reseed applies, and must never drop that state on forceRefresh/requireFreshRegistration. Also covers two or more editors: an unregistered editor joining a relay that already holds one still gets the fenced canonical projection; reconcile it through reconcileText instead of adopting. Regression test: replay of the api.md 2026-09-29 15:35 sequence (acked typing, then quarantine, then handoff, then re-register) against both plugins, plus an editor-parity check.
- [ ] 🚧 [#layoutobservehandoff] [agent-doc] `pane layout observation refused: controller not authoritative (handoff_state=Preparing)` was shown to the operator on src/haiven-dev/tasks/api.md during an install-driven controller handoff (2026-09-29 ~14:50). A layout observation that arrives during a handoff must be deferred to, and applied by, the successor controller, not refused to the editor. The controller was `stable` again once the handoff finished.
- [ ] [#syntheticstrike] [agent-doc] An answered free-text queue head is never struck when its closeout commits as a synthetic cycle. agent-doc-bugs.md 2026-09-29 14:28: the capture belonged to cycle-1790704937323, but the commit landed as synthetic-1790706539439. `active_captured_response_for_queue_strike` requires `closeout.cycle_id == capture.cycle_id` and an open phase, so the controller's answered-free-text Computed never fired and "Fix …/issues/59" stayed live with a 🚧 marker. The pure projection strikes it correctly (probed with the real inputs and baseline). Fix the capture-to-synthetic-cycle ownership, or strike from the committing synthetic cycle when it committed that capture's response.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
- [ ] [#focuscrossrootverify] [operator-verify] [agent-doc] Confirm the cross-root editor focus delegation live (`#focuscrossroot`, 730d74600). Agent-side is complete and verified: 6 tests, make check green, installed and pushed. The remaining step needs a human at a real IDE because only the editor surface reproduces it — the CLI `agent-doc focus` already resolves the document's own project root and never took the broken path. UNBLOCK: after the controllers pick up 730d74600 at their next idle boundary, switch an IDE tab (rooted at the agent-loop superproject) to a submodule document such as src/haiven-dev/tasks/api.md and expect its pane to take focus, with `controller_focus_delegated document=... owner_root=.../src/haiven-dev focused=true` in agent-loop's .agent-doc/logs/ops.log and no further `reason=missing_actor_record` for that document.2
- [ ] [#retainedadvanceadmit] [agent-doc] Advances #drivecapturedsplice. The captured-splice live run is refused at `agent-doc-write-runtime-io/src/lib.rs:3135` ("retained document-write delivery from a prior cycle remains unsettled") while the captured operator edit is still unsaved in the editor (`native_editor_save_request_deferred`; authority and disk diverge on `exchange`). Decide whether a separate canonical advance must be admitted while a captured operator edit is retained. The recipe exists to prove that admission. Fix whichever side is wrong: the admission guard, or the harness ordering. Add SimWorld coverage for "retained capture + independent advance". Then rerun the recipe from tasks/agent-doc/plan-captured-editor-splice-recovery.md with a CLI build that matches the IDE cdylib.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [agent-doc] Run the captured-splice live proof to completion (#activateinstalledjetbrai). HARNESS FIXED 2026-09-28 (095d2af54): the old 'xdotool keystrokes always arrived' premise was false — `type --window` goes through XSendEvent and JetBrains drops send_event=True keys, so nothing ever reached the editor. The harness now types via XTEST under an absolute-path focus guard, keeps scratch docs in tmp/live-repro/ (under .agent-doc/ they became a nested project with their own controller and log), waits only on fresh doc-scoped receipts, pipes a real response into the advance, runs it in the scratch doc's owner pane, and seeds a lint-valid boundary. A live run then produced the first fresh doc-scoped `editor_op_capture_proof operator_ops=N shadow_replay=agreed` any scratch doc has carried. REMAINING BLOCKER: the canonical advance is refused with 'retained document-write delivery from a prior cycle remains unsettled' while the captured edit is still unsaved in the editor (native_editor_save_request_deferred; authority/disk diverge on exchange). Decide whether an independent advance must be admitted while a captured operator edit is retained — that admission is what this recipe exists to prove — and fix whichever side is wrong. Re-run from a desktop with no stale-binary supervisors and a CLI build matching the IDE cdylib (the runs saw crdt_replica_build_mismatch_recovery_requested x4). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. Rechecked 2026-09-29: exit 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases. Nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
