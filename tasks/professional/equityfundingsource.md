---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
agent: codex
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260623-220137.md`*

Compacted content:
❯ - Archived 1 response topic(s): Queue drain after email reply visibility closeout
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: Unified admin chat interface — gpt-5

> **Queue prompt:**
>
> ~~Add the unified chat interface to the admin portal. This chat portal should detect which borrower, lender(s), or broker that is being viewed and display the appropriate conversation(s). For example, if an available deal dialog is open, show the chat with the borrower. If viewing interested lender(s), the chat should be with the lender(s) and the borrower in a separate tab. Size the admin message dialog appropriately to see the conversation and tabs if there are multiple participants. Lender groups should allow messaging with the entire group as a multiway conversation or with an individual lender via tabs.~~ — auto-struck: answered this cycle (#ftstrike)


> **Queue prompt:**
>
> Add the unified chat interface to the admin portal. This chat portal should detect which borrower, lender(s), or broker that is being viewed and display the appropriate conversation(s). For example, if an available deal dialog is open, show the chat with the borrower. If viewing interested lender(s), the chat should be with the lender(s) and the borrower in a separate tab. Size the admin message dialog appropriately to see the conversation and tabs if there are multiple participants. Lender groups should allow messaging with the entire group as a multiway conversation or with an individual lender via tabs.

Implemented and pushed the unified admin chat interface.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/shared/portal_messages.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/investor_management/index.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/lending_groups/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/messages_unified.ts`
- `src/equityfundingsource-dev/tests/portal-messages-component.test.ts`
- `src/equityfundingsource-dev/tests/investor-portal-e2e.test.ts`

What changed:
- Added `AdminConversationTabs` and admin-scoped message contexts for borrower, broker, deal-interest lender, and lending-group conversations.
- Added a dedicated admin Deal Detail `Messages` tab plus contextual inline chat tabs in Loan Detail, Interested Lenders, and Multi-Lender Groups.
- Added borrower deal-level admin message endpoints backed by persisted borrower chat sessions and borrower notifications.
- Added lending-group deal message endpoints backed by live `messages` storage, including group broadcast fanout to active linked members and individual lender tabs.
- Included lending-group message rows in the lender unified inbox aggregation.

Verification:
- `bun test tests/portal-messages-component.test.ts` — passed, 13 tests.
- `bun test tests/investor-portal-e2e.test.ts tests/admin-investor-pipeline.test.ts tests/lender-deal-allocation-dialog.test.ts` — passed, 143 tests.
- `git diff --check` — passed.
- `make check` — passed, 1167 tests plus app build and worker build.
- `gh run list --workflow CI --limit 1` — could not run because this repository has no workflow named `CI`.

Commits:
- EFS submodule: `f91141d68cc3e391acec29cfdd92c41fe2acc3e8` (`feat(admin): add context-aware chat tabs`), pushed to `equityfundingsource-dev/main`.
- Parent submodule pointer: `cc31cce25` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main`.

Escalation: none.
There were no actionable follow-up items to capture.
Confidence: high.
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=902510d1-f73b-4fa8-a60b-bb0abfc69bb2 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782254690087
```

### Re: Deploy — gpt-5

> **Queue prompt:**
>
> deploy

Deployed the current EFS Worker build after the unified admin chat interface commit.

Changed paths: none from deployment; `src/equityfundingsource-dev` is clean after deploy.

Commands:
- `make deploy`

Verification:
- Worker build completed.
- Remote D1 migration check completed with `No migrations to apply!`.
- Cloudflare asset upload completed with 1 updated browser asset.
- Worker deployed at `https://equityfundingsource.brian-takita.workers.dev`.
- Current Cloudflare Worker version: `e787a040-70c7-45e0-a1d8-087c924c363c`.

Escalation: none.
Confidence: high.

### Re: Snapshot reconciliation — gpt-5

Reconciled the session snapshot after the no-follow-up proof wording correction.

There were no actionable follow-up items to capture.

### Re: Shareable pipeline URLs, admin message bubble, deploy — gpt-5

> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline > Qualified Lead and every other pipeline item should have a unique url so links can be shared between admins. This should use pushState so the page does not reload.

> **Queue prompt:**
>
> The admin page should have the message bubble in the bottom right corner. The message bubble opens the "unified chat interface" implemented above. If not on a page with user(s), have a searchable selection to open the message for any user in the system.

> **Queue prompt:**
>
> deploy

Implemented, pushed, and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/pipeline.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/messages.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/layout.tsx`
- `src/equityfundingsource-dev/lib/ui--server--efs/portal.ts`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- parent submodule pointer `src/equityfundingsource-dev`

What changed:
- Pipeline cards now produce shareable in-place dialog URLs for loan, deal, lead, and incomplete-application cards via `history.pushState`, and restore those dialogs from the URL after refresh. Closing/replacing the dialog state still avoids a page reload.
- The admin shell now mounts a bottom-right Messages bubble. The bubble opens the admin Messages hub with role tabs, All Messages, and Contact Messages; user/thread selection inside the bubble uses `sync_url={false}` so it does not rewrite the current admin page URL.
- Added sizing CSS for the admin launcher panel and regression coverage for both the pipeline URL contract and the admin launcher wiring.

Verification:
- `bun test tests/admin-pipeline.test.ts tests/portal-messages-component.test.ts` — passed, 76 tests.
- `make check` — passed, 1168 tests plus app build and worker build.
- `gh run list --workflow CI --limit 1` — no workflow named `CI` exists for this repo.
- Pre-deploy `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` — `No migrations to apply!`.
- Pre-deploy `bun run cf:d1:info` and `bun run cf:d1:time-travel:info` — D1 info read succeeded; current Time Travel bookmark recorded by Wrangler.
- `make deploy` — passed; Worker deployed at `https://equityfundingsource.brian-takita.workers.dev`; current Worker version `5f29f309-53dc-46d2-9da9-9d2bf699d173`.
- Post-deploy migrations list — `No migrations to apply!`.
- Post-deploy smoke tests: `/` returned `200`; `/admin/pipeline` returned expected auth redirect `303`.

Commits:
- EFS submodule: `c476fcf4a8c8b18a1879d3b09d3516dfad1ebae0` (`feat(admin): add shareable pipeline dialog links`), pushed to `equityfundingsource-dev/main`.
- Parent repo: `f515046d5` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main`. The push also published pre-existing parent commit `dbed747e1` (`chore: update agent-doc submodule`) that was already ahead before this work.

Escalation: none.
There were no actionable follow-up items to capture.
Confidence: high.

Suggested commit message:

```txt
feat(admin): add shareable pipeline dialog links

Add pushState-backed Deal Pipeline dialog URLs for loan, deal, lead, and incomplete-application cards, plus a bottom-right admin Messages launcher that opens the existing searchable Messages hub without rewriting the current page URL. Covers the wiring with admin pipeline tests and deploys the Worker.
```

### Re: Admin message dialog full screen and deploy — gpt-5

> **Queue prompt:**
>
> Esc key should close the admin message dialog. The admin message dialog should be the full screen.

> **Queue prompt:**
>
> deploy

Implemented, pushed, and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/messages.tsx`
- `src/equityfundingsource-dev/lib/ui--server--efs/portal.ts`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- parent submodule pointer `src/equityfundingsource-dev`

What changed:
- The admin Messages launcher now opens through the shared full-screen `PortalModal`, so Escape, backdrop click, focus handoff, and the standard modal close affordance all use the central modal stack.
- The launcher panel now fills the viewport and puts the embedded conversations in the standard `modal-body` scroll region.
- The admin pipeline regression test now locks the full-screen launcher contract, close affordance, and viewport-height message body sizing.

Verification:
- `bun test tests/admin-pipeline.test.ts tests/portal-table-dialog.test.ts tests/portal-messages-component.test.ts` — passed, 91 tests; the expected non-fatal `d1 down` log is from the outbox failure test.
- `git diff --check` — passed.
- `make check` — passed; exit 0, with the same expected non-fatal `d1 down` log.
- `gh run list --workflow CI --limit 1` — no workflow named `CI` exists for this repo.
- Pre-deploy remote migration list — `No migrations to apply!`.
- Pre-deploy D1 info and Time Travel info — succeeded; current Time Travel bookmark `000004f6-00000010-00005093-7c700bcf75341d5877758c6abb5e3e0a`.
- `make deploy` — passed; Worker deployed at `https://equityfundingsource.brian-takita.workers.dev`; current Worker version `31399fb1-fb21-4303-a294-d5f6ec6cc317`.
- Post-deploy remote migration list — `No migrations to apply!`.
- Post-deploy smoke tests: `/` returned `200`; `/admin/pipeline` returned expected auth redirect `303`.

Commits:
- EFS submodule: `416510e339ffb052844d34133e0b731a8e7eda33` (`fix(admin): make messages launcher full screen`), pushed to `equityfundingsource-dev/main`.
- Parent repo: `bff48727e` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main`.

Escalation: none.
There were no actionable follow-up items to capture.
Confidence: high.

Suggested commit message:

```txt
fix(admin): make messages launcher full screen

Render the admin Messages launcher in the shared full-screen PortalModal so Escape and the standard close affordance close it, and size the embedded conversation body with the modal-body scroll contract. Covers the launcher contract with admin pipeline tests and deploys the Worker.
```

### Re: Message dialog shortcuts and deploy — gpt-5

> **Queue prompt:**
>
> Add Ctrl+M/Cmd+M to toggle the message dialog open/close on all portals.

> **Queue prompt:**
>
> In the Admin Message dialog, add Alt+B to open Borrowers tab, Alt+L to open Lenders tab, Alt+r to open Brokers tab. Underline the B, L, and r.

> **Queue prompt:**
>
> In the Admin Message dialog, when a tab is opened, select the search input so the admin can begin typing in the search term.

> **Queue prompt:**
>
> deploy

Implemented, pushed, and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/shared/ui.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/shared/portal_messages.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/messages.tsx`
- `src/equityfundingsource-dev/tests/msg-send-shortcut.test.ts`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- parent submodule pointer `src/equityfundingsource-dev`

What changed:
- Added a shared Ctrl/Cmd+M message-dialog shortcut helper.
- Wired Ctrl/Cmd+M to toggle the borrower, lender, broker, and admin floating message launchers. Inline admin thread cards do not bind the global shortcut.
- Added Admin Message dialog Alt+B, Alt+L, and Alt+R role-tab shortcuts.
- Underlined the shortcut letters in Borrowers, Lenders, and Brokers.
- Focuses and selects the Admin Message dialog search input when the dialog opens, when the Messages tab opens, and when a role tab is selected.

Verification:
- `bun test tests/msg-send-shortcut.test.ts tests/admin-pipeline.test.ts` — passed, 76 tests.
- `bun test tests/portal-messages-component.test.ts tests/message-avatar-shortcut.test.ts tests/msg-send-shortcut.test.ts tests/admin-pipeline.test.ts` — passed, 106 tests.
- `git diff --check` — passed.
- `make check` — passed after rebasing onto current `origin/main`: 1170 tests plus app build and worker build.
- `gh run list --workflow CI --limit 1` in both the EFS submodule and parent repo — no workflow named `CI` exists.

Deployment:
- Pre-deploy D1 migration list showed pending `0104_deal_photos.sql`, `0105_borrower_document_requests.sql`, and `0106_other_documents.sql` from the remote commits rebased under this work.
- Pre-deploy D1 info and Time Travel info succeeded; bookmark `000004f6-0000003a-00005094-3854784ad3a70f53f7cd3bd60f051cfa` recorded before deploy.
- `make deploy` — passed; applied migrations `0104` through `0106`, uploaded 2 updated browser assets, and deployed Worker `https://equityfundingsource.brian-takita.workers.dev`.
- Current Worker version: `6eabdc5f-ad2a-46b2-9113-6219bd06d37c`.
- Post-deploy migration list — `No migrations to apply!`.
- Post-deploy smoke tests: `/` returned `200`; `/admin/pipeline` returned expected admin-login redirect `303`.

Commits:
- EFS submodule: `5b7e58017f2f10f88bbc84a51350de41c3fe2086` (`feat(portal): add message dialog shortcuts`), pushed to `equityfundingsource-dev/main`.
- Parent repo: `baa17c268c207486decb449f06a2957958913355` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main`.

Escalation: none.
Preflight warnings were existing backlog/review cleanup candidates and were left untouched.
There were no actionable follow-up items to capture.
Confidence: high.

### Re: Admin message keyboard navigation and deploy — gpt-5

> **Queue prompt:**
>
> In Admin message dialog: While the search input is focused, support Up/Down arrow keys to select previous/next message. Enter will focus on the conversation input. While in the conversation input, Esc will focus on the search input.

> **Queue prompt:**
>
> deploy

Implemented, pushed, and deployed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/messages.tsx`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- parent submodule pointer `src/equityfundingsource-dev`

What changed:
- Added explicit search/composer focus helpers in the admin Messages dialog.
- Up/Down from the search input now selects the previous/next visible conversation while keeping the search field usable.
- Enter from the search input focuses the conversation composer.
- Escape from the composer returns focus to the search input.
- Extended the existing admin launcher regression test to lock the keyboard-navigation contract.

Verification:
- `bun test tests/admin-pipeline.test.ts` — passed, 63 tests; the expected non-fatal `d1 down` log remains from the outbox failure test.
- `git diff --check` — passed.
- `make check` — passed, 1170 tests plus app build and worker build.
- `gh run list --workflow CI --limit 1` in both EFS and parent repos — no workflow named `CI` exists.

Deployment:
- Pre-deploy `git pull --rebase origin main` — already up to date.
- Pre-deploy remote migration list — `No migrations to apply!`.
- Pre-deploy D1 info succeeded; Time Travel bookmark `000004f6-00000064-00005094-cdc8cb457f0ab2e5b052471146ef5931` recorded.
- `make deploy` — passed; no migrations applied, 1 updated browser asset uploaded, Worker deployed at `https://equityfundingsource.brian-takita.workers.dev`.
- Current Worker version: `4aedf8cb-47de-47ed-8818-4f03fc818752`.
- Post-deploy migration list — `No migrations to apply!`.
- Post-deploy smoke tests: `/` returned `200`; `/admin/pipeline` returned expected admin auth redirect `303`.

Commits:
- EFS submodule: `af537e6130ccdfb842d450dc46d05bf8e160f6b8` (`feat(admin): add message dialog keyboard navigation`), pushed to `equityfundingsource-dev/main`.
- Parent repo: `5b093017f` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main`.

Escalation: none.
Preflight warnings were existing backlog/review cleanup candidates and were left untouched.
There were no actionable follow-up items to capture.
Confidence: high.

Suggested commit message:

```txt
feat(admin): add message dialog keyboard navigation

Add keyboard navigation to the admin Messages dialog so Up/Down from search moves between conversations, Enter focuses the composer, and Escape returns from the composer to search. Covers the contract in the admin pipeline regression test and deploys the Worker.
```

### Re: Queue residue repair — gpt-5

Cleared the two already-answered free-text queue heads after the closeout commit so the queue will not re-run stale work.
<!-- agent:boundary:e1b144dd -->
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

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- In the pipeline views (Deal + Broker), the arrow keys navigate between the items. Up/Down in the same column, Left/Right move between columns. Pressing up at the first item or empty column selects the +/- toggle. Enter activates the selected item.
- deploy
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
