---
name: reference_mutation_check_false_green_from_stale_build_cache
description: "Mutation checks report false \"still green\" when the build cache serves pre-mutation artifacts — pytest __pycache__, cargo mtime, cmake objects"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 3d03b5bd-7fd7-4085-a88b-d60d10242690
  modified: 2026-08-02T20:38:07.279Z
---

A mutation check that cannot invalidate its own build is just another false green, one level up. Confirmed in the lazily fail-closed dispatch sweep (2026-08-02): the lazily-py pass reported 5 sites as "still green after mutation" — all 5 were real fail-opens. Cause was stale `__pycache__`: mutate → run → restore cycles landing inside the same filesystem second let pytest reuse the pre-mutation rewritten bytecode.

**Why:** every mutation-check harness assumes the runner recompiles what it just edited. Source-mtime-keyed caches (Python bytecode, cargo fingerprints, cmake object timestamps) break that assumption exactly when the mutate/restore loop is fast — which is always, because the loop is scripted.

**How to apply:** per mutation case, defeat the cache explicitly.
- Python: `PYTHONDONTWRITEBYTECODE=1` **and** clear `__pycache__` per case.
- Rust/C++/anything mtime-keyed: `touch` every file after restoring it (see [[reference_cargo_stale_build_when_mutation_check_restores_mtime]]).
- Zig: the working driver `utime`d after restore for the same reason.

Treat a "mutation survived" result as *suspect until the cache is proven cold* — the failure mode is asymmetric: a stale cache can only manufacture false greens, never false reds. Related: [[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]], [[reference_lazily_conformance_delivery_three_failure_modes]].
