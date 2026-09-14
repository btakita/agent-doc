---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0a113-68bf-7131-b405-9035a3ada6e7
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

Editor-route layout supersede, controller bind-before-serve, the JB accept-leg bound, and the queued-clear mislabel are fixed and pushed (5577cfeeb, b0e9530a6, 232c6c103); make check 9316 passed. #fixrunlazily is gated on one IDEA restart to load plugin 0.2.380. Open: #tmuxautosyncwedge, #cargotokenrevoke.
<!-- /agent:status --> 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260914-210034.md`*

Compacted content:
- Archived 33 response topic(s): #timestampresponseheader; #idledispatchstack; preflight stall fixed; docs.md reverse-order lists traced to one cycle; 30 more
- Prior summary/context: prior compacted content: Archived 38 response topic(s): #actionable-review; #restartideaplugin restarted, do #pypiquo...
<!-- agent:boundary:44108b98:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

t## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#release035376verify] [release] Verify GitHub Release run 34737823202 completes green and v0.35.376 publishes all six platform assets; all matrix jobs were still externally queued at closeout.
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#fpecompactheal] [operator-verify] Confirm fpe.md's corrupted `<dynamic_context_ref>` block is gone after one compact on the FIXED binary (installed 0.35.367, ~/.cargo/bin/agent-doc at 23:01:39). Must be run FROM THE PANE THAT OWNS src/haiven-dev/tasks/fpe.md — that is a live session (b69e5aeb, `queue: go`) and this cycle deliberately did not hand-align its disk. Expected after `agent-doc compact src/haiven-dev/tasks/fpe.md`: exactly ONE block, with the two self-referential handles (absolute `.../src/haiven-dev/tasks/fpe.md` and relative `tasks/fpe.md`) gone, leaving at most the four instruction-file handles; and the `Prior summary/context:` line still summarising the prior compact list rather than quoting XML.
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate installed JetBrains plugin 0.2.379, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. Automated lifecycle and transition regressions plus make check tmux-ci passed; source v0.35.362 is published. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Installed JetBrains plugin 0.2.379 is staged while running IDEA PID 1035669 remains on 0.2.378; one restart activates both the reverse tmux-focus fix and the current package generation. Remaining splice proof: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028, and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1035669 runs plugin 0.2.378; CLI/native 0.35.361 is installed. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. The unrelated Stop-hook gate-selection defect is fixed in v0.35.361; plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ha3hpolicy] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml receive.auto_create_components = ["pending"] (or = false); flip notify/write default from auto-create to consult-recipient-config. Hard-blocked until multi-user workspace support exists; issue #40 (open) explicitly says not to ship before that prerequisite. Moved out of agent:review because it is an unstarted future feature, not code-complete work awaiting validation. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
