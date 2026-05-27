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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `0988ee2c-fac5-4e60-a39c-4107791ce895` as of 2026-05-27; local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes a separate Borrower Login action alongside Apply for a Loan. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260527-091319.md`*

Compacted content:
- Archived 1 response topic(s): deploy
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ From judith-krug
❯ > Plus hold up on any more work till we meet for the demo and discuss. We are trying to get access to TMO back now. We got locked out by mistake.

❯ Please break down the features and how we can implement the features to replace TMO in a better, more integrated, easier to use, and resellable way.

❯ What are #next-steps?
### Re: TMO replacement plan — gpt-5

I will hold on any new build work until the demo/meeting unless there is a production support issue you explicitly want handled. For the meeting, I would frame this as replacing the parts of TMO that actually run EFS day to day, not cloning every TMO screen. The better product is an EFS operating system: borrower, broker, investor, lender, document, funding, and staff workflows in one place, with TMO treated first as an integration/data source and later as something we can phase out module by module.

Current foundation we can demo: public site and `/apply`, role-based login, borrower application/status/document areas, investor deal/funding flow, admin loan/deal/user/lender-request management, D1/R2-backed document cache direction, token upload work, and partial TMO status/funding integration. Broker is the biggest visible gap because the UI exists but backend/TMO-backed records are still pending.

| Area | Replacement feature | Better, integrated, resellable implementation |
| --- | --- | --- |
| Borrower intake | Prequalification, full application, status tracker, profile, document upload | One borrower timeline from first lead through funded/declined/closed, with staff-visible status, upload links, and notifications. |
| Broker intake | Submit deal, pipeline, document requests, commissions, referral status | Broker portal with real backend records, admin review, notifications, and commission tracking instead of static/demo data. |
| Staff loan ops | Admin loan queue, status changes, tasks, underwriting checklist, closing/funding state | Single admin workflow across borrower and broker submissions with audit history, reusable checklists, and controlled TMO sync/writeback. |
| Investor/lender ops | Deal marketplace, diligence docs, commitments, lender account requests, positions/distributions | Investor portal driven by EFS-approved opportunities, R2-secured documents, admin approval, and TMO funding calls only where still needed. |
| Documents | Uploads, requested docs, secure download, document type tracking | R2 as canonical storage, D1 metadata, token uploads, Dropbox/TMO as adapters, and future OCR/search layered on top. |
| Messaging/notifications | Borrower/admin messages, email alerts, broker/investor updates | D1 message threads plus email notifications and later inbound email replies, so users do not need to know which portal action triggered an update. |
| TMO integration | Import/cache, webhook or polling sync, writeback, outbox replay | No live TMO dependency during page loads; use cache + outbox first, then replace TMO functions as each module becomes mature. |
| Reporting/audit | Pipeline, funding, stale sync, staff activity, compliance proof | Built-in dashboards and event history that can be reused by future private-lending clients. |
| Resale/productization | Tenant branding, configurable fields/statuses, permissions, document templates | Package the system as a white-label private-lending portal, not a one-off EFS custom app. |

Implementation path:

1. Stabilize the demo: freeze new feature work, show what exists on workers.dev, and use the demo to identify which workflows EFS actually needs before spending more build time.
2. Restore and inventory TMO access: capture screenshots/exports for borrowers, loans, lenders, statuses, docs, funding, reports, notifications, and any API/webhook settings. This prevents us from guessing at replacement scope.
3. Build the replacement in layers: EFS database/cache first, portal workflows second, TMO sync/writeback third, AI document extraction/search fourth, and tenant-ready configuration last.
4. Keep TMO as a bridge, not the core: while access is unstable, the app should keep accepting leads/uploads/messages and queue anything that needs TMO replay later.
5. Productize only the stable primitives: roles, portals, loan pipeline, documents, notifications, audit log, configurable statuses/fields, branding, and tenant isolation. Do not productize EFS-specific shortcuts.

For the demo meeting, I would make the decisions concrete:

- Which TMO workflows are must-replace first: borrower applications, broker submissions, admin pipeline, investor funding, documents, lender records, reports, or notifications.
- Whether EFS wants a TMO bridge for 30-90 days or a harder cutover once core data is migrated.
- Which roles must be usable in the first replacement release: admin + borrower only, or admin + borrower + broker + investor.
- Whether resale means white-label for similar private lenders, or just a cleaner internal EFS system with optional future reuse.
- Who approves legal/disclosure language, document types, status names, email copy, and funding workflow rules.

#next-steps:

1. Do not start new feature implementation before the demo unless explicitly approved.
2. Prepare a demo script around the current live system: public site/apply, borrower flow, admin review, investor funding, document handling, and known broker/TMO gaps.
3. Once TMO access is restored, inventory the actual screens, fields, statuses, reports, and integration options EFS uses.
4. Turn the meeting decisions into a phased roadmap: demo stabilization, MVP replacement, TMO bridge/cutover, AI document/search layer, resale packaging.
5. After approval, resume with the smallest replacement slice that proves the integrated model: borrower/broker intake to admin review to document collection to status/notification updates.

Closeout evidence: no product code or deployment changes were made. I added backlog items for the demo agenda, TMO inventory, and phased replacement roadmap. Commands run: `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, targeted `tsift` searches, targeted doc reads, and this `agent-doc finalize`. Verification is document-only; no test suite was needed. Confidence is medium-high pending the restored TMO access inventory. Escalation: none.
<!-- agent:boundary:77fa174b -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#y17k] Prepare a no-new-build demo agenda for the EFS/TMO replacement meeting: current workers.dev walkthrough, TMO pain points, replacement MVP decisions, and post-meeting approval gates.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
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
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks: confirm outbound webhook support (Robert to check with Mortgage Office)
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
- [/] [#1v5m] Resolved 2026-05-25: created an EFS-specific Cloudflare API token scoped to account 1e7e194ff8da4ad9e40ce041aea84f76 with D1, Workers Scripts, R2, observability, and read-only user/account metadata permissions; updated passage btak/equityfundingsource/cloudflare_api_token. D1 migration list/apply now succeeds and reports no pending migrations.
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->
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
- 2026-05-24 [#2rpv] Set Cloudflare Access env vars (ACCESS_TEAM_DOMAIN, ACCESS_AUD) on the equityfundingsource Worker and configure the CF Access application that protects /login/access-callback, then verify an admin sign-in via Google SSO.
- 2026-05-24 [#0r24] Apply migration 0025_upload_tokens.sql to remote D1 once review #1v5m unblocks D1 API token permissions, then smoke test POST /api/admin/upload-tokens and POST /api/uploads/by-token/<token> against the live Worker.
- 2026-05-24 [#7chq] Apply migration 0026_broker_portal.sql to remote D1 alongside 0025 once review #1v5m unblocks D1 API token permissions, then smoke test POST /api/broker/deals against the live Worker.
<!-- /agent:done -->
