---
name: reference_efs_local_portal_drive_recipe
description: "How to drive the EFS lender/portal locally to visually verify a change (seed, mint session, inject cookie)"
metadata: 
  node_type: memory
  type: reference
  originSessionId: b3dccfb6-6015-4191-a360-0c6e98472fdd
---

Recipe to drive the equityfundingsource portal on a local worker and screenshot a real render (used to verify the "Spoken For" / Funded Deals feature).

1. **Local D1 is often stale.** A 500 `D1_ERROR: no such column: fa.archived_at` on `/api/investor/deals` (and other endpoints) means the local miniflare D1 is behind on migrations. Fix: `cd app/equityfundingsource && bun run cf:d1:migrate:local` (applies pending, e.g. 0102→0119). Migrations are additive; seeded rows survive.
2. **Seed** via sqlite3 against `.wrangler/state/v3/d1/miniflare-D1DatabaseObject/<hash>.sqlite`: insert a `loans` row (needs id, user_id, tmo_loan_application_rec_id, tmo_loan_number, borrower_full_name, borrower_email, created_at, updated_at; other NOT NULLs have defaults) then an `investment_deals` row (status in published/reserved/funded/closed). A `reserved` deal renders "Spoken For" for a non-involved lender; `funded`/`closed` go to the Funded Deals page.
3. **Run:** `bunx wrangler dev -c ../../wrangler.toml --var EMAIL_DELIVERY_MODE:local --var LOCAL_EMAIL_CAPTURE_ENABLED:true --port 8787` (background). Rebuild first with `bun run build:worker` — wrangler serves the prebuilt `dist/worker/index.js`, it does NOT recompile TS on change.
4. **Auth is a signed-cookie JWT** (`efs_session`, HS256 over `env.AUTH_SECRET`), no sessions table. Mint one: bun-import `session__sign` from `app/equityfundingsource/lib/auth/session.ts`, read `AUTH_SECRET` from repo-root `.dev.vars`, sign `{ sub: <user id>, email, name, role: 'lender' }`. Keep the token out of logs (write to a scratch file).
5. **Cookie is HttpOnly** → can't set via `document.cookie`. Use chromium-bridge's Chrome (CDP :9222): open an isolated tab via `PUT /json/new`, `Network.setCookie {name:'efs_session', value, url:'http://127.0.0.1:8787', httpOnly:true}`, `Page.navigate`, `Page.captureScreenshot`, then `/json/close/<id>`. Don't hijack the user's existing tabs (Gmail/prod admin are usually open).

See [[reference_efs_deploy_from_main_checkout_not_fresh_install_worktree]] for the deploy side.
