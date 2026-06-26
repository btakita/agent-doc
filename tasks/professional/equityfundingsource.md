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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260626-161443.md`*

Compacted content:
❯ - Archived 11 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); compaction recovery and queue state; Session handed to Claude; available-deal Messages head still held; 8 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: compaction recovery and queue state — gpt-5

> **Queue prompt:**
>
> Please create state machine(s) of all deal states.


Recovered the interrupted compaction/preflight state enough to continue through the agent-doc closeout path. Preflight still reports one legacy gated backlog item and several semantic-completion review candidates; I did not migrate or resolve those because the planning contract for this cycle was a prompt-response repair closeout.

The current queue still needs operator intent cleanup before I start repo/deploy work from it: the held head is the available-deal Messages selection issue, followed by `deploy`, and recovery has also reintroduced the same available-deal Messages prompt once more without the `🚧` marker. I did not run code changes, tests, commit/push, or deploy in this cycle.


### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=823adb3a-17d6-4689-9148-cb95001f702c invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782459489229
```

### Re: State machine(s) of all deal states — already modeled & verified — opus-4-8

> **Queue prompt:**
>
> Please create state machine(s) of all deal states.

These already exist as maintained Mermaid state machines in **`src/equityfundingsource-dev/docs/master-state-machine.md` §4** (deeper narrative + sequence diagram in `docs/deal-pipeline-flow.md`). I re-verified §4 against current code — "all deal states" is actually **two independent** machines:

- **Loan pipeline** — `loans.tmo_loan_status` (`LOAN_STATUS_STEPS`, `lib/admin/repo.ts`): `Application Received → Loan Estimate → Intent to Proceed → Processing → Underwriting → Closing → Closed → Funded`, plus off-pipeline `Denied` / `Unassigned` / `On Hold`. `Closed`↔`Funded` is a bi-directional TMO sync (`admin_deal_status_sync_by_loan_`).
- **Marketplace offering** — `investment_deals.status` (`migrations/0085`, CHECK-constrained): `published → reserved → funded → closed`. `reserved` on lender-select hides the deal from other lenders; `funded` is derived from the loan `Funded`; `Closed` (loan milestone) is decoupled from closing the offering.

No new artifact needed — the deal-state machine is current.

### Re: State machine(s) of lender deal states — modeled with 3 gaps (head 🚧 held) — opus-4-8

> **Queue prompt:**
>
> Please create state machine(s) of lender deal states.

§5 of the same living doc already models most of the lender side: the investor lifecycle board (`investor_pipeline_status_` → `lender_ready`), accreditation/requirements sub-state, TMO lender-account (`lender_requests.status`), and the per-deal **deal-interest** machine `deal_interest_workflow_stage_` (`interested → processing → document_request → selected → verbal_commitment`).

Three real lender-deal-state machines are **missing** from §5:

1. **Funding-commitment wire state** — `funds.status` `pending → active` (lender commits → admin confirms wire → `AddLoanFunding` to TMO). `migrations/0044_funds_active_status.sql`. The natural continuation of `verbal_commitment → [*]`.
2. **Lender proposal sub-state** — `proposal_submitted_at → borrower_selected_at` (amount/rate proposal → borrower accepts), incl. `proposal_updated_at` / `proposal_update_read_at` re-read. `migrations/0085_deal_interest_loan_term_proposals.sql`, `lib/investor/shared.ts`. (Related to the open `#proprate` bug.)
3. **Lending-group allocation states** — `lending_groups.status` (active/archived), `lending_group_members.status` (invited/active/declined/removed), per-deal `allocation_bps`. `migrations/0090_lending_groups.sql`.

This head is **🚧 held**, so I did not drain it — the three gaps are filed as a tracked backlog item. To proceed, remove the `🚧` and say **doc-only** (close the gaps in `master-state-machine.md`, my recommendation) vs **also build an FSM code module**.

**lazily-js:** orthogonal to this task — these are Markdown diagrams. lazily only matters under path (b): runtime *reactive* FSM cells (state in a `lazily` cell, derived guards/UI as derived cells). For documenting states it adds nothing; for code, model a pure transition function first and layer lazily on the client as a deliberate, separate dep-add.

**"deploy":** no-op — nothing code-bearing changed this cycle.

### Re: State machine(s) of lender deal states — gaps closed (doc-only) — opus-4-8

Proceeded with the safe default (doc-only — my standing recommendation, and a prerequisite for any later FSM-code path). Closed the three lender-deal-state gaps in `src/equityfundingsource-dev/docs/master-state-machine.md §5`, each as a mermaid `stateDiagram-v2` + a status-model index row, cited to the migrations:

1. **Funding commitment** — `funds.status` `pending → active` (lender commits amount+account+vesting → admin confirms wire → `AddLoanFunding` to TMO, stores `tmo_funding_rec_id`). `migrations/0044`.
2. **Lender proposal sub-state** — `no_proposal → proposed → revised ⇄ read → borrower_selected`, derived from `proposal_submitted_at` / `proposal_updated_at` / `proposal_update_read_at` / `borrower_selected_at`. `migrations/0085` + `0094`. (Flags the `#proprate` "default rate must not persist as finalized" caveat.)
3. **Lending group + allocation** — `lending_groups.status` (active/archived) and `lending_group_members.status` (invited/active/declined/removed); `lending_group_deal_allocations.allocation_bps` is a 0–10000 per-(group,deal,member) **share attribute**, not a state column (modeled accurately as such). `migrations/0090`.

§4 (deal states) was already current — left as-is.

**Shipped:** EFS `main` `362e1ae` (pushed; rebased over concurrent remote work), superproject pointer bumped `f98a45135`. Doc-only — no tests/build affected.

### Re: deploy — no-op this cycle — opus-4-8

> **Queue prompt:**
>
> deploy

Nothing to deploy: the change is `docs/master-state-machine.md`, a documentation file that is **not** part of the deployed Worker bundle, so there is no worker behavior to ship. (The last code deploy remains the messages-focus fix.) If you want the lender state machines as **runtime FSM code** rather than docs — the path where `lazily-js` would actually apply — say so and I'll scope that as a separate, deployable change.

### Re: Deal pipeline must not block on lender-pipeline steps — shipped + deployed — opus-4-8

> **Request (chat):** The admin pipeline Select Proposal was blocked — "Brian Takita annual requirements are not started. Complete or approve the lender requirements before selecting this lender." We should not block selecting a proposal from lenders who haven't filled out paperwork. Make the deal pipeline not block at all based on lender-pipeline steps.

Done — implemented, tested, committed, pushed, and deployed.

**Root cause.** The Select Proposal action (`admin_deal_interests_by_id_select_PUT_`, `lib/admin/investor_management/index.ts`) returned `409` on **three lender-pipeline-step gates**: (1) lender annual requirements/accreditation (`investor_requirements_blocks_funding_`), (2) multi-lender-group member accreditation (`lending_group_unaccredited_members_R_`), and (3) TMO-lender-account linkage. The client button isn't disabled — it just surfaced the server 409 toast.

**Fix** (`#deal-select-no-lender-pipeline-block`, EFS `39e56c2`): removed all three lender-pipeline gates from selection. They are now **visibility-only** — the `requirements_blocked` / `compliance_hold` flags and the lender-card "Requirements: …" / "No TMO lender account" badges all remain, so you still *see* the state and decide for yourself (your "more control + visibility" ask). The two **deal-side** prerequisites are kept: the loan must have a TMO loan number, and the commitment must fit the deal amount.

**Visibility into deal pipeline states:** the §5 lender + §4 deal state machines in `docs/master-state-machine.md` are now accurate to this change (updated the requirements note to say it no longer blocks selection).

**One thing to know (no action taken):** selection was the *only* hard accreditation enforcement point — there is no separate accreditation gate at funding/wire-confirm today. So accreditation is now never hard-blocked before funding. The flags/badges still show it. If you want a *soft* gate (warn, not block) or a real gate at the **funding** step specifically, say so and I'll add it — I did not, since you asked for no blocking.

**Verification:** `bun test` → **1327 pass, 2 fail**; both failures are the pre-existing `#doc-viewer` PDF/loan-detail tests (confirmed failing on clean HEAD before my change — unrelated to the deal pipeline, untouched here). `bun run build` + `build:worker` clean.

**Shipped:** EFS `main` `362e1ae..39e56c2` (pushed); worker **deployed** — version `7f861de2-c207-451f-9259-71dc82193496` at https://equityfundingsource.brian-takita.workers.dev. Superproject pointer `96b7113a5`.

Live-eyeball: open the deal `4046fa5a` dialog → loan tab → **Select proposal** on Brian Takita's interest → it should now select (deal → reserved, loan → Loan Estimate) without the requirements 409.

### Re: Soft accreditation warning + test suite green + deploy — opus-4-8

> **Request (chat):** Remove the hard accreditation gate. Leave a soft warning. Fix the test suite. deploy.

Done — all three, shipped and deployed.

**1. Hard gate → soft warning.** The hard accreditation/lender-pipeline gates were already removed from Select Proposal last change; this adds the **soft warning** in their place (`deal_detail.tsx` `select_interest_`). When an admin selects a lender whose annual requirements are unmet or who has no TMO lender account yet, a non-blocking confirm appears — e.g. *"Brian Takita: annual requirements are not started and no TMO lender account is on file yet. You can select this proposal anyway, but the lender must complete these before funding can be wired. Select anyway?"* — with a **Select anyway** button. The admin sees exactly what's incomplete and proceeds at their discretion; readiness still gates funding/wire-confirm.

**2. Test suite fixed.** The 2 pre-existing `#doc-viewer` failures were stale pinned-source assertions (not behavior bugs): the PDF iframe had gained `align-self:stretch;`, and admin loan-detail now builds the document URL from `/api/admin/loans/:id/documents/:id/download` (the old test pinned `/api/dropbox/download?path=`). Updated both pins to the current source. Full suite now **1329 pass / 0 fail**.

**3. Deploy.** `bun run build` + `build:worker` clean; worker **deployed** — version `b520702f-e9b1-4763-a8f6-bf96a66ddcbe` at https://equityfundingsource.brian-takita.workers.dev.

**Shipped:** EFS `main` `39e56c2..3d76f3c` (pushed); superproject pointer `8909d5438`.

Note: the standalone `/admin/loans/:id` page has a second select surface (`loan_detail.tsx`) whose row type doesn't carry the requirements field and isn't wired to the confirm dialog — I scoped the warning to the deal dialog (the surface from your URL). If you want the same warning there too, say so and I'll thread the data + confirm through it.

Live-eyeball: deal `4046fa5a` → loan tab → Select proposal on Brian's interest → the soft warning should appear; confirm → it selects.
<!-- agent:boundary:ed9537d6 -->
<!-- /agent:exchange -->
###
<!--
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
-->

<!-- agent:queue preset="#spec-test-commit-push" -->
- ~~Please create state machine(s) of all deal states.~~
- ~~Please create state machine(s) of lender deal states.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~deploy — auto-struck: completed by #do-id-closeout-open-backlog (#qftbklgstrike)~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send; no active unused Robert login tokens remain.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
