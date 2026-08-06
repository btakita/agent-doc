---
name: reference_lazily_queue_family_release_2026_07_26
description: lazily queue-family flavor release 2026-07-26 — rs 0.51.0 is the only behaviour change; the other 7 were patch bumps for the coverage-table sync
metadata: 
  node_type: memory
  type: reference
  originSessionId: a5a9e782-82c8-4457-962f-9adae81c9c8a
  modified: 2026-07-26T19:00:41.716Z
---

**Released 2026-07-26**, second release that day. rs shipped the topic and
work-queue thread-safe/async flavors (see
[[project_lazily_queue_family_flavors_rs_reference]]); every other binding got a
**patch** bump because `make coverage-sync` regenerates the shared coverage table
into all nine READMEs and only the Rust column moved.

| binding | version | how |
|---|---|---|
| rs | `lazily 0.51.0` | crates.io, manual `cargo publish --locked` (token via passage) — **the only behaviour change** |
| py | 0.37.1 | tag → `wheels.yml` |
| kt | 0.38.1 | tag → `release.yml` (GH Packages) |
| dart | 0.27.1 | `dart pub publish --force`, cached OAuth, live |
| zig | v0.31.1 | `gh release create` |
| cpp | v0.25.1 | `gh release create` |
| go | v0.23.1 | `gh release create` + proxy warm — **CI red, see [[reference_lazily_go_thread_safe_map_data_race_ci_red]]** |
| js | 0.29.1 | agent tagged; **operator ran `npm publish` with the OTP** — the standing division of labour |
| cs | — | deliberately excluded from this release |

**Patch, not minor, for the seven doc-only bindings.** Their behaviour did not
change and the flipped ✅ marks belong to Rust. lazily-dart's CHANGELOG says this
out loud, which is worth copying: a reader skimming a binding's README table
could otherwise misread the sync as *that* binding gaining the flavors.

**Worth knowing before the next one:**
- lazily-go's prior coverage syncs (`afcc246`, `ddffea2`, `ac131c2`, `31204fb`)
  were plain `docs:` commits that never got a tag. If the convention is that
  coverage syncs ride the next real release, the go tag here was optional.
- lazily-js `package-lock.json` still says `0.27.0` at its root — pre-existing
  drift, the 0.29.0 release did not fix it either.
- js `Release` CI is still expected-red on the OIDC E404
  ([[reference_lazily_js_npm_trusted_publisher_e404]]); check npm directly.

Mechanics unchanged from [[reference_lazily_reactivemap_core_release_2026_07_26]]:
bump `Cargo.toml` **and** `cargo update -p lazily --precise X.Y.Z` (make check is
`--locked`), bump BENCHMARKS.md's "Generated for package" line in the same commit,
and expect `benchmark-check` to skip with no Criterion evidence.
