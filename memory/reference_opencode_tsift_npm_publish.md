---
name: reference-opencode-tsift-npm-publish
description: "opencode-tsift npm publish: use OIDC trusted publishing (no token); btakita account enforces 2FA-bypass so classic AND granular tokens E403; first publish needs a manual OTP bootstrap"
metadata:
  node_type: memory
  type: reference
  originSessionId: c458d4ef-9192-4854-89d1-c93d5b5acf2d
---

Publishing **opencode-tsift** (the OpenCode plugin for tsift) to npm.

- **Package:** `src/tsift/packages/opencode-tsift/` — name `opencode-tsift` (unscoped), `publish:check` = `npm publish --access public --dry-run`. Still **not on npm** as of 2026-06-06 (404 — never bootstrapped).
- **CI publish path:** `src/tsift/.github/workflows/release.yml` job `publish-opencode-plugin`, on a `v*` tag, gated by `vars.TSIFT_ENABLE_NPM_PUBLISH == 'true'`.

**Tokens do NOT work for this account — stop trying them.** The `btakita` npm account enforces 2FA for writes, so publish fails `E403: "Two-factor authentication or granular access token with bypass 2fa enabled is required"`. Verified 2026-06-06 with BOTH a classic/automation token AND the granular-named token at `passage btak/GRANULAR_NPM_TOKEN`: each authenticates (`npm whoami` → `btakita`) but the publish PUT is forbidden, identically in CI and via clean local `npm publish`. (401/ENEEDAUTH = bad token; 403-2fa = valid token lacking bypass — not a wiring bug, don't re-run.)

**Correct approach = OIDC trusted publishing (no secret).** `release.yml` switched to it 2026-06-06 (`#npm-oidc-trusted-publishing`, commit b1fb815): job has `permissions: id-token: write` + `contents: read`, `actions/setup-node@v4` node 24, `npm install -g npm@latest` (OIDC needs npm CLI ≥ 11.5.1 / Node ≥ 22.14), plain `npm publish --access public`, NO `NODE_AUTH_TOKEN`.

**STATUS 2026-06-06: fully set up — npm releases are automated, no token.** The bootstrap + trusted-publisher config are DONE:
- `opencode-tsift@0.1.63` bootstrapped to npm (manual first publish; npm/cli #8544 — OIDC can't create a brand-new name, no "pending publisher" like PyPI).
- Trusted publisher configured via CLI: `npm trust github opencode-tsift --file release.yml --repo btakita/tsift --allow-publish --yes` (verify with `npm trust list opencode-tsift` → type github / repo btakita/tsift / file release.yml / permissions publish). The npmjs.com web form (package → Settings → Trusted Publisher) is the equivalent.
- `NPM_TOKEN` GH secret DELETED (release.yml uses OIDC). `TSIFT_ENABLE_NPM_PUBLISH=true` var kept (gates the job).

**Result:** every future `v*` tag release publishes opencode-tsift via OIDC with no token and no OTP. Nothing manual remains for npm.

**2FA note for btakita npm account:** it's a browser/passkey factor, NOT TOTP — authenticator apps (Authy/Google/Samsung) show no code / reject the secret ("unsupported format"). Don't chase an OTP app. Any 2FA-gated op (publish bootstrap, `npm trust`) is approved via the browser link npm prints (`npm ... --auth-type=web` or the EOTP URL), run from a real terminal with a browser — not headless (background/`/dev/null` → EOTP, and the URL is token-masked so it can't be relayed).

Docs: https://docs.npmjs.com/trusted-publishers . Related: [[reference_tsift_release_publish_list]].
