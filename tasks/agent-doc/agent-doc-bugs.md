---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a065a2-d7bc-77b1-9c70-75eabcf6998a
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

Issue #47 fixed on main as 0.35.303; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260903-190913.md`*

Compacted content:
- Archived 8 response topic(s): Compact Exchange and session recovery; Response; do [#jetbrainsrunroute-z09d]; 5 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Automatic stale-supervisor recycle; Response; do [#fixclaimtmu...
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
Please fix agent-doc causes of turn closeout not completing in backend.md. I was typing in the queue while the turn was closing:
```
  Verified by reproducing in a clean worktree at 628bf4ac with the submodule at its own pin 2cd516e, then running the
  preflight workflow's steps in order at both heads — task tidy + git diff --exit-code clean, go test ./... 25 packages
  / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go
  build, all exit 0 — then confirmed on GitHub's runners. Pushed with --force-with-lease; temp worktree removed.

  Two things need you:

  1. The document closeout is deferred, not lost. agent-doc respond reports editor_projection_pending — the editor
     acknowledged the canonical content (df8a7991…) but hasn't projected its native save to disk. The retained intent
     1788462227612561795-1-df8a7991… commits itself once delivery converges; the binary explicitly forbids re-sending,
     forcing disk, or recycling, so I stopped after six session-check polls over ~2 minutes, all still INTERRUPTED. A
     save in the editor should converge it. Contributing factor worth knowing: the lint gate blocked the first two
     attempts on a pre-existing stray go attribute on <!-- agent:backlog priority go --> (queue-only, ignored, also
     flagged by preflight), and removing it required a disk edit under the live buffer — which is plausibly what the
     editor is now reconciling.
  2. #stalewtclean bit again. I couldn't rebase #413 to drop its now-redundant harness hunk because
     feat/studio-exposure-vocabulary is still checked out by ~/.claude/jobs/93c87c28/tmp/be-studio. Not urgent — git
     drops that hunk automatically (a trial rebase reported dropping … patch contents already upstream), and #413/#416
     stay green and MERGEABLE meanwhile.
```
<!-- /agent:queue -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#restartintellijidea] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.370 becomes live, then confirm native queue-prompt styling for Codex, Claude, and OpenCode, FFI loading through the resolved agent-doc executable, and pane synchronization remains stable.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
