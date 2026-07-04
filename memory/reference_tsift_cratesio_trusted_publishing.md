---
name: reference_tsift_cratesio_trusted_publishing
description: tsift crates.io OIDC trusted publishing configured for 23 crates — but tsift-local-model + tsift-kg (added in v0.1.73) were token-bootstrapped and still need trusted-publisher config or future CI publishes fail on them
metadata: 
  node_type: memory
  type: reference
  originSessionId: f9b476fd-c03b-4ed3-89d5-1c9f28965da8
---

crates.io OIDC Trusted Publishing for tsift is fully set up as of 2026-06-19. A GitHub Actions Trusted Publisher is registered for ALL 23 crates in `release.yml`'s publish list (`repository_owner=btakita`, `repository_name=tsift`, `workflow_filename=release.yml`, no environment). The repo var `TSIFT_ENABLE_CRATES_PUBLISH=true` (set 2026-05-21) gates the crates job. The OIDC migration code (`#1td4`, `rust-lang/crates-io-auth-action@v1`) shipped earlier in fe4144b/f29f7a2.

This completed the operator-gated `#vmax`/`#1td4` (both now done-archived in tasks/software/tsift.done.md). **Future `v*` releases auto-publish to crates.io — no token/secret, no per-crate registration.** Do NOT re-treat crates.io publish as operator-gated.

**EXCEPTION — `tsift-local-model` + `tsift-kg` (NEW in v0.1.73):** these two crates are NOT yet registered as Trusted Publishers. They were created on crates.io via a **manual token publish** during the v0.1.73 release (2026-06-21, all 25 crates published in dep order with the local `CARGO_REGISTRY_TOKEN`). crates.io has **no "pending publisher"** like PyPI — you CANNOT pre-configure trusted publishing for a crate that doesn't exist; a new crate's first publish MUST be manual with a token, THEN you register the trusted publisher. Until these two are registered, a future `v*` tag's CI crates job will FAIL on them (OIDC has no config). Tracked as the now-OPTIONAL/non-blocking `#1yks` in tasks/software/tsift.md. So: 23 crates auto-publish via OIDC; **these 2 still need the one-time trusted-publisher registration** (operator browser/passkey, or chromium-bridge POST below).

Setup mechanism (for re-use / adding a new crate to the set): the registration is a `POST /api/v1/trusted_publishing/github_configs` with body `{github_config:{crate,repository_owner,repository_name,workflow_filename,environment}}` against an authenticated crates.io session — done over chromium-bridge `evaluate --tab "crates.io/users"` (operator's browser logged in as btakita). chromium-bridge `evaluate` does NOT await promises: use a `window.__x='pending'; fetch(...).then(d=>window.__x=...)` global-var + poll pattern. The crate must already exist on crates.io and you must own it (both now do, after the v0.1.73 token publish). Related: [[reference_opencode_tsift_npm_publish]] (npm OIDC).
