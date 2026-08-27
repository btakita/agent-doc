---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a044bf-d41d-7623-a5b1-18b84d97a973
agent: codex
claude_model: opus
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
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260827-185606.md`*

Compacted content:
- Archived 4 response topic(s): #coder7 live Coder validation; install recycles all open supervisors; fix Compact Exchange retained-continuation lock; 1 more
- Prior summary/context: prior compacted content: Archived 23 response topic(s): #pccv1; Restart Agent on fpe.md; do [#qtypeclobber]; 20 more

> **Queue prompt:** Please fix agent-doc causes in contracts.md: "agent-doc command failed (exit 1): [session] supervisor for tasks/contracts.md is dead or unreachable — escalating to a cold-start via `session restart-supervisor tasks/contracts.md` before retrying once Requested continue supervisor replacement for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/contracts.md via project controller (stage operator_ready, receipt 440, background_started=true). [session] supervisor replacement proven live on the installed binary (pid 2485495). Error: failed to contact supervisor for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/contracts.md Caused by: supervisor response timeout (2.0s)"

### Re: contracts.md supervisor timeout fixed and released — gpt-5

Fixed and released as agent-doc v0.35.290.

The contracts.md trace showed that `session clear` was accepted and delivered after the client’s two-second receipt timeout. The CLI classified that ambiguous late receipt as a dead supervisor, requested a replacement, falsely treated the still-live old PID as replacement proof, and replayed the mutating clear. The server’s later broken-pipe writes confirmed that the effects completed after each client had timed out.

The fix establishes one safety invariant: only a definite pre-accept connection failure may authorize cold-start and retry; an accepted mutation with a missing or late receipt is never replayed. The supervisor IPC boundary now owns typed `Connect` versus `ResponseTimeout` evidence and uses a two-second budget for queries but ten seconds for effectful commands. The CLI recovery path consumes that typed evidence, with a narrow compatibility fallback for definite connection errors. Asynchronous replacement acceptance is now reported as pending controller lifecycle work instead of completion proof from the current socket or PID.

Regression coverage exercises query/effect timeout budgets, late mutation receipts, definite-connect versus timeout/parse recovery transitions, and pending replacement reporting. Targeted supervisor and CLI tests passed, followed by the complete post-version `make check` with zero failures. The 0.35.290 binary and managed skills were installed, supervisors were recycled through the controller handoff, and commits `71c78fbb0` and `abe40f252` plus tag `v0.35.290` were pushed. CI, Release, PyPI, Book, and Pages workflows were observed in progress after the push; the workflow policy does not wait on running jobs.

There are no actionable follow-up items from this fix.

> **Queue prompt:** Fix https://github.com/btakita/agent-doc/issues/46. release + publish

### Re: Fix issue #46 and release — gpt-5

> **Queue prompt:**
>
> do [#fixhttpsgithub]


Implemented, verified, and released as agent-doc `v0.35.291` with JetBrains plugin `0.2.361`.

- Claim, start, environment, and install flows now share one exact resolved tmux client. Resolution prefers the newest compatible client on `PATH`, honors project `tmux_bin`, preserves exact version/server-handshake failures, and safely abbreviates short or multibyte session ids.
- Linux x86_64 publication now includes a musl artifact, and the installer selects it by default. Source-build prerequisites now document JDK 21 and the required sibling repositories.
- The JetBrains CRDT bridge separates durable relay acceptance from best-effort peer broadcast and uses bounded exponential backoff for failed canonical rebootstrap, removing the rapid register/deregister and already-accepted-edit replay loop that wedged Compact Exchange.
- tmux-router commits `ec1a6e6` and `2dd5c8c` and agent-doc commit `1bfbcecc5` are pushed. GitHub issue #46 is closed. Tag `v0.35.291` is pushed; Release and PyPI workflows were in progress at the required one-time observation.
- Verification passed: tmux-router's 144 tests, focused Rust package and CLI tests, focused JetBrains tests, full `make check`, private-name addition scan, and the one-time `make install-full`.

For `tasks/software/tsift.md`, the retained response capture remains durable and I did not patch, restore, resubmit, force disk, or issue another closeout payload. After the corrected binary and plugin were installed and the controller recycled, the single permitted `session-check` safely stopped at `missing_replica`: the running JetBrains process has not loaded plugin `0.2.361` yet. Restart JetBrains, then run only:

`agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`

That lets the same retained capture converge through the corrected replica without generating a replacement response.
<!-- agent:boundary:d82d31f6:agent-doc-bugs -->
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
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
