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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260622-014423.md`*

Compacted content:
❯ - Archived 4 response topic(s): drain the queue (email round-trip + portal messaging); deploy; Realtime websocket messaging for the unified message system; 1 more
> **Queue prompt:** I replied to a Gmail thread (chromium-bridge or corky to view) but the message did not appear in the funder portal.

### Re: email reply didn't appear in the funder portal — opus-4-8

> **Queue prompt:**
>
> I replied to https://mail.google.com/mail/u/1/#inbox/FMfcgzQgMVjNzCbNqqXSGchWJnhWZGKh (chromium-bridge or corky to view) but the message did not appear in the funder portal.


**Why it didn't appear:** the inbound email→portal path was never enabled or wired in prod. This is the operator-gated `#msgmail4` phase. Concretely, three things were all missing:

1. **No `email()` handler** on the worker — Cloudflare Email Routing had nothing to deliver inbound mail to.
2. **No Cloudflare Email Routing** rule at all.
3. **None of the inbound secrets/vars** are set (`MESSAGE_EMAIL_INBOUND_ENABLED`, `_SECRET`, `_REPLY_TO`), and the `/api/email/inbound` route returns **501 (disabled)** without the flag.

So your reply sat in the mailbox; nothing wrote it to D1; the portal (and the new realtime push, which only fires on a D1 write) never saw it. The outbound notify + Reply-To plumbing (`#msgmail1–3`) was already built — only the inbound enablement was missing.

**What I built + shipped this turn (the code half — `#msgmail4a`):**

- A Cloudflare **`email()` Worker handler** (`lib/messages/inbound_email.ts`): parses inbound MIME with `postal-mime`, derives the thread token from the plus-addressed recipient, strips quoted reply history, and writes the message.
- A **shared inbound core** (`lib/messages/inbound.ts`, `message_inbound_apply_`) — one authorize/route/write/notify path used by BOTH the `email()` handler and the existing `/api/email/inbound` route, so they can't diverge. Non-participant senders are rejected (no thread injection).
- Writes ride the `#msgrt` realtime tick, so once enabled a reply shows in the portal live.
- Tests (6 new + refactor of the existing inbound tests), full suite green (1089), built, committed (`fbba9eb`), pushed, **deployed** (worker `0ab29102`). It is **inert** until enabled — deploying it changed nothing in prod by itself.

**What's left — operator infra to turn it on (`#msgmail4b`, filed in backlog):** in the Cloudflare dashboard, add an **Email Routing rule** delivering the portal Reply-To address (e.g. `info@briantakita.me` or a dedicated `replies@…`) to **this Worker** (Email Worker binding, not forward-to-address); verify the receiving domain's MX + Email Routing; then set `MESSAGE_EMAIL_INBOUND_ENABLED=true` and `MESSAGE_EMAIL_INBOUND_REPLY_TO` (and `MESSAGE_EMAIL_INBOUND_SECRET` for the HTTP path). Send a test reply and confirm it lands in the thread. I left this to you per the chosen split (it's security-sensitive and needs dashboard/MX access). Plan: `prds/message-email-roundtrip.md`.
> **Queue prompt:** do [#f2w8]: Should the reply-to email be an "app email" with a token to enable inbound emails? I don't have a domain attached to the app yet. What domain/subdomain can we use for this staging app? I prefer a dedicated replies@... email.

### Re: reply-to app email + token, and which domain for staging — opus-4-8

> **Queue prompt:**
>
> do [#f2w8]: Should the reply-to email be an "app email" with a token to enable inbound emails? I don't have a domain attached to the app yet. What domain/subdomain can we use for this staging app? I prefer a dedicated replies@... email.


**Yes — a tokenized app reply-to is exactly the design that's already built.** The Reply-To on each notification is `replies+{TOKEN}@yourdomain`, where `{TOKEN}` is an opaque code that encodes the thread (which party, deal/account, and user). When the recipient replies, Cloudflare delivers the mail to the worker's `email()` handler, which reads the token and drops the message into the right thread. No code change needed for this — `message_reply_to_` already appends the token to whatever address you configure in `MESSAGE_EMAIL_INBOUND_REPLY_TO`. Your `replies@…` preference is fully supported: set it to `EFS Replies <replies@yourdomain>` and the code emits `replies+{TOKEN}@yourdomain`.

**The one real constraint — Cloudflare Email Routing + the `+token` plus-address.** The token rides as a plus-address (`replies+{TOKEN}@…`). Cloudflare Email Routing custom-address rules match the **exact** address only — a rule for `replies@` will **not** match `replies+{TOKEN}@`. So inbound must be delivered via a **catch-all rule → the EFS Email Worker**. That's the gotcha that drives the domain choice.

**Domain options for staging:**

1. **Dedicated subdomain/domain (recommended).** Use a domain or subdomain that carries *no other mail*, point its MX at Cloudflare Email Routing, and set a **catch-all → EFS Worker**. Then `replies+{TOKEN}@<that-domain>` all route to the worker with zero collateral. Since you don't have the real app domain yet, easiest staging paths: a cheap dedicated domain, or a subdomain on a Cloudflare zone you control that isn't used for other email. Set `MESSAGE_EMAIL_INBOUND_REPLY_TO = "EFS Replies <replies@<that-domain>>"`.

2. **Reuse `briantakita.me`** (already your Cloudflare-verified *sender*). Fastest, but a catch-all there would capture **all** mail to `briantakita.me` (including `info@` and anything personal) and hand it to the EFS worker. Only do this if `briantakita.me` receives no mail you care about — otherwise it's risky.

You can keep **sending** from the verified `info@briantakita.me` (From and Reply-To are allowed to differ), so you don't need to re-verify a sender to use a different reply domain.

**If you'd rather avoid a catch-all:** I can add a token-as-local-part scheme so the reply address is `{TOKEN}@replies.<domain>` (no plus) — that still needs a catch-all on the `replies.` subdomain, but it keeps the apex clean and reads better. Say the word and I'll wire it; otherwise the current plus-address scheme is ready.

**My recommendation:** pick a dedicated subdomain on a Cloudflare zone, enable Email Routing with a catch-all → the EFS Worker, and use `replies@<subdomain>`. Once you tell me the exact domain/subdomain, I'll set `MESSAGE_EMAIL_INBOUND_REPLY_TO` (+ flip `MESSAGE_EMAIL_INBOUND_ENABLED=true`) in `wrangler.toml` and deploy; you add the Cloudflare Email Routing catch-all rule (dashboard) and we send a test reply to confirm it lands. That completes `#msgmail4b`/`#f2w8`.
❯ Can we use a subdomain such as reply@efs.briantakita.me? Would the root domain email briantakita.me still go to Protonmail?
> **Queue prompt:** Can we use a subdomain such as reply@efs.briantakita.me? Would the root domain email briantakita.me still go to Protonmail?

### Re: subdomain reply@efs.briantakita.me + keeping Protonmail on the root — opus-4-8

**Yes on both counts — and this is the clean setup.**

**1. A subdomain works.** Cloudflare Email Routing now supports subdomains (Dashboard → Email → Email Routing → Settings → **Add subdomain**). Add `efs.briantakita.me`, then create a **catch-all on that subdomain → the EFS Email Worker**. The catch-all is what captures the plus-addressed token (`reply+{TOKEN}@efs.briantakita.me`).

**2. Your root `briantakita.me` email stays on Protonmail.** Email delivery is decided by **MX records**, and MX is per-hostname. Enabling Email Routing on the **subdomain** adds MX records to `efs.briantakita.me` only — it does **not** touch the apex `briantakita.me` MX (which points at Protonmail). So personal mail to `@briantakita.me` keeps flowing to Protonmail, untouched. (This is exactly why the subdomain is better than a catch-all on the apex, which *would* have hijacked Protonmail.)

So the recommended config:
- Cloudflare: add subdomain `efs.briantakita.me` to Email Routing, set a **catch-all rule → EFS Worker** (Email Worker action).
- App: `MESSAGE_EMAIL_INBOUND_REPLY_TO = "EFS Replies <reply@efs.briantakita.me>"` → outbound Reply-To becomes `reply+{TOKEN}@efs.briantakita.me`.
- Keep **sending** from the verified `info@briantakita.me` (From and Reply-To can differ).

**One timing note:** outbound notifications are already live in prod, so I should set `MESSAGE_EMAIL_INBOUND_REPLY_TO` + flip `MESSAGE_EMAIL_INBOUND_ENABLED=true` **only after** the `efs.briantakita.me` subdomain + catch-all exist — otherwise real emails would carry a `reply+…@efs.briantakita.me` Reply-To that dead-ends until routing is live. So the order is: (1) you add the subdomain + catch-all in Cloudflare, (2) tell me, (3) I set the two vars and deploy, (4) we send a test reply and confirm it lands in the thread. That closes `#f2w8`/`#msgmail4b`.

Sources: [Cloudflare Email Routing — Subdomains](https://developers.cloudflare.com/email-routing/setup/subdomains/), [Email Routing subdomain support (Cloudflare blog)](https://blog.cloudflare.com/email-routing-subdomains/).
> **Queue prompt:** Is the email replies to real-time messages working? Will the replies also go to the efs admin email?

### Re: is email-reply working yet, and do replies reach the EFS admin email — opus-4-8

> **Queue prompt:**
>
> Is the email replies to real-time messages working? Will the replies also go to the efs admin email?


**Email replies into threads: not working yet.** The handler is deployed but **inert** — it only turns on after the two #msgmail4b steps that are still pending: (1) you add the `efs.briantakita.me` subdomain + catch-all in Cloudflare, and (2) I flip `MESSAGE_EMAIL_INBOUND_ENABLED=true` + set `MESSAGE_EMAIL_INBOUND_REPLY_TO`. So right now, replying to a notification email does **not** post back into the thread. (To be clear, the **in-app realtime messaging** over websocket *is* live and working — that's the separate `#msgrt` piece. Email-reply-into-the-thread is the `#msgmail4` piece that's still gated on the Cloudflare setup.)

**Yes — replies will also reach the EFS admin email** (once inbound is enabled). The inbound path reuses the existing notify flow, which is symmetric:
- A **party** (lender/broker/borrower) reply → the app emails **all active EFS admins** (every `role='admin'` user, at their own email) **and** drops an in-app notification, **and** posts the message into the thread (firing the realtime tick so any open admin view updates live).
- An **admin** reply → the app emails the **party** and posts to the thread.

So the chain is bidirectional: a party's email reply lands in the thread *and* notifies admins by email; an admin can then reply (in-app or by email) and the party is emailed — "and so on." Note the admin notification already happens **today** for in-app party messages (outbound notifications are enabled); the email-reply path simply feeds the same notify once `#msgmail4b` is switched on.

**Net:** nothing to flip on my side until the Cloudflare subdomain + catch-all exist. Add those and ping me, and I'll enable + deploy + we test a round-trip. (`#f2w8`/`#msgmail4b`)
<!-- agent:boundary:7f4089ff -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?
---
After 20:30
- login to TMO. Make sure we are using the correct credentials.
- TMO Sync
-->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#cmtsec] [recommended] (UPDATED w/ D1 data 2026-06-20) d6b78994 "Committed amount" section shows a REAL submitted proposal: investor@ (btak.dev+investor@gmail.com) proposal_amount=$100,000 @ 6.5%, submitted 2026-06-19, selected_at=null — NOT a null-amount artifact, so for this deal the block is correct. Latent UI note still valid for OTHER rows: interest_has_proposal_ (admin/deal_detail.tsx:730) renders the block whenever proposal_submitted_at is set, which lib/investor/shared.ts:504 stamps on ANY of amount/rate/points — a rate/points-only proposal would show "Committed amount: -". Optional: gate on proposal_amount != null or relabel "Proposed terms". If the $100k investor@ proposal was unintended/test data, clear that interest row to remove the section.
- [/] [#5q7z-ship] [operator-verify] DEPLOYED 2026-06-19 (worker version 0dd3c983; EFS d8c17b4, pointer 58a2c5fea). Remaining = operator live-verify only: open the admin approve flow on an already-in-TMO loan (deal 7a0c1463 / loan 45ddae53 GQI751) and confirm it no longer returns TMO 400 Duplicate loan number and the Accept-and-Send button reads Already in TMO. Advances #5q7z.
- [/] [#cutover-checklist] [recommended] Write (do not execute) the workers.dev to equityfundingsource.com custom-domain + Google Workspace SSO cutover checklist doc: routing for /, /funded-deals, admin routes; /login/access-callback; rollback steps. Execution stays gated on #m6ch creds. Advances #efs-cutover-sso-plan/#next-steps.
- [/] [#67v2-aa-local] [recommended] Admin pipeline auto-advance local half (advances #67v2-auto-advance): define stage-requirement checks from the TMO-aligned Kanban and enqueue audited status updates to tmo_cache_outbox; replay stays gated on #dzmm. Depends on the #67v2 Kanban stages being built first.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#msgmail4] [recommended] Enable email round-trip in prod (ops, operator-gated): configure Cloudflare Email Routing to POST inbound mail to the worker w/ the shared secret; set MESSAGE_EMAIL_INBOUND_ENABLED/_SECRET/_REPLY_TO; verify sending domain (#f4rq). Do not enable blind. Plan: src/equityfundingsource-dev/prds/message-email-roundtrip.md
- [/] [#mcme] [recommended] #msg-read-status Email-open-based read receipts on messages — DEFERRED: gated by the operator on "only if we can detect email reads", which EFS cannot today (sends via Cloudflare Email, no open tracking). Needs a ~3-5 day greenfield subsystem: switch message notifications to Resend with open-tracking, add a signed Resend event webhook (email.opened), an email_events table, and message<->email linking, then surface email_opened_at in the message DTO/UI. In-app portal read cursors already exist for borrower chat (chat_sessions.*_last_read_at) but are not email reads.
- [/] [#msg-unify4] [gated: awaiting dual-write prod soak] Lender cutover — dual-write (best-effort mirror into messages, both lender sources) + backfill (migration 0098) SHIPPED (EFS b5a6a55). Remaining: after soak, switch the inbox to read lender data from messages (dual-read), then drop deal_interest_lender_messages + lender_account_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#msg-unify3] [gated: awaiting dual-write prod soak] Borrower cutover — dual-write (best-effort mirror into messages) + backfill (migration 0097) SHIPPED (EFS 411a4da). Remaining: after the dual-write soaks live, switch the unified inbox to read borrower data from messages (dual-read), then drop chat_messages. Plan: src/equityfundingsource-dev/prds/message-unification.md.
- [/] [#lender-card-msg-email] [recommended] Email-to-lender parity for #lender-card-msg account messages (mirror notify_lender_deal_message_): notify the lender by email when an admin posts an account message, like the deal-interest thread does. Rides the verified EFS Resend sending domain gate (#f4rq). Plan: src/equityfundingsource-dev/prds/lender-card-messaging.md.
- [/] [#proprate] Lender proposal persists a DEFAULT interest rate never finalized. investor/deal_detail.tsx:362 pre-fills proposal_rate from deal.monthly_interest when deal.proposal_interest_rate==null; submit (:425-435) sends it whenever an amount is present; :421 requires a finite rate, so a lender cannot commit an amount with rate TBD — the pre-filled default (6.5% on d6b78994) persists as if finalized. Duration NOT affected (no proposal_term_months column). DATA CLEANUP DONE 2026-06-20: nulled proposal_interest_rate on the d6b78994/b8ccaa76 row (kept proposal_amount=100000 + proposal_submitted_at; D1 changes=1). CODE FIX still pending operator go: do NOT pre-fill the rate with the deal default; allow proposal_interest_rate=null ("amount committed, rate TBD"); relax :421 validation + admin LoanTermProposalSummary display.
- [/] [#nthc] [operator-verify] DEPLOYED 2026-06-20 (worker version 17b06176-fc91-44b6-b35d-1cd61beed01b; EFS 8889a4c). Remaining = operator live-verify only: open the Lender Bros group Edit overlay on deal d6b78994, bump lender2 from 0%, confirm the split saves and the "60% confirmed (partial)" badge reconciles.
- [/] [#pmhf] [recommended] Reuse the rebalancing share editor (lib/ui--browser--efs/portal/shared/lending_groups_alloc.ts: rebalance_stakes_/distribute_evenly_/lock pins) in the per-deal split UI (deal_detail allocations) so per-deal % editing gets the same 100%-balanced, lockable inputs.
- [/] [#dealtab500] [operator-verify] Root-cause the deal 7a0c1463 / loan 45ddae53 (GQI751) loan-tab "Internal Error". Static trace independently re-confirmed exhausted: GET /admin/pipeline serves only a shell (no server-side deal processing); every client-fetched endpoint is guarded — admin_loan_R_ is a plain SELECT, admin_deal_R_ LTV/onboarding chain (onboarding_json_/loan_ltv_calc_) is fully try/catch + null guarded, /documents is harmless; all client fetches degrade to error cards, not the worker Internal Error page. Regression guard shipped (efs a39be3d) locking the thin-TMO-loan onboarding shape against throws (gformula deal-economics helpers). REMAINING (operator-gated, cannot be agent-done): reproduce live via `bun run cf:worker:tail` + a browser click on the loan tab to capture the real throwing endpoint + stack, OR inspect loan 45ddae53/deal 7a0c1463 D1 rows once the #cftk token lands. Relates to #5q7z/#214f.
- [/] [#5q7z] [recommended] 'Accept and send to TMO' duplicate-loan bug: for a loan already associated with a TMO record (deal 7a0c1463 / loan 45ddae53 has tmo_loan_number=GQI751, tmo_loan_rec_id=3840D27B..., tmo_loan_application_rec_id=cda67748), the approve flow (admin/applications/index.ts application_tmo_writeback_attempt_ -> tmo_writeback.ts:308 LOS.svc/NewLoan) sends NewLoan with the existing GQI751 -> TMO 400 'Duplicate loan number. Unable to save record.' Fix (2 parts): (1) approve/writeback must detect an existing TMO association (tmo_loan_rec_id / writeback_status='sent') and associate+mark accepted locally instead of calling NewLoan (or call an UpdateLoan endpoint); (2) hide/disable the 'Accept and send to TMO' button in lib/ui--browser--efs/portal/admin/full_applications.tsx:424 when the app/loan is already in TMO. Design Q: on accept of an already-in-TMO loan, associate-only (no TMO call) or also push UpdateLoan? Relates to #dzmm/#37ah/#214f.
- [/] [#214f] [recommended] Gate investor deal publish on acceptance: admin_deal_publish_ (app/equityfundingsource/lib/admin/repo.ts:2054) + POST /api/admin/loans/:id/deal publish a loan to the investor marketplace with NO acceptance check (only 'if (!loan) throw'); any non-Funded loan becomes status='published'. So a deal can be an Available Deal while its application is not accepted (observed: investment_deal 7a0c1463 published from loan 45ddae53 status='submitted', tmo 'Application Received', with NO full_application row). Decide the business rule ('accepted before available') and add the gate before status='published' (and/or surface why a loan is ineligible to publish). Relates to the full_applications<->loans<->investment_deals decoupling.
- [/] [#cftk] RESOLVED 2026-06-20: passage btak/equityfundingsource/cloudflare_api_token is an ACTIVE token WITH D1 access — token verify=active; D1 database list + SELECT query both succeeded against account 1e7e194ff8da4ad9e40ce041aea84f76 / DB 280e1539 (the D1 /query API requires Account D1 Edit, so write scope is confirmed). .env line 7 already references this exact passage path, so #cftk is unblocked with no change needed. D1-gated investigations (#dealtab500, #cmtsec) can now run.
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
