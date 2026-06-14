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
queue: stop
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

### Re: Make each uploaded file viewable by clicking it (in-browser viewer) — opus-4-8

> **Queue prompt:**
>
> Can we make each uploaded file viewable by clicking on it? Can we have open a viewer of each supported file type?

Yes — shipped a shared in-browser document viewer (EFS `c636ea8`). Clicking an uploaded file now opens an in-page modal that renders the file inline instead of forcing a download.

**How it works** (`shared/ui.tsx`):
- `document_view_kind_()` classifies each file → **image** (`<img>`), **PDF** (`<iframe>`), **text/CSV/JSON/MD/XML** (`<pre>`), or **unsupported**. HEIC/HEIF/TIFF have image content-types but no broad browser support, so they're treated as download-only.
- `use_document_viewer_()` + `DocumentFileLink` (a drop-in for the old `<a href download>` filename links). The file is **fetched as a blob and shown via an object URL**, so the server's `Content-Disposition: attachment` can't force a download and the *same* secured href serves both the inline view and the modal's **Download** button. Modified-clicks (⌘/Ctrl/middle-click) still download natively. Unsupported types and load errors fall back to a Download link.

**Wired this cycle (borrower-facing uploaded files):**
- Loan-application **Documents** rows (borrower full application + admin/shared application detail) — click the filename to view.
- Borrower **Documents** page — uploaded-doc cards get a **View** action; public-document cards open the viewer.

**Per `#spec-test-commit-push`:** new `document-viewer.test.ts` (classifier + wiring) and updated `download-security.test.ts` to assert the HTTPS guard is now centralized in the shared component. `bun run check` green (656 tests), worker build clean. Committed + pushed to `main`.

**Follow-up (queued `#docview-tables`):** the admin/investor/broker/lender document **tables** still use download-only buttons — those are a distinct set of table-action surfaces (`shared/table_actions.tsx` + ~7 files). Added a "View" action to each is tracked as actionable backlog so this cycle stays a coherent, tested unit.

Not deployed — this preset is commit+push only; the `deploy` item next in the queue will push it live.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed. Both queued changes from this drain are now live:

- `a8db25e` — per-type upload zone reworded to **"Drop file or click to select file"**.
- `c636ea8` — **in-browser document viewer** (#doc-viewer): click an uploaded file to preview it inline (images, PDF, text/CSV/JSON) on the borrower application-documents rows and the borrower Documents page.

`deploy:worker:no-migrate` (no schema change) → version `7c1f462f`, https://equityfundingsource.brian-takita.workers.dev.

Remaining in the queue: `do [#c7cs]` — extend click-to-view to the admin/investor/broker/lender document **tables**. That's the next drain cycle.

### Re: do [#c7cs] — extend click-to-view to admin/investor document surfaces — opus-4-8

> **Queue prompt:**
>
> do [#c7cs]


Done (EFS `5f5e504`). Extended the in-browser document viewer to the remaining download-only surfaces, so **every uploaded file across the portals is now viewable by clicking it**:

- **shared/ui.tsx** — added `DocumentViewActionButton` (eye icon-button for table action columns) and a `class` prop on `DocumentFileLink` for button-styled links.
- **admin/documents.tsx** — table action columns open the in-page viewer instead of a new tab (removed the `inline_document_url_`/`view_url` new-tab indirection).
- **admin/document_request_detail.tsx**, **investor/documents.tsx** — provided-document cards open the viewer on click.
- **admin/deal_documents.tsx** — "View" now opens the in-page modal *and* runs the href through the HTTPS guard (it previously opened a raw href in a new tab — a small security tightening).
- **admin/upload_detail.tsx**, **investor/available_deals.tsx** — added a **View** action beside Download.

**Left as-is:** lender documents already ship their own stacked-modal previewer (`LenderDocumentPreviewModal`, #doc-stack-esc) — untouched.

**Per `#spec-test-commit-push`:** `document-viewer.test.ts` asserts each surface offers click-to-view; `download-security.test.ts` updated for the now-centralized HTTPS guard. `bun run check` green (657 tests), worker build clean. Committed + pushed.

That drains the queue. Note: a stale route-owned supervisor (pid 861902, pre-current binary) is still throwing the recurring File-Cache-Conflict / IPC-drift dialogs this session — restart it with `agent-doc session restart-supervisor tasks/professional/equityfundingsource.md --force` (or restart the session) to clear them; it doesn't affect the shipped code.

### Re: do [#67v2b] — Admin pipeline drag-to-change-status TMO writeback outbox — opus-4-8

> **Queue prompt:**
>
> do [#67v2b]

Done (EFS `dcfca4e`). Phase B of the admin loan pipeline (#67v2) now records the **EFS → TMO loan-status writeback** durably, so a drag between columns is captured for TMO sync once the writeback gate clears.

**What was already there:** the drag-between-columns UI and the loan-dialog status dropdown both already PUT `/api/admin/loans/:id/status` with an optimistic card move + rollback-on-error (`change_loan_status_`, `pipeline_card_move_`). That EFS-local half shipped in an earlier cycle.

**What this phase adds (the gated TMO-writeback half):**
- `tmo_loan_status_outbox_enqueue_` (`lib/tmo-api/index.ts`, re-exported via `admin/shared.ts`) inserts a `tmo_cache_outbox` row — `entity_type='loan'`, `operation='update_loan_status'`, `status='pending'`, `payload_json={ loan_number, loan_status, short_name, tmo_rec_id }` — mirroring the `add_loan_funding` outbox precedent. Returns `null` (non-fatal) when the loan has no TMO loan number or the insert fails, so the EFS-local status change always stands.
- `admin_loans_by_id_status_PUT_` calls the enqueue right after `admin_loan_status_update_`, ahead of the legacy direct `tmo_loan_status_sync_`. The outbox is the durable gated-replay record; the direct sync stays as the pre-existing config-gated path.

**Replay stays gated (#7a4d / #dzmm):** no consumer drains `update_loan_status` rows yet — they accumulate as `pending` until the TMO write field map (#7a4d) + sync direction (#dzmm/#k2xg) are confirmed. A regression test guards that no premature replay worker was added.

**Per `#spec-test-commit-push`:** updated the PRD Phase B section (`prds/67v2-admin-loan-pipeline-kanban.md`) with the implementation block; added 5 tests to `tests/admin-pipeline.test.ts` (outbox row shape, no-loan-number skip, non-fatal failure, status-PUT wiring order, replay-gated guard). `bun run check` green (662 tests), worker build clean. Committed + pushed to `main` (submodule pointer bumped to `dcfca4e`).

Not deployed — this preset is commit+push only. Remaining queue: `do [#67v2d]` (admin dashboard pipeline summary) then `#advance-review`.

### Re: do [#67v2d] — Admin dashboard pipeline summary (Phase D) — opus-4-8

> **Queue prompt:**
>
> do [#67v2d]

Done (EFS `47fb134`, submodule pointer `1d1033685`). Phase D of the admin loan pipeline (#67v2) now surfaces a **per-stage pipeline summary with next-actions on the admin dashboard**, mirroring the `/portal-admin/pipeline` board.

**No drift by construction:** the board's column build in `admin_pipeline_GET_` was extracted into a shared `admin_pipeline_columns_(auth, now_s)` (`lib/admin/loan_management/index.ts`). The board GET and the dashboard summary now consume the **same** columns + counts, so they can't diverge.

**What shipped:**
- `admin_dashboard_pipeline_summary_(columns)` (`lib/admin/dashboard/index.ts`) — pure, unit-tested builder mapping each board column to a stage `{ id, label, count, action_needed, href }`, plus `total` and `action_needed_count`. `action_needed` flags the **open funnel** (Incomplete Applications + Applications / Processing / Documents / Underwriting), mirroring the Deal Pipeline action badge; terminal/parked stages (Funded / Closed / On Hold / Unassigned / Application Denied) are not.
- `GET /api/admin/dashboard` adds a `pipeline_summary` field (built in the existing `Promise.all`; a summary failure logs and degrades to an empty stage list — never a 500).
- **Dashboard UI** (`portal/admin/dashboard.tsx`) renders a **Loan pipeline** section under the metric grid: a per-stage count grid with action-needed stages highlighted + an "Action" badge, plus an "Open pipeline board" link. Each stage card deep-links to `/portal-admin/pipeline?tab=pipeline&column=<id>`.
- **Pipeline board** (`portal/admin/pipeline.tsx`) reads `?column=<id>` and scrolls the targeted stage column into view with a highlighted border, so the dashboard deep-link lands on the right stage.

**Per `#spec-test-commit-push`:** updated the PRD Phase D section (`prds/67v2-admin-loan-pipeline-kanban.md` → ✅ shipped) and added tests to `tests/admin-dashboard.test.ts` (pure summary builder: stage order, action flags, totals, deep-link hrefs; endpoint wiring from the shared builder; UI deep-links + board `?column=` scroll). `bun run check` green (665 tests), worker build clean. Committed + pushed to `main`.

Not deployed — this preset is commit+push only. Remaining queue: `#advance-review`.

### Re: #advance-review — opus-4-8

> **Queue prompt:**
>
> #advance-review


> **Queue prompt (preset `#advance-review`):**
>
> Go through the review items. Add actionable backlog items to complete or advance the review items.

Went through all 9 `agent:review` items. The portal is substantially built out, so most review items are **parked on external/operator dependencies** (no agent-actionable step), and a few describe work that has **already shipped**. I found **one** genuinely EFS-local, ungated next step and queued it; the rest are triaged below.

**Queued (actionable, EFS-local):**
- `#vd8s-timeline` — **Per-investor activity timeline** in the in-page investor detail modal (#vtx8): sign-up, accreditation decision, lender link, commitment, wire-confirmation, and distribution dates, all from existing D1 tables. No TMO gate. Plan: `prds/investor-pipeline-admin.md` (line 92). This is the only non-gated remainder of `#vd8s` — the drag-to-confirm-wire from the Committed lane it also mentions already shipped (`#vd8s-dragwire`: `ConfirmWireModal` + multi-pending-fund disambiguation in `investor_pipeline.tsx`).

**Triage of the rest (no agent-actionable step right now):**
- `#67v2` — **near-complete**: Phases A (board), B (status writeback outbox), D (dashboard summary, just shipped), E (leads funnel) are all done. The only remaining sub-phase is **C (per-stage requirement checklist + auto-advance)**, whose requirements list is TBD pending Robert/April (PRD Q3). Operator-gated, not codeable yet.
- `#xcvg` — message **email** round-trip is gated on a verified Resend domain (`#f4rq`) + Google Workspace SMTP (`#m6ch`/`#p40t`). The in-app half (admin owner + unread-count badges) already ships in `admin/messages.tsx`.
- `#37ah` — broker portal backend shipped (`#37ah-backend`); only the TMO writeback sync remains, gated on `#dzmm` (`#7a4d`/`#k2xg`).
- `#dzmm` — TMO webhooks unavailable per operator; read-side polling shipped. EFS→TMO writeback (outbox replay) is gated on the `#7a4d` write field map + `#k2xg` support answers.
- `#zwws` — remaining investor-dashboard phases are TMO-writeback-gated (`#dzmm`/`#k2xg`/`#7a4d`) + email-gated (`#f4rq`).
- `#next-steps` — workers.dev prod is healthy; custom-domain cutover is intentionally deferred and true Google SSO is gated on Google admin creds.
- `#jnzs` — awaiting Judith/EFS reviewer **human approval** on funded-deal copy/lien wording before launch sign-off.
- `#dropbox-sync-adapter` — gated on Dropbox access being provisioned.

So: 1 actionable item queued (`#vd8s-timeline`); the other 8 review items are correctly parked on TMO/Resend/Google/Dropbox/human-approval gates and stay in `agent:review`. (Exchange is at ~156 lines / 8 sections — noting size; queue stays active.)
<!-- agent:boundary:334130b5 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#vd8s-timeline] Investor Pipeline: per-investor activity timeline in the in-page investor detail modal (#vtx8) — sign-up, accreditation decision, lender link, commitment, wire-confirmation, and distribution dates from existing D1 tables. EFS-local (no TMO gate). Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md. Advances review #vd8s.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#vd8s] [recommended] Investor Pipeline phase 2 — remaining after stalled-stage highlighting shipped (EFS 055a73c): drag-to-confirm-wire from the Committed lane (needs multi-pending-fund disambiguation) and TMO AddLoanFunding sync state in the Active lane once the writeback gate (#dzmm/#k2xg) clears. Per-investor activity timeline folds into the #vtx8 detail modal. Plan: src/equityfundingsource-dev/prds/investor-pipeline-admin.md
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
