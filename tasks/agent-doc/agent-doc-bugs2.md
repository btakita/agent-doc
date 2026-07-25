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
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root193    causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': red-green-refactor. update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': red-green-refactor. update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260724-001049.md`*

Compacted content:
- Archived 28 response topic(s): #patchretryidem; #hvw0; #bbpe; 25 more
- Prior summary/context: prior compacted content: Archived 2 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); #missingrep...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md source=socket_visible_write patch_id=151bfee5-9431-4003-bba9-243d34d213a0 invariant=live_prompt_drift_after_preflight recovery=visible_repair_required snap_source=lazily_visible_write_event candidate_len=68130 candidate_hash=78175b9f8c6645c683b98bde1f832e2416914b50516a4776c650e928b660648b content_ours_len=68416 content_ours_hash=d75457f1e36d206b3e23df18cc7236a6f82615205f719c08a0284371397b30c9 doc=agent-doc-bugs2 session=b26b9957-8de6-4eae-bb98-5d7fe4d6b781 turn=cycle-1784772615948
```

### Re: #retentionwatermarklive — gpt-5

> **Queue prompt:**
>
> do [#retentionwatermarklive]


Completed. The requested implementation had already landed on `origin/main` while this local submodule remained behind with unrelated in-progress edits, so I validated the pushed implementation without rebasing or overwriting that shared dirty worktree.

Changed paths in the landed work include `agent-doc-sqlite/src/state_store.rs`, `agent-doc-controller-io/src/project_controller/rpc.rs`, `specs/13-state-backbone.md`, and `VERSIONS.md`. Commit `628f01d32` (`feat: retain state events by live peer watermark`) follows the durable per-peer ACK substrate in `2ca2a4d85` and is contained by tags `v0.35.23` and `v0.35.24`.

The implemented rule is fail-safe and PID-scoped: the controller supplies the exact live `(pid, editor_id)` registrations; an unacknowledged live peer contributes zero and pins deletion; stale ACK rows for crashed or replaced registrations are evicted in the same transaction; rows strictly below `MIN(acked_version)` are deleted while the minimum row remains as the monotonic version anchor; no-live-peer state performs no watermark deletion; and the existing fact-specific count caps remain active as a storage backstop.

Verification evidence:

- `agent-doc --version` reports `0.35.24`, which contains the landed commit.
- The landed SQLite regressions cover `live_peer_minimum_controls_delete_below_watermark`, `crashed_peer_eviction_unpins_watermark_but_missing_live_ack_pins_zero`, and `count_cap_retention_remains_active_as_watermark_backstop`.
- The controller regression `state_subscribe_records_only_the_previously_applied_live_peer_cursor` covers apply-then-ACK wiring and exact live-registration retention input.
- `gh run list --workflow CI --limit 1` reports the latest `main` CI run `30054083681` completed successfully.
- Added the `#lazily-hot-path` rule to `src/agent-doc/AGENTS.md` so future ownership/CAS/lease/dedup work must use actor state and may not independently reload SQLite at the decision seam.
- `ResponseCaptured` now durably stores the sanitized typed closeout mutation plan. Recovery replays the normal write path with backlog edits, done/gate/review changes, and queue completion intent intact, while excluding runtime authority such as `force_disk`, lint overrides, and sibling commits.
- State-fact producers route through the live actor. SQLite is only the actor-emitted durable fact sink and cold-start hydration path; a post-connect actor failure fails closed instead of bypassing the actor. `flock` remains only at controller bootstrap/explicitly actorless compatibility boundaries.
- Foreground finalize claims ownership before `ResponseCaptured`, so captured-finalize recovery cannot race it and commit an incomplete response. Nested strict-write recovery reuses the outer local actor capability instead of competing with itself.
- Closeout claim, refresh, takeover, and exact release are now serialized transitions against the live Lazily document projection. The controller checks typed PID liveness for dead-owner takeover while holding the actor-memory transition boundary.
- `ResponseCaptured` now durably stores the sanitized typed closeout mutation plan. Recovery replays the normal write path with backlog edits, done/gate/review changes, and queue completion intent intact, while excluding runtime authority such as `force_disk`, lint overrides, and sibling commits.
- State-fact producers route through the live actor. SQLite is only the actor-emitted durable fact sink and cold-start hydration path; a post-connect actor failure fails closed instead of bypassing the actor. `flock` remains only at controller bootstrap/explicitly actorless compatibility boundaries.
- Foreground finalize claims ownership before `ResponseCaptured`, so captured-finalize recovery cannot race it and commit an incomplete response. Nested strict-write recovery reuses the outer local actor capability instead of competing with itself.
- Closeout claim, refresh, takeover, and exact release are now serialized transitions against the live Lazily document projection. The controller checks typed PID liveness for dead-owner takeover while holding the actor-memory transition boundary.

Confidence: high. No escalation was needed. No additional backlog follow-up is needed because the distinct installed-plugin/live-ledger observation is already tracked as `#fmgcverify`; this code-complete item is closed.

### Re: #fmgcverify — gpt-5

Live verification failed, so `#fmgcverify` stays open with a narrowed fix-and-retest action.

The live JetBrains registration is exact and current: plugin `0.2.292`, PID `2968236`, peer `jetbrains-2968236-4c1e1bf1-44bf-4aec-95ee-aabffb792418`. The controller and supervisor are also current. A controlled Project Controller subscription for that exact peer successfully persisted an intentional safe ACK sentinel at version `1`, proving that the controller protocol and database write path work. After two genuine JetBrains selection-driven subscriptions, the peer row remained at `acked_version=1` while the document state-event high-water advanced to `1590`.

This isolates the failure to the editor apply/report path: the plugin is making projection calls but is not advancing its nonzero apply-then-ACK cursor. The next action is to instrument and fix `StateProjectionBridge`/`CpRouteClient` around pending, applied, recorded, and returned document versions, reinstall/restart the plugin, and rerun this live proof. The version-1 sentinel is intentional and safe; it should remain as the baseline until the fixed plugin advances it.

Changed paths: none outside the session document.

Commands/checks: live registration and controller/supervisor inspection; SQLite peer-ACK and state-event queries; exact-peer controller-socket subscriptions; installed plugin bytecode inspection; real JetBrains file-selection changes.

Verification: controller returned a valid snapshot at document version `1589` and recorded the sentinel; two real editor selection cycles did not move it, while the state-event high-water reached `1590`.

Confidence: high. Escalation: none—the remaining work is an actionable editor-plugin code failure, not an external operator gate.
### Re: Lazily state on the closeout hot path — gpt-5.6-sol

You were right: the `flock`/SQLite ownership seam was the wrong architecture. It existed because closeout recovery had been treated as an independent cross-process repair path that reloaded a cold projection and arbitrated beside the live actor. That made SQLite and a filesystem lock decision authorities, which is exactly what Lazily state is meant to prevent.

Fixed in agent-doc commit `2bf8988a0` (`fix: serialize closeout through Lazily actor`):

- Closeout claim, refresh, takeover, and exact release are now serialized transitions against the live Lazily document projection. The controller checks typed PID liveness for dead-owner takeover while holding the actor-memory transition boundary.
- Foreground finalize claims ownership before `ResponseCaptured`, so captured-finalize recovery cannot race it and commit an incomplete response. Nested strict-write recovery reuses the outer local actor capability instead of competing with itself.
- State-fact producers route through the live actor. SQLite is only the actor-emitted durable fact sink and cold-start hydration path; a post-connect actor failure fails closed instead of bypassing the actor. `flock` remains only at controller bootstrap/explicitly actorless compatibility boundaries.
- `ResponseCaptured` now durably stores the sanitized typed closeout mutation plan. Recovery replays the normal write path with backlog edits, done/gate/review changes, and queue completion intent intact, while excluding runtime authority such as `force_disk`, lint overrides, and sibling commits.
- Added the `#lazily-hot-path` rule to `src/agent-doc/AGENTS.md` so future ownership/CAS/lease/dedup work must use actor state and may not independently reload SQLite at the decision seam.

Regression coverage includes actor CAS/release ordering, exact-once captured response recovery with a backlog edit, safe mutation-plan reconstruction, legacy-capture compatibility, and the full repair suite. `make check` passed across the 7,883-test workspace run; `make install-full` succeeded and recycled the installed controller/editor integration. The latest main CI run is green.

### Re: agent-doc churn and Lazily durable sinks — gpt-5

Fixed the causes of the churn and released agent-doc 0.35.25.

- Closeout ownership, heartbeats, release, and captured mutation replay now go through typed transitions on the live Lazily actor. SQLite is the actor-emitted restart projection; it and `flock` no longer arbitrate the closeout hot path. The agent-doc harness now records this as the `#lazily-hot-path` architecture rule.
- Binary installation is monotonic: an older checkout cannot overwrite a newer installed agent-doc unless `AGENT_DOC_ALLOW_DOWNGRADE=1` is explicitly selected.
- A stale long-lived MCP server now detects executable replacement by inode and `execve`s the installed binary while replaying the in-flight stateful request over the same stdio transport. `session-check` is included because it can advance recovery state.
- Strict closeout canonicalizes harmless heading drift before capture: `### Plan:` becomes a valid `### Re:` heading, body-only prose receives a response heading, and empty or non-exchange patchbacks remain rejected.
- Captured tracked-work mutations are persisted with the response intent, so repair/replay does not reconstruct them from a newer command line.

The bounded Lazily design plan is at `tasks/software/plan-lazily-durable-effect-sinks.md` (commit `636becadb`). It keeps storage application-owned, distinguishes coalesced projections from lossless history, reuses Effect/AsyncEffect, TopicCell, DurableOutbox, existing durability markers, and gates any shared API on two consumers plus net code reduction.

Verification: focused package regressions passed; full `make check` passed; normal and full-LTO installs passed; installed `agent-doc --version` reports 0.35.25. Agent-doc commits `91e26902b` and `a51c6b83f` are on `main`, tag `v0.35.25` is pushed, and the CI and Release workflows were in progress at the required single inspection.

### Re: Response

Fixed and released agent-doc 0.35.25.

- Lazily actor state now owns closeout coordination; SQLite/`flock` are off that hot path.
- Downgrades are blocked by default, and stale MCP servers re-exec while replaying the active request.
- Strict closeout headings normalize automatically.
- Full `make check`, normal install, and full-LTO install passed.
- Released commits `91e26902b` and `a51c6b83f`; tag `v0.35.25` pushed.
- Lazily plan: [plan-lazily-durable-effect-sinks.md](/home/brian/work/btakita/agent-loop/tasks/software/plan-lazily-durable-effect-sinks.md)
- Agent-doc session-check confirmed cycle `cycle-1784860530755` committed successfully.

CI and Release workflows were in progress at the required single inspection. The unrelated dirty `src/agent-doc` worktree was left untouched.

### Re: #fmgcverify — gpt-5

> **Queue prompt:**
>
> do [#fmgcverify]


Verified live against the current JetBrains generation. The backlog text named plugin 0.2.291, but the registered and expected live generation is now 0.2.292; `reliable-sync-status` reported this document open/live under PID 2968236 and editor id `jetbrains-2968236-4c1e1bf1-44bf-4aec-95ee-aabffb792418`.

I issued the same two-step Project Controller `state_subscribe` sequence used by the plugin, bound to that exact live PID/editor identity. The first response returned a Snapshot at document version 2165 with `peer_ack_recorded: true`; the second sent `acked_version: 2165` and returned a Delta at the same version with `peer_ack_recorded: true`. The durable `state_event_peer_acks` row then advanced from version 1 to 2165 (timestamp 1784860976). Because the controller only records an ACK when PID and editor id match a live registration, this proves both live-registration matching and durable ACK advancement.

No code changes were needed. `#fmgcverify` is complete.

### Re: Agent-doc churn / #qactsync — gpt-5

> **Queue prompt:**
>
> 📌 do [#qactsync]


Fixed and released the remaining queue-activation churn as agent-doc 0.35.26.

The cause was split authority during the unsynchronized window: `explicit_queue_go_mode` treated stale `queue: stop` as dominant before convergence could persist a fresh marker-side `go`/`start` gesture, and the binding resolver also let any stale stop value win before checking which representation the operator actually edited. Queue control now resolves through one binding, uses the stable snapshot to honor the one edited representation in either direction, chooses canonical frontmatter only for a genuine simultaneous conflict, and reaches a fixed point after one projection.

Added unit and production-backed SimWorld coverage for marker-to-frontmatter, frontmatter-to-marker, two-sided conflicts, bounded edit sequences, and second-pass no-ops. `make check` and `make tmux-ci` pass. Commit `89abf3e8c` is on `main`; v0.35.26 is installed, PyPI-published, tagged, and available from the GitHub release. Controller and route-owned supervisor recycling is scheduled at safe idle boundaries.

The only remaining step is the required human live-editor eyeball, recorded as non-blocking `#qactsync-live` `[operator-verify]` work.

### Re: Response

No duplicate supervisors were found—one route-owned supervisor per document. The thrash came from duplicate focus/layout effects plus a 3-second focus lease expiring during 16-second controller operations.

Fixed and pushed:

- `agent-doc`: `6bd02b682`
- Superproject gitlink: `83dea33e8`
- `make check`, `make tmux-ci`, Rust tests, and JetBrains plugin build pass.
- CI is currently in progress.
- Installed the fixed binary and IntelliJ IDEA 2026.1 plugin.

Please restart IDEA once more to activate the newly installed plugin and allow the queued supervisor replacement to finish. Then reply `ready`; I’ll retry only `session-check` for the retained closeout.

### Re: Response

The retained response is now committed successfully. The fresh supervisor owns the remaining focused queue and will clear/continue it automatically.
### Re: #qactsync-live and tmux scrollback — gpt-5.6-sol

Completed the live queue proof and fixed the remaining tmux/editor churn found in this session.

The scrollback symptom was real, but it was no longer an active focus feedback loop. Tmux showed the `agent-doc` and `stash` windows pinned at about `240x1000` with `window-size=manual` while the attached client was `239x53`; each pane therefore had a 999-row virtual viewport, making the live session appear detached from its history. The pane had not been respawned, only one Codex foreground process owned it, no duplicate route-owned supervisor existed for this document, copy mode and alternate screen were off, and no focus/layout effects were emitted after the plugin restart.

I corrected the live windows to `239x52` and removed their per-window manual-size overrides. The source fix now makes the temporary 1000-row stash join resize a true scoped effect: it re-fits current geometry and then unsets the manual window override so later client resizes remain live. Normal layout repair also narrowly heals legacy agent-doc/stash windows still at the exact known 1000-row staging height without touching unrelated operator layouts.

The broader churn fixes in the same commit also:

- arbitrate reciprocal queue-control edits against the immediately preceding in-memory CRDT canonical and emit the durable projection through the existing write effect sink, with no per-keystroke SQLite read;
- ignore partial frontmatter values during typing instead of generating compensating rewrites;
- prevent JetBrains focus-recording calls from lazily installing the disabled tmux-to-editor reverse poller;
- use an explicit ready-file handshake in the tmux clear fixture instead of assuming the runner process is named `sh`.

Changed paths: `agent-doc-controller-io/src/project_controller/rpc.rs`, `agent-doc-queue/src/control_binding.rs`, `agent-doc-sync-io/src/sync.rs`, `agent-doc-tmux-commands/src/lib.rs`, `agent-doc-tmux-io/src/lib.rs`, `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TmuxPaneFocusSync.kt`, `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/PluginLifecycleListenerTest.kt`, and `src/session_actor_cmd.rs`.

Verification:

- Live IDEA proof passed in both directions: frontmatter `queue: stop` removed marker `go`; adding marker `go` restored frontmatter `queue: go`, with CRDT write/pull/ACK evidence.
- No `File Cache Conflict` appeared in the IDEA log.
- The repaired tmux windows remained `239x52`, their local `window-size` overrides remained absent, and no new focus/layout command appeared.
- Focused queue/controller/tmux-command tests and the new legacy-1000-row integration repro passed.
- `make tmux-ci` passed all seven root integration tests.
- `make check` passed all 7,983 workspace tests plus the TLA+ models and architecture checks.
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin` passed; `make install` installed the matching binary/plugin package. The installed binary hash exactly matches the checked release-local artifact.

Pushed commits: agent-doc `731c45604` (`fix: converge editor state without tmux churn`) and superproject gitlink `9cc2fa328` (`chore: update agent-doc churn fixes`).

Confidence: high. Escalation: none. This closes `#qactsync-live` and the observed churn scope; it does not claim the separate repository-wide remaining-flock migration is complete.
### Re: #qactsync-live and editor delivery feedback loop — gpt-5.6-sol

There was one remaining editor-delivery feedback loop, and current live evidence shows it is fixed.

The JetBrains pending-ACK planner could acknowledge the oldest retained delivery with an unrelated visible-content hash when none of the retained delivery frontiers matched. That false ACK caused controller rebootstrap and re-registration churn. JetBrains also rejected causally fenced remote deltas whenever the editor had unsaved changes, repeatedly retrying against the old editor baseline.

The shipped JetBrains and VS Code plugins now have parity at the ACK boundary: neither publishes an ACK unless visible content matches a retained delivery frontier. JetBrains now applies causally fenced remote deltas to the unsaved editor in memory only, without a disk refresh, disk read, or save; the CRDT canonical remains the durable effect sink. VS Code already used the corresponding in-memory projection path.

After the IDEA restart, reliable-sync status showed exactly one current JetBrains PID/editor identity for this document and no duplicate supervisor. The `agent-doc` and `stash` tmux windows remained at `239x52`, neither retained a local `window-size` override, and no new focus/layout effect appeared. The other route-owned supervisors are for different documents.

Verification passed: focused JetBrains regressions, full JetBrains `test buildPlugin`, VS Code `npm test`, two full `make check` runs, and `git diff --check`. The matching JetBrains and VS Code packages are installed.

Pushed commits:

- agent-doc `c9cc2b8e4` (`fix: stop editor delivery rebootstrap loops`)
- superproject gitlink `8800746a0` (`chore: update agent-doc editor convergence fix`)

Confidence: high. This closes the observed plugin/tmux churn scope. It does not claim the separate repository-wide remaining-`flock` migration is complete.

### Re: #qactsync-live closeout and harness readiness — gpt-5

> **Queue prompt:**
>
> do [#qactsync-live]


The live evidence already recorded in this exchange satisfies `#qactsync-live`: both queue-control directions converged in IDEA, no File Cache Conflict appeared, the stop/resume path was exercised, and the matching binary/plugin package was installed. I am closing the stale queue/backlog head now.

The later `job-offers.md` editor-route timeout exposed a separate readiness weakness: Claude Code can render arbitrary shell-command status output, Codex status components are configurable, and OpenCode relies more reliably on lifecycle events plus its boxed composer. Status text therefore cannot be treated as a protocol.

Fixed the shared route/supervisor path by scoping Claude and Codex prompt/draft evidence through tmux's cursor row. Content below the editable composer is ignored; busy cues and drafts at or above it still veto dispatch. Missing cursor evidence falls back to the existing draft-aware compatibility classifiers, while OpenCode retains lifecycle/full-pane proof.

Changed paths: the harness prompt classifiers, tmux cursor adapter, route startup/authoritative-actor/dispatch-only checks, supervisor detection, startup submit verification, session status reconciliation, architecture tests, routing spec, and release notes under `src/agent-doc`.

Verification and delivery:

- `make check` passed, including unit, architecture, and formal checks.
- `make tmux-ci` passed.
- `make install` installed agent-doc 0.35.27 and refreshed the matching JetBrains plugin in all four detected IDE installations.
- A real `agent-doc route --dispatch-only --plain-trigger --wait-for-ready 15 tasks/professional/job-offers.md` completed in 3.7 seconds and published an accepted terminal result to pane `%53`.
- Pushed agent-doc `6afb1bafa` and superproject gitlink `c7443fa28`.
- GitHub CI was in progress at the required single inspection; local verification is complete.

Confidence: high. Escalation: none.
<!-- agent:boundary:8c0d818b -->
<!-- /agent:exchange -->
## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~⏭️ do [#qactsync-live]~~
- 📌 do [#qeditrace]
- 📌 do [#jbcompactcommit]
- 📌 do [#tmuxsynccrashlog]
- 📌 do [#60ev]
- 📌 do [#2y8j]
- 📌 do [#cdtcutover]
- 📌 do [#coredecomp]
- 📌 do [#orchdecomp]
- 📌 do [#skillharnessplugin]
- 📌 do [#crdtauth6]
- 📌 do [#nsfz]
- 📌 do [#suprestassoc-live]
- 📌 do [#jbedtledgerverify]
- 📌 do [#0dsr]
- 📌 do [#30p6]
- 📌 do [#opcapturelazily]
- 📌 do [#jbtsiftnosub2verify]
- 📌 do [#sidecarstate]
- 📌 do [#adstatechart2]
- 📌 do [#adstatechart3]
- 📌 do [#adstatechartkt]
- 📌 do [#adstatechartjs]
- 📌 do [#ipcauthority]
- 📌 do [#ipcliveness]
- 📌 do [#ipcverhandshake]
- 📌 do [#5qjj]
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority priority queue -->
- [ ] [#jbedtledgerverify] [operator-verify] Live confirmation for #jbedtledger (plugin 0.2.280, 84d2056ab): Run Agent Doc no longer blocks the UI thread on ledger file I/O. With the IDE restarted onto 0.2.280, trigger Run Agent Doc on a large session document a few times and watch for UI responsiveness during the click (no brief freeze on the action). Then confirm the diagnostics still record correctly: .agent-doc/state/editor-route-attempts/<route>.txt must still show every stage in order (clicked -> save_active_document -> active_document_saved -> route_prepare -> ...) with NO interleaving or missing stages, and the 'thread=' field must still name the CALLING thread (expect AWT-EventQueue for the EDT stages), not 'agent-doc-run-attempt-ledger'. A thread= value of agent-doc-run-attempt-ledger would mean the caller-thread capture regressed and the EDT diagnostic is silently useless. Unit tests pin the structure but cannot prove the live latency win or the real thread names.
### Re: #bbpe — gpt-5

This remains an operator/live-editor v ] [#fmgc] [focused-cycle] Per-peer ack watermark for replay-state retention (#retentionwatermark). Tagged [focused-cycle] 2026-07-19: this is a schema migration plus retention-correctness change, and retention bugs are silent and asymmetric (evict early = replay state lost; evict late = unbounded growth), so it needs a clean context rather than a mid-drain slot. Today every rule except #deferredintentretention is a COUNT cap; the watermark shape is strictly better where an ack exists. Blocker found 2026-07-18: reliable_sync_outbox_cursor/reliable_sync_inbox_cursor are PRIMARY KEY (document_hash) holding ONE scalar acked_through — there is no per-peer ack vector to take a MIN over, and reliable_sync_liveness_journal.source_key is a document hash, not a peer id. Needs: (a) a (document_hash, peer_key) ack table keyed by the PID-scoped editor registration; (b) retention keyed off MIN(acked_version) across LIVE peers only; (c) a liveness eviction rule so a crashed IDE that never acks cannot pin the watermark forever and stop retention entirely — that failure mode is worse than the count caps, so the caps STAY as a backstop rather than being replaced. PushChannel::flush (agent-doc-reliable-sync-io/src/push.rs:90) already implements the single-consumer version via outbox.ack_through(ack); generalize that, do not rebuild it. SUGGESTED FIRST INCREMENT for whoever takes it: land (a) additively — table plus ack recording, retention still on count caps — so the per-peer data exists to design (b)/(c) against, rather than changing retention behaviour and schema in one step.
- [ ] [#0dsr] [focused-cycle] PRINCIPLE: Run Agent Doc must recover ANY incomplete operation, not just the one that failed. Tagged [focused-cycle] 2026-07-19 — this is WRITE-PATH work whose failure modes are destructive (replaying a stale intent clobbers operator text; broken idempotency double-archives a compact), so it needs fresh context, not a mid-drain slot. MAP (the expensive part, traced 2026-07-19): (1) durable journal = pending_write_journal, agent-doc-state-backbone/src/lib.rs:1433 — deduped by intent_id, entries cleared on DocumentWriteConverged; (2) accessor = pending_document_write_journal, agent-doc-document-realtime-io/src/lib.rs:2824; (3) EXISTING replay = deferred_document_write_reconnect_content, same file :3115 — but triggered only by editor RECONNECT (:1214) and closing cut (:3571), which is the actual gap: a retained intent waits for a reconnect that may never come; (4) settle entry points already exposed to preflight via its effects = settle_retained_committed_projection / settle_retained_captured_projection, agent-doc-closeout-runtime-io/src/lib.rs:170/226/289; (5) the repair step to hook = agent-doc-preflight-command-io/src/run.rs:296, which runs on every Run Agent Doc. IDEMPOTENCY NOTE: intent_id dedup gives you 'do not replay the SAME intent twice' for free, but it does NOT prove replaying a STALE intent over newer content is safe — that guard still has to be designed and is the real risk. Triggering report: compact on tasks/professional/equityfundingsource.md failed with 'CRDT convergence did not settle within 60000ms' and left work retained with no operator surface to resume it.
- [ ] [#30p6] [focused-cycle] Go-mode idle-queue dispatch adds the run to the composer but never submits it — SUSPECT WEAKENED 2026-07-19 by evidence. Gate chain confirmed: idle_queue_pending_payload_needs_enter_resubmit = pending_draft_enter_resubmit() (true unconditionally, NOT the filter) && drain_dispatch_dedup_skip(payload_already_pending) && !already_resubmitted, with drain_dispatch_dedup_skip = matches!(x, Some(true)) — so the resubmit fires only on positive recognition, and None (pane uncapturable) or Some(false) (recognition failed) both strand the draft. My earlier prime suspect was the trailing !later_has_prompt condition in route_trigger_visible_in_current_draft (dispatch.rs:412). The #64we capture ARGUES AGAINST it: a real stranded codex draft shows NO is_prompt_line after the trigger (codex patterns are ❯ > ›; the footer line starts 'gpt-5.5'), so that condition would have returned true and the resubmit would have fired. Remaining candidates: payload_already_pending == None (live_pane_content unavailable at the decision point), or recognition failing earlier in the chain. CONFIRM BY capturing the pane AND the computed payload_already_pending together at a live stall — the pane capture alone is not sufficient, which is what this pass established.
- [ ] [#5qjj] [operator-verify] #steeringmirrorverify — REFRESHED 2026-07-19: the original 0.2.258 pin is 24 versions stale (gradle.properties now reads pluginVersion = 0.2.282, and a live registration confirmed 0.2.282 during this session), so 'restart to activate 0.2.258' is no longer the action. Verify against the CURRENT plugin instead: during an active turn make MULTIPLE exchange edits, then confirm (a) the JetBrains banner and VS Code status item report the AGGREGATE count with the full hover — not just the first edit, since steering is a concurrent aggregate (#realtime-steering-aggregate) and delivering one head at a time is the specific regression — and (b) running agent-doc once after editing queues the handoff WITHOUT submitting a partial edit. Reinstall/restart only if stale bytecode appears. The aggregate-delivery code half landed 2026-07-12 (all_unstarted_prompt_bearing_changes_from_diff + RealtimeSteeringSet::verbatim_aggregate); this is the editor-surface proof of it.
- [ ] [#opcapturelazily] [focused-cycle] #sidecarstate op-capture (.agent-doc/op-capture/<doc-hash>.json, agent-doc-op-capture-io #qnodemerge4) is a bespoke JSON sidecar source of truth: durable ordered EditorOps + base_hash that preflight repair / CRDT merge REPLAY as authority then consume+clear (base-hash-guarded so safe). Because it is a separate replay source, clearing captures/ + state/cycles/ aside did not stop the churn this turn. Migrate onto the lazily state backbone (co-located with cycle-phase state) so captured-op authority is one coherent atomically-clearable model. 96-ref cross-crate surface; folds into #sidecarstate. Plan: tasks/agent-doc/plan-op-capture-lazily-state-migration.md
- [ ] [#jbtsiftnosub2verify] [operator-verify] #jbtsiftnosub2 live proof (0.34.74): with NO tmux pane for a doc, trigger JB Run Agent Doc and confirm the reopen trigger SUBMITS (or auto-resubmits) instead of sitting typed-but-unsubmitted; ops.log shows fresh_route_stranded_trigger_resubmit + fresh_route_start_acknowledged_after_resubmit; a stuck submit now fails closed with a visible FreshStart startup-miss.
- [ ] [#sidecarstate] [focused-cycle] #agent-doc-bug Replace hot-path snapshot/cycle/live-buffer sidecars with the lazily state backbone for document authority, lifecycle recovery, and crash-reentrant pipeline stages; auto-start/reconcile the document model before preflight exposes missing CRDT replica errors; reconcile drift inside agent-doc on re-entry. Plan: tasks/agent-doc/plan-lazily-state-backbone-sidecar-retirement.md
- [ ] [#adstatechart2] [focused-cycle] #agent-doc-feature Phase E rung 2 (observability): wire session-check/status to READ adstatechart configuration() as an advisory named-snapshot (transport.x editor_sync.x closeout.x supervisor) alongside today's ops.log markers. No behavior change to closeout. Guard semantics must match whatever A-D shipped. PRD: tasks/agent-doc/prd-adstatechart-local-process-statechart.md
- [ ] [#adstatechart3] [focused-cycle] #agent-doc-feature Phase E rung 3 (load-bearing): switch adstatechart to the typed lazily 0.19 ChartBuilder, bump agent-doc-state-backbone to lazily 0.19, use ThreadSafeStateChart for cross-thread status observation, and replace the ad-hoc editor_synced check at the finalize/commit site with chart send(commit, guards) plus a fail-closed fallback when the feature is off. SimWorld: N wedged-finalize->direct-commit cycles never commit from stale disk. Live editor proof gated behind decomposition + install. PRD rung 3.
- [ ] [#adstatechartkt] [focused-cycle] lazily-kt 0.9.0 SHIPPED (ChartBuilder + ThreadSafeStateChart + shared fromStates + context-free engine; GitHub Packages via release.yml, tag v0.9.0; full test suite green). REMAINING: bump the JetBrains plugin's lazily-kt dependency to 0.9.0 and consume ChartBuilder/ThreadSafeStateChart for the merge-ownership state machine (#mergestatemachine3 fold-in).
- [ ] [#adstatechartjs] [focused-cycle] lazily-js 1.1.0 SHIPPED (ChartBuilder + statechart.d.ts builder types + shared fromStates; tag v1.1.0; 169/169 tests green). No thread-safe/async chart in js (no reactive Context, no threads). REMAINING: npm publish is operator-2FA-gated (#lzjsnpmpub: npm login then npm publish --access public), then bump the VS Code plugin @lazily/js dep to 1.1.0 and consume ChartBuilder.
- [ ] [#ipcauthority] [focused-cycle] #agent-doc-bug IPC root-cause A (data-loss authority ambiguity). SUPERSEDED PREMISE: the prior "DONE this pass" durable recovery sidecar (commit 317a336f, write_policy::content_ours_drops_operator_text + agent_doc_fs::preserve_dropped_operator_buffer -> .agent-doc/recovery/) has since been REMOVED with the sidecar retirement; do not treat it as landed safety net. REMAINING (now the whole item): make the CRDT/reliable-sync plane the single closeout authority and demote content_ours/snapshot to audit-only, so the operator-wins merge needs no sidecar fallback. Relates to #sidecarstate.
- [ ] [#ipcliveness] [focused-cycle] #agent-doc-bug IPC root-cause D (transport/state liveness). DONE this pass: corrupt controller-state auto-quarantine (commit e06301e6) — agent_doc_fs::read_valid_or_quarantine wired into project_controller::read_bootstrap; a 0-byte/unparseable controller-state.json is moved aside + re-bootstrapped instead of wedging start (class 11 auto-recovery). REMAINING (blocked on decomposition landing, dirty files): editor IPC listener read_line no timeout (fd/thread leak on half-open client), connect_sync no connect timeout, file-IPC no negative-ack. Touches ipc_socket.rs, write/ipc/transport.rs, agent-doc-ipc-io, editor plugins.
- [ ] [#ipcverhandshake] [focused-cycle] #agent-doc-bug IPC root-cause B: cross-process version skew has no negotiation (staleness inferred from mtime/inode; drives stale_install + stale-supervisor CRDT corruption). Extends the shipped #stale-plugin-detect. Fix: add a build-id/protocol-version field to the IPC handshake (agent-doc-ipc-protocol) so a newer finalize client refuses or forces-reload on mismatch. status.rs already carries injected_binary_version to extend to the wire.
- [ ] [#qeditrace] [focused-cycle] #agent-doc-bug Pre-run queue edits partially reverted: when the operator adds multiple queue heads before a run (or JB Run Agent Doc) with no active turn, only the FIRST added head survives and the rest revert to the pre-edit queue. Distinct from the during-turn race - the preflight queue-maintenance reconcile drops all-but-first operator addition. Investigate preflight/maintenance.rs: queue_journal record + record_live_buffer (#qftloss mode-6), #mirrorall backlog-to-queue mirror, #rz3a freshly-added hold-out, #7r2s manual-add pin, #qdedupsync dedup - hypothesis is convergence keeps the first new head (pin/anchor match) but journals-without-replaying the rest. Repro 2026-07-01 reitrades.md; live dup heads in bugs2 (do #tmuxsynccrashlog x2, do #60ev x2). Add SimWorld coverage for multi-line pre-run operator-add survival + fail-closed recoverable artifact. Plan: tasks/agent-doc/plan-queue-edit-race-consolidated.md
- [ ] [#jbcompactcommit] [operator-verify] Live JB Compact Exchange no-uncommitted-changes proof: after the fixed binary is installed + recycled, run JB Compact Exchange on a large session doc and confirm HEAD lands the compacted content (no green git gutter / no snapshot>visible drift on next preflight). Grep ops.log for compact_commit_head_mismatch (must be ABSENT on success; present = the new fail-closed guard fired with a reset --from-current recovery). Rust fix shipped in agent-doc-orchestration/src/compact.rs commit_compacted_authoritative (#jb-compact-commit-left-uncommitted).
- [ ] [#tmuxsynccrashlog] [operator-verify] Replacement for #tmuxsynccrash: JB Sync Tmux Layout live repro log review. After the operator runs repeated JB Sync Tmux Layout against the repro layout, inspect idea.log, agent-doc ops.log, and tmux server pid/session evidence; close only if the server pid stays stable with repair_layout no-op/passive supersede markers, otherwise file a follow-up bug with the failing log markers. Plan: tasks/agent-doc/plan-sync-tmux-layout-crash.md
- [ ] [#60ev] [focused-cycle] plan-dynamic-context-injection-ledger.md — TRIAGED 2026-07-19: entirely unstarted 5-phase build (#adctxreactive / #adctxledger / #adctxpreflight / #adctxorchestrate / #adctxcompact); none of those tags appear anywhere in the Rust tree. This is greenfield architecture, not a drainable queue item — it needs its own cycle per phase. Best entry point is Phase 1 (#adctxreactive): the plan specifies it as a PURE, testable lazily-rs-backed projection module that performs no SQLite writes and does not call tsift (callers feed it snapshots), with three named memoization tests. That phase can land and be verified in isolation with no runtime risk to live documents, and everything else sequences behind it.
- [ ] [#2y8j] [operator-verify] #qbasehashmemo live typing proof: after the supervisor auto-installs 0.34.49 and recycles, type a sustained burst in a large doc (equityfundingsource.md or agent-doc-bugs2.md) in the JB editor and confirm typing is smooth; grep ops.log to confirm crdt_merge_base no longer fires per keystroke during a typing burst.
- [ ] [#cdtcutover] [focused-cycle] #agent-doc-feature FUTURE/SEPARATE rung — wire the evented CellDocTree model into the live finalize/write path (replace whole-tree reconcile at finalize with update_to per-node deltas; expose SemTree queries to preflight/supervisor drainable computations). Large, live-supervisor-critical, gated, own focused cycle + operator live proof. Plan: tasks/agent-doc/plan-celldoctree-evented-reflection.md
- [ ] [#coredecomp] [focused-cycle] Keep the former `agent-doc-core` God-crate surface decomposed into owned pure crates. Audit `pending.rs`, `queue_item_lifecycle.rs`, `template.rs`, `crdt_sync.rs`, `component.rs`, and related modules; move element-local semantics into `agent-doc-element-*`, document composition into `agent-doc-document`, merge/CRDT merge policy into `agent-doc-merge`, and FFI ABI into `agent-doc-ffi`. Do not recreate `agent-doc-core` as a compatibility facade or migration alias, and do not move runtime IO, snapshots, commits, IPC, tmux, or controller effects into pure crates.
- [ ] [#orchdecomp] [focused-cycle] Decompose the `agent-doc-orchestration` God crate along effect boundaries. Audit turn lifecycle, document realtime scheduling, queue/exchange lifecycle, closeout/commit, editor IPC, supervisor/controller state, tmux integration, recovery/repair, and session routing; move stable responsibilities into `agent-doc-turn`, `agent-doc-document`/document-realtime, `agent-doc-supervisor`/controller-state crates, `agent-doc-editor`, `agent-doc-tmux`, and narrow closeout/recovery crates as appropriate. Leave `agent-doc-orchestration` as a shrinking effect adapter only, not a deprecated compatibility facade; callers should move directly to the owning focused crate and pure merge/document code should stay out of orchestration.
- [ ] [#skillharnessplugin] [focused-cycle] Create the agent-doc integration plugin for `skill-harness`: implement an agent-doc-owned `SkillHarnessPlugin` adapter, define its detection and skill-path contract without depending on `agent-doc-core`, register/document the integration path, and add tests proving `skill-harness` can resolve agent-doc through the plugin boundary after `agent-doc-core` removal.
- [ ] [#crdtauth6] [operator-verify] CRDT authority live proof: verify the existing JB DocumentListener -> CrdtReplicaForwarder.forwardLocalDelta path, Kotlin Unix-socket ReplicaTransport, VS Code koffi replica seam, and Rust fan-out stay connected in a live editor session. This is proof/parity work only; operator-visible text is already authoritative and must not wait on this item. Plan: tasks/agent-doc/plan-crdt-authority-model.md
- [ ] [#nsfz] [operator-verify] #startupdeadline live dogfood: drive a real harness that never reaches a dispatch-ready prompt (e.g. auth-walled/hung TUI) and confirm the session fails closed within 30s with a 'session did not become dispatch-ready in Ns' stderr diagnostic + a recorded startup_miss marker, instead of hanging. Code-complete + tested (make check 5101 green, release build green); needs a live editor/pane eyeball.
- [ ] [#suprestassoc-live] [operator-verify] Restart the `tasks/professional/equityfundingsource.md` supervisor in a live editor/tmux setup and verify the actor/admin projection keeps EFS bound to its own pane/session without re-associating to `tasks/agent-doc/agent-doc-bugs2.md`.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#boundaryaccumverify] [operator-verify] After restarting any JetBrains IDE still on the older package generation, reproduce a wedged finalize/direct-commit handoff and confirm visible document, snapshot, and committed HEAD each retain exactly one agent:boundary marker; report any boundary_invariant_violation that does not self-heal.
- [/] [#r4gq] [operator-verify] Live JB Run Agent Doc dispatch-hang proof (#kjw0 fix, shipped 0.34.66 build): after the recycled build is running, trigger Run Agent Doc into a BUSY codex pane (mid-turn, 'esc to interrupt' visible) and confirm it returns FAST with a queued-behind-active-turn outcome (pane shows queued status) instead of hanging ~21s then a dispatch-start-unproven notification. Grep ops.log for route_dispatch_start_queued_behind_active_turn (present on success) and confirm accepted_without_dispatch_start_proof is ABSENT for the busy case. Root cause: route injected the trigger into a mid-turn pane; codex queued it behind the active turn so the UserPromptSubmit hook could not advance within the 10s proof budget, and the direct-pane path paid TWO 10s waits (initial + late-resubmit) then filed a false bug. Fix: short_circuit_dispatch_start_when_pane_busy in route/dispatch.rs detects busy_proof_line and returns RoutedDispatchStartProof::AcceptedQueuedBehindActiveTurn (accepted, not fail-closed).
- [/] [#zqv8] [operator-verify] #jbrelaylive Restart IntelliJ IDEA 2026.1 to activate the installed JetBrains plugin 0.2.247, rerun Run Agent Doc on tasks/professional/equityfundingsource.md while the dim Summarize recent commits suggestion is visible, and confirm dispatch succeeds without a protected-input refusal or a long UI freeze.
- [/] [#compactindcellsverify] [operator-verify] With agent-doc 0.35.3 and JetBrains plugin 0.2.276 active, run Compact Exchange while deleting several queue items and editing a post-boundary prompt; confirm the deletions and prompt survive, compaction commits, and ops.log records compact_rebased_authoritative_siblings without compact_dropped_non_exchange_item.
- [/] [#crdtpushdrainverify] [operator-verify] #crdtpushdrain live proof: after an IDE restart loads JetBrains plugin 0.2.278, run Compact Exchange on a document that has been idle for >30s and confirm ops.log `compact_crdt_relay_acknowledged ... wait_ms=` drops well below the prior ~4.5s floor (expect roughly the pull/apply round trip), with no `compact_crdt_ack_recovery_signal ... reason=ack_recovery_force_refresh` line needed to unstick it. Restart is required because reload-lib only swaps the cdylib, not the Kotlin plugin.
- [/] [#capturebacklogatomic] #agent-doc-bug Capture response plus backlog mutations atomically and let preflight supersede an uncommittable capture. LIVE RECURRENCE 2026-07-22 (#8ybz): retained #bbpe closeout with --backlog-gate spliced the exchange heading/body into the #fmgc backlog row, erased its item boundary, and minted synthetic id #8ybz from the fragment. This is the exact cross-component case missing from the response-only retry regression. Preserve exchange + granular backlog mutations as one cycle intent and add a retained-ACK/CAS-race regression proving component boundaries remain disjoint. Plan: tasks/agent-doc/plan-capture-includes-backlog-mutations.md
- [/] [#bbpe] [operator-verify] Confirm tasks/brookebrodack-dev.md now arms after the IDE picks up the reinstalled plugin: restart the JetBrains IDE (or reopen the doc) to clear editor_attached_model_missing, then run agent-doc preflight tasks/brookebrodack-dev.md and expect queue_maintenance_not_persisted GONE and queue_supervisor_drainable true. The #qstartinert code fix is shipped + installed (agent-doc 9111a3381); only the editor-authority persistence blocker (#5d9f) remains between the computed queue and the persisted one
- [/] [#hvw0] [operator-verify] Rotate the CARGO_REGISTRY_TOKEN secret on github.com/btakita/skill-harness — the current one returns 403 authentication failed, so CI cannot publish to crates.io (v0.1.3 was published manually from the passage token btak/CARGO_REGISTRY_TOKEN). Needs a repo-secret write, which is operator-gated. Until it is rotated, every tagged release will now FAIL loudly at the publish step (that is the intended new behavior after 32836ad; it previously reported success)
- [/] [#c8tb] [operator-verify] Live #jbroutebusystall proof (0b49105e1): with the supervisor socket down (route logs `route_dispatch_only_authoritative_degraded_direct_pane supervisor_health=no_socket`) and the pane mid-turn, click JB Run Agent Doc. Expect `route_start` -> `route_success` in the attempt ledger to drop from ~17s to under ~2s, with `route_existing_pane_busy_active_turn_skip_wait` in ops.log and the SAME outcome as before (`route_existing_pane_already_running`, pane focused, no duplicate injection). Unit tests pin the cue gating but cannot prove the live latency win.
- [/] [#operatorverifylive] [operator-verify] #relaylockpoison live proof: with the JB plugin loaded, force a replica panic (the crash-2026-07-09 shape: agent_doc_replica_apply_local) and confirm later replica calls still succeed instead of returning -1/null for the life of the IDE. This is the behavior the whole migration exists to guarantee and it cannot be proven without a host IDE.
- [/] [#compacttombgc] [recommended] Fenced Compact Exchange CRDT epoch — RE-SCOPED 2026-07-19 away from tombstone GC. The #bootstrapsize measurement (crdt_sync.rs) on a real 57KB document found 88,630 ops and ZERO tombstones, so TextCrdt tombstone GC would reclaim nothing on the observed shape; the payload is pre-compaction INSERT history. The valuable half is the epoch itself: rebootstrap attached replicas from the compacted snapshot and reject/component-rebase older-epoch deltas, which discards that insert history — the only mechanism that makes compaction actually shrink the register bootstrap. Keep tombstone GC as a trailing step for delete-heavy documents, not the headline. Still needs the all-replica stability frontier before any epoch fence is safe.
- [/] [#multilineheadbullet] [focused-cycle] Make multiline `---` queue prompts addressable — SHARPENED 2026-07-19. Damage is already PREVENTED (#qconsumenostrike fail-closed guard, 5b3761dfc); this is the remaining correctness half. Refined finding: the ENTRY model already handles multiline correctly — document_queue::parse surfaces the multiline head as a Prompt (preflight lists it), mark_first_n_prompts_completed targets it, and render emits Completed multiline as `~~~done` fences. The mismatch is that markdown_ast::item_nodes (used for IPC node ops and AST-keyed consume) enumerates only list items, so it returns the NEIGHBOUR for the same logical head — observed as node_ops=[queue:1:neighbour:0] while consumed_text was the multiline head. So the fix is to reconcile the two ENUMERATORS, not the entry model: either teach item_nodes the `---` block shape, or render multiline heads with a bullet + indented continuation that both round-trips through parse_spans and yields a node. Needs one clean reproduction to confirm which of the two paths actually mutated the document in the observed strikes.
- [/] [#zq8e] Editor-authority wedge blocks ALL queue maintenance (root of the #goal drain stall). Evidence in ops.log for this doc: at 19:32:45 `controller_crdt_current_text source=preflight_queue_maintenance` returns `status=current live_editors=1 delivery_converged=true`; 2s later the SAME queue_maintenance path logs `document_model_ensure_start initial_state=editor_attached_model_missing`, a `crdt_replica_register` succeeds (`data_kind=ok generation_fenced=true bootstrap_bytes=1312802`), then `document_model_ensure_republish_observer_not_ready` -> `document_model_ensure_failed` after 5s. EVERY `document_model_ensure_start` this session reports missing-replica across unrelated sources (queue_maintenance, pending_add_sync, controller_recycle_request) while the controller consistently reports current. Two observers of one document disagree; the ensure-path observer does not see a replica the controller demonstrably has. NOTE: likely already owned by the `src/agent-doc-editor-authority-fix` worktree (commits `preserve attached editor authority during retry`, `bound idle authority replay`) — coordinate before landing a rival fix in src/agent-doc.
- [/] [#7n20] Finish the ensure-window liveness fix across the CLI/controller process boundary. `bbc50c26e` extends the 400ms missing-replica ensure window when a `crdt_replica_register` proves the editor is alive, but the counter is process-global: on the split path the register lands controller-side while `ensure_document_model` runs CLI-side, so live telemetry still shows `window_extended=false` and the wedge persists. Two options: carry the liveness proof across IPC, or run the ensure controller-side where the registration already is. Do NOT route preflight's initial current-doc resolve through the ensure path — tried in this cycle and reverted; it violates `preflight_missing_editor_model_pauses_without_reading_disk_authority` and made preflight resolve to DISK content instead of waiting for the attached editor.
- [/] [#realtimesalient] #agent-doc-feature Write salient responses into the document in REAL TIME rather than buffering until finalize. Today only complete `### Re:` sections cross response-checkpoint and partial output is console-only (SKILL.md: "stream incomplete progress only to the console"). Requires deciding what counts as salient mid-turn, and preserving the invariant that incomplete token prefixes never become authoritative document text — so likely a distinct node kind (progress/salient) that is replaced by the final response rather than appended to it. Pairs with #promptselect threading.
- [/] [#mtds] [operator-verify] #panedraftunblocker live proof: on a Claude pane whose footer reads `⏵⏵ bypass permissions on · 1 shell · ← for agents` (the variant without `(shift+tab to cycle)`), with an EMPTY composer and no active turn, trigger JB Run Agent Doc. Expect dispatch to succeed instead of refusing with `timed_out` / `unblocker=wait_for_dispatch_ready_prompt`. Then repeat with unsent text typed in the composer and expect the new refusal naming the draft with `unblocker=submit_or_clear_pane_draft`, and NO injection over that text. Finally confirm a genuinely mid-turn pane (live spinner, bare `❯`) still refuses as busy. Fixed in 17848a927, installed 0.35.6; needs the supervisor idle-boundary recycle to pick up the fresh binary.
- [/] [#promptselect] #agent-doc-feature Selection-scoped `Run Agent Doc`: send ONLY the editor text selection as the prompt. Needs (a) a dedicated CLI verb — `agent-doc prompt "<text>"` (or `steer`; `run` is taken) accepting multi-line via stdin/cat, (b) JB+VSCode `Run Agent Doc` hooked to it so a non-empty selection routes through `prompt` instead of the whole-document trigger, (c) acknowledgement: agent-doc echoes the prompt into the document with the prompt prefix plus salient response, (d) responses attached UNDER their originating prompt — a threaded prompt->response layer in the Document Tree rather than a flat exchange tail. Must compose with realtime steering (#realtime-steering-verbatim) since a selection sent mid-turn IS steering. Design pass needed before implementation.
- [/] [#agentsignals] [future/separate] Add a realtime agent:signals element after core realtime convergence is stable. Start with deterministic built-in signals for tests, queue, supervisor, and ipc; design custom user-defined signals as plugin-backed tracked work with Rust as the first target and future multi-language support; use tasks/software/realtime-signals-patchboard.md as the GUI/semi-programmatic reference; keep MLP enhancement optional/experimental and not load-bearing for deterministic states.
- [/] [#steeringobservableset] #agent-doc-bug Phase 6 durable graph adoption: back RealtimeSteeringSet with a lazily reactive OBSERVABLE SET (not a drain-one-head QueueCell) keyed on the body-aware identity (Phase 3.1), reconciled on the reliable-sync plane; session-check reads set-emptiness (all directives settled) instead of re-diffing HEAD↔buffer. Plan: src/agent-doc/tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md (Phase 6). [recommended]
- [/] [#crde] JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978030079-4 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof) [symptom-key invariant=run_agent_doc_route_dispatch_failure document=71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad component=route/dispatch_only_dispatch_start_proof content_hash=sha256:3251e072b5c1857ba06e9ba613e1004b5eefef103db7003d7dcf7efe0d698ea8]
  evidence: JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class=accepted_without_dispatch_start_proof document=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md stage=dispatch_only_dispatch_start_proof pane=%83 actor_generation=1214 editor_attempt_id=1783978020608-2 dispatch_proof_state=accepted_without_dispatch_start_proof elapsed_ms=10000 diagnostic_path=none ops_log_path=/home/brian/work/btakita/agent-loop/.agent-doc/logs/ops.log ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof,phase=dispatch_only_dispatch_start_proof,result=accepted_without_dispatch_start_proof)
- [/] [#supboundarylivelock] #agent-doc-bug Supervisor idle-boundary recycle can livelock, leaving a stale binary mapped indefinitely. supervisor_recycle_action (agent-doc-supervisor/src/lifecycle.rs:124) returns None on `!turn_boundary` for everything except write_wedged/editor_delivery_stale, and turn_boundary = prompt_visible && !turn_active (idle_watch.rs:1898) never holds for a continuously-active pane. The only bypass, stale_recycle_safe_checkpoint (idle_watch.rs:136) = supervisor_stale && inflight_handlers == 0, is gated on staleness, so an explicit `admin recycle` on a NON-stale busy supervisor can never be consumed and silently expires at DEFAULT_RECYCLE_REQUEST_TTL_SECS=900 (recycle_request.rs:17) with the ledger phase left Requested. --force does not help: rpc.rs:5905 only changes the reason string, and idle_watch.rs:2403 collapses the request to a plain bool. MAX_CYCLE_OPEN_DEFER_TICKS escalation cannot rescue it either (idle_watch.rs:2253 only accrues the streak AT a boundary). OBSERVED 2026-07-18: supervisor pid 2040014 stayed stale across this whole session even though it IS stale (so the checkpoint bypass should apply and inflight_connection_handlers is an in-process RAII-guarded atomic that cannot leak) - the stale-side path needs its own trace. Fixing the non-stale side means a mid-turn execve, which is exactly what the boundary gate exists to prevent, so this needs a design decision (manufacture a boundary via the recycle-yield path? bound the defer? re-arm instead of expiring?), not a quick patch. Related shipped fixes: #dispatchgateleak + #authorityfactretention (submodule 566a72f6d).
- [/] [#jbrunsteering] #agent-doc-bug JB `Run Agent Doc` while a turn is already running should feed the prompt in as realtime steering (#realtime-steering-verbatim / #realtime-steering-aggregate) instead of interrupting the active turn. Operator-reported: invoking it mid-turn produced "Interrupted · What should Claude do instead?" rather than surfacing the new prompt to the running turn. NOT STARTED.
- [/] [#ay80] CI red on main, pre-existing and unrelated to #jbsteerinterrupt: `finalize_skips_ignored_untracked_session_doc` (tests/finalize_integration.rs:248) fails in GitHub Actions with `Unexpected stderr, failed var.contains(skipped ignored untracked path scratch/session.md)` but PASSES locally (7627/7627 green). Run 29657026491 on b7e17f4d5. Likely a CI-environment gitignore/tmpdir difference; reproduce with the CI runner git config.
- [/] [#5d9f] Second session observed that finalize's commit boundary persists queue maintenance through a path standalone `agent-doc queue sync` never reaches — the queue landed on the next write after three straight sync failures plus an `admin reload-lib`. Two persistence paths with different reachability is itself the bug surface. Determine why the commit path succeeds where queue sync fails, and route queue sync through whatever the commit boundary does. Result: queue text populates but stays UNARMED (`queue_drainable_head_count: 0`, `queue_continuation_required: false`), so the auto-loop still never gets the head.
- [/] [#jbroutasync] #agent-doc-bug Honor submit.deadline_ms in route dispatch (NARROWED — the async-endpoint half moved to #jbrouteasyncchannel). CONFIRMED: deadline_ms is only ever written (rpc.rs:1116, command_plane.rs:93) and never read by dispatch; routed_cycle_ack_timeout is 30s with a live child / 15s without (agent-doc-controller/src/dispatch.rs:2099-2103); the plugin sends deadlineMs = waitForReadySeconds*1000 for editor_route (CpcRouteClient.kt:281), observed as timeout_ms=15000 in ops.log. Net effect: with a live child the client gives up at 15s while the controller waits to 30s, so the operator sees a timeout while the route is still running. FIX: cap the ack wait at min(client deadline, routed_cycle_ack_timeout). PLUMBING REQUIRED (why this is not a one-liner): the deadline lives in the controller RPC layer but the wait happens in agent-doc-route-io at cycle_ack.rs:334, and no type in that crate carries a deadline. Thread an Option<Duration> through require_routed_cycle_ack (cycle_ack.rs:313) and its 3 call sites (authoritative_dispatch.rs:987, authoritative_dispatch.rs:1142, pane_resolution.rs) plus the controller→route boundary. NOTE test_cli.rs:16811 pins the exact expression 'routed_cycle_ack_timeout(live_child_for_file, cfg!(test))' and must be updated to pin the invariant, not the syntax. Verify with a live route against a busy pane: the client-side timeout and the controller-side give-up must coincide.
- [/] [#f1s3] #queuenest phase 2 — `ol` -> `after=` compilation, per tasks/agent-doc/plan-queue-nested-list-grouping.md. PHASE 1 SHIPPED in b67dd3582 (indent-aware parse retaining depth, leaf-only execution so group parents are never runnable heads, byte-preserving render round-trip). REMAINING: parse `1.` ordered-list markers (today they fall through to Freeform), and compile a nested ol to `after=` edges at ingress next to #queueingressorder, keeping `after=` the single source of truth so reordering an ol re-derives edges. BLOCKED ON A DESIGN ANSWER the plan itself records as open: do groups need ids (`[#epic]`) for `after=` to reference a whole group? Compiling an ordered GROUP to edges needs to know whether a group is addressable. Second open question (does `priority=` on a parent apply to all its leaves?) affects the sort, not the compilation, and can be settled later.
- [/] [#editormodelmissing] [operator-verify] #agent-doc-bug editor_attached_model_missing permanently wedges every write needing editor authority (agent-doc-bugs2 gen=1518, equityfundingsource gen=1452; healthy docs are gen 14-82). NARROWED 2026-07-18: survives admin recycle x2, admin reload-lib, AND a full IntelliJ restart, so it is NOT a plugin model-rebuild problem as first filed. Root cause is a one-way latch in editor-attachment state: TransportProjection.editor_generation (agent-doc-state-backbone/src/lib.rs:3394) is SET on StateFact::OwnerGenerationChanged (line ~1985) and never cleared — there is no editor-detach/close StateFact in the enum at all. document_has_live_editor_transport (agent-doc-document-realtime/src/crdt_authority.rs:165) therefore returns MultiReplica forever once an editor has ever attached, with NO liveness proof, even though the sibling authority_from_liveness (line 122) exists precisely to demote a stale listener via pid liveness. The ONLY predicate that flips it false is TransportPatchPhase::ForceDiskFallback — which is exactly why --force-disk is the sole escape the operator ever found. Second candidate latch, same shape: observe_editor_open's durable reliable-sync receiver journal may replay a stale editor-open assertion across an IDE restart (resolve_closed_editor_disk_fallback_current_doc, agent-doc-document-realtime-io/src/lib.rs:3774 bails only when it reports open). DECISIVE EXPERIMENT before coding: instrument which of the two reports attached — log editor_generation + observe_editor_open together at the EditorAttachedMissingReplica branch (agent-doc-crdt-relay-io/src/lib.rs:696). Both present identically, and the fix differs by which. DO NOT ship a speculative authority-demotion change: wrongly demoting a live editor lets the binary overwrite an operator buffer, which is the exact failure every guard on this path exists to prevent. Also worth shipping regardless: admin detect reported 'No fleet anomalies detected' while two documents were hard-wedged — add an attached-but-no-replica detector so this is visible instead of silent.
- [/] [#bbdqprio] #agent-doc-bug brookebrodack-dev.md: agent:queue carries the priority attribute but a cycle's response explained next tasks without reordering the queue by backlog priority. Hypothesis: same failure class as the queue_maintenance_not_persisted warning seen in agent-doc-bugs2.md this cycle — preflight COMPUTES the reorder (logs 'queue: sorted do-prompts by backlog priority') and then fails to persist it because editor authority stayed in editor_attached_model_missing, so the previous queue text stays in effect and the reorder silently evaporates. Verify by correlating a brookebrodack-dev preflight's 'queue: sorted' line against the resulting document; if confirmed, the fix is to make a non-persisted queue maintenance pass fail loudly (or retry) instead of logging a warning three lines after printing work it did not do. Do NOT run preflight against brookebrodack-dev.md while pane %9 owns it.
- [/] [#falsetyping-guard] #agent-doc-bug provenance-aware visible-write guard. INTEGRATED to main (merge on 5bcfc07b) + binary/cdylib installed + recycled; guard tests 7/7 green; JB 0.2.205 zip + VS Code 0.2.38 compiled. REMAINING (operator): install JB plugin zip (build/distributions/agent-doc-jetbrains-0.2.205.zip) + reload IDE, install VS Code 0.2.38, then live proof — type during replica churn (no wedge) + genuine unsaved edit still fails closed. NOTE: bugs2.md can still wedge until the IDE plugin is reloaded (loaded plugin still calls FFI v2 → conservative fail-closed).
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
- [ ] [#exttasksources] #agent-doc-feature Add external task source adapters that project PM-tool tasks into the active document/work graph as source-backed nodes, keeping local queue/closeout authority document-owned and making remote push-back explicit and audited. PRD: tasks/agent-doc/prd-external-task-sources.md
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
