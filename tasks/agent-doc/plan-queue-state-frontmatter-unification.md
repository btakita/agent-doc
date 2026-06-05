# Plan: Unified `queue:` frontmatter state (deprecate `auto` + `queue_active`)

Status: phases 1-4 implemented + live; harness-parity doc rewrites remaining

## Progress

- **2026-06-04 — Phase 4 done** (`src/agent-doc` commit `f256ff71`). Writer emits
  canonical `queue: start`/`queue: stop` via `frontmatter::merge_queue_state`,
  clearing legacy `queue_active:` in the same write (all 14 write sites migrated).
  Replay-hash / boundary-compare normalizers strip both fields together (no drift
  loop); drift-authority logic unchanged (reads via `live_continuation_head`).
  Spec + tests updated; `make check` green; both binaries reinstalled. Remaining:
  harness-parity instruction-surface doc rewrites only.

- **2026-06-04 — Phase 3 done** (`src/agent-doc` commit `60a466af`). Marker-side
  control: `<!-- agent:queue go -->` / `start` fresh-activates the queue (Auto
  trigger, like legacy `auto`); `stop` forces inactive + clears `queue_active`;
  control token stripped on drain/halt; `start`/`go`/`stop` recognized as
  queue-only attrs (no typo warning). `marker_control()` +
  `strip_control_from_tag()` + 2 `run_queue_maintenance` integration tests + 1
  allowlist test. Orchestration lib 2263 passed / 2 pre-existing route failures;
  core 702 passed; clippy clean. Not live until install (concurrent uncommitted
  route work still in tree).

- **2026-06-04 — Phase 1+2 done** (`src/agent-doc` commit `3f9b32d4`,
  branch `20260422/codex-agent-backend`). Added `QueueControl` enum +
  `Frontmatter.queue` field + `normalize_queue_control()` folding
  `queue: start|go|stop` onto `queue_active` in both parse paths, so every
  existing reader honors the canonical key. `queue_active:`/`auto` remain
  deprecated input. 7 core + 2 orchestration tests; spec updated. Verified green
  in a clean-HEAD sibling worktree (2257 pass / 2 pre-existing route failures
  unrelated). **Not yet built/installed live** — working tree carries a
  concurrent session's uncommitted `route.rs`/`session_check.rs`, so a live
  install is deferred until that lands.
Owner doc: tasks/agent-doc/agent-doc-bugs2.md
Related: #backlog-queue-sync-attr, queue control fences (`--- start`/`--- stop`), Claude `/loop`, Codex Stop-hook, OpenCode auto-loop.

## Problem

Queue activation today resolves from **four** independent sources
(`specs/07-orchestration-commands.md:225`):

1. `auto` attribute on the `<!-- agent:queue ... auto -->` marker
2. inline `--- start` / `--- stop` control fences
3. exchange-triggered `do queue` / `run queue`
4. persisted `queue_active: true|false` frontmatter

`auto` (marker) and `queue_active` (frontmatter) carry overlapping intent but
live in different places, are not bidirectionally synced, and `auto` can only
express *start*, never *stop*. The user wants one canonical, bidirectional,
verb-based control that reads like the rest of agent-doc's directive grammar
and behaves identically across Claude Code / Codex / OpenCode.

## Recommended framework: single tri-state `queue:` frontmatter key

Canonical persisted state is one frontmatter field:

| frontmatter | meaning | replaces |
|-------------|---------|----------|
| `queue: start` (alias `queue: go`) | active — drive the queue now | `queue_active: true` + marker `auto` |
| `queue: stop` | inactive — halt | `queue_active: false` |
| (absent) | unmanaged / inactive | absence of both |

Rationale for a verb tri-state over `queue_auto: true` (boolean) or
`queue: auto`:
- A boolean records *state* but loses *intent*; a verb (`start`/`stop`/`go`) is
  an imperative that matches the existing directive grammar — `do #id`,
  `run queue`, and the `--- start` / `--- stop` control fences already in the
  queue body. Same vocabulary, one less concept.
- `auto` cannot express stop; a single key with start/stop is symmetric and
  bidirectional by construction (there is exactly one field to read and write).

## Migration / aliases (back-compat, normalize-on-read)

- `<!-- agent:queue ... auto -->` → normalize to `queue: start`, strip `auto`
  from the marker. `auto` becomes a deprecated *input alias* only.
- `queue_active: true|false` → normalize to `queue: start|stop`. Deprecated
  input alias; keep parsing for existing docs (this doc included).
- `go` ≡ `start`; `stop` is the off state.
- Drain transition: when the queue drains, set `queue: stop` (today's behavior:
  "`auto` removed, `queue_active` cleared" — same effect, one field).
- Manual `queue: stop` halts and the binary never re-adds `start`/`auto`.

## Harness parity (Codex / Claude Code == OpenCode)

The three continuation paths already gate on `queue_active` / `auto`:
- Claude Code `/loop` auto-loop (preflight `queue_active` + `queue_trigger`)
- Codex Stop-hook continuation (`active_auto_queue_prompt`, codex_hook.rs:769)
- OpenCode auto-loop

Point all three at the unified `queue:` state so `queue: start` / `queue: go`
in any harness's frontmatter starts and drains the queue identically. This is
primarily a read-path consolidation: preflight keeps emitting `queue_active`
/ `queue_trigger`, now derived from the canonical `queue:` field.

## Phases

- `#queue-state-1` parse + normalize: accept `queue: start|go|stop`; map legacy
  `auto` / `queue_active` onto it; round-trip writer keeps a single field.
- `#queue-state-2` activation resolution: fold the four-source resolver
  (orchestration spec:225) onto canonical `queue:`; drain sets `queue: stop`.
- `#queue-state-3` harness parity: Claude `/loop`, Codex Stop-hook, OpenCode all
  read the unified state; preflight `queue_trigger` reflects start/go/stop.
- `#queue-state-4` spec + tests + VERSIONS.md; migration note for legacy fields.

## Out of scope (separate item)

Exchange-prompt → backlog → queue driving ("`queue: start` adds exchange prompts
to backlog and prepends new ids to the queue") is a related but distinct feature
tracked as its own queue prompt; it composes on top of this state model.

## Verification

`make check`; deterministic parser/round-trip tests for each alias mapping;
harness-parity integration coverage where modelable without a live pane.
