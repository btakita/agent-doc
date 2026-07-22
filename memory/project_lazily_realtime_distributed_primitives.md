---
name: project_lazily_realtime_distributed_primitives
description: "8 new lazily reactive-primitive families (realtime + distributed) shipped rs/kt/js + spec/formal, released, Signal demoted; py/zig/dart/go/cpp mirror in progress"
metadata: 
  node_type: memory
  type: project
  originSessionId: af0619ab-e769-43ae-8dc1-b5c1de2066c7
---

Plan `tasks/software/plan-lazily-realtime-distributed-primitives.md` — 8 primitive families added to lazily, each as a pure compute **core** (BytesPayload-eligible, C++-portable) split from a thin reactive **cell** projecting onto a `Cell` with edge/change-only invalidation. Every family: lazily-spec doc + `conformance/<dir>/*.json` fixtures + lazily-formal Lean model + rs/kt/js impl + fixture-replay conformance tests.

**Families (tag → conformance dir):**
- `#lztime` temporal → `temporal/` — TimerCell/IntervalCell/CronCell/DeadlineCell + `TimelineSource` cores, logical clock.
- `#lzrateshape` → `rateshape/` — Debounce/Throttle(leading,trailing)/Sample(count,time)/ProbabilisticSample. Also LIFTED WindowPolicy/ExpiryPolicy/RatePolicy out of `relay_policy` into new `rateshape.rs` (rs re-exports at crate top-level; relay_policy regression test passes unmodified).
- `#lzmemb` → `membership/` — MembershipCell (SWIM + Phi-accrual, Akka logistic phi formula — copy EXACTLY for byte-portable transitions), PeerSet, PeerChangeEvent. Generic peer id.
- `#lzcoord` → `coordination/` — LeaseCell(+monotone fencing token)/LeaderCell/LockCell/SemaphoreCell/BarrierCell+quorum().
- `#lzpresence` → `presence/` — Ephemeral/Presence/AwarenessCell + `Ephemeral`/`Durable` plane markers (durable sink statically rejects ephemeral — rs compile_fail doctest).
- `#lzwindow` → `windowing/` — Tumbling(count,time)/Sliding/Session windows; aggregation reuses `MergePolicy` (aggregate = associative fold).
- `#lzresilience` → `resilience/` — CircuitBreaker(Closed/Open/HalfOpen)/RetryPolicy(exp backoff)/Bulkhead/Timeout.
- `#lzservice` → `service/` — Health/Readiness/Discovery/ServiceRegistry (durable log + replay). Composes memb/lease/resilience.

**Released 2026-07-14** (minor bump, operator-authorized): lazily-spec v0.34.0, lazily-formal v0.34.0, lazily-rs **v0.40.0 (crates.io published)**, lazily-kt v0.29.0 (GitHub Packages via release.yml on v* tag), lazily-js v0.19.0 (tag+gh release; **npm publish still owed — owner 2FA/OIDC**). rs crates.io token: `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`.

**Signal demoted from a core primitive → derived construct** across all docs (`Signal ≡ Slot.eager` = memo Slot + puller Effect). Core primitives are now Cell/Slot/Effect. Coverage table is generated from `lazily-spec/coverage.json` via `node scripts/sync-coverage.mjs` (updates docs/coverage.md + every sibling README between `<!-- coverage-table:start -->` markers; `--check` in CI). New families added to coverage.json (rs/kt/js ✅, py/dart/zig/go/cpp `—` pending mirror).

**Mirror DONE 2026-07-14** — all 8 families ported green to lazily-py (446 tests), lazily-dart (399), lazily-go (360), lazily-cpp (29 ctest), lazily-zig (full `zig build test`); each committed+pushed to its origin/main. coverage.json flipped to ✅ for all 8 families × all 8 languages and re-synced (all repo READMEs committed; `sync-coverage --check` green). **8-BINDING FULL PARITY, ALL RELEASED + PUBLISHED 2026-07-14:** py v0.30.0 (**PyPI**, `make build`+twine, token `passage show btak/PYPI_PUBLISH_TOKEN`), dart v0.19.0 (**pub.dev**, real dart at `~/.puro/envs/default/flutter/bin/cache/dart-sdk/bin/dart` since PATH dart is a broken flutter wrapper; stored creds `~/.config/dart/pub-credentials.json`, `pub publish --force`), go v0.15.0 (module = tag), zig v0.24.0 (tag+gh release), cpp v0.18.0 (tag+gh release). Plus the 5 core repos earlier (rs v0.40.0 crates.io, kt v0.29.0, js v0.19.0, spec/formal v0.34.0). **Remaining gaps: lazily-js npm publish owed (owner 2FA/OIDC — `NPM_PUBLISH_TOKEN` exists in passage but pkg OIDC misconfigured/E404); superproject gitlinks NOT bumped (avoid concurrent-session clobber — operator's call).** [[reference_lazily_release_targets]] [[feedback_stop_cutting_agent_doc_releases]] (that STOP is agent-doc only, not lazily).
