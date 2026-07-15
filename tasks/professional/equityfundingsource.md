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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260715-183538.md`*

Compacted content:
- Archived 7 response topic(s): Remove "Wiring Instructions (Secure Link)" from the deal Funding Package tab; Loan Package "6/13" should be "13/13" + green/red tab completeness; Remove re-render jank after uploading a file on the lender Lender Documents tab; 4 more
- Prior summary/context: prior compacted content: Archived 14 response topic(s): Deal-modal tab-switch resize jank + consistent large size; De...

### Re: Is a TMO lender ID absolutely necessary? — opus-4-8

Short answer: **not for a lender to exist or use EFS, but yes at the point EFS hands funding off to TMO.** The field in question is `users.lender_account` — migration `0014_investor_lender_account.sql` literally calls it "the TMO lender account created for an investor." It's the canonical EFS↔TMO key, not an internal EFS id.

**What drives it — is it the TMO API?** Fundamentally yes. TMO identifies a lender by *its own* account number, not by an EFS user id. So the requirement shows up on both directions of the TMO integration:
- **Push (EFS→TMO):** `tmo_selected_lender_outbox_enqueue_` no-ops without it (`tmo-api/index.ts:414`), and the payload is `gated_by: ['#7a4d','#dzmm']` — the AddLoanFunding field map. TMO can't accept a funding writeback without the lender's TMO account.
- **Pull (TMO→EFS):** `user_id_by_lender_account_` is the primary, deterministic match key for reconciling TMO fundings back to EFS lenders (`tmo-sync.ts:517-526`).

**Where it's actually hard-gated in EFS:**
- Revealing wire instructions in the deal marketplace: 409 "A registered lender account is required before wire instructions can be revealed." (`investor/deal_marketplace/index.ts:1109`).
- Lender pipeline "ready" state: a lender only reaches `lender_ready` / clears `compliance_hold` once `lender_account` is set (`admin/repo.ts:1902,1921`).
- Admin save of the number: "Lender account number is required." 400 (`admin/user_management/index.ts:237`, `investor_management/index.ts:3189`).

**Where it is NOT required:**
- A lender can sign up, get accredited, satisfy requirements, browse deals, and express interest with no `lender_account` — they just sit at `requirements_current` under `compliance_hold` rather than `lender_ready`.
- TMO pull matching has a fuzzy **email fallback** (`user_id_by_lender_email_`) plus an unmatched-funding parking table (`tmo_unmatched_fundings`) for admin binding, so a funding can still be reconciled after the fact without the account pre-set.
- TMO accounts are created out-of-band — admin-managed via `lender_requests` (migration `0015`, "admin-managed TMO lender creation") — and the result is written back onto the user.

So the "absolutely necessary" requirement is really an EFS product/compliance boundary: don't reveal wire instructions or call a lender fundable until they hold a real TMO lender account — reinforced by the fact that the money actually moves inside TMO keyed by that account. It is not forced on the read/browse/express-interest paths, and the EFS→TMO writeback is currently deferred anyway (`#7a4d`/`#dzmm`).

### Re: Searchable dropdown of TMO lenders for the lender-account field — opus-4-8

Done and deployed. The free-text "TMO lender account" input is now a **searchable dropdown of the LOS/LSS lenders** everywhere an admin enters that account, and it still accepts a manually typed account (so nothing regresses if TMO is unreachable).

**What ships**
- **`TmoLenderPicker`** (new shared combobox): as you type, it filters TMO lender master records by account, name, or email; pick one and it fills the exact TMO account number. Free-text is preserved, arrow keys + Enter select, Escape closes.
- Wired into **all four** admin entry points, not just one: the lender-request **Complete Request** card, the user **Lender Profile**, the investor-pipeline **advance-lender** modal, and the deal-detail **Link TMO Account** row.
- **Backend `GET /api/admin/tmo-lenders`** (admin-guarded) → `tmo_live_lenders_R_` pages `LSS.svc/GetLenders` (`PageSize`/`Offset`), dedupes by account, returns `{account, full_name, rec_id, email}`. When `TMO_API_TOKEN`/`TMO_DATABASE` are unset it returns `configured:false` + empty list (never an error), so the field falls back to plain manual entry.

**Direction note (deliberate):** `docs/TMO/get-lender.md` previously said EFS does *not* wire the LSS lender endpoints. Your request is the explicit direction change, so I wired **`GetLenders` read-only** for this picker and updated the doc. Lender **create/update/delete** in TMO stays manual — I did not wire `NewLender`/`UpdateLender`/`DeleteLender`.

Note "LOS" here: lender master records actually live in TMO's Loan Servicing service (`LSS.svc`), which is what `GetLenders` reads; the picker surfaces those.

**Verification**
- `make check` green: **1633 pass / 0 fail**, browser + worker bundles built.
- Added tests: GetLenders paging/dedupe/sort + row normalization, picker source assertions, and updated the existing lender-link-ui markup pins.
- Committed (`a909426`), pushed, and **deployed to prod** (worker version `bbe1e5f8`). Live-verified: `/api/admin/tmo-lenders` is auth-gated (401, not 404) and the deployed browser bundle contains the picker (`api/admin/tmo-lenders`, `combobox`).

**One thing I can't check from here:** whether prod actually has TMO credentials set so the dropdown returns real lenders — that needs an authenticated admin session. Queued as `#tmo-lender-picker-verify` for a 30-second live check; the field works as manual entry regardless.

### Re: LOS lenders only — LSS is not used by EFS — opus-4-8

Fixed and redeployed. You're right — I'd sourced the dropdown from `LSS.svc/GetLenders`, and EFS doesn't use LSS at all. Switched it to a **LOS-only** source.

**What changed**
- Dropped the `LSS.svc/GetLenders` client entirely. The picker's lender list now comes from **LOS loan fundings** — the distinct lenders on `LOS.svc/GetLoanFundings` records (funding `Account` / `FullName` / `LegalVesting` / `EmailAddress`), which EFS already caches as `loan_funding` entities during TMO sync.
- `GET /api/admin/tmo-lenders` now reads those cached funding rows and aggregates distinct lenders by account (deduped, name/email backfilled across fundings, sorted). No live LSS call; still admin-guarded and `?q=` filterable.
- `docs/TMO/get-lender.md` rewritten: EFS is LOS-only; picker sourced from LOS fundings; do not wire `LSS.svc`.
- The four picker call sites (lender-request card, Lender Profile, advance-lender modal, deal link) are unchanged — only the data source moved.

**Verified**
- `make check` green (1633 pass / 0 fail); committed `bbfb694`, pushed, **deployed** (worker `c379d51b`). Confirmed live: endpoint 401 auth-gated, and the built worker has **zero** `LSS.svc/GetLenders` references.
- Checked prod D1 for the `#tmo-lender-picker-verify` populate check: the cache has **18 loans / 10 payment schedules but 0 loan-funding records**. So the dropdown is wired correctly but is **empty right now** because no loans are funded yet in the LOS — as fundings sync in, lenders appear automatically. Until then the field is manual-entry (correct fallback). Closing the verify item; the "does it list names" moment just waits on the first funded loan.

### Re: Can we create loan funding records in the API? — opus-4-8

**Yes — and EFS already does, through one existing admin endpoint.** That's also the exact write that populates the lender dropdown.

**The path that exists today**
- `PUT /api/admin/funds/:id/confirm-wire` (`admin_funds_by_id_confirm_wire_PUT_`) builds a TMO funding body (LoanNumber, `Account` = the lender's TMO account, AmountFunded, investor name/email/address) and calls `tmo_loan_funding_add_` → `POST /LOS.svc/AddLoanFunding`.
- On success it **upserts a `loan_funding` tmo_cache row** — the same rows the new lender picker reads. So the moment a wire is confirmed, that lender appears in the dropdown.

**Why prod shows 0 fundings right now:** no fund has reached confirm-wire yet. That endpoint gates on (a) a **fund existing** — i.e. a lender selected and allocated on a deal — and (b) `final_docs_ready_for_funding_at` being set (escrow/title confirmed signed docs ready). Until a deal gets that far, there are no LOS lenders to list. There's also the **pull** side: TMO sync pulls fundings from `LOS.svc/GetLoanFundings` into the same cache, so fundings created directly in TMO show up too.

**So there are three ways a funding record gets created:**
1. **EFS admin confirm-wire** (above) — the real business flow, pushes to TMO + caches locally.
2. **Directly in TMO** (LOS) — then EFS's periodic sync pulls it into the cache.
3. **The raw client** `tmo_loan_funding_add_` — what confirm-wire calls under the hood.

**If the goal is just to get the dropdown populated sooner** (before real wires happen), I can add a small admin-only "add funding / link lender" endpoint that writes a `loan_funding` record (optionally pushing `AddLoanFunding` to TMO, or cache-only for seeding) without the full ready-for-funding gate. Want me to wire that? If so, tell me whether it should push to TMO or just seed the local cache.

### Re: Queue drain — both handled above — opus-4-8

Draining the two queued prompts; both are already resolved in the responses above.

> **Queue prompt:** do [#tmo-lender-picker-verify]. "LOS/LSS lenders": I only want LOS lenders. LSS is not used by EFS. deploy

Done — picker re-sourced to LOS loan fundings (no LSS), deployed (worker `c379d51b`), `#tmo-lender-picker-verify` closed. Prod cache has 0 fundings so the list is empty until a wire is confirmed.

> **Queue prompt:** Can we create loan funding records in the api?

Answered above: yes, via `PUT /api/admin/funds/:id/confirm-wire` → `AddLoanFunding`, which also writes the `loan_funding` cache row the picker reads. Offered a direct seed/create endpoint — awaiting your go on push-to-TMO vs cache-only.
We need a bi-directional way to add a funding (with the lender (create or link to existing lender)) or to link to an existing funding. TMO already tracks fundings pre-wire, so we need this link once the loan application is accepted. This is very important but should not block deal progress.

### Re: Bi-directional funding + lender linking at acceptance — opus-4-8

Got it, and this lines up well with what's already in place — a good chunk of the substrate exists, so this is mostly connecting it, not building from zero.

**What your requirement maps onto**
- **Link to an existing (pre-wire) funding** — already have the primitive: TMO sync pulls `LOS.svc/GetLoanFundings` into cache, unmatched ones land in `tmo_unmatched_fundings` with an admin **bind** endpoint. That's exactly "TMO already tracks fundings pre-wire → link to it."
- **Add a funding with a lender (create or link lender)** — the `funds` table + `admin_funds_*` endpoints already model a lender-backed funding; the new `TmoLenderPicker` handles *link an existing lender*, and a free-text account/name handles *create*. `AddLoanFunding` carries the lender fields inline (LOS-style), so "create lender" needs no LSS call.
- **At acceptance, not at wire** — today `AddLoanFunding` only fires at confirm-wire behind the `final_docs_ready_for_funding` gate. The change is to expose add/link earlier (once `application_writeback_accept_` marks the app accepted) and keep it **non-blocking** — no new hard gate on deal progress, exactly as you said.

**Plan** (`src/equityfundingsource-dev/prds/funding-lender-linking.md`), phased so nothing blocks the pipeline:
1. **#fundlink1** — admin add/link-funding action on an accepted loan (picker = link lender, free-text = create), writes `funds` + `loan_funding` cache row, no ready-for-funding gate.
2. **#fundlink2** — link an accepted loan to an existing pre-wire TMO funding (reuse the unmatched-funding bind).
3. **#fundlink3** — bi-directional push to TMO (`AddLoanFunding`/reconcile) — stays behind the existing #dzmm/#7a4d writeback gate + idempotency so it never blocks or duplicates.

**Two decisions before I build (my assumptions in parens):**
- **D1 — "create lender":** embed the lender fields on the LOS funding (`AddLoanFunding`), not an LSS lender-master create — since EFS is LOS-only. *(Assume yes.)*
- **D2 — push timing at acceptance:** at "accepted", **link-first** — bind to a funding TMO already tracks pre-wire and keep any *new* funding local until wire — vs push a new `AddLoanFunding` to TMO right away. Your "TMO already tracks fundings pre-wire" points at link-first. *(Assume link-first; new fundings push at wire via the existing gated path.)*

Queued **#fundlink1 / #fundlink2** as actionable. Confirm D1/D2 (or just say "assumptions are fine, go") and I'll build phase 1.
<!-- agent:boundary:36d85ad0 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~do [#tmo-lender-picker-verify]. "LOS/LSS lenders": I only want LOS lenders. LSS is not used by EFS. deploy~~
- ~~Can we create loan funding records in the api?~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
- Admin loan delete: /api/admin/loans/:id
- Admin bulk loan delete: /api/admin/loans/bulk-delete
- Admin TMO loan delete: /api/admin/tmo-loans/:loan_number
- Admin TMO bulk delete: /api/admin/tmo-loans/bulk-delete
- Borrower self-delete: /api/borrower/loans/:id
- Admin user delete: /api/admin/users/:id, because loans.user_id cascades from users, then investment_deals.loan_id
  cascades from loans
---
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Syncs
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#mam7] Fractionalized loans (future): support up to ~10 lenders/loan with siloed per-lender info in the shared loan view. [followups: Fractionalized loans]
- [/] [#jzqj] Bidirectional TMO conditions sync (TMO as source of truth for conditions) + LOS lender-ID / deep-link into TMO pages. (gated on the Tue TMO screen-share + API access) [followups: TMO]
- [/] [#1067] Accreditation/suitability questionnaire: full form in the lender portal, income/net-worth options only, not a hard funding gate, admin-fillable, editable PDF + investigate DocuSign. Needs the DRE form + a DocuSign account. [followups: Accreditation]
- [/] [#nyc5] [P0] Available-deals upload: API endpoint + API key to upload investor offers to the marketplace (replaces manual YAMM) + a manual add-deal form (same template, admin-created). Needs Brian's offer-format spec. [followups: Available-deals]
- [/] [#tcb0] [P0] Go-live: publish the EFS site this week — disable/hide not-yet-functional portals, keep apply-for-loan + contact inquiries working; verify apply->TMO end-to-end. (needs operator go on which portals to hide) [followups: Go-live]
- [/] [#04v8] [recommended] Validate TMO cross-service lender visibility in a real TMO sandbox/tenant before relying on automated LOS funding writes: create or link a lender through `LSS.svc`, confirm `GetLender`, then attempt `LOS.svc` `AddLoanFunding`/`UpdateLoanFunding` on a test loan or get written TMO support confirmation that LOS funding resolves the LSS lender/vendor master `Account`.
- [/] [#apply-invite-fail-ux] Public `/apply` invite-failure UX: when the borrower invite email fails or is skipped after lead persistence, stop telling the borrower to check their inbox; show a received/follow-up message, keep the failed invite status visible to admins, and add a regression test.
- [/] [#email-bg-outbox] Durable email delivery observability/retry: record lifecycle, secure-message, broker, lending-group, investor-interest, login-link, invite, reminder, and feedback email results in D1 with status/error/retry/dead-letter instead of relying on console-only warnings for background notifications.
- [/] [#email-prod-ready] Email production-readiness hardening: provision a dedicated `CLOUDFLARE_EMAIL_API_TOKEN` secret with Email Sending scope, verify `BROKER_EMAIL_NOTIFICATIONS_ENABLED` if broker email is expected, add an admin/CI diagnostic that fails when production delivery would fall back to local capture, and document that `MESSAGE_EMAIL_INBOUND_SECRET` is only needed for the HTTP parsed-inbound route.
- [/] [#mcme] [recommended] #msg-read-status Email-open-based read receipts — DEFERRED: Cloudflare Email Sending has no open-tracking webhook, and transactional mail should stay on Cloudflare Email Sending. If email-open receipts become mandatory, design a Cloudflare-compatible tracking-pixel/event subsystem or accept portal-only read cursors. In-app portal read cursors already exist for borrower chat (`chat_sessions.*_last_read_at`) but are not email reads.
- [/] [#msg-unify4] [gated: awaiting dual-write prod soak] Lender cutover — dual-write (best-effort mirror into messages, both lender sources) + backfill (migration 0098) SHIPPED (EFS b5a6a55). Remaining: after soak, switch the inbox to read lender data from messages (dual-read), then drop deal_interest_lender_messages + lender_account_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#msg-unify3] [gated: awaiting dual-write prod soak] Borrower cutover — dual-write (best-effort mirror into messages) + backfill (migration 0097) SHIPPED (EFS 411a4da). Remaining: after the dual-write soaks live, switch the unified inbox to read borrower data from messages (dual-read), then drop chat_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#lender-card-msg-email] [recommended] Email-to-lender parity for #lender-card-msg account messages (mirror notify_lender_deal_message_): notify the lender by email when an admin posts an account message, like the deal-interest thread does. Rides the Cloudflare Email Sending / verified EFS sender gate (#f4rq). Plan: src/equityfundingsource-dev/prds/lender-card-messaging.md.
- [/] [#proprate] Lender proposal persists a DEFAULT interest rate never finalized. investor/deal_detail.tsx:362 pre-fills proposal_rate from deal.monthly_interest when deal.proposal_interest_rate==null; submit (:425-435) sends it whenever an amount is present; :421 requires a finite rate, so a lender cannot commit an amount with rate TBD — the pre-filled default (6.5% on d6b78994) persists as if finalized. Duration NOT affected (no proposal_term_months column). DATA CLEANUP DONE 2026-06-20: nulled proposal_interest_rate on the d6b78994/b8ccaa76 row (kept proposal_amount=100000 + proposal_submitted_at; D1 changes=1). CODE FIX still pending operator go: do NOT pre-fill the rate with the deal default; allow proposal_interest_rate=null ("amount committed, rate TBD"); relax :421 validation + admin LoanTermProposalSummary display.
- [/] [#nthc] [operator-verify] DEPLOYED 2026-06-20 (worker version 17b06176-fc91-44b6-b35d-1cd61beed01b; EFS 8889a4c). Remaining = operator live-verify only: open the Lender Bros group Edit overlay on deal d6b78994, bump lender2 from 0%, confirm the split saves and the "60% confirmed (partial)" badge reconciles.
- [/] [#pmhf] [recommended] Reuse the rebalancing share editor (lib/ui--browser--efs/portal/shared/lending_groups_alloc.ts: rebalance_stakes_/distribute_evenly_/lock pins) in the per-deal split UI (deal_detail allocations) so per-deal % editing gets the same 100%-balanced, lockable inputs.
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
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
- [ ] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send. (Deferred from backlog 2026-07-03; operator replaced the line — restore if unintended.)
- [ ] [#cutover-checklist] [recommended] Write (do not execute) the workers.dev to equityfundingsource.com custom-domain + Google Workspace SSO cutover checklist doc: routing for /, /funded-deals, admin routes; /login/access-callback; rollback steps. Execution stays gated on #m6ch creds. Advances #efs-cutover-sso-plan/#next-steps.
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
