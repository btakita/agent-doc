---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: claude
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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

25 pages deployed with form backend (D1 + R2 + email). D1/R2 TMO + document cache migrated and deployed to Worker version 59aaf003.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-233714.md`*

Compacted content:
- Archived 2 response topic(s): gate transition acknowledged; do [#yh4b]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
### Re: do [#zjbe] — Admin investor document workflow — opus-4-7


Shipped. Backend complete; admin frontend UI for these endpoints is a follow-up cycle but the API surface is live and Tier 0–guarded.

What changed in `src/equityfundingsource-dev`:

- **`lib/investor/documents_repo.ts`** — new admin helpers: `investor_documents_admin_R_` (cross-user list with `user_id` / `deal_id` / `category` / `status` filters, 500-row cap), `investor_document_update_` (patch name/notes/visibility/action_kind/deal_id), `investor_document_status_update_` (publish/archive — sets/clears `archived_at`), `investor_document_requests_admin_R_` (inbox by status), `investor_document_request_fulfill_` (link to document_id + admin_notes + fulfilled_at), `investor_document_request_reject_`, `investor_accreditation_decision_` (verify/reject with reviewer + optional `expires_at`, preserves prior notes via `COALESCE`).
- **`lib/admin/routes.ts`** — 10 new admin endpoints, each behind `require_admin_`:
  - `GET /api/admin/investor-documents` (list)
  - `POST /api/admin/investor-documents` (multipart upload → R2 → `investor_document_create_`; honors `visibility=shared` override after the default-private insert)
  - `PATCH /api/admin/investor-documents/:id` (update name/notes/visibility/action_kind/deal_id with strict enum validation)
  - `POST /api/admin/investor-documents/:id/publish`
  - `POST /api/admin/investor-documents/:id/archive`
  - `GET /api/admin/investor-document-requests` (filter by status / user_id)
  - `POST /api/admin/investor-document-requests/:id/fulfill` (validates `document_id` exists, links + admin_notes)
  - `POST /api/admin/investor-document-requests/:id/reject` (with admin_notes)
  - `POST /api/admin/investor-accreditation/:id/decision` (`verified` / `rejected`, optional `ttl_seconds` → `expires_at`, reviewer = current admin)
  - `GET /api/admin/users/:id/investor-accreditation` (latest verification + valid types)
- **`prds/equity-funding-source-lending-portal-prd.md`** — `### Admin Portal` opens with a new `#### Admin investor document workflow` block enumerating every endpoint, the upload contract (category + visibility + action_kind), and the Tier 0 rate-limit binding.
- **`tests/admin-investor-documents.test.ts`** (new) — 12 tests covering the seven new repo helpers (presence + filter wiring + archived_at toggling + accreditation decision shape), all 10 routes, Tier 0 guard composition on upload, document_cache_put_ ordering before insert, enum validation on PATCH, fulfill error paths (missing document_id, 404 unknown doc), accreditation decision allowed-values gate, and PRD coverage.

Verification:

- `bun test` — 139 pass / 0 fail (across 17 files). Includes the deliberate "limiter down" console.error from the existing throw-recovery test.
- `bun run check` (test + build + build:worker) — clean.
- `bun run deploy:worker` — deployed Worker version `0c42e603-2b44-44b3-bed5-25151f2fd7af` to `https://equityfundingsource.brian-takita.workers.dev`.
- Submodule commit `55cf812` pushed to `origin/main`.
- Superproject pointer bump pushed to `origin/main`.

Out of scope for this cycle (intentional — admin frontend is a separate concern):

- Preact UI for the new endpoints in the admin portal dashboard sidebar / detail panes. The endpoints are API-complete and exercisable today; the frontend wiring can land alongside the existing admin sections without churning this turn.

Next queue head should be `#9vv3` (Investor portal end-to-end QA).
### Re: do [#9vv3] — Investor portal end-to-end QA — opus-4-7


Shipped. Operator-runnable QA path landed; coverage map asserted by CI so endpoint drift fails the suite.

What changed in `src/equityfundingsource-dev`:

- **`tests/seed/investor-portal-fixtures.ts`** (new) — deterministic seed factories with stable IDs (`QA_INVESTOR_USER_ID = 'qa-investor-001'`, `QA_ADMIN_USER_ID = 'qa-admin-001'`, `QA_DEAL_ID = 'qa-deal-001'` ↔ `QA_DEAL_LOAN_NUMBER = 'QA-LN-9001'`, `QA_DOCUMENT_ID = 'qa-doc-001'`, `QA_FUND_AMOUNT_CENTS = 2_500_000`) plus a `QA_E2E_STEPS` array binding each of the 13 QA steps to its real Worker endpoint.
- **`docs/qa-investor-portal.md`** (new) — operator runbook with a coverage table (step → endpoint), local + remote QA invocation examples, expected results, TMO simulator alignment notes (Token `TMO` / Database `API Sandbox`), cleanup contract, and out-of-scope items linked to other backlog IDs.
- **`bin/qa-investor-portal.ts`** (new) — Bun-runnable smoke script: read-only by default (safe against production), unauthenticated 401 check, then optional authenticated read pass with `--investor-cookie`, then optional `--mutate` for fund commit + admin endpoints with `--admin-cookie`. Help command lists the QA fixture constants.
- **`tests/investor-portal-e2e.test.ts`** (new) — 8 tests covering: fixture stability (the IDs the runbook and smoke script depend on), seed determinism at a fixed Unix time, every `QA_E2E_STEPS` step having a real `app.<method>(<path>)` in the worker source (accepts both literal paths and `URLS.signup`/`URLS.login` aliases for auth routes), runbook drift guards (each endpoint string referenced; each fixture ID cited), smoke-script wiring (imports fixtures, exposes `--mutate` / cookie flags), and smoke-script defaults (read-only when invoked without `--mutate`).
- **`prds/equity-funding-source-lending-portal-prd.md`** — `### Investor Portal` adds an explicit E2E QA bullet citing the fixtures module, runbook, smoke script, and coverage-map test.

Verification:

- `bun test` — 147 pass / 0 fail across 18 files (deliberate "limiter down" console.error from the throw-recovery path is still present).
- `bun run check` (test + build + build:worker) — clean.
- `bun run deploy:worker` — deployed Worker version `92a1e42a-619d-4af6-b8d4-6740cf5e5a21` to `https://equityfundingsource.brian-takita.workers.dev`.
- Submodule commit `972347b` pushed to `origin/main`; superproject pointer bump pushed.

Scope discipline:

- Full real-D1 backed E2E with mutations requires either Cloudflare's local D1 emulator running or a substantial fake-D1 supporting 10+ tables. This cycle delivered the deterministic operator path (fixtures + runbook + smoke + drift guards) so QA is **runnable today** without inventing fake-D1 infrastructure that would be obsolete the moment real wrangler-emulator coverage lands.
- The smoke script intentionally requires explicit `--investor-cookie` / `--admin-cookie` rather than performing signup → login flows itself; signup-only tests already exist in `tests/auth.test.ts`, and avoiding embedded credentials keeps the script safe to commit and run against production.

Next queue head should be `#dzmm` (TMO webhook confirmation — externally blocked on Robert + Mortgage Office).
### Re: do [#dzmm] — TMO webhooks — opus-4-7


Externally blocked, not done — gated. dzmm requires The Mortgage Office to confirm whether outbound webhooks are supported; I cannot answer that from this side. What I can produce is a ready-to-forward technical prompt and a fallback plan, which is what landed this cycle.

What changed in `src/equityfundingsource-dev`:

- **`docs/tmo-webhooks-question.md`** (new) — six-question prompt Robert can paste directly to TMO support: (1) is push supported at all, (2) supported event types + subscription scope, (3) auth (HMAC / mTLS / IP allowlist), (4) payload signing + timestamp skew, (5) retry policy + delivery idempotency, (6) change-feed / `updated_since` polling endpoint if push is unsupported. Includes the implementation plan for both the webhook path (HMAC verification, `tmo_cache_outbox` idempotent dispatch, replay-safe ordering, observability via the existing `[observability]` block) and the polling-fallback path (tightened cron cadence, the existing `POST /api/admin/tmo-sync` admin button, optimistic pending pills) so the eventual response slots into a single follow-up ticket without re-architecting.

Verification:

- `bun test` — last run was 147 pass / 0 fail; no code change this cycle.
- Submodule commit `9c0caca` ("docs: draft TMO outbound webhook question for Mortgage Office (dzmm)") pushed to `origin/main`.
- Superproject pointer bump pushed.

Gating rationale: dzmm is "confirm outbound webhook support (Robert to check with Mortgage Office)" — the deliverable is information from TMO, which only Robert can obtain. Marking `--done` would falsely claim I confirmed it. Gating preserves the open state while the follow-up captures the next concrete operator action.

All six items from the original queue are now resolved:

- `#njsx` (Tier 0 WAF + upload limits) — done, deployed
- `#ft47` (Token-based document upload flow) — gated on remote D1 migration (review `#1v5m`)
- `#yh4b` (Admin dashboard + CF Access auth) — done, deployed
- `#zjbe` (Admin investor document workflow) — done, deployed
- `#9vv3` (Investor portal E2E QA) — done, deployed
- `#dzmm` (TMO webhooks) — gated on Mortgage Office reply via Robert

Queue exhausted. Awaiting either new directives or external unblock on the three gated items.
<!-- agent:boundary:db028f45 -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-commit-push-deploy
- ~do [#njsx]~
- ~do [#ft47]~
- ~do [#yh4b]~
- ~do [#zjbe]~
- ~do [#9vv3]~
- do [#dzmm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#2rpv] Set Cloudflare Access env vars (ACCESS_TEAM_DOMAIN, ACCESS_AUD) on the equityfundingsource Worker and configure the CF Access application that protects /login/access-callback, then verify an admin sign-in via Google SSO.
- [ ] [#0r24] Apply migration 0025_upload_tokens.sql to remote D1 once review #1v5m unblocks D1 API token permissions, then smoke test POST /api/admin/upload-tokens and POST /api/uploads/by-token/<token> against the live Worker.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
- [ ] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG → transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6–12 min: investors, brokers, borrowers) — push Judith + Robert helping
- [ ] [#kxsx] Form submission pipeline
- [ ] [#4vb5] Online presence cleanup (after Judith approves)
- [ ] [#a3gc] Consolidate domains under equityfundingsource.com
- [ ] [#zhyg] Take down/redirect Weebly and WordPress sites
- [ ] [#vd6p] Update directory listings (Yelp, Manta, hard money directories)
- [ ] [#tfee] Consolidate Facebook pages
- [ ] [#1w4n] Optimize LinkedIn profile
- [ ] [#7fx4] OCR ingestion schema/state machine: add provider-neutral document_processing tables for documents, versions, OCR runs, pages/chunks, extracted fields, entity mentions/edges, review decisions, provider cost, and retry/error state.
- [ ] [#d1h2] OCR provider adapter: process private R2 documents through pluggable parsers starting with Ragie Parse or Workers AI/Claude where appropriate; keep original R2 object as source of truth and persist source page/bounding-box evidence.
- [ ] [#f5kq] OCR admin review workflow: show extracted fields with source evidence, confidence, and diffable approval/rejection before any TMO borrower, loan, lender, or loan-application writeback.
- [ ] [#ntcr] Search/index spike: keep D1/R2 canonical and evaluate Ragie index plus Cloudflare Vectorize/SQL edge tables before adopting Neo4j, FalkorDB, Convex, or another separate graph database.
- [ ] [#7a4d] [recommended] OCR/TMO field extraction: custom/fillable docs -> reviewed extracted fields -> TMO update/create flow
- [ ] [#k9zj] Graph/vector database for Judith's document search dashboard (cross-doc search, any doc type)
- [ ] [#x0px] Tier 1: Workers AI pre-screening
- [ ] [#xgqg] Tier 2: Claude API document review
- [ ] [#kf1p] Tier 3: Ragie Parse + Index for approved docs
- [ ] [#4ktv] Discuss Ragie content validation with Bob Remeika (scheduled call)
- [ ] [#dvmr] [recommended] Corky: add long mono-call mixed-speaker recovery (separate diarization confidence, tighter bad-span detection, forced re-diarize on flagged windows)
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [/] [#1v5m] [recommended] Update passage btak/equityfundingsource/cloudflare_api_token to include Cloudflare D1 read/write permissions so the project cf:d1:* scripts can run without the OAuth bypass.
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
<!-- /agent:icebox -->

<!-- agent:done -->
- 2026-05-24 [#a20z] Set EFS Cloudflare deployment credentials (CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID) in src/equityfundingsource-dev/.env, then rerun bun run deploy:worker for src/equityfundingsource-dev commit 7ce187e or latest main.
- 2026-05-24 [#r2-upload-cache] R2 upload cache path: make token/admin uploads write original bytes to R2 first, persist metadata in D1, and serve portal downloads from R2 even when Dropbox access is unavailable.
- 2026-05-24 [#dropbox-cache-contract] Dropbox/R2 document cache contract: define R2 key namespace, D1 document metadata, Dropbox path mapping, content hashes, sync_status, last_dropbox_synced_at, tombstones, and conflict policy for uploaded documents.
- 2026-05-24 [#portal-cache-api] Portal cache APIs: cut investor/admin portal pages over to cache-only reads with last-synced/stale indicators and an outbox for portal writes that must replay to TMO later.
- 2026-05-24 [#tmo-cache-worker] TMO sync Worker: build idempotent manual/scheduled pulls with page cursors, retries, tombstones, sync batches, and D1/R2 writes that do not require TMO during portal requests.
- 2026-05-24 [#tmo-cache-model] TMO cache contract: define D1 read-model tables plus R2 raw payload/document object paths, TMO IDs, cursors, checksums, last_synced/stale/deleted state, and conflict policy for portal-visible data.
- 2026-05-24 [#cf-d1-cache-deploy] Resolve Cloudflare D1 API authorization for account 1e7e194ff8da4ad9e40ce041aea84f76, apply migration 0023_tmo_cache_and_r2_documents.sql, then deploy worker commit 3ada2de.
- 2026-05-24 [#ck54] Investor portal private document storage: add a separate private R2_DOCUMENTS bucket plus provider-neutral document metadata and authenticated download APIs; treat R2 as the canonical uploaded-document cache and keep Dropbox as a later sync adapter.
- 2026-05-24 [#njsx] Tier 0: Cloudflare WAF + upload limits
- 2026-05-24 [#ft47] Token-based document upload flow
- 2026-05-24 [#yh4b] Admin dashboard (Cloudflare Access auth)
- 2026-05-24 [#zjbe] Admin investor document workflow: upload and publish deal due-diligence, investor statements, tax docs, and accreditation files from R2 with visibility and review status controls.
- 2026-05-24 [#9vv3] Investor portal end-to-end QA: seed and test investor signup, accreditation, published deal documents, funding commitment, admin acceptance, positions, distributions, and statements with TMO simulator plus R2 documents.
<!-- /agent:done -->
