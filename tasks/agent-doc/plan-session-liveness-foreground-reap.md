# Plan: Session liveness by agent-foreground signal (stale sessions.json reap)

**Status:** phase 1+2 shipped (the bug fix) · phase 3+4 follow-up · **Scope:** agent-doc core (`src/agent-doc`) · **Owner cycle:** own focused agent-doc cycle (not a lazily session)

## Progress (2026-07-02)

- **Phase 1 `#adsessreap1` — DONE.** New shared predicate
  `session_liveness::pane_owns_live_agent(tmux, pane)` =
  `pane_alive && !bare_shell(#{pane_current_command})`, with a pure
  `pane_owns_live_agent_from_signals` core for deterministic tests
  (`agent-doc-orchestration/src/session_liveness.rs`). Wired as the reap
  predicate at all three sites: `close_stale_dead_pane_actors_with_tmux_for_caller`
  (`project_controller.rs`), `validate_file_claim` (`claim.rs`), and
  `prune_targeted_in` (`resync/prune.rs`). Bare-shell detection reuses
  `agent_doc_tmux::pane_current_command_is_bare_shell`; unknown/foreign foreground
  commands stay conservatively "owning" to preserve the false-alive-over-false-dead
  bias for record reaping.
- **Phase 2 `#adsessreap2` — DONE.** `find_alive_pane_via_open_session_log`
  (`sync.rs`) now requires `pane_owns_live_agent` before re-owning a pane off an
  open session-log, so a `claude → zsh` pane is treated as ownerless and the
  editor live-buffer sync stays a separate lifecycle. The other
  `reregister_recovered_owner` vectors already prove liveness via supervisor-pid /
  process-tree containment, so they were left untouched.
- **Tests.** Pure-signal unit tests (dead pane, bare shell incl. login `-zsh`,
  agent commands never reaped, unknown command kept, missing sample kept) plus a
  `#[ignore]` live-tmux regression (`pane_owns_live_agent_tracks_foreground_command`)
  reproducing the `lazily-rs.md`/`%3` shape on an isolated server. `make check`
  green; clippy clean.
- **Phase 3 `#adsessreap3` — follow-up, gated.** Must ride the in-flight
  `#adstatechart` actor-lifecycle work (supervisor child-exit → `Closed`
  transition), not a side path. Deferred to coordinate.
- **Phase 4 `#adsessreap4` — follow-up.** The existing `supervisor_lease` /
  `last_heartbeat` + `supervisor_lease_is_fresh_and_alive`
  (`project_controller.rs` / `agent-doc-sqlite`) is the reuse target for the TTL
  reap. Carries false-dead risk (reaping a spoofed-alive pane), so it wants live
  operator verification before wiring into the registry reap paths.

## Symptom

`agent-doc finalize` / `write --commit` refuses with `pane ownership mismatch:
owned by pane %X, current pane is %Y` for a document whose owning agent session
has already exited. Observed 2026-07-02: `lazily-rs.md` stayed bound to pane
`%3` after that pane's `claude` process exited to a bare `zsh`; a still-open
JetBrains editor buffer kept the record from ever being reaped. The doc was not
live anywhere, yet every reaper treated it as alive.

This is a recurring wedge class: any `claude → zsh` degradation strands the
`sessions.json` binding, forcing a manual `agent-doc claim --pane %<live>
--force <FILE>` to recover.

## Root cause

`sessions.json` is **already a snapshot** — a projection of the SQLite actor
store (`documents` table / `ActorRecord`), regenerated on every write by
`emit_sessions_projection` (`project_controller.rs:1922`; "keeps the projection
and the source of truth in lockstep", `:1502`). So the fix is **not** to move
authority into memory (that would only weaken crash recovery). The bug is that
**liveness is measured by signals that outlive the agent process:**

| signal | location | why it's wrong |
|---|---|---|
| `pane_alive` = tmux `#{pane_dead}=0` | `tmux-router/src/tmux.rs:165-191` | a pane that dropped `claude → zsh` still reports **alive**; it never checks `#{pane_current_command}` |
| recorded `pid` via `/proc/{pid}` | `gc.rs:641` | that pid is the **launcher/supervisor**, not the agent child — there is no separate agent-pid field |
| open editor **session-log** | `sync.rs:3970-4002` | `find_alive_pane_via_open_session_log` + `reregister_recovered_owner` (`:3820`) **actively resurrect** the record for an alive-but-not-claude pane |

Every close/prune path is gated on `!pane_alive` or `!pid_alive`:
- `claim::validate_file_claim` — `!pane_alive` (`claim.rs:536,552`)
- `resync::prune_dead_entries_for_target` — `!pane_alive` (`resync/prune.rs:21,27`)
- bulk `tmux_router::prune` — `retain(pane_alive)` (`sessions.rs:709` test)
- actor close `close_stale_dead_pane_actors_for_caller` — `if pane_alive { continue }` (`project_controller.rs:1304`), only a **dead** pane sets `Closed` + clears `pane_id` (`:1324-1327`)
- `emit_sessions_projection` retains rows whose pane is in `live_actor_panes` (`:1953`)
- `gc::clean_orphaned_sockets` keeps rows if `pid_alive` **or** socket responds; Phase-2 prune needs **both** socket-gone **and** pid-dead (`gc.rs:686,695,722`)

There is **no process-exit hook** that removes a session's own record — `Drop`
only joins the owner thread (`session_actor.rs:594-609`). The design
deliberately prefers false-alive over false-dead ("an active Claude session
should never be killed automatically", `prune.rs:275`).

**The design flaw:** that "never kill a maybe-live agent" bias is correct for
*not terminating* a session, but it is wrongly applied to *reaping a stale
binding*. Liveness must be tied to the thing whose death we care about — the
**agent process** — via a signal that dies with it, not proxies (pane
existence, launcher pid, open editor) that outlive it.

## Fix (4 parts; (1)+(2) are the bug fix, (3)+(4) make it robust)

### Phase 1 — agent-foreground check in the liveness predicate  `#adsessreap1`
`pane_current_command_is_bare_shell` already exists (`route/dispatch.rs:373`).
Introduce a shared `pane_owns_live_agent(pane)` = `pane_alive(pane) && !bare_shell(pane)`
and use it as the reap predicate in:
- `close_stale_dead_pane_actors_for_caller` (`project_controller.rs:1304`) — a
  bare-shell pane transitions the actor to `Closed` + clears `pane_id`, so the
  projection drops the row.
- `claim::validate_file_claim` (`claim.rs:536`) and
  `resync::prune_dead_entries_for_target` (`prune.rs:21`).

Keep the existing `!pane_alive` fast path; add the bare-shell case as an
additional reap trigger. **Do NOT** apply the bare-shell check to any path that
*terminates* an agent — only to record reaping.

### Phase 2 — decouple editor-buffer liveness from session-owner liveness  `#adsessreap2`
`find_alive_pane_via_open_session_log` (`sync.rs:3980`) and
`reregister_recovered_owner` (`:3820`) must stop re-owning a pane to a session
whose agent is gone. An open editor / session-log means "document is open in an
editor," not "the agent session owns this pane." Recovery should require
`pane_owns_live_agent(pane)` before re-registering; otherwise treat the pane as
ownerless (the editor's live-buffer sync remains a separate lifecycle from the
session-owner record).

### Phase 3 — real `Closed` transition on agent child exit  `#adsessreap3`
Have the supervisor observe the agent child's exit (it already tracks the
child) and emit an actor `Closed` transition (or a genuine `session_end` that
maps to `Closed`), so the projection drops the row promptly on clean exit. Best
effort — crashes skip it; Phases 1 & 4 are the backstop.

### Phase 4 — heartbeat/lease TTL on the actor record  `#adsessreap4`
Add a `last_heartbeat` (or reuse an existing liveness stamp) written by the
live agent/supervisor loop. A record with no heartbeat within a TTL (config,
e.g. `session_lease_ttl_secs`) is reapable regardless of pane/pid/editor state.
This is the belt-and-suspenders that survives any missed signal, including the
`claude → zsh` case even if `#{pane_current_command}` detection is fooled.

## Tests

- **SimWorld / deterministic:** simulate a pane whose command transitions
  `claude → zsh` with the actor record still `Ready`; assert the next
  claim/resync/projection reaps it (Phase 1). Regression for the exact
  `lazily-rs.md`/`%3` scenario.
- Simulate an open session-log (no `session_end`) on a bare-shell pane; assert
  recovery does **not** re-own it (Phase 2).
- Simulate agent-child exit; assert actor → `Closed` and projection drops the
  row (Phase 3).
- Simulate a stale-heartbeat record on a (spoofed-alive) pane; assert TTL reap
  (Phase 4).
- Guard test: a genuinely-live `claude` pane is **never** reaped by any phase
  (protects the "never kill a live session" invariant).

## Non-goals / risks

- **Not** moving session authority from SQLite into a live in-memory process —
  that weakens crash recovery and does not address the liveness-signal bug.
- Risk: `#{pane_current_command}` can read `claude` wrappers/subshells
  differently across shells; the bare-shell allowlist must be conservative
  (reap only on *known* bare shells, never on unknown commands) to preserve the
  false-alive-over-false-dead safety bias for record reaping.
- Coordinate with in-flight `#adstatechart` actor-lifecycle work — the `Closed`
  transition (Phase 3) should ride the statechart, not a side path.

## Recovery today (until fixed)

`agent-doc claim --pane %<live> --force <FILE>` overwrites the stale binding,
removes the stale registry entry, and reaps stale live-buffer sidecars; then
`agent-doc write --commit <FILE>` lands cleanly.
