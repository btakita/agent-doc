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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-055024.md`*

Compacted content:
- Archived 6 response topic(s): Investor dashboard annual-requirements alert; Investor dashboard requirements dialog stack; Queue stall logged as #agent-doc-bug + state repaired; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Annual requirements edit submission — gpt-5

> **Queue prompt:**
>
> The "Annual requirements" edit page + dialog "Edit submission" button should open a dialog to edit the existing submission under review. deploy


Implemented, pushed, and deployed.

Changed paths:
- `app/equityfundingsource/lib/investor/accreditation/index.ts`
- `app/equityfundingsource/lib/investor/documents_repo.ts`
- `app/equityfundingsource/lib/investor/routes.ts`
- `app/equityfundingsource/lib/investor/shared.ts`
- `lib/ui--browser--efs/portal/investor/accreditation.tsx`
- `lib/ui--browser--efs/portal/investor/dashboard.tsx`
- `tests/investor-documents.test.ts`
- `tests/investor-portal-e2e.test.ts`

What changed:
- The Annual requirements page button now opens a modal dialog instead of an inline blank form.
- The dashboard Requirements dialog `Edit submission` button now opens the same shared edit modal.
- The edit modal receives the existing accreditation submission, preloads verification type and notes, and keeps replacement evidence uploads on the existing upload flow.
- Added `PATCH /api/investor/accreditation` for metadata-only edits to the latest pending submission that already has evidence, so investors can update type/notes while EFS is reviewing without uploading another file.

Commit:
- `81b06ca fix(investor): edit under-review requirements submissions (#req-review)` pushed to `origin/main` in `src/equityfundingsource-dev`.

Verification:
- `bun test tests/investor-documents.test.ts tests/investor-portal-e2e.test.ts` passed.
- `make check` passed: 706 tests, app build, and worker build all completed with exit 0.
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` reported no remote migrations to apply before and after deploy.
- `make deploy` completed and deployed Worker version `5601f050-a8cf-4b26-8b16-d396e37ff943`.
- Live smoke checks: `/` returned `200`; `/portal-investor` returned `303` auth redirect.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Escalation: none.
Confidence: high.

### Re: Edit submission should show current data — gpt-5

> **Queue prompt:**
>
> "Edit your submission" dialog/page does not show the current submission data. It is a new submission form. Where are the uploaded documents and the filled out data? deploy


Implemented, pushed, and deployed.

You were right: the previous edit dialog still looked like a fresh upload form. The only current data it prefilled was verification type + notes, and the API did not return document metadata for the edit UI to show. There is not a larger stored questionnaire table behind this screen right now; the persisted annual-requirements data is status/dates, verification type, notes, and uploaded accreditation evidence documents. This change now surfaces that persisted data directly.

Changed paths:
- `app/equityfundingsource/lib/investor/accreditation/index.ts`
- `lib/ui--browser--efs/portal/investor/accreditation.tsx`
- `tests/investor-documents.test.ts`
- `tests/investor-portal-e2e.test.ts`

What changed:
- `GET/PATCH/POST /api/investor/accreditation` now return `evidence_documents` with file name, display name, extension, size, content type, published date, secure investor download URL, and a `current` flag for the active evidence document on the latest review row.
- The Requirements page now shows a `Current submission` panel when there is evidence: verification type, submitted notes, and uploaded document cards.
- The `Edit your submission` dialog shows that same `Current submission` panel above the editable fields, so the investor sees the existing submission before changing type/notes or uploading replacement/additional evidence.
- Uploaded documents are view/download links through the existing authenticated `/api/investor/documents/:id/download` route, with an `Active evidence` badge on the document tied to the latest under-review row.

Commit:
- `b0bbc3a fix(investor): show current requirements submission data (#req-review)` pushed to `origin/main` in `src/equityfundingsource-dev`.

Verification:
- `bun test tests/investor-documents.test.ts tests/investor-portal-e2e.test.ts` passed.
- `make check` passed: 706 tests, app build, and worker build all completed with exit 0. The tsift digest still labels an expected test fixture log line (`d1 down`) as a failure, but the command exit code and Bun summary are green (`706 pass`, `0 fail`).
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` reported no remote migrations pending.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deployment:
- `make deploy` completed successfully.
- Worker version: `861ca702-d667-4e71-8990-e1b785323ac9`.
- Live smoke checks: `/` returned `200`; `/portal-investor` returned `303` auth redirect.

Escalation: none.
Confidence: high.
<!-- agent:boundary:192e264f -->
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
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend — D1-backed submit-deal, pipeline, commissions, documents, and notifications shipped (#37ah-backend, 2026-06-13). Remaining: TMO writeback sync, gated on #dzmm (TMO write field map #7a4d / six-question support email #k2xg).
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: after Dropbox access is available, push/pull R2_DOCUMENTS with idempotent retries, backfill, tombstone handling, and per-document sync logs.
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
