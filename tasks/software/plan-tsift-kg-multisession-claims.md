# Plan: tsift kg multi-session model claims (refcount-gated unload)

User request (tsift.md queue): *"tsift kg should handle multiple sessions. Where it
remains loaded if at least one session has a claim. Make sure the system is robust so
clashed sessions can be unclaimed."*

## Goal

A KG extractor model (e.g. `qwen3-32b-q4` on Ollama) stays resident while **≥1 live
session holds a claim**, and is unloaded only when the **last** claim is released or
reaped. Crashed / clashed sessions must not pin the model forever — their stale claims
are reclaimed by pid-liveness.

## What already exists (do NOT rebuild)

`packages/tsift-local-model/src/lib.rs` already has the cooperative refcount substrate:

- `GpuLeaseRegistry` — file-backed (`~/.tsift/gpu-lease.json`, `TSIFT_LEASE_FILE` /
  `XDG_STATE_HOME` overrides), keyed `profile_id -> Vec<GpuLeaseRecord>` (multi-holder).
- `GpuLeaseRecord { holder_pid, holder_command, lease_mode, idle_ttl_seconds, ... }`.
- `LeaseMode::{Shared, Exclusive, CpuOrHash}` — `Shared` already permits N holders.
- `is_pid_alive(pid)` (`kill -0`), `prune_stale_leases(now, is_alive)` (reaps dead-pid /
  TTL-expired holders), atomic `read_lease_registry` / `write_lease_registry`.
- `GpuLeaseRelease { outcome, remaining_holders }` — release already reports the count.

The **gap** is purely wiring: `tsift kg extract` / `smoke` never acquire a claim, and
`cmd_kg_unload` (`packages/tsift-cli/src/commands/kg.rs`) runs `build_unload_actions` /
`execute_unload_actions` **unconditionally** — no refcount gate, no stale reap.

## Design decision

Use **explicit session claims** plus a **refcount-gated unload**, with `extract`/`smoke`
auto-acquiring a short-lived claim for the duration of their own run:

- New `tsift kg claim <profile> [--ttl <secs>]` — acquire a `Shared` claim for the
  caller's session (holder_pid = caller, or a `--session-pid` override so a supervisor
  can claim on behalf of a child). Prints the new `remaining_holders`.
- New `tsift kg unclaim <profile> [--session-pid <pid>]` — release this session's claim;
  prints `remaining_holders`.
- `tsift kg unload` becomes refcount-gated: `prune_stale_leases` first (reap crashed
  sessions), then if `remaining_holders > 0` **skip** the provider unload and report
  `kept-resident: N live claim(s)`; only when 0 remain do the existing
  `build_unload_actions` / `execute_unload_actions` run. A `--force` flag overrides the
  gate (current unconditional behavior, for operator recovery).
- `extract` / `smoke` acquire a Shared claim on entry and release it on exit (RAII guard),
  so a concurrent `unload` during a run sees a live holder and keeps the model resident.

## Phases (one backlog id each)

- **#kgclaim1** — `claim` / `unclaim` subcommands + CLI plumbing over the existing
  registry fns. JSON + human output, `remaining_holders`. Tests: acquire→count=1,
  second session→count=2, unclaim→count drops, stale pid reaped on next op.
- **#kgclaim2** — refcount-gated `kg unload`: prune stale → gate on `remaining_holders` →
  `--force` override. Tests: unload skipped while a live claim exists; unload proceeds
  after last unclaim; a dead-pid claim does not block unload (reaped).
- **#kgclaim3** — auto-claim guard in `extract` / `smoke` (acquire on entry, release on
  exit even on error). Test: an `unload` racing an in-flight `extract` keeps the model.
- **#kgclaim4** — spec (`specs/local-kg-model.md` § Multi-Session Model Claims) +
  `VERSIONS.md`; `make check`; build + install; commit + push.

## Risks / notes

- pid-liveness via `kill -0` is best-effort and host-local; cross-host Ollama claims would
  need a heartbeat, out of scope (note it in the spec as a known bound).
- Default `idle_ttl_seconds = 0` (no TTL) means a claim only drops on explicit unclaim or
  pid death — correct for "stays loaded while a session lives," but pair every auto-claim
  with a guaranteed release so a long-lived supervisor doesn't pin the model.
