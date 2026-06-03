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
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `734df89a-8af6-401e-83e4-fd5215a48e3f` as of 2026-05-28 from EFS commit `0afd62b`; public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260529-004005.md`*

Compacted content:
- Archived 4 response topic(s): [#2z8r] custom addresses; use recommendation [#2z8r]; use recommendation [#2z8r]; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
❯ Please review the TMO provided links.
❯ - https://www.borrowersviewcentral.com/portal/applynew?id=EFSI
❯ - https://www.borrowersviewcentral.com/portal/apply4?id=EFSI

❯ Should these replace our borrower onboarding/portal for the demo?

Can we replicate the behavior of these workflows with the onboarding/portal?

### Re: TMO BorrowerView links — what they are — opus-4-8

Both are **The Mortgage Office "BorrowerView Central"** hosted intake forms, served from TMO's `borrowersviewcentral.com` and branded with the EFSI logo (`Logos\EFSI_Small.gif`). Important: submissions land in **The Mortgage Office's** system, not in our D1.

They are two different application templates:

- **`applynew` — short business-purpose / private-money app.** Referring-broker block, then borrower(s) (Primary / Co-Borrower / Guarantor / Co-Signer, SSN, DOB, marital status), loan amount + use of funds, property(ies) with liens/encumbrances (Commercial, Land, SFR, Est. Market Value, ARV), and a **"Declaration of Non-Owner Occupancy & Business Use of Proceeds"** + typed-initials e-signature. This is the non-owner-occupied, business-purpose flow — explicitly *not* a consumer loan (exempt from TILA / SAFE Act / HPA). It mirrors EFS's hard-money model.
- **`apply4` — full consumer URLA / 1003.** Personal info + citizenship + credit type, 2-year address history, employment/self-employment + gross-monthly-income breakdown, other-source income, assets & liabilities, subject property + occupancy, Real Estate Owned schedule, Declarations A–M (bankruptcy, foreclosure, judgments, lawsuits), and an owner-occupied "household purpose" declaration + typed-initials e-signature. This is the standard residential 1003, owner-occupied flow.

### Re: Should these replace our portal for the demo? — opus-4-8

**No — keep the EFS-built `/apply` as the demo front door.**

- Our `/apply` is a lightweight public **prequalification** that writes leads into our own D1 `application_leads` table with an admin review pipeline. That EFS-owned data + admin workflow is the whole demo thesis — and the foundation for the OCR / document-intelligence and TMO-replacement roadmap (backlog 0e1p, 7fx4, 7a4d).
- The BorrowerView forms are **TMO-hosted**: submissions go into The Mortgage Office, not EFS. Demoing them puts TMO's own product on screen and undercuts the "EFS-owned platform that can eventually replace TMO" story we're selling.
- Their real value is as a **reference spec**: canonical field set, section structure, declarations, and legal / e-sign language for the full intake we eventually build. Mine them; don't front them.

Nuance: if the demo must show a *full* application (beyond prequal) before our own exists, we can link out to the TMO form as the "official application" step *after* prequalification — but the EFS portal stays the entry point and the data owner.

### Re: Can we replicate these workflows in our portal? — opus-4-8

**Yes — the form / data-capture side is low-risk; the gating work is the TMO writeback, already tracked.**

- **`applynew` (business-purpose)** is close to what we already have. Extend `application_leads` with `borrowers[]` (+ type), loan use, `properties[]` with `liens[]`, and the business-use declaration + e-sign. Small build.
- **`apply4` (consumer URLA)** is a much larger form (employment/income, assets/liabilities, REO, declarations A–M) but fully standard — the Fannie/Freddie 1003. Replicable, bigger effort.
- **E-signature:** they only "type initials." To make ours defensible, add an ESIGN/UETA consent checkbox + audit trail (timestamp, IP) — a small improvement over theirs.
- **Status-check-by-email** maps onto our existing Borrower Login (already behind Cloudflare Access).
- **The real constraint is the integration, not the UI.** BorrowerView writes straight into TMO's LOS. If our form captures into D1, we still need the writeback/sync into The Mortgage Office — exactly what's pending on TMO's webhook/API answer (backlog dzmm, k2xg) and the field-extraction to TMO create/update flow (7a4d). Replicating the *form* is easy; replicating the *integration* depends on TMO's outbound support, still unconfirmed.

**Bottom line for the demo:** mirror **`applynew`** (matches EFS's hard-money business and our current schema), keep `/apply` prequal as the front door, and treat the consumer `apply4` 1003 as a later phase gated on the TMO writeback decision.

### Re: spec-test-commit-push queue (7tan / 88qv) — opus-4-8

> **Queue prompt:**
>
> [#7tan]
>
> [#88qv]


Done — scope per your call: **spec/PRD doc only, deduped, run once.**

**Duplicate resolved.** 88qv was an accidental dup of 7tan (my earlier failed finalize's `--pending-add` landed anyway). Both queue items are now closed — 7tan as the real deliverable, 88qv as the duplicate. No work ran twice.

**Spec delivered:** `prds/efs-full-application-intake-spec.md` in `equityfundingsource-dev` (commit `ed124f8`, pushed to `main`). Design-only contract (no app code), companion to the portal PRD, covering:

- The two variants — `applynew` (business-purpose, **Phase 1**) and `apply4` (consumer URLA/1003, Phase 2) — with section/field breakdowns taken from the live forms.
- Proposed D1 data model: `applications` + borrowers / properties / liens / signatures tables; Phase-2 employment / income / assets / REO / declarations.
- TMO writeback mapping to `NewLoanApplication` / `UpdateLoanApplication` (`Fields[]` key-value, `OnlineStatus` enum) — staff-reviewed and gated, never automatic.
- ESIGN/UETA e-signature + audit trail (timestamp, IP, declaration version) improving on TMO's typed-initials-only capture.
- Status tracking via the authenticated Borrower Login; phasing; non-goals; open gating items (TMO outbound support + EFS custom-field key map — dzmm / k2xg / 7a4d).

**Verification:** additive docs-only change — no code, build logic, or instruction surface touched — so the worker build/test gate (`make check` = `bun test` + worker build, which needs Cloudflare bindings) wasn't run; it wouldn't exercise a standalone markdown file. The spec is committed and pushed; remote `main` is `ed124f8`.

Queue kept active per your choice; both items are now drained, so the auto-loop terminates next cycle.
❯ What are #next-steps to implement the borrower portal? Also, did you see the TMO report when submitting the application?
### Re: did you see the TMO report on submit? — opus-4-8

No — I didn't submit an application, so there was no TMO report to see. Last cycle I only rendered the two **public** BorrowerView forms read-only (a chromium markdown dump) to inventory their fields; I never filled or submitted them, and I didn't exercise the EFS portal's own submit flow.

I have since read the submit path, so here's what the "TMO report" actually is in our code (`lib/loan_applications/routes.ts`). On a borrower new-application submit we POST to TMO `NewLoanApplication` via `tmo_post_`, then:

- parse the response object (`Data`, `Status`, `ErrorMessage`, `ErrorNumber`);
- **reject** if `Status !== 0` or `ErrorNumber` is set (surfaced as "TMO rejected the loan application");
- otherwise extract the **Loan Application Reference Number** (from `LoanApplicationReferenceNumber`, or a `Loan Application Reference Number: <x>` text match) and store it on the loan.

That reference-number response is the submission "report." If you want a real sample, I can run a test submission through the dev portal against the TMO simulator (`lib/tmo-sim`) — or the TMO API sandbox — and capture the response. I held off because that's a state-changing call I shouldn't make unprompted.

### Re: next-steps to implement the borrower portal — opus-4-8

Per `docs/current_progress.md` and the code, the borrower portal is **mostly built** — dashboard, new-application (live TMO `NewLoanApplication` submit + Dropbox doc upload), application-status tracking, documents, settings/profile, notifications, and D1-backed secure messages all exist. Production infra is largely done too (review item next-steps: D1 token, migrations 0025/0026, Cloudflare Access).

Remaining work, prioritized:

1. **Payments sections are UI-only.** `make-payment` and `payment-history` are registered borrower sections with no backend — no schedule/history wiring, no payment processing. Decide: read-only payment **schedule + history from TMO** (`docs/TMO/payment-schedule-field.md` + `GetLoanFundings`) vs. actual collection (ACH/Stripe). For a hard-money lender where TMO/trust does servicing, read-only-from-TMO is the likely MVP. *(added to backlog)*
2. **Richer full-application intake.** The current new-application is the short onboarding flow. Build it out to the EFS-owned full application per the spec just shipped (`prds/efs-full-application-intake-spec.md`): Phase 1 = `applynew` business-purpose fields + ESIGN/UETA e-sign + audit, then `apply4` (1003). Form capture is low-risk; the TMO field-key writeback map is the gate (dzmm / k2xg / 7a4d).
3. **Borrower message email round-trip.** Your earlier (commented) note asked for an admin owner for borrower-portal messages, an email notification when a borrower sends a message, email replies routed back into the chat, and borrower emails when an admin replies — a concrete enhancement on top of the existing D1 chat. *(added to backlog)* Gated on the Resend EFS sending domain (f4rq) and ideally Google Workspace SMTP (m6ch / p40t).
4. **Notifications delivery.** Switch `APPLICATION_LEAD_NOTIFICATION_*` off Resend testing-mode to an EFS-verified domain (existing f4rq).
5. **Production hardening (existing, gated):** custom-domain cutover (icebox e1kb) and true Google Workspace SSO (m6ch).

Want me to pick one and queue it with spec-test-commit-push? Highest demo value is #1 (payments read-only-from-TMO) or #3 (message email round-trip).

### Re: e245 borrower payments — done — opus-4-8

Implemented read-only-from-TMO (the MVP from the item) and shipped to `equityfundingsource-dev` (commit `e2abc2b`, pushed to `main`).

**What was wrong:** both borrower sections rendered static mock — `payment-history` had a hardcoded `MOCK` array (fake ACH refs, `$2,621.75` rows) with a footer *falsely* claiming it was TMO-sourced; `make-payment` was an entirely fake form (hardcoded balances + a submit that just flips to "Payment submitted" and does nothing). Meanwhile the backend already had it all: `loan_payments` table, TMO `PaymentSchedule` sync (`tmo-sync.ts`), and a loan-detail endpoint returning real payment rows.

**What I changed** (`lib/ui--browser--efs/portal/borrower/`):

- `payment_history.tsx` — fetches `/api/borrower/loans` + `/:id` and renders the real `loan_payments` schedule via the shared `PaymentScheduleTable`, with loading/empty/error states and a real paid-count/total. Mirrors the existing `loan_summary` pattern. `MOCK` removed.
- `make_payment.tsx` — **product decision under the MVP:** EFS does not collect payments (TMO/the trust account services the loan), so I replaced the fake form with the real next-due amount/date from the schedule plus remittance instructions, and removed the fake submit / "Payment submitted" flow. ACH/Stripe collection stays the deferred non-MVP path.
- PRD: documented the read-only-from-TMO borrower-payments requirement.
- Tests: `tests/borrower-payments-ui.test.ts` asserts real-data fetch and absence of mock.

**Verified:** `bun run check` green — 229 tests pass, app + worker build clean.

Next in queue: **xcvg** (borrower message email round-trip). Heads-up that it's gated on the Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch) — actual email send/receive can't fully ship until those creds exist, so I'll build what's possible and gate the rest.

### Re: Stripe + TMO payments — can we do both? — opus-4-8

Yes, technically — and they're complementary, not exclusive. TMO stays the servicing/ledger source of truth; Stripe becomes the online collection rail. This is exactly the "ACH/Stripe collection" path I deferred in the e245 MVP.

Flow:

1. Borrower pays online via Stripe (ACH or card) from `make-payment`.
2. Stripe fires a `payment_intent.succeeded` (or `charge.succeeded`) webhook → the EFS Worker verifies the signature, records the receipt in a new EFS D1 table (e.g. `loan_payment_transactions`), and marks the matching `loan_payments` schedule row paid (read-only views update immediately).
3. EFS pushes the payment to TMO so TMO's ledger reflects it.

**"If a Stripe payment is made, can we update TMO?"** — that's step 3, and it's the open question. The TMO endpoints we have documented (`docs/TMO/`) cover loan/application writes, investor `AddLoanFunding`, and schedule/funding reads — I don't see a borrower-payment/receipt **write** endpoint. So:

- If ABS/TMO exposes a "record payment/receipt" API → real-time Stripe→TMO posting under the same staff-reviewed/gated writeback pattern as the rest (the dzmm / k2xg TMO-writeback gate).
- If it doesn't → EFS records the payment in D1 and shows it immediately read-only; the TMO ledger update falls back to batch/manual reconciliation until a payment-write API is confirmed.

Caveats worth flagging before building: Stripe ACH for loan repayment carries KYC / processing-agreement requirements, and reconciliation (partial payments, NSF reversals, processor fees) needs explicit handling. I'd confirm with ABS/TMO whether a borrower-payment write endpoint exists first — added to the backlog.

### Re: xcvg (message email round-trip) — gated, need a call — opus-4-8

`xcvg` is still queued but gated on creds I don't have: the Resend EFS sending domain (f4rq, not yet verified) and ideally Google Workspace SMTP (m6ch, awaiting Google admin creds). I can't fully ship send/receive without a verified sending domain + inbound routing (Resend inbound or Cloudflare Email Routing → Worker).

Two options — your call:

- **(a) Scaffold now, gate the send:** build the D1 admin-owner assignment for borrower-portal threads + notification triggers (borrower-sends → admin notified; admin-replies → borrower notified) wired to the existing chat, with actual email send/receive behind a feature flag until the domain is verified. Spec + tests + commit.
- **(b) Defer** `xcvg` until f4rq (and optionally m6ch) land.

I held rather than auto-implementing a gated feature.

*(Recovery note: the earlier e245 finalize hit a queue-consume divergence from a live edit; e245 is fully committed — code `e2abc2b`, response in HEAD — and I cleared the stuck queue line. Also: the exchange is getting large; say the word and I can `agent-doc compact` it.)*
<!-- agent:boundary:bc6257f2 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#cm3e] [recommended] Borrower online payment collection (Stripe) + TMO sync: add ACH/card payment in the borrower portal make-payment section; on Stripe payment_intent.succeeded webhook, verify signature, record the receipt in a new EFS D1 loan_payment_transactions table, reconcile the matching loan_payments schedule row, and push the payment to TMO. GATE: confirm ABS/TMO exposes a borrower-payment/receipt WRITE endpoint (current docs/TMO cover loan/application writes, investor AddLoanFunding, and schedule/funding reads only) — ties into the TMO-writeback gate (dzmm/k2xg). Deferred non-MVP collection path from e245; handle KYC, partial payments, NSF reversals, fees.
- [ ] [#xcvg] [recommended] Borrower message email round-trip: add an admin owner for borrower-portal secure messages, email notification when a borrower sends a message, inbound email reply that appends into the D1 chat, and borrower email when an admin replies. Builds on existing chat_sessions/chat_messages. Gated on Resend EFS sending domain (f4rq) + Google Workspace SMTP (m6ch/p40t).
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
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
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
