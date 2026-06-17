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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260617-040237.md`*

Compacted content:
- Archived 12 response topic(s): Mark Call Completed; Start Underwriting; Call scheduled; 9 more
- Prior summary/context: prior compacted content: Archived 11 response topic(s): Uploaded Documents order; deploy; Remove "Interested Lenders"...

### Re: Session recovery — gpt-5

Recovered the session through the required `agent-doc finalize` closeout path so the visible document content is committed through the binary-owned workflow. The borrower secure-message email round-trip remains tracked as `#xcvg` / `#xcvg-prod-email`, and the active queue remains ready to start with the available-deals borrower-communication label/document attachment work.

### Re: Remove borrower communication type label — gpt-5

> **Queue prompt:**
>
> Available deals page/dialog: "Borrower communication" "Communication type" label should be removed.


Implemented and pushed the first queue item. The visible `Communication type` label is removed from the Borrower communication selector in both admin deal-detail surfaces, while the select keeps an accessible `aria-label="Borrower message mode"`.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/investor-portal-e2e.test.ts`

Commands / verification:
- `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts` passed.
- `bun run check` passed: 758 tests, 0 fail, browser build done, worker build done. The digest reported the expected logged `Error('d1 down')` from the non-fatal outbox failure test, but the command exited 0 and the test summary was 0 fail.
- `gh run list --workflow CI --limit 1` found no workflow named `CI`; `gh workflow list` returned no workflows for this repo.

Commits / push:
- EFS repo: `27c6b2d` (`Remove borrower communication type label`) pushed to `origin/main`.
- Parent repo: `c635974dd` (`Update EFS borrower communication label`) pushed to `origin/main`.

Escalation: none. Confidence: high.

Borrower portal messages note: the admin-side borrower communication UI exists; production email notification, inbound email reply-to-chat, and borrower email-on-admin-reply remain tracked under `#xcvg` / `#xcvg-prod-email` and are still gated on the decided production email/GHL path.

### Re: Show uploaded request documents — gpt-5

> **Queue prompt:**
>
> Available deals page/dialog: If the borrower uploaded a file for a Request Document(s) email, the acutal document should appear 


Implemented and pushed the borrower-uploaded document display for Request Document(s) borrower communication. I treated the wrapped queue line (`next to the Borroewr communication message.`) as part of this same item: when a request has a borrower-provided upload, the admin Borrower communication request row now shows an `Uploaded document:` link next to the request text in both admin deal-detail surfaces.

Changed paths:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/deal-interest-document-requests.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/repo.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/investor_management/index.ts`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/investor-portal-e2e.test.ts`

What changed:
- `provided_document_id` is now enriched in admin workflow payloads with the existing provided-document metadata and download URL.
- The deal detail and lender pipeline borrower communication request rows render a `DocumentFileLink` for the uploaded file.
- Tests cover metadata preservation, admin payload wiring, and both UI source surfaces.

Commands / verification:
- `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts` passed.
- `bun run check` passed: 759 tests, 0 fail, browser build done, worker build done. The digest again reported the expected logged `Error('d1 down')` from the non-fatal outbox failure test, but the command exited 0 and the test summary was 0 fail.
- `git diff --check` passed.
- `gh run list --workflow CI --limit 1` found no workflow named `CI`; `gh workflow list` returned no workflows for this repo.

Commits / push:
- EFS repo: `ee787ee` (`Show uploaded borrower request documents`) pushed to `origin/main`.
- Parent repo: `df7742b0d` (`Update EFS borrower request documents`) pushed to `origin/main`.
- Parent repo: `a73347b80` (`Update EFS lender communication messages`)
- EFS: `4e1d612` (`Replace lender contact fields with messages`)
- GitHub Actions check: `gh run list --workflow CI --limit 1` reports no workflow named CI, and `gh workflow list` returns no workflows.
- `git diff --check` passed.
- `bun run check` exited 0: 759 pass, 0 fail, builds completed. The digest runner still labels the known logged `error: d1 down` outbox-path message as a failure group, but the command exit code and test tail are clean.
- `bun test tests/admin-investor-pipeline.test.ts tests/investor-portal-e2e.test.ts` passed.
- Updated tests to assert the new message table/API/UI behavior and the removal of the old timestamp fields.
- Replaced the Lender communication date fields in both admin UIs with a message list and `Message` composer, matching the borrower communication pattern.
- Added `POST /api/admin/deal-interests/:id/lender-messages` and threaded `lender_messages` through workflow summaries, deal-interest admin responses, the lender pipeline, and deal detail.
- Added `deal_interest_lender_messages` with migration `0082_deal_interest_lender_messages.sql`; it drops `lender_last_contact_at`, `lender_next_follow_up_at`, and the old follow-up index.

Escalation: none. Confidence: high.

### Re: Queue cleanup — gpt-5

Removed the dangling wrapped queue line `next to the Borroewr communication message.` after completing it as part of the uploaded-request-document item. No additional code changes were needed in this repair cycle.

Changed paths: `tasks/professional/equityfundingsource.md` only.

Commands / verification: `agent-doc preflight`, `agent-doc plan`, and `agent-doc finalize` closeout. Confidence: high. Escalation: none.
<!-- agent:boundary:507d6fc6 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~Available deals page/dialog: "Borrower communication" "Communication type" label should be removed.~~
- ~~Available deals page/dialog: If the borrower uploaded a file for a Request Document(s) email, the acutal document should appear~~
- :pushpin: Lender communication should have a "Message" flow...just like the browser communication message flow. Remove "Last contact" and "Next follow-up" fields, db columns, and logic.
- :pushpin: deploy
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
