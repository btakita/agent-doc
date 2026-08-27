---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a041a6-c307-70a0-8897-a4264df34934
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260826-182906.md`*

Compacted content:
- Archived 23 response topic(s): #pccv1; Restart Agent on fpe.md; do [#qtypeclobber]; 20 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Removing AGENTS.md instructions; Extract AGENTS.md into skills...

### Re: #coder7 live Coder validation — gpt-5

`#coder7` cannot be truthfully completed on this host, so I moved it to review with the exact external prerequisites instead of recording invented passes.

The live matrix was not run: `agent-doc env --json` classifies this machine as a local display-backed `external` terminal host; the `coder` CLI is absent; no `CODER_*` workspace identity, Coder SSH target, or Coder client configuration is present; and VS Code is unavailable. JetBrains is installed locally, but there is no reachable Gateway-backed Coder workspace to exercise. Those conditions block all five required cases: JetBrains cold start, VS Code cold start, simultaneous dual-IDE convergence, workspace stop/start recovery, and the image-without-tmux failure path.

The unblock is concrete: provide a reachable Coder workspace URL or SSH target with authentication, enable the current agent-doc plugin in the JetBrains remote backend and extension in the VS Code remote host, and provide a workspace/image variant without tmux. Then run the diagnostics from `runbooks/coder-workspace.md` and record each matrix result in `tasks/agent-doc/plan-coder-ide-hosted-tmux.md` before resolving `#coder7`.

No additional backlog follow-up is needed because `#coder7` itself now retains the complete matrix and the exact external unblock conditions in review.

Local repository verification passed: `make check` exited 0. The latest GitHub Actions `Agent Doc` run failed before either job acquired a runner (both jobs had zero steps), so it is an external CI-start failure rather than a code or tmux-test regression. The required `tsift init` refresh also updated the root navigation instruction block from v0.1.87 to v0.1.88.

### Re: install recycles all open supervisors — gpt-5

Fixed and released as `v0.35.288`.

The root cause was process-namespace drift in install-time discovery. Most live supervisors retain relative document arguments such as `tasks/agent-doc/agent-doc-bugs.md`, but `lib-install` canonicalized those paths against the agent-doc source submodule's CWD. That silently skipped supervisors opened from the parent project. Discovery also considered only route-owned starts rather than every live `agent-doc start` supervisor.

The install boundary now enumerates all open supervisors, parses both route-owned and operator-started `agent-doc start` commands, and resolves each relative document argument against that process's `/proc/<pid>/cwd`. It writes the existing durable, idempotent recycle request for every discovered document before controller recycling; each supervisor consumes it at its next safe idle boundary and preserves the live child PTY across `execve`.

Regression coverage exercises per-supervisor relative CWD resolution, absolute/fallback paths, and discovery of both route-owned and operator-started supervisors. The controller/parser package suites passed, and the full versioned `make check` suite passed twice.

The live clean-worktree release install proved the original fleet case:

- 13 live supervisors existed before install; 11 used relative document arguments.
- `make install-full` installed `agent-doc 0.35.288` and reported `13 open supervisor(s) marked`, `0 skipped`.
- The independent `agent-doc admin recycle --all-projects --json` check reported `supervisors_marked: 13`, `supervisors_skipped: 0`.
- Ten idle supervisors had already re-execed onto the new binary inode at inspection time. Three busy supervisors, including this active turn, remain safely marked and will re-exec at their next idle boundary instead of being interrupted.

Source commit `42a4ddd12` is pushed to `main`; tag `v0.35.288` is pushed. The Book workflow is green. CI, Release, and PyPI workflows were queued/in progress at closeout and were inspected without waiting, per the repository's external-CI policy. The private superproject projection is committed separately as `d74ee5e06`.

### Re: fix Compact Exchange retained-continuation lock — gpt-5

Implemented, installed, and live-verified the fix for `#fixcausescompact`.

The root cause was a lifecycle type error: after editor authority and disk had converged, a same-Exchange conflict was still returned as an ordinary error. The controller therefore retained the compact continuation, retried it on every relevant delivery edge, and rejected every later Compact Exchange as already pending.

The corrected invariant is: unsettled delivery may remain retryable, but settled authority must end a retained compact exactly once—either `Settled`, or `Superseded` when the compact-owned component advanced. Supersession never rewrites the document, checkpoints a snapshot, or commits the stale target.

Ownership and reactive topology are now explicit:

- `agent-doc-compact-io` owns the typed rebase/completion decision from retained targets plus converged authority.
- `agent-doc-state-backbone` owns the identity-matched durable `DocumentCompactProjectionSuperseded` transition.
- The existing projection Source → compact-resume Computed → completion Effect → durable receipt Source remains the only execution path. No polling, retry loop, or error-string classifier was added.

The transition table is covered for retryable authority/native-save readiness, exact settlement, sibling/tail rebase, settled same-component supersession, and delayed identity-mismatched receipts. Focused state/compact/controller regressions and the full local `make check` pass; GitHub CI run `33021813476` also passed `make check` and `make tmux-ci`. The updated binary was installed and its automatic supervisor recycle replayed the live ledger: continuation `compact-projection-c91a04ba7db247c880ab58ff482850cb276e55550c0b743e0672f6d092857652-94c52d8e-d440-49e8-941c-6f26a20eeabd` now has a `document_compact_projection_superseded` terminal fact. `tasks/software/tsift.md` was not rewritten and Compact Exchange was not retried.

Code commit: `5316b1818` (`fix: retire superseded compact continuations`). Parent submodule-pointer commit: `34ae399fa`.

> **Completed queue prompt:** Fix agent-doc causes of Compact Exchange on tsift.md failed: [compact] pending: the compacted target is retained for editor delivery and is not yet committed. Do not retry Compact Exchange while this target is pending; if delivery remains pending, restart the editor so its agent-doc plugin reconnects
>
> [compact] pending: existing continuation compact-projection-c91a04ba7db247c880ab58ff482850cb276e55550c0b743e0672f6d092857652-94c52d8e-d440-49e8-941c-6f26a20eeabd already owns this document (commit_requested=true)

❯ `agent-doc-compact-io` owns the typed rebase/completion decision from retained targets plus converged authority.

> **Queue prompt:** On tsift.md, I added the same queue item and the same backlog id was created `- do [#fixopenissues]`. Then agent-doc auto-struck the queue item. Please fix the act of adding a queue item with the same prompt that was previously run...since I want to repeat an action.

### Re: repeatable queue work incarnations — gpt-5

Fixed and released as agent-doc `0.35.289`.

The failure was an incarnation bug: preflight treated every id in historical `agent:done` as permanently complete. When the same free-text action generated the stable id `#fixopenissues` again, that old completion record caused the new queue head to be struck immediately.

Preflight now treats a newly admitted id, or an id backed by a currently open backlog item, as a fresh work incarnation. Those ids are excluded from the done-strike set until normal closeout reaps the live backlog item. Historical completion remains archived, while repeated maintenance passes preserve the new unchecked `- do [#fixopenissues]` head.

I added the lifecycle contract to the document-format and orchestration specs and added an exact regression that starts with historical `#fixopenissues` completion, admits the same prompt/id again, runs maintenance repeatedly, and verifies the replacement head stays live. The 61 queue-maintenance tests, the full `agent-doc-preflight-io` package, and the clean versioned-tree `make check` all pass. The first clean-tree gate encountered four transient Lazily controller connection resets; every affected test passed in isolation and the standard full rerun passed.

Source fix: `9047d6422` (`fix: preserve repeated queue work incarnations`). Release: `dc2ec7cef`, tag `v0.35.289`. Parent projection: `a694464a79`. `make install-full` installed `agent-doc 0.35.289`, its shared library, and the matching JetBrains package; the source and parent branches and release tag are pushed. The one-shot post-push checks found CI, Release, and PyPI workflows in progress.

Operationally, I also stopped an accidental stale orchestration dispatch before it made any code changes; it did not influence this implementation or release.
<!-- agent:boundary:fac83440:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~On tsift.md, I added the same queue item and the same backlog id was created `- do [#fixopenissues]`. Then agent-doc auto-struck the queue item. Please fix the act of adding a queue item with the same prompt that was previously run...since I want to repeat an action.~~ — auto-struck: answered this cycle (#ftstrike)
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
