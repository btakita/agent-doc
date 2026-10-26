# Deterministic Simulation Testing

`agent-doc` keeps targeted integration tests for real filesystem, git, tmux,
editor, and harness behavior. It also has a fast deterministic simulation layer
for workflow states that can be modeled in memory.

The simulator is not a second implementation of the CLI. It is a test-only
`SimWorld` that represents document text, snapshot text, cycle phase, captured
response body, fake commit outcome, backlog state, boundary markers, a small
route/controller actor model, and sync-layout ownership projections. Each
command in a generated schedule mutates that world through production pure
functions whenever possible:

- `diff::unified_diff_from_contents` and `diff::classify_prompt_bearing_changes`
  classify unresolved prompt-bearing changes.
- `template::parse_patches`, `template::apply_patches`, and boundary
  reposition helpers apply response bodies and normalize exchange boundaries.
- `agent_doc_element::element::parse` scopes checks to live template elements.
- `pending::detect_malformed_item_lines` verifies tracked backlog/icebox lines
  that would otherwise be hidden from pending guards.

The first supported command set covers closeout behavior:

```text
EditPrompt
EditLaterPrompt
AddMalformedBacklogItem
CaptureResponse
CaptureFallbackResponse
ApplyCapturedResponse
Commit
FailCommit
RepairBoundary
DuplicateVisibleResponse
CrashAt(FaultPoint)
PostCommitIpcRepositionSignal
Recover
SessionClear
BindRouteOwner
SupervisorReady
SupervisorBusy
SupervisorWaitingInput
SupervisorBlocked
SupervisorClosed
DispatchRoutePrompt
ProveDispatchAccepted
StaleSupervisorUpdate
ObserveStalePane
ObserveMissingPane
DriftProjection
RepairProjection
RepairBusyProjectionWithReadyPrompt
AdminPauseQueue
AdminPauseQueueStale
AdminResumeQueue
AdminDrainQueue
AdminHandoff
AdminHandoffStale
AdminReap
AdminReapStale
SupervisorHeartbeatReattach
SupervisorHeartbeatStale
SyncProtectedGrowthManual
SyncProtectedGrowthPassive
SyncProtectedGrowthFocusVisible
SyncDetachableReplaceManual
SyncDetachableReplacePassive
SyncVisibleFocusPreserve
SyncRerequestVisibleEditorManual
SyncRerequestVisibleEditorPassive
```

Every generated failure must include a stable seed and command trace. The
normal fast corpus runs in `cargo test`; the wider deterministic corpus is an
ignored test that `make check` runs explicitly after the normal suite.

The default development suite must stay pure/fast. Tests that create an
`IsolatedTmux` server are live integration tests and must be marked ignored so
plain `cargo test`, `make test`, and `make check` do not pay tmux startup,
window reconciliation, shell readiness, or process-tree sampling costs. The
explicit `make tmux-ci` target owns the ignored live-tmux sweep and GitHub
Actions must run that target on Linux with `tmux` installed.

Every live tmux test gets a server no other test run can reach. Tests build
their server with `agent_doc_tmux_io::isolated_test_tmux(label)`, never
`tmux_router::IsolatedTmux::new(name)` directly: `tmux -L <name>` resolves to
`$TMUX_TMPDIR/tmux-<uid>/<name>` (default `/tmp/tmux-<uid>/`), a directory shared
by every process of the user, so a fixed name is one server for every
concurrent suite (two worktrees, or two `cargo test` runs). Such runs collided
on `new-session -s main` (`duplicate session: main`) and the first to finish
killed the server under the other (`server exited unexpectedly`). The helper
appends the pid, a per-process sequence, and a start-time nonce to the label,
so session names inside the server (`main`, ...) need no uniqueness. Dropping
the handle kills the server and removes its socket file, which tmux itself
leaves behind. The `agent-doc-tmux-io` test
`workspace_builds_isolated_tmux_servers_only_through_the_helper` rejects any
other `IsolatedTmux::new` call in the workspace, and
`concurrent_runs_of_one_test_get_separate_tmux_servers` (`make tmux-ci`)
reproduces the shared-name collision as its control before asserting that two
same-label servers both create `main` and survive each other's teardown. A
subprocess under test reaches the same server through `AGENT_DOC_TMUX_SOCKET`
set from the handle's `server_socket`; no test may reach the operator's default
server.

Tests that mutate or inspect process-global state (`std::env` or the process
current directory) must hold the reentrant shared test env lock from
`test_support::env_lock()` or a helper backed by that same lock, and mutations
must restore prior values through RAII. This keeps focused `cargo test <name>`
and unbounded `cargo test` runs from poisoning `session-check` regressions
while `make check` remains the supported full-suite entrypoint with
`TEST_THREADS=2`.

Relay-hub tests isolate their keys, not the registry
(`#relayhubtestisolation`). The `agent-doc-crdt-relay-io` hub registry,
replica identity registry and retained projections are process-global because
one controller serves every document, and the editor-pid operations
intentionally cross documents: `reap_exited_editor_replicas(pid)` retires every
membership of an exited editor process, and a registration retires the same
pid's older editor ids on other documents (`#staleeditorregprune`). Tests must
not serialize the suite or swap in a test-only registry to dodge that. Each
test owns a unique document (a `temp_doc` path) and, whenever its replica
identity names an editor pid, a unique live pid from a per-test
`TestEditorProcess` child that is killed and reaped on drop; its
reliable-sync `Open` fact is seeded for that same pid. Sharing
`std::process::id()` as the editor pid let parallel tests prune and reap each
other's memberships, failing plain `cargo test` while `--test-threads=1` and
nextest passed. Plain parallel `cargo test -p agent-doc-crdt-relay-io`,
`--test-threads=1`, and nextest must all pass.

The same rule binds every crate whose tests drive those registries in one
process, including `agent-doc-controller-io`. The
reliable-sync liveness plane's `alive` register is keyed by editor pid, and
`register_replica_for_file` refuses admission when the document's only open
pid is dead. Tests that exercise editor exit correctly record `Alive{false}`
for their pid, and the durable journal they write keeps that death fact
foldable into the global plane by any later journal refresh. A test that needs
a live editor and claims `std::process::id()` is therefore refused whenever a
sibling marked the test binary's pid dead, and a test that reaps that pid
retires every sibling's `jetbrains-<own pid>-*` membership. A controller-io
test that registers an editor replica and records or depends on that editor's
liveness owns a per-test child process for the pid, kills and reaps it to model
the exit, and never reuses the test binary's pid.

Current deterministic budgets:

- Fast corpus: seeds `0..512`, `24` commands per seed, CPU-time budget `3s`,
  always run by `cargo test`.
- Medium corpus: seeds `0..2048`, `32` commands per seed, CPU-time budget
  `12s`, run by `make check`.

The budgets are perf-regression guards on the simulator's own work, so they are
measured in the corpus thread's CPU time (`CLOCK_THREAD_CPUTIME_ID`), never wall
clock (`#loadbudgettests`). The corpora run single-threaded and in memory; a
thread descheduled by host load accrues no CPU time, so a loaded `make check`
(load average 70-140 with several suites running) cannot fail them while the
same work stretched the wall-clock run from ~1s to 5.7s. The diagnostic line
prints both (`cpu_ms=… wall_ms=… cpu_budget_ms=…`). A platform without a
per-thread CPU clock reports the run and skips the budget rather than falling
back to wall clock.

Tests in the default suite must not assert a wall-clock bound to prove a
property that host load can stretch. Pin the clock instead: inject `now` (the
rename-debounce lease window), count observations and injected sleeps (a
zero-budget wait never sleeps), or answer from state (a replayed editor-op
epoch is consumed when it reads back absent, not when one bounded clear round
trip beat the clock).

A "not blocked by a blocked effect" property is proven by ordering, not by a
latency bound (`#remainingwallclock`): the effect's test fixture blocks on a
gate the test holds closed, and the test requires that the gate has not been
passed when the call under test returns (a call that waited on the effect could
only return after the gate opened), after first waiting for the effect to reach
the gate so the check is not vacuous. The gate opens itself at a hang ceiling,
so the regression fails instead of hanging the suite. The gate belongs to the
test that closed it: it is armed for that test's own document (its
`document_hash` or canonical path) and returns a test-owned handle whose
counters see only that document's effect, and dropping the handle opens and
disarms it. A process-global open flag and counter shared by every test that
uses the fixture is a cross-test race under the parallel crate suite: one
test's open released another's parked effect (the queued stale target then
applied before retirement), and one test's close reset another's counts.
"Admitted without waiting" is proven by counting observations, and "refused
before the backstop" by the refusal's type (the backstop's overrun is untyped).

A lock-order property is proven by an order checker, not by a race
(`#stateplanesubscriber`). When the two locks do not form a cycle under the
current runtime (Lazily runs Effects outside its state lock, so a state-plane
subscriber that inverts the order only stalls a publisher), no interleaving
deadlocks and a deadlock-shaped test passes with the inversion in place. The
test instead forces the path that takes each order with gates: a subscriber
parked on the condvar (a counter written under the channel lock proves it got
there) and a publication Effect held inside the context by a gated sink after
it notified. The debug-build checker panics at the first acquisition in the
wrong order, and a `should_panic` test keeps the checker armed.

Where a wall-clock bound remains, it is a hang ceiling, never a latency budget:
either a generous ceiling (30-60s, well under nextest's 180s per-test guard)
over an operation that otherwise hangs, run on a helper thread or child process
when the regression would block the test thread forever, or a ceiling at half
of the long deadline the test distinguishes from ("woken by the event, not by
its 30s deadline" asserts under 15s), with the test's input deadline raised
until that margin holds.

A transport deadline compiled short for tests (the 2s test-mode controller RPC
deadline) exists so that an absent or silent peer fails fast. Against a peer
the test has proven live, it is only a hang ceiling. Requests to a socket
served by an in-process test state actor (`start_state_actor_for_tests`) wait
up to 30s on both actor transports: the controller RPC client and the direct
state-wire transport (`send_ndjson_request_to_actor`, which carries the 250ms
editor-op projection and checkpoint budgets). The warm-socket registry and the
one deadline policy (`actor_response_timeout`) live in `agent-doc-state-wire`
so the two transports cannot drift; a registry only the controller client
consulted let a loaded `make check` fail a binary-owned capture resume with
`no response from Lazily actor within 250ms` from a live warm actor. Every
other socket, and every build without `test-support`, keeps the production
budget. Fixtures serve their RPCs from such a warm actor, not from a
cold-launched detached controller. Test setup that only needs a fact recorded
uses the retaining or state-deciding production path (`record_editor_ops_retaining`
plus `wait_for_retained_editor_ops`, `consume_replayed_editor_op_epoch`). It does
not use the strict single-reply 250ms optional-evidence budget.

A test that starts a listener in-process waits for the listener's own
readiness event, not a connect or `status` poll (`#replace120shang`). Both
servers announce at their serve-ready edge: the socket is bound under its
final name (after any stale or superseded endpoint is evicted), accept is
armed, and the accept loop runs next. The Project Controller announces from
`serve_with_options` right after its `controller_listener_serve_ready` marker
(tests start it with `serve_announcing_ready`); the editor IPC listener
announces through `start_listener_with_logger_until_ready`. The announcement
carries the bound socket path, and a start that returns without serving
(startup error, a skipped public serve, or the refusal to evict a live
same-build peer) drops the sender unsent, so the waiter fails at once instead
of at a deadline. The 120s wait is only a hang ceiling below nextest's 180s
per-test guard. After the announcement the test makes one request (a
controller `status`, or an IPC hello round trip) to confirm it was not early.

To replay one seed while reducing a failure, run a focused test with the seed
mentioned in the panic trace and temporarily promote that schedule into a named
regression test in `src/sim_world.rs`. To validate the wider budget locally
without the full check suite, run:

```sh
cargo test closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget -- --ignored
```

When a generated seed exposes a production bug, reduce the trace, add the
minimized command sequence as a focused regression, and keep the original seed
covered by the fixed fast corpus if it is cheap enough. If the fix needs a
larger generated range, adjust the corpus constants and this budget section in
the same change.

**Exhaustive model budget (`#simworldcitimeout`).** An exhaustive model
(every event sequence up to a fixed depth) must finish well under nextest's
180s per-test guard on a 2-core CI runner; a model that outgrows it is made
cheaper, never given a longer timeout, an `#[ignore]`, or a CI-only shallower
depth. Enumerating `events.pow(depth)` sequences and replaying each from the
initial world grows with every event a lane phase adds:
`two_ide_navigation_model` reached 20 events at depth 5 (3.2M sequences,
about 90s on two cores, past 180s on the GitHub runner). It now searches
breadth-first over distinct `(mode, world)` states: `step` and `check` read
only those, so two sequences reaching one state share every future, and a
state first reached at the shallowest level covers every sequence through it,
the same reachable set the enumeration checks, with a shortest trace for each
violation. The world drops its append-only violation logs once a check passed
(`World::normalized`): each entry's verdict is fixed when it is pushed, `step`
never reads them, and a mode switch clears the mode-relative ones (`writers`,
`deferred`), so normalization changes no later verdict;
`normalizing_passed_violation_logs_preserves_every_verdict` cross-checks every
mutation in both modes against an unnormalized search. Depth 5 now visits
about 19k distinct states (under 7s on two cores) and every mutation test is
unchanged.

The closeout simulator also supports named Phase 3 fault points:

- `SnapshotSave`
- `FallbackPatchWrite`
- `IpcDelivery`
- `TemplateMerge`
- `WorkingTreeWrite`
- `IndexUpdate`
- `GitCommit`
- `PostCommitBoundaryReposition`
- `SessionCheck`

Fault outcomes are deterministic. Authoritative write/commit/session-check
faults fail closed with an interrupted phase and a trace; `Recover` must either
complete the closeout and align the snapshot with the committed document or
surface the remaining invariant violation. Non-authoritative IPC delivery
faults are modeled as invariant-preserving no-ops after the document and
snapshot are already committed.

Closeout invariants currently exercised by the simulator:

- A later unresolved `prompt_target` blocks closeout after a response write.
- A malformed tracked checklist line in backlog/icebox blocks closeout.
- A captured or write-applied response cannot be reported successful until it
  crosses the commit boundary.
- Duplicate visible response patchbacks are rejected before commit.
- Boundary cleanup leaves exactly one live exchange boundary marker. The focused
  repeated-wedge regression runs eight post-commit handoff failures and recoveries
  and requires both the binary-owned committed projection and snapshot to retain
  that singleton on every cycle.
- A captured response cannot commit until its tracked-work mutation envelope is
  also captured; retained-delivery recovery preserves and applies both exactly
  once.
- A mixed tracked-work envelope either publishes every semantically valid
  backlog/review/done projection or none of them; a late invalid addition cannot
  leave earlier resolves, ungates, or an external done archive partially written.
- A malformed agent target is rejected before delivery and cannot corrupt the
  canonical document, while later valid retained transitions continue to make
  monotonic progress.
- After a committed closeout the working tree stays equal to HEAD modulo `(HEAD)`
  annotations and the transient boundary marker id
  (`#postcommit-ipc-worktree-corruption`). `PostCommitIpcRepositionSignal` models
  the live IPC listener firing the post-commit boundary-reposition signal at the
  working tree; because repositioning an already-clean committed boundary is
  idempotent, the visible file must not drift from the committed blob. A negative
  control proves a spliced/stale working-tree buffer fails the invariant closed.
- A sync projection never presents the same document under two visible panes
  (no-duplicate-editor-pane cardinality invariant).
- IPC snapshot duplicate-prompt repair removes an extra live-typed prompt copy
  before the repaired response crosses the commit boundary.
- A focused post-exchange scratch-comment ownership fixture preserves visible
  ordinary comments through route-style cleanup, preflight-style recovery,
  direct write normalization, IPC/plugin snapshot handoff, repair-style
  write-closeout, and compact-style exchange replacement, while still proving
  generated duplicate comment residue without ownership proof is scrubbed
  line-by-line.
- Intent-scope regressions cover late post-exchange scratch-comment edits:
compact exchange rebases its semantic component intent, while whole-document
and file-transport variants are unrepresentable in the maintained ABI.
- Baseline-drift replay coverage pins the three user-commit branches:
  benign edits outside the captured response auto-refresh replay hashes,
  edits inside the committed response fail closed, and user-normalized response
  bodies that still match the captured response after prompt-prefix stripping are
  adopted.
- Each named closeout fault point has an explicit fail-closed, recovery, or
  no-op outcome.
- The exhaustive CRDT-lineage world distinguishes a clean crash-lost queue add
  from an operator deletion: durable Lazily/CRDT lineage restores the former, while the
  operator frontier retracts the latter and every later replay ordering preserves
  the deletion. `formal/tla/CrdtLineageFence.tla` checks the same safety and
  liveness properties independently with TLC.

The simulator also covers route/controller schedules with a deliberately small
actor model:

- Durable actor state is the only authority for the active session actor
  generation, session id, pane id, and supervisor lifecycle.
- Tmux observations are ephemeral evidence. Durable actor, bootstrap, layout,
and delivery state live only in `state.db`; simulation must not create a JSON
compatibility authority.
- Supervisor lifecycle facts can move through starting, ready, waiting-input,
  blocked, and closed states. Route dispatch is accepted only for the current
  ready generation.
- Controller queue controls can pause, resume, or drain dispatch. Paused queues
  and draining non-ready actors must fail closed before prompt delivery and
  record deterministic backpressure coverage.
- Admin handoff and reap mutate durable actor ownership only when the observed
  generation matches the current generation; stale admin observations are
  rejected without mutating the route projection.
- Supervisor heartbeat reattach can repair stale/missing projection state from
  the current durable generation, while stale heartbeat generations are rejected.
- A JetBrains Clear Session Context simulation keeps `/clear` as an explicit
  session operator action while proving the following `Run Agent Doc` reroute
  still fails closed until the starting actor reaches a dispatch-ready prompt.
- Dispatch proof must match the same durable generation, session, and pane that
  accepted the dispatch.
- Stale actor generation updates, stale pane observations, and missing pane
  observations block dispatch/proof instead of silently creating duplicate
  authoritative owners or sending prompts to stale panes.

The simulator also owns pure sync-layout ownership schedules that do not need a
live tmux server:

- `sync_sim_tmuxbudget_seed_3001...` covers the open-closeout attach case:
  a hidden requested pane is brought into the visible projection even when the
  only unwanted visible pane owns an open closeout cycle. The open-cycle owner
  remains alive but can be stashed so sync does not create temporary visible
  pane growth.
- `sync_sim_tmuxbudget_seed_3002...` covers the replacement case: a hidden
  requested pane must replace an unprotected unwanted visible pane while a
  different open-cycle pane remains alive but detached from the visible
  projection.
- `sync_sim_tmuxbudget_seed_3003...` covers the focus proof: a preserve-layout
  return still reselects an already-visible requested focus pane.
- `sync_sim_tmuxbudget_seed_3004...` covers the remaining pure attach/focus
  variant: a hidden requested pane is attached while an open-cycle owner can be
  stashed and sync still focuses an already-visible requested sibling.
- `sync_sim_tmuxbudget_seed_3005...` covers the no-duplicate-editor-pane
  regression class ("3 tmux panes with 2 editor panes"): when the editor
  document is already visible and sync re-requests the same document — the
  duplicate-claim / pane-id churn surface logged as `duplicate_live_pane_claim`
  in `agent-doc-orchestration::sync` — the projection must keep a single editor
  pane rather than attaching a second one. The cardinality side of this is also
  enforced as a structural invariant on every command in the generated corpus
  (`SyncProjection.visible` must never list the same document twice).

The live cold-start side of this regression is guarded deterministically by
`auto_start_candidate_files` in `agent-doc-orchestration::sync`, which dedups the
auto-start candidate set by path so a document requested in more than one column
cannot cold-start a second pane before its first freshly-provisioned pane is
discoverable (covered by `auto_start_candidate_files_dedupes_repeated_documents_preserving_order`).
A faithful live-tmux reproduction would require spawning a real harness in the
provisioned pane (the reuse path cannot duplicate), so per the deterministic-first
policy above the candidate-dedup unit test plus this SimWorld cardinality trace
are the promoted coverage, with the existing live `find_associated_panes` tests
covering already-discoverable live-pane dedup.

This no-duplicate-editor-pane invariant is owned by the sync claim-tracking
(`claimed_sync_panes` / `SyncProofCache`), the auto-start candidate dedup, and the
`tmux-router` per-column pane dedup. It is independent of the `lazily-rs`
dependency graph: the only
`lazily-rs` use on the sync path is `parse_frontmatter_for_sync`, which
constructs a fresh per-invocation `CycleContext` (each slot computes at most once,
then drops), so no lazily-cached state survives across sync runs to drive
duplicate pane creation, and `tmux-router` does not depend on `lazily-rs` at
all.

## Editor integration harness (`#swint`)

The simulator also owns a deterministic editor-buffer actor, `SimEditor`, that
speaks the same Lazily reliable-sync + CRDT replica protocol as the editor
plugins. It does **not** load an IDE host, the Kotlin/TypeScript plugin classes,
or the native FFI binding; it is the fast production-Rust protocol/core layer.
It publishes editor-visible text through the controller-owned
replica relay and reads "current document" back through the production
`realtime_model::resolve_current_doc` seam (rung 3b, `#rtwatch`) — the same seam
`preflight` / `write` / `session-check` source the current document through.
This turns the File-Cache-Conflict / IPC-drift / queue-flood classes — which
previously only reproduced in a live IDE — into deterministic regressions.

A `SimEditor` attaches to a real on-disk document and can type an unsaved edit,
save (flush to disk), close (deregister the replica and publish a reliable-sync
OR-set close), adopt a CRDT-merged
broadcast from a peer editor, reload from disk, and absorb an external disk
write (agent-doc patchback) while open. Each editor is one of `EditorKind`
(`Generic`, `JetBrains`, `VsCode`, `Zed`); the read-authority contract is identical
across kinds, and the kind only changes the surfaced `CacheConflict` signal
(JetBrains modal dialog, VS Code non-modal badge, or Zed keep-buffer action) for
an external disk write that lands while the buffer is dirty.

- Slice 1 (foundation): `simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit`
  is the deterministic form of the live-only `#rtwverify` proof — an unsaved
  buffer edit is authoritative over stale disk (`resolve` returns
  `editor_buffer` and emits the grep-able `realtime_doc_resolve` ops.log marker),
  and the edit survives the agent's commit instead of being clobbered.
  `simeditor_save_then_close_falls_back_to_disk_authority` pins the "fall back to
  disk" half: a present in-sync buffer is `in_sync` (disk canonical) at the pure
  `reconcile_current_doc` seam, while the durable feed suppresses it to no-feed,
  and closing the document removes Lazily liveness so resolve reports `editor_absent`.
- Slice 2 (JB + VS Code parity): `simeditor_jb_and_vscode_buffer_authority_parity_with_kind_specific_conflict`
  proves both editor kinds agree on read authority (a dirty buffer always wins,
  a clean buffer defers to disk) while differing only on the surfaced
  cache-conflict signal — the File-Cache-Conflict class (`#w42v`) made
  deterministic. An external disk write never silently clobbers an unsaved edit.
  Compaction-specific convergence (`#jbcompactcrdt`) is pinned by the write-path
  tests `compact_convergence_is_exchange_scoped_preserving_concurrent_queue_edits`
  (the editor-IPC patch only `op:replace`s the changed `exchange`, so an operator
  concurrently typing `queue` items is never clobbered) and
  `try_compact_editor_converge_converges_via_editor_ipc_with_listener` (a live JB
  listener converges compaction through `transport=editor_ipc` instead of the
  direct disk write that raises a `File Cache Conflict`).
- Slice 3 (multi-editor `#rtwbcast`): `multi_editor_crdt_broadcast_converges_without_file_cache_conflict`
  opens two editors on one document, merges a divergent edit from each through the
  production `merge::merge_contents_crdt` path against the shared on-disk baseline,
  asserts the merge unions both edits with zero conflict markers, and broadcasts
  the merged document back so both buffers converge. This is the only way to test
  `#rtwbcast` without two live IDEs.
- Slice 4 (integrated system): `integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back`
  connects the editor seam to the route/controller actor model and the public
  `drain_owner` lease: an editor-queued edit routes to the owner pane, a fresh
  drain-owner lease (`#kp5z`) gates the supervisor drain, the controller drain
  applies + commits the document (the edit survives), the stuck-handoff reaper
rejects stale-generation handoff/reap under multi-owner contention, and the
committed document broadcasts back so both editors reconverge on disk.
- Slice 5 (three equal peers):
`three_simulated_editors_are_equal_peers_and_reconnect_to_the_same_crdt_cut`
opens JetBrains, VS Code, and Zed replicas at one frontier, makes a concurrent
local edit in all three before any pull, and requires every peer to converge
through the production relay and exact content-hash ACK path. Zed then
disconnects while the other two continue editing and must catch up from its
retained frontier on reconnect. Zed participates here because this layer tests
the shared protocol/core; its native plugin endpoint remains staged.

The cross-editor suite adds two deliberately named layers in
`tests/cross_editor_simworld.rs`:

- `cross_editor_plugin_protocol_harnesses_peer_through_real_agent_doc_controller`
runs production-shaped Rust replicas through an actual `agent-doc` binary and
Project Controller socket. It proves controller protocol behavior, not native
plugin execution.
- `native_plugin_harnesses_peer_through_real_agent_doc_controller`, invoked by
`make cross-editor-simworld`, starts the shipped JetBrains
`CrdtReplicaForwarder` / `CpSocketReplicaTransport` / `NativeReplicaNode` and
the shipped VS Code `CrdtReplicaForwarder` /
`ControllerSocketReplicaTransport` / `NativeReplicaNode` as headless peer
processes. Both load the just-built `libagent_doc`, exchange concurrent edits
through the real controller, ACK the converged cut, and prove offline reconnect.
This executes native plugin logic but intentionally does not emulate the IDE
Document/VFS GUI host; live editor smokes still own that final wiring.

Peer selection is capability-driven by `editors/plugin-parity.tsv`. A peer joins
the native suite only when all required feature rows, including
`cross_editor_native_harness_v1`, are `supported`. A `staged` peer is explicitly
excluded, and the test asserts the staging row so missing implementations cannot
produce a vacuous green. JetBrains and VS Code are supported; Zed remains staged
until its native endpoint and receipts exist.

Real tmux tests are still required in `make tmux-ci` for pane/window movement,
`tmux-router` reconcile behavior, shell/process ownership proof, and end-to-end
editor smokes. Pure ownership/cardinality variants should be promoted to named
SimWorld traces before the duplicate tmux-backed edge test is demoted from
local default coverage to the ignored live-tmux sweep. The `SimEditor` harness
does not replace the live editor smokes either: it pins the deterministic
read-authority / merge / drain contracts, while the live plugin smokes still
prove the IPC socket, VFS watch, and Document API wiring end-to-end.

The simulator tests print schedule count, command count, elapsed time, and the
active budget for each corpus so CI logs show when generated coverage starts to
approach the budget.

## Lazily-backed durable closeout harness

`tests/lazily_simworld.rs` is the first consumer slice built on the newer Lazily
simulation foundations. It pins Lazily commit
`92656d6840e9171b3e55a9e6b11205f820ec984c` as a test-only dependency and runs
agent-doc's production `DocumentWritePipeline::transition` reducer through
Lazily's `ReplayHarness`. Every step is fingerprinted, replayed from a fresh
graph, and compared at stride one; schedule reordering fails before value
comparison because the exact materialized log digest changed.

The same harness records accepted write phases in Lazily's backend-neutral
`DurableOwnerCore`, reconstructs the owner from its durable image after each
step, proves duplicate delivery is exactly-once, and proves a stale owner fence
cannot mutate the recovered prefix. This is deliberately a pure deterministic
slice: the existing filesystem, git, controller-socket, editor, and live-tmux
tests continue to own their real adapter boundaries.

The current Go `SimConsumerTestkit` accepts only Postgres and NATS real-service
adapters. Agent-doc therefore cannot yet express its CLI process, filesystem,
controller socket, or editor replica as a selected real adapter without lying
about the adapter kind. That missing generic process/port adapter is tracked in
the Lazily backlog rather than copied into this repository as a consumer-specific
fork.

FlowCore also has a pure cross-flow regression that does not need the simulator
state machine: it proves routed dispatch-proof failure, editor-visible
typing/IPC write deferral, closeout `session-check` interruption, and queue
child plain-response patchback normalization all emit typed flow events that
`ops summary` can bucket. Promote new route/write/closeout/orchestration bug
classes into SimWorld only when the behavior needs schedule interleavings or
durable document state beyond those pure event contracts.
