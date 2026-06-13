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
queue: start
---

## Status

<!-- agent:status patch=replace -->

committed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-060756.md`*

Compacted content:
- Archived 5 response topic(s): "Open full application" should open a Full Application dialog with a page link; Add Decision / Feedback report / Sent feedback + Activity history to the Full Application dialog; Move Activity History to the right of Application Progress; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Where are the sample upload documents? — opus-4.8

> **Queue prompt:**
>
> Where are the sample documents that I can upload for the borrower applications and the lender acreditation?


They live in `src/equityfundingsource-dev/tmp/`, split by audience. They're synthetic, safe-for-testing fixtures (documented in `tmp/README.md`) using only extensions allowed by `app/equityfundingsource/lib/security/upload_guard.ts`. Note: `tmp/` is **gitignored** — these are local-only on this machine, not in the repo.

**Borrower application uploads** — `file:///home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/tmp/borrower/`
- `borrower-appraisal-report.pdf`
- `borrower-bank-statement.csv`
- `borrower-entity-authorization.txt`
- `borrower-payoff-statement.txt`

**Lender / investor accreditation uploads** — `file:///home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/tmp/investor/`
- `investor-accreditation-letter.pdf`
- `investor-bank-statement.csv`
- `investor-tax-k1-summary.csv`
- `investor-entity-formation.txt`
- `investor-wire-instructions.txt`

When you click upload in the borrower application document flow or the lender accreditation flow, browse to the matching folder above. They're tiny (under 1 KB each) and exist only to exercise the upload/validation paths, not to look like real documents.

If you want a richer or more realistic set (or want them checked into the repo / a seed script so they survive a fresh clone), say the word and I'll generate them.

### Re: Sample documents for all required uploads — opus-4.8

> **Queue prompt:**
>
> Please look at all required documents for the Deal Pipeline and Investor Pipeline. Please make sample documents in a valid format that I can use to upload to Loan Applications and Investor Applications and any other form that requires documents.


Done. I traced every required document type from the DB catalogs and generated a complete set of **valid, openable** sample files (verified with `file`, `pdftotext`, and ImageMagick `identify`).

**Required-document sources (source of truth):**
- **Loan Applications / Deal Pipeline** — `document_types` table (migrations 0010 + 0056): Appraisal Report, Preliminary Title Report, Property Photos, Purchase Agreement, Executed Promissory Note, Deed of Trust.
- **Investor accreditation** — `investor_accreditation_verifications.verification_type` (migration 0024): income, net_worth, institution, professional.
- **Investor documents** — `investor_documents.category` (migration 0024): tax (K-1), legal (subscription agreement), due_diligence (PPM), statement (account statement).
- Formats restricted to what `upload_guard.ts` allows (pdf / png / csv / txt; ≤25 MB).

**Where they are:** `file:///home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/tmp/samples/`
- `loan-applications/` — appraisal-report.pdf, preliminary-title-report.pdf, property-photos.png (640×420 RGB), purchase-agreement.pdf, executed-promissory-note.pdf, deed-of-trust.pdf
- `investor-accreditation/` — accreditation-income.pdf, accreditation-net-worth.pdf, accreditation-institution.pdf, accreditation-professional.pdf
- `investor-documents/` — tax-k1-2025.pdf, subscription-agreement-legal.pdf, due-diligence-ppm.pdf, account-statement.pdf

14 files, each a real PDF (v1.4, one page, readable text) or a real PNG — so they pass the upload guard and actually open. All clearly watermarked "SAMPLE — synthetic … for upload testing only."

**Reproducible:** the generator is committed at `bin/make-sample-documents.ts` (hand-rolls valid PDF + PNG bytes, no deps), so the set survives a fresh clone — regenerate any time with `bun run bin/make-sample-documents.ts`. (`tmp/` itself stays gitignored.)

**Verified:**
- `bun test` — **574 pass / 0 fail**, incl. new `#sample-upload-docs` coverage (PDF/PNG validity, guard-allowed extensions, size limit).
- No app/worker change, so nothing to deploy this cycle.

If you also want a CSV/spreadsheet variant for the K-1 or bank statements (some reviewers prefer those), or a deliberately-oversized/disallowed file to test the rejection path, say so.

### Re: Borrower dashboard "Needs action" component — opus-4.8

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/portal-borrower/dashboard should have a similar needs action component as the admin dashboard. An incomplete application or application that needs some action should be on the borrower dashboard. deploy


Done and deployed. The borrower dashboard now has a **"Needs action"** panel that mirrors the admin dashboard's attention card and surfaces both incomplete applications and items needing action.

**What I found:** the borrower dashboard already had a small "Needs attention" card, but it only listed loans missing documents — **incomplete (draft) applications weren't surfaced at all** (the dashboard data only read the `loans` table, never `full_applications`).

**Changes:**
- **Backend** (`borrower/dashboard/index.ts`): added a query for the borrower's own **draft** full applications (`full_applications WHERE borrower_user_id = ? AND status = 'draft'`) and returns them as `incomplete_applications` plus an `incomplete` count.
- **Frontend** (`borrower/dashboard.tsx`): renamed the card to **"Needs action"** and rebuilt it as a unified, prioritized list:
  1. **Finish your loan application** — one per incomplete draft, linking to `/portal-borrower/new-application?application_id=<id>` to complete + submit it (shown first, since nothing progresses until it's done).
  2. **Documents needed** — submitted applications still missing required docs (existing behavior, preserved).
  - Shows the top 2 with a count badge and a "Show N more" expander, same pattern as before.

So an incomplete application — or any application needing action — now appears on the borrower dashboard, like the admin side.

**Verified + deployed:**
- `bun test` — **576 pass / 0 fail** (added `#borrower-needs-action` coverage for the panel + the draft-application API; updated the e2e source assertions for the rebuilt component).
- `bun run build:worker` clean; `bun run deploy:worker` live (version `0482680d`).
- EFS commit `a7372df`; submodule bump `b950f9eb1`.

Hard-refresh `/portal-borrower/dashboard` to see it (it only appears when there's an incomplete application or a docs-needed item).

### Re: All document upload fields allow multiple uploads — opus-4.8

> **Queue prompt:**
>
> All document upload fields should allow multiple uploads. deploy


Done and deployed. Every document upload field now accepts multiple files (the OS picker allows multi-select, and the handlers upload all selected files).

**8 upload fields updated:**
- **Borrower required-doc slots** (`documents.tsx`, grid + list) — backend already accepts a `files` array, so all selected files go up in one request.
- **Borrower new-application uploader** (`doc_uploader.tsx`) — this is a "choose file → pick its category" flow, so multi-select now **queues** the files and walks you through one category at a time (drop or file-picker).
- **Borrower full application** + **admin full-application detail** (`full_application.tsx`, `full_application_detail.tsx`) — per-type slots upload every selected file.
- **Investor accreditation** (`accreditation.tsx`) — attach several supporting documents at once under the chosen verification type.
- **Admin public documents** (`documents.tsx`), **lender documents** (`lender_documents.tsx`), **deal documents** (`deal_documents.tsx`) — multi-select uploads each file (single-name field still applies when you pick one).

I intentionally left the **image** fields single-file — the deal hero image and public-content image (`PortalFileInput`) are single-image pickers, not document collections.

**Verified + deployed:**
- `bun test` — **586 pass / 0 fail** (added `#multi-upload` coverage asserting every document field is `multiple` and its handler iterates all files).
- `bun run build:worker` clean; `bun run deploy:worker` live (version `45c271fd`).
- EFS commit `fc4447f`; submodule bump `933d5eb7b`.

Hard-refresh to pick up the new bundle. (`tsc` still flags two pre-existing non-strict lines in `admin/documents.tsx` — a `confirm_` string arg and a json `value:unknown` — neither touched by this change; esbuild builds clean and the suite is green.)

### Re: Loan application "Replace file" buttons → multi-file upload form — opus-4.8

> **Queue prompt:**
>
> The loan application still has "Replace file" buttons ![img_8.png](img_8.png). These should be a multi-file upload form. deploy


Done and deployed. The loan application Documents section is now a **single multi-file upload form** instead of per-type "Replace file" buttons.

**New flow** (from img_8.png's per-type buttons → one form):
1. A drop-zone at the top — **drop or choose many files at once**.
2. Each selected file appears in a "Ready to upload" list with a **category dropdown** (auto-defaulted to the next required-but-missing type) and a Remove button.
3. **Upload all** — groups files by category and uploads them in one action.
4. Below, a read-only **checklist** of the required types shows each one's status (Uploaded / Required / Optional) with the uploaded filename — so borrowers still see exactly what's needed, without per-type buttons.

Applied to **both** surfaces: the borrower loan application (`full_application.tsx` — the screenshot) and the shared admin/pipeline-dialog full-application detail (`full_application_detail.tsx`), so they stay unified. The per-type "Replace file"/"Choose file" controls are gone.

**Verified + deployed:**
- `bun test` — **587 pass / 0 fail** (added `#app-docs-multi-upload` coverage: the form exists, `upload_all` groups by type, and the old per-type Replace control is gone).
- `bun run build:worker` clean; `bun run deploy:worker` live (version `3da65bae`).
- EFS commit `8e5960f`; submodule bump `6d5961c60`.

Hard-refresh the application form to see it. Note this builds on the prior multi-file change: each category still accepts multiple files, and the backend stores them per type.
<!-- agent:boundary:6fa8c865 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- The upload UI has too much friction. Each form should have its own drop file area along with supporting click to open the file dialog. deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
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
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
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
- [ ] [#z3kx] GoHighLevel CRM as messaging/notifications path of record (Robert's enterprise GHL); integrate with the site so notifications also surface in-app; avoid SendGrid/extra recurring fees. From 2026-06-03 call. Relates to #xcvg.
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
