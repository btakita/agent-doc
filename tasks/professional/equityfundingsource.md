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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260613-084854.md`*

Compacted content:
❯ - Archived 1 response topic(s): #advance-review
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: do [#qakx-impl] — already shipped by #rpzm; verified + closed — opus-4-8

> **Queue prompt:**
>
> do [#qakx-impl]


Investigated before building: #qakx-impl is already delivered by the #rpzm dashboard-rework work, so this cycle is verify-and-close, not net-new code.

**Already in place (zero fixtures remain):**
- **Documents** (`lib/ui--browser--efs/portal/investor/documents.tsx`) — D1/R2-backed via `GET /api/investor/documents`, request submission via `POST /api/investor/documents/request`, and authenticated per-document download via `GET /api/investor/documents/:id/download` (user-scoped, `status=published` gate, streams from the `R2_DOCUMENTS` bucket).
- **Accreditation** (`accreditation.tsx`) — D1-backed via `GET /api/investor/accreditation` with authenticated evidence upload via `POST /api/investor/accreditation/upload` (FormData → R2 via `document_cache_put_`).
- **Statements** — intentionally retired in #rpzm phase 2b (the standalone Distributions/Statements section was removed). "statement" now lives as a document **category** and request **kind**, not a fixture page.
- Schema: `investor_documents`, `investor_document_requests` (+ `provided_documents`), `investor_accreditation_verifications`, `public_documents` (migrations 0024 + 0058). R2 binding `R2_DOCUMENTS`; auth is JWT-in-cookie via `session_R_user_`.

**Verification:** `bun test` → **621 pass / 0 fail** (incl. `tests/investor-documents.test.ts` 5/5 covering the migration tables + route wiring). Grep confirmed zero `sample/fixture/mock/demo/static` data left in `lib/ui--browser--efs/portal/investor/`. No source changed → nothing to commit/push.

Marking **#qakx-impl done**. Next in the go-queue: #vd8s-dragwire.

### Re: do [#vd8s-dragwire] — drag-to-confirm-wire shipped — opus-4-8

> **Queue prompt:**
>
> do [#vd8s-dragwire]


Implemented the non-TMO half of Investor Pipeline phase 2: drag a Committed-lane card onto the Active lane to confirm a wire, with multi-pending-fund disambiguation. Shipped in EFS `b081fb2` (superproject pointer `08f5d2e3c`).

**What changed:**
- **Backend** (`repo.ts`, `admin/investor_management/index.ts`): the pipeline card now carries a per-commitment `pending_funds` list (id, amount, loan #, borrower/property label) via a second **read-only** SELECT joining `funds → investment_deals → loans`. The board endpoint stays mutation-free.
- **Board** (`investor_pipeline.tsx`): Committed cards (`pending_fund_count > 0`) are draggable (grab cursor + "Drag to Active to confirm wire" hint); the Active lane is the drop target ("Drop to confirm wire"). The drop opens `ConfirmWireModal`.
- **Disambiguation**: a single pending fund is preselected; multiple force an explicit radio pick before confirming — the multi-pending-fund disambiguation the PRD flagged as the blocker.
- **Confirm**: calls the existing `PUT /api/admin/funds/:id/confirm-wire` (moves the fund pending→active and performs the TMO AddLoanFunding writeback). If TMO is unavailable the endpoint error surfaces in the dialog — the wire is never silently lost. Clicking a card still opens the detail modal (#vtx8); a drag suppresses the click.
- **Spec**: `prds/investor-pipeline-admin.md` Phase 4 recorded as shipped. The remaining "Future phase" is the TMO sync-**state** display in the Active lane (distinct from this confirm trigger), still gated on #dzmm/#k2xg.

**Verification:** `bun run check` → **626 pass / 0 fail** (+5 new tests), TypeScript build + worker bundle clean. Committed + pushed.

Marking **#vd8s-dragwire done**. Next in the go-queue: #67v2-kanban.

### Re: do [#67v2-kanban] — already shipped (LOAN_PIPELINE Kanban); verified + closed — opus-4-8

> **Queue prompt:**
>
> do [#67v2-kanban]


The TMO-aligned-stage loan Kanban already exists — verify-and-close.

**Already in place:**
- **TMO-aligned stages** (`repo.ts` `LOAN_PIPELINE`, `status_source: 'tmo'`): Applications → Processing → Documents → Underwriting → Funded → Closed, plus On Hold / Unassigned / Application Denied. Lanes map 1:1 to TMO `tmo_loan_status` values — TMO is the single source of truth.
- **Kanban + drag + manual advance** (`pipeline.tsx`): loan cards drag between columns → `change_loan_status_` → `PUT /api/admin/loans/:id/status`, which writes `tmo_loan_status`, syncs the linked investor-deal status (`admin_deal_status_sync_by_loan_`), and the board live-refreshes on pushed admin events (dashboard sync). The loan dialog's status dropdown shares the same write path.
- Leads funnel column is read-only (a lead isn't a loan/TMO status); the board is Kanban-only with in-place search (PRD phase 3b).

**Gated remainder:** the "auto-advance when stage requirements met" half of review #67v2 needs the TMO writeback gate (#dzmm/#k2xg) — already deferred there; the manual-advance Kanban is complete.

**Verification:** `bun test tests/admin-pipeline.test.ts` → **44 pass / 0 fail**. No source changed → nothing to commit/push.

Marking **#67v2-kanban done** (review #67v2 stays gated for the TMO auto-advance piece). Next in the go-queue: #37ah-backend.

### Re: do [#37ah-backend] — already shipped (D1-backed broker portal); verified + closed — opus-4-8

> **Queue prompt:**
>
> do [#37ah-backend]


The broker portal backend is already D1-backed across all five named surfaces — verify-and-close.

**Already in place** (`app/equityfundingsource/lib/broker/routes.ts`, all behind `require_broker_` auth):
- **Submit-deal** — `POST /api/broker/deals` (+ `POST /api/broker/pre-qual`).
- **Pipeline** — `GET /api/broker/pipeline` → `broker_deals_R_(db, broker_id, {status})` with `BROKER_DEAL_STATUSES`.
- **Commissions** — `GET /api/broker/commissions` → `broker_commissions_R_(db, broker_id)`; admin side `POST /api/admin/broker-commissions` + `/:id/mark-paid`.
- **Documents** — `GET /api/broker/documents` → per-deal documents via `broker_deals_R_`.
- **Notifications** — `GET /api/broker/notifications` (D1 rows, empty-DB guarded).
- Dashboard metrics + admin `POST /api/admin/broker-deals/:id/status`. The UI (`portal/broker/api.ts`) calls each via fetch.
- The only remaining static data is `BROKER_RATE_SHEETS` (published rate sheets in `data.ts`) — not deal/pipeline/commission data, intentionally static.

**Gated remainder:** the TMO-backed sync of broker deals (review #37ah's "TMO-backed") stays behind the #dzmm webhook gate; the D1 backend is complete.

**Verification:** `bun test tests/broker-portal.test.ts` → **35 pass / 0 fail**. No source changed → nothing to commit/push.

Marking **#37ah-backend done** (review #37ah stays gated for the TMO sync). Next in the go-queue: #xcvg-admin.

### Re: do [#xcvg-admin] — already shipped (borrower secure-message admin); verified + closed — opus-4-8

> **Queue prompt:**
>
> do [#xcvg-admin]


The non-email half of the borrower secure-message system is already built — verify-and-close.

**Already in place:**
- **Admin owner/assignment** — migration `0034_chat_admin_owner` (`chat_sessions.admin_owner_user_id`); ownership is claimed by the first admin who replies via `borrower_chat_session_admin_owner_set_if_unset_` (called from `POST /api/admin/messages/:session_id`). Unowned threads notify all active admins.
- **In-app notification surface** — `notify_admin_of_borrower_message_` / `notify_borrower_of_admin_message_` write the existing notifications table immediately; `GET /api/messages/count` returns the admin unread count (`borrower_chat_admin_unread_count_R_`) and the admin nav badge counts message sessions.
- **Inbound-reply → D1 chat-append plumbing** — the admin reply POST appends an `efs` `chat_messages` row; the inbound **email**→chat handler is scaffolded behind `MESSAGE_EMAIL_INBOUND_ENABLED` and inert until inbound routing is configured.
- Borrower side: `GET/POST /api/borrower/messages` (+ WS live updates), read cursors (0031), session titles (0032).

**Gated remainder (as designed):** email send (`MESSAGE_EMAIL_NOTIFICATIONS_ENABLED`) and inbound email receive (`MESSAGE_EMAIL_INBOUND_ENABLED`) stay off until the Resend sending domain (#f4rq) + Google Workspace SMTP (#m6ch/#p40t) land — `notify.ts` documents this split explicitly.

**Verification:** full `bun test` → **626 pass / 0 fail** (incl. `borrower-messages`, `borrower-message-notify`, `contact-messages`). No source changed → nothing to commit/push.

This also closes the borrower-portal-messages question from earlier in the session: admin owner + in-app round-trip is live now; the email legs are the gated piece.

Marking **#xcvg-admin done** (review #xcvg stays gated for the email legs). Next in the go-queue: #dzmm-followup (the email to Robert).

### Re: do [#dzmm-followup] — drafted the TMO-webhook email to Robert — opus-4-8

> **Queue prompt:**
>
> do [#dzmm-followup]: robert-waldman does not know these answers. Let's craft the email to Robert explaining what this for. My running assumption is that the TMO API is basic and may not support many of the things you are asking for. Could you also explain mitigations if TMO does not support what we want from them?


Right call that Robert won't know the answers — he's Judith's tech/marketing consultant (`rwaldman100@gmail.com`), not a Mortgage Office engineer. So the email explains the *purpose* in plain terms, asks him to forward the technical question or introduce us, states the "TMO API may be basic" assumption up front, and shows the fallback so it's clearly not a blocker either way. **Draft only — not sent.** Source: `docs/tmo-webhooks-question.md`.

---

**To:** Robert Waldman &lt;rwaldman100@gmail.com&gt;
**Subject:** Getting EFS portal updates from TMO faster (and our fallback if it can't)

Hi Robert,

Wanted to give you the context on a technical question about The Mortgage Office before we ask them anything, since it touches how fresh the data is in the EFS portal.

**What this is about.** Right now the EFS site pulls loan information from TMO once a day. So when a loan's status changes in TMO (moves to Processing, Underwriting, Funded, and so on), it can take up to a day before borrowers, investors, and the admin team see that change on the site. Everything works, it's just delayed, and users currently see a "last synced" note that makes the data feel stale.

What we'd like is for TMO to tell us the moment something changes instead of us checking once a day... in plain terms, a "push" from TMO to our system whenever a loan, funding, or document record updates.

**The question for Mortgage Office.** I've written up the specific technical question for their support team (I can send it as a one-page attachment). The short version: does TMO support outbound webhooks (automatic notifications when a record changes), and if so, how do we authenticate and subscribe? If you can forward that to your Mortgage Office contact, or introduce us directly, that's all we need from them.

**My assumption, and why it's not a blocker.** I'm assuming TMO's API is fairly basic and may not support push notifications at all. That's completely fine. If they can't, here's the fallback, which gets us most of the benefit without anything from TMO:

1. Check TMO more often. Instead of once a day, we pull every 5 to 15 minutes, and only the records that changed, so it stays light. This is a small config change on our side.
2. A "Sync now" button for the admin team, so a reviewer can force a fresh pull immediately instead of waiting on the schedule.
3. A clear "TMO sync pending" label on anything mid-update, so users always know whether the site is current.

So whether or not TMO supports webhooks, the portal gets noticeably fresher. The webhook route is just the cleanest option if it happens to exist.

Happy to hop on a quick call if that's easier. Thanks, Robert.

Best,
Brian

---

**Send path:** I left this as a draft. Say the word and I'll push it to Gmail Drafts via corky (threaded to Robert), or adjust tone/length first. Marking **#dzmm-followup done** (the craft is complete; sending is your call). Next in the go-queue: #jnzs-approval.

### Re: do [#jnzs-approval] — drafted the funded-deals approval request to Judith — opus-4-8

> **Queue prompt:**
>
> do [#jnzs-approval]


The technical side of Funded Deals shipped 2026-05-23 (D1 migration, Worker deploy, live smoke tests), so #jnzs is purely a sign-off gate. Here's the approval-request email to Judith (CC Robert) covering the four items she owns: images, copy, lien-position wording, and legal/disclosure language. **Draft only — not sent.** (Large exchange noted; queue still draining.)

---

**To:** Judith Krug &lt;judithkrug@gmail.com&gt;
**CC:** Robert Waldman &lt;rwaldman100@gmail.com&gt;
**Subject:** Funded Deals page ready for your review before launch

Hi Judith,

The Funded Deals page is built and live on the staging site, and the technical side is done... the data, the deploy, and the page itself all tested out as of late May. Before we put it in front of the public and move to invoicing, I'd like your sign-off on the parts that are really yours to approve: the wording and the legal language.

You can see it here: https://equityfundingsource.brian-takita.workers.dev/funded-deals

Four things I'd like you to look at:

1. **Deal images.** Are these the right photos for each funded deal, and are you comfortable showing them publicly?
2. **Copy.** The headlines and descriptions for each deal... anything you'd word differently.
3. **Lien-position wording.** How each deal states its lien position (first, second, and so on). I want that language to match how you'd put it.
4. **Legal and disclosure language.** Any disclaimers or disclosures on the page... please confirm they're accurate and complete for your compliance comfort.

If everything looks good, just reply "approved" and I'll treat that as the green light for launch and invoicing. If anything needs changing, mark it up however is easiest and I'll turn it around quickly.

Robert, copying you in case you want to weigh in on the marketing copy.

Thanks both.

Best,
Brian

---

**Send path:** left as a draft. Say the word and I'll push it to Gmail Drafts via corky (to Judith, CC Robert), or tweak first. Marking **#jnzs-approval done** (craft complete; sending + the actual approval are operator/Judith actions — review #jnzs stays gated until she signs off).

**Go-queue drained — all 7 items closed.** Summary in the next note.
<!-- agent:boundary:fdddaafe -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
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
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, lien-position wording, and legal/disclosure language before invoicing or launch sign-off. Technical D1 migration, Worker deploy, and live funded-deals/admin-route smoke tests completed 2026-05-23.
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
