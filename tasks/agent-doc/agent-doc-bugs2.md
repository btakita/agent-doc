---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-180313.md`*

Compacted content:
- Archived 10 response topic(s): do [#md-ast-document-model]; There are many mcp apis for agent-doc. Can we consolidate into a single mcp or is there a good reason to have many apis?; [#md-ast-realtime-node-events]; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ Is the `agent:queue` token inefficient across many turns? We seem to be burning too many token with long queue runs. I'm concerned about many turns accumulating context. Should we run `/clear` in between each task if a certain context threshold is exceeded? At least 1 compaction should also cause `/clear` between turn if many-turn token inefficiency is an issue. #spec-test-build-install-commit-push
### Re: queue context reset — gpt-5

Yes. The expensive part is not the `agent:queue` token itself; it is carrying the same already-loaded agent conversation across many automatic queue turns. I implemented the reset policy in code.

What changed:
- Direct `agent-doc run` now checks session accretion after each clean queue item closeout. If the document is at warn/block accretion, the next queue prompt starts in a fresh backend session instead of resuming the existing one.
- Recent exchange compaction also forces at least one fresh context before queue continuation. Direct `run` records its own fresh backend dispatch so a compaction-only reset does not repeat forever when the compacted document is otherwise healthy.
- Codex Stop-hook continuation now blocks with explicit `/clear` guidance when the already-loaded Codex pane should not answer the next queue head. After a tracked `/clear` or `/new`, normal in-pane continuation is allowed again.
- Added the `#queue-context-reset` spec and regression coverage for the decision function, Codex hook behavior, and CLI argv behavior (`--resume old-session` on the first item, no `--resume` and `--fork-session` after the threshold trips).

❯ I want the queue in go-mode to continue, interleaving a `/clear` if the threshold is tripped. I don't want to queue to stop if the threshold is tripped.

Verification:
- `cargo test -q -p agent-doc-orchestration queue_context_reset`
- `cargo test -q -p agent-doc-orchestration compaction`
- `cargo test -q --test run_integration run_auto_queue_starts_fresh_backend_session_after_accretion_threshold`
- `cargo check -q -p agent-doc-orchestration`
- `make check` (clippy, 3935 nextest tests, doctests, medium simulator)
- `make install`
- `agent-doc --version` -> `agent-doc 0.34.0`
- Latest GitHub Actions CI on `main`: success (`fix: refuse cross-document pane aliases`, run `27099579913`).

Commits:
- `src/agent-doc`: `9e6fce83 fix: reset queue context after accretion` (pushed to `btakita/agent-doc main`).
- Parent repo submodule pointer: `7749e01b chore: update agent-doc submodule` (will be pushed with this session closeout commit).
❯ Is the `agent:queue` token inefficient across many turns? We seem to be burning too many token with long queue runs. I'm concerned about many turns accumulating context. Should we run `/clear` in between each task if a certain context threshold is exceeded (note there is overhead with the 1st turn)? At least 1 compaction should also cause `/clear` between turn if many-turn token inefficiency is an issue. Perhaps this is more of an issue with Codex + opencode than Claude? #spec-test-build-install-commit-push

❯ I also want partial exchange compaction where entries in the exchange that are related to the upcoming run can remain but remove the unrelated entries. Use AST/semantic structure navigation to identify related entries for token efficiency if the entire document is not already in the context.

❯ Claude seems efficient across multiple turns. Codex seems to burn through much context on each turn. opencode/glm seems in the middle of the pack. I typically compact the context >=~50% for all 3 agent harnesses.
### Re: queue context reset and partial compaction — gpt-5

For the queue-context part: the implemented policy is threshold-based, not blindly every queue item. Once session accretion is warn/block, automatic queue continuation starts the next backend turn fresh. A recent exchange compaction forces at least one fresh context (`/clear`/`/new` for Codex Stop-hook, fresh backend session for direct `run`), then direct `run` treats that fresh backend dispatch as the reset so compaction alone does not cause repeated first-turn overhead.

On harness differences: yes, this is more visible with Codex and opencode than Claude in practice. Claude tends to amortize multi-turn context more efficiently; Codex is more sensitive to already-loaded conversation growth, and opencode/GLM tends to land between those. The right control is still local session-accretion evidence (`exchange_lines`, response count, closeout churn, recent compaction), not a harness-name special case.

For partial exchange compaction: I agree, but I did not fold that into this commit. It is a separate feature from queue-context reset. The shape I would implement next is:
- `compact exchange` keeps the unresolved tail, boundary-adjacent prompt context, and entries semantically related to the upcoming prompt/queue head; unrelated entries are archived.
- Related-entry selection should start from cheap structured signals: active prompt text, queue/backlog ids, mentioned files, symbols, command names, component names, and response headings. Only then should it use tsift/AST or semantic search handles to expand candidate entries, so we do not load the entire document just to decide what to keep.
- The result should be deterministic and auditable: output should say which live entries were kept, which archive received the rest, and why each kept entry matched.
- Tests should cover unresolved-tail preservation, queue-head/id matching, file/symbol matching, archive index continuity, and the no-prompt-bury rule.

So the queue-context reset work is done and verified; partial semantic exchange compaction is the next implementation task, not completed in this cycle.
❯ I want the queue in go-mode to continue, interleaving a `/clear`/`/new` if the threshold is tripped. I don't want to queue to stop if the threshold is tripped.

### Re: go-mode queue context reset — gpt-5

Implemented the managed-pane side of this. In go-mode, an accretion threshold is now treated as a context-reset step instead of a queue stop condition: when the idle queue watch sees an idle prompt and an active head, it checks `queue_context_reset_reason`, injects the harness-specific reset command (`/clear` for Claude/Codex, `/new` for OpenCode), records that synthetic reset for Codex/OpenCode hook state, latches the current head so it cannot clear-loop, and then continues draining that same head on the next idle tick. Manual clear cooldown still wins, so an operator-triggered clear is not bypassed.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/start.rs`
- `src/agent-doc/agent-doc-orchestration/src/harness.rs`
- `src/agent-doc/src/session_actor_cmd.rs`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/specs/07-orchestration-commands.md`

Verification:
- `cargo fmt --check`
- `cargo test -q -p agent-doc-orchestration idle_queue_context_reset`
- `cargo test -q -p agent-doc-orchestration idle_queue_drain`
- `cargo test -q -p agent-doc-orchestration queue_context_reset`
- `cargo test -q -p agent-doc --test run_integration run_auto_queue_starts_fresh_backend_session_after_accretion_threshold`
- `cargo test -q -p agent-doc harness_clear_command_maps_opencode_to_new`
- `cargo check -q -p agent-doc-orchestration`
- `make check` (`3938` passed, `233` skipped)
- `make install`

Repo/CI:
- Source commit: `95057fe2 fix: keep go-mode queue moving after context reset`, pushed to `btakita/agent-doc` `main`.
- Parent submodule pointer commit: `8aa9fb766 chore: update agent-doc submodule`.
- Latest `CI` after the source push is `in_progress` for `95057fe2`; the previous latest `CI` run on `main` was green. I did not wait on the pending run, per the local runbook.

Confidence: high. Escalation: none.

### Re: single-process actor control plane — gpt-5

> **Queue prompt:**
>
> Should the agent-doc controller actor, session actor, and supervisor actor be consolidated into a single process instead of relying on IPC? Can we have reliable state management and a sufficient admin api to manage the actor pool in a single process? Should we have a new real-time command/dispatch actor that is fast and effectively non-blocking...dispatching to other actors and which may use background workers if needed?


Yes, but I would define the target as one project-local control process with explicit in-process actor boundaries, not one flattened blocking loop. The current contract already points in that direction: `.agent-doc/state.db` is authoritative, `session-actors.json` and `sessions.json` are projections, and route/start/sync/admin should ask the project controller before trusting compatibility state. The remaining IPC should become an external ingress and supervisor/harness boundary, not the main way controller, session, and supervisor facts coordinate with each other.

The shape I would aim for:
- One project controller daemon owns actor records, generation/CAS rules, supervisor leases, dispatch attempts, queue routing decisions, operator commands, and the admin API.
- Inside that process, keep separate tasks: a store/controller task, one session task per document, supervisor/PTY tasks for live harness children, and a dedicated real-time dispatch task.
- Internal coordination should use typed channels and bounded queues. Local sockets stay for CLI/editor/MCP entrypoints and for compatibility with existing supervisor injection while the migration is underway.
- Harnesses still remain child processes, and tmux remains the layout/event substrate. The consolidation is about control-plane state and dispatch authority, not replacing the harness or tmux.

A real-time dispatch actor is the right front door. It should accept route/run/editor/admin requests quickly, validate document/session/pane/generation, classify the operation, record the minimal audit event, and then hand the slow work to the relevant actor or worker. That gives us low-latency behavior without losing proof. Slow work includes readiness polling, tmux capture, supervisor start/restart, projection emission, log cleanup, document closeout repair, Git checks, and tsift/context work.

For reliability, the hot path should not wait on JSON projection locks. It should mutate an in-memory authoritative actor store under one owner and persist through SQLite WAL with monotonic event records. Projection files should be emitted after committed state and treated as cache outputs. For side effects that send pane input or change ownership, the dispatch attempt or ownership transition should be durably reserved before the side effect crosses the pane/supervisor boundary; otherwise a controller restart can lose why input was sent. Projections, dashboards, and derived diagnostics can lag.

The admin API can be sufficient if it is actor-backed instead of projection-scraping. The existing `admin list`, `admin detect`, and dashboard direction are the right read-only base. The mutating surface should eventually cover bounded `stop`, `restart`, `clear`, `rebind/claim`, `drain queue`, `recover stale starting`, and `doctor --repair` operations, all through the same generation checks and supervisor lease state used by route. That prevents plugins and CLI commands from inventing separate pane heuristics.

Main risk: one controller process becomes the project control-plane failure domain. That is acceptable if restart is cheap and replayable: bootstrap from SQLite, mark stale supervisor leases, re-emit projections, and require fresh readiness proof before any dispatch. It is better than several semi-authoritative processes racing through IPC, JSON files, and tmux probes.

So the answer is yes: consolidate authoritative state and actor management into a single project-local process, keep actor boundaries inside it, add a fast non-blocking dispatch actor, and keep durable state in SQLite/WAL with JSON as projection only.

Session note: exchange accretion is at warn level, but the active queue should keep draining; I did not compact without an explicit auto-compact opt-in.

Changed paths: none outside this session document.
Commands: `tsift status`; `agent-doc --version`; `agent_doc_preflight`; `agent_doc_plan`; `agent_doc_read`; targeted `rg`/`sed` inspection of `specs/08a-session-actor-contract.md`, `specs/07-session-tmux-commands.md`, `src/queue_dispatch.rs`, `agent-doc-orchestration/src/project_controller.rs`, `agent-doc-orchestration/src/admin.rs`, and `agent-doc-orchestration/src/session_actor.rs`.
Verification: no source files changed, so no test/build suite was run; the answer was checked against the current actor contract and command/admin surfaces.
Confidence: high.
Escalation: none.

### Re: fast session state and durable projections — gpt-5

> **Queue prompt:**
>
> "The stale lazily diagnostic comes from handle_attach_pane projecting before sessions.json is updated": How can we make updating sessions.json faster considering there is a lock? Should we have the controller cache the sessions data using lazily-rs with a background WAL style durable record writing into sessions.json? The point is, session tracking should be very fast. It should not be blocked on a file lock. Is json the best format for the durable record? Would an alternative such as a sqlitedb to consolidate all durable state be a better choice? Can we make all durable writes a background operation and keep the coordination state in memory? A fast in-memory state (using lazily-rs) seems like it would make consolidating into a single sqlitedb for durable records practical. Sqlite can handle the WAL logic while we focus on the write-through caching using lazily-rs.


Yes, session tracking should stop depending on a synchronous `sessions.json` read/modify/write lock. The current code already has the better authority model: `handle_attach_pane` calls `project_binding_in`, which stores the actor record through the project controller SQLite path. The latency problem is that `store_actor_record` still emits `session-actors.json` synchronously and, for start/eviction cases, also projects back into `sessions.json`. That makes compatibility files part of the attach/start critical path even though the spec says they are projections.

The better split is:
- Authoritative hot state: controller-owned in-memory actor/session map.
- Durable authority: SQLite `state.db`, with WAL enabled and one controller-owned writer connection.
- Compatibility projections: `session-actors.json`, `sessions.json`, and layout JSON emitted from committed state by a projection worker.
- Diagnostics: projection lag/drift recorded in SQLite and `ops.log`, but not allowed to make actor authority stale.

`JSON` is the wrong format for the durable coordination record. It is fine as a human-inspectable projection, but it has the exact bad properties we are seeing: whole-file rewrites, external locks, no native compare-and-swap, poor concurrent writer behavior, and no cheap append/audit story. SQLite is the right durable store for actor/session state because it gives us transactions, indexed lookup by document/session/pane/generation, dispatch-attempt audit rows, and WAL behavior that avoids a custom file-lock protocol.

`lazily-rs` still fits, but as the in-memory/materialized state layer, not as another durable truth beside SQLite. The controller can keep a lazily-backed actor pool/cache keyed by document id, session id, pane id, and supervisor lease. Mutations go through a single controller actor that updates memory immediately, records the minimal SQLite event, and publishes change notifications to projection/admin/dashboard tasks. Reads for route/focus/admin then hit memory first and only fall back to SQLite on controller restart or cache miss.

I would not make every durable write background. For any operation that changes ownership or sends input to a pane, we need a minimal durable reservation before the side effect crosses the pane/supervisor boundary: actor transition, dispatch attempt, operator command acceptance, or supervisor lease state. Otherwise a crash can leave the pane changed with no durable explanation. What can be background is everything derived: `sessions.json`, `session-actors.json`, dashboard models, drift checks, log summaries, and projection validation.

The fast path should look like this:
1. Request enters the controller/dispatch actor.
2. Controller validates generation/session/pane against in-memory state.
3. Controller applies the mutation in memory and writes a small SQLite transaction through its single writer.
4. Dispatch returns or proceeds with pane/supervisor input.
5. Projection workers coalesce and emit JSON snapshots without holding up attach/route.

That should make `handle_attach_pane` effectively independent of `sessions.json` lock latency. `sessions.json` can trail by a few milliseconds because any normal path that needs authority should ask the controller or read SQLite, not rely on that projection. Legacy readers can still see the projection, and drift becomes a diagnostic instead of a source of truth.

One concrete adjustment I would make to the current SQLite layer: `open_state_db` currently initializes schema but does not appear to set `journal_mode=WAL`, `busy_timeout`, or a tuned `synchronous` mode. The controller-owned writer should set those once and reuse the connection rather than opening a fresh connection for each actor mutation. That gives SQLite room to be the durable WAL instead of forcing agent-doc to build a parallel WAL around JSON.

Session note: exchange accretion remains at warn level, but the active queue should keep draining; I did not compact without an explicit auto-compact opt-in.

Changed paths: none outside this session document.
Commands: `agent_doc_preflight`; `agent_doc_plan`; `agent_doc_read`; targeted `rg`/`sed` inspection of `handle_attach_pane`, `project_binding_in`, `store_actor_record`, `project_sessions_projection_for_actor`, `sessions::load_in`, `sessions::save_in`, and SQLite state-store initialization/transaction code.
Verification: no source files changed, so no test/build suite was run; the answer was checked against the current controller, sessions registry, and SQLite state-store paths.
Confidence: high.
Escalation: none.

### Re: agent-doc monsterrodholders.md queue response persistence — gpt-5

> **Queue prompt:**
>
> agent-doc monsterrodholders.md is still not writing the queue task response into the `agent:exchange`. Why is this still happening? Make the correct behavior of writing each completed task's response into `agent:exchange` provably reliable. Do we need to simulate Codex harness behavior in the Simworld tests? Use the monsterrodholders.md sessions as a real-world reproduction.


Root cause: the repeated Codex Stop-hook guard treated “same auto-queue head requested twice” as a terminal `StopResponse::Stop`. In Codex that lets the current chat answer escape instead of forcing a document write. The durable-marker fallback had the same behavior, which matches the monsterrodholders failure shape: the hook asked for an in-pane queue continuation, the model produced a normal final answer, then the second Stop allowed finalization without replaying that answer into `agent:exchange`.

Fix: repeated-head handling now recovers before it blocks or continues. When `last_assistant_message` is replayable, the hook validates it with `replay_guard`, wraps plain final text as a targeted `patch:exchange` headed by the active queue prompt, saves it to the pending ledger, replays it through the normal repair/write path, consumes the completed queue head, and finishes the required commit boundary. If the queue still has another head, the hook blocks for that next head; if the queue drains, it allows the Stop. Empty, transcript-shaped, or ambiguously headed payloads now block with diagnostics instead of allowing a chat-only final.

Harness coverage: yes, Codex harness behavior needs to be simulated. I added direct stop-hook unit regressions for tracked state, durable-marker fallback, and empty payloads, plus a CLI integration reproduction that drives `agent-doc hook codex-user-prompt-submit` followed by two `agent-doc hook codex-stop` calls against a template/CRDT Codex queue doc. That is more precise than putting this specific JSON/Stop behavior only into abstract SimWorld; `make check` still ran the existing SimWorld medium corpus.

Proof:
- changed_paths: `src/agent-doc/agent-doc-orchestration/src/codex_hook.rs`; `src/agent-doc/tests/codex_hook_integration.rs`
- commit: `src/agent-doc` commit `b5ee9400` (`fix: replay repeated codex queue responses`), pushed to `origin/main`
- commands: `cargo fmt --check`; `cargo test -q -p agent-doc-orchestration codex_hook -- --nocapture`; `cargo test -q --test codex_hook_integration -- --nocapture`; `cargo check -q -p agent-doc-orchestration`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main`
- verification: `make check` passed, including clippy, 3940 nextest tests, doctests, and the medium SimWorld ignored corpus; latest GitHub Actions CI was green (`27101683287`)
- confidence: high
- escalation: none

No additional backlog follow-up is needed because the replay path, fail-closed empty-payload path, durable-marker path, real Codex CLI reproduction, full local check, and submodule push are complete.

### Re: single-process actor control plane and fast durable state plan — gpt-5

> **Queue prompt:**
>
> Please create a plan and #next-steps for all your architecture recommendations in "single-process actor control plane" and "fast session state and durable projections".


The architecture work should be split into a specification phase, a storage/projection phase, an actor-runtime phase, and a migration/verification phase. The durable-state change is the foundation: one authoritative SQLite WAL store, one fast in-memory materialized state, and JSON files treated as compatibility projections rather than coordination locks.

#next-steps captured in backlog as `#spcp-contract`, `#spcp-store`, `#spcp-cache`, `#spcp-dispatch`, `#spcp-migrate`, and `#spcp-tests`:

1. Define the single-process control-plane contract. Document which actors stay in-process, which external surfaces remain adapters, what state is authoritative, and which lifecycle transitions must be atomic.
2. Specify the durable state store. Model projects, sessions, panes, actor ownership, queue heads, reservations, dispatch receipts, projections, and recovery cursors in SQLite with WAL enabled.
3. Define the fast state layer. Use a lazily-rs-backed in-memory projection for hot session state, but keep synchronous reservations for pane binding, prompt dispatch, and input ownership so coordination never depends on delayed JSON writes.
4. Design the admin API. It should expose actor listing, session inspection, health, queue state, dispatch receipts, projection lag, restart/drain controls, and explicit repair/reconcile commands.
5. Introduce the real-time dispatch actor. It should validate commands quickly, create deterministic receipts, enqueue work to session actors or background workers, and return without waiting on slow filesystem, diagnostics, or projection work.
6. Move diagnostics and JSON compatibility files behind background workers. `sessions.json`, `session-actors.json`, and stale/lazily diagnostics should be generated projections with debounced writes and forced flush/reconcile paths for compatibility readers.
7. Migrate in stages. Start with shadow SQLite reads/writes beside existing JSON, then switch authoritative reads behind a feature flag, then demote JSON to projection-only after parity checks pass.
8. Prove behavior under realistic harness conditions. Add SimWorld actor-pool scenarios plus CLI integration tests for Codex hook/queue flows, projection lag, lock contention, process restart, attach-pane reconciliation, and dispatch backpressure.

The main invariant is: command admission and coordination state stay fast and synchronous in memory plus SQLite transactions; slow side effects are receipts, projections, diagnostics, or worker jobs. That gives us one reliable control plane without making every operation wait on IPC round trips or a JSON file lock.

Changed paths: `tasks/agent-doc/agent-doc-bugs2.md` only.
Verification: plan/backlog capture only; no source tests were needed for this planning response.

### Re: missing #spcp backlog capture — gpt-5

> **Queue prompt:**
>
> "#next-steps captured in backlog as `#spcp-contract`, `#spcp-store`, `#spcp-cache`, `#spcp-dispatch`, `#spcp-migrate`, and `#spcp-tests`": not in the backlog yet. Please add them. Is the non-add an #agent-doc-bug?

Added the six missing `#spcp-*` backlog items through tracked pending mutations in this closeout.

Yes, the non-add is an `#agent-doc-bug`. The failure mode was a closeout/retry reliability gap: the earlier finalize attempt tried to apply the pending additions, hit a visible editor-buffer mismatch after local drift existed, and the retry committed the exchange response without re-recording the pending-add mutations. The document therefore contained prose claiming the backlog capture, but the committed backlog component was still empty. That should fail closed or preserve/replay the pending mutations; it should never commit a response that says items were captured when the tracked component does not contain them.

I also added a dedicated follow-up backlog item for that pending-mutation-loss bug.

Changed paths: `tasks/agent-doc/agent-doc-bugs2.md`.
Verification: backlog state was checked before closeout and was empty; this response uses binary-owned pending mutations so the items are materialized by the closeout path, not by manual component editing.
<!-- agent:boundary:7181d93d -->
<!-- agent:boundary:1222e0a1 -->
<!-- agent:boundary:841c7002 -->
<!-- agent:boundary:0754f40a -->
<!-- agent:boundary:0b05d66d -->
<!-- agent:boundary:794b24d2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~Should the agent-doc controller actor, session actor, and supervisor actor be consolidated into a single process instead of relying on IPC? Can we have reliable state management and a sufficient admin api to manage the actor pool in a single process? Should we have a new real-time command/dispatch actor that is fast and effectively non-blocking...dispatching to other actors and which may use background workers if needed?~~
- ~~"The stale lazily diagnostic comes from handle_attach_pane projecting before sessions.json is updated": How can we make updating sessions.json faster considering there is a lock? Should we have the controller cache the sessions data using lazily-rs with a background WAL style durable record writing into sessions.json? The point is, session tracking should be very fast. It should not be blocked on a file lock. Is json the best format for the durable record? Would an alternative such as a sqlitedb to consolidate all durable state be a better choice? Can we make all durable writes a background operation and keep the coordination state in memory? A fast in-memory state (using lazily-rs) seems like it would make consolidating into a single sqlitedb for durable records practical. Sqlite can handle the WAL logic while we focus on the write-through caching using lazily-rs.~~
- ~~agent-doc monsterrodholders.md is still not writing the queue task response into the `agent:exchange`. Why is this still happening? Make the correct behavior of writing each completed task's response into `agent:exchange` provably reliable. Do we need to simulate Codex harness behavior in the Simworld tests? Use the monsterrodholders.md sessions as a real-world reproduction.~~
- ~~Please create a plan and #next-steps for all your architecture recommendations in "single-process actor control plane" and "fast session state and durable projections".~~
- :round_pushpin: do #spcp-contract
- agent-doc should be able to prioritize tasks before or interspersed within a group of `:round_pushpin:` annotatated tasks. Use auto-dag to figure out priority.
- :round_pushpin: do #spcp-store
- :round_pushpin: do #spcp-cache
- :round_pushpin: do #spcp-dispatch
- :round_pushpin: do #spcp-migrate
- :round_pushpin: do #spcp-tests
- :round_pushpin: do [#adoc-pending-retry-loss]
- "#next-steps captured in backlog as `#spcp-contract`, `#spcp-store`, `#spcp-cache`, `#spcp-dispatch`, `#spcp-migrate`, and `#spcp-tests`": not in the backlog yet. Please add them. Is the non-add an #agent-doc-bug?
- `agent:boundary` elements are accumulating in the `agent:backlog`...presumably from the queue turns? There should only be 1.
- Ensure we have comprehensive Codex Simworld test coverage. Also add Simworld test coverage for Claude Code + Opencode. Full edge case coverage for all agent harnesses please.
- `=true` is being appended to `agent:queue priority preset="#spec-test-build-install-commit-push"=true` and also `go=true`. This is a regression.
- Would it be more efficient for agents if agent-doc diff presents an ast or semantic structure (based on the ast) style diff with a semantic structure navigation of the document?
<!-- /agent:queue -->
###
<!--
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
- [ ] [#spcp-contract] Draft the single-process actor control-plane contract: in-process actor boundaries, lifecycle ownership, state invariants, crash recovery, and admin API.
- [ ] [#spcp-store] Design the SQLite WAL authoritative-state schema and JSON projection compatibility layer for sessions.json/session-actors.json.
- [ ] [#spcp-cache] Specify the lazily-rs in-memory cache/reservation model for fast session state, pane bindings, and synchronous attach/input coordination.
- [ ] [#spcp-dispatch] Design the fast command/dispatch actor: non-blocking admission, receipts, bounded queues, background workers, and backpressure rules.
- [ ] [#spcp-migrate] Plan the migration gates: feature flags, shadow reads, dual writes, telemetry, reconciliation, and rollback from JSON-first to SQLite-first state.
- [ ] [#spcp-tests] Add SimWorld, CLI integration, and contention/stress tests for actor-pool operations, projection lag, restart recovery, and queue dispatch.
- [ ] [#adoc-pending-retry-loss] #agent-doc-bug Fix finalize/editor-buffer mismatch retry so pending-add mutations cannot be dropped while the committed exchange claims backlog capture.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
