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
queue: stop
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

### Re: issue #48 and v0.35.304 — gpt-5

> **Queue prompt:**
>
> do [#fixissuerelease]


Issue #48 is fixed and closed. The follow-up shipped in commit `d92a13511`:

- JetBrains 0.2.368 now resolves `agent-doc` through the shared project/home/local/Cargo/`PATH` policy before invoking `lib-path`. The native loader logs the selected executable and every attempted location.
- Transient executable or library startup failures now enter a bounded retry state. Proven unsafe native-generation transitions still retain the restart-required fence.
- Claim admission now accepts only explicit `--pane`, `--position`, or process-owned `TMUX_PANE` evidence. With none of those, it provisions one fresh pane in the authoritative project session instead of binding tmux's ambient last-active pane.

v0.35.304 is publicly published:

- GitHub release: https://github.com/btakita/agent-doc/releases/tag/v0.35.304 — six platform assets.
- PyPI: https://pypi.org/project/agent-doc/0.35.304/ — four wheels.
- Release workflow `33454191778` and PyPI workflow `33454191819` both completed successfully.
- The installed local CLI reports `agent-doc 0.35.304`; JetBrains 0.2.368 is installed for IntelliJ IDEA 2026.1. Restart IDEA once more to activate this newly installed plugin generation.

Verification passed: focused claim/tmux tests, the JetBrains native-loader and resolver tests, the source ownership regression, full `make check`, packaged `buildPlugin`, and local `make install`. The issue report's minor concerns were audited as already satisfied by v0.35.303: local plugin installation rejects stale-version zips, and the published GNU/Linux artifact requires no newer than GLIBC 2.18.

The authorized PyPI cleanup remains intact after publishing: PyPI reports v0.35.304 as latest, 141 retained releases, and zero releases older than 0.35.0.

Architecture closeout:

- Invariant preserved: durable editor/pane ownership is derived only from explicit, auditable evidence.
- Policy owners: `TerminalUtil` owns JetBrains executable resolution; claim admission owns pane provenance; install targets own package-version integrity.
- Transition table covered: resolved versus exhausted executable search; retryable failure versus restart-required native state; explicit pane, positioned pane, process pane, or provision-new claim.
- Reactive topology: immutable inputs enter pure decision functions; existing one-shot native-load and claim boundaries perform effects. No polling island, ambient tmux authority, or new long-lived reactive node was added.
- Edit surfaces and tests stayed within the JetBrains resolver/native loader, claim/tmux boundaries, release metadata, and their focused regressions.
❯ Fix Run Agent Doc on backend.md:
```
failed to activate queue in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md: route_queue_activation: retained route write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/backend.md converged to a newer editor projection; recovery=retry_crdt_merge
```

### Re: Fix Run Agent Doc on backend.md — gpt-5

> **Queue prompt:**
>
> do [#fixrunbackend]


Fixed the live runtime and the agent-doc cause.

The backend document is no longer blocked by a retained write. The recovery check proved the prior response is committed and no write remains retained; the remaining editor/disk divergence is a fresh unanswered live-buffer edit, so the next backend agent-doc cycle can admit and answer it without force-disk recovery. The corrected local runtime reports `agent-doc 0.35.305`, and its supervisors/controllers have been marked for idle-boundary recycle.

The incident logs exposed the exact bug: all three queue-activation attempts had the same `expected_hash` and the controller's current hash equaled that expected baseline, but the route waiter classified any converged text unequal to its target as a “newer editor projection.” It therefore mistook the unchanged old baseline for concurrent drift and repeated the same doomed merge.

The route projection now models the three real states explicitly:

- exact retained target: continue dispatch;
- unchanged expected-current baseline: remain pending until the target becomes visible;
- genuinely third editor projection: re-read and CRDT re-merge.

The realtime workflow spec and a focused regression test cover that transition table. The change stays inside the existing route-owned convergence effect: no new polling node, controller, or ambient authority was introduced.

Verification passed: the focused route regression, full `make check`, `git diff --check`, local `make install`, and the single release-parity `make install-full`. Commit `3d7124c7a` is pushed to `main`; tag `v0.35.305` is pushed. GitHub CI is in progress, while the Release and PyPI workflows are queued, so publication will complete through the normal tag-driven path.

### Re: Response

> **Queue prompt:**
>
> do [#processesawaycomputer]


Fixed the overnight idle CPU problem in agent-doc 0.35.306.

The cause was a stale supervisor waiting for a safe turn boundary. That unchanged deferral was treated as urgent on every 500 ms poll, so the idle-watch thread repeatedly ran the expensive reconciliation path and appended the same `supervisor_binary_stale_recycle_deferred` event. One affected session accumulated 193,824 copies of that event.

The supervisor now owns process-scoped deferral transition state. It still checks the installed binary identity every 500 ms, but while the same safe-boundary deferral remains active it performs full reconciliation only on the existing five-second quiescent cadence. The diagnostic is emitted once when the deferral begins, clears with the state, and rearms for a later episode. Turn-boundary safety is unchanged.

Verification completed:

- Focused transition and cadence tests passed.
- The full `make check` suite passed.
- `make install` completed and installed agent-doc 0.35.306 plus the existing editor packages.
- A live ready/busy stale supervisor averaged 0.67% CPU in `idle-queue-watch` over 15 seconds, down from 6.3% before (about an 89% reduction).
- Across that live measurement, the deferred-event count remained exactly 193,824: zero repeated log entries after the transition receipt.
- The latest completed `main` CI run is green.

The release is committed locally as `12a8a0d09` (`fix: quiesce stale supervisor deferrals`). The required post-closeout push, tag, and publication follow this document commit.

The earlier “Run Agent Doc on backend.md” item remains fixed by 0.35.305; removing its in-progress marker required no additional code change.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
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
