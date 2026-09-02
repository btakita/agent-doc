---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a05ebd-87c0-7063-94e3-0d53d7519cf0
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

> **Queue prompt:**
>
> did you already do this task? Is this a leftover? If yes, please fix agent-doc not cleaning this up:
> ```
> failed to activate queue in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md: route_queue_activation: retained route write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md converged to a newer editor projection; recovery=retry_crdt_merge
> ```


> **Queue prompt:**
>
> do [#fixclaimtmux]

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

### Re: do [#fixclaimtmux] — gpt-5

stream: false
origin: skill

Fixed the shared causes and released agent-doc v0.35.311 (commit `5bc9643fa`).

- `payments-ledger.md`: restored a visible Codex pane at `%64` in window `@3`. The actor and supervisor are fresh v0.35.311, projection lag is false, and its queue remains paused behind the File Cache Conflict quarantine. Explicit harness authority now survives cleanup/restart, so `agent: codex` no longer falls through to Claude.
- `monsterrodholders.md`: pruned the dead projection, created replacement pane `%65` in `@3`, attached generation 1714, and started the requested Codex harness. The controller/supervisor are fresh, the pane is live, and the destructive Cloudflare-token queue head remains paused.
- `backend.md`: added a Lazily retained-frontier proof from relay registration through the controller and JetBrains client. A retained editor buffer is accepted only when canonical state causally covers its retained frontier; false or unknown proof stays on hold, preserving the earlier no-clobber safety rule. Pane `%25` remained attached, and its supervisor was safely re-execed onto v0.35.311.
- Focus now promotes a stashed registered pane before selecting it. Supervisor replacement resolves the installed binary instead of re-executing an obsolete process image. `agent-doc start --harness codex|claude|opencode` provides explicit recovery authority, while status precedence is document declaration, durable actor, then ambient harness.

Installed locally:

- agent-doc v0.35.311
- JetBrains plugin v0.2.370
- VS Code extension v0.2.69

Verification passed: relay/controller Rust tests, JetBrains tests, full local `make check`, CI, six-platform Release, and PyPI publication.

IntelliJ is still running plugin v0.2.368. Restart IntelliJ before editing or restoring `payments-ledger.md` or reopening `backend.md`; that is required to load v0.2.370's causal recovery/File Cache Conflict guard. I intentionally did not rewrite the quarantined payments document while the old plugin remained attached, because it could delete the restored components again.

### Re: Response

Fixed and released agent-doc v0.35.311 (`5bc9643fa`).

- `payments-ledger.md`: Codex pane `%64` is visible in `@3`, fresh and quarantined.
- `monsterrodholders.md`: replacement Codex pane `%65` is live; destructive queue work remains paused.
- `backend.md`: pane `%25` was preserved and its supervisor safely upgraded.
- Harness selection, hidden-pane focus, stale-binary restart, and Lazily retained-frontier recovery were fixed.
- Local tests, `make check`, CI, Release, and PyPI all passed.
- Agent-doc session check passed and the response was committed.

Restart IntelliJ now. Plugin v0.2.370 is installed, but the running IDE still has v0.2.368 loaded. I intentionally left `payments-ledger.md` content quarantined until restart so the old plugin cannot delete its components again.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
- ~~do [#fixclaimtmux]~~
~~~done
did you already do this task? Is this a leftover? If yes, please fix agent-doc not cleaning this up:
```
failed to activate queue in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md: route_queue_activation: retained route write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md converged to a newer editor projection; recovery=retry_crdt_merge
```
~~~
- When I call JB `Recycle Supervisor` while a session is running, the session immediately is interrupted and restarts but does not continue once restarted. Please make sure the session continues if already running.
<!-- /agent:queue -->

<!-- agent:backlog -->
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
