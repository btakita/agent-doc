---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a086bb-92a9-7152-bd30-9b04effbc938
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

0.35.350 installed, committed, pushed, and tagged. GH #49/#51 closed; the visible editor window has two agent panes. Full check and tmux suite passed. Release/CI workflows are running; PyPI publication remains subject to the existing storage quota blocker.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260909-043527.md`*

Compacted content:
- Archived 6 response topic(s): Response; #fixqueueitem; #fixhttpsgithub; 3 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): monsterrodholder.md stability; Response; #mrhsyncslowpath; 7 ...

### Re: Compact commits, editor routes, and controller timeout — gpt-6

> **Queue prompt:** Compact Exchange did not result in a commit in backend.md.

The shared controller fixes are in 0.35.349:

- Compact targets that are already visible in the editor keep the native-save effect active until the exact disk receipt settles the retained write. Settlement then runs the existing compact commit continuation. This also covers a target rebased over newer editor content.
- Restored pane layouts now activate their worker through an explicit readiness Source. Route awaits distinguish an unstarted effect, an in-flight effect, and an actual retry, and immediately recognize superseded generations.
- The five-minute error was Compact Exchange on `dev.md`. Controller-local continuation events now enter the live document graph through the same typed admission and deduplication boundary as external events. The compact handler no longer reloads the entire project ledger before replying.

The invariant is that retained work stays connected to its lifecycle graph until its exact receipt settles it. The document graph owns save/compact readiness; the process-scoped layout graph owns layout convergence. Sources for retained intent, delivery, worker readiness, and effect receipts feed the existing Computeds and Effects. The manual compact reload and identical-value startup replay are removed; await-result formatting remains a pure, one-shot transport adapter.

Regression coverage includes pending delivery → visible target → native save → settlement → compact continuation; rebased fixed points; restored layouts and fresh identical route generations; every await phase and supersession; and immediate, deduplicated controller-local event publication. The local-publication and layout-start regressions reproduced the missing edges before the fixes.

`make check` passed: 9,254 tests passed, 256 skipped, plus native editor peers, parity, simulator, and formal gates. The full 0.35.349 release install completed and the affected controller recycled. Live SDK and project-document layouts converged on attempt 1 with their correct panes (`%48` and `%42`). The retained `infra.md` compact automatically settled and committed as `b0fee52`.

`dev.md` still has a retained compact because its attached editor currently reports `editor_attached_model_missing`, including after the supported recovery request. Its target is preserved. Reopening that document or restarting the editor must reconnect the replica before its existing continuation can finish; do not repeat Compact Exchange. This remaining live check is recorded in review.

Source commit: `91d022aa8`; workspace plan/install mirrors: `a44e0cc89`. Release/tag publication follows this response closeout. The latest observed CI run was still in progress.

The original `backend.md` compact recovered independently in commit `fb5d356`, retaining its `20260909-043501` archive reference. Architecture and evidence: [plan-compact-save-layout-dispatch.md](plan-compact-save-layout-dispatch.md).

### Re: GH #49/#51 and the extra shell panes — gpt-6

> **Queue prompt:** Fix GH https://github.com/btakita/agent-doc/issues/49 + https://github.com/btakita/agent-doc/issues/51. release + publish

Implemented and installed **0.35.350**. Source commit `aaaa6b6cb` and tag `v0.35.350` are pushed; both GitHub issues are closed.

- **#51:** the server-identity observer now represents an absent server or empty bootstrap reply explicitly. It preserves the previous identity and registry until a valid observation can apply the existing initialize/keep/replace policy. Malformed nonempty identities, executable failures, and failed queries against a live server retain errors. The four reported start tests passed locally even before this change; the new direct-observer tests cover the missing absence boundary independently.
- **#49:** the native path stdout/stderr fix already shipped in 0.35.345 and remains covered by the test that seeds a newer release notice. This release fixes an additional UUID gap: claim persists its generated identity before occupied-pane and cross-root provisioning. Documentation now distinguishes the official compatibility builds from binaries built against a newer local glibc.
- **Extra panes:** my initial claim regression exposed a socket-isolation defect and created two idle shells on the live server. I verified their exact temporary-directory ownership and removed them. Socket selection now belongs to the shared tmux constructor, so claim, route, and startup stay on the selected server. The current editor window has exactly two agent panes (`%159` and `%54`).

The policy owners remain the registry observation adapter, pure identity transition policy, claim's authority-backed initialization, and shared tmux constructor. These are bounded startup/protocol operations; no new retry loop, cache, or disconnected reactive graph was added. Existing document authority and provisioning effects remain the lifecycle boundary. No changing derived state was extracted into a new Computed because these changes handle one-shot bootstrap observations before the managed session exists.

**Verification:** `make check` passed **9,259 tests**, with 256 skipped, including editor parity, native peers, simulator, and formal gates. `make tmux-ci` passed. New coverage checks absence without losing prior identity, later replacement cleanup, malformed replies, launch/query failures, and the claim UUID/provisioning boundary. The installed optimized CLI and native library match their release artifacts byte-for-byte; JetBrains 0.2.376 and VS Code 0.2.70 packages are installed, and the workspace controller recycled successfully. An installed-binary smoke created and reused a fresh isolated tmux server while leaving every default-server pane unchanged.

**Publication:** GitHub Release, PyPI, and CI workflows were in progress at the latest observation. The tag is published; release artifacts are not yet verified. PyPI's latest completed upload failed at the project storage limit, and no versions after 0.35.343 are currently present. The existing quota review item now tracks 0.35.350; no old artifacts were deleted.

Workspace plan and installed-skill mirrors: `dec03ee5e`. Architecture and scope: [plan-gh49-gh51-bootstrap.md](plan-gh49-gh51-bootstrap.md). The queue is currently stopped and empty; that operator state is preserved.
<!-- agent:boundary:d997bd6b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#compactdevreplica] [operator-verify] Reconnect the editor replica for src/haiven-dev/tasks/dev.md and verify its retained compact continuation commits. Installed 0.35.349 fixes local event ingress and native-save continuation; live read and supported recovery still report editor_attached_model_missing. Reopen the document or restart the editor, then observe the existing continuation; do not repeat Compact Exchange or force disk.
- [/] [#pypi345quota] [operator-verify] Publish agent-doc v0.35.345 to PyPI after quota space is available. The preceding upload failed with HTTP 400 because the project reached its 10 GB storage limit. Unblocks: obtain a quota increase or explicitly approve old-release cleanup, then run gh workflow run PyPI --ref v0.35.345 and verify uploaded artifacts.
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
