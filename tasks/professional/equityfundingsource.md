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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260619-051517.md`*

Compacted content:
- Archived 9 response topic(s): What is "Funding Commits" in the Deal Pipeline dialog?; Is the Funding Commit driven by TMO?; Document the entire Deal Pipeline flow (all roles + Broker variant); 6 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): Why "Application progress" doesn't show the accepted step; Ho...

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Deployed the EFS Worker to production via the canonical `make deploy` path (build → apply remote D1 migrations → upload Worker), per `runbooks/deployment.md`.

**Pre-deploy:** submodule clean, on `main`, synced with `origin/main` (0/0); no pending D1 migrations; `make check` green — **822 pass, 0 fail**, `build` + `build:worker` succeeded.

**Deploy:** `make deploy` uploaded `equityfundingsource` (8.67s). **Version ID `73dd0840-8196-4b39-9f54-d016468b56a7`**. No new asset files to upload; no migrations to apply.

**Post-deploy verification:**
- Live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**.
- Migrations pending → **none**.

The recent doc-only changes (bi-directional TMO documentation) carried no code/schema delta, so this ships the current `main` Worker state cleanly.

### Re: TMO writeback "unavailable" — error placement (above button + toast) and root cause — opus-4-8

> **Queue prompt:**
>
> :pushpin: Why is there a "TMO writeback not sent: unavailable" error?


> **Queue prompt:**
>
> :pushpin: Admin > Deal Pipeline > Full Application dialog > Accept and send to TMO got an error "TMO writeback not sent: unavailable". The error message should appear above the button and there should be a toast.


> **Queue prompts:** (1) Accept and send to TMO error "TMO writeback not sent: unavailable" should appear above the button + a toast. (2) Why is there a "TMO writeback not sent: unavailable" error?

**(1) Placement + toast — done.** The "Accept and send to TMO" result now renders **inside the decision card, directly above the action buttons** (green note on success, red error on failure) and also fires a **toast** (`add_toast`, success/error) via the shared `ToastContainer`. Previously the only inline banner sat at the top of the detail page, which scrolls out of view inside the embedded Deal-Pipeline dialog — so a failure looked like nothing happened near the button. (`lib/ui--browser--efs/portal/admin/full_applications.tsx`: import `add_toast`; `approve_()` pushes the toast + sets the inline message; `decision_card` renders `note`/`error` above the button row.)

**(2) Why "unavailable".** It is **not** anything the server sends — it's the admin UI's last-resort string. `approve_()` does `const d = await res.json().catch(() => null)` then `set_error(\`TMO writeback not sent: ${d?.reason ?? d?.error ?? 'unavailable'}\`)`, so `unavailable` only appears when **`d` is null — the approve endpoint returned a body the client could not parse as JSON.** Every *controlled* failure (`not found`, `draft`, `not linked`, incomplete application, a rejected `NewLoan`) already returns JSON with a real `error`. The opaque `unavailable` meant an **unhandled exception escaped as a non-JSON 500**: `application_tmo_writeback_attempt_` ran its prelude **outside** its `try/catch` — the loan-number generator hits **live TMO** (`tmo_live_loan_R_`, which re-throws on any non-404/not-configured error), plus several D1 reads/writes. With TMO unreachable (flaky/gated per #dzmm) any throw bypassed the JSON path → 500 → `d=null` → `unavailable`.

**Fix:** wrap the entire attempt in one `try/catch` so the endpoint **always** returns structured JSON `{ ok:false, status:'failed', error }` (502); the catch marks failed best-effort (`.catch(() => null)`) so a D1 hiccup can't regress into a non-JSON 500. Admins now see the real reason — a TMO connectivity message, or for an already-associated loan like deal `7a0c1463` / `GQI751`, TMO's `NewLoan failed with 400: Duplicate loan number` (the duplicate-loan handling itself stays tracked in #5q7z).

**Verification:** `make check` green — **824 pass, 0 fail**. Committed + pushed: EFS `e869e73` (origin/main), superproject pointer `5cd5c7c56`. The EFS repo has no CI workflow, so local `make check` is the verification of record. Not deployed (preset was commit+push, not deploy); the UI change ships on the next `make deploy`.

### Re: Why "TMO writeback not sent: unavailable" (already answered above) — opus-4-8

> **Queue prompt:**
>
> :pushpin: deploy error message fix


> **Queue prompt:** Why is there a "TMO writeback not sent: unavailable" error?

Answered in the previous response (section **(2) Why "unavailable"**). In short: `unavailable` is the admin UI's last-resort fallback string, not a server message — it shows only when the `/approve` response can't be parsed as JSON (`res.json().catch(() => null)` → `null`). That happened because `application_tmo_writeback_attempt_` ran its prelude (live-TMO loan-number generation + D1 reads/writes) **outside** its `try/catch`, so a throw there escaped as a non-JSON 500. Fixed by wrapping the whole attempt so it always returns structured JSON `{ ok:false, error }`; admins now see the real reason. Shipped in EFS `e869e73` (origin/main), superproject `5cd5c7c56`, `make check` 824/0.

### Re: deploy error message fix — opus-4-8

> **Queue prompt:**
>
> Once an application is accepted by the admin, it should immediately be published. deploy


> **Queue prompt:** deploy error message fix

Deployed the EFS Worker to production via `make deploy`. The build picked up the `#tmowbjson` TMO-writeback fix (always-JSON `/approve` + inline error/toast above the Accept button) — wrangler uploaded the rebuilt frontend bundle **`index.browser-XPUZTLSF.js`** (1 new asset; 43 unchanged).

**Pre-deploy:** EFS submodule clean, on `main`, synced with `origin/main` (0/0); HEAD `e869e73` (the fix). No pending remote D1 migrations (`No migrations to apply!`).

**Deploy:** `bunx wrangler deploy` uploaded `equityfundingsource` (8.17s), Worker startup 28 ms, all bindings intact (DB, R2 ×3, Durable Objects, Access env).

**Post-deploy verification:** live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**. The admin Deal-Pipeline "Accept and send to TMO" flow now surfaces the real failure reason (structured JSON) with the message above the button + a toast, instead of the opaque "unavailable".

### Re: Auto-publish on acceptance + deploy — opus-4-8

> **Queue prompt:** Once an application is accepted by the admin, it should immediately be published. deploy

Implemented and deployed. When the admin accepts a full application (`POST /api/admin/full-applications/:id/approve` → TMO writeback succeeds, loan persisted, application → `accepted`), the loan is now **immediately published to the investor marketplace** as an Available Deal.

**Change (`#accept-autopublish`, EFS `d6c6651`):** `application_tmo_writeback_attempt_` calls `admin_deal_publish_(db, loan_id)` right after acceptance. It's **best-effort** — a publish hiccup is caught + logged and does NOT undo the successful TMO writeback/acceptance (re-publishable via `POST /api/admin/loans/:id/deal`); the approve response now carries a `published` boolean. `admin_deal_publish_` is idempotent (upserts the `investment_deals` row, computes terms/LTV). This is the complement of review #214f ("publish only when accepted") — accepted now implies published.

**Verified:** spec + test added (`#accept-autopublish` ordering: accept → publish → return, best-effort); worker build `done`; `make check` full suite **825 pass, 0 fail**.

**Deployed:** `make deploy` → no D1 migrations; uploaded `equityfundingsource` (Worker startup 21 ms); live smoke test `https://equityfundingsource.brian-takita.workers.dev/` → **200**. Pushed: EFS `d6c6651` (origin/main), superproject pointer `541f9a7bb`.

### Re: FTM631 still not published — keep Published, just backfill this one loan — opus-4-8

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=loan&dialog_loan_id=bbd437b0-913f-4a6f-960b-901c72a410c9 is still not published. Should we remove the Published state or make it computed based on the application being accepted?


I queried prod D1. Here's what's actually going on:

- App `94fd3f07` → `status = accepted` (accepted ~02:33am)
- Loan `bbd437b0` / **FTM631** → `tmo_loan_status = Application Received`, **no `investment_deals` row** (`deal_id`, `deal_status`, `published_at` all null) → not in the marketplace.

It was accepted **~8 minutes before `#accept-autopublish` deployed** (~02:41am), so the new auto-publish hook never ran for it. I checked the whole table: **exactly one** accepted loan has no deal — this one. Every acceptance *going forward* auto-publishes, so this is a one-time gap, not a recurring bug.

**Recommendation: keep the stored Published state — don't remove it, don't make it purely computed.** "Published" is not the same fact as "accepted":

1. The `investment_deals` row is its own entity with a lifecycle **published → funded → closed**. `funded` (`tmo_loan_status = Funded`) and `closed` (deal closed, or a loan-lifecycle change closes it) are **not** derivable from `application.status = accepted`. A computed-from-accepted flag loses the ability to close/withdraw a deal from the marketplace while the application stays accepted, and has no place for funded/closed.
2. The row **snapshots deal terms** at publish time — `sold_rate`, `min_investment_amount`, `term_months`, LTV — pulled from the TMO cache + platform settings. A computed-on-read flag has nowhere to store these and would re-derive them every read against live TMO-cache freshness.
3. Publishing fires a **side-effect** (the deal-published lender notification email) and stamps `published_at` for marketplace ordering. A boolean has no firing edge for "newly published."

So removing it or fully computing it would actually regress the deal lifecycle. The thing the "computed" idea is reaching for — *consistency* — is better gotten two ways:

- **Now (the real fix):** one-time backfill of FTM631. Two options:
  - `admin_deal_publish_(db, 'bbd437b0-…')` directly → **quiet**, no lender email (this is exactly what the auto-publish path does).
  - `POST /api/admin/loans/bbd437b0-…/deal` → publishes **and** emails lenders (`deal-published-email`).
- **Optional follow-up:** add a reconcile affordance in the pipeline loan dialog — when an app is accepted but has no published deal, surface a one-click **Publish** (or auto-reconcile on open). That gives you the consistency without throwing away the stored lifecycle.

This is a prod write that makes the loan publicly visible in the investor marketplace, so I haven't run it. **Want me to backfill FTM631 — quiet (no email) or with the lender notification?** Captured as `#ftm631-publish-backfill`.

### Re: do #e9fy — FTM631 one-time publish backfill (quiet) — opus-4-8

> **Queue prompt:**
>
> do [#e9fy]

Done. Backfilled the missing `investment_deals` row for **FTM631** (loan `bbd437b0`) so it's now an Available Deal in the investor marketplace. I used the **quiet** path — no lender email — exactly replicating what `#accept-autopublish` would have done on acceptance, which is the gap this loan fell through.

**What I did (faithful to the auto-publish path, not POST /deal):** the auto-publish hook calls `admin_deal_publish_(db, loan_id)` with **no terms input** (`applications/index.ts:1137`), so it uses `DEFAULT_PLATFORM_SETTINGS`. Traced through `deal_terms_` + `gformula.ts` against the live loan, that produces:

- `status = 'published'` — loan `tmo_loan_status = 'Application Received'` (not `Funded`).
- `term_months = 12` (constant).
- `sold_rate = NULL` — empty terms input means `sold_rate_calc_(undefined, 1)` is null. (The richer POST /deal path would pull TMO `SoldRate`/`NoteRate`, but that path also emails lenders; auto-publish does not.)
- `min_investment_amount = 10000` — default `constant` mode.
- `ltv = NULL` — onboarding has `proposed_loan_amount=100000` but **no `appraised_value`/`purchase_price`**, so `loan_ltv_calc_` returns null.

**Executed** as a single raw INSERT against prod D1 (`equityfundingsource`, `280e1539…`) via the `cf:d1:query:remote` wrapper — no `notify_email_`, so **no lender notification fired**:

- New deal `d6b78994-0970-498f-9e10-6f0035902c0d`, `published_at = 1781852961` (now), `created_at`/`updated_at` matched, `closed_at` NULL.
- `changes: 1`; verified the row reads back with the exact field set above and joins to FTM631.

**Pre-flight confirmed exactly one gap, as expected:** before the write, `deal_count = 0` for this loan (INSERT branch, not the UPDATE branch), consistent with the earlier finding that FTM631 was the single accepted loan with no deal. Stored Published state is preserved — I did **not** remove it or make it computed.

**Scope notes:** this is a one-time data backfill, not a code change — the EFS working tree is clean, so the queue preset's `#spec-test-commit-push` steps (spec/test/commit/push) are N/A; there's nothing to commit or deploy. (Separately, preflight flagged a `stale_install` for the agent-doc binary itself — unrelated to EFS; leaving it for a post-session `make install` rather than rebuilding against the live supervisor.)
<!-- agent:boundary:afae9a37 -->
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
- do [#e9fy]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [x] [#e9fy] #ftm631-publish-backfill [recommended] One-time backfill: loan FTM631/bbd437b0 (app 94fd3f07) is accepted but was accepted ~8min BEFORE #accept-autopublish deployed, so it has no investment_deals row. Exactly 1 such loan exists. Publish it via admin_deal_publish_ (quiet, no lender email) or POST /api/admin/loans/bbd437b0.../deal (emails lenders). Keep stored Published state; do NOT remove or fully-compute it.
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
