---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260624-043637.md`*

Compacted content:
❯ - Archived 15 response topic(s): state backbone queue closeout - gpt-5; state backbone implementation; lazily-rs state machines; 12 more
❯ - Prior summary/context: prior compacted content: Archived 4 response topic(s): response overwrite sidecar recovery - gpt-5; #advr02 - gpt-5; ...

### Re: #advr11 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr11]


Implemented and verified `#docdriftgrace` for the planned stale-snapshot drift case.

Changes:
- Added component-scoped stale snapshot drift classification before the old size-ratio reset guard fails closed.
- Auto-rebases safe visible drift by refreshing snapshot and CRDT sidecars when drift is limited to historical `### Re:` response-block trimming, allowed frontmatter agent changes, and turn-independent component changes such as sibling queue bookkeeping.
- Preserves fail-closed behavior for active capture response loss and active queue-driver edits.
- Reloads refreshed snapshots in commit and stream paths after a safe rebase.
- Added regression coverage for safe historical exchange trimming with sibling queue additions, and for blocking active queue-driver removal.
- Updated FlowCore token budgets and `VERSIONS.md` for the new audited safe-rebase log marker.

Verification:
- `cargo fmt`
- `cargo test -p agent-doc-orchestration stale_snapshot_reset_drift`
- `cargo test -p agent-doc-orchestration qpcwcmerge`
- `cargo test -p agent-doc-orchestration commit_blocks_stale_snapshot_commit_when_active_capture_response_missing`
- `cargo test -p agent-doc --test run_integration preflight_suppresses_owned_pane_self_invocation_for_independent_queue_edit`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted`
- `make check`
- `git -C src/agent-doc diff --check`
- `make install`
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success` (`28074608395`).

Commits:
- `src/agent-doc`: `d03d5113 fix(write): rebase safe stale snapshot drift`
- Parent repo: `289af6b66 chore: advance agent-doc submodule`

Closeout note: this live session document had also been compacted to a `### Session Summary`, which is outside the narrower planned historical-`### Re:` trim classifier. I used `agent-doc reset --from-current --preserve-session` before closeout so the compacted visible document, snapshot, and CRDT sidecars were aligned without dropping session metadata.

### Re: #advr12 — gpt-5

> **Queue prompt:**
>
> :pushpin: do [#advr12]


Confirmed `#hap7/#gf5n` covers the same-node partial free-text queue duplication shape. No production guard was needed.

I added a focused regression in `agent-doc-orchestration/src/merge.rs`: `crdt_merge_same_free_text_queue_node_extension_no_truncated_duplicate`. It models a snapshot containing a partial free-text queue item while the operator completes that same queue node and the agent appends exchange content. The merge keeps the completed operator queue line exactly once and does not retain the truncated partial sibling.

Verification:
- `cargo fmt`
- `cargo test -p agent-doc-orchestration crdt_merge_same_free_text_queue_node_extension_no_truncated_duplicate -- --test-threads=2`
- `cargo test -p agent-doc-orchestration crdt_merge -- --test-threads=2`
- `make check`
- `git -C src/agent-doc diff --check`
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success` (`28074608395`).

Commits:
- `src/agent-doc`: `1223ebb8 test(merge): cover partial free-text queue extension`
- Parent repo: `d7fc19723 chore: advance agent-doc submodule`
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success` (`28074608395`).
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.PatchGenerationFenceTest` passed.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.NormalizeExchangePrefixesTest` passed.
- `cargo test -p agent-doc-orchestration plugin_owner -- --test-threads=2` passed: 8 tests, including single-owner election, stale/dead owner handoff, and release ownership.
- `make install` rebuilt the binary/cdylib after the stale-install warning.
- `NativeLib.kt` exposes the FFI methods to Kotlin.
- `PatchWatcher.ownsDocument()` calls that FFI lease primitive; untargeted/broadcast patch files defer when this window is not the owner, while editor-targeted patches bypass the gate; `dispose()` releases owned documents so another live window can take over.
- `src/ffi.rs` exports `agent_doc_plugin_owner_try_acquire` and `agent_doc_plugin_owner_release` for the JetBrains plugin.
- `agent-doc-orchestration/src/plugin_owner.rs` provides the per-document `.agent-doc/plugin-owner/<hash>.json` lease, 30s TTL, live-pid self-heal, owner refresh, and owner-only release.
- The original plan `tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md` records Phase 1 `#8bfz` as shipped in `237f5fb2`.

### Re: #advr13 — gpt-5

Confirmed `#8bfz` is already implemented in the current tree; no new source change was needed.

Proof points:
- The original plan `tasks/agent-doc/plan-supervisor-direct-queue-edit-race.md` records Phase 1 `#8bfz` as shipped in `237f5fb2`.
- `agent-doc-orchestration/src/plugin_owner.rs` provides the per-document `.agent-doc/plugin-owner/<hash>.json` lease, 30s TTL, live-pid self-heal, owner refresh, and owner-only release.
- `src/ffi.rs` exports `agent_doc_plugin_owner_try_acquire` and `agent_doc_plugin_owner_release` for the JetBrains plugin.
- `PatchWatcher.ownsDocument()` calls that FFI lease primitive; untargeted/broadcast patch files defer when this window is not the owner, while editor-targeted patches bypass the gate; `dispose()` releases owned documents so another live window can take over.
- `NativeLib.kt` exposes the FFI methods to Kotlin.

Verification:
- `make install` rebuilt the binary/cdylib after the stale-install warning.
- `cargo test -p agent-doc-orchestration plugin_owner -- --test-threads=2` passed: 8 tests, including single-owner election, stale/dead owner handoff, and release ownership.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.NormalizeExchangePrefixesTest` passed.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests com.github.btakita.agentdoc.PatchGenerationFenceTest` passed.
- `gh run list --workflow CI --limit 1` reported latest `CI` as `completed success` (`28074608395`).

Commits: none in this cycle; implementation was already present, and `src/agent-doc` is clean.
<!-- agent:boundary:52e8c2bc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#advr11]~~
- ~~:pushpin: do [#advr12]~~
- :pushpin: do [#advr13]
- :pushpin: do [#advr14]
- :pushpin: do [#advr15]
- :pushpin: do [#advr16]
- :pushpin: do [#advr17]
- :pushpin: do [#advr18]
- :pushpin: do [#advr19]
- :pushpin: JB `Run Agent Doc` still does not quickly submit the prompt.
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#advr14] Advance original #qpauseux: finish paused-queue plugin UX with info/action notifications, Resume or restart-supervisor/resume actions, no raw error toast, plugin artifacts, and render/ops proof.
- [ ] [#advr15] Advance original #jbdisprecycle: convert recycle-boundary Run Agent Doc verification into deterministic proof for exactly one `dispatch_start_proof` with submit and no stranded start_session failure; ops marker `route_dispatch_submit_recycle_settle`.
- [ ] [#advr16] Advance original #tb4q: complete #qfocsup focused-cycle supervisor clear-and-continue proof with install/recycle if needed, `session-check ui_outcome=deferred_for_supervisor_drain`, `context_reset`, `go_drain_dispatch`, and fresh-agent drain evidence.
- [ ] [#advr17] Advance original #fbwireverify: complete #fbwire/#fullboundary acceptance with slow-ACK no inflight pileup and wedged no-ACK bounded timeout + ERROR marker + playback artifact; add deterministic tmux-ci/sim coverage where possible.
- [ ] [#advr18] Advance original #qnodemerge4verify: after JB/VS Code plugin install, live-test non-ASCII edit-during-merge around `café 日本 😀`; prove accepted byte-offset editor ops and no mojibake/interleaving, plus automated log checker for `editor_op_recorded` + `editor_ops_for_base`.
- [ ] [#advr19] Advance original #cyh0: identify and instrument remaining File Cache Conflict trigger by capturing exact action, agent-doc command, and ops marker such as `write_authority action=... transport=disk`; distinguish ack-content, VCS-refresh/save, and editor-IPC convergence surfaces.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
