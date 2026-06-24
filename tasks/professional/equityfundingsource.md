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
queue: start
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260624-050641.md`*

Compacted content:
❯ - Archived 4 response topic(s): Lender signup first-password flow + deploy; Deploy queue closeout; IPC recovery closeout; 1 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...
**IPC proof issue dogfood log**

Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=7a2a4a5d-921b-4613-80c4-40f1650ee082 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782277467207
```
Implemented and pushed the lender-pipeline dialog URL work, but I could not complete the required `agent-doc` document closeout.

> **Queue prompt:**
>
> In https://equityfundingsource.brian-takita.workers.dev/admin/lender-pipeline, all dialogs should have a unique url using pushState. So the links can be shared.


Product commits:
- EFS: `2415765` `feat(admin): share lender pipeline dialog URLs`
- Parent gitlink: `9a5cfc236` `chore: update equityfundingsource submodule`

Verification passed:
- `git diff --check`
- focused dialog tests
- `make check` with 1180 tests, app build, Worker build
- `gh run list --workflow CI --limit 1` found no `CI` workflow in either repo

Blocker: `agent-doc finalize` and `agent-doc write --commit` repeatedly failed with `editor IPC did not prove the write`; the pending response is retained for retry, but the session document was not updated. `agent-doc admin inspect` reports `supervisor:stale`, and `kill-supervisor --dry-run` refused because that supervisor is this session’s own ancestor. The queue head is still active in the session doc.

### Re: Shareable lender-pipeline dialog URLs — gpt-5

> **Queue prompt:**
>
> I just signed up as btak.div+lender3@gmail.com. This user has Requirements Current in the Lender Pipeline. Why are the requirements current? The lender has not completed the signup...adding documents.


> **Queue prompt:**
>
> In https://equityfundingsource.brian-takita.workers.dev/admin/lender-pipeline, all dialogs should have a unique url using pushState. So the links can be shared.

Implemented, committed, and pushed.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/investor_pipeline.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/funding_commits.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/lender_management.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/lender_documents.tsx`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/table-row-opens-dialog.test.ts`
- `src/equityfundingsource-dev/tests/admin-dashboard.test.ts`
- `src/equityfundingsource-dev/prds/investor-pipeline-admin.md`

What changed:
- The Lender Pipeline tab itself now follows `popstate`, so browser back/forward restores the tab from the URL.
- The existing Pipeline card / Lender Profile / Advance Lender dialog URLs remain intact.
- Added shareable URL state for the embedded Funding Commits allocation editor, Lenders request dialog, and lender document preview dialogs.
- Dialog links now clear stale sibling dialog query params when switching dialog families, so shared URLs carry one coherent dialog target.
- Document previews in both the Documents tab and Advance Lender required-documents checklist use `pushState` and restore from URL after the document list loads.

Verification:
- `git diff --check` — passed.
- `bun test tests/admin-investor-pipeline.test.ts tests/table-row-opens-dialog.test.ts` — passed, 97 tests / 0 failures.
- `bun test tests/admin-dashboard.test.ts -t "dashboard attention links deep-link into pipeline dialogs"` — passed.
- `make check` — passed, 1180 tests / 0 failures, app build, and Worker build. The known non-fatal `error: d1 down` fixture in `admin-pipeline.test.ts` still logs during the suite.
- `gh run list --workflow CI --limit 1` in both EFS and parent repos — no workflow named `CI` exists.

Commits:
- EFS submodule: `2415765` (`feat(admin): share lender pipeline dialog URLs`), pushed to `equityfundingsource-dev/main`.
- Parent gitlink: `9a5cfc236` (`chore: update equityfundingsource submodule`), pushed to `agent-loop/main` from a temporary `origin/main` worktree so unrelated local parent commits/files were not published.

Deployment: not run for this queue head; a separate `deploy` queue item remains.

Escalation: none.
Confidence: high.



```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=bde39450-2463-48cb-8bcd-39c72c8d1a49 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782279548507
```
### Re: IPC recovery closeout — gpt-5

Recovered the orphaned closeout and corrected the queue state. The prior recovery pass struck the next Requirements Current prompt even though this cycle only completed the shareable lender-pipeline dialog URL work, so I restored that prompt as the live queue head.

Verification:
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md --component queue` confirmed the queue drift before this correction.
- No product code changes in this closeout.

Escalation: none.
Confidence: high.



```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=ea287f42-466c-4275-a70a-c50f3e5aa01f invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782279950206
```

### Re: Why `btak.div+lender3@gmail.com` is showing Requirements Current

Root cause: the production Worker is still running the old lender-pipeline requirements predicate. That old predicate treated signup self-attestation (`users.accredited_confirmed_at`) as enough to make annual Requirements `current`, so a lender who checked the accredited/self-attestation box during signup could land in `requirements_current` before completing the document/evidence flow.

For `btak.div+lender3@gmail.com`, the live D1 row is:
- `users.accreditation_status = 'pending'`
- `users.accredited_confirmed_at = 1782274175` (`2026-06-24T04:09:35Z`)
- no latest `lender_accreditation_verifications` row
- no `evidence_document_id`
- no `lender_account`

A direct SQL projection shows the difference:
- old deployed logic: `requirements_current`
- current checked-in logic: `signed_up`

The checked-in fix is already on `main` as `dd1969c fix(admin): require verified lender requirements`, committed `2026-06-24T05:55:28Z`. It changes `investor_requirements_status_()` so Requirements become `current` only when `accreditation_status === 'verified'`; `accredited_confirmed_at` remains an accreditation/self-attestation flag but no longer clears annual Requirements. The latest Cloudflare deployment I found is version `77deacf5-86ec-4093-8910-055d8ea7e0ac`, created `2026-06-24T04:59:43Z`, which predates that fix.

So the answer is: Requirements are current because production has not yet deployed the fix that separates signup self-attestation from admin-verified Requirements evidence. Deploying current `main` should move this user to Signed Up / Requirements Not started until they submit evidence and an admin verifies it.

Changed paths: no product code changes in this cycle. Existing product fix is already in `src/equityfundingsource-dev` at `dd1969c`.

Verification:
- `tsift status --json .` in `src/equityfundingsource-dev` reported a fresh index.
- Queried remote D1 for `btak.div+lender3@gmail.com` and confirmed pending user status, self-attestation timestamp, no verification row, no evidence document, and no lender account.
- Queried remote D1 with old-vs-current stage projection: old deployed logic returns `requirements_current`; checked-in logic returns `signed_up`.
- `bun test tests/admin-investor-pipeline.test.ts` passed: 93 tests, 0 failures.
- `wrangler deployments list` via the repo wrapper showed latest deployment at `2026-06-24T04:59:43Z`, before `dd1969c`.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI` in this repo.

Escalation: deploy current `main` when ready; no data correction is required for this specific row.
Confidence: high.

### Re: Requirements Current for incomplete signup - gpt-5

> **Queue prompt:**
>
> I just signed up as btak.div+lender3@gmail.com. This user has Requirements Current in the Lender Pipeline. Why are the requirements current? The lender has not completed the signup...adding documents.

Root cause: `investor_requirements_status_()` treated `users.accredited_confirmed_at` as enough to make annual Requirements `current`. That field is only the lender signup self-attestation, so a new lender who checked the accredited box during signup could incorrectly land in Requirements Current before uploading evidence.

Implemented fix:
- `app/equityfundingsource/lib/admin/repo.ts` now marks Requirements current only when `accreditation_status === 'verified'`; signup self-attestation alone stays `not_started`.
- `app/equityfundingsource/lib/admin/investor_management/index.ts` now feeds the pipeline from the latest `lender_accreditation_verifications.status`, with `users.accreditation_status` only as a legacy fallback.
- `tests/admin-investor-pipeline.test.ts` covers the reported pending/self-attested signup state and the verified-but-missing-account state separately.
- `prds/investor-pipeline-admin.md` and `docs/master-state-machine.md` document that admin-verified evidence, not self-attestation, drives Requirements Current.

Verification:
- `git diff --check`
- `bun test tests/admin-investor-pipeline.test.ts`
- `make check` (1180 tests, app build, worker build)
- `gh run list --workflow CI --limit 1` in both the EFS submodule and parent reported: `could not find any workflows named CI`.

Commits:
- EFS submodule: `dd1969c fix(admin): require verified lender requirements`
- Parent gitlink: `ff6f88f87 chore: update equityfundingsource submodule`

Deployment: not run for this queue head; the separate `deploy` queue item remains.

Escalation: none.
Confidence: high.
<!-- agent:boundary:0c84fc41 -->
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
- ~~In https://equityfundingsource.brian-takita.workers.dev/admin/lender-pipeline, all dialogs should have a unique url using pushState. So the links can be shared.~~
- ~~I just signed up as btak.div+lender3@gmail.com. This user has Requirements Current in the Lender Pipeline. Why are the requirements current? The lender has not completed the signup...adding documents.~~
- In the Lender Pipeline > Requirements Dialog, remove the Message Lender section. The contextual messaging (chat bubble) should open to the message the lender for the card.
- In the Requirements Dialog, support the up/down arrow to jump between buttons and fields. This includes alerts. So if the Advance Lender button is present, it should be in focus (support Enter) when the dialog opens. Pressing the down arrow should jump to the next actionable field...which would be Close...then Advance Lender. The up arrow key navigates to the previous actionable item.
- In the Requirements Dialog, support the up/down arrow to jump between buttons and fields. This includes alerts. So if the Advance Lender button is present, it should be in focus (support Enter) when the dialog opens. Pressing the down arrow should jump to the next actionable field.
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
