# Plan: Supervisor-in-process consolidation (#pcp-consol)

Source backlog item: `#pcp-consol` — "Supervisor-in-process consolidation (single-process control
plane, specs/08b): make the supervisor an in-process adapter in the controller so the session actor
can own one ordered write queue + one filesystem watcher. Prerequisite that unblocks #pcp3b, #pcp4,
#pcp7."

This is the structural prerequisite the `#pcp3a` implementation finding surfaced: today the
supervisor and the agent finalize run in **separate OS processes**, serialized only by advisory
`flock` + SQLite compare-and-swap, so an *in-process* ordered write queue (`#pcp3b`), a single
controller-owned watcher (`#pcp4`), and a read-only plugin WatchService (`#pcp7`) cannot serialize
the real writers until the supervisor lives inside the controller process. This plan decomposes that
migration. It is multi-day and must be done in a fresh, unsaturated context — each phase is
independently shippable and SimWorld-verifiable.

## Status (2026-06-10): phases 1–4 done; cutover at `pcpc5e1` observe rung

**The entire pure-Rust migration is built, committed, and on the agent-doc submodule's `main`.**
Verified from `git log` (submodule HEAD `98e6ac0e`):

| Phase | Commit | State |
|-------|--------|-------|
| `#pcpc1` session-actor message runtime (`session_actor.rs` mailbox + 3 SimWorld tests) | `55243328` | ✅ done |
| `#pcpc2` in-process supervisor adapter (`supervisor/in_process.rs`) | `bcb63a1f` | ✅ done |
| `#pcpc3` ordered write queue (`write_queue.rs`, wired into `write.rs`) | `ef47c07e` | ✅ done |
| `#pcpc4` single controller-owned watcher (`document_watcher.rs`) | `038f0165` | ✅ done |
| `#pcpc5a–c` write-authority gate ladder (`write_authority.rs`) | `600797b3` | ✅ done |
| `#pcpc5d` direct-run writer routed through the gate | `6f588d45` | ✅ done |
| `#pcpc5e1` supervisor-host cutover gate — **observe rung** (`supervisor_authority.rs`) | `98e6ac0e` | ✅ done (HEAD) |

**Only the live-gated cutover swap remains** (`#pcpc5e1` authority rung → removal rungs): actually
host the child on `supervisor::in_process::InProcessSupervisor` when `AGENT_DOC_SUPERVISOR=in-process`,
advance `AGENT_DOC_WRITE_AUTHORITY` shadow→dual-write→authority, then delete the out-of-process
direct-disk writers and demote the editor WatchService to read-only. This is **doubly constrained**:
(1) it needs a live IntelliJ/Codex editor-boundary proof, and (2) per `specs/08b` §"Migration gates"
the authority change must **never land inside an agent-doc response cycle** (every cycle persists its
`### Re:` through exactly the write path being cut over). Run it **out-of-cycle in a plain Claude Code
session**, each rung behind its rollback flag. The "Current state" and "Actionability" sections below
describe the pre-migration baseline and are kept for historical context.

## Relationship to existing plans/specs (do not duplicate)

- `specs/08b-single-process-control-plane.md` — the target. §"In-process actors" already specifies a
  dispatch actor, a store actor (single SQLite writer), per-document session actors, **supervisor
  adapters** (one per managed harness child, in-process), and projection workers. §"Document write
  and watch authority" (added in `#pcp1`) specifies the single disk writer + ordered write queue +
  single watcher this migration realizes.
- `plan-project-controller-session-actor-mvp.md`, `plan-single-owner-session-actor-transition.md`,
  `plan-controller-singleton-zero-downtime-handoff.md`, `plan-controller-admin-backpressure.md`,
  `plan-sqlite-session-store-concurrency.md` — the controller/session-actor/store substrate this
  builds on. They cover ownership generations, CAS lifecycle, SQLite state, and admin/backpressure.
  This plan adds the **supervisor → in-process adapter** step they do not yet decompose.
- `plan-pcp-consolidate.md` — the parent capstone; `#pcp-consol` is its prerequisite that unblocks
  `#pcp3b`/`#pcp4`/`#pcp7`.

## Current state (from the #pcp3 architecture map, 2026-06-08)

- `agent-doc-orchestration/src/supervisor/` — the supervisor is a child process spawned by
  `agent-doc start`; it owns the harness PTY/stdin, restart/clear, readiness evidence, heartbeat. It
  controls via a Unix socket (`supervisor/ipc.rs`). It does **not** itself write the `.md` today; the
  binary's finalize/stream/run/repair `atomic_write` does, and supervisor idle/file-watch writes
  (where they exist) go through the same disk path.
- `agent-doc-orchestration/src/session_actor.rs` — a **data struct** with SQLite-backed CAS guards
  (`store_record_in`, `transition_state_in`), not a message-passing actor with a mailbox/queue.
- `agent-doc-orchestration/src/project_controller.rs` (242KB) — hosts actor records/cycles in SQLite;
  no per-document in-process write serializer.
- Write serialization today: per-operation advisory `flock` (`write.rs`) + SQLite CAS. No in-process
  ordered write queue.

## Phases (each independently shippable + SimWorld-verifiable)

Dependency order: actor runtime → supervisor adapter → write queue → watcher → migration cutover.

### `#pcpc1` — Session-actor message runtime ✅ done (`55243328`)
Turn `session_actor.rs` from a data struct into a real in-process actor: a per-document mailbox
(command channel) processed on a single owning thread/task, so all document-scoped operations
(write submit, closeout, queue-head selection, lifecycle transitions) are serialized through one
owner. Keep the SQLite CAS as the durable backstop. SimWorld: concurrent commands to one document
actor execute in a deterministic serialized order.

### `#pcpc2` — In-process supervisor adapter ✅ done (`bcb63a1f`)
Replace the out-of-process `agent-doc start` supervisor with an in-process supervisor adapter hosted
by the controller (per `08b` §"In-process actors"): it owns the harness child PTY/stdin, readiness,
restart/clear, and heartbeat as a controller-managed task rather than a separate OS process. Preserve
the existing Unix-socket external boundary for CLI/editor callers (they still talk to the controller
process). SimWorld: supervisor heartbeat loss + reattach, restart/clear, all in-process.

### `#pcpc3` — In-process ordered write queue (realizes `#pcp3b`) ✅ done (`ef47c07e`)
With the supervisor and finalize now in the same process, route all `write`/`stream`/`finalize`/
repair disk writes **and** supervisor idle/file-watch writes through the session actor's single
ordered write queue (built on `#pcpc1`). The `flock` stays a cross-process backstop only. This kills
the R6 self-race / exit-75 at the root (not just the `#pcp3a` drain-retry mitigation). SimWorld:
concurrent supervisor-write + agent-finalize serialize; finalize never observes a half-applied write.

### `#pcpc4` — Single controller-owned filesystem watcher (realizes `#pcp4`) ✅ core done (`038f0165`)
Replace the per-process `notify` watchers with one controller-owned watcher per document feeding the
session actor; demote the editor plugin WatchService to read-only buffer reporting (realizes `#pcp7`,
which also needs its own plugin change + live verification). SimWorld: one watcher event stream, no
duplicate-watcher reconcile race.

### `#pcpc5` — Migration cutover (08b gate ladder) — IN PROGRESS
Drive the `08b` migration gates (shadow → dual-write → read-switch → authority → removal) for the
write/watch authority move, each with a rollback flag, removing the obsolete out-of-process supervisor
+ direct-disk writers only after SimWorld + CLI + live-tmux coverage proves the in-process path for
Claude/Codex/OpenCode.

**Done (pure-Rust scaffolding, all behind default-off rollback flags):**
- `#pcpc5a–c` (`600797b3`) — write-authority gate ladder: `AGENT_DOC_WRITE_AUTHORITY`
  off→shadow→dual-write→authority routes `write::atomic_write` through `write_queue` (`write_authority.rs`).
- `#pcpc5d` (`6f588d45`) — direct-run writer routed through the same write-authority gate.
- `#pcpc5e1` observe rung (`98e6ac0e`) — supervisor-host ladder mirror: `AGENT_DOC_SUPERVISOR`
  off→shadow→in-process flag + `supervisor_authority.rs`; observe-only (always still hosts
  out-of-process, logs `supervisor_host_gate`). Zero live behavior change.

**Remaining (live-gated, OUT-OF-CYCLE only — must NOT land in an agent-doc response cycle):**
- `#pcpc5e1` authority rung — swap `agent-doc start` hosting onto
  `supervisor::in_process::InProcessSupervisor` when `AGENT_DOC_SUPERVISOR=in-process`.
- Removal rungs — delete the obsolete out-of-process supervisor + direct-disk writers; demote the
  JetBrains/VS Code WatchService to read-only buffer reporting (realizes `#pcp7`).
- Advance `AGENT_DOC_WRITE_AUTHORITY` to dual-write/authority live with editor-boundary proof.

## Verification obligations
Per `08b` §"Verification obligations": deterministic SimWorld coverage for concurrent
supervisor/finalize serialization, single-watcher event streams, supervisor heartbeat loss/reattach,
and crash-before/after-commit; CLI integration proving route/start/sync/finalize use the in-process
path; a small live-tmux smoke layer for the external pane/editor boundary.

## Safety
Multi-day, multi-surface (orchestration + supervisor + controller + plugins). Do NOT attempt in a
saturated session. Each phase: `make check` + fresh `cargo install` + `cargo build --release`, and
the watcher/plugin-demotion phases need live IntelliJ/Codex verification. Follow the gate ladder with
rollback flags so a regression never strands committed controller rows.

## Actionability — RESOLVED (see "Status (2026-06-10)" above)

> **Historical note (2026-06-09):** the section below assessed which phases were actionable without a
> live editor. **It is now obsolete:** every SimWorld-doable phase it listed (`#pcpc1`–`#pcpc4` plus
> the pure-Rust cutover rungs `#pcpc5a`–`#pcpc5d` and the `#pcpc5e1` observe rung) has shipped. The
> only work left — the `#pcpc5e1` authority swap, the supervisor/direct-writer removal, and the
> JetBrains/VS Code WatchService demotion (`#pcp7`) — is the live-gated, out-of-cycle remainder
> described in the Status section. Do **not** re-queue `#pcpc1`–`#pcpc4` as actionable backlog ids.

The "fresh, unsaturated session" requirement is a **focus/size** constraint, not a hard block. Most
of this work is pure-Rust + SimWorld and can be picked up in any focused (non-degraded) session
without a live IDE:

- **`#pcpc1` (session-actor message runtime), `#pcpc2` (in-process supervisor adapter), `#pcpc3`
  (ordered write queue), `#pcpc4` (single controller-owned watcher core)** — all SimWorld-verifiable
  with deterministic coverage (concurrent serialization, heartbeat loss/reattach, single event
  stream). **No live editor needed.** These are queued as actionable `agent:backlog` ids. Dependency
  order: `#pcpc1 → #pcpc2 → #pcpc3 → #pcpc4`.
- **Genuinely live-gated (stay in review, run under `#xkpf`):** the `#pcpc4` plugin WatchService
  read-only demotion (`#pcp7`) and `#pcpc5` migration cutover authority/removal gates — these need a
  live IntelliJ/Codex pane to prove the external editor boundary, and `#pcpc5` is additionally
  spec-08b-gated against landing in a response cycle.

So the architecture is **not** blocked end-to-end: phases 1–4 (minus the plugin demote) are
actionable now; only the cutover + plugin demote await a live session.
