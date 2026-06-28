# Plan — corky state machines (typed lifecycle enums)

Replace corky's scattered/stringly-typed lifecycle status with typed `enum` state
machines that own their own transitions. Per the `#next-steps` evaluation
(corky.md exchange), **typed state machines = real win; reactive graphs =
overkill** for corky's batchy CLI/daemon shape. This plan delivers the typed
machines only.

Repo: `~/work/btakita/corky` (v0.29.0, origin/main `39f266a`, 5-crate workspace).
Target crate layout: `corky-social`, `corky-mail`, binary `src/`.

## Current state model (audited)

| Lifecycle | Location | State repr | Guards | Gap |
|-----------|----------|-----------|--------|-----|
| Social publish | `corky-social/src/social/draft.rs:12` (`DraftStatus` Draft/Ready/Published) | typed enum ✅ | ad-hoc `if` in `publish.rs:26-42` (PB1) + `schedule.rs:102` (skip Published) | no transition fn; status mutated directly (`publish.rs:127`); **no `Publishing` transient** → mid-publish crash can double-publish on retry |
| Email draft | `corky-mail/src/schedule.rs:146,178` | **stringly-typed** (`"scheduled"`/`"draft"` via `to_lowercase()` + legacy `**Status**` regex) | inline string compare | no enum; no terminal `Sent`/`Failed`; dual parse paths (YAML + regex) drift-prone |
| Sync run | `corky-mail/src/sync/types.rs:76` (`SyncState` = cursor data only) | data, not state | none | no persisted run phase; a crashed watch tick / `sync refetch` re-fetches from cursor with no resume/progress marker |
| Watch tick | `src/watch.rs:327-381` | implicit phase order + loose `u64` cycle counters | `run_tick` panic isolation ✅ (#ckywatchpanic) | no typed `WatchPhase`; failed phase just `eprintln!`s, no backoff/circuit-breaker on repeated auth failure |

## Phases (priority order)

### P2 — `#ckypubsm` Social publish state machine (crash safety + de-dup)
**Why first:** only machine guarding an **irreversible side effect** (a duplicate
public post). Highest correctness value.

- Extend to `PublishState { Draft, Ready, Publishing, Published, Failed }`.
- `impl PublishState { fn transition(self, e: PublishEvent) -> Result<PublishState> }`
  centralizing PB1 (`publish.rs:26-42`) into `can_publish()` / rejected-transition errors.
- Persist a `Publishing` marker **before** the platform API call
  (`publish.rs:112`) — frontmatter `status: publishing` + `publish_started_at`;
  cleared to `Published` (`publish.rs:127`) on success or back to `Ready`/`Failed`
  on error. A retry then sees `Publishing` and reconciles by `post_id` instead of
  re-creating. (Today's "write frontmatter before reconciliation" note at
  `publish.rs:124-126` is the half-measure this formalizes.)
- `schedule.rs:102` skip check → `state.is_terminal()`.
- Unit tests: every transition incl. rejected ones; `Publishing`-crash retry path.

### P2 — `#ckyemailsm` Email draft status enum (replace string compares)
- `EmailDraftStatus { Draft, Scheduled, Sent, Failed }` (+ serde lowercase) in
  `corky-mail`, replacing `meta.status.to_lowercase() != "scheduled"`
  (`schedule.rs:146`) and the legacy `**Status**: scheduled` regex
  (`schedule.rs:178-189`). `parse_draft_yaml` status field → typed.
- `transition()` so only `Scheduled`+due fires; `Sent`/`Failed` terminal so the
  scheduler skips them like social's `Published`.
- Update `parse_email_scheduled` + the S3/S7/YAML schedule tests.

### P3 — `#ckysyncsm` Sync run-state machine (resume + progress)
- `SyncRunState { Idle, Fetching, Merging, Writing, Done, Error }` per account
  run; persist (sidecar under data dir) so a crashed watch tick or
  `corky sync refetch` resumes instead of re-fetching. `SyncState` cursor data
  (`types.rs:76`) stays as-is; this layers run-phase on top.
- Design-first (write the transition table before coding); lower correctness
  urgency than the publish/email machines.

### P3 — `#ckywatchsm` Watch phase enum + backoff (polish)
- `WatchPhase` enum + ordered tick list, replacing the loose
  `cycles_since_*: u64` counters (`watch.rs:320-325`) with a typed schedule.
- Add backoff / circuit-breaker for a phase that fails N times (e.g. repeated
  OAuth failure) instead of silent `eprintln!` every tick.
- Lowest value; only if watch grows more phases.

## Verification
Per item: `cargo test -p <crate> --features transcribe-cuda`, `cargo clippy
--all-targets` (= `make check`), `make build` + `make install`. Full workspace
suite stays 690/0/0. No reactive-graph / lazily-rs dependency introduced.
