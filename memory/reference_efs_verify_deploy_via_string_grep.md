---
name: reference_efs_verify_deploy_via_string_grep
description: Verify an EFS Cloudflare Worker deploy is live (incl. your own change) without admin auth by grepping fetched worker + browser bundles for unique string literals
metadata: 
  node_type: memory
  type: reference
  originSessionId: 4a114695-0a1e-41f7-b4e9-f9bece164864
---

To confirm an EFS change is actually LIVE in prod (esp. when a concurrent session owns the deploy), you usually can't hit admin routes — the admin auth middleware returns 401 for ANY `/api/admin/*` path *before* route matching, so "route exists" is indistinguishable from 404 via HTTP status.

Instead verify by **grepping deployed bundles for a unique string literal from your change** (identifiers are minified away, but string literals survive):

1. **Worker (server) bundle** — fetch the deployed script from Cloudflare and grep:
   ```
   curl -s --config "$HDR" \
     "https://api.cloudflare.com/client/v4/accounts/1e7e194ff8da4ad9e40ce041aea84f76/workers/scripts/equityfundingsource" -o /tmp/prod_worker.js
   grep -c "<unique server string, e.g. an email subject>" /tmp/prod_worker.js
   ```
   (`$HDR` = the passage bearer file from [[reference_efs_d1_token_working]].)
2. **Browser bundle** — read the hashed asset name from live HTML, fetch it, grep:
   ```
   hash=$(curl -s "$BASE/admin/login" | grep -oE "index\.browser-[A-Za-z0-9_]+\.js" | head -1)
   curl -s "$BASE/$hash" | grep -c "<unique UI string, e.g. a button label>"
   ```
   `$BASE=https://equityfundingsource.brian-takita.workers.dev`.

Corroborate with the deployment timeline: `GET …/workers/scripts/equityfundingsource/deployments` → `.result.deployments[].created_on`; a deploy timestamp AFTER your commit time (`git show -s --format=%ci <sha>`, EDT→UTC +4) means the live build was cut from a tree containing your commit. Content-hashed asset names matching a clean local build of the same commit is the strongest proof (identical hash ⇒ identical source).

Frontend strings live ONLY in the browser bundle, not the worker JS — grep the right bundle. Relates to [[feedback_concurrent_sessions_force_push_shared_superproject]] (on a shared EFS worktree the concurrent session's auto-deploy loop often ships your pushed commit within minutes; verify rather than re-deploy a stale build, which would regress their newer commits).

**Sandboxed sessions (no prod DNS):** some sessions can't resolve `equityfundingsource.com` / `*.workers.dev` at all (`curl: (6) Could not resolve host`), so you cannot fetch the served bundle — but `wrangler` still reaches the Cloudflare API. Verify a deploy you just ran without HTTP by: (1) `grep -c "<unique string from your change>" app/equityfundingsource/dist/worker/index.js` — that local bundle is byte-identical to what `deploy:worker` uploaded; and (2) `bunx wrangler deployments list` → confirm a NEW `Version(s): (100%) <id>` whose `Created:` is after your commit. `make deploy` (not bare `bun run deploy:worker`) so `.env` secrets are sourced; `deploy:worker` also runs `cf:d1:migrate:remote` against prod D1, so a no-new-migration change is a safe no-op there.
