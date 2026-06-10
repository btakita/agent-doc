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
queue: stop
---

## Status

<!-- agent:status patch=replace -->
EFS workers.dev production: As of Worker version `ff80afcd-c969-4377-bedd-ac33af4e8a44` (deployed 2026-06-09): the admin loan board is renamed **Deal Pipeline** (nav item, board page, lead-detail card; section id/route/endpoint unchanged) and its sidebar nav badge now counts **action-needed** items — open Leads (`new`/`contacted`) + loans in Applications/Processing/Documents/Underwriting — instead of `COUNT(*) FROM loans`, excluding terminal/parked stages (#pipeline-action-count, EFS `909b866`). The admin **Payments** section (`/portal-admin/payments`) adds a **Pipeline** board as the default view (with a Table toggle via `?view=table`) tracking borrower payments one-card-per-loan grouped Overdue/Due Soon/Current/Paid Off, each card showing next payment due date, amount, principal, and overdue count + total; read-only snapshot served by `GET /api/admin/payments/pipeline` (registered before `/:payment_id`), mutations stay on the Table view (#payments-pipeline, EFS `d8eb783`). Neither feature required a D1 migration. `/apply` public prequalification + D1-backed `application_leads` admin review and the public `/contact` form (D1-backed `contact_messages` + `/portal-admin/contact-messages` admin review) remain live (contact-form persistence from EFS commit `79ac942`, migration `0054_contact_messages` applied remote). Loan-application Approval is gated on a complete full application — `POST /api/admin/full-applications/:id/status` no longer accepts `accepted` (in_review/declined only) and `/approve` fails closed unless the submitted application is complete (#approve-requires-full-app, EFS `b136519`); the admin "Accept and send to TMO" button is disabled with a missing-parts list on incomplete apps (EFS `b731aa4`). Portal form-submission errors render a red banner at the top of the form plus an error toast via `portal/shared/form_error.tsx` (#form-error-standard). The admin Deal Pipeline (`/portal-admin/pipeline`) has a read-only "Leads" funnel column from open prequalification leads and the entry column is "Applications" (#pipeline-leads, EFS `877c3eb`); there is no separate "Approved" column (TMO has no such loan status). Admin Management sidebar links show item-count badges loaded from `GET /api/admin/nav-counts` (#admin-nav-counts, EFS `0d9708a`). Individual application leads have a detail page at `/portal-admin/application-leads/:lead_id` (`GET /api/admin/application-leads/:id`), linked from the leads list and the pipeline Leads column (#lead-detail, EFS `ef3b76e`). Contact-form staff notifications deliver to `CONTACT_NOTIFICATION_TO=btak.dev@gmail.com` (EFS `af4935d`) from `EFS_EMAIL_FROM=Equity Funding Source <info@briantakita.me>` via the Cloudflare Email Sending API (CLOUDFLARE_API_TOKEN secret); a live POST smoke test recorded staff_notification_status=sent. NOTE: staff_notification_status=sent means the Cloudflare Email API returned 2xx, not a guaranteed inbox delivery — if mail does not arrive, investigate briantakita.me SPF/DKIM/DMARC and Cloudflare Email Sending external-delivery limits. Public homepage rate-estimator dollar prefixes use `-translate-y-1/4`, and amount inputs use `pl-6`/24px left padding loaded after the base input rule. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications use `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com`; an EFS-owned sending domain in Resend is still pending (#f4rq). Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260609-225500.md`*

Compacted content:
- Archived 3 response topic(s): Move Deal Pipeline above Loan Applications; deploy; Plan a move to Hetzner US-East (Docker + nginx)
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...

### Re: Deal Pipeline controls in the admin dialog (+ deploy) — opus-4-8

Shipped and deployed. The interactive **Deal Pipeline controls** from `img_3.png` (Status dropdown · Staff-email status · Send/Resend borrower invite) are now inline in the Deal Pipeline card dialog, so an admin can change a lead's status and (re)send the borrower invite without leaving the board.

**What changed (EFS `9234ba8`, superproject pointer `7b696114f`):**
- **New shared component** `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx` — extracts the status dropdown + staff-email status + send/resend-invite block, reusing the existing `/api/admin/application-leads/:id/status` and `/invite` endpoints and handing the updated lead back via `on_lead`.
- **Pipeline lead dialog** (`pipeline.tsx` `LeadModalBody`) now renders the controls; the redundant read-only "Staff notified"/"Borrower invite" rows were removed. Kanban lead cards stay read-only / non-draggable — only the dialog gained controls.
- **Full-page lead detail** (`application_lead_detail.tsx`) now consumes the same shared component (no behavior change, just DRY).
- Tests updated to follow the extraction + assert the dialog controls.

**Verification:** `bun run check` green — **493 tests pass** (added 2), `build` + `build:worker` clean. **Deployed** to production (Version `36773d8b-81ef-468f-86db-0b45df30c110`). Smoke test: `/`, `/funded-deals`, `/apply` → 200; `/portal-admin/pipeline` → 303→login (auth guard intact).

**On "all dialog types" — please confirm scope.** These controls are **lead-specific** (statuses new/contacted/converted/archived + borrower invite), so they map to the lead pipeline dialog (the screenshot) and the lead detail page — done. The other dialog types don't take the same controls:
- The **loan/deal** pipeline dialog already has its own inline actions (publish/close deal, delete loan, open full loan).
- **loan / payment / lender / funding / document / contact** detail modals aren't leads — a borrower-invite/lead-status block doesn't apply.

If by "all dialog types" you meant each dialog should get an **editable status dropdown for its own entity** (e.g. change a loan's pipeline stage from the loan dialog), that's a real but separate feature — each needs its own status-write endpoint. Tracked as **#pipeline-dialog-controls-scope**; tell me which dialogs + which actions and I'll build them.

### Re: Wider/shorter Deal Pipeline dialog + mobile scroll — opus-4-8

> **Queue prompt:**
>
> Make the dialog wider and shorter. ![img_4.png](img_4.png) Use a scrollbar for short monitors. Make sure this works on mobile.


Done and deployed. The root issue: `PortalModal` had `overflow:hidden` with **no max-height**, so the tall lead dialog (`img_4.png`) could clip or run off the bottom of short monitors with no way to scroll.

**What changed (EFS `3d90502`, pointer `658df787e`):**
- **Viewport-fit + scrollbar (all dialogs)** — `PortalModal` now caps at `max-height:calc(100dvh - 40px)` and scrolls internally (`overflow-y:auto`, `overscroll-behavior:contain`, touch scrolling). `dvh` accounts for mobile browser chrome, so it fits short screens and phones instead of clipping. This fixes every dialog, not just the lead one.
- **Wider** — the pipeline lead + loan dialogs go `max-w-lg` → `max-w-2xl` (32rem → 42rem).
- **Shorter** — the lead dialog body is now a responsive 2-column grid (`auto-fit, minmax(240px,1fr))`): two columns on desktop (roughly half the height), collapsing to one column on mobile.

**Verification:** `bun run check` green — full suite passes (added 2 layout tests), `build` + `build:worker` clean. **Deployed** to production (Version `4298b450-3a7d-480d-92ef-1030fcf7d875`, no DB migrations). Smoke test: `/`, `/apply` → 200; `/portal-admin/pipeline` → 303→login.

To see it: open `/portal-admin/pipeline`, click a lead card — the dialog is wider, the data sits in two columns, and on a short window it scrolls inside the dialog instead of overflowing.

### Re: deploy — opus-4-8

> **Queue prompt:**
>
> deploy


Already live. The wider/shorter dialog change was deployed earlier this turn as part of completing it — production is running **Version `4298b450-3a7d-480d-92ef-1030fcf7d875`** (EFS `3d90502`, pointer `658df787e`), with no commits since, so this `deploy` is already satisfied. No redundant redeploy. Smoke test was green (`/`, `/apply` → 200; `/portal-admin/pipeline` → 303→login). Queue drained.

### Re: Add a close button to the pipeline dialogs + deploy — opus-4-8

> **Queue prompt:**
>
> Add a close button to the pipeline dialogs. deploy


Done and deployed. All three admin pipeline dialogs now have a top-right **×** close button.

**What changed (EFS `56d45ac`, pointer `9765131ba`):**
- **New shared component** `lib/ui--browser--efs/portal/shared/ModalCloseButton` (in `portal/shared/ui.tsx`) — an accessible (`aria-label="Close dialog"`) × button carrying `data-modal-close`, so it works through both its `on_close` handler and `PortalModal`'s backdrop-close path.
- Wired into the **Deal Pipeline** lead dialog and loan dialog (`portal/admin/pipeline.tsx`) and the **Investor Pipeline** card dialog (`portal/admin/investor_pipeline.tsx`), placed next to the status pill in each header.
- The **loan dialog previously had no explicit close affordance at all** (backdrop click only) — it now has the × plus the existing action row. The lead and investor dialogs keep their bottom "Close" text button as well.

**Verification:** `bun test` 495 pass / 0 fail; `bun run build` + `build:worker` clean. Deployed Worker version **`99826cca-635d-49a7-9e38-952b2a5b4e79`**. Smoke test green: `/` → 200, `/apply` → 200, `/portal-admin/pipeline` → 303→login.

### Re: Close button on all dialogs + Esc-to-close (spec + tests, commit + push, deploy) — opus-4-8

Done, tested, pushed, and deployed.

**What changed (EFS `1854dbd`, pointer `2d2fb2410`):**
- **Esc closes every dialog.** `PortalModal` (the shared base for all portal dialogs) now closes on the `Esc` key, wired centrally so all ~16 dialog usages inherit it for free. A module-level modal stack ensures only the **topmost** dialog closes when modals are stacked (e.g. a confirm over a form), instead of collapsing the whole stack.
- **Close × on the payment dialog.** I audited all PortalModal dialogs: every one already had a visible dismiss (× / Close / Cancel) **except** the admin Payment detail dialog, which had only Confirm/Deny/Hide action buttons. It now has the standard header × (`ModalCloseButton`) beside the status pill. The three pipeline dialogs got their × last cycle (`56d45ac`).
- **Spec + tests** (`#spec-test-commit-push`): added three contract tests to `tests/portal-table-dialog.test.ts` — Esc-to-close wiring + topmost-only stack, the shared `ModalCloseButton` primitive, and a guard that **every** PortalModal dialog exposes a visible close affordance (so a future dialog can't ship without one).

**Verification:** `bun test` **498 pass / 0 fail** (3 new); `build` + `build:worker` clean. Pushed to `btakita/equityfundingsource-dev@1854dbd` and superproject pointer `2d2fb2410`. Deployed Worker version **`2bf66430-b229-469c-ae59-25f0ac274817`**. Smoke test green: `/` → 200, `/apply` → 200, `/portal-admin/pipeline` → 303→login, `/portal-admin/payments` → 303→login.

### Re: Better Lead-dialog format — use horizontal space (img_5) + deploy — opus-4-8

Done and deployed. The Lead dialog (and the other pipeline detail dialogs) now use the dialog's horizontal width instead of stacking everything on the left half.

**The problem (img_5):** the Deal Pipeline controls — Status, Staff email, Resend-invite — were three full-width rows pinned to the left, leaving the entire right half of the dialog empty.

**What changed (EFS `b64d2a0`, pointer `2c45901f4`):** applied the lead data grid's responsive two-column guideline consistently across all pipeline detail dialogs:
- **`lead_pipeline_controls.tsx`** — Status and Staff email now sit **side-by-side** in an `auto-fit` grid (label-above-field), with the invite action row below.
- **Loan dialog** (`pipeline.tsx`) and **Investor dialog** (`investor_pipeline.tsx`) — their field rows now flow through the same `repeat(auto-fit,minmax(240px,1fr))` two-column grid the lead data section already used, so they're wide-and-short rather than a tall single column.
- All collapse to one column on narrow/mobile via `auto-fit`.
- **Payment detail dialog** — editable status dropdown (Pending / Under review / Paid / Denied) wired to the existing `PATCH /api/admin/payments/:id`, alongside the existing Confirm/Deny/Hide quick actions.
- **Loan/deal pipeline dialog** — editable status dropdown (Application Received → Processing → Documents → Underwriting → Funded, plus On Hold / Closed / Unassigned / Denied). I refactored the existing drag-between-columns writer into a shared `change_loan_status_`, so the dropdown and drag-drop now use **one code path**: optimistic card move, `PUT /api/admin/loans/:id/status` (auto-closing a published investor deal, same as drag), the open dialog stays in sync, and it rolls back on error.

**Spec/tests:** added a contract test (`#dialog-horizontal-format`) locking the two-column grid guideline for the pipeline dialogs + shared controls, so a future dialog can't regress to single-column stacking.

**Verification:** `bun test` **498 pass / 0 fail** (+ the new guideline test, 8 in the dialog suite); `build` + `build:worker` clean. Pushed `equityfundingsource-dev@b64d2a0` + pointer `2c45901f4`. Deployed Worker version **`df29318f-c556-43cc-afc3-0946cf072e54`**. Smoke test green: `/` → 200, `/apply` → 200, `/portal-admin/pipeline` → 303→login.

### Re: #pipeline-dialog-controls-scope — editable status dropdowns on loan + payment dialogs — opus-4-8

Scope confirmed and built. You asked for editable pipeline-status dropdowns on the loan/deal, payment, and investor dialogs. Two are shipped; the third isn't a settable status — here's why.

**Shipped (EFS `d4b7e55`, pointer `7e9b2501f`):**
- **Loan/deal pipeline dialog** — editable status dropdown (Application Received → Processing → Documents → Underwriting → Funded, plus On Hold / Closed / Unassigned / Denied). I refactored the existing drag-between-columns writer into a shared `change_loan_status_`, so the dropdown and drag-drop now use **one code path**: optimistic card move, `PUT /api/admin/loans/:id/status` (auto-closing a published investor deal, same as drag), the open dialog stays in sync, and it rolls back on error.
- **Payment detail dialog** — editable status dropdown (Pending / Under review / Paid / Denied) wired to the existing `PATCH /api/admin/payments/:id`, alongside the existing Confirm/Deny/Hide quick actions.

**Investor dialog — not a settable status (important).** The Investor Pipeline lifecycle stage is **derived**, not stored: the board computes each investor's stage as "the furthest stage reached" from accreditation status + lender account + active/pending fund counts. There's no column to write, so a "set stage" dropdown would be meaningless. The genuinely editable investor action is the **accreditation decision** (Accredit / Pending), which has its own endpoint. I've queued that as a concrete follow-up in the backlog rather than ship a fake control.

**Verification:** `bun test` **501 pass / 0 fail** (3 new contract tests); `build` + `build:worker` clean; the loan/payment/controls files are TypeScript-clean. Committed + pushed per `#spec-test-commit-push` (no deploy this cycle — say "deploy" or re-queue with `#spec-test-commit-push-deploy` to ship to production).

### Re: Borrower portal messaging — admin inbox + email round-trip — opus-4-8

Good news: the borrower secure-messaging feature (`#xcvg`) is already built. Three of your four asks are done (some behind an off-by-default flag); the fourth is scaffolded but not yet functional. Here's the honest status of each.

**1. Is there an admin for the borrower portal messages? — Yes, live.**
There's a full admin inbox at **`/portal-admin/messages`** (in the admin nav). It lists borrower chat sessions, lets you search by borrower (`q`), open a thread, reply, and it has realtime updates over WebSocket (plus a "watch" mode and unread-count badges). It's a *separate* surface from the `/contact` form inbox (`/portal-admin/contact-messages`) — that one is public contact-form submissions; this one is authenticated borrower↔admin chat. Reply endpoint: `POST /api/admin/messages/:session_id`.

**2. Email notification when a borrower sends a message? — Built, currently off.**
When a borrower sends a message, `notify_admin_of_borrower_message_` always fires an **in-app** notification immediately (to the owning admin, or all active admins if unassigned). It *also* sends an **email** to those admins — but only when `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED` is on **and** email delivery is configured (Cloudflare Email Service, or Resend/local-capture). That flag is off by default, so today you get the in-app notification but no email. The email links straight to the thread in the admin portal.

**3. Email response back into the chat (reply-by-email)? — Scaffolded, not yet working.**
There's an inbound endpoint `POST /api/email/inbound`, but it deliberately returns `501` until `MESSAGE_EMAIL_INBOUND_ENABLED` is set **and** verified-sender authentication is in place — and the actual "append the parsed reply to the thread" step is still a `TODO`. So replying from your email client does **not** post into the chat yet. Finishing this needs: (a) an inbound mail route (Cloudflare Email Routing or Resend inbound) pointed at that endpoint, (b) sender-auth so only a verified admin address can inject a message, and (c) wiring the parsed body through `borrower_chat_message_create_admin_`. It's intentionally fail-closed so unauthenticated mail can never inject chat messages.

**4. Borrower gets emailed when an admin replies? — Built, currently off.**
Symmetric to #2: `notify_borrower_of_admin_message_` always sends the borrower an **in-app** notification, and emails them too **when the same `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED` flag + delivery are configured**. The email links to `/portal-borrower/messages`.

**To turn on the email round-trip (#2 + #4) — config only, no new code:**
- Set `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED=true`
- Configure delivery (Cloudflare Email Service token/account, or your Resend setup) and `MESSAGE_NOTIFICATION_FROM` (defaults to `info@equityfundingsource.com`)
- These should use an EFS-verified sending domain — same dependency as backlog **#f4rq** (verify the Resend sending domain). I'd gate the flag flip on that.

**Reply-by-email (#3)** is the only one that needs real implementation, not just config. Say the word and I'll queue it as a concrete backlog item with the three steps above.

**Queue housekeeping:** the queue is currently **inactive**, so `#eyp0`/`#6r8v` aren't auto-running. They were duplicate entries of the same item (the deferred *investor* accreditation-control follow-up — unrelated to this messaging work), so I folded the endpoint detail into **#eyp0** and closed **#6r8v** as a duplicate. Note: preflight flagged #eyp0 as possibly-done against `#pipeline-dialog-controls-scope` — that's a false positive; #eyp0 is the genuinely-not-yet-built follow-up, not the shipped loan/payment dropdowns. Want me to build #eyp0, or enable the messaging email round-trip? Activate the queue or just say which.

### Re: go — built the Investor Pipeline stacked profile dialog (#b0wq) — opus-4-8

Done, per `#spec-test-commit-push`. On the Investor Pipeline board, **"Open investor profile"** now opens the editable profile as a **stacked dialog on top of** the card modal instead of navigating away to `/portal-admin/users/:id`.

**Shipped (EFS `6542d9d`, pointer `ef3d28999`):**
- Extracted the editable account+address form out of the full-page `UserDetail` into a shared **`UserProfileEditor`** (same fetch / edit / Save / Delete logic), so the page and the dialog render the *same* form — no duplication.
- New **`UserProfileModal`** renders that editor stacked at `z-[1100]` above the card's `z-[1000]`. `PortalModal` already handles stacking correctly (Esc closes only the topmost). Delete from the dialog closes the dialog; an **"Open full page"** link is retained for the standalone account view.
- The card's profile action is now a button (`set_profile_open(true)`), not an `<a href>`.

**Verification:** `bun test` **502 pass / 0 fail** (updated `#vtx8`, added `#b0wq` coverage for the stacked dialog + shared editor); `build` + `build:worker` clean (browser bundle compiled the new TSX — no circular import). Committed + pushed, pointer bumped. No deploy this cycle (say "deploy" / `#spec-test-commit-push-deploy` to ship).

**Queue (still inactive — staged, not auto-running):**
- **`#eyp0`** — investor accreditation Accredit/Pending control. Codeable next; say "go" to build it.
- **`enable email round trip`** — captured as **`#emrt`**. This one is **config-only, no code**, but blocked on **`#f4rq`** (verify an EFS-owned Resend sending domain) + setting the production flag/secrets, which need Robert's domain access. I can't safely flip production secrets from here. Want me to draft the exact wrangler secret commands + the Resend domain-verification steps for Robert?
- **Honest constraint:** the endpoint only accepts `verified`/`rejected`, and it operates on a *verification record*. So the control acts only once the investor has actually **submitted** their accreditation questionnaire — if there's no submission yet it shows "No accreditation submission yet" rather than a fake toggle. "Pending" is the pre-decision state, not a settable button. If you want an explicit admin "reset to pending" or the ability to accredit someone who hasn't submitted, that needs a small backend change — say the word and I'll add it.
- A decision **reloads the board** so the derived lane re-computes (an accredited investor can move Signed Up → Accredited).
- New `InvestorAccreditationControl` in the card dialog. On open it fetches the investor's latest verification record (`GET /api/admin/users/:id/investor-accreditation`), shows the current status (Pending / Verified / Rejected), and offers **Accredit** and **Reject** actions wired to the existing admin endpoint `POST /api/admin/investor-accreditation/:verification_id/decision` (it was already admin-guarded but had no UI until now).

Next, or activate the queue and I'll keep draining.

### Re: #eyp0 — admin accreditation control in the Investor Pipeline card dialog — opus-4-8

> **Queue prompt:**
>
> [#eyp0]


Built, per `#spec-test-commit-push`. The Investor Pipeline card dialog now has an **Accreditation decision** control — the genuinely editable investor action (the lifecycle *stage* is derived, so there's nothing to set there).

**Shipped (EFS `25a55ab`, pointer `344c8d7a1`):**
- New `InvestorAccreditationControl` in the card dialog. On open it fetches the investor's latest verification record (`GET /api/admin/users/:id/investor-accreditation`), shows the current status (Pending / Verified / Rejected), and offers **Accredit** and **Reject** actions wired to the existing admin endpoint `POST /api/admin/investor-accreditation/:verification_id/decision` (it was already admin-guarded but had no UI until now).
- A decision **reloads the board** so the derived lane re-computes (an accredited investor can move Signed Up → Accredited).
- **Honest constraint:** the endpoint only accepts `verified`/`rejected`, and it operates on a *verification record*. So the control acts only once the investor has actually **submitted** their accreditation questionnaire — if there's no submission yet it shows "No accreditation submission yet" rather than a fake toggle. "Pending" is the pre-decision state, not a settable button. If you want an explicit admin "reset to pending" or the ability to accredit someone who hasn't submitted, that needs a small backend change — say the word and I'll add it.

**Verification:** `bun test` **504 pass / 0 fail** (+2: the card control + endpoint guard); `build` + `build:worker` clean. Committed + pushed, pointer bumped. No deploy (say "deploy" to ship).

Next in the queue: **`#emrt`** (enable the messaging email round-trip) — that one's config-only but blocked on `#f4rq` (Resend domain) + Robert's access, so I'll surface it rather than flip production secrets.

### Re: #emrt — enabled the borrower-messaging email round-trip — opus-4-8

> **Queue prompt:**
>
> [#emrt]


Enabled, per `#spec-test-commit-push`. And a correction to what I said earlier: this was **not** actually blocked on Resend / `#f4rq`.

**What I found:** the borrower-messaging email goes through the **Cloudflare Email Sending API** — the exact same path and token (`CLOUDFLARE_EMAIL_API_TOKEN`, already a production secret) that the `/contact` form and application-lead notifications already use successfully. The `RESEND_API_KEY` in the code/PRD was never wired into the actual send path. So enabling was just two vars, not a new provider + domain.

**Shipped (EFS `83c1875`, pointer `f4cdd3a58`):**
- `wrangler.toml`: `MESSAGE_EMAIL_NOTIFICATIONS_ENABLED = "true"` + `MESSAGE_NOTIFICATION_FROM = "Equity Funding Source <info@briantakita.me>"`. I set the From to the **already-verified** Cloudflare sender (`info@briantakita.me`, same as the contact form) — the code's default `info@equityfundingsource.com` is **not** verified in Cloudflare Email yet, so leaving it default would have failed delivery.
- Now live on deploy: admin gets emailed when a borrower messages (#2), borrower gets emailed when an admin replies (#4). Each links into the respective portal thread.
- Inbound email→chat (#3) stays **off** — that one genuinely needs new code (the `/api/email/inbound` scaffold returns 501).
- Corrected the PRD line (it claimed Resend + "stay off until domain verified").

**Verification:** `bun test` **506 pass / 0 fail** (added the prod-enablement assertion; also rebased onto a teammate's `#zjbe` investor-email change that landed mid-cycle); `build` + `build:worker` clean. Committed + pushed.

**Two things to know:**
1. **It goes live on the next deploy** — this cycle was commit+push only (no deploy per the preset). Say "deploy" to ship it.
2. **Sender is `info@briantakita.me`** until `info@equityfundingsource.com` is verified in Cloudflare Email (still tracked as `#f4rq`). Functionally fine; just not EFS-branded yet.

Queue drained — that was the last item (`#eyp0` + `#emrt` both done; `#b0wq` done earlier).
<!-- agent:boundary:1aac1fe1 -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue preset="#spec-test-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#8tkq] Loan/borrower-scoped wire-link issuance + routing: generate secure wire-links bound to a specific loan/borrower from the admin loan + wire-confirmation surfaces (not just the global Settings card), and model the title/escrow -> EFS -> lender delivery routing. Builds on the #wir3 delivery mechanism.
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
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
- [/] [#cm3e] [recommended] Borrower online payment collection (Stripe) + TMO sync: add ACH/card payment in the borrower portal make-payment section; on Stripe payment_intent.succeeded webhook, verify signature, record the receipt in a new EFS D1 loan_payment_transactions table, reconcile the matching loan_payments schedule row, and push the payment to TMO. GATE: confirm ABS/TMO exposes a borrower-payment/receipt WRITE endpoint (current docs/TMO cover loan/application writes, investor AddLoanFunding, and schedule/funding reads only) — ties into the TMO-writeback gate (dzmm/k2xg). Deferred non-MVP collection path from e245; handle KYC, partial payments, NSF reversals, fees.
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
