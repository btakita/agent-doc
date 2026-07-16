---
name: reference_lazily_publish_and_coverage_generator
description: lazily release mechanics per binding + the single-source feature-matrix generator
metadata: 
  node_type: memory
  type: reference
  originSessionId: e8ca3026-6052-4ed4-b68b-fdce0aa59d52
---

lazily bindings publish via CI on a `v*` tag push:

- **lazily-js → npm**: `.github/workflows/release.yml` uses **OIDC trusted publishing** but the npm Trusted Publisher is NOT configured for `@lazily-hub/lazily-js`, so CI publish 404s (E404 on PUT). Workaround: publish locally — `btakita` has read-write on `@lazily-hub/lazily-js`, run `npm publish --access public` in `src/lazily-js`, but npm 2FA is passkey/OTP so it returns `EOTP` and needs the operator to complete auth. Package exists (published up to **0.7.0**, latest=0.7.0). To fix permanently: configure the npm Trusted Publisher (org `lazily-hub`, repo `lazily-js`, workflow `release.yml`). **Confirmed 2026-07-10 (v0.8.0):** both `~/.npmrc` `_authToken` AND passage `npm/token` (40-char) now return `E401` on `npm whoami` — stale — so no non-interactive publish path exists; operator must `npm login` (2FA passkey/OTP) then `npm publish --access public` in `src/lazily-js`. Git tag/commit/CI-trigger all succeed; only the registry PUT is blocked.
- **lazily-kt → GitHub Packages**: tag push → `./gradlew publish` (uses `GITHUB_TOKEN`); works in CI. Note: kt CI `test` job checks out ONLY lazily-kt (no lazily-spec sibling), so any conformance test must have its fixture VENDORED under `src/test/resources/conformance/` (classpath fallback) or CI fails.
- **lazily-spec / lazily-formal**: push to main → mdBook Pages deploy + CI. Pages "Deployment failed, try again later." is a transient GitHub infra error — just `gh run rerun <id> --failed`.

Single-source feature matrix (so the coverage table is edited in ONE place): canonical data = `lazily-spec/coverage.json`; `lazily-spec/scripts/sync-coverage.mjs` renders it into `docs/coverage.md` and every sibling `../lazily-*/README.md` between `<!-- coverage-table:start/end -->` markers. `make coverage-sync` updates all; `make coverage-check` (in `make check` + spec CI) fails on drift. Sibling repos absent → skipped, so spec CI only guards coverage.md. See [[reference_lazily_release_targets]].
