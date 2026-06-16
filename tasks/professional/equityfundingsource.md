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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260616-064243.md`*

Compacted content:
- Archived 1 response topic(s): copy-button layout + deploy
- Prior summary/context: prior compacted content: Archived 8 response topic(s): compact card labels + deploy; Borrower Info inline values + ge...

### Re: lender selection pipeline — gpt-5

> **Queue prompt:**
>
> :pushpin: Continue setting up the lender selection pipeline. Where lenders are sent information, communication with the lender, etc.


Continued the lender selection pipeline setup by adding structured lender communication follow-up fields to the existing deal-interest workflow. The Lender Pipeline modal now records last lender contact time, method, contact summary, and next follow-up date alongside the package/call/selection/verbal-commitment milestones.

Changed paths: `src/equityfundingsource-dev/app/equityfundingsource/migrations/0070_deal_interest_lender_communication.sql`, admin investor workflow API/types, `lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`, `prds/investor-pipeline-admin.md`, and `tests/admin-investor-pipeline.test.ts`.

Verification: `bun test tests/admin-investor-pipeline.test.ts` passed (61 tests); `make check` passed (741 tests, app build, Worker build); `git diff --check` passed. GitHub Actions note: neither the EFS submodule nor the parent repo has a workflow named `CI`; the parent repo's available `Agent Doc` workflow was queued as run `27599285422` after the push.

Committed and pushed: EFS submodule `6ba172b` (`feat(admin): track lender communication follow-ups (#deal-interest-communication)`) to `btakita/equityfundingsource-dev:main`; parent repo pointer `7f62390ad` (`chore(efs): advance lender communication pipeline`) to `btakita/agent-loop:main`.

Escalation: none. Confidence: high. No deploy was run; this queue item used the `#spec-test-commit-push` path, and the new D1 migration will be applied by the normal deploy path. Exchange is large, but the active queue was drainable, so I did not pause for compaction.

### Re: Deal Pipeline + GoHighLevel — gpt-5

> **Queue prompt:**
>
> I'm talking about the Deal Pipeline. https://equityfundingsource.brian-takita.workers.dev/portal-admin/loans/45ddae53-3e7f-43b1-a627-17b79b06fa3f and the Available Deal dialog. The admin will communicate with lenders over email and select a lender. Would Go High Level integration be a good idea here?


Yes, GHL is a good fit here, but only as the communication/CRM layer, not as the source of truth for the Deal Pipeline.

For the Available Deal dialog, EFS should own the durable deal state: which lenders are interested, who was selected, package/document permissions, borrower document requests, verbal commitment, funding allocation, audit history, and TMO/R2/D1 links. That state is too EFS-specific and too tied to documents/security to move into GHL.

GHL is useful around that core workflow: send the lender package email, include a calendar link, create follow-up tasks, track replies/conversations, update a CRM opportunity/contact, and keep Robert/Judith's outreach automations in one familiar CRM. Official docs support the basic shape: HighLevel has APIs for contacts, messaging, workflows, calendars, webhooks, and more; the Conversations API has a `POST /conversations/messages` endpoint; workflows include email/SMS/call/manual-action steps and create/update opportunity actions. Sources checked: https://help.gohighlevel.com/support/solutions/articles/48001060529-highlevel-api-documentation, https://marketplace.gohighlevel.com/docs/ghl/conversations/send-a-new-message/, https://help.gohighlevel.com/support/solutions/articles/155000002294-what-are-workflow-actions-complete-list-.

Recommended architecture:

1. EFS remains canonical. The Available Deal dialog should show interested lenders and selection controls directly from D1 (`investor_deal_interests`, funds/allocations, documents).
2. GHL is an adapter. Store GHL contact/conversation/opportunity/message IDs on the EFS interest/deal rows, but never require GHL to render the Deal Pipeline.
3. Start narrow. First integration should be "Send package / start follow-up" from the Available Deal dialog: sync or create lender contact in GHL, send/trigger the email, store the GHL message/conversation ID, and log the result in EFS.
4. Pull replies back into EFS only as communication history. A reply can update last-contact/next-follow-up fields, but selecting a lender should remain an explicit admin action in EFS.
5. Avoid GHL as the pipeline board. Its opportunity pipeline can mirror a deal for CRM visibility, but EFS needs to model multiple interested lenders per deal, multi-lender allocations, private document access, and TMO writeback gates.

Caveats: use current HighLevel API docs and private integration/OAuth rather than legacy V1 API patterns; their docs say V1 reached end-of-support on December 31, 2025. Also confirm Robert's enterprise/sub-account access includes the API/workflow features needed, because HighLevel says API access varies by plan.

Changed paths: session document only. Verification: reviewed local EFS Deal Pipeline/Lender Pipeline notes and current official HighLevel API/workflow docs. Escalation: none. Confidence: medium-high.
<!-- agent:boundary:3851851f -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- do [#deal-pipeline-ghl-adapter]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#deal-pipeline-ghl-adapter] [recommended] Deal Pipeline GoHighLevel adapter spike: keep EFS/D1 as source of truth for Available Deal lender selection, document permissions, and audit; evaluate a narrow GHL integration for outbound package emails, calendar/follow-up automation, inbound reply capture, and CRM opportunity/contact visibility. Decide required GHL IDs to store on `investor_deal_interests` / deal rows and whether to trigger GHL workflows or send via Conversations API.
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
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document
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
- [ ] [#e1kb] Point equityfundingsource.com and www.equityfundingsource.com at the Cloudflare Pages project equityfundingsource, then verify / and /funded-deals on the custom domain; if admin routes are expected on the custom domain too, decide whether to route those paths to the Worker or move the dynamic app fully behind the same hostname.
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
