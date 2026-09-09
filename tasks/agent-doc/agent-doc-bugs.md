---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a083a9-7606-73c2-a46f-aa9057555481
agent: codex
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

0.35.345 installed, committed, pushed, and tagged. Queue response-evidence guard and issue #50 fixed. GitHub Release/CI running; PyPI publication gated on the 10 GB project quota.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260909-001651.md`*

Compacted content:
- Archived 10 response topic(s): monsterrodholder.md stability; Response; #mrhsyncslowpath; 7 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): fenced closeout reports becoming queue work; response copied i...

### Re: Response

Fixed and installed **0.35.344**.

- Auto-sync now retains managed-pane ownership across tab changes; live layouts are converging automatically.
- STALE SUPERVISOR titles are pane-scoped and reactively refreshed.
- Upgrade requests survive history compaction, and acknowledgments stop repeated recycling.
- All 16 supervisors adopted the installed binary, preserving their harness children. No forced restart was needed.

Ownership decisions now live in the controller’s reactive graph; the legacy recycle ledger remains a compatibility boundary for older supervisors.

Verification: 9,242 tests, 8 live tmux tests, and CI passed. Changes are pushed and tagged; GitHub Release/PyPI publishing is running.

[operator-verify] Switch editor tabs once to confirm the visible pane behavior.

### Re: #fixqueueitem — gpt-6

Fixed the missing queue-completion evidence path in agent-doc **0.35.345**.

The previous closeout used a generic `### Re: Response` heading without identifying its free-text queue prompt. The conservative completion matcher therefore had no exact answer evidence and left the item active. Strict write now rejects that shape before capture and names the exact `> **Queue prompt:**` quote required with the result or concrete deferral. Selection alone still cannot mark unfinished work complete.

`agent-doc-queue::queue_closeout_guard` owns this pure policy. Tests cover missing evidence, an exact answer, an explicit deferral, unselected/struck/id-backed heads, foreign exchange steering, and rejection without capture or document mutation. The existing document-scoped completion Computed and Effect still own the strike. Pre-capture validation is an immutable-candidate check with no new cache or retry loop; an already-captured stream is not revalidated against later selection changes.

### Re: #fixhttpsgithub — gpt-6

> **Queue prompt:**
>
> do [#fixhttpsgithub]. Ensure plugin parity for the last few releases. Also track what we have parity on and ensure we are always at parity.


> **Queue prompt:**
>
> do [#fixqueueitem]


Fixed [issue #50](https://github.com/btakita/agent-doc/issues/50), committed and pushed as **dfb9db790**, and tagged **v0.35.345**. The CLI/native library and editor packages are installed locally; JetBrains **0.2.375** contains the loader fix.

`lib-path` now skips startup upgrade notices. JetBrains parses stdout only and inherits stderr separately, so warnings from older binaries cannot become library paths or fill an unread stderr pipe. Failed resolution records the candidate, exit status, and existence check. The subprocess remains an actorless bootstrap boundary; no polling or controller policy was added.

Verification passed: **9,245 Rust tests**, **507 JetBrains tests**, full `make check` including release audits and formal checks, formatting, private-name checks, signed plugin build, and `make install-full`. The seeded newer-release cache regression proves the library-path output stays clean. CI and GitHub Release workflows were in progress when inspected after the push.

**PyPI publication remains externally blocked.** The preceding upload failed with HTTP 400 because `agent-doc` reached its **10 GB project storage limit**. The new tag triggered PyPI again, but quota space is still required. Gated **#fixhttpsgithub** retains the remaining step: obtain a quota increase or approved storage cleanup, then rerun `gh workflow run PyPI --ref v0.35.345`. No additional backlog follow-up is needed because that item owns the full remaining publication step.

The existing **#restartintellijidea** review item now names plugin 0.2.375; restart IntelliJ to activate its Kotlin loader change. Plan and ownership notes: `src/agent-doc/tasks/agent-doc/plan-queue-response-evidence-and-lib-path.md`.
<!-- agent:boundary:d997bd6b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#restartintellijidea] [operator-verify] Restart IntelliJ IDEA to activate installed agent-doc JetBrains 0.2.375, then verify native library loading still works when a newer agent-doc release exists. The CLI/native library is installed as 0.35.345; plugin tests and signed build passed.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
