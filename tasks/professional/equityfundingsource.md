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

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260626-040603.md`*

Compacted content:
❯ - Archived 14 response topic(s): Deal image carousel across portals (house placeholder); do [#dealcarousel-borrower]; do [#dealcarousel-broker]; 11 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=f9eedbbe-320d-4720-9bca-2856e0b7378a invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782446080826
```

### Re: contextual lender chat — holding for the complete spec — opus-4-8

> **Queue prompt:**
>
> In the lender portal, make the chat contextual. So if a lender is viewing an available deal or a interested in deal, show the thread releating to that deal. Outgoing email threads will have the property as the subject.


Recovered the session first: the doc had just been compacted (archive `…-20260626-040603.md`), so the snapshot/CRDT still held the full pre-compact exchange (37KB) while the visible file was the 20KB compacted version — `reset --from-current --preserve-session` re-aligned the sidecars and the compaction is now cleanly committed to HEAD.

I'm **holding** rather than building, because the queue spec is still being written:

1. `🚧` **In the lender portal, make the chat contextual** — when a lender is viewing an available deal or an interested-in deal, show the thread relating to that deal; outgoing email threads use the property as the subject. *(held by your 🚧 marker)*
2. **"If the lender does not have a deal open, show all communication. Full screen message dialog wi…"** — this line is cut off mid-word, so the fallback/full-screen behavior isn't fully specified yet.
3. `deploy`

What I've got so far: deal-scoped thread when a deal is open, property name as the email subject, and an all-communication view with a full-screen message dialog when no deal is open. Finish item 2 (and clear the `🚧` on item 1 when you're ready) and I'll build it under the `#spec-test-commit-push` preset. I did **not** auto-drain — the head is explicitly held and the spec is incomplete.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=3ae68396-d61c-4583-b743-3b1b227962af invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782446978966
```

### Re: contextual lender chat — full spec captured, holding on 🚧 — opus-4-8

> **Queue prompt:**
>
> If the lender does not have a deal open, show all communication. Full screen message dialog with each thread as a tab.


My earlier hold response just landed (recovered from the degraded-IPC orphan, committed `0bde5b23e`). Recovering it caused the binary to auto-strike the head — "make the chat contextual…" — as *consumed*, and to auto-strike "deploy". **Neither is actually done.** The contextual-chat directive is still to-build; its detailed requirements are the items below it, and `deploy` stays gated behind building.

The spec now reads complete and coherent. My consolidated understanding of the **contextual lender-portal chat**:

- *(struck headline)* Make the lender chat contextual; outgoing email threads use the **property name as the subject**.
- **No deal open** → full-screen Messages dialog, **each thread as a tab**.
- **Deal open** → show all threads but **auto-select the property thread**; if that thread has no messages, still show it for the deal; if the lender hasn't expressed interest, the deal thread is **hidden** unless the lender sent a message or expressed interest.
- A **"General"** thread for all other EFS correspondence (general questions, accreditation help, etc.) is the **first tab** in the Messages dialog.

The head is still **🚧-held**, so I have **not** started building. Clear the 🚧 (and correct anything above that's off) and I'll implement the whole feature as one unit under `#spec-test-commit-push` — build + tests + push, with `deploy` held for your separate go. I'm not auto-looping while the head is held.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=6d46759b-e939-44c3-a8e6-b94ce5457022 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782447206996
```

### Re: contextual lender chat — shipped (all queue items) — opus-4-8

> **Queue prompt:**
>
> The general thread is for all other correspondence with EFS. Such as general questions, help with the Accredation process, etc. This general thread should be the first tab in the Messages dialog.


> **Queue prompt:**
> If the lender has a deal open, show all threads but select the property thread. If there are no messages in the thread, show it for the deal. If the lender does not express interest, the deal thread should not show up unless the lender sent a message or expressed interest.

> **Queue prompt:**
> The general thread is for all other correspondence with EFS. Such as general questions, help with the Accreditation process, etc. This general thread should be the first tab in the Messages dialog.

Built, tested, and pushed — EFS `b23dd00` on `origin/main`. The lender (investor) chat launcher is now a **full-screen tabbed Messages dialog** instead of a single thread.

**What ships:**
- **"General" tab, first** — all non-deal correspondence (onboarding, accreditation, general questions). Backed by the existing account-scope aggregate via `/api/investor/messages?scope=account`.
- **One tab per deal thread**, labelled by the **property**. A deal tab is visible only when the lender **expressed interest** OR **sent a message** about it — exactly your rule.
- **Deal open** (lender on a deal page) → that deal's property thread is **merged in even if absent** from their thread list (no interest, no messages yet) and **auto-selected**; an empty thread still shows so they can start chatting about the deal.
- **Outgoing deal emails use the property as the subject** (`[EFS] {property}`), via a new native lender→deal message store (`party=lender, scope=deal`) that needs no investment-interest row — so "sent a message" and "expressed interest" stay distinct triggers, with no double-counting against the existing admin deal-interest thread (proven by test).

**Server:** `GET /api/investor/messages?scope=account|deal&deal_id=` (no-scope response unchanged), `GET /api/investor/deal-threads`, `POST /api/investor/deals/:id/messages` (gated on deal visibility). All logic in pure unit-tested helpers.

**Verification:** `bun run check` green — **1294 tests pass** (26 new), browser + worker build clean.

**One deviation (filed):** deal-tab compose is **text-only** for now — the lender-scoped unified attachment **download** route isn't mounted yet, and shipping broken download links would be worse. Admin replies with attachments still render. Backlogged as `#lenderdealattach`.

**Deploy:** held per the `#spec-test-commit-push` preset — ready via `bun run deploy:worker` on your go.

### Re: do [#lenderdealattach] — lender deal-tab attachments shipped — opus-4-8

> **Queue prompt:** do [#lenderdealattach]

Done (EFS `7fade6c`, pushed to `origin/main`). This closes the one deviation from the contextual-chat build — deal-tab compose is no longer text-only.

- **Upload:** `POST /api/investor/deals/:id/messages` now accepts multipart attachments (5 files / 3.6 MB), reusing the lender-account/borrower upload guards + R2 storage + document records.
- **Download:** new `GET /api/investor/message-attachments/:id/download` (lender auth), gated by a pure, fail-closed predicate `lender_message_attachment_download_allowed_(message, lender_id, deal_visible)` — serves bytes only when the lender **owns** the message's thread OR it's a deal-scope message for a deal the lender **can view** (same `deal_message_visible_` gate as the POST). Wrong-lender / non-visible-deal both 404 (tested).
- **Wiring:** the unified read path rewrites native lender→deal attachment `download_url`s to the lender route (no double-count); the deal tab flips to `attachments: true`.

**Verification:** I re-ran `bun run check` myself — **1310 pass, 0 fail**, browser + worker builds clean. Reviewed the download handler + predicate directly before pushing.

**Deploy:** still held per `#spec-test-commit-push` — the full contextual lender chat (threads + tabs + property-subject emails + attachments) is ready via `bun run deploy:worker` on your go.

### Re: deploy — contextual lender chat live — opus-4-8

> **Queue prompt:** deploy

Deployed to production (you confirmed the held `🚧 deploy`). The full contextual lender chat is now **live**.

Ran `bun run deploy:worker` on `7fade6c` (`origin/main`, working tree clean):
- **Worker upload:** `equityfundingsource` uploaded (6.69 s), all bindings intact (D1, R2, Durable Objects, message-email vars).
- **D1 migrations:** none to apply — the feature rides the existing unified `messages` / `message_attachments` tables.
- **Live:** https://equityfundingsource.brian-takita.workers.dev — Current Version ID `ae98ab33-8c3a-4c49-a91b-5e8b50a4d1af`, hourly schedule intact.

Lenders now get the tabbed Messages dialog (General first + per-deal property threads), deal-open auto-select, property-as-subject outgoing emails, and deal-tab attachments (upload + lender-scoped download) in production.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/professional/equityfundingsource.md source=file_ipc patch_id=c8d242a8-6ad9-4642-9e45-44e690b10fa2 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782450652921
```

### Re: lender messages — open-deal fix + General/subject tabs + search — opus-4-8

> **Queue prompt:** The Message dialog for https://equityfundingsource.brian-takita.workers.dev/lenders/deals/866e1c5a-e428-4b35-bacb-a58f5d94b7da does not show an open conversation for that property. It show and selects only the interested deal which is different. It should show the messages for the opened deal.

> **Queue prompt:** A lender may have many deals, so show the General tab, the subj

> **Queue prompt:** A lender may have many deals, so show the General tab, the subject as a tab, and a search box to go through all conversations.

Done (EFS `9d0d71d`, pushed to `origin/main`). Both your points landed in one rework; the second/third lines are the same request (the second was a draft), so I built the complete version.

**Bug — open-deal mis-select (fixed):** the dialog rendered **one tab per deal**, and the async `/api/investor/deal-threads` fetch reordered those tabs (sorted by property label); the "keep active tab valid" effect then reset the selection to `tabs[0]`, so it landed on a *different interested deal* instead of the opened one. Root-fixed by removing per-deal tabs entirely and adding a pure `lender_selected_conversation_` resolver where **the open deal always wins** as the default (a search pick overrides until cleared). The mis-select vector no longer exists.

**Rework — scales to many deals:**
- **Tabs:** now at most two — **General** (always first) + the **opened deal's property** tab (only when a deal is open). No more one-tab-per-deal.
- **Search box:** searches across **all** your conversations (every deal thread by property + General); picking a result opens that thread (loads `scope=deal` + posts to the native deal endpoint, attachments enabled).

**Verification:** I re-ran `bun run check` myself — **1320 pass, 0 fail**, browser + worker builds clean. Reviewed the selection resolver + tab/search helpers directly before pushing.

**Deploy:** held per `#spec-test-commit-push`. Queue `deploy` when you want the open-deal fix + tab/search rework live (no D1 migrations).
<!-- agent:boundary:4ff5ba98 -->
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

<!-- agent:queue preset="#spec-test-commit-push" go -->
- ~~The Message dialog for https://equityfundingsource.brian-takita.workers.dev/lenders/deals/866e1c5a-e428-4b35-bacb-a58f5d94b7da does not show an open conversation for that property. It show and selects only the interested deal which is different. It should show the messages for the opened deal.~~
- ~~A lender may have many deals, so show the General tab, the subj~~
- ~~A lender may have many deals, so show the General tab, the subject as a tab, and a search box to go through all conversations.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~deploy~~
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
