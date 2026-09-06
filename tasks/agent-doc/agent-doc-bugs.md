---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a073ec-e819-7bd3-a292-ccc16b25bc14
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

Fenced closeout queue parsing fixed as 0.35.328; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260904-035542.md`*

Compacted content:
- Archived 12 response topic(s): / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go; / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go; Lazily conformance and ordered native-save release; 9 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): Compact Exchange and session recovery; Response; do [#jetbrain...

### Re: fenced closeout reports becoming queue work — gpt-5

> **Queue prompt:** / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_ROOT scoped to ./cmd/contractpublish only, go vet, go

Fixed in agent-doc 0.35.328 (`93e04437e`). The malformed admission happened because the canonical queue parser could recover a prose-plus-fence task only when it occupied the whole component and placed an explicit request after the fence. With completed rows before the report, a request before the fence, and a trailing empty `- ` editor placeholder, parsing fell through and promoted list-looking lines inside the pasted evidence to independent queue prompts.

The parser now recovers that suffix as one lossless multiline prompt, accepts an unmistakable request on either side of a balanced evidence fence, keeps ordinary balanced Markdown fences inert, and treats an empty list placeholder as non-runnable content. Continuation and queue-consumption regressions prove the complete report is selected and only its exact source span is drained.

Architecture closeout:

- Invariant: evidence inside a balanced Markdown fence never gains queue authority; only the surrounding explicit operator request is runnable, and consumption is limited to its exact parser-owned span.
- Policy owner: `agent-doc-queue::document_queue::parse_spans` owns raw queue bytes to typed `Prompt`/`Freeform` classification. Continuation and queue I/O consume that typed projection rather than reinterpreting Markdown.
- Transition path: operator bytes → typed queue entries → computed live head → continuation → exact-span consume/commit. The concurrent operator replacement of this queue remained authoritative and was not overwritten.
- Reactive topology: source state is the current document body and operator revision; computed state is parser entries, head identity, and drainability; effects are continuation dispatch and exact-span persistence. No one-shot repair path or duplicate policy owner was added.

Verification passed: focused `agent-doc-queue` and `agent-doc-queue-io` suites, `cargo fmt --all -- --check`, and the full `make check`. `make install` installed CLI 0.35.328 and requested a safe idle-boundary controller recycle. The latest visible CI run was green; the new commit will be pushed after this binary-owned closeout, while tag/publication remains held by the document’s existing PyPI-quota status.

### Re: response copied into `agent:queue` — gpt-5

> **Queue prompt:** Run Agent Doc on admin.md wrote the response into the agent:queue.

Fixed in agent-doc 0.35.329 (`511ae8ab0`). The failure was in visible-write snapshot adoption, not in the `admin.md` queue parser. A stale 8,019-byte editor projection fused the already-answered prompt and its `### Re:` response into `agent:queue`; because the number of prompts did not increase, the existing prompt-duplication guard accepted that projection and displaced the newer canonical queue head.

Snapshot adoption now compares the candidate queue with the assistant response in canonical `content_ours`. If response prose appears as a standalone queue entry or as a line inside malformed queue freeform, adoption is blocked, `content_ours` remains authoritative, the rejected editor state is fingerprinted, and the canonical projection is redelivered. The same pure classifier strengthens the existing `session-check` backstop while retaining the regression that permits an assistant response to discuss a queue prompt mid-sentence.

Architecture closeout:

- Invariant: assistant response prose may materialize only in `agent:exchange`; `agent:queue` remains operator-owned.
- Policy owner: the visible-write snapshot-adoption guard in `agent-doc-write-converge-io`.
- Transition path: candidate editor projection plus canonical `content_ours` → component-aware contamination classification → blocked adoption → canonical snapshot plus bad-state fingerprint → editor redelivery.
- Reactive topology: source state is the candidate projection and canonical exchange; computed state is the contamination set; effects are the blocked-adoption record, proof-failure/recycle signal, and canonical redelivery. There is no one-shot bypass.

The committed `admin.md` document still contains the correct response and its newer engineering-estimate queue prompt, so I did not overwrite the foreign-owned live buffer manually. Installing 0.35.329 requested safe idle-boundary supervisor/controller recycling, which activates the repaired adoption path.

Verification passed: the exact fused multiline regression, the write-runtime authority/redelivery regression, the workflow architecture ownership test, `cargo fmt --all -- --check`, and the full `make check` gate. The 0.35.329 CLI, shared library, JetBrains plugin, and VSCodium extension are installed locally. Publication remains held by the existing PyPI-quota status.

### Re: malformed backlog residue — gpt-5

> **Queue prompt:** This document backlog shows my quoted text from another issue and is not well-formed.

Fixed. The backlog is clear again. The quote and command log were orphaned when granular item removal deleted only a tracked parent line and left its flush-left evidence behind.

agent-doc 0.35.330 makes the tracked-work parser the single policy owner for removal boundaries. Removing a matched item now removes its indented continuation plus malformed flush-left quote, log, or diff spill through the next tracked item or structural boundary; headings, components, and other structural postludes remain untouched.

Architecture closeout: the invariant is that deleting a tracked work item deletes its complete logical block and cannot orphan flush-left evidence. `PendingLayout` and `op_remove_matching_tracked_line` own that policy. The transition tests cover matched items with indented continuation, matched items with flush-left spill, preservation at structural boundaries, and unchanged nonmatches. This remains a pure one-shot document transform, so no lifecycle state or Source/Computed/Effect topology is needed; the I/O adapter invokes the policy owner.

Verification passed: focused `agent-doc-element-backlog` tests, the backlog I/O regression, the full `make check` gate, and the final `make install`. The implementation is committed as `07fefad93` (`fix(backlog): remove multiline item spill`).

### Re: do [#fixcausespartial] — gpt-5

> **Queue prompt:**
>
> do [#fixcausespartial]


Implemented and committed agent-doc 0.35.331.

Root cause: after a native save rebuilt the canonical CRDT lineage, the reliable document-op plane correctly returned `StaleLineage`, but that terminal outcome only advanced the durable ACK cursor. It did not fence the still-registered editor that authored the same operations. The editor could therefore replay its partial pre-rebuild buffer over `replica_update`; component reconciliation preserved the full queue item and the stale prefix as two distinct list rows.

Architecture contract:

- Invariant: rejecting a stale-lineage or post-rotation legacy document-op frame must fence every registered author peer before another transport can relay the same intent; canonical text remains on the current lineage.
- Policy owner: `DocumentOpDeltaOutcome` classifies quarantine, and `RelayHub::fence_registered_document_op_origins` owns the registered-origin projection transition. Queue/exchange normalizers do not infer duplicates from text.
- Evidence: typed CRDT operation ids identify insertion authors; delete clocks identify deletion authors. Unknown or retired peers are ignored, and matching-lineage frames do not create a fence.
- Reactive topology: durable frame outcome plus author ids publishes the canonical-projection-required source and queues a projection receipt; the existing direct-update decision observes that source and becomes reproject-only until visible receipt clears it.
- Imperative audit: the missing effect was in the relay-I/O bridge, which previously only logged `StaleLineage`/`LegacyQuarantined`. It now publishes the projection transition while holding the hub, then signals the existing replica event boundary. No polling, wire-schema change, or prompt-prefix heuristic was added.
- Allowed surfaces were limited to realtime relay policy, relay I/O, the realtime workflow spec, and regression tests. The foreign active session document was left untouched.

Regression coverage recreates the exact partial item `- Fix the rebase + merge conflicts in`, rotates to a canonical document containing the full item, rejects the old durable frame, then submits its direct-update twin. Both stale-lineage and legacy-unscoped cases now leave exactly one full item and zero partial rows. Existing coverage also proves an unknown retired author does not fence the current replica and a current-lineage replay stays idempotent.

The latest prior CI run exposed a separate test race: its claimed pre-lock synchronization signal fired before the function call. An explicit post-status/pre-claim phase hook now makes that contention test deterministic; it passed 10 consecutive focused runs.

Verification:

- `cargo test -p agent-doc-document-realtime`: 266 passed.
- `cargo test -p agent-doc-crdt-relay-io`: 78 passed.
- Final-tree `make check`: 9,207 passed, 254 skipped; release projection, docs audit, and all TLA+ models passed.
- `make install`: installed CLI/native library 0.35.331 plus the local JetBrains and VS Code packages.
- Implementation commit: `776333556` (`fix(crdt): fence stale document op origins`).

The implementation push follows this binary-owned session closeout so the new CI run contains the committed result.

### Re: queue resumed — gpt-5

The replica reconciliation has cleared: this cycle was admitted and the `queue: go` edit is committed. Automatic queue draining is resumed; the pending haiven-dev post-item stall report remains intact as the next queue head.

### Re: The agent-doc sessions in haiven-dev keep on stalling after finishing a queue item. Please fix agent-doc causes. — gpt-5

> **Queue prompt:**
>
> The agent-doc sessions in haiven-dev keep on stalling after finishing a queue item. Please fix agent-doc causes.

Fixed in agent-doc 0.35.332 and pushed as `1740de974`.

There were two agent-doc causes:

- Claude had only a cosmetic idle hook at its Stop boundary. One haiven-dev trace genuinely stopped after a clean closeout even though `session-check` said another queue head was drainable.
- Claude's `/loop` admission depended on the agent remembering a separate `drain-claim` shell step. Three other reported stalls were false positives: the agents immediately re-entered `/loop`, but without a fresh lease, so reconciliation classified the healthy continuation as stalled.

The lifecycle is now enforced by the binary:

1. Claude `UserPromptSubmit` binds the exact agent-doc session.
2. Clean closeout records the durable continuation state.
3. The first Claude Stop is blocked when the shared queue detector finds a drainable head; the injected reason directs the agent back through `/loop` without recursively invoking agent-doc from the owner pane.
4. `/loop` admission acquires or refreshes the `claude_loop` lease before preflight reconciliation, removing the separate remembered claim step.
5. `stop_hook_active` permits the bounded fallback Stop, avoiding an infinite hook loop while preserving supervisor recovery.

The durable session binding and continuation marker/stall projection are the source state; the shared queue detector owns drainability; Stop JSON and lease refresh are effects; the next admitted preflight consumes the pending continuation. Unbound Claude sessions remain a no-op.

Changed surfaces include the prompt parser and preflight adapter, the new exact-session Claude Stop handler and CLI entrypoint, hook installation, the lifecycle spec/runbook, generated harness artifacts, and the 0.35.332 version set. The updated binary and editor integrations are installed locally; supervisor/controller recycle was scheduled through the installer.

Verification is green:

- Focused regressions cover direct versus loop-wrapped prompts, lease-before-preflight ordering, exact-session Stop isolation, clean-closeout blocking, recursion bounds, Claude payload compatibility, and idempotent hook installation.
- Local `make check`: 9,212 passed, 254 skipped; clippy, documentation/version audits, and formal models passed.
- Local `make install` passed.
- GitHub CI, Book, and Pages workflows passed for `1740de974`.

Operator verification: finish a queue item in a Claude haiven-dev session while another head is drainable. The first Stop should inject the next-head continuation; the subsequent `/loop` preflight should hold the lease and should not emit `queue_stall_detected`.

Commits: agent-doc `1740de974` (pushed) and parent integration `8bc4f0811`. No tag or package publication was attempted; this cycle's preset was commit/push, not release.

### Re: In contracts.md, I ran ! agent-doc admin recycle and the agent-doc tmux window went away. Restore the runtime as soon as you diagnose the issue. The fix the agent-doc causes. — gpt-5

Restored immediately and fixed at all three owning boundaries.

- The live `contracts.md` runtime never died: actor generation 13 and pane `%23` remained healthy. I moved its `@36` `agent-doc` window back to tmux session `0`, joined `%23` into `0:0`, and selected it. After the fixed controller observed it repeatedly, `0:0 @36` still held `%23` as the active pane. Both controller PID 3956015 and supervisor PID 3097040 now run the installed binary inode and report fresh/ready.
- Recycle root cause: the replacement controller inherited its ambient `TMUX_PANE` from an IDE-owned session and windowless sync selected that ambient session before consulting the generation-fenced actor bindings. Sync now derives session authority from the focused live actor, then a visible desired sibling, before ambient/controller context. A stashed focused actor cannot pull an already-visible shared `agent-doc` window into another session.
- Manual-pane reversion root cause: an editor `projection` generation remained structurally authoritative after it had converged, so later pane selection or an added live actor pane was treated as drift and replayed. Editor projection is now apply-once per generation. Once that exact generation converges, later focus/cardinality drift is operator-owned; a genuinely new editor observation creates a new generation and may reconcile once.
- IDEA terminal root cause: JetBrains `Run Agent Doc` explicitly called `IdeTerminalCoordinator.ensureAndAttach` before its controller route, and the shared attach decision activated a surviving IDE terminal tab even when the tmux session already had an attached client. Run now submits `editor_route` directly and never creates, attaches, selects, focuses, or reveals an IDE terminal. Autostarting Sync retains terminal bootstrap, but any attached client wins even if an IDE tab still exists.

The authority order and post-convergence ownership rules are specified in `specs/07-session-tmux-commands.md` and the JetBrains spec. Regression coverage includes pure session-selection tests, stashed-focus/visible-sibling tests, a live isolated-tmux recycle-context test, controller projection tests for manual selection and manual pane addition, and JetBrains tests for direct Run routing plus attached-session terminal suppression.

Verification and publication:

- Full `make check`: passed on the final combined tree.
- Targeted JetBrains and Rust regression suites: passed.
- `make install`: passed; binary, native library, JetBrains plugin package, and VS Code extension installed. The corrected Kotlin plugin takes effect after the required IDEA plugin restart.
- Product commit `b8d6192d5` (`fix: preserve operator tmux placement`) pushed to `origin/main`; GitHub CI run 33947369512 is in progress.
- Parent submodule integration commit: `3aa58a4d8`. The parent branch has 377 unrelated local commits, so I did not push that unrelated history.
- GitHub Actions run `34010967526` was in progress with no failure when this response was written; the prior `main` CI run was green.
- Across a 10-second live sample, the relevant retry-event counters stayed flat (`10309 → 10309` in this project and `11206 → 11206` in the document's owning project). The now-idle owning controller averaged 0.7% of one CPU core.
- Public-repository private-name denylist and `git diff --check` passed.
- Release build/install passed for the CLI, shared library, JetBrains plugin, and VS Code extension.
- Full `make check` passed.
- Focused policy, command-rendering, start-admission, reexec, and architecture-boundary tests passed.
- Operator verification: the installed 0.35.333 controllers automatically recreated `monsterrodholders.md` in pane `%88`; both its controller and supervisor match the installed binary inode; its original document-scoped pause and reason remain intact.
- Edited surfaces: pure supervisor policy and tests, start command rendering and tests, route/start runtime plumbing, CLI parsing, source-contract coverage, and the tmux/control-plane specifications.
- Imperative boundaries: route startup classifies `skip_wait` as `layout-provision`; start-command rendering carries the typed hidden CLI value; start I/O applies the same typed admission decision. Queue watching remains the later dispatch fence.
- Reactive topology: editor surface source → desired pane-layout projection → structural layout effect → observed pane/receipt source. Successful provision updates the observation, removes the mismatch, and quiesces the effect.
- Transition rows:
- Policy owner: `agent-doc-supervisor::route_owned` owns the pure `RouteOwnedStartPurpose`, `RouteOwnedStartAdmission`, and `route_owned_start_blocked_by_queue_control` decision.
- Invariant: queue control gates autonomous command dispatch, not generation-fenced structural ownership needed to realize the desired editor layout. Once the pane is observable, an unchanged desired projection must settle and stop timer retries.

> **Queue prompt:**
>
> In contracts.md, I ran `! agent-doc admin recycle` and the agent-doc tmux window went away. Restore the runtime as soon as you diagnose the issue. The fix the agent-doc causes.
> **Queue prompt:**
>
> Why is agent-doc thrashing in the background? Please make agent-doc more efficient when idle. Also, agent-doc is not auto-syncing the tmux pane with monsterrodholder.md. monsterrodholder.md has had many issues. Please fix these issues.

### Re: Idle efficiency and tmux auto-sync — gpt-5

Fixed and shipped in agent-doc 0.35.333.

The thrash and the missing tmux pane were the same convergence bug. The editor surface correctly kept `monsterrodholders.md` in the desired layout, but its queue was intentionally paused for operator-gated work. Route startup treated that pause as a reason to reject every startup, including the provision-only startup used by layout sync. Because no pane could be created, the observed layout never matched the desired layout and both project controllers retried the structural effect every five seconds indefinitely.

The queue pause now fences autonomous command dispatch without fencing structural pane/supervisor provisioning. Provision-only startup carries a typed `layout-provision` purpose across both process boundaries, creates an idle route-owned supervisor without sending an initial reopen command, and leaves the idle queue watcher responsible for honoring the existing pause before any later dispatch.

Architecture closeout:

- Invariant: queue control gates autonomous command dispatch, not generation-fenced structural ownership needed to realize the desired editor layout. Once the pane is observable, an unchanged desired projection must settle and stop timer retries.
- Policy owner: `agent-doc-supervisor::route_owned` owns the pure `RouteOwnedStartPurpose`, `RouteOwnedStartAdmission`, and `route_owned_start_blocked_by_queue_control` decision.
- Transition rows:

  | Admission | Effective queue pause | Result |
  | --- | --- | --- |
  | route-owned new `dispatch` session | active | blocked |
  | route-owned new `layout-provision` session | active | idle supervisor allowed; no reopen |
  | route-owned supervisor reentry | active | allowed |
  | non-route-owned startup | active | unchanged/allowed |

- Reactive topology: editor surface source → desired pane-layout projection → structural layout effect → observed pane/receipt source. Successful provision updates the observation, removes the mismatch, and quiesces the effect.
- Imperative boundaries: route startup classifies `skip_wait` as `layout-provision`; start-command rendering carries the typed hidden CLI value; start I/O applies the same typed admission decision. Queue watching remains the later dispatch fence.
- Edited surfaces: pure supervisor policy and tests, start command rendering and tests, route/start runtime plumbing, CLI parsing, source-contract coverage, and the tmux/control-plane specifications.
- Operator verification: the installed 0.35.333 controllers automatically recreated `monsterrodholders.md` in pane `%88`; both its controller and supervisor match the installed binary inode; its original document-scoped pause and reason remain intact.

Validation:

- Focused policy, command-rendering, start-admission, reexec, and architecture-boundary tests passed.
- Full `make check` passed.
- Release build/install passed for the CLI, shared library, JetBrains plugin, and VS Code extension.
- Public-repository private-name denylist and `git diff --check` passed.
- Across a 10-second live sample, the relevant retry-event counters stayed flat (`10309 → 10309` in this project and `11206 → 11206` in the document's owning project). The now-idle owning controller averaged 0.7% of one CPU core.
- GitHub Actions run `34010967526` was in progress with no failure when this response was written; the prior `main` CI run was green.

Product commit `7365e3ceb` (`fix: quiesce paused layout provisioning`) was pushed to `agent-doc/main`. Superproject integration commit `f07eba70c` is local only because the superproject branch already contains 400 unrelated unpushed commits.
<!-- agent:boundary:d997bd6b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 Why is agent-doc thrashing in the background? Please make agent-doc more efficient when idle. Also, agent-doc is not auto-syncing the tmux pane with monsterrodholder.md. monsterrodholder.md has had many issues. Please fix these issues.
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
