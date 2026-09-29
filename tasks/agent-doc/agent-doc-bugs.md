---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: e0d7af8d-d8cc-4727-885f-25e6e9f37b87
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260929-003942.md`*

Compacted content:
- Archived 32 response topic(s): complete #confirmfixlive; do #opcapturedormant; monsterrodholders migrate nag; 29 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): queue prompt on cargovodtrim; actionable review items into bac...
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#drivecapturedsplice]
- do [#publishjetbrainssibling]
- do [#fpestopreplay]
- do [#pastetriggeradmit]
- do [#nameelseclaude]: This happened after transitioning from `agent: claude` to `agent: codex`
- do [#focuscrossrootverify]
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#fpestopreplay] [agent-doc] Codex Stop hook replayed Codex's closing console status into a committed document as two ❯ operator prompts (src/haiven-dev/tasks/fpe.md, 2026-09-28; repaired by removing nodes p:48ccd96c2ccb/p:9c5170502608, commit 4a2c4b37). Sequence from ops.log: 23:01:23 codex_stop_post_commit_prompt_cycle_reopened source=replayable_stop_payload behind the already-committed #advance-review response → codex_stop_capture_saved (raw last_assistant_message, no heading) → the hook's own repair (which wraps bare prose as '### Re: recovered response') lost the closeout lease to captured_finalize_resume in controller pid 165487, flagged as a stale binary → that resume replayed the capture unwrapped → normalize_user_prompts applied=2 → Codex's agent-doc commit committed 43acc513 and session-check wedged on an unanswerable prompt tail. Two fixes were tried and reverted: gating post-commit replay on a ### Re: heading (plain prose is the normal Codex answer shape, pinned by tests), and wrapping at capture time (breaks ordinary_same_thread_followup_after_clean_closeout_auto_closes_fresh_cycle, because replayers attach a prompt-targeting heading themselves). Next: (1) reproduce against the CURRENT captured_finalize_resume to learn whether it lands a raw Stop capture without a heading, and fix it to wrap with the prompt-targeting heading if so; (2) decide whether a recurring queue head its committed predecessor already answered should ever reopen a cycle from a Stop payload.
- [ ] [#pastetriggeradmit] [agent-doc] [recommended] The UserPromptSubmit hook treated a pasted transcript whose first line was `agent-doc tasks/fpe.md` as an agent-doc trigger, resolved the path against the session's own root (agent-loop) rather than the pasted doc's root (src/haiven-dev), and refused admission with 'failed to canonicalize .../agent-loop/tasks/fpe.md'. Harmless here only because the running cycle was already admitted. A trigger should be the whole prompt, or at minimum a first line followed by nothing but whitespace; a multi-line paste that merely begins with the command text is operator content to reason about, not an invocation.
- [ ] [#nameelseclaude] [agent-doc] Name what else has a Claude Code conversation open instead of leaving the operator with a bare "This conversation is open in another app / press R to continue" lock. Operator-reported 2026-09-28 on src/haiven-dev/tasks/api.md. The information is already on disk and agent-doc can read it: `~/.claude/sessions/<pid>.json` carries `sessionId`, `pid`, `cwd`, `tmux` (session:window.pane), `entrypoint` (cli / sdk-ts), `kind`, `name`, and `status`, so a holder can be named exactly, and liveness proven with /proc/<pid>. Verified at the time of the report: the four live haiven-dev CLI records held four DISTINCT session ids, so agent-doc had not routed a second client onto the same conversation — the other holder was not a registry-visible CLI, leaving the Chrome native host (pid 281712) / desktop / web client as the remaining shape. Deliverable: surface live holders of a document's conversation (agent-doc session status and/or doctor), report stale records whose pid is dead, and say plainly when no second CLI holder exists so the operator knows to look outside the terminal.
- [ ] [#focuscrossrootverify] [operator-verify] [agent-doc] Confirm the cross-root editor focus delegation live (`#focuscrossroot`, 730d74600). Agent-side is complete and verified: 6 tests, make check green, installed and pushed. The remaining step needs a human at a real IDE because only the editor surface reproduces it — the CLI `agent-doc focus` already resolves the document's own project root and never took the broken path. UNBLOCK: after the controllers pick up 730d74600 at their next idle boundary, switch an IDE tab (rooted at the agent-loop superproject) to a submodule document such as src/haiven-dev/tasks/api.md and expect its pane to take focus, with `controller_focus_delegated document=... owner_root=.../src/haiven-dev focused=true` in agent-loop's .agent-doc/logs/ops.log and no further `reason=missing_actor_record` for that document.2
- [ ] [#publishjetbrainssibling] [agent-doc] Publish a `-signed` JetBrains sibling from the release `plugins` job. The client half shipped in 7649ccafd — `find_asset` now matches `<prefix>*-signed.<ext>` by shape, so a signed asset would be selected the moment one exists — but the job publishes only the unsigned zip. `signPlugin` in editors/jetbrains/build.gradle.kts reads CERTIFICATE_CHAIN / PRIVATE_KEY / PRIVATE_KEY_PASSWORD from the environment and none are available to that job, so wiring `signPlugin` in today would fail the release rather than produce a sibling. UNBLOCK: decide whether either IDE host actually requires a signed package, and if so add those three repo secrets; then add signPlugin + a second cp to the plugins job and extend the two-asset verify to four. [recommended]
- [ ] 🚧 [#drivecapturedsplice] [agent-doc] Run the captured-splice live proof to completion (#activateinstalledjetbrai). HARNESS FIXED 2026-09-28 (095d2af54): the old 'xdotool keystrokes always arrived' premise was false — `type --window` goes through XSendEvent and JetBrains drops send_event=True keys, so nothing ever reached the editor. The harness now types via XTEST under an absolute-path focus guard, keeps scratch docs in tmp/live-repro/ (under .agent-doc/ they became a nested project with their own controller and log), waits only on fresh doc-scoped receipts, pipes a real response into the advance, runs it in the scratch doc's owner pane, and seeds a lint-valid boundary. A live run then produced the first fresh doc-scoped `editor_op_capture_proof operator_ops=N shadow_replay=agreed` any scratch doc has carried. REMAINING BLOCKER: the canonical advance is refused with 'retained document-write delivery from a prior cycle remains unsettled' while the captured edit is still unsaved in the editor (native_editor_save_request_deferred; authority/disk diverge on exchange). Decide whether an independent advance must be admitted while a captured operator edit is retained — that admission is what this recipe exists to prove — and fix whichever side is wrong. Re-run from a desktop with no stale-binary supervisors and a CLI build matching the IDE cdylib (the runs saw crdt_replica_build_mismatch_recovery_requested x4). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. As of 2026-09-27 that check exits 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases — so there is nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
