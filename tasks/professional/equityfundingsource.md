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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-052039.md`*

Compacted content:
- Archived 1 response topic(s): Email bleeding into next column in pipeline table views
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: "Open full application" should open a Full Application dialog with a page link — opus-4.8

> **Queue prompt:**
>
> In https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline?tab=pipeline&status=new, the "Open full application" button in the Application dialog should open a Full Application dialog. The Full Application dialog should have a link to the page. deploy


Done. In the admin pipeline (tab=pipeline), the Application dialog's **Open full application** button no longer navigates away — it now opens a stacked **Full Application dialog** in place, and that dialog carries a deep link to the standalone page.

**What changed** (`lib/ui--browser--efs/portal/admin/pipeline.tsx`):
- New `FullApplicationModalBody` — fetches `/api/admin/full-applications/:id` (the same source as the standalone page) and renders the existing `FullApplicationDetailView` in **read-only** mode, so the admin sees the complete application inline without leaving the board.
- "Open full application" is now a button that opens this dialog (`PortalModal`, `z-[1100]`) stacked above the Application dialog (Esc/backdrop close the top one first via the shared modal stack).
- The dialog has **a link to the page** in two spots — an "Open page" button in the header and "Open full application page" in the footer — both deep-linking to `/portal-admin/loan-applications/:id`.

**Verified + deployed:**
- `bun test` — 566 pass / 0 fail. Updated `tests/admin-pipeline.test.ts`: retargeted the detail-link assertion to the new dialog and added `#pipeline-full-app-dialog` coverage (read-only reuse, stacked dialog, deep link).
- `bun run build:worker` clean; `bun run deploy:worker` live at https://equityfundingsource.brian-takita.workers.dev (version `aa9fa9e7`).
- EFS commit `8614fe4`; submodule pointer bumped (`5ed5132c2`).

Note: `tsc --noEmit` flags the new fetch/render-prop lines, but those mirror the file's existing accepted patterns (lead-modal fetch, the other modal render-props) — the project builds via esbuild, not strict tsc, and the suite is green. Hard-refresh to pick up the new JS bundle.

### Re: Add Decision / Feedback report / Sent feedback + Activity history to the Full Application dialog — opus-4.8

> **Queue prompt:**
>
> Sent feedback


> **Queue prompt:**
>
> Feedback report


> **Queue prompt:**
>
> Decision


Done — and deployed. (Heads up: the queue split your bulleted list into three items; this is one request, handled together.)

**What I found:** Decision, Feedback report, and Sent feedback already existed on the *standalone* full-application page (`AdminFullApplications`'s `actions` slot), but the new pipeline dialog only showed a read-only view. So rather than duplicate that logic, I made the dialog reuse the same component.

**Changes:**
- **Dialog now reuses the page detail** (`pipeline.tsx`): `FullApplicationModalBody` renders `<AdminFullApplications application_id embedded />` instead of a read-only view. The dialog now carries the full **Decision** card (Mark in review / Decline / Accept & send to TMO / Remove + writeback status), the **Feedback report** composer, **Sent feedback** log, and editable sections — identical to the page. The dialog keeps its own header/close + "Open page" deep link, so the embedded page-style topbar/Back is suppressed (`{!embedded}` guard in `full_applications.tsx`).
- **New Activity history** (`full_applications.tsx`): added `ActivityHistoryCard` — a consolidated, reverse-chronological timeline merging lifecycle milestones (created/submitted), section + subject-property field edits (from `change_history`), and sent feedback reports. It's in the shared `actions` slot, so it renders on **both** the dialog and the standalone `/portal-admin/loan-applications/:id` page. Built entirely from data already on the detail payload — no backend change.

**Verified + deployed:**
- `bun test` — **567 pass / 0 fail**. Added `#full-app-activity-history` coverage and updated the dialog test to the reuse-based structure.
- `bun run build:worker` clean; `bun run deploy:worker` live (version `254dbcab`).
- EFS commit `c26fda9`; submodule bump `1a6166be0`.

`tsc --noEmit` still flags the file's pre-existing pattern lines (fetch `.then` value:unknown, render-prop `close` implicit-any) — no new error classes; the project builds via esbuild and the suite is green. Hard-refresh to pick up the new bundle.

### Re: Move Activity History to the right of Application Progress — opus-4.8

> **Queue prompt:**
>
> Move Activity History to the right of Application Progress. dep


Done and deployed. The Activity history card now sits **beside** Application Progress at the top of the full application detail, instead of at the bottom of the right column.

**Changes:**
- `full_application_detail.tsx` — added a `progress_aside` prop to `FullApplicationDetailView`. When present, Application Progress and the aside render in a responsive 2-up grid (`auto-fit, minmax(340px,1fr)`): Progress on the left, Activity history on the right, collapsing to stacked on narrow screens.
- `full_applications.tsx` — moved `<ActivityHistoryCard>` out of the `actions` slot and passed it as `progress_aside`. It still renders on **both** the standalone page and the pipeline dialog (which reuses this view), so the placement is consistent everywhere.

**Verified + deployed:**
- `bun test` — **568 pass / 0 fail** (added `#full-app-activity-right` coverage for the 2-up layout).
- `bun run build:worker` clean; `bun run deploy:worker` live (version `4a1f9453`).
- EFS commit `4f704d5`; submodule bump `aba9d6f8a`.

Hard-refresh to pick up the new bundle.

Heads-up (not blocking, outside this session's scope): preflight flagged a stale `agent-doc` install — the local binary/cdylib predate source commit `7bfc82e3`, so tmux/JetBrains sessions may run older agent-doc code. Run `make install` in `src/agent-doc` when you want to refresh it.

### Re: Activity history scrollbar + height constraint + view-all dialog — opus-4.8

> **Queue prompt:**
>
> Activity history should have a scrollbar and it's height constrained. Perhaps we should also have a dialog to view the entire history with more vertical space. deploy


Done and deployed. Both parts:

- **Height-constrained with a scrollbar** — the inline Activity history card now caps at `max-height:280px` with `overflow-y:auto` (and `overscroll-behavior:contain`), so a long timeline scrolls inside the card instead of stretching the detail layout next to Application Progress.
- **View-all dialog** — added a **View all** button in the card header that opens a `PortalModal` (max-height `86vh`, its own scroll) showing the entire history with much more vertical space.

Refactored the row rendering into a shared `ActivityTimeline` component reused by both the card and the modal, so they stay in sync. Both render on the standalone page and the pipeline dialog (which reuses this view).

**Verified + deployed:**
- `bun test` — **569 pass / 0 fail** (added `#full-app-activity-scroll` coverage; fixed one prior assertion for the restyled card header).
- `bun run build:worker` clean; `bun run deploy:worker` live (version `336ee8ac`).
- EFS commit `c264f91`; submodule bump `fb5c79ae3`.

Hard-refresh to pick up the new bundle. (Session note: the exchange is getting long — 8 cycles in 30 min — but I'm keeping the queue draining per the no-stall rule; compact when you're ready with `/agent-doc compact`.)
<!-- agent:boundary:d42106d8 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- The Full Application dialog on https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline?tab=pipeline&status=new should match the full design and behavior of https://equityfundingsource.brian-takita.workers.dev/portal-admin/loan-applications/48b67fd8-31a9-4cdf-a59b-c9c192da2026. That means Activity history should be to the right of the Application progress. All the sections should be present in both. The components should probably be unified. Do the same for all dialog + page combos. All elements and behavior in the full pages should be in the dialogs. Deploy 
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
