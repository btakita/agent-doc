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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260614-190908.md`*

Compacted content:
- Archived 3 response topic(s): Rename "Applications received" tag to "Accepted applications"; Accepted-application dialog; deploy
- Prior summary/context: ### Session Summary ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db...

### Re: Published deal tag + Published deals column (portal-admin/pipeline) — opus-4-8

> **Queue prompt:**
>
> Add a Published deal tag and a Published deals column in https://equityfundingsource.brian-takita.workers.dev/portal-admin/pipeline


Done — both shipped in the admin Deal Pipeline board (`lib/ui--browser--efs/portal/admin/pipeline.tsx`), committed to `equityfundingsource-dev` as `a41e097`. Frontend-only: the backend already attaches `deal_status: 'published' | 'funded' | 'closed' | null` to each loan pipeline card (`loan_pipeline_card_` in `admin/repo.ts`), so no API change was needed.

**Published deal tag** — a green `Published` pill (`PipelineDealBadge`, shows only when `deal_status === 'published'`) now renders everywhere a loan card appears:
- on the board card chip row (next to the stage + aging chips),
- in the Table view's Stage cell,
- in the loan detail dialog header.

**Published deals column** — a read-only cross-cut lane (`published_deals_board_column_`, id `published-deals`) appended after the existing status phases. It gathers every loan card with a live published deal regardless of which TMO status lane it sits in. It's not a drop target (the id isn't a TMO loan status), it auto-collapses to a thin rail when empty like the other lanes, and it respects the active/all scope + search filters since it's built from the same filtered view data.

**Verification:** `bun run check` green — 682 tests pass, 0 fail, worker build clean. Added deterministic coverage to `tests/admin-pipeline.test.ts` (asserts the column builder, the badge component, and the badge wiring).

One note (not blocking): because the lane honors the Active/All scope toggle, a published deal that has advanced to Funded/Closed only appears in the lane under "All" scope. That matches how every other lane behaves; flag it if you'd rather the Published lane always show published deals regardless of scope.

### Re: Available deal dialog on the investor dashboard — opus-4-8

> **Queue prompt:**
>
> Add an Available deal dialog in https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard. Clicking on the Available deal card opens the dialog. deploy

Done — shipped, pushed, and deployed.

**What changed** (`lib/ui--browser--efs/portal/investor/dashboard.tsx`): the "Available deals" preview cards on the investor dashboard used to be plain anchor links that navigated straight to `/portal-investor/deals/<id>`. They are now buttons that open an in-dashboard dialog when clicked.

- **`AvailableDealDialog`** renders through the shared `PortalModal` (backdrop click + Esc close, focus-friendly), with `role="dialog" aria-modal="true"` and a `ModalCloseButton`. It shows the deal heading + location, a 2-column detail grid (Loan amount, LTV, Sold rate, Available amount), the remaining-funding line, and a **View full deal** link to the existing detail page so navigation is still one click away.
- The card itself became a `<button>` (`aria-haspopup="dialog"`) that sets `selected_deal`; no behavior change to the data fetch or the 3-card / non-active filter.

**Verification:** `bun run check` green — **683 tests pass, 0 fail**, worker build clean. Added a deterministic source-assertion test to `tests/investor-portal-e2e.test.ts` covering the button-opens-dialog wiring, the `PortalModal` import, the dialog component, and that the card no longer renders the old anchor.

**Shipped:**
- `equityfundingsource-dev` commit `5de3fa4`, pushed to `origin/main`.
- Superproject pointer bump `ff7a60ef5`.
- Deployed to Cloudflare: `https://equityfundingsource.brian-takita.workers.dev` (Version `488a6f7f`).

Advances the #zwws investor-dashboard rework.
<!-- agent:boundary:32e0c25d -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- The Lender Account Request form should be a prerequisite to view Available deals. So it should not be on https://equityfundingsource.brian-takita.workers.dev/portal-investor/deals/7a0c1463-27bb-4d7c-82f7-2281b3c008d3. But it should be part of the workflow to request a lender account. Both https://equityfundingsource.brian-takita.workers.dev/portal-investor/available-deals and https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard can gate Avalable deals until the Lender Account Request is complete. We may require EFS to accept but that decision is TBD.
- The Available deal dialog on https://equityfundingsource.brian-takita.workers.dev/portal-investor/dashboard should be the same as the "Full Deal" page on https://equityfundingsource.brian-takita.workers.dev/portal-investor/deals/7a0c1463-27bb-4d7c-82f7-2281b3c008d3
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
