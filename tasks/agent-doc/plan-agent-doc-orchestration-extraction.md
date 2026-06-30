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
focused crate under the old path.

### Modified execution plan

1. Treat `agent-doc-orchestration` as a shrinking effect adapter, not the final
   product boundary.
2. Extract pure policy/data seams into the owning domain crate first; create a
   new focused crate only when no current crate owns that domain.
3. Use fan-out extractors for disjoint seams. Each extractor owns a narrow write
   set and can move multiple related seams only when they share the same
   destination and tests.
4. Keep docs, cross-crate boundary tests, final no-shim scans, `make check`,
   commit, push, and one CI check in the main integration pass.
5. Prefer direct dependency inversion over shared catch-all utilities. A shared
   IO/base crate is tactical only when it unblocks a domain extraction.
6. Rename deprecated internal symbols and module names to the current domain
   vocabulary in the same pass as the related extraction; do not preserve the
   obsolete path as a compatibility alias.

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
- `agent-doc-turn` now owns hook response projection policy: extracting prompt
  targets from `### Re:` response headings, stripping agent-doc patch metadata
  from captured response summaries, and bounding hook summary text. Orchestration
  keeps hook execution, capture loading, project-root/git facts, and tsift
  process invocation while importing the focused response-text helpers directly.
- `agent-doc-prompt-contract` now owns harness prompt parsing policy: stripping
  binary-owned `agent-doc <file>` invocations, classifying claim/compact
  invocations as non-session prompts, matching the invoked file, preserving
  non-invocation prompt bodies, and rendering the synthetic harness diff.
  Orchestration keeps only the env/Codex-hook prompt-source adapter and imports
  the focused parser directly.
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
- We are in the current seam-by-seam extraction wave after the transitional
  orchestration crate move. Practically, this is **Wave 2**: shrink
  `agent-doc-orchestration` by moving durable policy into focused domain crates,
  leaving orchestration as effect adapters.
- `agent-doc-queue` now owns one-shot backlog/icebox/pending queue-sync request
  collection and sync-report policy for `agent-doc queue sync`; `queue_cmd`
  keeps only file IO, rendering, snapshot persistence, and CLI output.
- `agent-doc-queue` now owns id-backed queue-head closeout/removal decisions and
  free-text queue-head provenance decisions used by `session_check`, including
  committed queue presence, completed-residue detection, response-proof
  classification, suppression-marker residue, and the response-heading
  plausibility heuristic. Orchestration keeps guard-mode formatting,
  cycle-state loading, ops logging, and effectful response-materialization
  checks owned by `agent-doc-turn`.
- `agent-doc-queue` now owns queue consumption entry policy used by
  `write/queue_consume`: selecting prompt texts, counting leading done-id
  consumes, matching prompt text with cosmetic priority markers ignored,
  marking entries completed by done ids, and normalizing consumed done-id bags.
  Orchestration keeps document locks, file/snapshot IO, IPC node ops,
  response-echo embedding, ops logging, and editor convergence.
- `agent-doc-queue` now owns preflight queue convergence policy: re-aligning a
  pre-maintenance baseline to the post-maintenance queue component, carrying
  in-progress-marker-only backlog/pending/icebox convergence, and classifying
  non-selected future queue-body edits while ignoring exchange boundary churn.
  Preflight keeps queue maintenance, snapshot loading/saving, baseline file IO,
  and diagnostics while importing the focused queue convergence helpers
  directly; the old local helper cluster and pure test were deleted from
  orchestration instead of kept as facades.
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
- `agent-doc-element-backlog` already owned open backlog id extraction; preflight
  now calls `open_backlog_ids_in_content` directly for `do #id` closeout
  evidence instead of re-parsing backlog components locally.
- `agent-doc-turn` now owns committed-without-response-body closeout policy:
  detecting assistant `### Re:` response bodies in `agent:exchange` and deciding
  whether committed cycles with no visible response body pass, skip as no-op
  commits, or interrupt. Orchestration keeps cycle-state loading, file parsing,
  ops-log emission, side-effect note rendering, and recovery-message formatting.
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
- `agent-doc-controller` now owns route-trigger current-draft visibility
  policy: matching visible trigger text, wrapped trigger captures, and
  equivalent relative/absolute `agent-doc <path>` drafts while using a caller
  supplied prompt-line predicate. Route/start keep pane capture, harness
  predicates, diagnostics, and submit/resubmit effects while calling the focused
  controller policy directly.
- `agent-doc-turn-executor-tmux` now owns context-clear submit policy: active
  composer visibility detection for `/clear` and OpenCode `/new`, submitted
  observation/status vocabulary, Enter-resubmit gating, and clear-submit
  observation/resubmit/blocked line rendering. The root CLI keeps pane capture,
  retry delivery, ops-log writes, and error raising; start detection keeps
  supervisor payload decisions and harness predicates. Both callers import the
  focused policy directly with no local shim or `clear_direct_*` compatibility
  names.

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

Each seam is an independent green-to-green cycle with focused unit tests,
static no-facade boundary tests, and `make check`.
