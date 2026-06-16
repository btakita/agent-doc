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

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260616-231128.md`*

Compacted content:
- Archived 1 response topic(s): Remove "Loan Terms" from Available deal admin page & dialog. Deploy
- Prior summary/context: prior compacted content: Archived 1 response topic(s): inactive queued Interested Lenders ordering request

### Re: Uploaded Documents order — gpt-5

> **Queue prompt:**
>
> Move "Uploaded Documents" above "Interested Lenders" in the Available Deal admin detail page and dialog.


Implemented and pushed the Available Deal admin detail ordering update.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/loan_detail.tsx`
- `src/equityfundingsource-dev/prds/67v2-admin-loan-pipeline-kanban.md`
- `src/equityfundingsource-dev/tests/document-viewer.test.ts`
- `src/equityfundingsource-dev/tests/tmo-loan-details-density.test.ts`
- `src/equityfundingsource-dev` submodule pointer in the parent repo

What changed:
- Moved the `Uploaded Documents` section above `Interested Lenders` in `AdminLoanDetail`, which covers the standalone admin loan detail page and the Available Deals dialog surface that embeds the same component.
- Kept the document viewer wiring intact through a small `AdminLoanUploadedDocumentsSection` component.
- Updated the PRD and tests to assert `Uploaded Documents` appears before `Interested Lenders`.

Verification:
- `bun test tests/tmo-loan-details-density.test.ts tests/document-viewer.test.ts` passed: 10 tests.
- `make check` passed: 750 Bun tests, app build, and worker build.
- `git diff --check` passed.
- `gh run list --workflow CI --limit 1` could not check CI because this repo has no workflow named `CI`; `gh workflow list` returned no workflows.

Commits/push:
- Submodule pushed: `3340f6d fix(admin): reorder available deal documents (#67v2)`.
- Parent pushed: `39840abf3 chore(efs): advance available deal admin UI (#67v2)`.

Confidence: high. No escalation needed. The parent repo still has unrelated pre-existing dirty files, but the EFS submodule is clean and synced with `origin/main`.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> deploy


Deployed the current EFS Worker build from `src/equityfundingsource-dev` commit `3340f6d fix(admin): reorder available deal documents (#67v2)`.

Changed paths:
- None in this cycle; deploy-only run. The EFS submodule was already clean and synced with `origin/main`.

Commands / verification:
- `git pull --rebase origin main` in `src/equityfundingsource-dev`: already up to date.
- `make check`: passed — 750 Bun tests, app build, and worker build.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`: no migrations to apply before deploy.
- `make deploy`: passed — build, remote D1 migration step, and Worker deploy completed.
- Deployed Worker version: `e150c1ed-4618-4e16-abbd-c7d051f753f4`.
- Post-deploy `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`: no migrations to apply.
- Smoke test `https://equityfundingsource.brian-takita.workers.dev/`: `200`.
- Smoke test `https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline`: `303`, expected Cloudflare Access redirect for admin route.

Confidence: high. No escalation needed. The parent repo still has unrelated pre-existing dirty files and local agent-doc commits ahead of origin; I left those untouched.

### Re: Remove "Interested Lenders" from "Available Deal" -> "Loan Detail" tab. deploy — gpt-5

> **Queue prompt:**
>
> Remove "Interested Lenders" from "Available Deal" -> "Loan Detail" tab. deploy


Implemented, committed, pushed, and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/loan_detail.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/prds/67v2-admin-loan-pipeline-kanban.md`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/tmo-loan-details-density.test.ts`
- `src/equityfundingsource-dev` submodule pointer in the parent repo

What changed:
- Added a default-on `show_interested_lenders` option to `AdminLoanDetail`; when disabled it skips the interested-lenders fetch and hides the inline Interested Lenders card.
- Passed `show_interested_lenders={false}` from the Available Deal detail page's `Loan Detail` tab, leaving the dedicated `Interested Lenders` tab intact.
- Updated PRD/test coverage for the new Available Deal Loan Detail contract.

Verification:
- `bun test tests/admin-pipeline.test.ts tests/admin-investor-pipeline.test.ts tests/tmo-loan-details-density.test.ts tests/investor-portal-e2e.test.ts` passed: 165 tests.
- `make check` passed: 750 tests, app build, and Worker build.
- `git diff --check` passed.
- `gh run list --workflow CI --limit 1` could not check CI because this repo has no workflow named `CI`; `gh workflow list` returned no workflows.

Deploy:
- Pre/post deploy remote D1 migration check: no migrations to apply.
- `make deploy` passed.
- Deployed Worker version: `bf2394b5-12a8-4c24-a668-700159f90a24`.
- Smoke test `https://equityfundingsource.brian-takita.workers.dev/`: `200`.
- Smoke test `https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline`: `303`, expected Cloudflare Access redirect for admin route.

Commits/push:
- Submodule pushed: `27163f2 fix(admin): hide duplicate interested lenders (#67v2)`.
- Parent pushed: `f85e7a249 chore(efs): advance available deal loan detail (#67v2)`.

Confidence: high. No escalation needed. Preflight warnings were unrelated semantic-completion candidates in review items, so I left them untouched. The EFS submodule is clean; the parent repo still has unrelated pre-existing dirty files outside this task.

Suggested commit message:

```txt
fix(admin): hide duplicate interested lenders (#67v2)

Suppress the inline Interested Lenders section in the Available Deal Loan Detail tab while keeping the dedicated Interested Lenders tab and reusable loan-detail section intact. Update PRD/test coverage and deploy the Worker with no D1 migrations pending.
```
<!-- agent:boundary:da2ad36d -->
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
