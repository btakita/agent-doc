# Runbook: crates.io Trusted Publishing (OIDC)

Add a **new crate** to crates.io OIDC Trusted Publishing so `v*`-tag releases auto-publish with **no long-lived `CARGO_REGISTRY_TOKEN`** and **no OTP prompt during CI**. Working reference: `src/tsift/.github/workflows/release.yml`.

## What it is / why
Trusted Publishing lets crates.io accept a **short-lived token minted from the GitHub Actions OIDC identity**, scoped to one repo + workflow. No secret stored in GitHub, nothing to rotate or leak. The CI side is fully scriptable; the crates.io-side per-crate registration is a **web-UI + browser/passkey 2FA** step — operator-only, **cannot be API-automated**.

## Prerequisites
- The crate must already **exist** on crates.io (≥1 published version) — Trusted Publishing is configured on an existing crate's settings page. A brand-new name has no page yet (see **Bootstrap**).
- A GitHub repo with a release workflow triggered on `v*` tags.
- crates.io crate ownership + 2FA. The `btakita` account uses **browser/passkey** 2FA (not TOTP) — approve via the browser link from a real terminal; don't chase authenticator apps.

## 1. CI workflow setup (scriptable)
In the release workflow's publish job:
```yaml
permissions:
  id-token: write   # REQUIRED — lets the job mint the OIDC token
  contents: read
steps:
  - uses: actions/checkout@v4
  - uses: dtolnay/rust-toolchain@stable
  - name: Authenticate to crates.io via OIDC trusted publishing
    id: auth
    uses: rust-lang/crates-io-auth-action@v1   # exchanges OIDC -> short-lived crates.io token
  - name: Publish crate
    run: cargo publish -p <crate> --locked
```
`rust-lang/crates-io-auth-action@v1` exports the token into cargo's env — no `CARGO_REGISTRY_TOKEN` secret needed. **Delete** any `NPM_TOKEN` / `CARGO_REGISTRY_TOKEN` secret once trusted publishing is live (E403-prone and now unused).

## 2. Per-crate registration on crates.io (operator, browser + 2FA — NOT automatable)
For **each** crate:
1. Open `https://crates.io/crates/<crate>/settings` → **Trusted Publishing**.
2. **Add** a GitHub publisher:
   - Repository owner: `<owner>` (e.g. `btakita`)
   - Repository name: `<repo>` (e.g. `tsift`)
   - Workflow filename: `release.yml` (filename only, not a path)
   - Environment: leave blank unless the job uses a GitHub Environment.
3. Confirm via browser/passkey 2FA.

## 3. Bootstrap a brand-NEW crate name (the gotcha)
A name with no published version has no crates.io page, so step 2 can't be done yet. Two paths:
- **Preferred:** pre-register the Trusted Publisher for the not-yet-published name from your account's publishing settings; the first CI `v*` publish then creates the crate.
- **Fallback** (if crates.io rejects pre-config for a nonexistent name): do **one** manual `cargo publish -p <crate>` with a temporary token to reserve the name + create the page, then register the Trusted Publisher per step 2. All later versions go tokenless via CI.

## 4. Multi-crate workspaces (e.g. tsift)
- **Every** crate the top-level crate depends on (**including optional feature deps**) must (a) exist on crates.io, (b) have a Trusted Publisher configured, and (c) appear in the workflow's publish loop — or `cargo publish -p <top>` fails *"no matching package found"*. Keep the `for package in …` list(s) complete and in dependency order. (See memory `reference_tsift_release_publish_list`.)
- Publishes are **idempotent**: a re-run skips crates whose version already exists (tsift's `crate_version_exists` guard), so a force-moved `v*` tag safely re-triggers and unpublished tags can be moved forward to HEAD rather than version-bumped.

## 5. Verify
- `git tag vX.Y.Z <sha> && git push origin vX.Y.Z` → Release CI runs.
- Judge success by **each per-crate crates.io job conclusion**, not the aggregate top line (npm/other steps can poison it).

## Status (this repo)
- **tsift:** 23 crates configured for OIDC trusted publishing (2026-06-19) — future `v*` releases auto-publish tokenless; do **not** treat crates.io publish as operator-gated for those. `#1yks` tracks bootstrapping the last two new crates (`tsift-local-model`, `tsift-kg`) if not yet registered.
- **opencode-tsift (npm):** OIDC trusted publishing configured (2026-06-06); `v*` releases auto-publish, no token/OTP.
