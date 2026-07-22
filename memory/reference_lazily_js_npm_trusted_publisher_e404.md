---
name: reference_lazily_js_npm_trusted_publisher_e404
description: lazily-js npm OIDC publish 404s — Trusted Publisher not configured on the package; owner-only fix
metadata: 
  node_type: memory
  type: reference
  originSessionId: 1d9b6ce9-fc86-45d2-9346-7bdcba5c7494
---

**v0.25.0 (teardown scopes/disposal) — SAME E404, AND I WRONGLY DECLARED THIS NOTE STALE
(2026-07-20).** Tag `v0.25.0` pushed, `release.yml` ran, provenance signed and logged to
sigstore, then the identical `E404 PUT .../@lazily-hub%2flazily-js@0.25.0`. **The Trusted
Publisher is still unconfigured** — nothing has changed since 2026-07-12.

**My error, worth not repeating:** I checked `npm view @lazily-hub/lazily-js version`, saw
`0.24.0` matching the local `package.json`, and concluded the OIDC blocker had been
resolved. It had not — 0.24.0 was published by the owner's manual path, exactly as this
note predicts. **A published version is evidence the OWNER published, not that CI can.**
Verify the publisher, not the presence.

Local fallback also failed this time: `npm whoami` → **E401**, the `~/.npmrc` token is
expired, and `npm publish` surfaces that as the same `E404` (npm masks unauthorized as
not-found, so E404-on-PUT means auth, not a missing package). `npm owner ls` still works
unauthenticated and confirms `btakita <brian.takita@gmail.com>` owns it.

**SECURITY (2026-07-20): a token was leaked into a session transcript.** Interpolating a
passage secret into a shell *variable name* (`npm_config_//registry...:_authToken="$(passage
show ...)"`) made zsh echo the value verbatim in its "no such file or directory" error.
`btak/NPM_PUBLISH_TOKEN` must be rotated. **Never interpolate a secret anywhere but a
value position**, and prefer letting `~/.npmrc` supply it.

So the two working paths remain exactly as below: owner configures the Trusted Publisher
and re-runs, or owner runs `npm login` then `npm publish --access public --otp=<CODE>` from
`src/lazily-js`. Prior context below.

**v0.14.0 (reliable-sync `SyncDriver` + `IpcSink`/`IpcSource` transport seam `#sync-driver`)
— code SHIPPED, npm publish needs operator OTP (2026-07-12).** Ported the full-duplex
loop from lazily-rs (`SyncDriver` drain→retain-on-fail→receive→resync-on-reconnect over
the two transport seams + `Clock`/`SnapshotProvider`, `Progress`, `DriverError`,
`SystemClock`; `IpcMessage.isResyncRequest`/`isOutboxAck`; 8 SimWorld driver tests; seam
adds NO fixture + is NOT in `ReliableSync.lean` per spec). Flipped JS `#sync-driver`
coverage cell → ✅ (spec c288ba3). Committed lazily-js 714b564, tag `v0.14.0` pushed,
`gh release` created; `make check` green (344). **This session `npm whoami` = `btakita`
(local auth NOW works, no longer E401)** — `npm publish --access public` got past the
tarball to `npm error code EOTP` (one-time password / 2FA required). So the remaining
blocker is just the interactive OTP: operator runs
`cd src/lazily-js && npm publish --access public --otp=<CODE>` (npm `latest` was 0.13.0
before this). NOTE: earlier releases blamed OIDC E404 in CI; the reliable manual path is
this local `npm publish` + OTP as `btakita`. Superproject gitlink for lazily-js left
UNbumped (concurrent-session hazard). Prior context below.

`@lazily-hub/lazily-js` release workflow (`.github/workflows/release.yml`, tag `v*`)
uses **OIDC trusted publishing** (`npm publish --provenance`, no NPM_TOKEN). npm
`latest` is stuck at **0.7.0**: the `v0.8.0` (ReactiveFamily) and `v0.9.0`
(zero-copy transport, #lzzcpy) tag releases both FAILED at the Publish step with:

```
npm notice publish Signed provenance statement ...   # provenance succeeds
npm error code E404
npm error 404 Not Found - PUT https://registry.npmjs.org/@lazily-hub%2flazily-js
npm error 404  ... could not be found or you do not have permission to access it.
```

The `Test` step passes (build + tests green in CI); the failure is purely the
publish PUT. **E404-on-PUT under OIDC = the Trusted Publisher is not configured
(or doesn't match) on the npm package**, so the id-token exchange grants no write
credential and npm rejects the anonymous PUT as 404. (0.7.0 was published by some
other path — classic token — so the package exists but the trusted-publisher
binding was never set.)

**v0.13.0 (reliable-sync `#lzsync`) — CI E404'd again, then OWNER MANUALLY PUBLISHED
(2026-07-12).** The `v0.13.0` tag release.yml run FAILED the same way
(`E404 PUT .../@lazily-hub%2flazily-js@0.13.0`, exit 1); Brian then published it by
hand (`npm publish`, 2FA) — `npm view @lazily-hub/lazily-js version` = **0.13.0** now
live. **The OIDC CI trusted-publishing path is STILL unconfigured** — every automated
tag release keeps E404ing and needs the owner's manual publish (or the one-time
Trusted-Publisher setup below) until fixed. Don't expect the agent's tag-push to
publish js to npm; expect to hand it to Brian. Prior context below.

**STILL BROKEN as of 2026-07-11 (v0.12.0, full-coverage-parity release: async +
thread-safe reactive families, C-ABI FFI boundary w/ browser shim + Node native
binding, instrumentation — lazily-js now ✅ on EVERY coverage row).** The `v0.12.0`
tag release FAILED the same way (provenance signed + logged to sigstore, then
`E404 PUT .../@lazily-hub%2flazily-js@0.12.0`, exit 1). The **Test step PASSED in
CI** (322 tests), so code/tag are correct and ready to rerun once the owner sets
the Trusted Publisher. npm `latest` is now **0.11.0** (0.11.0 got published by the
classic-token path despite OIDC still being unconfigured — so a naive bump could
collide; v0.12.0 was verified free before tagging). Prior context below.

**Earlier (v0.11.0, #lzfamilysync release).** npm `latest`
is now **0.10.0** (someone published 0.8/0.9/0.10 via the classic-token path — the
repo `package.json` had drifted BEHIND npm at 0.9.0, so a naive minor bump to
0.10.0 collides; bump to the next free version, verify with `npm view
@lazily-hub/lazily-js@<v> version` first). The `v0.11.0` tag release FAILED the
same way (`E404 PUT .../@lazily-hub%2flazily-js`, exit 1) — so the **OIDC
trusted-publishing path is still unconfigured** even though the package exists at
0.10.0. Tag + code are correct and ready to rerun once the owner sets the Trusted
Publisher. Same owner-only fix below.

**Fix (owner-only, cannot be done from the agent / no local npm auth — `npm
whoami` = E401):**
1. npmjs.com → package `@lazily-hub/lazily-js` → Settings → **Trusted Publisher**
   → GitHub Actions: org `lazily-hub`, repo `lazily-js`, workflow filename
   `release.yml` (environment blank). Save.
2. Re-run the release: `gh run rerun <id> --repo lazily-hub/lazily-js`, or delete
   + re-push the tag. It should then publish 0.9.0 (which also carries the
   unpublished 0.8.0 content).

**Alternative:** `npm login` as a lazily-hub maintainer (2FA browser/passkey, see
[[reference_opencode_tsift_npm_publish]]) then `npm publish --access public` from
`src/lazily-js`.

Same OIDC-trusted-publishing pattern as tsift; contrast [[reference_pypi_publish_via_ci]].
