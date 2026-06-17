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
queue: stop
---

## Status

<!-- agent:status patch=replace -->

completed
<!-- /agent:status -->

## Exchahnge

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260617-063907.md`*

Compacted content:
- Archived 6 response topic(s): Rename investor routes to lender routes; Move borrower portal routes; Move broker portal routes; 3 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: email reply threading + deploy — gpt-5

> **Queue prompt:**
>
> The message emails should be from the configured site email address.


Implemented and deployed the secure-message email reply path for the EFS app.

What changed:
- Outbound secure-message notification emails can now use a session-specific `Reply-To` generated from `MESSAGE_EMAIL_INBOUND_REPLY_TO`, including plus-address forms like `messages+{session_id}@...`.
- `POST /api/email/inbound` now parses the originating session from an explicit `session_id` or the inbound recipient/headers, normalizes display-name `From` addresses, and appends the reply to the original `chat_sessions` thread.
- Inbound replies are still fail-closed: provider shared secret first, then the sender email must resolve to an active admin or the portal user who owns the thread. Admin replies append as EFS; borrower/broker/investor replies append as portal-user messages and notify the admin side.
- Admin message queries and unread counts now include broker portal users in the same chat stack, and the PRD/tests document the borrower/broker/admin reply-threading contract.

Commits / deploy:
- EFS implementation commit pushed: `f493336` (`feat(messages): thread email replies into chat (#xcvg)`).
- Parent submodule pointer committed locally: `19f488bb1`.
- Deployed via `make deploy`; Worker version `f2123f48-9ec9-4d19-a14f-a382c8bdf08f` is live at `https://equityfundingsource.brian-takita.workers.dev`.

Verification:
- `bun test tests/borrower-message-notify.test.ts tests/borrower-messages.test.ts` passed.
- `make check` passed: 763 tests, app build, worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` reports no migrations to apply.
- Live smoke checks: `/` returned `200`; `/api/email/inbound` returned expected `501` because inbound routing remains disabled until provider config is set.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows.

Config note: the deployed code is ready, but actual email reply ingestion remains intentionally disabled until inbound email routing is configured with `MESSAGE_EMAIL_INBOUND_ENABLED`, `MESSAGE_EMAIL_INBOUND_SECRET`, and `MESSAGE_EMAIL_INBOUND_REPLY_TO`.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> deploy


Deployed the current clean EFS worker commit again for the remaining `deploy` queue item.

Deployment:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- Remote D1 migrations reported `No migrations to apply`.
- Cloudflare uploaded/deployed `equityfundingsource` at `https://equityfundingsource.brian-takita.workers.dev`.
- Current Worker version: `a6386caf-17e0-4722-a36a-064ad5f7a1b6`.

Verification:
- `curl https://equityfundingsource.brian-takita.workers.dev/` returned `200`.
- `POST /api/email/inbound` returned the expected disabled-state `501`.
- `git -C src/equityfundingsource-dev status --short --branch` is clean on `main...origin/main`.

Changed paths: no EFS source changes; only this agent-doc closeout is being committed. Escalation: none.

### Re: lender available deal website messaging + deploy — gpt-5

> **Queue prompt:**
>
> The lender "Available deal conversation" component on all pages it appears on should support the lender to send messages from the website in addition to email. deploy

Implemented, pushed, and deployed the lender website message path for the Available deal conversation.

What changed:
- Added `POST /api/investor/deals/:id/lender-messages` for authenticated lender/investor users.
- The endpoint reuses or creates the lender's `investor_deal_interests` row, writes a `deal_interest_lender_messages` row with `sender_role: 'lender'`, and emits the existing admin realtime event so the admin interested-lender workflow refreshes.
- Updated the lender deal detail `Available deal conversation` component with a website message form. This covers the standalone `/lenders/deals/:id` page and the dashboard embedded deal-detail dialog, because both render `InvestorDealDetail`.
- Updated the lending portal PRD and test guards for the lender/EFS conversation data model and acceptance criteria.
- While rebasing onto the latest remote EFS commits, fixed two stale string guards for the new shared `PortalModal` ownership of dialog role/height.

Commits / push:
- EFS submodule commit pushed: `f6230aa` (`feat(investor): let lenders message EFS from deal pages`).
- Parent repo submodule pointer pushed: `432b46ee3` (`chore: update EFS lender messaging submodule`).

Deploy:
- Ran `make deploy` from `src/equityfundingsource-dev`.
- Remote D1 migrations reported `No migrations to apply`.
- Deployed Worker: `https://equityfundingsource.brian-takita.workers.dev`.
- Current Worker version: `b576a4f3-dab4-4720-a123-034b8ba47804`.

Verification:
- Focused tests passed: `bun test tests/investor-portal-e2e.test.ts`, plus the two previously failing post-rebase guards.
- Full local verification passed after rebase: `make check` with 767 tests, app build, and worker build.
- Live smoke: `/` returned `200`; unauthenticated `POST /api/investor/deals/smoke/lender-messages` returned expected `401`.
- GitHub Actions check: `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows.

Changed paths:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/investor/deal_marketplace/index.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/investor/routes.ts`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/investor/deal_detail.tsx`
- `src/equityfundingsource-dev/prds/equity-funding-source-lending-portal-prd.md`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/investor-portal-e2e.test.ts`
- parent submodule pointer `src/equityfundingsource-dev`

Preflight warnings were unrelated semantic-completion candidates for existing review items `#dropbox-sync-adapter`, `#dzmm-writeback-replay`, and `#zwws-vestings-local`; I did not change those gates. Confidence: high. Escalation: none.
<!-- agent:boundary:81dd763f -->
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
