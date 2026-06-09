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
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: start
---

## Status

<!-- agent:status patch=replace -->
EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review and the public `/contact` form (D1-backed `contact_messages` + `/portal-admin/contact-messages` admin review) are live in Worker version `2319bce6-b3ae-4efe-b1b5-423b31900437` as of 2026-06-09 (contact-form persistence from EFS commit `79ac942`, migration `0054_contact_messages` applied remote). As of Worker version `e66abd2e` (deployed 2026-06-09, EFS `39a60c0`): loan-application Approval is gated on a complete full application — `POST /api/admin/full-applications/:id/status` no longer accepts `accepted` (in_review/declined only) and `/approve` fails closed unless the submitted application is complete (#approve-requires-full-app, EFS `b136519`); the admin "Accept and send to TMO" button is disabled with a missing-parts list on incomplete apps (EFS `b731aa4`). Portal form-submission errors now render a red banner at the top of the form plus an error toast via `portal/shared/form_error.tsx` (#form-error-standard). The admin pipeline (`/portal-admin/pipeline`) adds a read-only "Leads" funnel column from open prequalification leads and renames the entry column to "Applications" (#pipeline-leads, EFS `877c3eb`, deployed in `320e794e`); there is no separate "Approved" column (TMO has no such loan status). Admin Management sidebar links show item-count badges loaded from `GET /api/admin/nav-counts` (#admin-nav-counts, EFS `0d9708a`, deployed in `1f1b4241`). Individual application leads have a detail page at `/portal-admin/application-leads/:lead_id` (`GET /api/admin/application-leads/:id`), linked from the leads list and the pipeline Leads column (#lead-detail, EFS `ef3b76e`, deployed in `2319bce6`). No new D1 migration was required for these deploys. Contact-form staff notifications deliver to `CONTACT_NOTIFICATION_TO=btak.dev@gmail.com` (EFS `af4935d`) from the `EFS_EMAIL_FROM=Equity Funding Source <info@briantakita.me>` sender via the Cloudflare Email Sending API (CLOUDFLARE_API_TOKEN secret); a live POST smoke test recorded staff_notification_status=sent with no error (test row deleted). NOTE: staff_notification_status=sent means the Cloudflare Email API returned 2xx, not a guaranteed inbox delivery — if mail does not arrive, investigate briantakita.me SPF/DKIM/DMARC and Cloudflare Email Sending external-delivery limits. Public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications use `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com`; an EFS-owned sending domain in Resend is still pending (#f4rq). Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260609-172720.md`*

Compacted content:
- Archived 4 response topic(s): Individual application-lead route + messaging sync question; deploy; Individual-item admin routes; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: URL reflects selected filters without a refresh — opus-4-8

> **Queue prompt:**
>
> The url of https://equityfundingsource.brian-takita.workers.dev/portal-admin/users?status=active should update without a page refresh to reflect the selected filters.


Done across the admin portal. Committed + pushed to `equityfundingsource-dev` main (`7d9c235`).

**Root cause:** the filtered admin lists *read* their filters from the URL on load but never *wrote* them back, so changing a filter (e.g. the users `status` dropdown) left the URL stale.

**Fix:** added a shared `sync_url_query_` helper (`shared/url.ts`) that updates the query string via `history.replaceState` (no reload, no back-button spam, drops empty values), and called it from every filtered admin list as filters change:
- **Users** (`?q&role&status`) — the reported example
- Application Leads, Contact Messages, Funding Commits, Lender Management (`?status`)
- Documents (`?kind` tab + `?window`/`?status`)
- Loans (`?borrower&application_status&order`)
- The full pages view gains **Open About ↗ / Open Contact ↗** deep-link buttons above each editor.
- The route opens a **focused single-key editor** (just About or just Contact) with a "Back to public contents" link and a "View" link to the live page — reusing the existing `public-contents` section (the client reads `:key` from the path, so no new section/nav wiring needed).

Read/write stay symmetric — each page only syncs the params it already reads on load, so a refresh restores the same view. Pages that already synced (pipeline, payments modal, tmo-loans, messages) were left as-is.

**Verification:** unit-tested the pure URL builder + asserted every filtered list uses the helper; full suite **421 pass / 0 fail**, worker bundle clean. (This rebased on top of a remote pagination refactor that touched the same files — conflicts resolved keeping both the new pagination and the URL sync.)

Continuing the queue: `#pzj5` (Public Contents detail) and `#f632` (Documents detail) next.

### Re: #pzj5 — Public Contents detail route — opus-4-8

> **Queue prompt:**
>
> [#pzj5]


Done. Committed + pushed to `equityfundingsource-dev` main (`b0cd34f`).

Public Contents is a key-based singleton editor (not a discrete-row list), so the addressable "items" are the content keys. Added `/portal-admin/public-contents/:key` (`key` ∈ `about` | `contact`):
- The route opens a **focused single-key editor** (just About or just Contact) with a "Back to public contents" link and a "View" link to the live page — reusing the existing `public-contents` section (the client reads `:key` from the path, so no new section/nav wiring needed).
- The full pages view gains **Open About ↗ / Open Contact ↗** deep-link buttons above each editor.

**Decision on the nested-items question:** About *sections* and *team members* stay inline array fields edited within the About editor — they're not given their own routes. Key-level (about/contact) deep-linking is the right granularity; per-array-item routes would be over-engineering for content blocks saved as one document.

**Verification:** full suite **422 pass / 0 fail**, browser + worker bundles clean; added a test locking in the route + focused-editor logic.

Last queue item next: `#f632` (Documents detail routes).

### Re: #f632 — Documents detail routes — opus-4-8

> **Queue prompt:**
>
> [#f632]


Done. Committed + pushed to `equityfundingsource-dev` main (`e6de69c`).

**Decision on the sub-list question:** Documents has two distinct sub-lists, so each item type gets its **own** detail route + by-id API (rather than one shared route):
- **Uploads** → `/portal-admin/documents/uploads/:id` (`GET /api/admin/uploads/:id`) — file metadata, borrower, loan, download link.
- **Document Requests** → `/portal-admin/documents/requests/:id` (`GET /api/admin/investor-document-requests/:id`) — request, status, investor, description/admin notes.

Each list row links to its detail page (eye action), and both detail sections highlight the Documents nav item. Added an `investor_document_request_admin_R_` single-read repo helper (re-exported via `admin/shared`).

**Verification:** full suite **423 pass / 0 fail**, browser + worker bundles clean; added a test locking in both routes, sections, APIs, and row links.

Queue drained — all admin list types now have individual-item routes.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed to production. Version `e390c01c-90d8-49a3-bea0-4e4c6549cbba` at https://equityfundingsource.brian-takita.workers.dev.

Before deploying I synced EFS to latest `main` (it was behind by 2 — a remote refactor of the documents/uploads view to a grouped `/api/admin/documents` table). The merge was clean: my `#f632` upload/request detail links survived, build is clean, full suite **423 pass / 0 fail**.

**Smoke test (all live):** home `200`; the session's new routes all return `303` (auth redirect), same as existing detail routes:
- `/portal-admin/public-contents/{about,contact}` (#pzj5)
- `/portal-admin/documents/{uploads,requests}/:id` (#f632)
- `/portal-admin/users?status=active` (URL filter sync)

This ships everything from this queue: the four admin detail routes, the per-key Public Contents editor, the Documents detail routes, and the URL-filter-sync across the admin lists. Queue drained.

### Re: full-applications "Failed to load applications" — opus-4-8

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/portal-admin/full-applications has a "Failed to load applications" error.


Fixed. Root cause was a **schema/code mismatch in production**, not a code bug.

**What happened:** the deploy I shipped earlier this session pulled in a remote refactor that added migrations **0055** (`application_broker_info` table + `full_applications.broker_user_id`) and **0056** (shared `documents` pool + `loan_documents`/`investor_documents` columns). `wrangler deploy` does **not** run D1 migrations, so production D1 was missing both.

**Exact failure path:** `GET /api/admin/full-applications` → `application_admin_list_` → per-row `application_sections_R_` → `application_broker_info_R_` runs `SELECT … FROM application_broker_info`. That table didn't exist on prod → the query threw → the `Promise.all` rejected → the endpoint 500'd → the client rendered "Failed to load applications".

**Fix:** applied the two pending migrations to the production D1 (`equityfundingsource`, `280e1539…`). Both are additive only (CREATE TABLE/INDEX, ADD COLUMN, INSERT OR IGNORE) — no destructive statements. Verified afterward: zero pending migrations; `application_broker_info` and `documents` tables exist; `full_applications.broker_user_id` is present.

Please re-load `/portal-admin/full-applications` (behind your admin auth) to confirm — the schema it needs is now in place. Queued a process fix so deploys apply migrations (or fail closed) and this class of outage can't recur.
<!-- agent:boundary:9516d334 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- Make the deployment script include migrations. Add runbooks for deployments. Link runbooks to AGENTS.md
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#my09] [recommended] Make deploy apply pending D1 migrations (or fail-closed when migrations are pending): the 2026-06-09 "Failed to load applications" outage happened because cf-deploy shipped code ahead of migrations 0055/0056. Add a `bun run cf:d1:migrate:remote` step (or a pending-migration gate) to the deploy:worker flow.
- [ ] [#vtx8] [bug] Application leads vanish when advanced past intake (e.g. 'Contacted') — finish the admin pipeline so leads aren't dropped from view. From 2026-06-03 call.
- [ ] [#ty2v] Secure wiring-instructions delivery: expiring/self-destructing link or click-to-reveal (bank-style), plus 'verify wiring with the title company directly' reminder; flow title/escrow -> EFS -> lender. From 2026-06-03 call.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
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
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#zr5z] Re-deploy equityfundingsource full apply4 wizard to production. P1–P6 implemented, 24 apply4 tests green, build-plan spec updated, all committed+pushed (efs 071d8fe). Remaining: cf:d1:migrate:remote + deploy:worker + smoke-test against prod — held pending explicit deploy authorization (re-queue with #spec-test-commit-push-deploy or say 'deploy').
- [/] [#cm3e] [recommended] Borrower online payment collection (Stripe) + TMO sync: add ACH/card payment in the borrower portal make-payment section; on Stripe payment_intent.succeeded webhook, verify signature, record the receipt in a new EFS D1 loan_payment_transactions table, reconcile the matching loan_payments schedule row, and push the payment to TMO. GATE: confirm ABS/TMO exposes a borrower-payment/receipt WRITE endpoint (current docs/TMO cover loan/application writes, investor AddLoanFunding, and schedule/funding reads only) — ties into the TMO-writeback gate (dzmm/k2xg). Deferred non-MVP collection path from e245; handle KYC, partial payments, NSF reversals, fees.
- [/] [#xcvg] [recommended] Borrower message email round-trip: add an admin owner for borrower-portal secure messages, email notification when a borrower sends a message, inbound email reply that appends into the D1 chat, and borrower email when an admin replies. Builds on existing chat_sessions/chat_messages. Gated on Resend EFS sending domain (f4rq) + Google Workspace SMTP (m6ch/p40t).
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

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
