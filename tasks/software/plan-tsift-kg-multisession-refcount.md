# Plan: tsift kg multi-session reference-counted lifetime

> **IMPLEMENTED 2026-06-18 (#kgreflease, tsift `5093e88`, origin/main).** A
> cooperative lease registry (`gpu-lease.json`: `acquire`/`release`/`show`,
> shared multi-holder leases, pid-dead + TTL pruning, atomic temp+rename writes)
> already existed in `tsift-local-model`, so this shipped as *hardening* rather
> than the greenfield build below: (1) `fs4` advisory file lock around the whole
> acquire/release/renew/reap read-modify-write (closes the TOCTOU lost-update
> race — the headline robustness fix); (2) explicit heartbeat `renew_lease` /
> `tsift local-model lease renew` (slides the TTL window; `acquired_at` doubles
> as last-heartbeat); (3) `reap_leases` / `tsift local-model lease reap`
> reporting `emptied_profiles`; (4) reference-counted unload via
> `lease release --unload-on-last-release` / `lease reap --unload-empty`
> (keep_alive:0 only when a profile drops to zero references). Spec:
> `specs/local-kg-model.md` § Multi-Session Reference-Counted Lease Lifetime.
> The phased greenfield plan below is retained as design rationale.


## Problem

The `tsift kg` substrate (loaded `graph.db` working set and, more expensively, the
Qwen3-32B model resident in Ollama VRAM) should:

- Stay loaded while **at least one** session holds a reference.
- Be released when the **last** reference goes away.
- Robustly reclaim references held by **crashed** sessions (a crash must never
  pin VRAM forever).
- Be arbitrated by a **deterministic binary** (`tsift` / `agent-doc`), the single
  writer to the reference registry — no ad-hoc multi-writer races.

This is a *reference-counted lease over a shared local resource with crash-safe
reclamation*, not a distributed consensus problem (single host, multiple
processes).

## Recommended mechanism (primary + backstop)

1. **OS advisory file locks as the primary liveness signal.** The kernel releases
   an `flock`/`LockFile` the instant the holder dies — including `SIGKILL` and
   hard crashes — with **zero false positives**. This is strictly more robust than
   a heartbeat for the single-host case (no false reclaim under GC pause / load
   spike). Pattern:
   - Each session takes a **shared** lock on `.tsift/kg-refs/<resource-key>.lock`.
   - To decide "refcount == 0, safe to unload," the manager attempts a
     **non-blocking exclusive** lock on the same file: it succeeds only when no
     shared locks remain. Kernel-backed refcount-zero detection, auto-reclaimed
     on crash, no manual counter to corrupt.
   - For enumeration/diagnostics, also hold one per-session exclusive lock at
     `.tsift/kg-refs/<resource>/<session>.lock`; sweep by non-blocking try-lock
     (success ⇒ owner dead ⇒ reap row).

2. **Heartbeat + TTL as a secondary reaper** for cases where flock is unreliable
   (NFS-backed state, future cross-host, stuck-but-alive holders). Registry row
   carries `last_heartbeat`; a sweep reclaims rows older than `TTL` (e.g. 3×
   heartbeat interval). Tunable false-reclaim window; only the backstop, not the
   primary.

3. **PID + process-start-time check** as a cheap cross-validation
   (`kill(pid, 0)` / `/proc/<pid>`), pairing PID with start-time to defeat PID
   reuse.

4. **Ollama `keep_alive` as the resource-side safety net.** While refs > 0 keep a
   long `keep_alive`; on last-release send `keep_alive=0` (already used by
   `tsift kg unload`). If the manager itself dies, Ollama's own keep_alive timeout
   still eventually frees VRAM.

## Rust libraries (tsift/agent-doc are Rust)

- `fs4` (maintained fork of `fs2`) or `fd-lock` (used by rustup) — cross-platform
  advisory file locks, shared/exclusive, non-blocking try-lock.
- `named-lock` — cross-platform named mutex (POSIX named sem / Win named mutex)
  if a path-free named primitive is preferred.
- `sysinfo` / `nix::sys::signal::kill` — PID liveness + start-time.
- SQLite (already a dep via libsql/rusqlite) in WAL mode for the `kg_refs`
  registry table; the binary is the single logical writer.

**Distributed analogues (for recognition; do NOT pull in for single-host):**
etcd leases, Consul sessions + health checks, ZooKeeper ephemeral znodes, Redis
TTL keys / Redlock. All implement the same "auto-expire on holder death" idea but
are overkill here.

## Phases

- **Phase 1 — registry + acquire/release.** `kg_refs` SQLite table (session_id,
  resource_key, pid, start_time, host, acquired_at, last_heartbeat, lock_path).
  `tsift kg ref acquire/release --session <id> --resource <key>`. 0→1 load,
  1→0 unload. WAL, single-writer.
- **Phase 2 — crash-safe reclamation.** Per-session flock files + non-blocking
  sweep; PID+start-time cross-check; TTL heartbeat backstop. `tsift kg ref reap`
  + sweep-on-acquire.
- **Phase 3 — agent-doc integration.** agent-doc session start/stop calls
  `kg ref acquire/release`; heartbeat tied to the existing session heartbeat /
  idle-watch so no extra thread.
- **Phase 4 — spec + tests.** SPEC section "KG multi-session reference lifetime";
  SimWorld-style deterministic tests for crash reclamation (spawn holder, SIGKILL,
  assert reap); concurrency test for acquire/release races.

## Spec target

`src/tsift/specs/local-kg-model.md` (or a new `kg-session-lifetime.md`) — document
the lease/refcount contract, reclamation guarantees, and TTL defaults.
