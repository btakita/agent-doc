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

EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `734df89a-8af6-401e-83e4-fd5215a48e3f` as of 2026-05-28 from EFS commit `0afd62b`; public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260605-081737.md`*

Compacted content:
- Archived 5 response topic(s): [#zwws] Investor dashboard #rpzm phase 5; Can we ungate phases 4 & 6 with the TMO cache?; [#zwws-local-first] Phase 4 EFS-local; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: [#67v2a] Admin loan pipeline Kanban — Phase A (read-only) — opus-4-8

> **Queue prompt:**
>
> [#67v2a]


Shipped Phase A of the TMO **All Loans** Kanban as a read-only admin **Pipeline** section, then `update spec + tests` and `commit + push` per `#spec-test-commit-push`. Committed to `equityfundingsource-dev` `main` (`bb98441..1ee9619`).

**What landed**
- **Pure board logic** in `app/equityfundingsource/lib/admin/repo.ts` (no D1, unit-tested): `PIPELINE_COLUMNS`, `pipeline_column_id_for_status_`, `pipeline_card_title_`, `pipeline_columns_group_`.
- **Read-only endpoint** `GET /api/admin/pipeline` (`admin_pipeline_GET_`, registered behind `require_admin_`). Hydrates cards from `admin_loans_R_` + `tmo_cache_rows_by_source_R_`; TMO wins for reads via `tmo_field_` (LoanAmount, EscrowNumber, PropertyStreet, ShortName, NoteRate). No mutation.
- **UI** `lib/ui--browser--efs/portal/admin/pipeline.tsx` (`AdminPipeline`) — horizontal lanes with per-column counts; cards show title (`{street} - {holding entity}`), loan #, amount, note rate, escrow, property street; each links to the loan detail. Wired into `index.tsx`, sidebar `layout.tsx`, and the server section route.

**Status → column mapping**
- `Application Received | Processing | Documents | Underwriting → Processing/Active`
- `Funded`, `Closed`, `Unassigned`, `Denied` map 1:1 to their columns
- `On Hold` column is reserved (no EFS status maps to it yet)
- blank/missing → Unassigned; unknown non-empty → Processing/Active (default)

**Verification** — full suite green: `bun test` 306 pass / 0 fail (14 new in `tests/admin-pipeline.test.ts`); `bun run build` + `build:worker` both bundle clean (worker 536kb).

**Still open** (PRD Qs 1–2, needs Robert/April): the exact TMO label for the leftmost active column and the full ordered column list. The current set is a faithful model of the captured screenshots but not yet confirmed. Phase B (#67v2b drag-to-change + outbox writeback) stays gated on the TMO write field map (#7a4d) and sync direction (#dzmm/#k2xg).

Next in the queue: `#zwws-phase6-dashboard`.

### Re: [#zwws-phase6-dashboard] Workflow-priority investor dashboard — opus-4-8

Shipped the **un-gated half** of #zwws phase 6, then `update spec + tests` and `commit + push` per `#spec-test-commit-push`. Committed to `equityfundingsource-dev` `main` (`1ee9619..31a9db5`). The new-deal *email* stays the gated half (#f4rq) and was out of scope for this item.

**What landed**
- **Pure derivation** in `lib/ui--browser--efs/portal/investor/data.ts`: `investor_action_items_(pipeline)` maps action-required pipeline stages → prioritized items (`Funding → wire` "Wire funds" rank 0; `Documents → docs` "Missing docs" rank 1; Reviewing/Committed/Active surface nothing), sorted most-urgent-first; `investor_has_urgent_()`. Unit-tested, no D1.
- **Dashboard reorder** (`dashboard.tsx`): new `DashboardModules` — when any item is urgent it renders an **Action required** panel (red count badge, per-item urgency chip + concrete next step + Resolve link) above the deal pipeline and available deals; when calm, **Available Deals lead** with the pipeline following. In-app indicator only.
- Derives from the same `INVESTOR_PIPELINE` the stage map uses, so it tracks the TMO single source of truth once the pipeline syncs from TMO in a later phase.

**Verification** — full suite green: `bun test` 310 pass / 0 fail (+4 new in `tests/investor-portal-e2e.test.ts`); `bun run build` + `build:worker` both bundle clean.

**Queue drained.** Both queued items are done: #67v2a (admin pipeline Kanban Phase A) and #zwws-phase6-dashboard. Remaining Phase 6 work is only the new-deal email, gated on the EFS Resend sending domain (#f4rq).
<!-- agent:boundary:fa5ef972 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
