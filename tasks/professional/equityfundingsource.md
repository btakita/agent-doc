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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-222009.md`*

Compacted content:
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
### Re: do [#qakx] — opus-4-7

Implemented the D1/R2-backed investor portal document workflow that replaces the static `INVESTOR_DOCUMENTS` fixtures on Documents, Statements, and Accreditation. Investor-side end of qakx is code-complete and deployed; gating until the schema lands on remote D1 and admin uploads start writing rows.

#### Schema (`migrations/0024_investor_documents_and_accreditation.sql`)
- `investor_documents` — user-scoped catalog with `category` (`tax | legal | due_diligence | statement | accreditation`), `action_kind`, `status`, R2 `bucket/key/content_hash`, `published_at`, optional `deal_id` / `loan_id`.
- `investor_document_requests` — `kind` (`tax | statement | legal | other`), `description`, `status` (`open | in_review | fulfilled | rejected`), optional `fulfilled_document_id` foreign-keyed back to `investor_documents`.
- `investor_accreditation_verifications` — `verification_type` (`income | net_worth | institution | professional`), `status`, `verified_at`/`expires_at`, `evidence_document_id`, reviewer metadata.

#### Repo + view helpers (`lib/investor/documents_repo.ts`)
- Whitelists/validators for category, request kind, and verification type.
- `investor_documents_R_`, `investor_document_R_`, `investor_document_create_`, `investor_document_requests_R_`, `investor_document_request_create_`, `investor_accreditation_latest_R_`, `investor_accreditation_upsert_`.
- `investor_accreditation_view_` derives `expired` when a verified row is past `expires_at` and computes a 60-day renewal-opens timestamp.

#### Authenticated APIs (`lib/investor/routes.ts`)
- `GET /api/investor/documents` — filterable list + open requests (investor-only, session-auth).
- `POST /api/investor/documents/request` — validates kind/description, stores an open request.
- `GET /api/investor/documents/:id/download` — owner-only R2 stream via `env.R2_DOCUMENTS.get`, sets `inline` or `attachment` from `action_kind`, falls back to 404/409 when the file isn't there yet.
- `GET /api/investor/accreditation` — returns the view-modeled latest record (or pending-default).
- `POST /api/investor/accreditation/upload` — multipart upload, 25 MB cap, writes to private R2 via `document_cache_put_` (`investor/accreditation` folder under `user-<id>`), inserts a published `investor_documents` row, then creates a pending verification linked to that evidence.

#### Portal pages (`lib/ui--browser--efs/portal/investor/*.tsx`)
- `Documents` — fetches `/api/investor/documents`, groups by `tax | legal | due_diligence`, renders empty/loading/error states, exposes an inline "Request document" modal that POSTs to `/request` and shows open requests with status pills.
- `Statements` — fetches `?category=statement`, keeps the right-rail summary, derives "next quarterly package" date dynamically, and "Generate custom report" now POSTs a `statement`-kind document request.
- `Accreditation` — fetches `/api/investor/accreditation`, shows status pill + verified/expires dates from the view, and a step tracker driven by `has_evidence` + `status`. Adds an upload card (file + verification type + notes) that POSTs to `/accreditation/upload`.
- Removed the static `INVESTOR_DOCUMENTS` / `InvestorDocument` fixtures from `data.ts`.

#### Tests (`tests/investor-documents.test.ts`)
- Whitelist coverage and accreditation view (pending default + verified-past-expiry → `expired` + renewal window math).
- Migration string checks (table names, R2 columns, defaults, foreign key on `evidence_document_id`).
- Routes string checks (each endpoint mounted; uses `document_cache_put_` + repo helpers).
- Portal components assert API consumption and fixture removal.

#### Verification
- `bun test` — 65 pass / 0 fail (5 new tests in `investor-documents.test.ts`).
- `bun run build` + `bun run build:worker` — clean (worker 379.6 KB).
- `bun run deploy:worker` — deployed `equityfundingsource` version `a5e0f4b5-10ef-427d-9d8f-628ca637e652`.
- `bun run cf:d1:migrate:remote` — fails with code 7403 (token lacks D1 write); same blocker tracked in the existing review item for the Cloudflare API token D1 scope. Migration `0024_investor_documents_and_accreditation.sql` is pushed to `main` but not yet applied to the remote D1 instance, so the new APIs will 500 in production until the D1 token is rotated and the migration is applied.

Gating qakx because end-to-end QA still needs: (1) the new migration applied on remote (waiting on the existing token-perms review item), (2) admin upload UI from the sibling admin-investor backlog item to actually populate `investor_documents`, (3) live investor flow covered under the portal end-to-end QA item. Code path is in place — `cf:d1:migrate:remote` will land the schema as soon as the token is rotated.
<!-- agent:boundary:38c1fe3d -->
<!-- /agent:exchange -->
###

<!-- agent:queue auto -->
preset #spec-test-commit-push-deploy
- do [#qakx]
- do [#zjbe]
- do [#yh4b]
- do [#ft47]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#zjbe] Admin investor document workflow: upload and publish deal due-diligence, investor statements, tax docs, and accreditation files from R2 with visibility and review status controls.
- [ ] [#yh4b] Admin dashboard (Cloudflare Access auth)
- [ ] [#ft47] Token-based document upload flow
- [ ] [#njsx] Tier 0: Cloudflare WAF + upload limits
- [ ] [#9vv3] Investor portal end-to-end QA: seed and test investor signup, accreditation, published deal documents, funding commitment, admin acceptance, positions, distributions, and statements with TMO simulator plus R2 documents.
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
<!-- /agent:done -->
