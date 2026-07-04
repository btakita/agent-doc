---
name: reference_tsift_release_publish_list
description: "tsift crates.io release — every crate tsift-cli/root depend on must be in release.yml's TWO publish lists; excluded optional deps block cargo publish"
metadata: 
  node_type: memory
  type: reference
  originSessionId: d646df8b-9c14-491d-88b5-835efcfcac2d
---

tsift releases publish ~19 workspace crates to crates.io via `.github/workflows/release.yml`, triggered by pushing a `v*` tag (verify → 3-OS build → GitHub Release → crates.io publish (idempotent skip-if-exists) → npm). The version bumps in lockstep (all `Cargo.toml` package versions + inter-crate `version = "X"` dep refs + `packages/opencode-tsift/package.json`). crates.io 0.1.62 was already public, so shipping accumulated Unreleased work needs a patch bump (e.g. 0.1.63), not a re-publish.

**Gotcha that caused a multi-attempt failure at 0.1.63:** `cargo publish -p tsift-cli` fails with `error: failed to prepare local package for uploading / no matching package named <X> found` when any dependency of tsift-cli (or root tsift) is not on the crates.io index — **including optional deps behind a feature**. Two such deps were missing: `tsift-memory` (a newer tsift-cli dep) was absent from release.yml's publish list, and `tsift-surrealdb` (tsift-cli's optional `backend-surrealdb` dep) was workspace-`exclude`d and never published. cargo reports only the FIRST missing dep, so fix-and-retry surfaces them one at a time — enumerate all blockers up front with a local dry-run (temporarily neutralize the first missing dep, `cargo publish -p tsift-cli --dry-run --allow-dirty --no-verify`, then revert).

**Rules when adding a crate that tsift-cli or root tsift depends on:**
- Add it to BOTH `for package in \` lists in release.yml — the "Crate package file check" (verify job) AND the publish loop — ordered before `tsift-cli`/`tsift`.
- An optional dep still must be published; you cannot `cargo publish` a crate referencing an unpublished crate. Either publish that crate (un-exclude it into the workspace + add to publish lists; `tsift-surrealdb` is clippy-clean, deps only on `tsift-core`, but pulls heavy surrealdb/cedar/async-graphql so workspace builds get slower) or drop the optional dep from the published manifest.

**Re-trigger after a partial/failed release:** already-published sub-crates skip idempotently, so move the `v*` tag forward to the fix commit and force-push (`git tag -d v0.1.63 && git tag -a v0.1.63 && git push origin v0.1.63 --force`). Confirm success from the per-job `publish crates.io package` conclusion + the crates.io index, NOT the top-line run conclusion — the npm leg failing (see [[reference_opencode_tsift_npm_publish]]) makes the overall run read "failure" even when crates.io + GitHub succeeded.
