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
EFS workers.dev production: As of Worker version `ff80afcd-c969-4377-bedd-ac33af4e8a44` (deployed 2026-06-09): the admin loan board is renamed **Deal Pipeline** (nav item, board page, lead-detail card; section id/route/endpoint unchanged) and its sidebar nav badge now counts **action-needed** items — open Leads (`new`/`contacted`) + loans in Applications/Processing/Documents/Underwriting — instead of `COUNT(*) FROM loans`, excluding terminal/parked stages (#pipeline-action-count, EFS `909b866`). The admin **Payments** section (`/portal-admin/payments`) adds a **Pipeline** board as the default view (with a Table toggle via `?view=table`) tracking borrower payments one-card-per-loan grouped Overdue/Due Soon/Current/Paid Off, each card showing next payment due date, amount, principal, and overdue count + total; read-only snapshot served by `GET /api/admin/payments/pipeline` (registered before `/:payment_id`), mutations stay on the Table view (#payments-pipeline, EFS `d8eb783`). Neither feature required a D1 migration. `/apply` public prequalification + D1-backed `application_leads` admin review and the public `/contact` form (D1-backed `contact_messages` + `/portal-admin/contact-messages` admin review) remain live (contact-form persistence from EFS commit `79ac942`, migration `0054_contact_messages` applied remote). Loan-application Approval is gated on a complete full application — `POST /api/admin/full-applications/:id/status` no longer accepts `accepted` (in_review/declined only) and `/approve` fails closed unless the submitted application is complete (#approve-requires-full-app, EFS `b136519`); the admin "Accept and send to TMO" button is disabled with a missing-parts list on incomplete apps (EFS `b731aa4`). Portal form-submission errors render a red banner at the top of the form plus an error toast via `portal/shared/form_error.tsx` (#form-error-standard). The admin Deal Pipeline (`/portal-admin/pipeline`) has a read-only "Leads" funnel column from open prequalification leads and the entry column is "Applications" (#pipeline-leads, EFS `877c3eb`); there is no separate "Approved" column (TMO has no such loan status). Admin Management sidebar links show item-count badges loaded from `GET /api/admin/nav-counts` (#admin-nav-counts, EFS `0d9708a`). Individual application leads have a detail page at `/portal-admin/application-leads/:lead_id` (`GET /api/admin/application-leads/:id`), linked from the leads list and the pipeline Leads column (#lead-detail, EFS `ef3b76e`). Contact-form staff notifications deliver to `CONTACT_NOTIFICATION_TO=btak.dev@gmail.com` (EFS `af4935d`) from `EFS_EMAIL_FROM=Equity Funding Source <info@briantakita.me>` via the Cloudflare Email Sending API (CLOUDFLARE_API_TOKEN secret); a live POST smoke test recorded staff_notification_status=sent. NOTE: staff_notification_status=sent means the Cloudflare Email API returned 2xx, not a guaranteed inbox delivery — if mail does not arrive, investigate briantakita.me SPF/DKIM/DMARC and Cloudflare Email Sending external-delivery limits. Public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications use `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com`; an EFS-owned sending domain in Resend is still pending (#f4rq). Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260609-225500.md`*

Compacted content:
- Archived 3 response topic(s): Move Deal Pipeline above Loan Applications; deploy; Plan a move to Hetzner US-East (Docker + nginx)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Deal Pipeline controls in the admin dialog (+ deploy) — opus-4-8

Shipped and deployed. The interactive **Deal Pipeline controls** from `img_3.png` (Status dropdown · Staff-email status · Send/Resend borrower invite) are now inline in the Deal Pipeline card dialog, so an admin can change a lead's status and (re)send the borrower invite without leaving the board.

**What changed (EFS `9234ba8`, superproject pointer `7b696114f`):**
- **New shared component** `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx` — extracts the status dropdown + staff-email status + send/resend-invite block, reusing the existing `/api/admin/application-leads/:id/status` and `/invite` endpoints and handing the updated lead back via `on_lead`.
- **Pipeline lead dialog** (`pipeline.tsx` `LeadModalBody`) now renders the controls; the redundant read-only "Staff notified"/"Borrower invite" rows were removed. Kanban lead cards stay read-only / non-draggable — only the dialog gained controls.
- **Full-page lead detail** (`application_lead_detail.tsx`) now consumes the same shared component (no behavior change, just DRY).
- Tests updated to follow the extraction + assert the dialog controls.

**Verification:** `bun run check` green — **493 tests pass** (added 2), `build` + `build:worker` clean. **Deployed** to production (Version `36773d8b-81ef-468f-86db-0b45df30c110`). Smoke test: `/`, `/funded-deals`, `/apply` → 200; `/portal-admin/pipeline` → 303→login (auth guard intact).

**On "all dialog types" — please confirm scope.** These controls are **lead-specific** (statuses new/contacted/converted/archived + borrower invite), so they map to the lead pipeline dialog (the screenshot) and the lead detail page — done. The other dialog types don't take the same controls:
- The **loan/deal** pipeline dialog already has its own inline actions (publish/close deal, delete loan, open full loan).
- **loan / payment / lender / funding / document / contact** detail modals aren't leads — a borrower-invite/lead-status block doesn't apply.

If by "all dialog types" you meant each dialog should get an **editable status dropdown for its own entity** (e.g. change a loan's pipeline stage from the loan dialog), that's a real but separate feature — each needs its own status-write endpoint. Tracked as **#pipeline-dialog-controls-scope**; tell me which dialogs + which actions and I'll build them.

### Re: Wider/shorter Deal Pipeline dialog + mobile scroll — opus-4-8

> **Queue prompt:**
>
> Make the dialog wider and shorter. ![img_4.png](img_4.png) Use a scrollbar for short monitors. Make sure this works on mobile.


Done and deployed. The root issue: `PortalModal` had `overflow:hidden` with **no max-height**, so the tall lead dialog (`img_4.png`) could clip or run off the bottom of short monitors with no way to scroll.

**What changed (EFS `3d90502`, pointer `658df787e`):**
- **Viewport-fit + scrollbar (all dialogs)** — `PortalModal` now caps at `max-height:calc(100dvh - 40px)` and scrolls internally (`overflow-y:auto`, `overscroll-behavior:contain`, touch scrolling). `dvh` accounts for mobile browser chrome, so it fits short screens and phones instead of clipping. This fixes every dialog, not just the lead one.
- **Wider** — the pipeline lead + loan dialogs go `max-w-lg` → `max-w-2xl` (32rem → 42rem).
- **Shorter** — the lead dialog body is now a responsive 2-column grid (`auto-fit, minmax(240px,1fr))`): two columns on desktop (roughly half the height), collapsing to one column on mobile.

**Verification:** `bun run check` green — full suite passes (added 2 layout tests), `build` + `build:worker` clean. **Deployed** to production (Version `4298b450-3a7d-480d-92ef-1030fcf7d875`, no DB migrations). Smoke test: `/`, `/apply` → 200; `/portal-admin/pipeline` → 303→login.

To see it: open `/portal-admin/pipeline`, click a lead card — the dialog is wider, the data sits in two columns, and on a short window it scrolls inside the dialog instead of overflowing.
<!-- agent:boundary:f84ee065 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~Make the dialog wider and shorter. ![img_4.png](img_4.png) Use a scrollbar for short monitors. Make sure this works on mobile.~~
- deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#hetz1] [recommended] Hetzner US-East (Ashburn) migration evaluation for EFS: EFS is a Cloudflare Workers app (D1, R2 x3, Durable Objects, Workers AI, CF Access, cron) with NO Dockerfile — a move to Hetzner Docker+nginx is a full replatform off CF primitives, not a container relocation. Confirm the driver (cost / data residency / long-running processes) and whether 'our Docker container' means EFS or monsterrodholders before scoping.
- [ ] [#pipeline-dialog-controls-scope] Confirm scope of 'all dialog types' for the Deal Pipeline controls: shipped on the lead pipeline dialog + full-page lead detail. The loan/deal pipeline dialog already has its own inline controls (publish/close deal, delete, open). If admin loan/deal cards or other admin detail modals should gain an editable pipeline-status dropdown, that needs its own status-write endpoint per entity — confirm which dialogs and which actions.
- [ ] [#8tkq] Loan/borrower-scoped wire-link issuance + routing: generate secure wire-links bound to a specific loan/borrower from the admin loan + wire-confirmation surfaces (not just the global Settings card), and model the title/escrow -> EFS -> lender delivery routing. Builds on the #wir3 delivery mechanism.
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
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
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
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
