# lazily #lzspecedgeindex — fan-out, disposal, and the family port

Continuation plan. Session of 2026-07-19 fixed an O(n²) dependency-edge defect
in `lazily-rs` and, chasing it with a load test, found four more. Five of eight
bindings still carry the original defect and six lack disposal entirely.

## State

Green: `make check` passes in `lazily-rs` and `lazily-spec`. Trees clean.

| repo | commits | note |
|---|---|---|
| `src/lazily-rs` | 15 | edge index, hasher, disposal, `ChildContext` |
| `src/lazily-spec` | 2 | `#lzspecedgeindex` implementation note + disposal |
| `src/ctx-core-dev` | 6 | rmemo drain + error contract (separate thread, done) |

## Baselines — re-measure before claiming any improvement

Every number below was taken on this box (Ryzen 9 9950X3D, 186 GiB, CachyOS,
`rustc` stable) against a **clean `git worktree` at the parent commit**, not from
memory. Do the same. Single runs drift ±30% on the narrow cases; take medians.

```sh
cd src/lazily-rs
cargo run --release --example fanout_scaling            # narrow vs wide, n=500..4000
cargo run --release --example pubsub_load               # width ladder + churn soak
LAZILY_PUBSUB_MAX_WIDTH=10000000 cargo run --release --example pubsub_load
cargo run --release --features thread-safe --example pubsub_load_thread_safe
cargo run --release --example rc_handle_spike           # task 12 option B
cargo run --release --example child_context_spike       # task 12 option C (shipped)
make benchmark-update                                   # regenerates BENCHMARKS.md
```

`pubsub_load` climbs a width ladder, projecting each rung's memory from the
previous one's measurement and refusing to build if it would not fit
(`LAZILY_PUBSUB_MEM_FLOOR_GIB`, default 16). 100M needs ~25 GB and ~5 minutes.

### Current numbers

Single-threaded `Context`, `pubsub_load`:

| width | build ns/sub | notify ns/sub | unread publish | bytes/sub |
|---:|---:|---:|---:|---:|
| 1,024 | 102 | 42.8 | 1,557 ns | 200 |
| 65,536 | 112 | 61.1 | 110 µs | 260 |
| 1,000,000 | 178 | 175.6 | 8.8 ms | 268 |
| 10,000,000 | 278 | 302.0 | 118 ms | 252 |
| 100,000,000 | 344 | 351.6 | 865 ms | 247 |

Per-subscriber memory is flat 260 → 247 B across a 1,500× range; that is what
makes the ladder's projection trustworthy. 100M = 23.5 GiB.

Session deltas, all measured against clean baselines:

| | before | after |
|---|---:|---:|
| wide fanout @ 4,000 | 47.9 ms | 2.25 ms (21×) |
| build @ 100M width | never finishes (~5×10¹⁵ ops) | 34 s |
| `ThreadSafeContext` build @ 262k | 50,799 ns/sub | 667 ns/sub (76×) |
| churn memory | 313.9 B/cycle, never reclaimed | 0.00 B/cycle |
| churn time | 23,419 ns/cycle | 126 ns/cycle (186×) |
| indexed registration | 83 ns | 45 ns |

`ChildContext` vs disposing by hand, per node: build 156.9 → 72.5 ns @1024,
teardown 61.1 → 56.1 ns @65,536.

## Load test strategy — ≥1M fan-out

Every binding gets this before its port is called done. The whole reason five
defects hid for so long is that `benches/scale.rs` measures scale as *node
count* and fixes fan-out at 2, so width was never a variable.

### Why ≥1M is the floor

Two things only become visible there:

- **The quadratic becomes fatal rather than slow.** Building a width-N topic
  with an unconditional scan is ~N²/2 comparisons. At 4,096 that is 8M and
  looks like a 20% regression; at 1M it is 5×10¹¹ and takes minutes; at 100M it
  is 5×10¹⁵ and never returns. A ladder that stops at 65k cannot tell "slower"
  from "impossible".
- **The workload goes memory-bound.** On this box, notify jumps 60 → 151 ns/sub
  between 65k and 1M — exactly where the working set (15 MiB → 236 MiB) crosses
  the 128 MiB L3. Any optimisation measured only below that boundary is
  measuring a different machine than production.

### Method: climb, project, refuse

Never jump straight to the target. At each rung measure bytes/subscriber,
project the next rung from *that measurement*, and abort if the projection
would not leave a floor free. `pubsub_load` does this
(`LAZILY_PUBSUB_MEM_FLOOR_GIB`, default 16); it printed

```
stopping before width N: projected X GiB from the last rung's Y B/subscriber,
but only Z GiB available with a 16 GiB floor
```

which is what makes a 23.5 GiB run safe to start. This is haiven's method: it
proved 698 B/conn flat from 5M to 100M *before* attempting 100M. A wrong guess
at this scale takes the machine down.

Rung shape that worked: `32, 64, 96, 128, 129, 160, 256, 1k, 4k, 64k, 1M, 10M,
100M`. The cluster around the promote threshold is not decoration — it is what
exposed the 371 ns/sub demotion thrash at exactly threshold+1, invisible at any
other width.

### What to assert, not just print

A load test that only prints numbers gets skimmed. Each of these has an answer
that could embarrass the library:

| assertion | why |
|---|---|
| build ns/sub grows < 2× from 1k → 1M | linear build; catches any surviving O(n²) |
| bytes/sub flat within ~20% across the ladder | capacity planning is only possible if per-entity cost is flat — haiven's central finding |
| churn returns to baseline RSS (≈0 B/cycle) | catches the disposal leak; needs disposal to exist first |
| every survivor observes the final publish | a stale index entry shows up as a missed update, not a crash |
| notify unchanged by an edge-index change | propagation walks bare ids; if it moves, the model is wrong |

### Per-binding targets

1M is the common floor. Above it is per-binding and must be *measured*, not
assumed — Rust reaches 100M at 247 B/subscriber in 23.5 GiB, and nothing else
should be expected to match that without evidence.

| binding | expect | watch for |
|---|---|---|
| rs, cpp, zig | 10M–100M | flat bytes/sub is the headline claim |
| go | 10M | GC pause during the ladder; measure with GC stats on |
| kt | 1M–10M | JVM needs an explicit heap flag; object header inflates bytes/sub |
| py | 1M | per-object overhead is large; 10M may be impractical and that is a fine result to report |
| js, dart | 1M–10M | V8/Dart old-space cap; `--max-old-space-size` needed past ~1M |

A binding that cannot reach 1M should say so with the measured ceiling and the
limiting factor. That is a real finding, not a failure.

### Out of scope

Do not port haiven's shed, backpressure, or latency-percentile suites. A
reactive graph has no queue, no admission control and no client — under load
`get` simply takes longer. Deliveries/sec has no analogue either; criterion
measures per-operation cost better than a load generator would.

These runs are minutes long and allocate tens of GiB. Keep them **manual and
on-demand**, like haiven's, not in CI. `make benchmark-check` stays the CI gate.

## What was wrong, and what it cost to find

Worth reading before touching the other bindings, because every one of these is
a trap the ports will hit.

1. **The threshold is not portable.** 32 came from a JS measurement. In Rust the
   crossover measured near 170 with SipHash and near 40 once ids were hashed
   with splitmix64 — a 4× shift from changing the hash function alone. Shipping
   32 made widths 33–170 *slower than before the fix*. Now 32 again, for a
   different reason.
2. **Demotion thrashes without hysteresis.** A dependent list oscillates by one
   on every recompute, so a single shared promote/demote boundary rebuilds the
   index every time: 371 ns/sub at the boundary against 89 either side.
3. **`ThreadSafeContext` has its own everything** — its own `edge_insert`, its
   own `HybridSet`/`HybridMap`. It is not a default feature, so `cargo build`
   and `cargo test` never compile it. Two separate linear scans there, and the
   second was only visible after fixing the first.
4. **Nothing could be disposed.** No `dispose_slot`/`dispose_cell` existed;
   `SlotHandle` is `Copy`, so dropping every handle reclaimed nothing.
5. **Disposal dropped nodes under the `RefCell` borrow**, so any capture whose
   `Drop` re-entered the context panicked. Live bug, reachable from safe code.

Findings 3–5 were invisible to the existing suite. `benches/scale.rs` runs 2M
nodes but fixes fan-out at 2, so it structurally could not see any of this.

**Method note:** every conclusion reached by inspection this session was wrong
at least once (the boxed field's cost, the `len()` explanation, the demote
experiment, the threshold twice, "per-node RAII is impossible"). Every
conclusion reached by bisection or a clean-baseline measurement held. Bisect.

## Next, in order

### 1. Port to the remaining bindings (task #4) — subagent fan-out

Five independent repos, no shared files. Good parallel work.

- `lazily-rs` — done, the reference
- `lazily-kt`, `lazily-cpp`, `lazily-zig`, `lazily-dart` — unconditional linear
  scan in edge dedup, same defect
- `lazily-js` — same; a hybrid prototype measured 11.4× at n=4000 with no narrow
  regression, not yet applied
- `lazily-go`, `lazily-py` — already hash-set edges, no change needed

Brief each agent with `lazily-spec/docs/reactive-graph.md` (the
`#lzspecedgeindex` implementation note), not with lazily-rs's constants. Require
a measured before/after from a clean baseline per binding. Reject any
performance claim without one — inventing plausible numbers is the likely
failure mode.

`lazily-zig` has an extra defect: `EdgeSet.remove` (`context.zig:558`) is a
linear scan called during cascade, an O(in-degree) cost per edge on wide fan-in
that no other binding has.

### 2. Family disposal decision (task #11) — needs a human

Only `lazily-js` and `lazily-rs` can dispose slots/cells. go/py/kt cannot, and
GC does not save them: `lazily-py/src/lazily/cell.py:28` holds
`_parents: set[Slot]` and `lazily-go/core.go:54` holds
`dependents map[reactiveNode]struct{}` — both strong, so a long-lived topic
keeps every slot that ever read it. cpp/zig/dart unchecked.

Three mechanisms, all spiked and measured in `lazily-rs` (see task #12):
explicit disposal, `ChildContext` groups, and `Rc`/`Weak` handles. Recommended:
groups + explicit, both on `Copy` handles. Only `Rc`/`Weak` closes the
cross-owner hazard, and it costs `Copy` — a `.clone()` at every fan-out capture
site, or a gtk-rs-style `clone!` macro. The GC'd bindings may want weak
back-edges instead, which is what `ctx-core`'s rmemo does and needs no API at
all. Decide before writing the same thing five times.

### 3. Flatten per-subscriber cost (task #10) — gated, in order

Growth past 1M is memory-bound, not algorithmic: notify jumps 60 → 151 ns/sub
exactly where the working set crosses this box's 128 MiB L3.

1. Measure where 247 B/node goes. Hypothesis, **unverified**: a node is 3–4
   scattered allocations (arena entry, `Rc<dyn Fn>` closure, `Arc<dyn Any>`
   value), so one logical touch is several cache misses.
2. Test transparent huge pages. 23.6 GiB in 4 KiB pages is ~5.9M PTEs.
3. Only then consider a dense flag arena for invalidation. Marking needs one bit
   but pays a full node touch; `lazily-js` already does this
   (`#lzjsarenanodes`). Would target the 865 ms unread publish at 100M.

Do not start 3 before 1 and 2.

### 4. Release: lazily family + ctx-core (tasks #5, #6)

Do not start until 1 and 2 land — publishing a partial family means bindings
disagreeing about `#lzspecedgeindex`.

#### Blockers, resolve first

- **`npm whoami` returns E401.** The stored `~/.npmrc` token is dead, so
  `publish--force` fails on all 192 `ctx-core` packages. Needs a human login.
  That login is the moment to switch to an **org-scoped granular token** rather
  than restoring a classic one — npm is deprecating 2FA-bypass tokens
  (`gh.io/npm-gat-bypass2fa-deprecation`).
- **`BENCHMARKS.md` must be regenerated** (`make benchmark-update` in
  `lazily-rs`) or `make check` fails on `benchmark-check`. Its earlier red was
  stale criterion data, not a regression — do not misread it as one.
- **Version bumps + `VERSIONS.md`** before any tag. `lazily-py` also needs
  `pyproject.toml`.

#### Publish order

Spec first, because the bindings replay its conformance fixtures.

1. `lazily-spec` — no registry; tag only. Version field is `"0"`; protocol
   versioning is per-message, not per-repo.
2. `lazily-formal` — no registry; tag only.
3. `lazily-rs` — crates.io. **Currently 0.45.0.**
4. Bindings in any order — kt, py, js, go, dart, cpp, zig, react.
5. `ctx-core` last: it is a separate dependency graph and does not consume
   lazily.

#### Per-target mechanics

| target | registry | how | auth |
|---|---|---|---|
| `lazily-rs` | crates.io | **local `cargo publish`** — only `ci.yml`, `mdbook.yml`, `regressions.yml` exist; no release workflow | **interactive** — use `CARGO_REGISTRY_TOKEN="$(passage show btak/CARGO_REGISTRY_TOKEN)"`; `credentials.toml` is stale and 403s |
| `lazily-js` | npm `@lazily-hub/lazily-js` | CI `release.yml`, OIDC + `--provenance` | none, *if* the Trusted Publisher is registered on the package — otherwise E404 on PUT |
| `lazily-py` | PyPI | CI `wheels.yml` on tag | none — `PYPI_PUBLISH_TOKEN` secret (`passage btak/PYPI_PUBLISH_TOKEN`) |
| `lazily-kt` | GitHub Packages | CI `release.yml` on `v*` tag, `GITHUB_TOKEN` | none |
| `lazily-go` | Go proxy | tag push, proxy pulls | none |
| `lazily-dart` | pub.dev | **local `dart pub publish`** — `ci.yml` is test-only | **interactive** — Google OAuth, expires |
| `lazily-cpp`, `lazily-zig` | GitHub Releases | tag + `gh release create` | none (persisted `gh` token) |
| `lazily-react` | npm | **not wired** — no workflows, `provenance: false` | **interactive** on first publish; npm has no pending-publisher, so the package must exist before a Trusted Publisher can be attached |
| `ctx-core` | npm ×192 | **local** `publish--force` | dead token — see blockers |

Three interactive auths in the steady state: crates.io, pub.dev, and
`lazily-react`'s first publish. Everything else is CI or a persisted token.

**Gotchas:** `lazily-kt` publishes to *immutable* GitHub Packages — check the
registry before tagging, a 409 kills the run. `lazily-zig` has no CI at all.
`coverage.json` is a hot multi-session file; commit it alone.

#### ctx-core (task #6, scope decided: changeset-affected only)

Three steps, from `ctx-core-dev/AGENTS.md`:

```sh
changeset-version-COMMIT_EDITMSG   # applies changesets, fills COMMIT_EDITMSG
recursive-git-commit               # commits per workspace package
publish--force                     # parallel bun publish; skips unchanged versions
```

`publish--force` (`lib/monorepo/bin/publish--force.sh:61`) runs
`bun publish --tag latest --access public` across **all 192 publishable packages
concurrently via GNU parallel**, comparing local version against the registry
and skipping matches. **Scope is changeset-affected only** — let changesets bump
`rmemo`, `ctx-core` and their dependents; do not force-publish the whole
monorepo.

This session changed `ctx-core/all/rmemo` (drain, error contract, `rmemo__warm`)
and `lib/rmemo` (size limits, README). Both are committed but have **no
changeset yet** — write one before versioning. Note the rmemo error-contract
change is **breaking**: a throwing memo now propagates instead of yielding
`undefined`.

#### Auth work (task #5, decided: move ctx-core into CI)

`ctx-core-dev` has **no `.github/` directory at all**. Add a tag-triggered
release workflow using one org-scoped granular npm token, so publishing leaves
the laptop and the credential is scoped and expiring rather than a long-lived
token with write access to 192 packages.

Full npm OIDC is *not* the near-term answer: Trusted Publishers are configured
**per package**, so covering `@ctx-core/*` means 192 separate registrations —
strictly more clicking than today. Stage it onto the ~14 high-blast-radius
standalone packages (`rmemo`, `relementjs`, `rebuildjs`, `esmcss`, `hyop`, …)
after the CI workflow exists.

Worth doing at the same time, each removing one interactive auth or one stored
secret:

- **crates.io** — add a release workflow to `lazily-rs` with
  `permissions: id-token: write` + `rust-lang/crates-io-auth-action@v1`, then
  register the Trusted Publisher for crate `lazily`. Highest value: also moves
  the release off the laptop.
- **pub.dev** — supports GitHub Actions publishing natively. Configure on the
  `lazily` package (repo `lazily-hub/lazily-dart`, tag pattern `v{{version}}`)
  and use `dart-lang/setup-dart`'s publish workflow. No secret at all.
- **PyPI** — replace the `PYPI_PUBLISH_TOKEN` secret in `wheels.yml` with a
  Trusted Publisher; retires a stored credential.

#### Current versions, for reference

`lazily-rs` 0.45.0 · `lazily-py` 0.33.0 · `lazily-js` 0.24.0 ·
`lazily-dart` 0.23.0 · `lazily-react` 0.1.0 (never published)

## Files worth reading first

- `src/lazily-rs/src/context.rs` — `EDGE_INDEX_THRESHOLD`, `EdgeIndex`,
  `SlotIdHasher`, `dispose_slot`, `ChildContext`
- `src/lazily-rs/src/thread_safe.rs` — `HybridSet`/`HybridMap`,
  `EDGE_HYBRID_THRESHOLD`
- `src/lazily-spec/docs/reactive-graph.md` — the normative note
- `src/lazily-rs/examples/pubsub_load.rs` — the ladder that found most of this
