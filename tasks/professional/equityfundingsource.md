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
queue: stop
---

## Status

<!-- agent:status patch=replace -->
EFS workers.dev production: As of Worker version `ff80afcd-c969-4377-bedd-ac33af4e8a44` (deployed 2026-06-09): the admin loan board is renamed **Deal Pipeline** (nav item, board page, lead-detail card; section id/route/endpoint unchanged) and its sidebar nav badge now counts **action-needed** items — open Leads (`new`/`contacted`) + loans in Applications/Processing/Documents/Underwriting — instead of `COUNT(*) FROM loans`, excluding terminal/parked stages (#pipeline-action-count, EFS `909b866`). The admin **Payments** section (`/portal-admin/payments`) adds a **Pipeline** board as the default view (with a Table toggle via `?view=table`) tracking borrower payments one-card-per-loan grouped Overdue/Due Soon/Current/Paid Off, each card showing next payment due date, amount, principal, and overdue count + total; read-only snapshot served by `GET /api/admin/payments/pipeline` (registered before `/:payment_id`), mutations stay on the Table view (#payments-pipeline, EFS `d8eb783`). Neither feature required a D1 migration. `/apply` public prequalification + D1-backed `application_leads` admin review and the public `/contact` form (D1-backed `contact_messages` + `/portal-admin/contact-messages` admin review) remain live (contact-form persistence from EFS commit `79ac942`, migration `0054_contact_messages` applied remote). Loan-application Approval is gated on a complete full application — `POST /api/admin/full-applications/:id/status` no longer accepts `accepted` (in_review/declined only) and `/approve` fails closed unless the submitted application is complete (#approve-requires-full-app, EFS `b136519`); the admin "Accept and send to TMO" button is disabled with a missing-parts list on incomplete apps (EFS `b731aa4`). Portal form-submission errors render a red banner at the top of the form plus an error toast via `portal/shared/form_error.tsx` (#form-error-standard). The admin Deal Pipeline (`/portal-admin/pipeline`) has a read-only "Leads" funnel column from open prequalification leads and the entry column is "Applications" (#pipeline-leads, EFS `877c3eb`); there is no separate "Approved" column (TMO has no such loan status). Admin Management sidebar links show item-count badges loaded from `GET /api/admin/nav-counts` (#admin-nav-counts, EFS `0d9708a`). Individual application leads have a detail page at `/portal-admin/application-leads/:lead_id` (`GET /api/admin/application-leads/:id`), linked from the leads list and the pipeline Leads column (#lead-detail, EFS `ef3b76e`). Contact-form staff notifications deliver to `CONTACT_NOTIFICATION_TO=btak.dev@gmail.com` (EFS `af4935d`) from `EFS_EMAIL_FROM=Equity Funding Source <info@briantakita.me>` via the Cloudflare Email Sending API (CLOUDFLARE_API_TOKEN secret); a live POST smoke test recorded staff_notification_status=sent. NOTE: staff_notification_status=sent means the Cloudflare Email API returned 2xx, not a guaranteed inbox delivery — if mail does not arrive, investigate briantakita.me SPF/DKIM/DMARC and Cloudflare Email Sending external-delivery limits. Public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications use `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com`; an EFS-owned sending domain in Resend is still pending (#f4rq). Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260610-023457.md`*

Compacted content:
- Archived 14 response topic(s): Deal Pipeline controls in the admin dialog (+ deploy); Wider/shorter Deal Pipeline dialog + mobile scroll; deploy; 11 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: "Open Investor Profile" → full investor profile in the 2nd-level dialog — opus-4-8

Done and deployed. The "Open investor profile" button on the admin Investor Pipeline card already opened a stacked 2nd-level dialog (`#b0wq`), but it only carried the **account + address** editor. It now shows the **entire investor profile**. Shipped EFS `b3e9ddd`, deployed (version `9652f4ab`).

**What the 2nd-level dialog shows now** (investor role only):
- **Account + Address** — the existing editable form (unchanged, still saves via `PUT /api/admin/users/:id`).
- **Investor profile** (new section): **lender account**, **accreditation status**, **verification type**, **accreditation evidence on file**, **verified-on** / **renews-or-expires** dates, and **accreditation notes** — read from the existing `GET /api/admin/users/:id/investor-accreditation`.
- **Investor Pipeline** (page → `/portal-admin/investor-pipeline`) ▸ **Investor Deals**, **Funding Commits**
- **Deal Pipeline** (page → `/portal-admin/pipeline`) ▸ **Application Leads**, **Loan Applications**

**Why those parts are read-only here:** the lender account is assigned through the lender-account request/approval flow, and accreditation has an Accredit/Reject decision workflow that already lives on the pipeline card (one level up, with reviewer + timestamp). Making them free-text fields in the generic profile form would bypass those flows, so the dialog surfaces them in full but routes edits to the right place (noted inline in the dialog).

**Verification:** `bun test` **518 pass / 0 fail** (+1 new); `build` + `build:worker` clean. Deployed to `https://equityfundingsource.brian-takita.workers.dev` — root and the borrower payments-config endpoint both return 200. No new D1 migrations (this is UI + an existing read endpoint).

This deploy also shipped the previously-pushed **admin-configurable payments + borrower reveal panel** (`c790ed3`) and the earlier `#eyp0`/`#emrt` work that was awaiting a deploy.

### Re: Nest Investor Pipeline under Deal Pipeline (collapsible nav groups) — opus-4-8

Done and deployed. The admin sidebar was a flat list; it's now two clickable pipeline pages that each own a collapsible submenu, with **Investor Pipeline moved directly under Deal Pipeline**. Shipped EFS `fad9013`, deployed (version `f1a5c05b`).

**New nav structure:**
- **Deal Pipeline** (page → `/portal-admin/pipeline`) ▸ **Application Leads**, **Loan Applications**
- **Investor Pipeline** (page → `/portal-admin/investor-pipeline`) ▸ **Investor Deals**, **Funding Commits**

Both parent rows are still real links — clicking the label navigates to the pipeline page; a caret next to it expands/collapses the submenu. The group that owns the current page **auto-expands and highlights**, and detail pages map to their owning item (e.g. a lead detail keeps **Application Leads** lit). Count badges still render on both parents and children.

I read "move under" as **reposition + group**, not literal 3-level nesting: your follow-up said each pipeline is its own page with its own submenu and listed each submenu's contents separately, so they're two sibling collapsible groups with Investor Pipeline placed right below Deal Pipeline. Contact Messages, Payments, Documents, etc. stay as top-level items.

**Verification:** `bun test` **519 pass / 0 fail** (added a nav-structure test; updated the nav-highlight + ordering tests for the new tree); `build` + `build:worker` clean. Deployed to `https://equityfundingsource.brian-takita.workers.dev` — root returns 200. No D1 migrations (UI + CSS only).

### Re: Website rename + tab-menu pipelines + drop nav submenus — opus-4-8

Done and deployed (EFS `001034c`, version `7c983e32`). Three changes:

**1. Public Content → Website.** Renamed the section + label + page title, and moved the route `/portal-admin/public-contents` → `/portal-admin/website`. Old URLs **301-redirect** to the new ones (verified live). The data API (`/api/admin/public-content`) is untouched.

**2. Website-style tab menu on both pipeline pages.** Reused the same "Pages | Blog Posts | Funded Deals" segmented menu (extracted as a shared `PortalTabs` + `?tab=` sync helper):
- **Deal Pipeline** → `Pipeline` · `Application Leads` · `Loan Applications`
- **Investor Pipeline** → `Pipeline` · `Investor Deals` · `Funding Commits`

The lists render in-page (added an `embedded` mode to Application Leads, Investor Deals, Funding Commits; Loan Applications already had one), so the active tab is remembered across reloads via the URL.

**3. Removed the left-nav submenus.** The sidebar is flat again — **Deal Pipeline** and **Investor Pipeline** are single items; the four sub-lists are now tabs rather than nav entries. Their pages (and detail pages like a lead or funding detail) highlight their parent pipeline item.

**Verification:** `bun test` **519 pass / 0 fail** (updated the nav/route/section tests for the new structure); `build` + `build:worker` clean. Deployed — root 200, `/portal-admin/website` reachable, `/portal-admin/public-contents` → 301 → `/portal-admin/website`. No D1 migrations.

### Re: Messages hub — Portal + Contact Messages tabs, chat moved to /portal-messages — opus-4-8

Done and deployed (EFS `8357ee3`, version `42a39fef`). Same tab pattern as the Website/pipeline pages:

- **Moved the portal chat** from `/portal-admin/messages` to **`/portal-admin/portal-messages`** (new `portal-messages` section). Old chat deep-links (`/portal-admin/messages/:id`) **301-redirect** to the new URL (verified live).
- **`/portal-admin/messages` is now a Messages hub** with the segmented tab menu: **Portal Messages** (the chat, default) and **Contact Messages** — each embedded in-page, with `?tab=` remembered across reloads.
- **Removed the standalone "Contact Messages" left-nav item.** The main nav keeps a single **Messages** item that opens the hub; Contact Messages (and its detail page) now highlight the Messages item.

**Verification:** `bun test` **520 pass / 0 fail** (+1 new, updated the contact-messages nav test); `build` + `build:worker` clean. The first `deploy:worker` attempt hit a transient Cloudflare error; the retry deployed cleanly. Live checks: root 200, `/portal-admin/messages` and `/portal-admin/portal-messages` reachable, `/portal-admin/messages/:id` → 301 → `/portal-admin/portal-messages/:id`. No D1 migrations.
<!-- agent:boundary:e327a475 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" -->
- Move /portal-admin/messages to /portal-admin/portal-messages. Create /portal-admin/messages which contains "Portal Messages" (default) and "Contact Messages". The admin main nav has the new "Messages" "/portal-messages/messages". Remove "Contact Messages" from the admin main nav. Deploy 
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#efspgo] [recommended] EFS online-payment go-live config (after Robert provides Stripe account): set Cloudflare secrets STRIPE_SECRET_KEY + STRIPE_WEBHOOK_SECRET + var STRIPE_PAYMENTS_ENABLED=true (wrangler secret put / wrangler.toml), point a Stripe webhook at /api/stripe/webhook, then in admin Platform Settings fill the borrower payment wire/ACH + lender ACH values and flip the per-method enable toggles. Deploy with bun run deploy:worker. Capability is BUILT + admin-configurable (EFS c790ed3); this is config + deploy only.
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
