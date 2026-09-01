---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a05913-bfc1-7ce0-969a-df6350db8622
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Issue #47 fixed on main as 0.35.303; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260831-182736.md`*

Compacted content:
- Archived 16 response topic(s): contracts.md supervisor timeout fixed and released; Fix issue #46 and release; Response; 13 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): #coder7 live Coder validation; install recycles all open super...


### Re: Issue #47 fixed; 0.35.303 publication blocked on PyPI quota — gpt-5

Issue #47 is fixed and closed by `75354d041` on `main`. All tmux construction now uses one project → global → `PATH` resolver, `agent-doc env` reports the selected source, editor-package installation fails closed and rejects mismatched JetBrains zips, the Linux glibc release uses the compatibility cross image, and the source-build documentation covers the sibling repositories and JDK 21.

Verification passed: focused resolver/diagnostic/packaging tests, two complete `make check` runs (including the post-install clean rebuild), `make install-full`, `git diff --check`, the private-name additions scan, and a live installed `agent-doc 0.35.303` environment probe. GitHub CI run `33429961606` is currently in `make check`; the tmux job has not started yet.

I pushed the release-ready commit but did not create `v0.35.303`. The previous PyPI run failed because the project has reached its 10 GiB storage quota. Live PyPI metadata reports 9.9984 GiB used, only 0.0016 GiB free, while a recent agent-doc release needs about 70 MB; PyPI publication has been stalled since `0.35.239`.

One operator decision is required before I can tag and publish: authorize deletion of old PyPI release files (and specify the retention cutoff), or obtain a PyPI project-size quota increase and tell me when it is approved. I recommend the quota increase if preserving historical installability matters; otherwise, authorize a concrete old-version deletion policy and I will resume the release.

### Re: Response

Issue #47 is fixed and closed by [commit `75354d041`](https://github.com/btakita/agent-doc/commit/75354d041). Version `0.35.303` passed `make check` and `make install-full` and is pushed to `main`.

I have not created the release tag because PyPI is at 9.9984 GiB of its 10 GiB quota; publication has stalled since `0.35.239`. [PyPI metadata](https://pypi.org/pypi/agent-doc/json) [quota guidance](https://docs.pypi.org/project-management/storage-limits/#requesting-a-project-size-limit-increase)

Please choose:

1. Request a quota increase (recommended), then tell me when approved.
2. Authorize deletion of old PyPI releases and specify the retention cutoff.

The agent-doc response was committed, with the release queue item left active.
### Re: Response

Reply with an explicit scope, for example:

> I authorize deleting all `agent-doc` PyPI releases older than `0.35.0`. Preserve `0.35.0` and newer. Then resume tagging and publishing `v0.35.303`.

That would free about 1.68 GB. Deletion is permanent and breaks installs pinned to deleted versions; yanking does not free storage. PyPI deletion also requires an authenticated owner/maintainer session through the [release-management page](https://pypi.org/manage/project/agent-doc/releases/). [PyPI deletion guidance](https://docs.pypi.org/project-management/storage-limits/#freeing-up-storage-on-an-existing-project)
❯ A Brave tab is already open at the PyPI login page. No PyPI plugin connection is available, so please:

1. Log in there, including 2FA.
2. Reply `logged in`.

❯ Do not send credentials or delete anything manually. I’ve resolved the authorized scope to 155 releases/363 files, totaling about 1.68 GB, while preserving every `0.35.x` release. Once authenticated, I’ll perform and verify the bounded deletion, then publish `v0.35.303`.

### Re: Response

The PyPI login page is open and active in Brave. Please complete login and 2FA, then reply “logged in.”

### Re: Response

> **Queue prompt:** Fix issue #47 and release + publish.

Published agent-doc v0.35.303 successfully.

- Fixed and closed issue #47.
- Fixed first-delete resurrection in JetBrains editor projections.
- Preserved deferred mutation plans so captured --backlog-add intent survives same-capture recovery.
- Allowed attached tmux policy resolution without unavailable editor-replica reads; auto-sync is working after the IDEA restart.
- Verified docs.md Run Agent Doc did submit text plus Enter to Claude Code; delivery took about 4.4 seconds during hot reload, and the pane actively processed it, so no lost-submit code change was warranted.
- Deleted every PyPI release older than 0.35.0: 155 releases / 363 files removed; 0.35.0 and newer preserved.
- GitHub Release and PyPI workflows passed. GitHub has six native assets; PyPI has four wheels and zero releases below 0.35.0.
- Installed and verified agent-doc 0.35.303 locally, including JetBrains 0.2.367 and VS Code 0.2.69.
- Commits: 6d94b65a1 and 10de5f73c; tag v0.35.303 points to 10de5f73c.

The newly typed queue item “Fix issue #48 and release.” was preserved for the next cycle.
<!-- agent:boundary:aeef9436:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#fixissuerelease]
- ⏭️ Fix issue #47 and release + publish.
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] 🚧 [#fixissuerelease] Fix issue #48 and release.
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
