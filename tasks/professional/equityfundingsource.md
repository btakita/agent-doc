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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260619-035044.md`*

Compacted content:
- Archived 15 response topic(s): Why "Application progress" doesn't show the accepted step; How is it an Available Deal but not accepted?; do #214f (gate publish on acceptance); 12 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Available Deal dialog full screen in Deal Pipeline; Applicatio...

### Re: What is "Funding Commits" in the Deal Pipeline dialog? — opus-4-8

> **Queue prompt:**
>
> What is "Funding Commits" in the Deal Pipeline dialog?


It's the tab in the deal dialog that lists the **lender funding commitments** for that deal — i.e. how much money each lender has pledged to fund the loan, and where each pledge is in the wire/funding process. (Source: `lib/ui--browser--efs/portal/admin/deal_detail.tsx` "Funding Commits" tab → `AdminFundingCommits` / `GET /api/admin/funds`.)

Each commitment row is one lender's investment in the deal, with a **status**:
- **Pending** — the lender has committed an amount but the wire hasn't been received yet. You confirm receipt with "Confirm wire," which marks the funds received and submits the funding to TMO.
- **Active** — funds received/confirmed; the lender is funding the loan.

The tab has a status filter (**All funds / Pending / Active**), and the tab's count badge is the number of commitments on the deal. From here you also manage **fund allocations** (splitting/assigning a commitment).

How it differs from the neighboring tabs:
- **Interested Lenders** = lenders who expressed interest / the one selected as the funding lender (intent), *before* money.
- **Funding Commits** = the actual dollar commitments and their wire/funded state (the money).

So the flow is: a lender shows interest → gets selected → their committed funds show up here as a Pending commitment → you confirm the wire → it becomes Active and is reported to TMO.

### Re: Is the Funding Commit driven by TMO? — opus-4-8

> **Queue prompt:**
>
> In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=7a0c1463-27bb-4d7c-82f7-2281b3c008d3&deal_tab=funds, the selected lender is not in the "Funding Commit".


No — the Funding Commit is **EFS-owned**, not driven by TMO. Here's the actual data direction in the code:

1. **Created in EFS.** A Funding Commit is a row in the EFS `funds` table (D1). It's created when a lender funds the deal through the **investor portal** (`POST` fund → `INSERT INTO funds ... status='pending'`), entering an amount, their account number, and vesting/beneficiary. At that point it has **no** TMO record id yet.
2. **Written back to TMO on wire confirm.** When an admin clicks **Confirm wire** (`PUT /api/admin/funds/:id/confirm-wire`), EFS calls TMO's `AddLoanFunding`, stores the returned `tmo_funding_rec_id`, and flips the commit to **Active**. So EFS *pushes* the commit to TMO — TMO doesn't originate it. (This writeback is gated on the TMO field-map/support items `#7a4d`/`#dzmm`.)
3. **Reconciled from TMO (amounts only).** The TMO sync reads TMO's fundings into a cache and **updates the `amount_funded` on funds rows that already exist** (matched by `tmo_funding_rec_id`, or account + loan number). It does **not** create new `funds` rows from TMO.

So: TMO is the downstream system of record EFS syncs *to* (and reconciles amounts *from*), but a Funding Commit will never appear just because TMO has a funding — it has to exist in EFS first.

**Why this matters for your earlier report:** selecting a lender writes only the interest's `selected_at` + reserves the deal; it creates no `funds` row, and TMO sync won't create one either. So the selected lender stays absent from Funding Commits until an **EFS-side** commit exists. That's exactly the decision I asked about — whether selecting a lender should auto-create that EFS commit (and for what amount), add an admin "Add commitment" action, or leave it to the lender's portal funding. Let me know which and I'll build it.

### Re: Document the entire Deal Pipeline flow (all roles + Broker variant) — opus-4-8

> **Queue prompt:**
>
> Create a document that explains the entire Deal Pipeline flow...which includes the actions of all roles: Borrower, Admin, Lender. Also include a variant of the deal pipeline for the Broker.


Done. Created **`src/equityfundingsource-dev/docs/deal-pipeline-flow.md`** (EFS submodule `7b2e9d2`), grounded in the current code with anchors so it stays maintainable.

What it covers:

1. **The status models** (the key insight — the pipeline is several *independent* status fields, not one):
   - Loan steps (`tmo_loan_status`): Application Received → Loan Estimate → Intent to Proceed → Processing → Underwriting → Closing → Closed → Funded (+ Denied / Unassigned / On Hold).
   - Deal offering (`investment_deals.status`): published → reserved → funded → closed.
   - Funding Commit (`funds.status`): pending → active.
   - Borrower application (`full_applications.status`): draft → submitted → in_review → accepted / denied.
2. **End-to-end flow** with a mermaid sequence diagram, then **role-by-role actions** for **Borrower**, **Admin**, and **Lender** — including the Interested-Lenders-vs-Funding-Commits distinction we just clarified, and the EFS→TMO writeback direction.
3. **Broker variant** — the broker portal's own `broker_deals` status flow (submitted → in_review → docs_needed → approved → funded / declined) + commissions (pending → paid / reversed), with its own mermaid diagram and a note on how an approved broker deal joins the main loan pipeline.
4. **Quick-reference table** mapping every concern to its field/table/values.

This is documentation only (no code), so no build/deploy needed. Open it at `src/equityfundingsource-dev/docs/deal-pipeline-flow.md`.

Note: the doc describes the current implemented flow. The `Loan Estimate → Intent to Proceed → Closing` steps exist as pipeline statuses but their per-step borrower/admin sub-actions aren't all built out yet — the doc reflects the intended flow at those steps. The open `#closed-step-decouple` is resolved (done), and the Funding-Commit-on-selection decision is still pending your call.

### Re: Mermaid parse error in deal-pipeline-flow.md — opus-4-8

Fixed. The parse error on line 8 was caused by **semicolons inside the sequence-diagram message text**. Mermaid treats `;` as a statement separator, so:

```
A->>A: Review (in_review); request documents
```

was parsed as the message `Review (in_review)` plus a dangling `request documents` statement, which then collided with the next line (`B->>A: Provide …`) — exactly where the caret pointed.

Two such lines had this hazard; both now use commas instead:

- `Review (in_review); request documents` → `Review (in_review), request documents`
- `Loan → Closed / Funded; close offering …` → `Loan → Closed / Funded, close offering …`

Docs-only change (no spec/test surface). Committed + pushed: EFS `32f985c` on `origin/main`, superproject submodule pointer bumped (`421777465`). No mermaid CLI is installed locally, so I validated by matching the parser error to the `;` split rather than rendering; the comma substitution removes the only statement-separator tokens in the message text.

### Re: Closing docs are wet-ink + notarized, bi-directional TMO sync — opus-4-8

> **Queue prompt:**
>
> "at the Closing stage (borrower-signature API + DB columns retained)." The borrower signs & notarizes the closing paper documents. This is not e-signed. We do want to record the closing documents are signed. This will be a bi-directional sync with TMO.


Got it — corrected the doc to match. The Closing-stage signing is **not** an in-app e-signature flow:

- Borrower **signs and notarizes the paper closing documents** (wet-ink, in person).
- EFS only **records that the closing documents are signed** (a signed-status flag; the `final_docs_*` D1 columns are retained for this).
- That signed status is a **bi-directional sync with TMO** (EFS ↔ TMO) — TMO can reflect signing status back, and EFS pushes its recorded status out.

Updated `src/equityfundingsource-dev/docs/deal-pipeline-flow.md` borrower-actions §4 to drop the "borrower-signature API" e-sign framing and state the wet-ink + notarize + bi-directional-TMO-sync model. Committed + pushed: EFS `2631896` on `origin/main`, superproject submodule pointer bumped.

This refines the open `#5eq8` (closing-docs redesign): the rebuilt step records a signed/notarized flag and syncs it bidirectionally with TMO rather than capturing an e-signature. (Exchange is large — flag if you want a compact.)

### Re: Loan → Closed is an automatic bi-directional TMO state change — opus-4-8

> **Queue prompt:**
>
> /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/docs/deal-pipeline-flow.md "Close offering (deal → closed)" still is a self-closed loop. It needs bi-directional TMO sync. Either the site or TMO can set the status.


> **Queue prompt:**
>
> Loan -> Closed diagram step should be bi-directionally driven with TMO. An automatic state change.


Updated the diagram and surrounding text to match. The `Loan → Closed / Funded` step is now modeled as a **TMO-driven automatic, bi-directional state change** (EFS ↔ TMO), not a manual admin action:

- **Sequence diagram** — the step changed from `A->>A: Loan → Closed / Funded …` to `TMO-->>A: Loan → Closed / Funded (automatic bi-directional status sync)`, with closing the offering split out as the separate `A->>A: Close offering (deal → closed)` admin step.
- **§1a Closed-status note** — now states the loan reaching `Closed`/`Funded` is an automatic bi-directional TMO sync, while *closing the offering* remains the explicit admin action (`PUT /api/admin/deals/:id/close`).
- **Admin action #7** — reworded from "Close the loan / offering" to "Close the offering," clarifying the loan-status milestone is TMO-automatic and does not auto-close the offering.

Committed + pushed: EFS `58e1e49` on `origin/main`, superproject submodule pointer bumped (`204c0473c`).

### Re: deal-pipeline-flow.md — all communication with TMO is bi-directional — opus-4-8

> **Queue prompt:**
>
> :pushpin: /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/docs/deal-pipeline-flow.md All communication with TMO is bi-directional.


Made the doc consistent with the invariant. It already described offering-close, loan-status, and closing-doc signed status as bi-directional EFS ↔ TMO syncs, but framed several other TMO touchpoints as one-way. Fixed:

- **New top-level principle callout** (after the intro): every EFS ↔ TMO interaction is a two-way sync; bi-directional communication does **not** change ownership (EFS stays system-of-record/originator for loans, deals, Funding Commits).
- **1c Funding Commit** — was "written back to TMO... **not** sourced from TMO"; now "EFS-owned and EFS-originated; its funding state is a **bi-directional EFS ↔ TMO sync**" (EFS posts `AddLoanFunding`, TMO reconciles back).
- **Sequence diagram** — added `TMO-->>A: Funding reconciled (tmo_funding_rec_id)` so confirm-wire shows the return leg.
- **Lender step 5** — "reported to TMO" → "**bi-directionally synced with TMO** (`AddLoanFunding` out, funding reconciliation back)".
- **§4 system-of-record note** — "**writes back** to TMO" reframed to "**All EFS ↔ TMO communication is bi-directional**" while keeping the ownership/origination clarification.

Docs-only change, no test suite applies. Committed + pushed: EFS `a3f04cb` on `origin/main`, superproject submodule pointer bumped (`7b17ff663`).

### Re: show bi-directional arrows to TMO in the diagram — opus-4-8

> **Queue prompt:**
>
> Show bi-directional arrows to TMO in diagram in /home/brian/work/btakita/agent-loop/src/equityfundingsource-dev/docs/deal-pipeline-flow.md


Converted the sequence diagram's TMO interactions to mermaid bidirectional arrows (`<<->>`). Previously each TMO touch was a one-way `A->>TMO` request followed by a separate `TMO-->>A` response; now every TMO interaction reads as a single two-way sync arrow:

- `A<<->>TMO: Approve writeback (accepted) ⇄ Loan created (Application Received)` — collapsed the approve + loan-creation pair.
- `A<<->>TMO: Confirm wire (AddLoanFunding) ⇄ Funding reconciled (tmo_funding_rec_id)`
- `A<<->>TMO: Loan → Closed / Funded (automatic bi-directional status sync)`
- `A<<->>TMO: Close offering (deal → closed) — either side can set it`

Non-TMO arrows (Borrower↔Admin, Admin↔Lender) are left one-directional since the directive was specifically about TMO. Docs-only change. Committed + pushed: EFS `c23f2f4` on `origin/main`, superproject submodule pointer `82d67a584`.

### Re: shorten the offering-close note + bi-directional arrow — opus-4-8

> **Queue prompt:**
>
> "Offering close is a bi-directional EFS ↔ TMO sync — either the site or TMO can set it" should have shorter text and a bi-directional arrow.


The `A<<->>TMO: Close offering` arrow added last cycle already carries the bidirectional arrow, so the long `Note over A,TMO` was redundant. Trimmed both:

- Note: `Offering close is a bi-directional EFS ↔ TMO sync — either the site or TMO can set it` → `Either side can set offering close`.
- Arrow: dropped the duplicate `— either side can set it` suffix; it's now just `A<<->>TMO: Close offering (deal → closed)`.

The bidirectional sync now reads from the arrow glyph itself, with a short clarifying note. Docs-only change. Committed + pushed: EFS `ff37629` on `origin/main`, superproject submodule pointer `b4de1bfe6`.
<!-- agent:boundary:9c13ddbe -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Sync
---
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
- [/] [#cftk] [blocked:operator-credential] Refresh CLOUDFLARE_API_TOKEN in src/equityfundingsource-dev/.env with a D1-Edit-scoped token on account 1e7e194ff8da4ad9e40ce041aea84f76 (DB 280e1539). No stored credential has D1 access (tested: 10000/wrong-acct/7403/malformed); operator must create the token in the Cloudflare dashboard. .env CLOUDFLARE_ACCOUNT_ID is a literal passage: placeholder (left intact).
- [/] [#5eq8] Closing docs step (redesign): rebuild the closing-documents/notarization flow as its own step (replaces the removed Final Loan Documents section). Closing docs are wet-ink signed AND notarized on PAPER (not e-signed); EFS records a signed-status flag (+ final_docs_* D1 columns preserved) and that signed status is a BI-DIRECTIONAL sync with TMO (EFS <-> TMO), not an in-app e-signature flow. Re-introduces a notarize control and SHOULD re-gate allocation wire status (Sent/Confirmed) on notarization — the UI gate + server 409 were removed in the 2026-06-18 AvailableDeal restructure so funding is not hard-blocked in the interim.
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
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked (Cloudflare API token at `passage btak/d1/CLOUDFLARE_API_TOKEN`); migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
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
