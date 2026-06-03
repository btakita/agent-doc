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
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `734df89a-8af6-401e-83e4-fd5215a48e3f` as of 2026-05-28 from EFS commit `0afd62b`; public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260603-203838.md`*

Compacted content:
- Archived 14 response topic(s): TMO BorrowerView links; Should these replace our portal for the demo?; Can we replicate these workflows in our portal?; 11 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: apply4 P2 — personal + addresses + e-sign — done — opus-4-8

> **Queue prompt:**
>
> [#f3b2]


P2 shipped (commit `a2b68f8`, pushed). `bun run check` green — 246 tests.

- **Migration 0037**: `application_personal` (SSN stored as a **masked token only**, never raw — security-tier-0), `application_addresses` (current/former/mailing), `application_signatures` (ESIGN/UETA `esign_consent` + `declaration_version` + IP/UA audit trail).
- **Section repo + API**: `GET …/sections` (resume), `PUT …/personal`, `PUT …/addresses`, `POST …/sign` (consent-gated; captures `cf-connecting-ip`/`x-forwarded-for` + `user-agent`), all ownership-checked.
- **Wizard UI**: 3 steps (Personal → Addresses → Review & e-sign) with save-per-step and resume from saved sections.

Continuing the queue to P3 (`#2eaf` — employment & income).

### Re: apply4 P3 — employment & income — done — opus-4-8

> **Queue prompt:**
>
> [#2eaf]. deploy to production


P3 shipped (commit `379ac66`, pushed). `bun run check` green — 249 tests.

- **Migration 0038**: `application_employment` (current/additional/former with the apply4 gross monthly income breakdown — base/overtime/bonus/commission/military/other), `application_income` (other-source income).
- **Repo + routes**: employment/income replace+read; `PUT …/employment`, `PUT …/income`, both folded into `GET …/sections` for resume.
- **Wizard**: new Employment step (current employer + self-employed, gross-income breakdown, add-more other-income rows) with save/resume.

Continuing the queue to P4 (`#1tk7` — assets/liabilities + Real Estate Owned).

### Re: deploy to production (after P3) — done — opus-4-8

Deployed P1–P3 live. Worker version `85fa9cfc` at https://equityfundingsource.brian-takita.workers.dev.

- **Applied prod D1 migrations 0034–0038** (chat admin owner; `loan_payment_transactions`; `full_applications` + personal/addresses/employment/income/signatures).
- **Prod collision fixed:** migration 0036 first failed — production has a **legacy `applications` table** (old intake schema, predates the migration system, unused by current code). Renamed the apply4 table to `full_applications` (FKs + repo + test updated; legacy table left untouched), re-migrated, redeployed. Commit `712b91e`.
- **Live smoke tests green:** `/apply` → 200; `/portal-borrower/full-application` (unauth) → 303 → `/login?role=borrower&next=…` (hybrid handoff + `next` live); `/api/borrower/payments/config` → `{stripe_enabled:false}`; `/api/stripe/webhook` (unsigned) → 501. All gated features fail-closed.

Note: public `/apply` now routes prequal submits into the partial wizard (Personal → Addresses → Employment → e-sign; final submit is P5). Continuing the queue to P4 (`#1tk7`); prod stays at P1–P3 until a re-deploy (queued reminder added).

### Re: apply4 P4 — assets, liabilities & REO — done — opus-4-8

> **Queue prompt:**
>
> [#1tk7]


P4 shipped (commit `678cd59`, pushed). `bun run check` green — 253 tests.

- **Migration 0039**: `application_assets` (account number stored as a **masked token**), `application_liabilities`, `application_reo` + `application_reo_mortgages` child table (cascade; masked tokens).
- **Repo + routes**: assets/liabilities replace+read; REO replace+read with **nested per-property mortgages**; `PUT …/assets`, `…/liabilities`, `…/reo`, all in `GET …/sections`.
- **Wizard**: new "Assets & property" step (add-more assets, liabilities, and REO properties with creditor/balance) with save/resume.

Continuing the queue to P5 (`#az2w` — subject property + Declarations A–M + submit). Prod stays at P1–P3 until the queued re-deploy.

### Re: apply4 P5 — subject property + declarations + submit — done — opus-4-8

> **Queue prompt:**
>
> [#az2w]


P5 shipped (commit `a91d791`, pushed). `bun run check` green — 256 tests. **This completes the borrower-facing apply4 wizard (P1–P5).**

- **Migration 0040**: `application_subject_property` (loan amount/purpose/occupancy/value/units/flags), `application_declarations` (JSON answers map for A–M).
- **Repo + routes**: subject-property + declarations upsert/read (in `…/sections`); `POST …/submit` transitions draft→submitted, **gated on a recorded e-signature**, idempotent (only promotes a draft).
- **Wizard**: Property & declarations step (loan/property fields + A–M yes/no) and a Submit action after e-sign, with a submitted-state confirmation.

The borrower can now complete and submit the full URLA end-to-end. Continuing to the final phase P6 (`#k9p5` — admin review of the full application + gated TMO writeback). Prod stays at P1–P3 until the queued re-deploy.
<!-- agent:boundary:cdd71071 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" auto -->
- ~[#0ag4]~
- ~[#f3b2]~
- ~[#2eaf]. deploy to production~
- ~[#1tk7]~
- ~[#az2w]~
- [#k9p5]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#zr5z] [recommended] Re-deploy equityfundingsource to production after P4–P6 land so the full apply4 wizard (incl. final submit) is live; prod is currently at P1–P3 (worker 85fa9cfc). Run cf:d1:migrate:remote then deploy:worker, smoke-test.
- [ ] [#k9p5] apply4 full application P6 — admin review of the full application + TMO writeback mapping (gated on TMO custom-field key map: dzmm/k2xg/7a4d). Plan: src/equityfundingsource-dev/prds/apply4-full-application-build-plan.md
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
