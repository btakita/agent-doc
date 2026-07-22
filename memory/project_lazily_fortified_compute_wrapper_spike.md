---
name: project_lazily_fortified_compute_wrapper_spike
description: "Fortified value-threaded Compute wrapper — SHIPPED in lazily v2 (2026-07-22) across rs/js/dart/go/zig/cpp/kt + spec + formal; thread-local/ambient tracking removed (rs) or bridged (py). NOT a spike anymore."
metadata:
  node_type: memory
  type: project
  originSessionId: 319d60e9-f1fe-4008-a70c-af4e66292817
  modified: 2026-07-22T06:37:09.439Z
---

**SHIPPED 2026-07-22** as part of the lazily v2 release (user drove it). Extends
[[project_lazily_get_set_unification_and_uniform_wrapper]]. Supersedes the old
"stashed spike" note — the cascade was dissolved via the **ComputeOps trait**
(genericize handle methods over the ctx: `fn get<C: ComputeOps>(&self, ctx: &C)
where Self: Read<C>`) instead of a concrete-type flip.

**What shipped:** a fortified, non-escapable `Compute<'a>` is the SOLE tracking
surface handed to every `computed`/`slot`/`computed_ripple_when`/effect recompute
closure. It carries the recomputing node id **as a value** (`slot_id`), so tracked
reads attribute correctly across `.await` (value survives suspension; a
thread-local is clobbered). `ComputeOps` = the compute-time SUBSET of the Context
API (get/set/source/cell/computed/computed_ripple_when/slot/effect/batch/…),
implemented by exactly two types: Context (untracked) and Compute (tracked).
Formal invariant proved in Lean: every dep edge registered during a recompute of
node n has n as dependent (`registerReads_dependent_is_recomputing_node`).

**rs**: thread-local `TRACKING_STACK` fully DELETED (GAT `type Compute<'a>` on the
capability trait). **js/dart/go/zig/cpp/kt**: ambient tracking removed / value-
threaded. **py**: full ambient removal SHIPPED in 0.35.0 (2026-07-22) — bridge
deleted after threading the caller's Compute reader into bare-read sites +
ctx-param family factory. All 8 bindings now value-threaded-only. See
[[project_lazily_py_ambient_removal_blocked_on_bare_reads]].

**Release (2026-07-22):** rs `lazily 0.47.0` + `lazily-macros 0.23.0` (crates.io),
go v0.21.0 / zig v0.29.0 / cpp v0.23.0 (GH releases), kt 0.35.1 (GH Packages;
0.35.0 + async batch-race fix), py 0.35.0 (PyPI; full ambient removal),
spec (GH Pages) + formal pushed, superproject pointers bumped. js 0.26.0 / dart
0.25.0 tagged; registry publish pending operator 2FA.
