---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

25 pages deployed with form backend (D1 + R2 + email). D1/R2 TMO + document cache migrated and deployed to Worker version 59aaf003.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260524-222001.md`*

Compacted content:
- Archived 6 response topic(s): R2/TMO Cache; R2 As Dropbox Cache; TMO/R2 Cache Queue; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
<!-- agent:boundary:2788e3be -->
<!-- /agent:exchange -->
###

<!-- agent:queue -->
preset #spec-test-commit-push-deploy
- do [#cf-d1-cache-deploy]
- do [#ck54]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
- [ ] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [ ] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
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
