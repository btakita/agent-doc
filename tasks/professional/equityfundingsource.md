---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
links:
- ./judith-krug.md
- ./robert-waldman.md
- ../personal/robert-ward.md
- ./bob-remeika.md
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---

## Status

<!-- agent:status patch=replace -->
25 pages deployed with form backend (D1 + R2 + email). Document pipeline architecture: Tier 0-3 (WAF -> Workers AI -> Claude -> Ragie).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-142913.md`*

Compacted content:
- Archived 6 response topic(s): TMO simulator API; TMO simulator persistence; Closeout correction; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ What are #next-steps to fully implement the functionality of the investor portal. Note I do not yet have access to Dropbox. The upload can go into a private R2 temporarily...R2 can also be used as a staging area in case of Dropbox issues.
### Re: investor portal next steps — gpt-5

Short version: do not wait on Dropbox. Use a separate private R2 bucket as the active document backend now, and design the database around provider-neutral document metadata so Dropbox can become a later export/sync target instead of the thing the portal depends on.

What is already real: investor auth/role routing exists; available deals load from published admin deals; deal detail loads live deal data; funding commitments persist to `funds`; admin acceptance calls TMO `AddLoanFunding`; positions/dashboard/distributions are derived from accepted fund records and TMO/payment-schedule data. The main unfinished investor-portal surface is document/account data: Documents, Statements, and Accreditation still use static fixtures, and existing borrower loan-document storage is named and wired around Dropbox.

I would implement the remaining work in this order:

1. Add private R2 document storage. Do not reuse `R2_ASSETS`; that bucket is for site/blog assets. Add a separate `R2_DOCUMENTS` binding and a small storage adapter with `put`, `get`, `delete`, and authenticated download/stream behavior. Store keys under predictable private prefixes like `loans/{loan_id}/...`, `deals/{deal_id}/...`, and `investors/{user_id}/...`.

2. Make document metadata provider-neutral. Either migrate `loan_documents` or add a new canonical document table so records can store `storage_provider`, `storage_key`, `content_type`, `file_size`, `sha256`, `owner_user_id`, `loan_id`, `deal_id`, `category`, `visibility`, `review_status`, and optional `external_provider/external_id/external_path`. Existing `dropbox_id` and `dropbox_path` can remain compatibility fields, but new code should not require them.

3. Replace Dropbox-only upload paths with the storage adapter. Borrower uploads and any investor/accreditation uploads should write to R2 first. If Dropbox later becomes available, add a background/manual sync that copies selected R2 objects to Dropbox and records the Dropbox id/path, without changing the portal-facing APIs.

4. Finish investor document APIs. Add authenticated endpoints for investor document lists and downloads, plus statement/accreditation records. Investors should only see documents explicitly visible to them or attached to published deals they can access. Deal due-diligence documents can reuse the admin loan-document source once visibility/category rules exist.

5. Wire the investor UI off live data. Replace `INVESTOR_DOCUMENTS` usage in Documents and Statements, and replace the hard-coded Accreditation status with user/document-backed state. The UI should support: request document, upload/update accreditation evidence, download/view statements and tax docs, and view deal due-diligence packages.

6. Add admin publishing and review controls. Admins need to upload or attach documents, mark them investor-visible, assign category/type, review accreditation files, publish statements/tax docs to a specific investor, and see a clear audit trail of who published or approved what.

7. Harden the file boundary before real users. Enforce Cloudflare/WAF upload size limits, MIME/extension checks, private-bucket-only access, role checks on every download, audit logging, and clear retention/delete behavior. The Tier 0-3 document intelligence pipeline can consume the same R2 object keys later.

8. Verify the full investor lifecycle. Seed a borrower/admin/investor path with the TMO simulator: publish a deal, attach R2 due-diligence docs, view them as investor, submit lender-account data, commit funds, accept the fund as admin, confirm positions/dashboard/distributions update, then publish and download a statement.

The key architecture choice is to make R2 the current source of truth for private uploads and treat Dropbox as an optional downstream integration. That avoids blocking on Dropbox credentials and also gives you a staging layer if Dropbox fails later.
❯ We do not yet have agent integration to perform OCR. Please carve out #next-steps for OCR integration.
❯ We may also need a proper DB. Can the sqlite + graphdb approach work? Is tsift useful?
❯ Should we use convex, neo4j, or another graphdb? Consider costs, effectiveness, and maintenance.
### Re: OCR integration and database choice — gpt-5

I would not make an interactive agent the OCR engine. OCR should be an app-native async pipeline: upload to private R2, enqueue processing, persist structured OCR/extraction results, send uncertain fields to admin review, and only write to TMO after staff approval. Agents can help inspect/review outputs later, but the portal should not depend on an agent session being present to process documents.

Concrete OCR next steps:

1. Add a document-processing schema around the private R2 document store: `documents`, `document_versions`, `ocr_runs`, `document_pages`, `document_chunks`, `extracted_fields`, `review_items`, `entity_mentions`, and optional `entity_edges`. Keep the original R2 key, content hash, content type, provider, run status, retry count, cost, and source page/bounding-box evidence.

2. Add an async state machine: `uploaded -> screened -> queued -> parsed -> extracted -> review_required -> approved/rejected -> indexed -> tmo_writeback_done`. This matches the PRD requirement that uploaded docs are screened, classified, extracted, reviewed, and only then written to TMO.

3. Build a parser adapter, not a single hard-coded provider. Interface: `parseDocument({ r2Key, contentType, documentType }) -> pages/chunks/elements`. Start with a manual/admin-triggered job before automating every upload.

4. Use Ragie Parse first for hard document OCR if budget allows, because it handles scanned PDFs, tables, forms, key-value pairs, signatures, handwritten notes, and bounding boxes. Keep the output in our D1/R2 schema so Ragie is replaceable.

5. Use cheaper Cloudflare-native steps for early screening: file type/size/MIME, basic classification, embeddings/search, and maybe markdown conversion where it is good enough. Do not send every document to expensive OCR/Claude by default.

6. Add a Claude extraction layer only after parse/OCR has produced page text/elements. Claude should produce typed JSON field candidates with citations back to page/box/chunk ids, not opaque summaries.

7. Build the admin review UI before TMO writeback. Required fields: extracted value, normalized value, confidence, source page/snippet, reviewer decision, reviewer note, and target TMO field. Approval should be explicit per field or grouped review packet.

8. Add cost and safety guardrails: per-document page count cap, per-account daily processing cap, provider timeout/retry limits, PII-aware logs, audit trail, and a manual reprocess button.

Database recommendation: keep D1/SQLite plus R2 as the canonical system for now. It is already deployed in this app, and it is the right shape for portal auth, users, loans, documents, funds, review queues, and audit history. For graph-like behavior, start with normal SQL tables: `entities`, `entity_mentions`, and `entity_edges`. That gives us enough to ask, "which borrowers/properties/docs/deals are related?" without running a second production database.

A full graph database should be a derived index later, not the source of truth. The trigger to add one is when SQL edge tables become painful for real workloads: repeated multi-hop relationship questions, graph visualization/analytics, fraud/relationship analysis, or knowledge-graph queries that are central to Judith's document dashboard. Until then, a separate graph DB mostly adds dual writes, sync bugs, auth duplication, backups, and operational cost.

Provider read:

- D1/R2 is the lowest-maintenance path. Current D1 pricing is usage/row based with no active-instance charge, and the paid plan includes large monthly read/write allowances before overage. R2 has 10 GB-month free, low per-GB storage, request-based Class A/B charges, and no egress fee. Sources: [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/), [R2 pricing](https://developers.cloudflare.com/r2/pricing/).
- Cloudflare Vectorize is a good first semantic-search/index layer if we want to stay Cloudflare-native; official examples price even a 50k-vector/200k-query production-ish workload at a few dollars/month before included usage. Source: [Vectorize pricing](https://developers.cloudflare.com/vectorize/platform/pricing/).
- Ragie is useful for OCR/parse/index if we want quality faster than building the pipeline ourselves. Current public pricing shows Starter at $100/month, Pro at $500/month, included page processing, and per-page overage for fast/hi-res processing. Source: [Ragie pricing](https://www.ragie.ai/pricing), [Ragie Parse](https://www.ragie.ai/parse).
- Convex is not a graph database. It is a realtime/backend platform. It could be good for a greenfield app, but adopting it here would mean migrating away from the existing Cloudflare Worker/D1 architecture. Current Professional pricing starts at $25/developer/month, with enterprise minimums much higher. Source: [Convex pricing](https://www.convex.dev/pricing).
- Neo4j is the serious graph option, but not the first move for EFS. AuraDB Professional currently starts at a 1 GB cluster around $65/month and scales from there. Use it only if graph queries become a primary product feature. Source: [Neo4j pricing](https://neo4j.com/pricing/).
- `tsift` is useful for us as a development and agent-navigation tool, not as the production EFS knowledge store. It can help inspect repo/session/docs and maybe prototype extraction ideas, but it is not tenant-authenticated, admin-reviewable, or designed as the client-facing document index.

My recommended architecture is: D1 for canonical records and review state, R2 for private originals and derived artifacts, Ragie Parse or a pluggable OCR provider for hard parsing, D1 tables for extracted fields/entities/edges, and Ragie/Vectorize as the derived retrieval layer. Defer Neo4j/FalkorDB/Convex until a measured search/relationship workload proves that SQL plus a vector/retrieval layer is insufficient.
❯ How do I access the admin section?
### Re: admin section access — gpt-5

Use the admin-specific login page, not the normal portal login:

- Live/custom domain: `https://www.equityfundingsource.com/admin/login`
- Local Worker dev: `http://127.0.0.1:8787/admin/login`

After a successful admin login, the app redirects to `/portal-admin`, which then redirects to `/portal-admin/loans`. You can also go directly to `/portal-admin/loans`, but unauthenticated users are sent back to `/login?role=admin` by the guard, so `/admin/login` is the cleaner entry point.

If no admin account exists yet, use `/admin/signup`. That signup requires the Worker secret `ADMIN_GLOBAL_PASSWORD`; it is intentionally not stored in the repo. Existing admin users can sign in with either their personal admin password or the global admin password. A non-admin account cannot access the admin portal; it will be rejected or redirected to its own role portal.

Verification: checked `lib/ui--server--efs/urls/index.ts`, `app/equityfundingsource/lib/auth/routes.ts`, and `app/equityfundingsource/lib/portal/admin/routes.ts`.

❯ http://127.0.0.1:8787/admin is a 404
❯ http://127.0.0.1:8787/admin/login redirects to the home page.

❯ Is the http://127.0.0.1:8787/admin/signup gated? I don't want anybody on the internet to be able to have an admin account.
### Re: admin access and signup gate — gpt-5

You were right to question this. Before this change, `/admin` had no route, `/admin/login` redirected a browser with an existing non-admin session back to home, and `/admin/signup` exposed the signup form publicly. The actual admin-account creation POST was gated by `ADMIN_GLOBAL_PASSWORD`, but I agree that a public self-signup form is not the right default.

I tightened the behavior in `src/equityfundingsource-dev`:

- `/admin` and `/admin/` now redirect to `/admin/login`.
- `/admin/login` now shows the admin login form even if the browser currently has a borrower/investor/broker session; it clears that non-admin session instead of redirecting home. Existing admin sessions still go to `/portal-admin`.
- `/admin/signup` now returns 404 unless `ADMIN_SIGNUP_ENABLED` is explicitly true.
- Even when signup is enabled for bootstrap, creating the admin account still requires `ADMIN_GLOBAL_PASSWORD`.
- Production defaults `ADMIN_SIGNUP_ENABLED = "false"` in `wrangler.toml`; local `.dev.vars.example` enables it only for bootstrap/local setup.
- The admin login page no longer advertises the signup link unless signup is enabled.
- Existing admins can create users through `/portal-admin/users`, so public self-signup does not need to stay open after bootstrap.

For your local Worker, restart `bun run dev:worker` before retesting `http://127.0.0.1:8787/admin`. If you are already signed in as a non-admin, the updated `/admin/login` will clear that cookie and render the admin login form.

Changed paths:

- `.dev.vars.example`
- `wrangler.toml`
- `app/equityfundingsource/lib/auth/routes.ts`
- `app/equityfundingsource/lib/auth/session.ts`
- `lib/ui--server--efs/auth/admin_login.ts`
- `app/equityfundingsource/bin/cf-secrets-push.ts`
- `docs/deploy.md`
- `tests/auth.test.ts`

Verification:

- `bun test tests/auth.test.ts` passed.
- `make check` passed: 49 tests, production build, Worker build.
- `git diff --check` passed.
- CI/tmux note: this repo currently has no GitHub Actions workflows visible to `gh workflow list`; `gh run list --workflow CI --limit 1` reported that no workflow named `CI` exists, so there was no remote CI/tmux leg to inspect.
❯ How do I create an admin account?
### Re: create an admin account — gpt-5

Use `/admin/signup` only as a temporary bootstrap path. In the tightened code, that route is intentionally closed unless `ADMIN_SIGNUP_ENABLED` is explicitly true.

For local dev:

1. In `src/equityfundingsource-dev/.dev.vars`, make sure these exist:

```env
AUTH_SECRET=<local random secret>
ADMIN_GLOBAL_PASSWORD=<private bootstrap password>
ADMIN_SIGNUP_ENABLED=true
```

2. If the local D1 database is fresh, apply migrations:

```bash
bun run cf:d1:migrate:local
```

3. Start or restart the Worker:

```bash
bun run dev:worker
```

4. Open `http://127.0.0.1:8787/admin/signup`.

5. Enter the new admin's name, email, personal password, and the `ADMIN_GLOBAL_PASSWORD` value from `.dev.vars`. After submit, it should sign in and redirect to `/portal-admin`.

6. After the admin account exists, set `ADMIN_SIGNUP_ENABLED=false` and restart the Worker. From then on, create admin or staff users from `/portal-admin/users` while signed in as an admin.

For production, do the same idea but keep the window short: deploy with `ADMIN_SIGNUP_ENABLED = "true"`, make sure `ADMIN_GLOBAL_PASSWORD` is set as a Cloudflare Worker secret, create the bootstrap admin at `https://www.equityfundingsource.com/admin/signup`, then immediately set `ADMIN_SIGNUP_ENABLED = "false"` and redeploy. Do not leave public admin signup enabled.

Changed paths this cycle: none; this was an answer-only turn. Commands run: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`. Verification: not applicable because no code changed in this cycle.
❯ /admin/login is getting a Database not found error. Should the dev db be the local sqlite db?
### Re: local dev database for admin login — gpt-5

Yes. In local Worker dev, the app should use Wrangler/Miniflare's local D1 database, which is SQLite-backed and stored under `src/equityfundingsource-dev/.wrangler/state/v3/d1/`. It should not use the remote Cloudflare D1 database unless you explicitly pass `--remote`.

I applied the local migrations now:

```bash
cd /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev
bun run cf:d1:migrate:local
```

That succeeded and applied the pending local migrations. I also verified the local D1 has the auth table:

```bash
cd /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/app/equityfundingsource
bun run cf -- d1 execute equityfundingsource --local -c ../../wrangler.toml --command "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'users'"
```

The query returned `users`, so the local auth schema exists now.

Next step: stop and restart the local Worker:

```bash
cd /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev
bun run dev:worker
```

Then retry `http://127.0.0.1:8787/admin/login`. If you still see "Database not found" after restart, the usual causes are: the server is not actually `bun run dev:worker`, another process is bound to port `8787`, or Wrangler is being launched from a different config/state path. The lightweight `bun dev` server on port `4200` is not enough for real auth because it does not provide the D1 binding.

Changed paths this cycle: none in source; only the ignored local Wrangler D1 state was updated. Verification: local migrations ran successfully and a local D1 query confirmed the `users` table exists.
❯ I have `ADMIN_SIGNUP_ENABLED=true` in .dev.vars but don't see a register link.
### Re: admin signup link visibility — gpt-5

Your running local Worker is now reading `ADMIN_SIGNUP_ENABLED=true` correctly. I checked both routes directly:

- `http://127.0.0.1:8787/admin/signup` returns `200 OK` and renders `Create admin account`.
- `http://127.0.0.1:8787/admin/login` includes the lower-page section:
  - `New administrator?`
  - link target `/admin/signup`
  - text `Register admin account`

So you can go directly here now:

```text
http://127.0.0.1:8787/admin/signup
```

If you still do not see the register link in the browser, the likely issue is browser state, not the Worker env: hard refresh `/admin/login`, open it in a private window, or go straight to `/admin/signup`. The link sits below the admin password form, so it may also just be below the fold depending on the viewport.

Changed paths this cycle: none. Commands run: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc plan`, `curl` checks against `/admin/login` and `/admin/signup`, and `ps -ef` to confirm `bun ./bin/dev-worker.ts` / `wrangler dev` are running. Verification: local HTTP checks confirmed signup is enabled and the register link is present in the served HTML.
❯ I see the /admin/login page. I don't see a way to register. Event with a hard refresh and in Incognito mode.
### Re: admin register button — gpt-5

You were not missing an env setting. The Worker was serving the signup route and the login HTML did contain the registration link, but the link was too easy to miss because it was a small secondary text link below the login form.

I changed the admin login page so that when `ADMIN_SIGNUP_ENABLED=true`, `/admin/login` now shows a full-width `Create admin account` button directly under `Sign In to Admin`. The gate is still intact: when `ADMIN_SIGNUP_ENABLED` is false, that button is absent and `/admin/signup` still returns 404. Account creation still requires the `ADMIN_GLOBAL_PASSWORD`.

On the running local Worker, I verified:

- `http://127.0.0.1:8787/admin/login` now contains `href="/admin/signup"` and the visible text `Create admin account`.
- `http://127.0.0.1:8787/admin/signup` returns `200 OK` while your local bootstrap flag is enabled.

Changed paths this cycle:

- `src/equityfundingsource-dev/lib/ui--server--efs/auth/admin_login.ts`
- `src/equityfundingsource-dev/tests/auth.test.ts`

Commands run: `tsift status`, `tsift status --fix`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `ps -ef`, `curl` checks against `/admin/login` and `/admin/signup`, `rg`, `git status --short`, `bun test tests/auth.test.ts`, `make check`, `git diff --check`, `gh workflow list`, and `gh run list --workflow CI --limit 1`.

Verification: focused auth tests passed, `make check` passed with 50 tests plus site and Worker builds, `git diff --check` passed, and local HTTP smoke checks confirmed the visible admin signup button. GitHub Actions note: this repo still has no workflow named `CI`, so there was no remote CI/tmux leg available to inspect.
<!-- agent:boundary:6702793c -->
<!-- /agent:exchange -->
###

## Pending / Not Built

<!-- agent:pending -->
- [ ] [#7fx4] OCR ingestion schema/state machine: add provider-neutral document_processing tables for documents, versions, OCR runs, pages/chunks, extracted fields, entity mentions/edges, review decisions, provider cost, and retry/error state.
- [ ] [#d1h2] OCR provider adapter: process private R2 documents through pluggable parsers starting with Ragie Parse or Workers AI/Claude where appropriate; keep original R2 object as source of truth and persist source page/bounding-box evidence.
- [ ] [#f5kq] OCR admin review workflow: show extracted fields with source evidence, confidence, and diffable approval/rejection before any TMO borrower, loan, lender, or loan-application writeback.
- [ ] [#ntcr] Search/index spike: keep D1/R2 canonical and evaluate Ragie index plus Cloudflare Vectorize/SQL edge tables before adopting Neo4j, FalkorDB, Convex, or another separate graph database.
- [ ] [#ck54] Investor portal private document storage: add a separate private R2_DOCUMENTS bucket plus provider-neutral document metadata and authenticated download APIs; keep Dropbox as a later sync adapter.
- [ ] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [ ] [#zjbe] Admin investor document workflow: upload and publish deal due-diligence, investor statements, tax docs, and accreditation files from R2 with visibility and review status controls.
- [ ] [#9vv3] Investor portal end-to-end QA: seed and test investor signup, accreditation, published deal documents, funding commitment, admin acceptance, positions, distributions, and statements with TMO simulator plus R2 documents.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
- [ ] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#kxsx] Form submission pipeline
- [ ] [#njsx] Tier 0: Cloudflare WAF + upload limits
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#yh4b] Admin dashboard (Cloudflare Access auth)
- [ ] [#ft47] Token-based document upload flow
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
<!-- /agent:pending -->

## Review

<!-- agent:review -->
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
<!-- /agent:review -->
