---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a04a81-a5e7-7793-8042-0520e71fd010
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260831-182736.md`*

Compacted content:
- Archived 16 response topic(s): contracts.md supervisor timeout fixed and released; Fix issue #46 and release; Response; 13 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): #coder7 live Coder validation; install recycles all open super...
<!-- agent:boundary:aeef9436:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- Fix issue #47 and release + publish.
<!-- /agent:queue -->

<!-- agent:backlog -->
- [/] [#restartintellijidea] [operator-verify] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.365 becomes live, then confirm a native patch ACK and stable monsterrodholders.md pane.
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
