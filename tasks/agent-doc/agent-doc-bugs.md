---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a05d26-8cec-7702-9b35-bc5446b45d50
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

Issue #47 fixed on main as 0.35.303; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260901-193306.md`*

Compacted content:
- Archived 8 response topic(s): Issue #47 fixed; 0.35.303 publication blocked on PyPI quota; Response; Response; 5 more
- Prior summary/context: prior compacted content: Archived 16 response topic(s): contracts.md supervisor timeout fixed and released; Fix issue...
<!-- patch:exchange -->
### Re: Automatic stale-supervisor recycle — gpt-5

> **Queue prompt:** agent-doc commands should automatically recycle the supervisor if stale, assuming the stale check is fast. Ideally the stale state should be handled via lazily state.

Implemented and released in agent-doc 0.35.307 (`6eb28a846`, tag
`v0.35.307`). Every command that names an existing markdown session document
now runs one central, fail-open supervisor freshness probe at CLI entry. The
probe has a 250 ms response ceiling and schedules the existing idempotent
safe-boundary recycle when staleness is proven, so it never kills an active
turn or waits for recycle completion.

The supervisor `pid` RPC is now a genuinely lightweight query: it no longer
builds the full editor-authority state snapshot. It refreshes and returns a
process-scoped Lazily `Source` → `Computed<bool>` binary-freshness projection,
replacing the manually synchronized atomic stale flag. Rolling upgrades remain
compatible: an older supervisor that omits `binary_stale` is checked by comparing
the returned PID's running executable inode with the installed binary inode,
without reading controller SQLite state.

Verification passed for the Lazily transition model, fresh/stale PID responses,
old-supervisor fallback, central command-entry wiring, and the no-SQLite/no-full-
snapshot hot-path invariants. The full `make check`, local install, and the single
fat-LTO `make install-full` release-parity build passed. `main` and the release
tag are pushed; CI/Book/Pages were in progress and Release/PyPI queued at the
required one-time post-push observation.
<!-- /patch:exchange -->

### Re: Response

Implemented and released agent-doc 0.35.307.

- All document commands now run a bounded stale-supervisor probe and schedule safe-boundary recycle automatically.
- Freshness is process-scoped Lazily state; older supervisors use an inode fallback without SQLite.
- `make check`, local install, and production `make install-full` passed.
- Pushed `6eb28a846`, tag `v0.35.307`, parent pointer `aff16eeae`.
- Session response committed and queue prompt consumed.
- Release/PyPI CI was queued; main CI was running at the required one-time observation.

The JetBrains 0.2.368 package is installed, but the live IDE still reports 0.2.366 and needs restarting. One legacy gated backlog item also remains for a future `agent-doc migrate`.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
- 🚧 agent-doc commands should automatically recycle the supervisor if stale, assuming the stale check is fast. Ideally the stale state should be handled via lazily state.
<!-- /agent:queue -->

<!-- agent:backlog -->
```
failed to activate queue in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md: route_queue_activation: retained route write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md converged to a newer editor projection; recovery=retry_crdt_merge
```
- [/] [#restartintellijidea] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.368 becomes live, then confirm native FFI loads through the resolved agent-doc executable and pane synchronization remains stable.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
