---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a12175-b9c0-7db0-af02-218c5501efe7
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.475. v0.35.476 is not yet tagged or published; the actionable review added the release gap and two correctness follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261008-011101.md`*

Compacted content:
- Archived 12 response topic(s): actionable review and release status; v0.35.476 release and review closeout; `/tmp` sizing; 9 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): release and delegated fixes; GH #172, GH #173, SDK Stop-hook r...
Fix in a subagent:
```
• Blocked by hook
  └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
dev/tasks/frontend.md. [session-check] INTERRUPTED: binary-owned captured closeout for `/home/brian/work/btakita/agent-
loop/src/haiven-dev/tasks/frontend.md` remains pending: response_captured: captured response is waiting for exact
authority/disk convergence before write-applied. The same capture is durable; do not issue another closeout payload and
do not force disk. Retry only `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
frontend.md`. The latest assistant text was captured into the pending/capture ledger before auto-close. The hook could
not finish the required commit boundary: refused editor IPC for /home/brian/work/btakita/agent-loop/src/haiven-dev/
tasks/frontend.md because reliable document liveness has no matching editor registration (reliable_editor_live=true,
registration_available=false); refresh or update the editor integration before retrying closeout. The retained capture
or projection is already durable and this exact retained intent commits itself once delivery converges — this is a
deferral, not a lost response. Wait for its controller-owned terminal state edge; do not use the document's current-
cycle status as proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force
disk, or `admin recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own
either: `admin reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor
holding the document is not serving its replica, and then its remedy governs. Do not send the final answer yet. If the
response is missing from the document, run `agent-doc repair /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
frontend.md` first. Then finish the commit boundary for this turn and end with `agent-doc session-check /home/brian/
work/btakita/agent-loop/src/haiven-dev/tasks/frontend.md`.
```

### Re: retained closeouts, queue integrity, GH #181–#184, and v0.35.478 — gpt-6.1-sol · 2026-10-08T00:34-04:00

> **Queue prompt:**
>
> do [#correspondingsubagentspr]


> **Chat prompt (#chatprompt):** • Hook stopped
>   └ agent-doc Stop hook already continued once for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
> frontend.md, but the cycle is still open. [session-check] INTERRUPTED: completed free-text agent:queue head(s) "Why
> does PR 653 have a `dev:stg` task? Why can't /sbx + /stg work with the `dev` task?" are still active in the committed
> queue even though exchange history contains a `Queue prompt` echo proving they were already answered — completed queue
> residue would re-run stale work
>     [session-check] hint: this cycle is already committed, so run `agent-doc preflight /home/brian/work/btakita/agent-
> loop/src/haiven-dev/tasks/frontend.md` first to open the binary-owned repair cycle and strike the answered head, then
> run `agent-doc commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/frontend.md` without resubmitting the
> response; add `<!-- no-free-text-queue-head-guard -->` only if keeping the answered row active is intentional (see
> #qheadresidue/#qresiduecommittedhint/#qresiduecommitonly)
>
>   Worked for 9m 1s • 9:12 PM

> **Chat prompt (#chatprompt):** subagent fix: › agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md
> • Blocked by hook
>   └ agent-doc Stop hook opened a fresh cycle for post-commit prompt work in /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/tools.md, then retained the response under that cycle but could not finish closeout. The latest
> assistant text was captured into the pending/capture ledger before auto-close. The hook could not finish the required
> commit boundary: serialized_atomic_write: retained editor-owned write for /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/tools.md before retrying live model reconciliation (intent_id=1791423594030432053-3-
> da36ebc5d3b5f64e23d49913e8ea5fcfa1f78da5b758ff23b37d1ff8f071e137): serialized_atomic_write: retained the canonical
> write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md in CRDT + Lazily state
> (intent_id=1791423592466877439-2-e2e45578149b93042f021ca906a1c2c7e2091d135b39982c6144948e14dd1c05), but the editor
> replica disappeared during delivery admission; disk was not written; supervisor_recycle=request_skipped
> reason=editor_reregister_primary editor_replica_reregister=requested:1;
> recovery=await_editor_replica_no_disk_write_then_session_check; run only agent-doc session-check for the existing
> binary-owned capture; do not resubmit finalize, write --commit, or --force-disk; The retained capture or projection is
> already durable and this exact retained intent commits itself once delivery converges — this is a deferral, not a lost
> response. Wait for its controller-owned terminal state edge; do not use the document's current-cycle status as proof
> because queue continuation can advance to a different cycle. Do NOT re-send the response, force disk, or `admin
> recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own either: `admin
> reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor holding the document
> is not serving its replica, and then its remedy governs pending change retained for retry
> [recovery=await_editor_replica_no_disk_write]. The registered editor endpoint REJECTED the delivery receipt (`IPC
> receipt rejected`) — it answered NO, so the delivery projection this write is waiting on will never converge and
> waiting will not commit it. The response is durable and NOT lost: do NOT re-send it or force disk. Recover by removing
> the rejecting endpoint: restart or reload the editor that has /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
> tools.md open (a backend still running deleted plugin jars after an update is the observed cause). Once that endpoint
> is gone or re-registered, the retained intent converges through document authority on its own; then run `agent-doc
> session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` and follow the recovery it names. Do
> not recapture the response or rerun finalize/write. Run `agent-doc session-check /home/brian/work/btakita/agent-loop/
> src/haiven-dev/tasks/tools.md` and let the binary-owned keyed retry finish.

> **Chat prompt (#chatprompt):** • The response is already durable in agent-doc’s retained ledger; I won’t resend or force-write it. I’m running only
>   the required session-check so the binary-owned retry can reconcile the editor projection and report the terminal
>   state.
>
> • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md
>   └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md total_ms=43…
>     Error: [session-check] INTERRUPTED: canonical editor authority and disk projection diverge for /home/brian/work/bt…
>     + 2 lines (ctrl+t to expand)
>
> • Agent-doc remains interrupted because the editor endpoint rejected the delivery receipt. The response is durable and
>   must not be resent or force-written.
>
>   Please restart or reload the editor holding tasks/tools.md. Once it re-registers, rerun agent-doc session-check
>   tasks/tools.md; the retained intent should converge automatically.

> **Chat prompt (#chatprompt):** also fix in subagent: Run Agent Doc in api.md: The text of my queue item was duplicated: `Make another PR to merge #666 into `dev`.Make another PR to merge #666 into `dev`.`

> **Queue prompt:** do [#fixsubagent]

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/181

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/182

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/183

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/184: Removing the Context Chip makes Run Agent Doc work. agent-doc should support Claude Code's context chip.

> **Queue prompt:** release + publish

Released agent-doc **v0.35.478** from commit `06f4d92c2`. The authoritative gate passed 9,669/9,669 main tests, 193/193 VS Code tests, the cross-editor native harness, bootstrap tests, editor parity, documentation checks, tmux CI, and all positive/non-vacuity TLA+ models. The tag and remote `main` push succeeded, and `install-full` completed for CLI/native 0.35.478, JetBrains 0.2.506, and VS Code 0.2.80. GitHub Release, CI, and PyPI workflows were still running at the one-time post-push observation; Book and Pages were green.

The delegated fixes shipped:

- **GH #181** (`29d586e4b`): durable capture evidence now drives guarded supervisor refresh instead of leaving a captured open cycle deferred after its wedge latch is spent.
- **GH #182** (`3bae8f74b`): queue identity is structural, admission reserves document IDs, and one invalid batch item no longer discards valid siblings.
- **GH #183** (`45bacc111`): misplaced queue-only flags receive one consistent verdict, and opening-marker lint errors block before response mutation.
- **GH #184** (`da042e5f5`): Claude Code's leading context chip is normalized as composer chrome—including the NBSP rendering—while genuine typed text after it still blocks routing.
- **Editor recovery and queue duplication:** registration-lagged closeouts recover through document authority; rejected editor generations are quarantined; JetBrains replacement replicas use bounded splices rather than CRDT identity-destroying whole-document replacement; exact answered bare residue can be acknowledged safely.

GH #181, #182, #183, and #184 are closed with release comments.

For `tools.md`, the installed rejected-generation recovery converged the retained response without resending it; its session check completed successfully.

For `api.md`, the malformed duplicated row is absent. The newly installed exact-ack path observed that convergence had already removed it; `agent-doc commit` then completed as `78d11d1cde`, and session-check reports a committed cycle with no drainable work.

For `frontend.md`, the retained response is present and supervisor/editor convergence health is good. The remaining artifact is the one-byte newline deletion `...`; the existing repair path has a passing exact regression. The file currently has no live JetBrains registration, so this item is gated on reopening/refreshing `frontend.md`. Once registered, run its existing `agent-doc repair`, then the committed-residue `preflight` + `commit` remedy without resubmitting the response.
#subagent: Fix sdk.md issue: My queue item was duplicated during Run Agent Doc.
```
- 🚧 In https://github.com/haiven-dev/haiven-tools/pull/2, describe what Mise is and how it will be used. Add docs which includes installing and usage. Copy the haiven-contracts pin logic from haiven-contracts. This will be the first tool. Setup .mise.toml which includes Python 3.14 and other dependencies.
- In https://github.com/haiven-dev/haiven-tools/pull/2, describe what Mise is and how it will be used. Add docs which includes installing and usage. Copy the haiven-contracts pin logic from haiven-contracts. This will be the first tool. Setup .mise.toml which includes Python 3.14.
```
The sdk.md current markers are drifted from the actual work state. Same with this document...there is drift between what is being worked on and which queue items have the `🚧` prefix:
```
- do [#testhttpsgithub]
- In https://github.com/haiven-dev/haiven-tools/pull/2, describe what Mise is and how it will be used. Add docs which includes installing and usage. Copy the haiven-contracts pin logic from haiven-contracts. This will be the first tool. Setup .mise.toml which includes Python 3.14 and other dependencies.
- 🚧 After a successful test of [#testhttpsgithub], squash merge the entire https://github.com/haiven-dev/haiven-sdk/pull/57 stack
```

### Re: GH #185 remote replacement column slot — gpt-6.1-sol · 2026-10-08T12:30-04:00

> **Queue prompt:** Make the tmux pane title contain the document name in all states.

**Deferred:** This remains queued for its dedicated implementation cycle; this closeout is scoped only to GH #185.

> **Queue prompt:** release + publish

**Deferred:** This remains queued until the outstanding release-bound fixes finish; this closeout is scoped only to GH #185.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/185

Completed and closed GitHub issue #185. The fix is commit `422c4847522d88ed5ad0e93050a66b80303297fe`, included in v0.35.479: one-for-one remote editor replacements inherit the dropped document's column slot, retained ordering is no longer mislabeled as editor geometry, and tmux preserves that upstream order. Focused editor-surface and tmux regressions passed, the JetBrains `LayoutDetectorTest` and `SyncLayoutActionTest` classes passed, and CI run 37746172117 succeeded at current `origin/main`.

### Re: release-critical drift and admission fixes — gpt-5 · 2026-10-08T14:45:50-04:00

Two independently green fixes are now merged into `main`: PR #192 preserves queued work across post-commit sync, and PR #194 records real operation-log actor provenance. GitHub automatically closed issue #187 from the merged fix.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/187

Completed: PR #194 merged as `8b90b77fa9a4abe9087e22c1d03c3981c9b82f76`; its full CI passed, and issue #187 is closed.

PR #191 contains the editor-authority/replay-settlement drift fix and is still in CI. PR #193 and PR #195 have review-driven race remediations in isolated full gates. The infra layout-convergence fix is implemented and awaiting its own uncontended gate. Issue #188 remains open until PR #191 proves the second half of that failure chain.

No release was published in this cycle. The remaining queue heads are claimed or waiting for a free subagent slot; they are intentionally left open. The exchange is over the size warning threshold, but the active queue is not being stalled for compaction.

### Re: tmux pane title document identity — gpt-5 · 2026-10-08T14:55:20-04:00

> **Queue prompt:** Make the tmux pane title contain the document name in all states.

Completed on current `main` by commit `e23876c3d` (`fix: keep document name in tmux pane titles`). The shared title policy and registered-document lookup cover all four controlled states: idle (`sample.md`), active (`⟳ sample.md — turn in progress`), stale (`⚠ STALE SUPERVISOR — sample.md`), and stale-active (`⚠ STALE SUPERVISOR — sample.md — turn in progress`).

Focused verification passed 16 tests with zero failures: the 14 `agent-doc-turn` turn-status tests, `registered_document_name_is_resolved_by_pane`, and `stale_title_marker_is_recognised_in_every_title_shape`. The only tmux `select-pane -T` producers are routed through this policy. The current-main worktree was clean, so no duplicate commit or no-op PR was created.

### Re: GitHub issue #186 — gpt-5 · 2026-10-08T14:56:29-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/186

Already completed and released in v0.35.479 by commit `b526f1c9e1a9837e3876101b7169cc471ccc939e` (`fix: expose lint diagnostics in JetBrains`); GitHub issue #186 is closed. The fix makes lint extraction multiline and CRLF-safe despite a trailing `[dogfood]` record, exposes the read-only `agent-doc lint` command, wires the JetBrains lint action, and records the first rule/line/column in blocked-operation diagnostics.

Fresh verification on current `main` passed: 24 lint-I/O tests, 2 lint CLI tests, 73 JetBrains `TerminalUtilTest` tests, and 6 popup-action tests. The verification worktree was clean, so no duplicate patch was created.

### Re: GitHub issue #188 — gpt-5 · 2026-10-08T14:57:10-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/188

Completed by merged PRs #191 and #192; GitHub issue #188 is closed. PR #191 preserves live editor authority, requires proven replay settlement, and makes the terminal stale-preflight transition atomic against turn-active writers. PR #192 preserves active one-shot queue intent across post-commit consume/sync and prevents a closed response from receiving a retroactive queue-prompt splice.

Both PRs passed their full CI gates before merge. Their merge commits are `2570cbe71c059fcd7cee3c25f5f348d2b180a441` (#191) and `403df31528e04a6dbefde815bb54a9e1d59c53ff` (#192).

### Re: release and publication — gpt-5 · 2026-10-08T15:14:07-04:00

> **Queue prompt:** release + publish

Completed for v0.35.479. The failed x86_64 macOS release job was rerun successfully at the original tag, after which GitHub published the release with Linux, macOS, Windows, VS Code, JetBrains, and checksum artifacts: https://github.com/btakita/agent-doc/releases/tag/v0.35.479

The original tag-bound PyPI workflow was then rerun; its build, publish, and verification jobs all passed. `agent-doc` 0.35.479 is now present on PyPI. The post-v0.35.479 fixes remain a separate future v0.35.480 batch and were not folded into this immutable tag.
> **Chat prompt (#chatprompt):** subagent: fix: • Failed (exit 1) agent-doc session-check tasks/fpe.md
>   └ [perf] session_check.operations file=tasks/fpe.md total_ms=7838 attributed_ms=7042 nested=true self_heal_response_…
>     Error: [session-check] INTERRUPTED: canonical editor authority and disk projection diverge for tasks/fpe.md (autho…
>     + 2 lines (^t to expand)

### Re: fpe.md editor-authority divergence — gpt-5 · 2026-10-08T15:15:38-04:00

Dispatched immediately to a dedicated subagent. The worker is reproducing the failure from read-only session evidence and state, without running recovery or mutation commands against live `tasks/fpe.md`. Its implementation scope is the agent-doc producer/recovery path that allowed canonical editor authority and the disk projection to diverge after response self-healing, with deterministic regression coverage and a full focused gate before a separate PR.

The previously assigned #196 worker was paused—not discarded—to free the execution slot; its queue claim remains intact for resumption after this urgent fix.
> **Chat prompt (#chatprompt):** • Agent-doc now reports needs_operator: the JetBrains endpoint rejected the save receipt and cannot converge.
>
>   Please restart or reload the editor that has tasks/fpe.md open. Do not manually save or edit the document; the
>   response is retained and will resume automatically.
>
> • Hook stopped
>   └ agent-doc Stop hook found a binary-owned closeout still converging for /home/brian/work/btakita/agent-loop/src/
> haiven-dev/tasks/fpe.md. [session-check] INTERRUPTED: binary-owned response delivery `1791486890557754463-1-
> cba5f428f43547873b1e4470a0af6ac735f3d2a86a68be22c92c9818e72ec6a1` is retained for `/home/brian/work/btakita/agent-loop/
> src/haiven-dev/tasks/fpe.md` (reason=editor_projection_pending, source=pending_write,
> target_hash=cba5f428f43547873b1e4470a0af6ac735f3d2a86a68be22c92c9818e72ec6a1); the same capture will resume
> automatically after editor/controller delivery converges. Same-capture recovery remains pending: retained target has
> not reached exact canonical/disk convergence. Do not issue another closeout payload and do not force disk; retry only
> `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md`. The captured response is
> retained and the agent-doc binary/supervisor owns the keyed editor/CRDT retry and terminal commit. Do not recapture the
> response, rerun finalize, kill the controller, or use `--force-disk`; only re-check session status after the binary
> reports recovery, unless it explicitly reports `needs_operator`. Do not send the final answer yet.
>
>   Worked for 6m 28s • 3:18 PM

> **Chat prompt (#chatprompt):** Should I restart now or do you need to diagnose?

> **Chat prompt (#chatprompt):** restarted

### Re: FPE editor restart and retained delivery — gpt-5 · 2026-10-08T15:20:49-04:00

The editor restart completed and the old JetBrains process exited; a new backend process is running. The first post-restart observation still received `IPC receipt rejected` for the retained FPE delivery, so no response was resent and no disk/editor mutation was attempted. The diagnostic evidence needed for the code fix is already preserved and the assigned subagent continues in an isolated worktree.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/197

**Deferred:** All three worker slots are occupied by the FPE convergence fix and the already-running #189/#190 fixes. Issue #197 is the next subagent dispatch when a slot returns; it is not being treated as complete or executed inline.

### Re: Claude Code session-document focus — gpt-5 · 2026-10-08T15:42:16-04:00

> **Queue prompt:**
>
> do [#possibletellclaude]


Completed and merged through PR #199 as merge commit `80e61a04a9fd1d24ca40dac4793849701b374eef`. Claude Code supports explicit file context interactively through `@file`/IDE context, but its headless `-p` interface has no dedicated local-file focus flag. Agent-doc now supplies the exact session-document path as primary, model-visible turn context across normal, resumed, streaming, orchestration, and direct-run paths, below the prompt-cache boundary.

Focused prompt-context, stream, direct-run, and orchestration tests passed; `make check-fast` and PR CI both passed before merge.
> **Chat prompt (#chatprompt):** Can we increase the number of subagents? At this rate, the release will take all day.

> **Chat prompt (#chatprompt):** would restarting the agent give us 6 subagents?

> **Chat prompt (#chatprompt):** Make my default subagent count 6

> **Chat prompt (#chatprompt):** Can I restart this agent process to get the new configuration? Or can we transfer to a new session?

### Re: Six-subagent default and session transfer — gpt-5 · 2026-10-08T16:51-04:00

Set the user-global Codex configuration in `/home/brian/.codex/config.toml`:

```toml
[agents]
max_concurrent_threads_per_session = 6
```

Codex 0.161.0 accepted the file under strict config parsing, and `codex doctor` completed with no failures. The setting is the stable spawned-subagent limit: the root agent is separate.

It applies when a new Codex session is created. Restart by exiting this process, starting a fresh Codex session, and invoking `agent-doc tasks/agent-doc/agent-doc-bugs.md`. Starting a fresh thread is safer than resuming this thread because this thread's four-slot runtime was fixed at session creation. The session document carries the queue and this handoff.

### Re: Claim for Tmux Pane components — gpt-5 · 2026-10-08T16:51-04:00

PR [#206](https://github.com/btakita/agent-doc/pull/206) adds the canonical scaffold order `status → exchange → queue → backlog → review → icebox → done → notes` through the shared generator used by CLI, JetBrains, and VS Code. Focused tests, strict Clippy, JetBrains tests, 8,068 affected tests, and all batched suites passed. The queue item remains open and its claim was released so the new session can integrate it after CI.

### Re: tools.md duplicate projection and fenced Markdown — gpt-5 · 2026-10-08T16:51-04:00

PR [#208](https://github.com/btakita/agent-doc/pull/208) replaces prefix/similarity inference with a fail-closed baseline → projection → observed structural proof and exact backlog receipt. It preserves one durable ID across CRDT orderings and retries while refusing ambiguous identities, reused IDs, extra rows, or unrelated edits. Focused tests, both affected crate suites, strict Clippy, and all 9,729 nextest tests passed; the remaining batched run was interrupted only for this session transfer after its reported binaries stayed green.

PR [#207](https://github.com/btakita/agent-doc/pull/207) fixes the fenced-content defect at the AST boundary: backlog extraction, malformed-item detection, and nested-subtask normalization now exclude pulldown-cmark literal ranges for fenced/indented code and raw HTML. Its full `make check-fast` passed, including 8,914 nextest tests and all batched/doc-test phases.

Both queue items remain open for integration, and their claims were released for the new session.

### Re: Codex Stop-hook continuation label — gpt-5 · 2026-10-08T16:51-04:00

PR [#209](https://github.com/btakita/agent-doc/pull/209) addresses the reported `Blocked by hook` presentation. Codex requires `decision: "block"` for a Stop continuation and renders that envelope with the alarming host label, so agent-doc now leads with `Queue continuation, not an error:` and explains the label. The fail-first regression and all 94 package tests passed; the broader gate had started but was intentionally stopped for transfer. Its queue claim was released.

The separate report that a steering Run Agent Doc was never submitted remains queued; I have not conflated that dispatch failure with the wording-only PR #209.

The exchange is oversized, but the active queue was not stalled for compaction.> **Chat prompt (#chatprompt):** Fix the cause of the error


### Re: retained-delivery false error — gpt-5 · 2026-10-08T17:24-04:00

Fixed by a dedicated subagent in [PR #210](https://github.com/btakita/agent-doc/pull/210), commit `f6e4e9b426fc26ff69e53f2d174ef79fa05933d6`.

The operation log proved a race: the controller could successfully settle and remove a retained write after the synchronous caller constructed a narrow projection refusal but before its error handler inspected `pending_document_write`. The handler then saw no pending intent and returned a stale `delivery_projection_pending` exit 1 even though delivery had already converged—the immediate successful `session-check` was the evidence of that mismatch.

The fix accepts this settled-before-inspection path only when the exact converging token matches, the response is committed, no retained intent remains observable, and the existing bounded delivery plus tracked-work landing proofs both succeed. Missing or mismatched evidence still fails closed.

The full authoritative gate passed: 9,728 nextest tests, the serialized controller/session-check tail, 193 VS Code tests, strict Clippy, cross-editor checks, Python tests, documentation checks, and TLA+ checks. GitHub CI is in progress.

The other messages were classified rather than conflated:

- The pending-capture refusal was expected because the response had no backlog mutation and omitted the explicit `--no-followups` declaration; this closeout uses that first-class declaration.
- PRs [#207](https://github.com/btakita/agent-doc/pull/207) and [#208](https://github.com/btakita/agent-doc/pull/208) remain the owners of fenced Markdown parsing and projection identity.
- The edited multi-queue prompt was forwarded verbatim to its existing `subagent:multi-queue-ids` owner without duplicate dispatch.

The exchange remains oversized, but the active queue was not stalled for compaction.

### Re: completed subagent batch and v0.35.480 reconciliation — gpt-5 · 2026-10-09T01:33-04:00

> **Queue prompt:**
>
> do [#reviewiceboxnotes]


The full claimed batch is complete and reconciled against its merged or released evidence. The tracked Claim for Tmux Pane work is complete in PR #206, merge `a51366d78049`. The orphaned `crossplatformmise-txpw` queue head was guardedly consumed only after the binary proved its backing row was already absent; PR #208 merged as `c4c434e88cd3`.

> **Queue prompt:** 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/189

Completed and closed through PR #201, merge `0f04178e2b`.

> **Queue prompt:** 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/190

Completed and closed through PR #202, merge `f2cf4919e1fc`.

> **Queue prompt:** 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/196

Completed and closed through PR #204, merge `c7348c41ee94`.

> **Queue prompt:** 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/197

Completed and closed through PR #205, merge `71633943b8fd`.

> **Queue prompt:** 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/198

Completed and closed through PR #217, merge `9a492e2077e9`.

> **Queue prompt:** 🚧 Add support for multiple `agent:queue` tags. Should we have a `id=...` attribute to distinguish between queues? I want to have different queues to allow different attributes. Some queues may be a subagents queue and others may be ordered. This post-release queue is a perfect example use case. If you can start the subagent now and keep it out of the upcoming release, do that. I want to be able to have different queues and even allow a queue to depend on another queue. Perhaps a release B queue can organize release B after the release A queue is complete.

Completed together with the multi-queue dependency and sequencing model in PR #220, merge `735c0d8fa2a4b28edc4a3e04bd54941a9af832a0`; its full CI passed.

> **Queue prompt:** I started a new Codex session and removed the `resume.codex` frontmatter. Run Agent Doc error: [route] active closeout for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md could not be drained before reroute; nothing was dispatched and existing queue head "reviewiceboxnotes" remains queued behind the closeout (blocker: closeout recovery retire_stale_capture [open_cycle]: proof: response heading "### Re: retained-delivery false error — gpt-5 · 2026-10-08T17:24-04:00" is already answered in live exchange) ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_owner_absent ui_outcome_class=blocked next_action=retry_after_closeout_recovery

Completed in PR #215, merge `c50447139b23`, and released in v0.35.480. The installed recovery reconciled the interrupted empty response and restored an admitted, committed boundary without a disk-authority fallback.

> **Queue prompt:** Give queues an auto-dag like queue items. So a queue id=B must run after queue id=A ends, should we have an after=A in queue id=B? If we want to make a reoccurring sequence, perhaps queue id=A could be later filled again by queue id=B once complete? I wonder if you have suggestions on how we can have an interface algorithm on adding queue items defined in the md file.

Completed in PR #220, merge `735c0d8fa2a4b28edc4a3e04bd54941a9af832a0`. Multiple named queues now use dependency-aware auto-DAG sequencing, including repeatable queue relationships, with full CI green.

> **Queue prompt:** A steering Run Agent Doc on this file was not submitted to Codex. I manually submitted it.

Completed on `main` by commit `f3ff1b4fae`.

> **Queue prompt:** I authored a queue item which fenced backlog items that were written in tools.md. agent-doc created backlog items for the fenced backlog items. agent-doc should have used the AST to understand the content was fenced...not regexes.

Completed in PR #207, merge `327671681dd6`. Queue/backlog extraction now respects the Markdown AST's literal ranges instead of treating fenced content as live tracked work.

> **Queue prompt:** fix tools.md issue:
>
> ```
> • Blocked by hook
>   └ agent-doc Stop hook kept an active `agent:queue auto` moving for /home/brian/work/btakita/agent-loop/src/haiven-
> dev/tasks/tools.md. The next queue prompt is "Update https://github.com/haiven-dev/haiven-tools/pull/2 description".
> Continue THIS turn in-pane: answer that prompt in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md and
> persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` (or `agent-doc
> write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/tools.md`). Do NOT run `agent-doc /home/brian/
> work/btakita/agent-loop/src/haiven-dev/tasks/tools.md` from this pane — that re-invokes the owner pane and hits the
> recursive-direct-invocation deadlock guard, and do not send the final answer yet.
> ```

Completed in PR #209, merge `bc48a8543ff8`. Codex Stop continuation now presents the required continuation as queue progress rather than an unexplained error while preserving the host protocol.

> **Queue prompt:** release + publish

Completed as agent-doc v0.35.480 at tag/`main` commit `a87ff750f618e`; checks, publication, and installation are green.

Separately, PR #219's detached-surface substrate merged post-release as `566c66922609`. PR #218's full Remote Dev isolation remains open because it still depends on the 262 split-plugin migration; it is not represented here as a completed queue item.

### Re: #221 usability and maintenance consequences — gpt-5 · 2026-10-09T11:16-04:00

> **Queue prompt:**
>
> do [#usabilitymaintenancecons]


Issue [#221](https://github.com/btakita/agent-doc/issues/221), “JetBrains plugin: split into frontend/backend/shared modules for Split Mode (Remote Dev),” is open and has no comments. The split is a real prerequisite for correct Remote Dev detached-frame support, not merely Gradle cleanup.

The current JetBrains plugin is one classic module: `editors/jetbrains/settings.gradle.kts` declares no subprojects, and `plugin.xml` has classic dependencies rather than modular `<content>` entries. It consequently mixes frontend UI/editor observation with backend project, filesystem, terminal, native-library, CRDT, and agent-doc process work. The decisive limitation is that a Remote Dev backend does not have the client’s `EditorWindow` topology, so backend-only `LayoutChangeDetector` evidence cannot reliably distinguish main-window and detached-frame views before path deduplication.

#### Usability consequences

Benefits:

- A frontend observer can report the real client frame/window topology, enabling correct main-window precedence, detached placeholders, separate terminal views, and fail-closed handling of incomplete evidence.
- Latency-sensitive observation and presentation can run on the client while project-model, filesystem, terminal, native-library, and binary orchestration remain backend-local.
- Monolithic IDE users need not lose functionality: JetBrains loads frontend and backend modules together outside Split Mode.

Costs and rough edges:

- Installation becomes two-sided in Remote Dev. Marketplace distribution can place compatible modules on both sides, but local ZIP/custom-repository installs require explicit backend and Client/Gateway handling.
- One-sided installation, RPC negotiation failure, reconnect, stale generations, or serialization incompatibility must fail closed. That is safer, but users may temporarily see unavailable/pending placeholder states.
- Splitting modules alone does not make every existing UI surface frontend-native. If actions, status widgets, and notifications stay in the backend module, they retain the backend-rendering latency and lifecycle behavior JetBrains warns about.
- Support can fragment by IDE line. Draft [PR #222](https://github.com/btakita/agent-doc/pull/222) keeps a classic artifact for 242–261 and clamps the modular artifact to `262.*`, while #221 reports IDEA `263.6259.32`. As drafted, it therefore does not solve the reported environment.

#### Maintenance consequences

Benefits:

- The boundary becomes explicit and testable: serializable DTO/RPC contracts in shared, client observation/presentation in frontend, and project/native/process work in backend.
- Most existing implementation can initially remain backend-side, limiting logic duplication.
- Separate classloaders expose accidental cross-side imports earlier and make platform-specific responsibilities easier to audit.

Costs:

- The release matrix expands to classic and modular build stacks, non-overlapping IDE ranges, coordinated versions under one plugin ID, both-side packaging checks, Plugin Verifier, split-mode sandboxes, and real two-process Remote Dev acceptance tests.
- Shared RPC becomes a compatibility surface. Contract evolution must tolerate frontend/backend skew, reconnects, suspend calls, and serialization changes.
- Dynamic unload/reload now spans multiple modules and classloaders, compounding the lifecycle/generation cleanup already handled by `PluginLifecycleListener`.
- PR #222’s reuse approach is pragmatic but brittle: its backend build text-transforms `PluginLifecycleListener.kt` between markers and derives a modular descriptor from classic XML sections. Source-marker or descriptor-layout changes need dedicated regression checks.
- JetBrains’ modular Plugin Model v2 and RPC APIs are experimental, increasing platform-upgrade churn. The detached-view work also uses internal exact-version APIs because no public API targets a specific detached `EditorWindow`; that requires source fencing, runtime probes, strict build clamps, and per-IDE-line verification.

#### Recommendation

Proceed with the split architecture if detached Remote Dev frames are a product requirement; the backend-only design cannot satisfy that requirement safely. Keep the frontend module narrow, keep a small versioned shared contract, and leave project/native/process orchestration backend-side initially.

Treat dual classic/modular artifacts as a temporary compatibility bridge with an explicit retirement policy. Prefer Marketplace or a managed custom repository, document two-sided local installation, and detect/report one-sided installs clearly.

Do not consider #221 complete based on the current exact-262 draft for the stated 263 environment. Either target and prove 263, or explicitly declare 262 as the supported line and reconcile the issue’s setup. Keep internal UI APIs isolated and version-clamped; broaden a compatibility range only after Plugin Verifier and real backend/Client acceptance both pass.

This cycle was architectural analysis only; no repository implementation or test/build work was required. The exchange remains oversized, but the active queue was not stalled for compaction.

### Re: IDEA frontend and backend — gpt-5 · 2026-10-09T11:54-04:00

> **Queue prompt:**
>
> do [#backendfrontendidea]


In JetBrains Remote Development, **frontend** and **backend** are two halves of IntelliJ IDEA itself. They do not mean the frontend and backend of the application you are developing.

- The **frontend** is the JetBrains Client process, normally running on the developer’s laptop. It owns what the user directly sees and manipulates: IDE windows, editor frames and splits, detached windows, focus, dialogs, and other latency-sensitive UI.
- The **backend** is an IntelliJ IDEA process running beside the project, normally on the remote host, container, or cloud machine. It owns the files, project model, PSI, indexes, code analysis, builds, run/debug processes, terminals, and other project-local work.
- **JetBrains Gateway** starts or connects the two processes. Gateway is the launcher/connection manager, not the plugin frontend.
- A plugin’s **shared module** defines the small serializable contract understood by both sides: DTOs, RPC interfaces, acknowledgements, and remote topics. It is not a third service and should not become a general-purpose logic module.

For Agent Doc, a sensible division is:

- **Frontend:** observe the actual JetBrains Client frames, distinguish the main window from detached “Open in New Window” frames, collect each frame’s visible document, and place a terminal or read-only placeholder into the intended client editor window.
- **Shared:** define surface snapshots, client/surface identities and generations, leases, presentation commands and receipts, and the RPC interface carrying them.
- **Backend:** authenticate the connection, resolve the real backend `Project`, validate and fence snapshots, pass them to Agent Doc’s project controller, and perform filesystem, process, native-library, Agent Doc, and tmux work. It sends the resulting presentation decision back to the frontend.

The split matters because the backend can know that a file is open or selected without owning the client’s actual `EditorWindow` or detached top-level frame. The frontend sees that window topology, but it should not directly manipulate the remote filesystem, run the Agent Doc binary, or own tmux policy.

This is unlike a web browser talking to a business API. Both halves are IntelliJ Platform processes that together present one IDE session; RPC is the transport across that IDE process boundary.

In an ordinary local, monolithic IDEA session, there is only one IDE process. The frontend- and backend-qualified plugin modules both load into that process, and shared RPC executes in-process rather than over a remote connection. The user still experiences one conventional plugin; the modular split mainly provides clean responsibility boundaries.

JetBrains documents the model in [Split Mode](https://plugins.jetbrains.com/docs/intellij/split-mode-and-remote-development.html), [Modular Plugins](https://plugins.jetbrains.com/docs/intellij/modular-plugins.html), and [RPC](https://plugins.jetbrains.com/docs/intellij/remote-procedure-calls.html). One relevant caveat remains: Plugin Model v2 is experimental, and Agent Doc’s draft PR #222 currently targets IDEA `262.*`, while issue #221 reports IDEA 263.

This was an explanatory task only, so no repository code, tests, build, or installation were needed. The exchange remains oversized, but the active queue was not stalled for compaction.

### Re: do [#singleplugindistribution] — gpt-6.1-sol · 2026-10-09T13:26:07-04:00

> **Queue prompt:**
>
> do [#singleplugindistribution]


Yes. JetBrains supports one modular plugin ZIP whose contents are selected by IDE role, and that contract is now implemented and verified on draft PR #222 at commit `254b75e68b9bbf974b95f5f5982e63f55657e13f`.

The exact-262 artifact contains all three modules:

- `shared` always loads.
- `frontend` declares `required-if-available="intellij.platform.frontend"`.
- `backend` declares `required-if-available="intellij.platform.backend"`.
- A normal monolithic IDE exposes both capabilities, so all three load from the same ZIP.
- Remote Dev installs the same ZIP in Client/Gateway and backend; each process activates its applicable module set.

The change adds executable role-matrix verification for frontend-only, backend-only, and monolithic environments; proves both split sandboxes receive the complete artifact; wires the modular build into `make check`, CI, release packaging, and plugin-version fencing; and makes classic JetBrains asset lookup reject the similarly prefixed modular ZIP.

Verification completed:

- Modular Gradle tests, ZIP build, descriptor verification, and split-sandbox checks passed.
- Rust plugin/asset-selection tests and release-package assertions passed.
- The Rust suite passed 9,836 tests with 141 skipped; editor parity, JetBrains 262 checks, Python bootstrap, Lean, and all TLA+ models passed.
- The coordinator-owned `make install` completed, including live classic JetBrains and VS Code package convergence; the targeted `make audit-docs` remedy then passed.
- Draft PR #222 was fast-forwarded to the implementation commit. CI run `37966158420` for that exact head is in progress; local proof is complete, so this closeout does not wait on it.

The modular artifact remains deliberately exact-262. PR #222 stays draft until the existing #218 acceptance gate is proven on real IDEA 2026.2.3 Remote Dev with the same ZIP installed on both sides; this does not claim 263 support. The outer superproject must also advance its `tmux-router` gitlink to `f4acd6d` or later before adopting the PR lineage.

During this cycle I also dispatched the newly added GH #223, GH #218, GH #224, and release-audit queue work to separate claimed worktrees. GH #223 is complete and pushed at `25c10e3c6a2ce12d720ab9af0b461fa97b18e805`; GH #218 was verified as already implemented by PR #222 and remains operator-gated; GH #224 is still in flight. The release audit found the batch is not yet releasable while those fixes and the Remote Dev proof remain open, and Homebrew publishing additionally lacks `HOMEBREW_TAP_TOKEN`.

The exchange remains above the size warning threshold, but the active queue was not stalled for compaction.

### Re: #gh-fix https://github.com/btakita/agent-doc/issues/223 — gpt-6.1-sol · 2026-10-09T13:32:19-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/223

Fixed and closed.

Commit `25c10e3c6a2ce12d720ab9af0b461fa97b18e805` is now fast-forwarded onto `main`. Retained persistence validates exact disk byte length plus SHA-256 against the immutable delivery epoch and treats that proof as a terminal durability receipt when no native-save route exists; missing or stale disk remains on the existing retry path.

Regression coverage passed for exact-disk settlement and stale/missing-disk rejection. The worker's authoritative `make check` passed, the coordinator-owned local `make install` completed successfully, the independent closeout review found no unrelated changes, and GitHub issue #223 was closed with the commit evidence.

### Re: #gh-fix https://github.com/btakita/agent-doc/issues/218 — gpt-6.1-sol · 2026-10-09T13:34:47-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/218

The code, tests, and one-ZIP role-selecting distribution are complete in draft PR #222 at `254b75e68`, but issue #218 intentionally remains open and the PR remains draft/unmerged. Fresh CI for that exact head and the real IDEA 2026.2.3 / branch 262 Remote Dev acceptance matrix are still required; this closes only the queue dispatch, not the GitHub issue.

A durable `#gh218remoteproof` operator-review gate now records the exact two-sided acceptance matrix and prevents the implementation from being mistaken for a completed live proof.
<!-- agent:boundary:d79eeb00:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#singleplugindistribution]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/223~~
- #gh-fix https://github.com/btakita/agent-doc/issues/218
- #gh-fix https://github.com/btakita/agent-doc/issues/224
- release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gh218remoteproof] [operator-verify] GH #218 / draft PR #222: require green CI for head 254b75e68 and real IDEA 2026.2.3 (262) Remote Dev proof with the same modular ZIP installed on backend and Client/Gateway: detached-only isolated terminal; same-document main-wins read-only placeholder and byte-identical main; focus/sync/resize no main mutation; close/rejoin; backend restart/client reconnect before first main publication; two simultaneous thin clients; capability loss/one-sided install fails closed. Only then mark PR ready, merge, and close #218.
- [/] [#fixsubagent] Fix in a subagent:
  ```
  • Blocked by hook
  └ agent-doc Stop hook intercepted an unfinished document cycle for /home/brian/work/btakita/agent-loop/src/haiven-
  dev/tasks/frontend.md. [session-check] INTERRUPTED: binary-owned captured closeout for `/home/brian/work/btakita/agent-
  loop/src/haiven-dev/tasks/frontend.md` remains pending: response_captured: captured response is waiting for exact
  authority/disk convergence before write-applied. The same capture is durable; do not issue another closeout payload and
  do not force disk. Retry only `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
  frontend.md`. The latest assistant text was captured into the pending/capture ledger before auto-close. The hook could
  not finish the required commit boundary: refused editor IPC for /home/brian/work/btakita/agent-loop/src/haiven-dev/
  tasks/frontend.md because reliable document liveness has no matching editor registration (reliable_editor_live=true,
  registration_available=false); refresh or update the editor integration before retrying closeout. The retained capture
  or projection is already durable and this exact retained intent commits itself once delivery converges — this is a
  deferral, not a lost response. Wait for its controller-owned terminal state edge; do not use the document's current-
  cycle status as proof because queue continuation can advance to a different cycle. Do NOT re-send the response, force
  disk, or `admin recycle`, which disturb the capture being awaited. Do not reach for `admin reload-lib` on your own
  either: `admin reload-lib` is the sanctioned recovery only when `agent-doc session-check` reports that the editor
  holding the document is not serving its replica, and then its remedy governs. Do not send the final answer yet. If the
  response is missing from the document, run `agent-doc repair /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/
  frontend.md` first. Then finish the commit boundary for this turn and end with `agent-doc session-check /home/brian/
  work/btakita/agent-loop/src/haiven-dev/tasks/frontend.md`.
  ```
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
- [/] [#qsubattr5] [agent-doc] Queue subagents attribute, phase 5 (plan: tasks/agent-doc/plan-queue-subagents-attribute.md). SETUP COMPLETE: tasks/software/tsift.md now has subagents=3 on branch qsubattr5-dogfood commit de5a8aa7c0d5e879547d4e321d159b1eff840073; gated pending the next real operator-created multi-head batch proof from distinct queue claims and ops.log that every eligible head dispatches exactly once and none drain inline.
- [/] [#steergatemodelrerun] [agent-doc] Rerun the steergatenamo model evaluation on real pause points. Gate: at least 200 labelled steering_gate_log rows with a text_tail (66 as of 2026-10-06); src/agent-doc/scripts/steergate-model-eval --dataset currently exits insufficient_data. Then integrate turnsense locally (ONNX via ort, lazily downloaded, opt-in) only if rules+perceptron+turnsense beats rules+perceptron on held-out log-loss; if close but short, fine-tune on the logged rows. Proxy results and decision: tasks/agent-doc/eval-steergatenamo.md
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
