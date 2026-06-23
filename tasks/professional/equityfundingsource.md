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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260623-155553.md`*

Compacted content:
❯ - Archived 4 response topic(s): Message lender textarea label; deploy; Advance lender hold action; 1 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: Advance Lender TMO sync action — gpt-5

> **Queue prompt:**
>
> ~~https://equityfundingsource.brian-takita.workers.dev/admin/lender-pipeline > Advance Lender > Link TMO account...there should be a Sync with TMO button with a closed Manually Link Account `<details>` that has the link form.~~ — auto-struck: answered this cycle (#ftstrike)


> **Queue prompt:**
>
> https://equityfundingsource.brian-takita.workers.dev/admin/lender-pipeline > Advance Lender > Link TMO account...there should be a Sync with TMO button with a closed Manually Link Account `<details>` that has the link form.

Updated the Advance Lender hold dialog so the no-request path now leads with a `Sync with TMO` primary button. The button calls the existing `POST /api/admin/tmo-sync` endpoint, shows success/error toast feedback, and refreshes the lender pipeline after a successful sync.

The direct account-number form is still available, but it now lives inside a default-closed `Manually Link Account` `<details>` block with the existing `PUT /api/admin/users/:id/lender-account` flow. The copy now tells the admin to sync first, then use manual linking only when the exact TMO lender account number is known.

EFS commit: `bc64531 fix: add tmo sync to advance lender hold`, pushed to `origin/main`.

Parent repo submodule pointer commit: `37eda0485 chore: update equityfundingsource submodule`. I did not push the parent repo because it was already ahead of `origin/main` and has unrelated dirty worktree state; pushing from here would publish unrelated work.

Changed paths:

- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`
- `src/equityfundingsource-dev/tests/advance-lender-dialog.test.ts`
- `src/equityfundingsource-dev` submodule pointer in the parent repo

Commands:

- `tsift status` in the parent repo and EFS submodule
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- targeted `tsift --envelope search`, `tsift --envelope source-read`, `rg`, and `sed` inspections for Advance Lender, TMO sync, and lender-account code
- `bun test tests/advance-lender-dialog.test.ts tests/admin-investor-pipeline.test.ts tests/advance-lender-documents.test.ts`
- `make check` in `src/equityfundingsource-dev`
- `gh run list --workflow CI --limit 1` and `gh workflow list` in `src/equityfundingsource-dev`
- `git commit -m "fix: add tmo sync to advance lender hold"` in `src/equityfundingsource-dev`
- `git push origin main` in `src/equityfundingsource-dev`
- `git commit -m "chore: update equityfundingsource submodule"` in the parent repo
- `gh run list --workflow CI --limit 1` and `gh workflow list` in the parent repo
- `make check` in the parent repo

Verification:

- Focused EFS tests passed: 107 tests across `advance-lender-dialog`, `admin-investor-pipeline`, and `advance-lender-documents`.
- Full EFS `make check` passed: 1,157 tests, app build, and Worker build.
- EFS `gh run list --workflow CI --limit 1` reported no workflow named `CI`; `gh workflow list` returned no workflows.
- Parent `gh run list --workflow CI --limit 1` reported no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- Parent `make check` passed, including instruction-files, agent-doc, and corky suites.
- EFS submodule is clean on `main...origin/main` after push.

Confidence: high.

Escalation: none. Production was not deployed in this cycle; `deploy` remains as the next queue prompt.

### Re: Advance Lender TMO sync action - gpt-5

> **Queue prompt:**
>
> "Link TMO account here" should perform the automated sync with TMO. Not just expect the user to enter the id from TMO.

Updated the Advance Lender flow so the primary action for pending and missing lender-account holds is now `Sync with TMO`, not a deep-link to the Lender Requests page or manual account-number entry. The modal keeps `Manually Link Account` collapsed as a fallback for any unlinked lender, and the specs/tests document that sync-first contract.

Important implementation note: I did not add a new TMO lender API call. The local TMO docs still say lender creation is manual and not to call TMO lender endpoints from the investor portal, so this uses the existing admin `POST /api/admin/tmo-sync` path and leaves exact-account-number entry as fallback.

EFS commit: `df98a8f fix: make advance lender tmo sync primary`, pushed to `origin/main`.

Parent repo submodule pointer commit: `b2443938f chore: update equityfundingsource submodule`. I did not push the parent repo because it is already ahead of `origin/main` with unrelated agent-doc history and has unrelated dirty worktree state; pushing from here would publish unrelated work.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`
- `src/equityfundingsource-dev/tests/advance-lender-dialog.test.ts`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/docs/TMO/new-lender.md`
- `src/equityfundingsource-dev/prds/investor-pipeline-admin.md`
- `src/equityfundingsource-dev` submodule pointer in the parent repo

Commands:
- `tsift status` in parent and EFS submodule
- `agent-doc preflight tasks/professional/equityfundingsource.md`
- `agent-doc plan tasks/professional/equityfundingsource.md`
- `agent-doc read tasks/professional/equityfundingsource.md`
- targeted `rg`, `tsift --envelope source-read`, and `sed` inspections for Advance Lender/TMO sync/lender account routes
- `bun test tests/advance-lender-dialog.test.ts tests/admin-investor-pipeline.test.ts`
- `git diff --check`
- `make check` in `src/equityfundingsource-dev`
- `gh run list --workflow CI --limit 1` and `gh workflow list` in EFS
- `git commit -m "fix: make advance lender tmo sync primary"` in EFS
- `git push origin main` in EFS
- `make check` in parent
- `gh run list --workflow CI --limit 1` and `gh workflow list` in parent
- `git commit -m "chore: update equityfundingsource submodule"` in parent

Verification:
- Focused EFS tests passed: 100 tests across `advance-lender-dialog` and `admin-investor-pipeline`.
- Full EFS `make check` passed: 1,157 tests, app build, and Worker build.
- EFS `gh run list --workflow CI --limit 1` reported no workflow named `CI`; `gh workflow list` returned no workflows.
- Parent `make check` passed.
- Parent `gh run list --workflow CI --limit 1` reported no workflow named `CI`; `gh workflow list` shows only `Agent Doc` active.
- EFS submodule is clean on `main...origin/main` after push.
- Parent worktree still has unrelated dirty paths, but the EFS submodule pointer change is committed.

Confidence: high.

Escalation: none. Production was not deployed in this cycle; `deploy` remains as the next queue prompt.

### Re: Deploy - gpt-5

> **Queue prompt:**
>
> deploy

Deployed the current EFS Worker build to Cloudflare production at `https://equityfundingsource.brian-takita.workers.dev`.

Deployment details:
- EFS code deployed from `src/equityfundingsource-dev` on `main...origin/main`.
- Worker deploy command completed successfully: `bun run deploy:worker`.
- Cloudflare Worker version: `9cd4405e-9d90-47e0-ae2e-d0d2f360aee1`.
- Deployed static asset update: `/index.browser-HINKPVJJ.js`.
- Cron trigger deployed with the Worker: `0 * * * *`.

Changed paths:
- None in this cycle. This was a deploy-only cycle for the already-committed EFS changes.

Commands:
- `agent-doc --version`
- `tsift status` in the parent repo
- `agent-doc preflight tasks/professional/equityfundingsource.md`
- `agent-doc plan tasks/professional/equityfundingsource.md`
- `agent-doc read tasks/professional/equityfundingsource.md`
- `tsift status` in `src/equityfundingsource-dev`
- `git status --short --branch` in EFS and parent
- bounded deploy-runbook/package inspections with `sed` and `rg`
- `bun run cf:whoami`
- `bun run cf:d1:info`
- `bun run cf:d1:time-travel:info`
- `bun run cf:d1:migrations:list:remote`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
- `bun run deploy:worker`
- live smoke `curl` checks for `/`, `/funded-deals`, `/admin/login`, `/admin/lender-pipeline`, `/blog`, `/blog/california-private-lending-guide`, and `/index.browser-HINKPVJJ.js`
- `bun run cf:d1:query:remote -- "SELECT COUNT(*) AS n FROM funded_deals WHERE status = 'visible'"`
- `bun run cf:d1:query:remote -- "SELECT COUNT(*) AS n FROM blog_posts WHERE status = 'published'"`
- `gh run list --workflow CI --limit 1` and `gh workflow list` in EFS and parent

Verification:
- Cloudflare auth succeeded for account `1e7e194ff8da4ad9e40ce041aea84f76`.
- Remote D1 `equityfundingsource` is reachable at database id `280e1539-b940-4c59-8988-fbed63c12ac0`.
- D1 time-travel current bookmark before deploy: `000004e5-00000000-00005093-e745e8e6a8254df782e97af71eac8074`.
- Remote D1 migrations reported `No migrations to apply!` before deploy, and deploy also applied migrations with `No migrations to apply!`.
- Local EFS `make check` passed: 1,157 tests, 0 failures, app build, and Worker build. The tsift digest classifier grouped one logged `error: d1 down` message, but the command exited 0 and the test output reported `1157 pass / 0 fail`.
- Worker deploy succeeded and uploaded `equityfundingsource` version `9cd4405e-9d90-47e0-ae2e-d0d2f360aee1`.
- Live smoke checks passed: `/` 200, `/funded-deals` 200, `/admin/login` 200, `/admin/lender-pipeline` 303 to admin login, `/blog` 200, `/blog/california-private-lending-guide` 200, `/index.browser-HINKPVJJ.js` 200.
- Remote D1 read checks passed: visible funded deals count `18`; published blog posts count `1`.
- EFS has no GitHub workflows; parent has no `CI` workflow and lists only `Agent Doc`.
- EFS submodule remains clean on `main...origin/main`.
- Parent repo still has unrelated dirty paths and is ahead of origin; no parent push was performed.

Confidence: high.

Escalation: none.
Implemented and pushed the EFS fix.

> **Queue prompt:**
>
> I synced the lender with TMO but got "TMO sync complete" "Updated 0 loans, 0 fundings, 0 schedules. Refreshing the lender pipeline."


The Advance Lender sync result now makes the real behavior explicit: it is a TMO loan/funding/schedule/cache refresh, not an auto-link for newly created TMO lender accounts. If the result is all zeroes, the dialog shows a warning and tells the admin to use `Manually Link Account` with the exact TMO lender account number.

Commits:
- EFS: `c9d8dae fix: clarify lender tmo sync result`, pushed to `origin/main`
- Parent submodule pointer: `ed99a6f0a chore: update equityfundingsource submodule`, not pushed because the parent repo is already ahead and has unrelated dirty paths

Verification passed:
- `bun test tests/advance-lender-dialog.test.ts`
- `bun test tests/advance-lender-dialog.test.ts tests/admin-investor-pipeline.test.ts`
- EFS `make check`: 1,158 tests, 0 failures, app build, Worker build
- Parent `make check`

One closeout issue remains: `agent-doc` could not get editor IPC proof to write the response into `tasks/professional/equityfundingsource.md`; it queued the patch but the editor did not acknowledge/apply it. I did not force a disk write because that would bypass the active editor/CRDT path.

### Re: TMO lender sync rationale — gpt-5

> **Queue prompt:**
>
> "The local TMO docs still say lender creation is manual and not to call TMO lender endpoints from the investor portal, so this uses the existing admin `POST /api/admin/tmo-sync` path and leaves exact-account-number entry as fallback.". Why?

Because the repo currently has two different concepts that look similar in the UI but are not the same integration.

`POST /api/admin/tmo-sync` is an admin-only TMO loan-data refresh. The implementation starts from local loans that already have `tmo_loan_number`, fetches TMO loan/payment-schedule/funding data, updates EFS read models, and writes cache/change-log records. It does not search TMO for lender accounts, create lenders, or link a newly created TMO lender back to an EFS user.

The local lender onboarding contract is still manual: investor submits lender info to EFS, admin reviews it, admin creates or finds the lender in TMO, then EFS stores the exact TMO account number in `users.lender_account`. `docs/TMO/new-lender.md` is explicit about that boundary: do not call TMO lender endpoints from the investor portal; lender creation is manual in TMO; the sync refreshes loan/funding/schedule/cache data and does not auto-discover newly created lender account numbers.

So the reason I left manual exact-account entry as the fallback is risk control, not preference. We do not yet have a confirmed TMO lender create/search/link API contract, field map, duplicate-prevention rule, idempotency key, or error semantics for mutating TMO lender master records from EFS. If we guessed at those endpoints from the investor portal or from the Advance Lender dialog, the likely failure mode would be duplicate or incorrectly linked TMO lender records.

I added a recommended backlog item for the separate real automation: confirm the TMO lender endpoint/search/create/link contract, then build an admin-only idempotent flow that can safely create or find the TMO lender and store the exact account number automatically. Until that exists, the current button should be understood as "refresh known TMO loan data," not "create/link this lender account in TMO."

Changed paths:
- None in repo code. This was a document response plus one recommended backlog item.

Commands:
- `tsift status`
- `tsift status --fix`
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc read` for `exchange` and `queue`
- targeted `rg` and `sed` inspections of `docs/TMO/new-lender.md`, `prds/investor-pipeline-admin.md`, `app/equityfundingsource/lib/tmo-sync.ts`, `app/equityfundingsource/lib/admin/loan_management/index.ts`, and `app/equityfundingsource/lib/admin/routes.ts`

Verification:
- Documentation and source inspection only; no code changed, so no test suite was run.
- Confirmed `POST /api/admin/tmo-sync` routes to `admin_tmo_sync_POST_`, which calls `tmo_sync_all_`.
- Confirmed `tmo_sync_all_` iterates existing local loans with `tmo_loan_number`, syncs TMO loan/payment-schedule/funding/cache data, and has no lender account create/search/link behavior.
- Confirmed `docs/TMO/new-lender.md` says lender creation is manual in TMO, TMO lender endpoints should not be called from the investor portal, and manual account-number entry remains required when sync does not link the lender.

Confidence: high.

Escalation: none.
<!-- agent:boundary:5ba0b308 -->
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
- "The local TMO docs still say lender creation is manual and not to call TMO lender endpoints from the investor portal, so this uses the existing admin `POST /api/admin/tmo-sync` path and leaves exact-account-number entry as fallback.". Why?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send; no active unused Robert login tokens remain.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
