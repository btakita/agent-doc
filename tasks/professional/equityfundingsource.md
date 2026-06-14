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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-233442.md`*

Compacted content:
- Archived 7 response topic(s): advance the review items; do [#tmopoll] + do [#xcvginbound]; Step 7 "Review & Sign" reverts to Step 6 on refresh; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
- *M. Declared bankruptcy within the past 7 years?*
- *D1. Applying for another mortgage on/before closing (not disclosed)?*
- *A. Will you occupy the property as your primary residence?*

### Re: Use proper label names on the Declarations summary — opus-4-8

> **Queue prompt:**
>
> Use proper label names ![img_12.png](img_12.png) Use the question presented to the user 


Fixed (EFS `95ec5db`). The application detail's read-only **"Step 5 - Declarations"** summary was labeling each row with just the raw key letter (A, B, C, D1, D2 …) instead of the actual declaration question. It now maps each declaration through `DECLARATION_FIELDS` so the full question text shows in canonical order — e.g. *"A. Will occupy as primary residence?"*, *"D1. Applying for another mortgage?"*, *"M. Bankruptcy in past 7 years?"* — matching what the **Edit** mode already displays. Any unknown key falls back to the raw key.

Verification: `bun run check` green (647 tests, incl. new `full-application-detail-declaration-labels.test.ts`), worker build clean.

Not deployed — this prompt didn't say "deploy", and a deploy is outward-facing. The build is ready; say the word (or add "deploy") and I'll push it live via `deploy:worker:no-migrate` (no schema change).

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed. Both pending declaration-label fixes are now live:

- `95ec5db` — Step 5 summary renders real declaration labels (not raw key letters A/B/C…).
- `57e4475` — those labels are now the **exact questions presented to the borrower**, via a shared `shared/declarations.ts` source of truth (form + admin detail can't drift).

`deploy:worker:no-migrate` (no schema change) → version `a0226791`, https://equityfundingsource.brian-takita.workers.dev.

Note: my prior-cycle write-back for the `57e4475` work was lost to the stale-supervisor IPC drift (pid 2127718, pre-`5ccf1354` build), so it isn't a separate `### Re:` section here — but the code shipped and is verified (648 tests green). Restarting that supervisor (`agent-doc session restart-supervisor` or restart the session) will stop the recurring drift.

### Re: "Drop or choose a replacement file" → "Drop file or click to select file" — opus-4-8

> **Queue prompt:**
>
> "Drop or choose a replacement file" should be "Drop file or click to select file".

Done (EFS `a8db25e`). The per-document-type upload zone showed **"Drop or choose a replacement file"** once a file already existed for that type. Reworded to **"Drop file or click to select file"** in both spots that render it:

- `lib/ui--browser--efs/portal/borrower/full_application.tsx` — borrower loan-application Documents step.
- `lib/ui--browser--efs/portal/shared/full_application_detail.tsx` — admin/shared application-detail Documents section.

The empty-state label ("Drop a file here or click to choose") was left as-is — your note was specific to the replacement-file wording.

Per `#spec-test-commit-push`: added a `multi-upload.test.ts` assertion that both files render the new label and no longer contain the old one. `bun run check` green (649 tests), worker build clean. Committed + pushed to `main`.

Not deployed — this preset is commit+push only; the `deploy` item later in the queue will push it live.
<!-- agent:boundary:1e759505 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~"Drop or choose a replacement file" should be "Drop file or click to select file".~~
- Can we make each uploaded file viewable by clicking on it? Can we have open a viewer of each supported file type?
- deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#c7cs] #docview-tables Extend the #doc-viewer click-to-view to the remaining download-only document surfaces (admin/documents.tsx, admin/lender_documents.tsx, admin/deal_documents.tsx, admin/document_request_detail.tsx, admin/upload_detail.tsx, investor/documents.tsx, investor/available_deals.tsx, broker portal, shared/table_actions.tsx): add a View action next to each Download button using DocumentFileLink/use_document_viewer_ from shared/ui.tsx. Add per-surface wiring tests.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked; migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#37ah] [recommended] Broker portal backend: replace static broker demo data with TMO-backed submit-deal, pipeline, commissions, documents, and notifications
- [/] [#dzmm] TMO webhooks assumed UNAVAILABLE per operator (2026-06-13) — TMO docs show no webhook/streaming API. Pursue the tightened-polling sync mitigation instead of push; the six-question TMO support email (#k2xg) is deferred (save for later).
- [/] [#qakx] Investor portal document pages: replace static Documents, Statements, and Accreditation fixture data with D1/R2-backed APIs and authenticated upload/request/download flows.
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
