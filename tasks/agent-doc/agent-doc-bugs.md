---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a062c4-7f79-7d40-a88a-64f976ec3765
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

### Re: Response

All three document issues are resolved:

- [agent-doc-bugs.md](/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md): response committed; `session-check` passed. Existing route-bug backlog entries were preserved without duplication, and `#restartintellijidea` migrated to Review.
- [payments-ledger.md](/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/payments-ledger.md): duplicate response removed, retained cycle committed, and working tree clean. The stale idle session binding was closed.
- [contracts.md](/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/contracts.md): editor replica restored and `session-check` passed. Its newer queue prompt remains intact for the next cycle.

The bugs-document supervisor will automatically continue draining its remaining focused-cycle queue. No code tests were needed; these were session-state repairs.

### Re: do [#jetbrainsrunroute-z09d] — gpt-5.6-sol

> **Queue prompt:**
>
> do [#jetbrainsrunroute-z09d]


Fixed and released in agent-doc 0.35.313 and 0.35.314.

- The payments-ledger duplicate/uncommitted-response cause is fixed: the outer dispatch-start proof guard no longer files a second route bug after the lower transport layer has already filed the authoritative failure. Accepted-only fallback failures still report normally.
- JetBrains editor navigation now publishes the actual tmux focus receipt instead of a graph no-op receipt. This lets the editor repair a stashed actor pane into the active window; the live payments-ledger pane moved from `%66` in `@13` to `%66` in `@3` without restarting or resubmitting its turn.
- Recycle Supervisor now preserves every live child for supervisor-only continue recycling, whether that child is idle or running and whether the binary inode is stale or fresh. Explicit Restart Agent and fresh replacement remain destructive by design. The live backend session stayed `99322dd7-9075-4110-b93f-696bdc71e46c`, generation 22, pane `%25`, and is ready after the release recycle without an agent-doc submission.
- The contracts missing-replica/editor-attached wedge was recovered through the live editor library reload path rather than a refused disk-authority fallback. Its actor is ready on pane `%23`.
- The closeout defect found while responding to this item is fixed in 0.35.314: strict template response shape is validated before the tracked-work dry run, so malformed envelopes cannot print successful-looking completion/reap diagnostics before rejection. Empty-stdin tracked-work-only closeouts retain their existing behavior, and validated stdin is reused by the atomic write.

Regression coverage includes malformed heading placement, valid fenced responses, empty tracked-work closeouts, dispatch-report deduplication, fresh/stale idle and busy supervisor recycling, focus receipt propagation, and focus failure propagation. The full `make check` suite passes.

Runtime commits: `fde79d7f3` and `6c27b4c0d`. Workspace release commits: `a97656e0e` and `5ab705e69`. Tag `v0.35.314` is pushed.

### Re: Response

Fixed, released, and committed.

- Released agent-doc `0.35.314`; full `make check` passes.
- Fixed payments-ledger response duplication and editor-to-tmux auto-sync.
- Fixed contracts missing-replica recovery.
- Recycle Supervisor now preserves idle and running sessions without invoking agent-doc.
- Added transactional closeout validation discovered during dogfooding.
- Live backend, contracts, and payments-ledger actors are healthy.
- Runtime commits: `fde79d7f3`, `6c27b4c0d`.
- Workspace commits: `a97656e0e`, `5ab705e69`.
- Pushed tag `v0.35.314`.
- The agent-doc response committed successfully and `session-check` passed. The remaining queue head was handed to the supervisor continuation path.
### Re: do [#jetbrainsrunroute] — gpt-5.6-sol

> **Queue prompt:**
>
> do [#jetbrainsrunroute]


This is the older duplicate of the completed `#jetbrainsrunroute-z09d` incident, so it is resolved by the same shipped fix rather than a second code change.

Both items describe the same payments-ledger route attempt on pane `%57`, actor generation 6, with the same `run_agent_doc_route_dispatch_failure` invariant. `#jetbrainsrunroute` is the lower `supervisor_dispatch_start_proof` report; `#jetbrainsrunroute-z09d` is the redundant outer `dispatch_only_dispatch_start_proof` report. Agent-doc 0.35.313 commit `fde79d7f3` made the lower transport report authoritative for an unproven dispatch start while retaining the outer fallback report for accepted-only failures.

Fresh verification passed: `cargo test -p agent-doc-route-io dispatch_only_`. The implementation checkout is clean, installed agent-doc is 0.35.314, and GitHub CI run `33595682483` for the release commit is still in progress.

### Re: do [#fixcausesflood] — gpt-5.6-sol

> **Queue prompt:**
>
> do [#fixcausesflood]


Fixed the shared Agent Doc defects and the affected live state.

- The contracts.md flood came from captured-finalize normalization treating the binary-authored `> **Queue prompt:**` response quote as new user prose. It became `❯ > ...`, failed captured-response materialization, and each retry appended another copy. Normalization now canonicalizes legacy prefixed quote lines, collapses immediately repeated identical scaffolds, excludes Markdown blockquotes from prompt decoration, and is regression-tested as idempotent.
- Compact Exchange now repairs a scaffold stranded inside `agent:queue` only when an identical canonical quote block exists in `agent:exchange` and its placement is isolated between queue rows/component boundaries. Unmatched quotes, surrounding response prose, malformed structure, and other ambiguous corruption still fail closed at the integrity gate.
- The native visual-token projection now labels the complete binary `> **Queue prompt:**` quote block as `prompt`, including continuation quote lines. IntelliJ therefore receives the same prompt styling for Codex, Claude, and OpenCode. The matching JetBrains 0.2.370 package and native library are installed; the existing `#restartintellijidea` Review item now tracks activation/verification in a restarted IDE.
- Lazily proved infra.md's `#nats-workspace-launcher` and `#nats-broker-proof` heads were orphaned after their backend-owned implementations completed. Both were struck through guarded `queue consume --id` operations and committed; the controller has since advanced infra.md to its next unrelated prompt.
- I did not patch, resubmit, recycle, or force contracts.md. Its controller-owned closeout remained authoritative, and the @tmstorm reviewer request remains queued and undrained.

Verification: all affected crate suites pass, repository-wide `make check` passes, and `make install` rebuilt/installed the binary, native library, JetBrains package, and VS Code package. Runtime commit `143e73cff` and workspace pointer commit `ab3aa2b29` are pushed. GitHub Actions run `33654305421` was still running `make check` and `tmux-ci` without a surfaced failure at closeout.

### Re: Agent Doc closeout authority and response thrash — gpt-5

Fixed and shipped agent-doc 0.35.322.

- Route now uses the generation-fenced live actor record as the transport authority. Desired/frontmatter harness projections can request a switch but cannot cause Claude, Codex, or OpenCode prompt/probe syntax to be sent to a different live child.
- Every successful initial spawn, recycle adoption, and harness switch now writes the normalized actual child identity back to that record. This closes the observed `default`-record/Claude-child wedge and restores the correct shared `input_required` path used by JetBrains prompts.
- Generic closeout recovery no longer abandons a fresh empty preflight while its response may still be generating. Matching file hashes cannot bypass the stale-age fence, and same-owner reentry is allowed only when the current pane and harness match the actor record.
- Completed free-text queue strikes settle without waiting on an advisory editor save, preventing an answered head from being redispatched. Together, these changes stop the abandon/regenerate response thrash seen at the end of `contracts.md`.
- The Compact Exchange scaffold repair remains fail-closed for ambiguous content; this document's typed queue is empty and its exchange parses cleanly now, so no binary conversation scaffold remains embedded in `agent:queue`.
- `infra.md` is clean with its completed heads consumed. In `contracts.md`, the stale answered head was struck and committed and its response landed once; the remaining missing-response status belongs to the newly entered prompt, which remains intact for its own cycle.

The verified 0.35.322 binary, native library, JetBrains 0.2.372 package, and VS Code package are installed. IDEA 2026.1 must be restarted once to activate the installed plugin generation; safe supervisor/controller recycling is already requested and will occur at an idle boundary without interrupting live work.

Verification: 9,188 tests passed, release projection and documentation audit passed, and all formal model checks passed. Source commit `e4f6d1eef` and workspace commit `9ebc736fb` are pushed. GitHub CI run `33704789115` is in progress with no surfaced failure at closeout.
<!-- agent:boundary:65937e75 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority start -->
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
