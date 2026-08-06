---
name: reference_lazily_reactivemap_core_release_2026_07_26
description: "lazily ReactiveMap Core-surface release 2026-07-26 — per-binding versions, what published automatically vs manually, and the one blocked target"
metadata:
  node_type: memory
  type: reference
---

**Released 2026-07-26** — the `ReactiveMap` Core surface bound to every flavor in
all nine bindings (see [[project_lazily_reactivemap_unification_rs_reference]]).

| binding | version | target | how |
|---|---|---|---|
| rs | `lazily 0.50.0` | crates.io | manual `cargo publish --locked` (token via passage) |
| py | 0.37.0 | PyPI | **tag → `wheels.yml`**, automatic |
| kt | 0.38.0 | GH Packages | **tag → `release.yml`**, automatic |
| dart | 0.27.0 | pub.dev | manual `dart pub publish --force` (cached OAuth) |
| zig | v0.31.0 | GitHub release | `gh release create` |
| cpp | v0.25.0 | GitHub release | `gh release create` |
| go | v0.23.0 | GitHub release + proxy warm | `gh release create`, then `GOPROXY=... go list -m` |
| cs | v0.2.0 | GitHub release | `gh release create` (no NuGet key yet) |
| js | 0.29.0 | npm | **operator** `npm publish` with OTP (CI OIDC path failed) |

**js npm: the CI path is still broken, but the operator path works — and this is
the standing division of labour.** `@lazily-hub/lazily-js` **does exist** on npm,
so this was never a missing-package problem. Two distinct failures:

- The tag-triggered `release.yml` OIDC publish fails with
  `npm error code E404 / 404 Not Found - PUT https://registry.npmjs.org/@lazily-hub%2flazily-js`.
  The trusted-publisher config does not authorize that workflow. **Still broken —
  every release will fail this job until the npm trusted-publisher setup for the
  repo is finished.** Treat a red `Release` run on lazily-js as expected, not a
  regression, and check npm directly.
- A local `npm publish --access public` as `btakita` (already logged in) returns
  `EOTP: This operation requires a one-time password` with a browser auth URL.
  The agent cannot complete this; **the operator runs it.** They did for 0.29.0,
  verified live with `latest -> 0.29.0`.

So the working recipe for js is: agent bumps + tags + pushes (which creates the
GitHub release and fires the doomed OIDC job), then the **operator** runs
`npm publish` from `src/lazily-js` with the OTP. Do not treat the failing CI job
as a blocker on the rest of the release.

**Version realignment:** `lazily-py/src/lazily/__init__.py __version__` had drifted
to `0.33.0` while `pyproject.toml` said `0.36.0`. Both are `0.37.0` now — check
this pair on every py release.

**rs release gotchas that bit here:**
- `make check` runs `--locked`, so bump `Cargo.toml` *and* run
  `cargo update -p lazily --precise X.Y.Z` before checking, or clippy fails with
  "cannot update the lock file … --locked was passed".
- `BENCHMARKS.md`'s "Generated for package `lazily` version `X.Y.Z`." line must
  be bumped in the same commit (the benchmark-version spec test).
- `benchmark-check` fails locally with `no Criterion estimates found` in a tree
  that never ran benches. Environmental, not in CI — everything else was green
  (624 `test result: ok`, 0 failed).

**zig tag push:** `git push --tags` is rejected wholesale because a *stale unrelated
local* `v0.25.0` differs from the remote. The new tag still lands — verify with
`git ls-remote --tags origin` rather than trusting the non-zero exit.

Related: [[reference_lazily_release_targets]], [[reference_lazily_v2_release_2026_07_22]],
[[reference_lazily_js_npm_trusted_publisher_e404]], [[reference_cargo_publish_token_passage]].
