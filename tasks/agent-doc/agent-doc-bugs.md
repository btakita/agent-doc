---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260927-043821.md`*

Compacted content:
- Archived 7 response topic(s): queue prompt on cargovodtrim; actionable review items into backlog; pypiquotafix complete; 4 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): #advance-review, tmux pane auto-focus, and queue edit converge...
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
- [#confirmfixlive]: confirmed
- do [#opcapturedormant]
- do [#bkqattrcli]
- do [#hswspawncount]
- do [#hswdisposabledoc]
- do [#splicesimcov]
- do [#pypiquotarecheck]
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#confirmfixlive] [agent-doc] [operator-verify] Confirm the #focusstashescalate fix live: after the controller recycles onto 0.35.418 + agent-doc 75963eec6, click between two visible session documents whose tmux panes are in DIFFERENT windows (one in 0:agent-doc, one in 1:stash) and confirm the second and every later switch focuses, not just the first. Evidence is now self-describing: ops.log carries controller_editor_surface_focus_outcome document= focused= reason= pane= for every selection, plus controller_editor_surface_focus_escalated when a stashed pane is handed to the layout owner. If a switch still does nothing, the reason field names which of the ~10 refusals fired.
- [ ] [#opcapturedormant] [agent-doc] [P1] The editor op-capture ledger records NOTHING: across 2026-09-24..27 this project logged 216 editor_open and 317 editor_op_epoch_closed events — every single one cause=non_operator_projection action=cleared — with ZERO editor_op_recorded, ZERO editor_ops_recorded, ZERO editor_op_record_failed and ZERO editor_ops_record_failed. Zero successes AND zero failures means editors/jetbrains/.../TypingTracker.kt:527 (`lib.agent_doc_record_editor_ops_json`) is never reached, not that it is failing. This is not a no-typing artifact: the operator typed four separate queue heads into tasks/agent-doc/agent-doc-bugs.md in IntelliJ on 2026-09-26/27 while it held a live registration (pid 3129637, plugin 0.2.428), and those edits arrived as crdt_document_op_delta_ingested deltas without ever being recorded as captured ops. Consequence: every captured-local-splice-recovery invocation (15 in the same window, all status=current, on agent-doc-bugs.md and tasks/software/lazily.md) ran against agent-driven projections only, so no operator-captured edit has ever exercised the splice path in production, and #activateinstalledjetbrai cannot be proven while this holds. NEXT: instrument or read the TypingTracker guard chain to find why the batch record call is not reached (candidate causes: the op epoch being closed as non_operator_projection before the operator burst is flushed, a base-hash memo miss, or the tracker not being installed for the document); note that a bare absence is now diagnosable because verify-op-capture names both producers as of 6b0eabc0c. Blocks #activateinstalledjetbrai and #splicesimcov.
- [ ] [#activateinstalledjetbrai] [agent-doc] [operator-verify] Prove captured local editor edits recover across an independently advanced canonical response — BLOCKED ON #opcapturedormant, and no longer validatable from logs. Asked to validate from logs 2026-09-26 and the answer is that the evidence class is ABSENT, not inconclusive: the splice-recovery path itself is healthy (15 captured-local-splice-recovery invocations 2026-09-24..27, all status=current authority=cp_model, with the canonical text demonstrably advancing between them — the 2026-09-26T00:11:48..52 burst runs text_len 42885 -> 42885 -> 42892 -> 42900 across three distinct hashes), but all 317 editor_op_epoch_closed events in that window are cause=non_operator_projection, so not one of those recoveries carried an operator-authored edit. Order of work: (1) fix #opcapturedormant so operator edits are recorded as ops at all; (2) re-run `agent-doc verify-op-capture <FILE>` — it now accepts the batch producer the JetBrains TypingTracker actually drives (6b0eabc0c), so a pass is meaningful and a failure names both producers; (3) only then does the disposable-document live proof have anything to prove. Do NOT re-narrow this gate to "just needs the live check" until step 1 lands. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [ ] [#bkqattrcli] [agent-doc] No command can add a component attribute, so "into backlog + queue" is unreachable from the CLI on any document whose `agent:backlog` marker lacks `queue`. Verified 2026-09-26 on a scratch copy of tasks/agent-doc/agent-doc-bugs.md: two `--backlog-add` items landed in agent:backlog and mirrored NOTHING into agent:queue. `--backlog-queue-placement` is documented as acting only "when the backlog carries a `queue` attribute" and `agent-doc queue sync` is "sync from backlog items with `queue` attribute", so both are gated on it; `agent-doc backlog` has no set-attr, `write` exposes no marker flag, `patch` replaces component CONTENT only, and `migrate` only STRIPS deprecated attributes. The one remaining route is hand-editing the marker, which is unavailable precisely when it matters: reliable-sync-status shows this document live under IntelliJ pid 3129637 / plugin 0.2.428, and a raw disk edit under a live editor becomes a disk candidate the agent must never resolve. Add `agent-doc backlog set-attr <FILE> <attr>` / `unset-attr` (accepting at least `queue` and `priority`) routed through the same editor-converging write path as `backlog add` — the marker span is already modelled as Component.open_start/open_end with parsed attrs in agent-doc-element/src/element.rs (parse_attrs at :736), so this is a marker-span mutation class rather than a content patch. Also emit a preflight warning when a capture cycle adds backlog items that the queue cannot mirror, so the "+ queue" half of an actionable-review-style instruction fails loudly instead of silently landing half.
- [ ] [#hswspawncount] [agent-doc] Add a SimWorld scenario asserting exactly ONE supervisor spawn per authoritative harness switch across a consecutive codex -> claude -> codex sequence in ONE document, with no refusal and no respawn storm. Rescoped 2026-09-26 after #harnessswitchverify was discharged from logs: the 1:1 invariant is already PROVEN live (3 harness_change_detected events 2026-09-24..27, all gate=WaitForBoundary, each producing exactly 1 agent_restart_performed action=spawn_fresh_harness — claude->codex on lazily.md 04:12:03->06Z, codex->claude on lazily.md 18:12:09->10Z, codex->claude on agent-doc-bugs.md 18:12:29->33Z with authority=editor_buffer), and the deferral half too (5 agent_restart_respec_refreshed with running_harness == resolved_harness produced ZERO spawns). What remains unproven is only a CONSECUTIVE multi-switch sequence on a single document, which is a deterministic-coverage job rather than another manual session. Grounding: four scenarios already exist (src/sim_world.rs:6556 persists the record so post-restart dispatch does not defer, :6663 accepted handoff then idle-watch fresh restart, :6769 paused supervisor holds for resume no restart, :6856 disabled restart bails explicitly) but NONE asserts a spawn COUNT over a multi-switch sequence. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [ ] [#hswdisposabledoc] [agent-doc] [recommended] Scaffold a disposable session document plus a one-screen operator script for live harness-switch checks. DOWNGRADED 2026-09-26: the live proof this was staging is discharged — #harnessswitchverify was resolved from ops.log evidence (3 detections, 3 spawns, 1:1, editor-buffer authority on the agent-doc-bugs.md switch, 5 non-authoritative respec refreshes with zero spawns), so nothing is blocked on having a throwaway document. Still worth having for future live editor checks that genuinely need one (for example the #tmuxfocussyncverify click or a same-document codex -> claude -> codex round trip if #hswspawncount's deterministic scenario is ever judged insufficient): `agent-doc init` a throwaway with `agent: codex` frontmatter and record which frontmatter line to edit unsaved plus the receipts to expect (harness_change_detected gate=WaitForBoundary, then exactly one agent_restart_performed action=spawn_fresh_harness).
- [ ] [#splicesimcov] [agent-doc] Add a cross-editor SimWorld scenario for captured-edit splice recovery across an INDEPENDENTLY ADVANCED canonical response — the single remaining proof in the #activateinstalledjetbrai review gate. Grounding (2026-09-26): tests/cross_editor_simworld.rs holds only three tests (cargo_target_debug_dir_honors_default_relative_and_absolute_targets, cross_editor_plugin_protocol_harnesses_peer_through_real_agent_doc_controller, native_plugin_harnesses_peer_through_real_agent_doc_controller) and none exercises the splice path; per tasks/agent-doc/plan-captured-editor-splice-recovery.md the native side is proven only through the ABI test plus one manual installed-native call. Cover the plugin-side sequence the plan describes: observe the controller cut, plan before replacing membership, CAS-register that cut, forward bounded splices, and assert the translated offsets land while an unrelated response change is in flight. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [ ] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision so the next PyPI ceiling is raised rather than cleared by another mass deletion. Context changed 2026-09-26: #pypiquotafix is DONE — the 34 oldest releases (0.35.0 through 0.35.112, 1.63 GiB) are deleted and PyPI settings now reports 8.4 GiB of the 10.0 GiB limit, so there is no urgency. #12214 is still open with only the github-actions bot auto-reply from 2026-09-10T22:46:21Z. Growth is already bounded: the 20 releases at or after the 0.35.383 thin-wheel cutover total 0.7 MiB, against roughly 68 MiB apiece before it, so the remaining 8.36 GiB is 131 historical fat wheels and is not growing. If the grant lands, record it and drop this item. If it is refused or stalls past the next ceiling, the next clean tranche is 0.35.113 through 0.35.152 (another 2.10 GiB) — but do not delete anything until a ceiling is actually in reach. Verify deletions from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json, which served a stale CDN view claiming 27 of the 34 deleted releases were still present.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
