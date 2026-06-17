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
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
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

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260617-013507.md`*

Compacted content:
- Archived 11 response topic(s): Uploaded Documents order; deploy; Remove "Interested Lenders" from "Available Deal" -> "Loan Detail" tab. deploy; 8 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Remove "Loan Terms" from Available deal admin page & dialog. D...

### Re: Mark Call Completed — gpt-5

> **Queue prompt:**
>
> Mark Call Completed


Completed and pushed the `Mark Call Completed` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `5e68d77` removes the `call_completed_at` admin/investor API contract, UI controls, stage/timeline logic, tests, and docs/spec wording; adds migration `0074_drop_deal_interest_call_completed.sql` to drop the column and recreate the workflow index without it. Parent repo commit `3741d3378` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `make check`; `gh run list --workflow CI --limit 1`.

Verification: focused tests passed (`108 pass`); full check passed (`750 pass`, build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Start Underwriting — gpt-5

> **Queue prompt:**
>
> Start Underwriting

Completed and pushed the `Start Underwriting` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `447538a` removes `underwriting_started_at` from the deal-interest workflow stage model, admin API serialization/update SQL, Interested Lenders UI controls, lender pipeline modal, loan-interest row typing, PRD wording, and tests; adds migration `0075_drop_deal_interest_underwriting_started.sql` to drop the column. Parent repo commit `c69fdbd2a` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`109 pass`); full check passed (`751 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Call scheduled — gpt-5

> **Queue prompt:**
>
> Call scheduled

Completed and pushed the `Call scheduled` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `fbe9965` removes `call_scheduled_at` from the deal-interest workflow stage model, admin/investor API row contracts, admin PATCH SQL, Interested Lenders UI controls, lender pipeline modal, investor activity timeline events, PRD wording, and tests; adds migration `0076_drop_deal_interest_call_scheduled.sql` to drop the column and recreate the workflow index without it. Parent repo commit `19078cc2f` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`110 pass`); full check passed (`752 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Verbal amount — gpt-5

> **Queue prompt:**
>
> Verbal amount

Completed and pushed the `Verbal amount` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `7a6e2ed` removes `verbal_commitment_amount` from the deal-interest admin/investor API contracts, admin update SQL, Interested Lenders UI controls and display chips, investor timeline labels, specs, and tests; adds migration `0077_drop_deal_interest_verbal_amount.sql` to drop the column. Parent repo commit `7a0da0171` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`111 pass`); full check passed (`753 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Contact method — gpt-5

> **Queue prompt:**
>
> Contact method

Completed and pushed the `Contact method` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `f7e71ea` removes `lender_last_contact_method` from deal-interest admin API contracts, admin update SQL, workflow summaries, Interested Lenders UI dropdowns, and tests; adds migration `0078_drop_deal_interest_contact_method.sql` to drop the column. Last-contact timestamp, summary, and next-follow-up tracking remain. Parent repo commit `acbb89b6e` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`112 pass`); full check passed (`754 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Pre-doc conditions — gpt-5

> **Queue prompt:**
>
> Pre-doc conditions

Completed and pushed the `Pre-doc conditions` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `d660669` removes `pre_doc_conditions` from deal-interest admin/investor API contracts, admin update SQL, workflow summaries, Interested Lenders UI controls, specs/docs, and tests; adds migration `0079_drop_deal_interest_pre_doc_conditions.sql` to drop the column. `Pre-doc instructions` remains for the next queued item. Parent repo commit `e9142e929` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`113 pass`); full check passed (`755 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Pre-doc conditions — gpt-5

> **Queue prompt:**
>
> Pre-doc instructions


> **Queue prompt:**
>
> Pre-doc conditions

Completed and pushed the `Pre-doc conditions` removal for the Available Deal -> Interested Lenders workflow.

Changed paths: `src/equityfundingsource-dev` commit `d660669` removes `pre_doc_conditions` from deal-interest admin/investor API contracts, admin update SQL, workflow summaries, Interested Lenders UI controls, specs/docs, and tests; adds migration `0079_drop_deal_interest_pre_doc_conditions.sql` to drop the column. Parent repo commit `e9142e929` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`113 pass`); full check passed (`755 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.

### Re: Lender contact summary — gpt-5

> **Queue prompt:**
>
> Lender contact summary

Completed and pushed the final Interested Lenders workflow field cleanup.

Changed paths: `src/equityfundingsource-dev` commit `efa40c0` removes `pre_doc_instructions` and `lender_last_contact_summary` from deal-interest admin/investor API contracts, admin update SQL, workflow summaries, Interested Lenders UI controls, specs/docs, and tests; adds migrations `0080_drop_deal_interest_pre_doc_instructions.sql` and `0081_drop_deal_interest_contact_summary.sql` to drop the columns. Last-contact timestamp and next-follow-up date remain. Parent repo commit `60f9967cf` updates the EFS submodule pointer.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`115 pass`); full check passed (`757 pass`, app build, worker build). CI check could not be inspected in the EFS repo because GitHub reported no workflow named `CI`.

Confidence: high. Escalation: none.
Deployed the EFS Worker to Cloudflare and applied the pending remote D1 migrations.

> **Queue prompt:**
>
> deploy...The fields are still there on https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=interests


Deployment:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- Applied migrations `0074` through `0081`, including the deal-interest column drops from the recent cleanup.
- Published Worker version `f70f4788-70c7-4839-b6d6-17cb0009e2d0` at `https://equityfundingsource.brian-takita.workers.dev`.

Verification:
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` now reports `No migrations to apply!`.
- The deployed browser asset `index.browser-3G2BPU4E.js` returns HTTP 200 and searching it for the removed labels/field keys returns no matches.
- The live admin pipeline URL returns HTTP 303 from an unauthenticated shell, which is the expected auth redirect for that route.
- Remote D1 schema for `investor_deal_interests` no longer includes the removed columns; it retains the intended date/workflow/GHL fields.

I could not perform an authenticated browser check of the admin dialog from this shell, but the deployed bundle and production schema both reflect the removal.

### Re: Request Document(s) — gpt-5

> **Queue prompt:**
>
> Request Document(s)


Completed and pushed the first Borrower communication UI pass for `Request Document(s)` in the Available Deal workflow surfaces.

Changed paths: `src/equityfundingsource-dev` commit `5e62b13` removes the visible GoHighLevel adapter controls from the deal detail Interested Lenders workflow and the Lender Pipeline deal-interest dialog, renames the visible borrower request block to `Borrower communication`, adds a `Communication type` dropdown with `Request Document(s)`, keeps the existing document-request API/composer, and updates source-inspection tests. Parent repo commit `58c97dd38` updates the EFS submodule pointer. Both commits were pushed to `origin/main`.

Commands: `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts`; `git diff --check`; `make check`; `gh run list --workflow CI --limit 1`; `git push origin main` in `src/equityfundingsource-dev`; `git push origin main` in the parent repo.

Verification: focused tests passed (`115 pass`); full `make check` passed (`757 pass`, app build, worker build). GitHub Actions CI could not be inspected because the EFS repo has no workflow named `CI`.

Confidence: high. Escalation: none.
<!-- agent:boundary:b52911ee -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
In Available deal dialog + page:

Hide "GoHighLevel adapter" section.
Move "Borrower document requests" under "Lender communication".

Rename "Borrower document requests" to "Borrower communication". Add a dropdown containing:
- ~~Request Document(s)~~
- Message (General message with EFS letterhead with the Application info)

The upload from the Request Document(s) email should send an email of receipt to the admin email. The upload should also be visible in the Borrower communication after the message sent by the admin. The message sent by the admin should remain visible after the borrower uploads the document(s).

"Lender communication" should be an email message system like "Borrower communication"...on EFS letterhead with the Application info.

Email responses by lender or borrower should be emailed to the admin and be visible in the Available deal dialog + page.

deploy
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
