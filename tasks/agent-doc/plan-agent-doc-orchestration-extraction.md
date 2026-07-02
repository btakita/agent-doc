# Plan: `agent-doc-orchestration` crate extraction (`#ysv9` / `#adoc-orchestration-crate`)

Scoping document produced by `do [#ysv9]`. This began as the **next big
extraction** after `#adcr` (which produced the now-deleted `agent-doc-core`).
The original plan was to move a tangled route/git/sessions/supervisor cluster
behind one `agent-doc-orchestration` crate, leaving a thin CLI shell:

```
agent-doc (CLI)  →  agent-doc-orchestration  →  focused domain crates
  entrypoint +        route/git/sessions/         pure data layer
  command dispatch    ipc/supervisor/sync/...     and effect adapters
```

That migration succeeded as a transitional step, but the current goal is
stricter: **`agent-doc-core` stays deleted and `agent-doc-orchestration` is
detangled into domain-specific focused crates.** The focused crates are the
greenfield Rust API. There are no external consumers to preserve, so we do not
keep compatibility shims or facade APIs for old paths.

## Motivation

- Historically, `agent-doc-core` cold-built in **9.87s / 74 crates** while the
  main crate was **129s / 266 crates**. `agent-doc-core` is now deleted, but the
  same evidence still identifies the orchestration dependency tree as the build
  and coupling hot spot: `tokio`, `hyper`/`reqwest`/`rustls`/`ureq`,
  `interprocess`, `notify`, `rusqlite` (bundled SQLite), git, `zstd`,
  `tmux-router`.
- The orchestration cluster is **~44,500 LOC (~29% of the 154K main crate)**. Extracting it lets the CLI shell + command dispatch stay near core's build profile, and gives a clean dependency-injected boundary for testing.
- The current extraction work is the follow-on: keep core deleted and move
  orchestration-owned policy into focused domain crates instead of recreating a
  broad "core" API.

## Module inventory (the cluster)

| Module | LOC | Role |
|--------|-----|------|
| `sync.rs` | 11,365 | tmux 2D layout reconciler |
| `git.rs` | 8,430 | commit/branch/squash + missed-patchback self-heal |
| `route.rs` | 8,069 | harness trigger → pane routing |
| `start.rs` | 5,571 | start harness in tmux pane |
| `supervisor/` | 3,534 | PTY/process supervision (8 files) |
| `repair.rs` | 3,310 | orphaned-response / stale-cycle recovery |
| `sessions.rs` | 1,738 | session registry (sessions.json) |
| `capture.rs` | 1,070 | durable response-capture ledger |
| `cycle_state.rs` | 1,015 | per-doc cycle phase/hash state |
| `ipc_socket.rs` | 414 | Unix-domain-socket IPC |
| **Total** | **~44,516** | |

Adjacent modules likely pulled along (orchestration-side, to confirm during impl): `start`'s agent backends (`agent/`), `terminal.rs`, `focus.rs`, `layout.rs`, `resync.rs`, `hooks.rs`/`hook_cmd.rs`, `project_controller`, `startup_miss`, `gc`, `autoclaim`, `codex_hook`, `ops_log`, `sessions`-adjacent `session_actor*`.

## Dependency structure (intra-cluster edges)

```
sessions      → (leaf)
ipc_socket    → (leaf)
supervisor/   → (leaf — PTY/process only, no cluster deps)
cycle_state   → git              (1 ref — the only back-edge)
git           → capture, cycle_state, ipc_socket
capture       → cycle_state, repair
repair        → capture, cycle_state, git
sync          → capture, cycle_state, git, repair, route, sessions, supervisor
route         → cycle_state, git, repair, sessions, supervisor, sync
start         → cycle_state, sessions, supervisor, sync
```

- **Leaves:** `sessions`, `ipc_socket`, `supervisor/` — no intra-cluster deps.
- **Near-one-way knot:** `git ↔ cycle_state` — `git→cycle_state` is 35 refs, `cycle_state→git` is **1 ref**. That single back-edge is the only true cycle to untangle (or it disappears once both are in the same crate).
- **Mutual pair:** `capture ↔ repair`.
- **Top orchestrators:** `route`, `sync`, `start` depend on nearly everything.

## Historical extraction strategy — move as ONE crate

Unlike `#adcr` (which split a *layer* with a clean dependency direction), the orchestration cluster has internal cycles (`git↔cycle_state`, `capture↔repair`, `route↔sync`). **Moving these into separate crates is infeasible without breaking the cycles.** Instead, move the **entire cluster into one new workspace member `agent-doc-orchestration`** — internal mutual deps become intra-crate and are a non-issue. The real engineering is the **boundary** between orchestration and the CLI shell.

### What stays in `agent-doc` (CLI shell)

- `main.rs` (clap entrypoint + command dispatch)
- Thin command modules that only parse args and delegate: `claim`, `focus`, `outline`, `history`, `upgrade`, `plugin`, `install`, `commands`, `convert`, `extract`, `undo`, `dedupe`, `clean`, `reset`, `init`, `session_cmd`, `audit_docs`, etc.
- The `ffi.rs` cdylib shell (see FFI below)

### What moves to `agent-doc-orchestration`

- The 10 cluster modules above + their orchestration-adjacent neighbors (confirm the exact set in Wave 1 by following the import graph from `route`/`sync`/`start`).
- Heavy deps move with them: `tokio`, `reqwest`/`ureq`/`hyper`/`rustls`, `interprocess`, `notify`, `rusqlite`, git, `zstd`, `tmux-router`.

## Current modified plan — no-shim domain extraction

Wave 0 (scaffold) + Wave 1a (`ipc_socket`) landed cleanly, and the later
big-bang move created the current `agent-doc-orchestration` transitional crate.
The old bottom-up "move leaves first" sequence is no longer the plan. Current
work extracts **behavior seams** out of orchestration into the focused crate
that owns the domain:

- document/realtime authority -> `agent-doc-document-realtime`
- turn lifecycle / closeout / operation-log vocabulary -> `agent-doc-turn`
- controller/route/CAS decisions -> `agent-doc-controller`
- supervisor lifecycle policy -> `agent-doc-supervisor`
- tmux command/diagnostic policy -> `agent-doc-tmux-*`
- queue syntax/scheduling/journal policy -> `agent-doc-queue`
- workflow cross-cutting decisions -> `agent-doc-workflow`
- Auto-DAG/work graph policy -> `agent-doc-work-graph`
- prompt/response line classification -> `agent-doc-prompt-lines`
- prompt/context assembly -> `agent-doc-prompt-context`
- response TOC/rendering vocabulary -> `agent-doc-response-toc`
- session accretion/restart-loop heuristics -> `agent-doc-session-accretion`
- SHA-256 content hashing vocabulary -> `agent-doc-hash`
- create a new focused crate only when no current crate owns the domain

### Shim policy decision: keep no Rust API shims

The active policy stays **no compatibility shims for Rust APIs**. Removing that
policy would be friendlier for short-lived migrations, but this repo is
greenfield from the consumer perspective: there are no downstream Rust callers
whose old paths need preservation. Keeping old paths alive would optimize for a
consumer that does not exist while making the internal ownership graph harder to
read.

- Do **not** preserve old `agent_doc_orchestration::*` or root-crate paths as
  facade APIs for extracted policy.
- Delete compatibility shims when extracting a seam. Internal callers must
  import the focused crate directly.
- A shim is any Rust function, module, or `pub use` that keeps an obsolete
  caller path alive after the policy moved. It is not enough for the old path to
  delegate correctly; the old path itself is the coupling being removed.
- Orchestration may keep only effectful adapters: file IO, git/tmux/process IPC,
  SQLite/socket ownership, ops-log emission, CLI formatting, and glue that
  supplies facts to focused policy.
- Static boundary tests should guard every extraction by asserting the focused
  crate owns the policy vocabulary and orchestration does not re-own or re-export
  it.
- Tests should move with the behavior. Adapter tests remain only for real side
  effects.

### Deprecated-name rename policy

Extraction should also retire obsolete internal names when the product/domain
language has moved on. A deprecated module, function, type, crate, test, or doc
name is another compatibility surface: even if it delegates correctly, it keeps
future work thinking in the old model.

- Rename deprecated internal names to the canonical current domain name while
  extracting the seam. Do not leave alias modules, `pub use` aliases, or wrapper
  functions behind.
- Prefer the current user-facing/domain vocabulary in file names and imports:
  backlog/tracked-work/review/icebox/queue/turn/document instead of historical
  names such as pending/core/shim/facade.
- Treat rename-only cleanup as valid extraction support when it replaces an old
  ownership cue with the current name. Keep it scoped and covered by static
  no-shim/no-alias scans.
- `pending_cmd` is a concrete candidate: it owns user-facing backlog and
  tracked-work subcommands, so it should be renamed to `backlog_cmd` or split
  into a clearer tracked-work command module during the backlog/queue extraction
  pass. The old `pending_cmd` module path should not remain as an alias.

### Command scope for icebox

Backlog/tracked-work commands should target live actionable work by default.
Icebox is parked work, so it should be reachable only through explicit icebox
targets or promotion/move operations, not through default backlog or queue
activation paths.

- `backlog_cmd` may own shared tracked-work file adapters, but default
  add/done/gate/reorder/reap verbs should continue to mean active backlog unless
  the command explicitly names `icebox` or a move/promote operation.
- Explicit icebox subcommands may backfill, edit, reorder, clear, reap completed
  icebox items, or move/promote items into the active backlog.
- Document-integrity maintenance may inspect icebox ids to avoid data loss,
  stale done markers, or broken references. That inspection is not the same as
  making icebox items drainable or auto-queueable.
- Queue/autodag/liveness policy must not treat icebox items as active work
  unless an item is explicitly promoted/enqueued. Warnings for misplaced queue
  attributes on `agent:icebox` are preferred over silently syncing parked work
  into `agent:queue`.

Allowed exception: exported editor/plugin FFI symbols are an ABI boundary, not a
Rust API. The cdylib may keep `extern "C"` wrappers that preserve those symbols,
but Rust call sites still move to focused crates directly.

Allowed non-shim adapters: `agent-doc-orchestration` may keep functions that do
real orchestration work, such as reading files, resolving frontmatter in project
context, querying git/tmux/process/IPC/SQLite state, building response TOCs, or
supplying gathered facts to focused policy. Those adapters are not compatibility
facades as long as they do not preserve an obsolete policy API or re-export a
focused crate under the old path. These effect adapters are part of the
post-epic architecture; only compatibility shims/facades are temporary and
should be deleted during extraction.

### Modified execution plan

1. Treat `agent-doc-orchestration` as a shrinking effect adapter, not the final
   product boundary.
2. Extract pure policy/data seams into the owning domain crate first; create a
   new focused crate only when no current crate owns that domain.
3. Use fan-out extractors for disjoint seams. Each extractor owns a narrow write
   set and can move multiple related seams only when they share the same
   destination and tests.
   The practical fan-out size is 4 by default and can grow toward 8 only when
   the write sets are genuinely disjoint and the main session can still review
   and integrate every diff. Extractors should run focused target-crate tests
   for their seam, but the main integration pass owns final boundary tests,
   `make check`, commits, pushes, plan updates, and CI sampling.
4. Extract by layer, not by historical file-wave. A single turn may move all
   helpers in a layer that do not need broad adapters: gather facts in
   orchestration, move pure policy/data decisions to the owning crate, move
   behavior tests with the policy, and leave only effect adapters behind.
5. Keep docs, cross-crate boundary tests, final no-shim scans, `make check`,
   commit, push, and one CI check in the main integration pass.
6. Prefer direct dependency inversion over shared catch-all utilities. A shared
   IO/base crate is tactical only when it unblocks a domain extraction.
7. Rename deprecated internal symbols and module names to the current domain
   vocabulary in the same pass as the related extraction; do not preserve the
   obsolete path as a compatibility alias.

### Current layer status

The legacy migration-wave labels are superseded. Current work is in the
domain-policy layer of Option B: orchestration remains the effect coordinator,
while pure decisions move into the crate that owns the vocabulary. As of the
2026-07-02 fan-out batches, the current layer has extracted the following
seams:

- Route-owned cycle change/completion policy moved from `start.rs` to
  `agent-doc-supervisor::route_owned`; orchestration now only converts
  `CycleState` into route-owned facts.
- Starting-pane recovery target selection moved from `route.rs` to
  `agent-doc-supervisor::startup_miss`; route keeps tmux/session-registry
  probing and calls the focused policy directly.
- Route pane snapshot diagnostic formatting moved from `route.rs` to
  `agent-doc-tmux-commands::input_diag`; route keeps redaction, timestamping,
  file writes, ops-log emission, and stderr output while calling the focused
  filename/log/hint formatters directly.
- Controller self-watchdog and stranded handoff-replacement policy moved from
  `project_controller/rpc.rs` to `agent-doc-controller::status`; orchestration
  now supplies bootstrap/socket/time facts.
- Sessions projection entry selection moved from `project_controller.rs` to
  `agent-doc-controller::status`; project-controller keeps SQLite reads,
  tmux-router registry adaptation, supervisor lease lookup, and projection file
  writes.
- Visible-write reconcile loop policy moved from `write.rs` to
  `agent-doc-document-realtime::write_policy`; orchestration keeps file,
  live-buffer, log, and mutation effects.
- Codex-network-access-on-non-Codex warning construction moved from
  `preflight/run.rs` to `agent-doc-model-tier`; preflight supplies document and
  active-harness facts.
- Routed dispatch-start proof classification moved from `route.rs` to
  `agent-doc-controller::dispatch`; route now adapts hook/tmux/session facts and
  calls `classify_codex_routed_dispatch_start_proof` or
  `opencode_pane_state_changed_from_idle` directly.
- Auto-trigger outcome and capability-proof gate wire-state policy moved from
  `start.rs` to `agent-doc-supervisor::auto_trigger`; start keeps the atomics,
  cancellation, process IO, and send effects.
- Supervisor child-exit provenance formatting moved from `start.rs` to
  `agent-doc-supervisor::crash_policy`; start keeps the `portable_pty`
  `ExitStatus` adapter and passes rendered status/success facts to the focused
  formatter.
- Response-capture repairability moved from `repair.rs` to
  `agent-doc-workflow::capture`; repair now filters active captures through
  `capture_state_is_repairable`.
- Stuck-`Preparing` controller threshold parsing/defaulting moved from
  `project_controller.rs` to `agent-doc-controller::status`; project-controller
  keeps only environment IO and passes the raw override value to the focused
  policy.
- Route closeout drain outcome vocabulary and user-facing outcome formatting
  moved from `route.rs` to `agent-doc-controller::dispatch`; route keeps closeout
  IO and calls `agent-doc-turn` directly for recovery-command projection before
  passing the result as a controller fact.
- Controller crash-recovery stats and marker payload formatting moved from
  `project_controller.rs` to `agent-doc-controller::status`; project-controller
  keeps SQLite/projection/file IO and records focused marker statuses.
- Existing-session owner conflict message formatting moved from `start.rs` to
  `agent-doc-supervisor::session_owner`; start keeps tmux/session lookup and
  passes gathered pane facts to the focused formatter.
- Stale response-capture retirement decisions moved from `repair.rs` to
  `agent-doc-workflow::capture`; repair now gathers file-backed drift/body/heading
  evidence and calls `decide_stale_capture_retirement`.
- Repair-template change aggregation moved from `repair.rs` to
  `agent-doc-workflow::capture`; repair now gathers the individual repair facts,
  then uses `RepairTemplateChanges` for persist and ordered log decisions while
  keeping file/snapshot IO and ops-log effects.

The no-shim policy remains active for these layer extractions: old helper names
were removed or confined to static test strings, and boundary tests now assert
the owning crates expose the policy while orchestration does not re-own it.

### Current extraction ledger

- `agent-doc-hash` now owns shared SHA-256 hex content hashing. Root,
  orchestration, debounce, and tests call it directly. The old
  `ops_log::content_hash`, `op_capture::content_hash`, and
  `agent_doc_debounce::content_hash` paths are intentionally deleted rather than
  retained as compatibility shims.
- `agent-doc-diff` now owns diff truncation/read-stability pure policy:
  extracting the last inserted non-empty line, deciding whether a line looks
  mid-typing/truncated, and formatting bounded diagnostic snippets. `diff_io`
  remains the snapshot/file/editor wait adapter and imports the focused helpers
  directly; the local helper implementations and pure tests were removed from
  orchestration rather than kept as wrappers.
- `agent-doc-diff` now also owns the partial-staging companion classifier:
  filtering committed/dirty path evidence, deciding source/test relatedness,
  extracting changed literals from committed and dirty diffs, and returning the
  sorted overlapping-literal finding. Orchestration keeps only git repo
  discovery, git command execution, repo-path attachment, ops-log emission, and
  warning rendering.
- `agent-doc-diff::semantic` now owns semantic-diff summary vocabulary and
  construction: component/frontmatter change projection, AST node event
  projection, bounded navigation targets, prompt-change previews, and stable
  summary ordering. Orchestration keeps only the preflight/run effect adapters
  that gather inputs, project summaries into durable op-log records, persist
  cycle/output state, and render JSON/warnings while importing the focused
  builder directly. There is no preflight re-export of
  `semantic_diff_summary`.
- `agent-doc-prompt-contract` now owns prompt preset request resolution:
  combining diff-level and harness-diff preset directives with prompt-target
  preset references, canonicalizing bare hashtag aliases, preserving ordered
  dedupe, and reporting missing preset names. Preflight keeps frontmatter/file
  fact collection and the user-facing missing-preset error while importing the
  focused resolver directly.
- `agent-doc-turn` now owns owner-pane self-invocation wedge counter policy:
  the persisted record schema, same-head increment/new-head reset transition,
  and halt threshold. Orchestration keeps the sidecar path/file IO adapter,
  renamed from the issue-tag `recguard_wedge` module to
  `owner_pane_wedge_counter`, and imports the focused policy directly without
  retaining the old module path or threshold wrapper.
- `agent-doc-turn` now owns response-text policy: extracting prompt targets from
  `### Re:` response headings, stripping agent-doc patch metadata from captured
  response summaries, bounding hook summary text, classifying imperative
  response evidence/blockers/status-only replies, deciding imperative
  diff/response contract outcomes, selecting the rejected trigger, and bounding
  trigger text. Orchestration keeps hook execution, capture loading,
  project-root/git facts, tsift process invocation, snapshot/diff gathering,
  ops-log emission, and fail-closed error adapters while importing the focused
  response-text helpers directly.
- `agent-doc-turn::response_replay` now owns response replay/application
  policy: captured-response heading extraction, orphan prompt/topic matching,
  visible response patch extraction between document bodies, live-exchange
  answered-heading detection, known-response prompt-change filtering, and
  contiguous response-applied matching with prompt-prefix and queue-echo
  normalization. Repair/capture/closeout/write keep only file-backed capture,
  snapshot, materialization, and recovery adapters while importing the focused
  replay helpers directly; the old `repair::*` Rust API path was deleted rather
  than retained as a facade.
- `agent-doc-turn::document_drift` now owns closeout/session-check document
  drift classifiers: exchange append-vs-rewrite detection, normalized exchange
  body extraction, exchange-only promptless drift, answered exchange plus
  backlog metadata drift, comment-only drift, direct response-patchback
  detection between two document bodies, bare prompt-target annotation for that
  detector, and component-content masking. Session-check keeps active-session
  lookup, snapshot/file/git normalization, repair attempts, and user-facing
  interruption messages while importing the focused drift policy directly.
- `agent-doc-turn::op_log` now owns session-check ops-log event vocabulary and
  pure event parsing: preflight-start/write-consumed/snapshot-saved/
  IPC-proof-insufficient event names, timestamp-prefix stripping, event-token
  extraction, and missing-commit write-event classification. Session-check keeps
  only project-root/log-file lookup, log scanning, recovery attempts, and
  user-facing diagnostics while importing the focused ops-log policy directly.
- `agent-doc-turn` now owns turn-status policy vocabulary: turn-active and
  stale-supervisor marker path constants, the readable turn-active marker schema,
  marker freshness/pane matching, and pane-title composition. Orchestration keeps
  current-dir project-root resolution, marker file IO, serde JSON persistence,
  supervisor stale marker file effects, and tmux title updates.
- `agent-doc-prompt-contract` now owns harness prompt parsing policy: stripping
  binary-owned `agent-doc <file>` invocations, classifying claim/compact
  invocations as non-session prompts, matching the invoked file, preserving
  non-invocation prompt bodies, reverse-scanning Codex hook prompt text for the
  real invocation while ignoring fenced placeholders, and rendering the
  synthetic harness diff. Orchestration keeps only the env/Codex-hook
  prompt-source adapter plus cwd/canonicalization path IO and imports the
  focused parser directly.
- `agent-doc-prompt-context` now owns document-section prompt context selection:
  deciding when session-accretion warnings with prompt targets should use the
  bounded response-context pack, parsing components for that pack, and falling
  back to the full document when the focused preconditions are absent.
  Orchestration keeps only diff prompt-target extraction, required-SSH
  frontmatter scope rendering, and live response-TOC lookup before calling the
  focused renderer directly.
- `agent-doc-prompt-context` now owns remote-host scope wording for prompt
  context: rendering the no-target and declared-target guidance plus the
  explicit warning against treating ambient SSH approvals or unrelated history
  as project evidence. Orchestration keeps only file/project-context
  frontmatter parsing to gather `required_ssh_targets` and calls the focused
  renderer directly.
- `agent-doc-supervisor` now owns supervisor child launch-argument precedence:
  resolving frontmatter/config `agent_args` plus harness-specific Claude/Codex/
  OpenCode aliases and the Claude env fallback from a focused fact struct.
  Orchestration keeps only frontmatter/config/env fact projection and calls the
  focused resolver directly from start/run assembly.
- `agent-doc-supervisor` now owns recycle-yield and recycle-in-flight marker
  policy: sidecar directory/file vocabulary, persisted marker schemas, canonical
  reasons, TTL env/default resolution, freshness checks, and recycle settle
  timing constants. Orchestration keeps only marker path resolution, JSON
  sidecar reads/writes/clears, sleep/poll loops, and user-facing diagnostics
  while importing the focused marker contracts directly.
- `agent-doc-supervisor` now also owns route-submit in-flight/blocked marker
  policy: sidecar directory vocabulary, persisted marker schemas, canonical
  route-submit reasons, reason-specific TTLs, marker constructors, and
  freshness checks. Orchestration keeps route marker path resolution, JSON
  sidecar reads/writes/removes, guard drop cleanup, and ops-log diagnostics
  while importing the focused marker contracts directly.
- `agent-doc-plugin-owner` now owns editor plugin-owner lease policy: persisted
  lease schema, TTL env/default resolution, lease path election, pid liveness,
  live-editor attachment facts, merge ownership projection, live consumer-id
  lookup, acquire/refresh, and release semantics. Orchestration keeps only the
  write/IPC/CRDT call sites and test setup; the old
  `agent-doc-orchestration/src/plugin_owner.rs` module was deleted rather than
  retained as a facade.
- `agent-doc-ipc-protocol` now owns callback wire protocol policy: request,
  response, patch, and pending-callback JSON schemas; callback request
  construction; response construction; request expiry; pending urgency buckets;
  pending projection; and stale response/request matching. Orchestration keeps
  only callback sidecar directory discovery, request/response file IO, cleanup,
  and pending-directory scanning effects while importing the focused protocol
  directly.
- `agent-doc-ipc-protocol` now also owns full-content IPC mode vocabulary and
  stable source labels for response fallback vs operator-owned replacement.
  Write IPC transport keeps file reads, socket/file delivery decisions, scope
  checks, and ops-log diagnostics while importing the focused mode type
  directly.
- `agent-doc-tmux` now owns associated-pane resolution policy: pane inventory
  candidate schema, proof-source vocabulary, source-summary rendering, stash
  classification, tmux inventory-line parsing, and deterministic winner/
  ambiguity selection. Orchestration keeps tmux inventory collection, process
  tree probing, registry/session-log/supervisor evidence gathering, owner
  filtering, recovery side effects, and user-facing sync diagnostics while
  importing the focused resolver directly.
- `agent-doc-tmux` now also owns stash prune policy: prune cleanup mode
  vocabulary, stash TTL candidate schema, TTL-enabled candidacy rules, and
  target pane-id filtering. Orchestration keeps tmux window/pane discovery,
  registry and supervisor evidence gathering, project config checks, ops-log
  writes, report-only diagnostics, and `kill-pane` effects while importing the
  focused prune policy directly.
- `agent-doc-workflow` now owns workflow-doctor policy: the stable report/fact
  schema, outcome vocabulary, invariant result schema, catalog evaluator, repair
  command selection, marker tests, and stable ops-log marker classification for
  queue-pause, stale-supervisor, generation-redirect, stale-generation, and
  editor-convergence failure lines. Orchestration keeps fact gathering, ops-log
  file reads, scan limits, warning collection, text/JSON output, and CLI
  adapters while importing the focused doctor policy directly.
- `agent-doc-workflow` now also owns session-cycle turn-scope and user-intent
  policy: prompt-target id extraction, queue-driver address resolution,
  exchange-tail floor calculation, turn-scope derivation, and prompt-preemption
  filtering for synthetic queue continuations, managed-state edits, and
  independent turn ops. Preflight keeps durable op-log persistence, cycle-state
  storage, and warning/output adapters while importing the focused workflow
  helpers directly; the old preflight glob export was narrowed so these helpers
  are not available through an orchestration path.
- `agent-doc-session-accretion` now owns recent restart-log counting policy:
  session-log timestamp parsing, blank/malformed line skips, recent-window
  filtering, restart-churn event extraction, and restart-heavy event counting.
  Orchestration keeps only session-log path resolution and optional file reads
  before calling the focused counter directly.
- We are in the current seam-by-seam extraction wave after the transitional
  orchestration crate move. Practically, this is **Wave 2**: shrink
  `agent-doc-orchestration` by moving durable policy into focused domain crates,
  leaving orchestration as effect adapters.
- `agent-doc-queue` now owns one-shot backlog/icebox/pending queue-sync request
  collection and sync-report policy for `agent-doc queue sync`; `queue_cmd`
  keeps only file IO, rendering, snapshot persistence, and CLI output.
- `agent-doc-queue` now owns id-backed queue-head closeout/removal decisions and
  free-text queue-head provenance decisions used by `session_check`, including
  committed queue presence, completed-residue detection, normalized/sorted
  reaped directive-head id intersection, response-proof classification,
  suppression-marker residue, and the response-heading plausibility heuristic.
  Orchestration keeps guard-mode formatting,
  cycle-state loading, ops logging, and effectful response-materialization
  checks owned by `agent-doc-turn`.
- `agent-doc-queue` now owns queue consumption entry policy used by
  `write/queue_consume`: selecting prompt texts, counting leading done-id
  consumes, matching prompt text with cosmetic priority markers ignored,
  matching response headings to the active queue head, marking entries completed
  by done ids, normalizing consumed done-id bags, and free-text struck-head
  annotation policy: the deterministic strike note, list-bullet stripping, and
  newly struck free-text queue-head annotation over queue AST node ranges.
  Orchestration keeps document locks, file/snapshot IO, IPC node ops,
  response-echo embedding, synthetic-head tracked-item authorization, ops
  logging, and editor convergence.
- `agent-doc-queue` now also owns the preflight resolved-id queue strike
  rewrite by direct reuse of `mark_entries_completed_by_done_ids`, including
  non-head resolved references and review-gated ids supplied by the caller.
  `agent-doc-element-review` owns gated review id extraction from
  `agent:review`. Preflight keeps document parsing context, done/archive id
  gathering, queue activation state, snapshot comparison, and diagnostics while
  importing both focused helpers directly; the old orchestration helper names
  were deleted instead of retained as shims.
- `agent-doc-queue` now owns queue prompt drift/drop accounting used by
  `write` and IPC adoption guards: queue component extraction, active prompt
  text extraction, consumed/struck prompt coverage, baseline-vs-live deletion
  detection, dropped candidate prompt detection, and content-ours preservation
  over unproven live queue deletions. Orchestration keeps response merge
  decisions, snapshot/file/editor adapters, repair redelivery, and ops logging
  while importing the focused queue policy directly.
- `agent-doc-queue` now owns preflight queue convergence policy: re-aligning a
  pre-maintenance baseline to the post-maintenance queue component, carrying
  in-progress-marker-only backlog/pending/icebox convergence, and classifying
  non-selected future queue-body edits while ignoring exchange boundary churn.
  Preflight keeps queue maintenance, snapshot loading/saving, baseline file IO,
  and diagnostics while importing the focused queue convergence helpers
  directly; the old local helper cluster and pure test were deleted from
  orchestration instead of kept as facades.
- `agent-doc-queue::queue_projection` now owns queue-entry projection and
  worklist policy used by preflight: extracting done ids from prompt/completed
  entries, building active prompt projection rows with noise/deferred filters,
  detecting in-progress retarget requests, deriving selected queue-head node
  keys, hashing rendered queue worklists, and projecting prompt/preset/dispatch
  worklist facts. Preflight keeps only file/project-root lookup, state-event
  mapping, ops-log emission, and active/inactive decision placement while
  calling the focused queue helpers directly.
- `agent-doc-queue::queue_directive` now owns preflight's directive lifecycle
  expectation input filter: normalizing raw `do #id` target ids, intersecting
  with open backlog ids, excluding ids auto-populated by backlog-to-queue sync,
  and preserving first-seen order. Preflight keeps live backlog file IO,
  queue-state collection, and cycle-state recording while calling the focused
  helper directly.
- `agent-doc-queue` now owns effective queue-continuation output policy:
  applying recycle-yield suppression, recycle guidance, required-continuation
  guidance, and queue-pause preamble selection after orchestration supplies
  effect-derived facts. Preflight keeps stale-supervisor/recycle-yield detection
  and queue-state gathering while importing the focused output resolver
  directly.
- `agent-doc-queue` now owns backlog-to-queue tombstone reconciliation policy:
  adding operator-deleted active ids when they disappear from the live queue and
  clearing tombstones when ids are re-added as active queue heads. Preflight
  keeps only tombstone sidecar path/load/save effects and calls the focused
  reconciler directly from an explicit file-backed adapter.
- `agent-doc-queue` now owns component-attribute warning policy for queue-only
  markers, backlog/pending queue-sync attributes, icebox queue misuse, priority
  component attributes, and typo detection. Preflight keeps only
  `PreflightWarning` output shaping and file-path message prefixing.
- `agent-doc-queue` now owns crash-durable queue-journal replay/dedupe policy:
  extracting unique live-buffer queue prompts across editor snapshots and
  suppressing replay when live or durable queue content already represents a
  prompt. Orchestration keeps journal path resolution, JSONL sidecar IO, fsync,
  live-buffer sidecar reads, snapshot reads, and clear/remove effects while
  calling the focused queue policy directly.
- `agent-doc-queue` also owns replay-neutralized queue-addition commit policy:
  queue-entry commit signatures, entry count comparison, and deciding when
  queue-only visible drift preserved an operator-added prompt even though
  replay-hash normalization neutralized the queue component. Git closeout keeps
  HEAD/snapshot/file reads, commit creation, and ops-log formatting while calling
  the focused queue policy directly.
- `agent-doc-queue` now owns drain-owner lease policy and sidecar storage for
  queue drains: the Claude `/loop` owner tag, persisted lease schema, TTL
  resolution, freshness filtering, loop-owner filtering, and read/write/clear
  sidecar operations live with the queue-drain domain. Orchestration and the
  root CLI call `agent_doc_queue::drain_owner` directly; the old
  `agent-doc-orchestration/src/drain_owner.rs` module was deleted rather than
  retained as a facade.
- `agent-doc-queue` now owns context-clear in-flight marker policy for queue
  drains: the marker TTL, stale-marker predicate, and post-clear settle tick
  transition live beside the existing idle-queue context-clear decision table.
  Orchestration keeps marker path/JSON IO and watcher side effects while calling
  the focused queue policy directly.
- `agent-doc-queue` now owns explicit queue-head classification for
  `agent-doc queue consume`: no-head, free-text, and id-backed head decisions
  live with active queue-head projection. Orchestration keeps file reading,
  queue-edit lease ownership, write-layer strike calls, and CLI diagnostics while
  calling the focused classifier directly.
- `agent-doc-queue` now owns route queue-dispatch policy as a whole layer:
  prompt-text extraction from routed changes, operator-priority pinning,
  preemptive queue insertion/single-auto supersession, queue component creation,
  route activation content transforms, inactive queue-head selection, marker
  `go`/`start`/`stop` handling, committed-snapshot head backing checks, and
  active-turn blocker source classification for queued dispatch-only reroutes.
  Route keeps only file/frontmatter/snapshot adapters, queue locks, editor
  convergence, snapshot writes, pane inspection, and ops-log diagnostics.
- `agent-doc-queue` also owns the active registered-preset queue-head
  classifier used by spent-preset pause repair. Project-controller RPC now keeps
  only the file-backed pause-repair adapter and calls the queue-owned classifier
  directly.
- `agent-doc-controller::dispatch` now owns dispatch diagnostic payload parsing
  and proof-payload joining. Project-controller RPC still gathers file-backed
  proof facts, but it no longer keeps local parser/formatter helpers for that
  pure payload policy.
- `agent-doc-supervisor::config` now owns the pure agent-doc dogfood session
  path policy used by supervisor auto-install. Project-controller RPC keeps only
  canonicalization, project-root discovery, Cargo.toml validation, and file IO.
- `agent-doc-queue::route_dispatch` now owns active auto-route queue prompt
  extraction and prompt-text normalization used by preflight route-queue
  commit-boundary recovery. Preflight keeps only cycle-state/snapshot/git
  recovery orchestration.
- `agent-doc-document::transient_markers` now owns transient document marker
  normalization: boundary stripping, `(HEAD)` heading cleanup, guard-marker
  stripping, pipeline-frontmatter normalization, and post-commit `Re:` heading
  attribution normalization. It also owns replay-hash document normalization:
  neutralizing queue component churn and queue activation frontmatter while
  preserving real response-body changes for stale-cycle hash comparisons. It
  also owns pure post-commit document repair policy for exchange prompt-prefix
  equivalence and stale committed-response
  collapse: comparing exchange prompt-prefix-normalized documents, detecting
  collapsed committed response headings, and producing the log-free document
  repair that restores the committed exchange while preserving other component
  drift. Git, preflight, session-check, realtime policy, repair, graph, and
  write paths call the focused document crate directly while keeping file, git,
  snapshot, and ops-log adapters outside the pure policy.
- `agent-doc-prompt-lines` now owns pure prompt/response line classification
  below diff, document, exchange, turn, supervisor, and orchestration:
  slash-command candidates, prompt-target detection, prompt-prefix repair
  starts, plain-response label detection, imperative directive normalization,
  approval words, and markdown-list parsing. This became its own focused crate
  instead of `agent-doc-document` because document depends on the element
  registry, and exchange elements also need these classifiers; a document-owned
  helper would create a dependency cycle. The old diff/exchange/document helper
  paths were deleted rather than kept as compatibility facades.
- `agent-doc-document::commit_normalization` now owns pure committed document
  normalization: answered-prompt prefix canonicalization, committed exchange
  artifact normalization, component-content absorb normalization, and component
  redaction for absorb comparison. Git, closeout/session-check, repair,
  write-converge, and realtime write policy call the focused document helper
  directly while keeping git/file/snapshot decisions in their adapters. The old
  `agent-doc-orchestration/src/git/normalize.rs` module was deleted rather than
  retained as a shim.
- `agent-doc-template::response_materialization` now owns more template
  response-materialization policy: constructing materialization probes from raw
  template responses, stripping transient guard markers from probes, removing
  partial response materialization from exchange content, and sanitizing
  patchback responses through replay-guard classification. The write layer keeps
  retry/file/ops-log adapters and imports the focused template helpers directly
  instead of preserving `write::materialize` wrappers.
- `agent-doc-template::stale_baseline` now owns stale-baseline template policy:
  default append-component classification for `exchange` and `findings`,
  boundary-insensitive comparison, append-vs-replace component filtering,
  missing append component/content detection, empty append-content skips, and
  non-template prefix fallback. Write/git paths keep git HEAD lookup, rebase
  decision placement, retry/file/ops-log adapters, and tests that exercise the
  effect boundary while importing the focused policy directly.
- `agent-doc-element-exchange` now owns pure exchange element prompt/content
  helpers and prefix-repair normalization policy: exchange body lookup/length,
  prompt-line normalization and counting, line-segment splitting,
  markdown-heading filtering, code-fence delimiter detection, response-heading
  classification, normalization target counting/matching, exchange user-region
  scanning, write-cycle normalization target extraction, pure
  snapshot-vs-baseline user prompt prefix normalization, committed HEAD
  prefix-state preservation, sidecar prefix verification, post-commit
  missing-prefix target extraction, and targeted exchange prefix repair. It also
  owns pure exchange prompt-reconciliation policy:
  response-aware prompt counts, prompt-growth detection, live exchange edit
  detection, prefix counts, duplicate prompt text detection, reconciliation line
  records, boundary-tail lookup, live prompt prefix-variant classification, and
  pure exchange-content repairs for tail prefix variants, adjacent
  prefixed/unprefixed duplicates, and before/after prompt-count excess. It now
  also owns response-before-prompt exchange order detection and log-free repair
  planning, including response signature matching, prompt-run detection after
  boundaries, prompt-authority checks, and boundary marker preservation or
  regeneration. `write`, `write/exchange_reconcile`, `write/normalize`, git
  closeout, repair, and IPC paths keep component replacement,
  duplicate-repair orchestration, shrink-guard, file, git, ops-log, snapshot,
  normalization safety threshold/logging, and delivery adapters while importing
  the focused exchange helpers directly.
- `agent-doc-element-backlog` already owned open backlog id extraction; preflight
  now calls `open_backlog_ids_in_content` directly for `do #id` closeout
  evidence instead of re-parsing backlog components locally. It also owns pure
  tracked-work target id normalization/extraction from
  backlog/review/done/icebox component bodies, including preferred-component
  selection and fallback to active backlog or other tracked-work components. It
  now also owns tracked-surface maintenance policy used by preflight and
  ops-proof: backlog/pending alias matching, surface labels, which surfaces may
  auto-reap already-done mirrors or ops-proof completions, backlog-body reorder
  projection, and review open/gated counts. Write-side pending checks,
  cycle-state checkpoint normalization, preflight, and ops-proof keep file IO,
  persisted-cycle state, baselines, snapshot reconciliation, ops-log emission,
  and guard formatting while calling the focused backlog parser directly. It
  now also owns completed tracked-item projection used by preflight and
  session-check: body-level completed item extraction, document/component-level
  tracked-item scanning, completed-item reference formatting, and the
  parse-error-aware no-completed-items guard. Preflight and session-check keep
  phase-specific interruption wording and effect-derived content/component
  sources while calling the focused backlog helpers directly.
- `agent-doc-turn` now owns committed-without-response-body closeout policy:
  detecting assistant `### Re:` response bodies in `agent:exchange` and deciding
  whether committed cycles with no visible response body pass, skip as no-op
  commits, or interrupt. Orchestration keeps cycle-state loading, file parsing,
  ops-log emission, side-effect note rendering, and recovery-message formatting.
- `agent-doc-turn` now owns carry-forward closeout signal policy used by write
  conflict tolerance: detecting prompt targets, questions, `dispatch`/`fix`/`do`
  directives, `spec-test` hints, hashtags, markdown queue mirrors, checkboxes,
  and pinned queue entries as next-cycle instructions. Write keeps only
  baseline/current diffing, exchange-scope checks, merge proof, and snapshot
  persistence decisions while importing the focused classifier directly.
- `agent-doc-turn` now owns partial `do [#id]` closeout-state policy: shipped
  response detection, suppress-marker handling, directed-id candidate filtering,
  and warn/pass decisions for open unresolved backlog ids. Orchestration keeps
  cycle/capture/backlog loading, ops-log emission, and session-check warning
  rendering, using current `--backlog-edit` / `--backlog-gate` terminology in
  the touched hint instead of legacy pending aliases.
- `agent-doc-turn` now owns blocked-closeout follow-up policy: blocked-signal
  response detection, no-follow-up and suppress-marker handling, gated/open/done
  id filtering, and warn/pass decisions for directed ids whose remaining work
  would otherwise live only in prose. Orchestration keeps guard-mode resolution,
  cycle/capture/review loading, ops-log emission, and strict-vs-warn formatting,
  with touched hints moved to `--backlog-edit` / `--backlog-add-after`.
- `agent-doc-turn` now owns gated-phase split advisory policy: suppress-marker
  handling, kept-open/directed-id filtering, multi-phase body detection, child-id
  breakout recognition, and warn/pass decisions for kept-open tracked items.
  Orchestration keeps cycle/capture loading, cached component parsing,
  tracked-item evidence projection, ops-log emission, and warning rendering.
- `agent-doc-turn` now owns queue-audit partial-completion advisory policy:
  committed-cycle gating, suppress-marker handling, response-text extraction,
  none-complete collapse detection, and warn/pass decisions. Orchestration keeps
  cycle/capture loading, ops-log emission, and warning rendering.
- `agent-doc-turn` now owns tracked-work completion missing-`--done` policy:
  pending-done suppression, response-text extraction, recorded-done and kept-open
  filtering, response-heading completion detection, and missing-id decisions.
  `agent-doc-element-backlog` owns document-content open tracked-work and open
  backlog id parsing. Orchestration keeps file IO, cycle/capture loading,
  malformed-item checks, auto-done mutation, guard-mode formatting, and warning
  rendering.
- `agent-doc-turn` now owns expect-done-or-gate closeout policy for `do #id`
  directive targets: pending-done suppression, open/committed-cycle gating,
  done/kept-open/reaped resolution, open-backlog intersection, and unresolved-id
  decisions. Orchestration keeps cycle/capture loading, open-backlog file IO,
  ops-log emission, guard-mode formatting, and warning rendering.
- `agent-doc-frontmatter` now owns pending-capture, pending-done, review-done,
  and auto-done guard precedence/default resolution across document
  frontmatter and project config. Orchestration keeps only file-backed and
  cached-context adapters that supply parsed frontmatter/config facts to the
  focused resolver, and unused cached review/auto adapters were deleted instead
  of kept as no-shim dead paths.
- `agent-doc-frontmatter` now owns cross-document security-review policy for
  shared collaboration documents: same-document bypass, shared source/target
  missing-review classification, and blank-review handling. Orchestration keeps
  only path canonicalization, source/target display labels, and user-facing
  error formatting while importing the focused decision directly.
- `agent-doc-turn` now owns closeout recovery evidence-to-state
  classification and closeout content-component signature policy. Orchestration
  gathers cycle/snapshot/HEAD/visible/submodule facts, classifies file-backed
  drift evidence, and calls the focused classifier; the old public
  `classify_closeout_recovery_state` path was replaced with the explicit
  file-backed adapter `classify_closeout_recovery_state_for_file`.
- `agent-doc-turn` now owns closeout recovery command rendering: mapping typed
  recovery states plus optional open-cycle checkpoint facts into user-facing
  recovery instructions. Orchestration keeps only document-path formatting and
  cycle-state sidecar loading/projection through
  `closeout_recovery_command_for_file`, avoiding a duplicate policy facade.
- `agent-doc-turn` now also owns closeout recovery recommendation projection:
  extracting the short copy-pasteable `agent-doc ...` command from a longer
  recovery recommendation. Route keeps only blocked-outcome field formatting
  and imports the focused parser directly; the route-local helper and test were
  deleted rather than left as a shim.
- `agent-doc-turn` now owns no-change cycle diagnostics: classifying abandoned
  or bookkeeping-only committed cycle-state facts into clean vs abnormal
  no-change verdicts, including recursive direct-invocation and no-response
  recovery messages. Orchestration keeps only snapshot comparison, cycle-state
  file loading, fact projection, and CLI stderr rendering; the old local
  `run.rs` verdict enum/classifier was deleted rather than retained as a
  wrapper.
- `agent-doc-turn` now owns owner-pane recursion diagnostics: the operator
  guidance for recursive direct invocation, recursive start, unresolved prompt
  miss, active auto-queue handoff, and repeated queue-wedge halt. Orchestration
  keeps Codex/tmux/session ownership detection, queue state checks, auto-queue
  halt effects, cycle abandonment, ops-log emission, and fact projection; the
  old `run.rs` message-builder functions were deleted instead of kept as local
  facades.
- `agent-doc-turn-executor` now owns Codex launch transport rejection policy:
  detecting 403 WebSocket / 429 HTTPS fallback failures and formatting the
  retry/restart diagnostic. Orchestration's Codex backend keeps process launch,
  stderr filtering, stream iteration, and error propagation while importing the
  focused `codex_launch` helpers directly; the old local transport
  classifier/formatter functions were deleted rather than wrapped.
- `agent-doc-turn-executor` now owns child capability-probe failure
  classification: network DNS/sandbox/refusal/timeout, required-SSH
  usage/sandbox/DNS/timeout, and writable-root sandbox/index-lock/missing-target
  messages. Orchestration's Codex backend keeps child process execution, marker
  validation, OpenCode usage-output detection/ANSI stripping, stdout/stderr
  fact extraction, and error propagation while importing the focused
  `codex_launch` helpers directly; the old local classifier functions were
  deleted instead of retained as facades.
- `agent-doc-turn-executor` now owns Codex resume/required-SSH transcript text
  policy: local browser/CDP capability-drift detection, resume retry notice
  text, SSH DNS/network/auth/config recognizers, command-execution SSH failure
  extraction, transcript success proof, and required-SSH failure formatting.
  Orchestration's Codex backend keeps SSH target resolution, isolated probe
  execution, stream/process lifecycle, retry effects, and fact projection while
  importing the focused `codex_launch` helpers directly; the old local transcript
  helper functions and pure tests were deleted from orchestration rather than
  kept as facades.
- `agent-doc-turn-executor` now owns Codex stderr noise filtering policy:
  marketplace plugin manifest default-prompt warning suppression, skill icon
  warning suppression, filtered stderr preservation, and suppression counts.
  Orchestration's Codex backend keeps process stderr collection and error
  surfacing while importing the focused `codex_launch` filter directly; the old
  local report/filter helpers and pure tests were deleted instead of retained as
  facades.
- `agent-doc-turn-executor` now owns Codex resume argv normalization:
  translating legacy sandbox flags to `sandbox_mode` config, stripping
  unsupported writable-root args for resumed sessions, preserving unrelated
  exec flags, and rejecting malformed/conflicting sandbox/config inputs.
  Orchestration's Codex backend builds the `Command`, appends model/env
  effects, and calls the focused argv helper directly without a local mutating
  wrapper.
- `agent-doc-turn-executor` now owns Codex network launch policy: resolving
  frontmatter/config network-access precedence, applying child env mutations,
  deriving effective network status from launch args and explicit parent-env
  facts, and formatting mismatch diagnostics. Start/orchestrate/Codex backend
  code keeps only env fact projection, config/frontmatter fact extraction, and
  child-process launch effects while importing the focused `codex_launch`
  helpers directly; the old `agent_doc_orchestration::agent::*` network policy
  path was deleted instead of retained as a facade.
- `agent-doc-tmux` now owns sync column-memory layout policy and safe-passive
  focus-only expansion policy: restoring remembered sibling panes, preserving
  duplicate-column layout state, returning restoration/expansion events for
  orchestration logging, and honoring exact-visible editor projections.
  Orchestration keeps only frontmatter/file-backed column classification,
  focused-column resolution, tmux queries, and ops-log emission; the old
  orchestration policy functions were deleted instead of kept as wrappers.
- `agent-doc-tmux` now owns destructive sync-repair policy: deciding when the
  rescue phase is already converged for target/stash state, naming the minimum
  interval between destructive repair passes, and classifying timestamp facts
  into throttle/run decisions. Orchestration keeps stamp-file IO, clocks, logs,
  tmux command effects, and user-facing throttle output while calling the
  focused tmux policy directly.
- `agent-doc-tmux` now owns pane foreground-process classification policy:
  classifying `#{pane_current_command}` as agent, idle shell, foreign, or
  transient, and folding a brief sample window into a stable process decision.
  Resync keeps tmux sampling, sleeps, registry fixes, pane moves, and kill
  effects while importing the focused classifier directly; the old
  `AGENT_PROCESSES` / `IDLE_SHELLS` arrays and local `PaneProcessKind` were
  deleted instead of preserved as wrappers.
- `agent-doc-supervisor::input` now owns pure supervisor input byte policy:
  filtering inherited Ctrl-D bytes before the first visible prompt, normalizing
  injected supervisor input from LF/CRLF to carriage returns, and summarizing
  operator prompt input for logs. `start.rs` keeps PTY reads/writes, prompt
  visibility facts, and session logging while importing the focused byte
  helpers directly instead of retaining local wrappers.
- `agent-doc-hash` now also owns compact diagnostic SHA-256 prefixes through
  `short_content_hash`. Route dispatch and pane-snapshot logging keep capture,
  redaction, and file IO effects while calling the focused hash helper directly;
  the old route-local short-hash helper was deleted.
- `agent-doc-topic` now owns compact exchange summary policy: summarizing
  archived response topics, prior compact summaries, trailing prompt context,
  whitespace collapse, and truncation. `compact.rs` keeps archive construction,
  component writes, and safety guards while calling the focused topic summarizer
  directly; the old local summarizer helpers were removed rather than wrapped.
- `agent-doc-session-accretion` now owns context-reset policy: resolving the
  default-off queue context-reset opt-in from frontmatter/project facts,
  resolving and clamping `/clear` context thresholds, and formatting recent
  compaction/session-accretion reset reasons from already-collected facts.
  Orchestration keeps document/config/log IO, startup-miss state, RunContext
  projection, and calls the focused policy directly without re-owning the
  decision table.
- `agent-doc-ipc-protocol` now owns plugin IPC ack classification: terminal ok,
  early pending/accepted, already-applied dedupe, failed error, and malformed
  legacy ack behavior. It also owns early-ack protocol vocabulary: tagging patch
  messages with `early_ack`, detecting incoming early-ack requests, emitting the
  canonical pending ack line, and naming the early-ack proof marker. `ipc_socket`
  keeps socket connection/listener mechanics, timeout handling, listener-thread
  state, and error wrapping; root simulator/tests import the focused protocol
  crate directly instead of the old
  `agent_doc_orchestration::ipc_socket::{classify_ack,AckClassification}` path.
- `agent-doc-document-realtime` now owns full-content visible replacement scope
  policy: rejecting whole-document editor replacement for template frontmatter
  and agent component markers across target/source/current buffers. The IPC
  transport keeps only mode-specific logging and file/effect control while
  calling the focused realtime write-policy helper directly.
- `agent-doc-document-realtime` now owns stale snapshot reset-drift
  discrimination: stripping transient boundary markers, applying reset-drift
  length thresholds, and suppressing safe agent-doc mutations before the
  effectful guard decides whether to refresh sidecars or fail closed.
  Orchestration keeps snapshot/CRDT sidecar writes, ops-log emission, and
  provenance-backed visible rebase effects.
- `agent-doc-document-realtime` now owns write snapshot/live-drift persistence
  policy: choosing whether snapshots persist final content or `content_ours`,
  classifying IPC snapshot adoption that would absorb live prompt drift,
  detecting unowned dropped exchange prompt lines, extracting exchange content
  for write-drift comparisons, and deciding whether a response target is
  disjoint from a concurrent user edit. The git-backed three-way merge remains
  an orchestration effect adapter supplied as a callback, so the focused crate
  owns the decision table without spawning git or preserving the old
  `agent_doc_orchestration::write::*` policy path.
- `agent-doc-controller` now owns route-trigger current-draft visibility
  policy: matching visible trigger text, wrapped trigger captures, and
  equivalent relative/absolute `agent-doc <path>` drafts while using a caller
  supplied prompt-line predicate. Route/start keep pane capture, harness
  predicates, diagnostics, and submit/resubmit effects while calling the focused
  controller policy directly.
- `agent-doc-controller` now owns editor route-error diagnostic naming policy:
  the diagnostics state directory, sanitized per-document diagnostic name, and
  `.txt` filename rendering. Orchestration keeps canonicalization, project-root
  lookup, saved-diagnostic deletion, and ops-log emission while importing the
  focused naming helpers directly.
- `agent-doc-controller` now owns fleet dashboard model/render policy: admin
  actor/finding rows, dashboard row diagnostics, deterministic model folding,
  terminal-frame rendering, queue/projection flag formatting, and truncation.
  Orchestration keeps actor-store/session-registry/tmux/controller inspection
  effects, JSON output, and the live polling loop while importing the focused
  fleet policy directly.
- `agent-doc-turn-executor-tmux` now owns context-clear submit policy: active
  composer visibility detection for `/clear` and OpenCode `/new`, submitted
  observation/status vocabulary, Enter-resubmit gating, and clear-submit
  observation/resubmit/blocked line rendering. The root CLI keeps pane capture,
  retry delivery, ops-log writes, and error raising; start detection keeps
  supervisor payload decisions and harness predicates. Both callers import the
  focused policy directly with no local shim or `clear_direct_*` compatibility
  names.
- `agent-doc-turn-executor-tmux` now also owns Codex prompt-placeholder parsing
  policy: idle placeholder recognition, wrapped placeholder recovery,
  safe-placeholder token filtering, and dim-vs-RGB ANSI classification for
  placeholder text. Orchestration's harness keeps only harness-specific routing
  decisions and protected-input wording while importing the focused prompt
  parser directly.
- `agent-doc-element-exchange` now owns response-body prompt-prefix repair for
  leaked harness `❯ ` markers inside committed `### Re:` response bodies,
  including leading-run stripping, late assistant proof/list stripping, and
  preservation of inner quoted prompt-like prose. Write/repair keep only CRDT
  merge normalization sequencing, snapshot suppression, disk writes, and
  ops-log effects while importing the focused exchange helper directly; the
  old `write::strip_prompt_prefix_from_response_body_first_lines` path was
  deleted rather than kept as a shim.
- `agent-doc-element-boundary` now owns document-level boundary ID lookup:
  parsing the target component, scanning for `<!-- agent:boundary:... -->`,
  and ignoring marker-looking text inside code ranges. Write IPC and git
  cleanup keep patch/file/commit delivery effects while importing the focused
  boundary helper directly; the old `write::find_boundary_id` path was deleted
  instead of preserved as a facade.
- `agent-doc-document-realtime` now owns response-delta already-applied
  detection for IPC/CRDT retry paths: normalizing exchange component content,
  computing actionable inserted response hunks from `base -> content_ours`, and
  requiring those hunks to already appear contiguously in the current visible
  document before adopting it. Write and IPC modules keep timeout, sidecar,
  snapshot, and normalization effects while importing the focused detector
  directly; the old `write.rs` helper cluster was deleted instead of preserved
  as a facade.
- Facade cleanup now removes low-value re-export and deprecated-name surfaces:
  `agent-doc-turn-executor-tmux` no longer re-exports tmux model types from
  `agent-doc-tmux`; `agent-doc-element-registry` no longer re-exports base
  element vocabulary from `agent-doc-element`; `agent-doc-document` imports base
  element types directly; `agent-doc-diff` removed the
  `extract_required_response_blocks` / `format_required_response_targets`
  compatibility wrappers in favor of current prompt-bearing names; and
  `agent-doc-element-backlog` removed the width-4 `generate_hash` wrapper in
  favor of `generate_hash_n(..., 4)`. Static guards now reject these old Rust
  API names so the no-shim policy applies to cleanup work as well as new
  extractions.
- A four-worker fan-out extracted another pure-helper layer with disjoint write
  scopes. `agent-doc-template::todo_patch_guard` now owns destructive
  `patch:todo` checklist-count guard policy while write orchestration imports it
  directly. `agent-doc-turn::response_replay` now owns captured-response
  materialization detection, including template response probes and transient
  marker normalization; orchestration, git, repair, and closeout callers use the
  focused turn API directly. `agent-doc-queue::document_queue` no longer
  re-exports queue projection marker helpers; queue/orchestration/plan callers
  import marker policy from `agent-doc-document::queue_projection` directly.
  `agent-doc-markdown-ast` removed deprecated id-keyed mutation wrappers in
  favor of node-key APIs, and watch tests now call the current discovery API
  directly instead of a test-only compatibility helper. Static guards cover all
  removed Rust API names.
- The next fan-out pass extracted harness policy and removed greenfield-dead
  compatibility surfaces. New `agent-doc-harness` owns `HarnessConfig`, prompt
  and chrome classification, busy/idle blocker detection, and harness restart
  arguments; orchestration callers import it directly and the old
  `agent-doc-orchestration::harness` facade module is deleted. The same pass
  retires remaining obsolete active API names instead of preserving aliases:
  preflight JSON exposes `user_intent_prompt_changes` without
  `prompt_bearing_changes`, `replace:pending` is the only pending replacement
  block syntax, `--allow-patch-pending` / `AGENT_DOC_ALLOW_PATCH_PENDING` are
  removed, and tracked-work completion uses `--done` only rather than the old
  `--pending-done` / `--backlog-done` spellings. Boundary tests assert the new
  owner crate and reject the removed surfaces.
- The follow-on pure-helper pass pulled another layer out of orchestration while
  waiting on fan-out extractors. `agent-doc-element-exchange` now owns exchange
  shrink-block decisions, live-ack retry predicates, document-level prompt-line
  dedupe, and live prompt-prefix dedupe; `agent-doc-prompt-contract` owns the
  prompt accumulator helpers used by preflight semantic diffing;
  `agent-doc-harness` owns output-prompt visibility for prompt/chrome
  classification; `agent-doc-turn-executor-tmux` owns OpenCode permission prompt
  stdin normalization and key translation; and `agent-doc-controller` owns the
  direct-pane Enter-resubmit retry budget parser. Orchestration imports each
  focused owner directly and keeps only file IO, pane capture, env lookup,
  supervisor state, and ops-log adapters.
- The next fan-out pass continued the same no-shim pattern. Exchange snapshot
  stripping moved from `claim.rs` to `agent-doc-element-exchange`;
  cross-document owner command-line/path matching moved from `sync.rs` to
  `agent-doc-controller::command_line`; pure git relative-path, output
  rendering, and index-lock text policy moved to the new `agent-doc-git` crate;
  harness-name normalization moved to `agent-doc-harness`; and ownership
  generation / transition-event parsing and rendering moved to
  `agent-doc-supervisor`. Orchestration imports those focused APIs directly and
  keeps only the surrounding snapshot, process, registry, pane, git-command, and
  actor-store effects.
- The follow-up local seam moved exchange-only post-commit IPC reposition safety
  from git orchestration into `agent-doc-element-exchange`. The focused crate
  now owns exchange-component redaction and the "only exchange changed" predicate
  with the regression tests; git orchestration only reads `HEAD^`/`HEAD` and
  decides whether to send the existing IPC signal. No compatibility wrapper was
  kept in `git.rs`.
- The same local pass moved queue context-clear in-flight marker payload
  construction into `agent-doc-queue`. The queue crate now owns the serializable
  marker body and head hash/length derivation; orchestration only resolves the
  project-scoped marker path, reads/writes JSON, and removes stale sidecar files.
  Static guards reject moving the payload fields back into the storage adapter.
- The transcript context adapter also shed one more parser seam:
  `agent-doc-model-tier` now owns Codex `session_meta.cwd` JSONL parsing for
  transcript/project matching. Orchestration still walks the Codex transcript
  tree and reads candidate files, but it no longer owns the JSON shape.
- The same context adapter now delegates transcript-content context percentage
  policy to `agent-doc-model-tier` as well. The focused crate owns the
  Claude/Codex/OpenCode parsing decision, unknown-model diagnostic reason, and
  unsupported-harness safe miss; orchestration only reads transcript files and
  emits the existing operator warning text.
- Drain-stall sidecar ownership was tightened as well: `agent-doc-turn` now
  owns continuation-pending marker construction, while orchestration keeps only
  path lookup, current-time capture, serialization, and sidecar file IO.
- Tmux input diagnostics now have separate pure/effect layers:
  `agent-doc-tmux-commands` owns the structured key/payload/byte/transform
  formatting policy, and `agent-doc-tmux-io` owns the effectful stderr plus
  ops-log adapter through an injected sink. Root and orchestration callers import
  the focused tmux IO adapter directly; the old
  `agent-doc-orchestration::input_diag` module was deleted rather than kept as a
  compatibility facade.
- The next fan-out/local integration pass pulled three more seams out of
  orchestration. `agent-doc-frontmatter-io::session` now owns file/config-backed
  frontmatter parsing, session-id reads, session injection, and document opt-in
  gate adapters; orchestration keeps only the `RunContext`-backed adapter.
  `agent-doc-supervisor-process::start_command` owns route-owned supervisor start
  command rendering and shell quoting; project-controller RPC imports it instead
  of keeping local command-rendering helpers. `agent-doc-supervisor` now owns the
  stateful PTY terminal escape filter and trace vocabulary; orchestration's PTY
  layer only selects harness-specific config and emits verbose trace logs.
- Route-submit sidecar policy was tightened too: `agent-doc-supervisor` now owns
  route-submit marker JSON serialization plus fresh/stale/malformed
  classification for in-flight and blocked markers. Orchestration keeps only
  marker path resolution, file reads/writes/removal, guard drop cleanup, and
  ops-log diagnostics.
- Queue preemption sidecar policy now owns deferred operator-clear marker
  payload construction and JSON parsing in `agent-doc-queue`; orchestration only
  resolves project-scoped marker paths, reads/writes/removes sidecar files, and
  lets the supervisor idle-watch deliver the recorded clear command.
- Tmux layout policy gained the editor-column split classifier:
  `agent-doc-tmux` now owns `is_first_column`, while route orchestration only
  forwards file/column facts to decide split-before placement.
- Startup path normalization for narrowed pane cwd now belongs to `agent-doc-fs`:
  `rewrite_start_path` rewrites super-root-relative document paths to cwd-relative
  paths for submodule panes, while route startup only provides the resolved cwd
  and original CLI path.
- Supervisor reexec ladder policy now belongs to `agent-doc-supervisor`:
  `build_reexec_candidates` owns the ordered, de-duplicated fresh/current/PATH
  candidate list, while `start.rs` only gathers current-exe and installed-binary
  facts before running the Unix `execve` adapter.

### Earlier closure finding

Analyzing the `sessions`/`supervisor` closure for the old Wave 1b invalidated
the original bottom-up module move:

- `sessions`/`supervisor` are leaves only by *intra-cluster* deps. Their full
  `crate::` closure pulls in `input_diag`, `env`, `harness`, `project_controller`,
  `session_actor`, `startup_miss`, `test_support`, then transitively
  `ops_log`, `config`, `prompt`, `fs_util`, `snapshot`, `harness_prompt`.
- **`snapshot` → `git`.** `snapshot` (a shared I/O utility) depends on `git`,
  a cluster module. So the closure transitively reaches `git` and, through it,
  the rest of the cluster. There is no small self-contained leaf set beyond
  `ipc_socket`.
- **Shared-util breadth:** `snapshot` is used by **35** files, `ops_log` by
  **27**, `config` by **11** — across *both* CLI commands and orchestration.
  They cannot move to pure `core` (they do `std::fs`), and orchestration cannot
  depend back on `agent-doc` (circular).

### Options

The current decision is not "one crate or no extraction"; the options are how to
finish shrinking the transitional orchestration crate.

- **Option A — keep one large orchestration adapter.** Stop extracting after the
  CLI shell split and treat `agent-doc-orchestration` as the broad adapter for
  route/git/sessions/supervisor/write/realtime concerns. This minimizes churn,
  but preserves the God-crate shape and keeps ownership boundaries social rather
  than enforced by Rust crate dependencies.
- **Option B — domain-specific detangling (recommended).** Continue extracting
  each policy seam into the focused crate that owns the domain, creating a new
  focused crate only when no destination exists. This is cleaner long term
  because `agent-doc-orchestration` is heavily tangled, not despite that fact.
  The tangle is mostly effect coordination, process/session ownership,
  git/tmux/IPC adapters, and historical cross-calls. Keeping domain policy in
  that crate treats the tangle as the API: every queue, realtime, workflow,
  graph, tmux, session, and hashing decision inherits the broad orchestration
  dependency graph and the old cross-call habits. Detangling into domain crates
  treats the tangle as implementation detail: orchestration gathers facts and
  performs effects, while policy lives where its vocabulary belongs. Moving
  queue policy to `agent-doc-queue`, realtime/write vocabulary to
  `agent-doc-document-realtime`, tmux decisions to `agent-doc-tmux-*`,
  workflow/session-cycle decisions to `agent-doc-workflow`, work graph decisions
  to `agent-doc-work-graph`, prompt/context-pack decisions to
  `agent-doc-prompt-context`, and generic content hashes to `agent-doc-hash`
  turns implicit social ownership into Rust-enforced dependency boundaries. The
  no-shim policy is what keeps this from becoming another facade layer: call
  sites must name the owning domain crate, so ownership stays visible in imports,
  tests, manifests, and future refactors.
- **Option C — shared base layer.** Create a lower `agent-doc-base` /
  `agent-doc-io` style crate for shared file/path/log utilities. This can be
  useful tactically for IO helpers, but by itself it does not detangle domain
  policy. It also remains blocked wherever utilities depend upward into
  orchestration behavior, such as the historical `snapshot -> git` coupling,
  unless those dependencies are inverted first.
- **Option D — defer.** Keep extracting only opportunistically when a local
  feature or bugfix touches a seam. This reduces immediate risk, but leaves the
  objective incomplete and lets orchestration continue to accumulate policy.

**Recommendation:** Option B is the target with the no-shim Rust API policy
intact. Option A already served as the transitional migration that got code out
of the CLI shell; keeping it as the final state would simply rename the God
crate. Option C can support Option B when a truly shared IO primitive is
blocking extraction, but it should not replace domain-specific crates. Option D
is only a scheduling choice, not a completion strategy.

The bottom-up wave sequencing below is **superseded** by this finding and by
the no-shim policy; it is retained only as the original reasoning.

## Migration waves (compilable increments)

Because the original cluster moved as one crate, these historical "waves" were
about migrating in shim-preserving, always-green increments. That shim strategy
is obsolete for current focused-crate extraction. New work should instead use
small, direct-import extractions with boundary tests and no compatibility
facades.

- **Wave 0 — scaffold.** Create `agent-doc-orchestration` workspace member depending on `agent-doc-core`. Empty lib + `COMPILE_BASELINE.md`.
- **Wave 1 — leaves.** Move `sessions`, `ipc_socket`, `supervisor/`. This was
  originally planned with `pub use` shims in main so the ~100 command modules
  compiled unchanged. Current work must delete such shims as seams are extracted
  and update callers to focused crates directly.
- **Wave 2 — state/capture core.** Move `cycle_state`, `capture`, `repair` (invert or co-locate the 1 `cycle_state→git` back-edge — likely trivial once `git` moves in Wave 3).
- **Wave 3 — git.** Move `git` (depends on capture/cycle_state/ipc_socket — now all in orchestration).
- **Wave 4 — orchestrators.** Move `route`, `sync`, `start` + neighbors (`terminal`, `layout`, `focus`, `resync`, `hooks`, `project_controller`, `startup_miss`, `gc`, `autoclaim`, `codex_hook`).
- **Wave 5 — prune CLI deps.** Remove now-unused heavy deps from `agent-doc/Cargo.toml`; measure the CLI-shell cold build; update `COMPILE_BASELINE.md`.

Do not repeat the "keep shims until final cleanup" approach for current work.
Every extracted seam should end with callers on the focused crate path.

## FFI handling

11 orchestration FFI functions remain in `src/ffi.rs` (`agent_doc_document_changed*`, `agent_doc_sync_*`, `agent_doc_is_tracked`, `agent_doc_set_status`/`get_status`, `agent_doc_*_ipc_listener_*`, `await_idle`/`is_idle`/`is_typing_via_file`, `write_ack_content`, `commit`, `resolve_project_path`).

**Decision: keep ONE cdylib in the `agent-doc` CLI crate.** Editor plugins link
a single `libagent_doc.{so,dylib}`. FFI symbols are an ABI boundary, not a Rust
API facade: the binary may keep `extern "C"` wrapper functions and force-link
references so the exported symbol table remains stable, but Rust callers should
not get compatibility re-export paths for extracted policy. FFI-adjacent
extractions must preserve the exported symbols and route their implementation to
focused crates directly.

## Risks

- **Boundary blast radius.** ~100 command modules and `main.rs` historically
  referenced `route`/`sync`/`start`/`git`/`sessions` through broad paths. Current
  extractions should not absorb that blast radius with compatibility shims.
  Instead, keep each seam narrow, rewrite its internal call sites directly, and
  add boundary tests that fail if orchestration re-owns or re-exports the moved
  policy.
- **`project_controller` / `session_actor` / `startup_miss` / `gc` coupling.** These cross-cut orchestration and may need to move together; confirm the import graph in Wave 1 before committing the module set.
- **cdylib symbol preservation.** Mitigated by the `force_link` pattern (proven). Every wave touching FFI must re-run `nm -D libagent_doc.so`.
- **`tmux-router` patch.** The superproject `.cargo/config.toml` `[patch.crates-io]` for `tmux-router` must follow the crate that depends on it (orchestration).
- **Test migration.** Each module's inline + integration tests move with it; the `tests/` integration suite (incl. the known-flaky live-tmux `codex_bare_run_inside_owning_pane` test) targets orchestration behavior and may need the dep on `agent-doc-orchestration` as a dev-dependency.
- **Build-time cost.** Multi-cycle (5+ waves). Largest agent-doc refactor since the original Wave 4.

## Acceptance criteria

1. `agent-doc-core` remains absent from the workspace and dependencies.
2. `agent-doc-orchestration` compiles as a transitional adapter crate but does
   not own newly extracted domain policy.
3. For each extracted seam, callers import the focused crate directly; no
   `pub use` facade or compatibility module keeps the old path alive.
4. Static boundary tests prove the focused crate owns the vocabulary/decision
   table and orchestration does not re-own it.
5. `agent-doc` and `agent-doc-orchestration` keep only CLI/effect adapters for
   git/tmux/process/editor/socket/SQLite/file IO.
6. `make check` remains green after each extraction. If FFI-adjacent code moves,
   `nm -D libagent_doc.so` still exports the full editor ABI surface.

## Decomposition into implementation sub-tasks

The old wave backlog (`#adoc-orch-scaffold`, `#adoc-orch-leaves`,
`#adoc-orch-state`, `#adoc-orch-git`, `#adoc-orch-orchestrators`,
`#adoc-orch-prune`) describes the historical one-crate move. New work should be
filed by domain seam instead, for example:

- queue journal policy -> `agent-doc-queue`
- session accretion policy -> a focused `agent-doc-session-accretion` crate if
  no existing crate owns it
- document actor/write queue vocabulary -> `agent-doc-document-realtime` or a
  focused document-actor crate if the runtime boundary is extracted
- remaining tmux command/input policy -> `agent-doc-tmux-*`
- remaining controller/route decisions -> `agent-doc-controller`
- stale controller/supervisor binary identity decisions and warning text ->
  `agent-doc-controller::status` while orchestration keeps process/socket/file
  evidence gathering

Each seam is an independent green-to-green cycle with focused unit tests,
static no-facade boundary tests, and `make check`.
