---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
resume: 019ed201-3acf-79e2-8b39-876609f3c6b3
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
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
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

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260616-212000.md`*

Compacted content:
- Archived 9 response topic(s): lender selection pipeline; Deal Pipeline + GoHighLevel; Deal Pipeline GHL Adapter; 6 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): copy-button layout + deploy

### Re: loan detail Interested Lenders + deploy — gpt-5

> **Queue prompt:**
>
> Add an Interested Lenders section to https://equityfundingsource.brian-takita.workers.dev/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f and the Deal Pipeline Available Deal dialog. deploy


Implemented and deployed the requested Interested Lenders section for the admin loan detail page and the Deal Pipeline dialog path.

What changed:
- Added `GET /api/admin/loans/:id/interests`, which resolves the loan's available-deal rows and returns the same admin interested-lender response shape used by the deal detail page.
- Added `AdminLoanInterestedLendersSection` to `AdminLoanDetail`, so `/portal-admin/loans/:id` now shows interested lenders inline with selected-lender summary, TMO account, workflow stage/next action, document request count, verbal amount, and a select action.
- Kept the Deal Pipeline dialog path covered because loan cards still embed `<AdminLoanDetail loan_id={loan_id} embedded />`; the new section appears there without a separate pipeline-only fork.
- The select action uses the existing `PUT /api/admin/deal-interests/:id/select` endpoint, so selection remains exclusive per deal and still queues the gated TMO selected-lender outbox payload.

Commits:
- EFS submodule: `4877ee0 feat(admin): show interested lenders on loan detail`, pushed to `btakita/equityfundingsource-dev main`.
- Parent pointer: `22cf9e4ad chore(efs): advance loan interested lenders UI`, pushed to `btakita/agent-loop main`.

Verification:
- `bun test tests/admin-investor-pipeline.test.ts` — 67 passed.
- EFS `make check` — 750 tests passed, app build passed, Worker build passed.
- Parent `make check` — passed.
- `git diff --check` and staged diff checks were clean.

Deploy:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- No remote D1 migrations were pending before or after deploy.
- Worker version: `943aa5ee-a72f-4e0f-97f1-e7f6d87c4a60`.
- Smoke checks: public root returned `200`; `/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f` and `/portal-admin/pipeline` returned `303`, matching Cloudflare Access redirect behavior.

CI note:
- EFS repo has no workflow named `CI` and no recent Actions runs listed.
- Parent repo has no workflow named `CI`; latest `Agent Doc` run for the pushed parent commit did not start jobs because GitHub reports account billing/spending-limit blockage, not a test failure: https://github.com/btakita/agent-loop/actions/runs/27649381906
- EFS repo has no workflow named `CI` and no recent Actions runs listed.
- Smoke checks: public root returned `200`; `/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f` and `/portal-admin/pipeline` returned `303`, matching Cloudflare Access redirect behavior.
- Worker version: `943aa5ee-a72f-4e0f-97f1-e7f6d87c4a60`.
- No remote D1 migrations were pending before or after deploy.
- Ran `make deploy` from `src/equityfundingsource-dev`.
- `git diff --check` and staged diff checks were clean.
- Parent `make check` — passed.
- EFS `make check` — 750 tests passed, app build passed, Worker build passed.
- `bun test tests/admin-investor-pipeline.test.ts` — 67 passed.
- Parent pointer: `22cf9e4ad chore(efs): advance loan interested lenders UI`, pushed to `btakita/agent-loop main`.
- EFS submodule: `4877ee0 feat(admin): show interested lenders on loan detail`, pushed to `btakita/equityfundingsource-dev main`.
- The select action uses the existing `PUT /api/admin/deal-interests/:id/select` endpoint, so selection remains exclusive per deal and still queues the gated TMO selected-lender outbox payload.
- Kept the Deal Pipeline dialog path covered because loan cards still embed `<AdminLoanDetail loan_id={loan_id} embedded />`; the new section appears there without a separate pipeline-only fork.
- Added `AdminLoanInterestedLendersSection` to `AdminLoanDetail`, so `/portal-admin/loans/:id` now shows interested lenders inline with selected-lender summary, TMO account, workflow stage/next action, document request count, verbal amount, and a select action.
- Added `GET /api/admin/loans/:id/interests`, which resolves the loan's available-deal rows and returns the same admin interested-lender response shape used by the deal detail page.

### Re: inactive queued EFS deal-interest request — gpt-5

I did not run the EFS repo work or deploy from this cycle. Preflight found the new EFS directive inside `agent:queue`, but the document frontmatter says `queue: stop` and the queue block has no active `go`, so agent-doc classified it as inactive queue residue rather than executable repo work.

I captured it as actionable backlog item `#efs-deals-interest-admin-sync` so it is not lost: rename the investor dashboard label to "Deals of interest", fix the lender-interest action so `btak.dev+investor@gmail.com` appears in admin Available Deal Interested lenders, update spec/tests, commit, push, and deploy.

Warnings surfaced:
- Preflight flagged `#dropbox-sync-adapter`, `#dzmm-writeback-replay`, and `#zwws-vestings-local` as possible stale review items because related done entries may already cover them. I left them unchanged.

Proof:
- Changed paths: session document/backlog only.
- Commands: `tsift status` in the parent repo, `tsift status` in the EFS submodule, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
- Verification: no application code changed; no tests or deploy run.
- Escalation: none.

### Re: inactive queued EFS deal-interest request — gpt-5

I did not run the EFS repo work or deploy from this cycle. Preflight found the new EFS directive inside `agent:queue`, but the document frontmatter says `queue: stop` and the queue block had no active `go`, so agent-doc treated it as inactive queue residue rather than executable repo work.

The requested work is preserved as backlog item `#efs-deals-interest-admin-sync`: rename the investor dashboard label to "Deals of interest", fix the lender-interest action so `btak.dev+investor@gmail.com` appears in admin Available Deal Interested lenders, update spec/tests, commit, push, and deploy.

Warnings surfaced:
- Preflight flagged `#dropbox-sync-adapter`, `#dzmm-writeback-replay`, and `#zwws-vestings-local` as possible stale review items because related done entries may already cover them. I left them unchanged.

Proof:
- Changed paths: session document/backlog only.
- Commands: `tsift status` in the parent repo, `tsift status` in the EFS submodule, `agent-doc preflight`, `agent-doc plan`, `agent-doc read`, `agent-doc repair --apply-recovery`.
- Verification: no application code changed; no tests or deploy run.
- Escalation: none.

### Re: dashboard Deals of interest + admin interested lender sync — gpt-5

> **Queue prompt:**
>
> [#efs-deals-interest-admin-sync]


Implemented, pushed, deployed, and backfilled the specific live interest row for `btak.dev+investor@gmail.com`.

What changed:
- Renamed the investor dashboard preview from `Pending deals` to `Deals of interest` for per-deal interest/funding-pending rows.
- Fixed the dashboard classification bug: a pending lender-account request no longer moves every browseable deal out of Available Deals before the investor can mark interest.
- Added a `Deal Interest` action to the shared investor deal-detail component, so the dashboard Available Deal dialog can POST/DELETE `/api/investor/deals/:id/interested` directly instead of telling the investor to use the list page.
- Threaded the dashboard dialog interest callback back into the dashboard preview state, including backfilling the Available Deals preview from the full fetched list after a card moves to Deals of interest.
- Updated `prds/rpzm-investor-dashboard-rework.md` and source-text guard tests for the new behavior.
- Inserted the missing live production `investor_deal_interests` row for loan `45ddae53-3e7f-43b1-a627-17b79b06fa3f`, deal `7a0c1463-27bb-4d7c-82f7-2281b3c008d3`, user `btak.dev+investor@gmail.com`; verified the admin-visible join returns that email. The user still has no TMO lender account, so the row appears with `lender_account = null` until EFS assigns one.

Commits:
- EFS submodule: `a12c3d9 fix(investor): persist dashboard deal interest (#efs-deals-interest-admin-sync)`, pushed to `btakita/equityfundingsource-dev main`.
- Parent pointer: `fba67ec6c chore(efs): advance dashboard interest fix`, pushed to `btakita/agent-loop main`.

Verification:
- `bun test tests/investor-portal-e2e.test.ts` — 41 passed.
- `bun test tests/investor-portal-e2e.test.ts tests/lender-account-gate.test.ts` — 50 passed.
- EFS `make check` — 750 tests passed, app build passed, Worker build passed.
- Parent `make check` — passed.
- `git diff --check` — clean.
- Remote D1 migrations before and after deploy — no migrations to apply.
- Live smoke: `/` returned `200`; `/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f` returned `303`, matching Cloudflare Access redirect behavior.

Deploy:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- Worker URL: `https://equityfundingsource.brian-takita.workers.dev`.
- Worker version: `0c87d79e-3b4a-4bae-a5eb-ad645320aa23`.

CI / warnings:
- EFS repo has no Actions runs listed and no workflow named `CI`.
- Parent repo has no workflow named `CI`; latest `Agent Doc` run for `fba67ec6c` was queued at closeout, so CI is pending rather than failed.
- Preflight semantic-completion warnings for `#dropbox-sync-adapter`, `#dzmm-writeback-replay`, and `#zwws-vestings-local` were not part of this queued item and were left unchanged.
- The session exchange is large, but the active queue was drainable, so I continued the queue item instead of stopping for compaction.

Proof:
- Changed paths: `src/equityfundingsource-dev/lib/ui--browser--efs/portal/investor/dashboard.tsx`, `src/equityfundingsource-dev/lib/ui--browser--efs/portal/investor/deal_detail.tsx`, `src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md`, `src/equityfundingsource-dev/tests/investor-portal-e2e.test.ts`, `src/equityfundingsource-dev/tests/lender-account-gate.test.ts`, parent submodule pointer `src/equityfundingsource-dev`, and the one production D1 interest row noted above.
- Commands: `tsift status`, `agent-doc preflight/plan/read`, targeted `rg`/`sed` inspection, remote D1 read/write queries, focused tests, EFS `make check`, migration list checks, `make deploy`, live `curl` smokes, EFS/parent commits and pushes, parent `make check`, `gh run list`.
- Verification: all local checks listed above passed; live data query confirms the named investor now joins to the admin loan's deal interest.
- Confidence: high.
- Escalation: none.
<!-- agent:boundary:f08e9007 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#efs-cutover-sso-plan] Workers.dev to custom-domain/SSO closeout: write and execute a cutover checklist for equityfundingsource.com/www routing, /login/access-callback, rollback, and Google Workspace SSO after #m6ch credentials arrive. Advances #next-steps.
- [/] [#zwws-vestings-local] [recommended] Investor dashboard phase 4 local half: capture/display vestings and special instructions in EFS D1/UI with tmo_cache_outbox enqueue; leave TMO push gated on #dzmm/#7a4d. Advances #zwws.
- [/] [#67v2-auto-advance] Admin pipeline auto-advance: define stage requirement checks from the TMO-aligned Kanban, run them against D1/tmo_cache, and enqueue audited status updates; keep replay gated by #dzmm. Advances #67v2.
- [/] [#jnzs-approval-followup] [recommended] Send/track funded-deals approval packet: attach current screenshots/copy/lien-position language plus the full_application ESIGN/UETA disclosure excerpt, record Judith/EFS reviewer decision, and convert requested edits into implementation backlog. Advances #jnzs.
- [/] [#xcvg-prod-email] [recommended] Borrower secure-message email production pass: after #f4rq/#m6ch/#p40t or #z3kx path-of-record is decided, enable borrower/admin outbound notifications, verify inbound reply threading, and add production env/runbook checks. Advances #xcvg.
- [/] [#dzmm-writeback-replay] EFS->TMO writeback replay: once #7a4d field map and #k2xg support answers arrive, replay tmo_cache_outbox with idempotency keys, conflict logging, retry/dead-letter state, and tests across broker/admin/investor writebacks. Advances #dzmm/#37ah/#67v2/#zwws.
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#ghl-stage-bridge] [recommended] Implement an event-driven GHL stage bridge from EFS Deal Pipeline state to HighLevel contacts/opportunities/workflows for Processing, Underwriting, Terms negotiation, and Closing while keeping EFS/D1 canonical for loan, lender-selection, borrower portal, document, audit, and funding state.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#m6ch] Get Google Suite admin credentials from Robert (SMTP + Google Calendar integration)
- [ ] [#p40t] [recommended] Google Calendar/G Suite notification integration after Google admin credentials are available
- [ ] [#q2x2] HTML email templates: lighter navy + gold, 2-3 drip for PBM orphan investor outreach
- [ ] [#x5rn] Convert EFS logo JPEG -> transparent PNG/SVG (Robert sent JPEG)
- [ ] [#kzt6] Collect testimonials for carousel (6-12 min: investors, brokers, borrowers) — push Judith + Robert helping
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
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
