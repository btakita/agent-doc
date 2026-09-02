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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260902-040620.md`*

Compacted content:
- Archived 7 response topic(s): Automatic stale-supervisor recycle; Response; do [#fixclaimtmux]; 4 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): Issue #47 fixed; 0.35.303 publication blocked on PyPI quota; R...

### Re: Compact Exchange and session recovery — gpt-5.6-sol

> **Queue prompt:** I called Compact Exchange with backlog additions. The compacted backlog is uncommitted.

Verified and closed out. The compacted exchange and the two route-failure backlog items (`#jetbrainsrunroute-z09d` and `#jetbrainsrunroute`) were already present in `HEAD`; the live diff was the queue activation marker, so I did not create duplicate backlog entries. There are no additional actionable follow-up items from this queue prompt because those two existing backlog items fully capture the work.


I also applied the preflight-requested migration that moved the legacy gated `#restartintellijidea` item from backlog into Review.

The two additional document repairs are complete:

- `payments-ledger.md`: response replay duplication was self-healed through the retained capture, the stale idle route host was retired, and `agent-doc session-check` now reports `committed (commit_already_current)`. That document has no working-tree diff.
- `contracts.md`: the missing JetBrains replica was restored by reloading the live native library; `agent-doc session-check` reports `committed (commit_success)` and repaired its historical captured-snapshot drift. A newer queue prompt in that document remains intact and uncommitted for its own response cycle.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
- 🚧 I called Compact Exchange with backlog additions. The compacted backlog is uncommitted.
- do [#jetbrainsrunroute-z09d]
- do [#jetbrainsrunroute]
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#jetbrainsrunroute-z09d] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/payments-ledger.md stage=dispatch_only_dispatch_start_proof pane=%57 actor_generation=6 editor_attempt_id=unknown dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=01c84c57b94f408496d07ab9fc1b9d0c1fdb1cb7638200326fe45eaf1127be39 component=route/dispatch_only_dispatch_start_proof content_hash=sha256:ae01f8f47888cedcf3548e7f603f4fc7d156db5a3be864bd2ceaf5d3ca081cee]
- [ ] [#jetbrainsrunroute] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/payments-ledger.md stage=supervisor_dispatch_start_proof pane=%57 actor_generation=6 editor_attempt_id=unknown dispatch_proof_state=none elapsed_ms=10175 diagnostic_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/route-submit/1788302182345-supervisor_dispatch_start_unproven-codex-_57-46eeee34c802.txt ops_log_path=/home/brian/work/btakita/agent-loop/src/haiven-dev/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=supervisor_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=01c84c57b94f408496d07ab9fc1b9d0c1fdb1cb7638200326fe45eaf1127be39 component=route/supervisor_dispatch_start_proof content_hash=sha256:f0c87b98bd1da3bde1b6e67dc6185450e129fc83ae96e45d60acc0766d8aeb8d]
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#restartintellijidea] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.368 becomes live, then confirm native FFI loads through the resolved agent-doc executable and pane synchronization remains stable.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
