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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260624-200744.md`*

Compacted content:
❯ - Archived 9 response topic(s): Full Application Property Type Dropdown; Pipeline Edit Field Audit And Deploy; Step 6 Review & Sign Optional And Deploy; 6 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: Remove Lead Dialog Open Page And Deploy — gpt-5

> **Queue prompt:**
> In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1, remove Open Page button.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/admin/pipeline.tsx`, `tests/admin-pipeline.test.ts`, and `prds/investor-pipeline-admin.md`. The Deal Pipeline lead dialog no longer renders the header `Open Page` button; the footer `Open Lead` deep-link remains available for admins who need the standalone lead page.

Commits: EFS submodule commit `2e6a267 fix(admin): remove lead dialog open page button` was pushed to `origin/main`. Parent repo gitlink commit `89851f8c2 chore(efs): bump lead dialog submodule` was committed locally only; I did not push parent `main` because it is already ahead and dirty with unrelated work.

Verification: focused `bun test tests/admin-pipeline.test.ts` passed (`64 pass / 0 fail`; it logs the known `d1 down` fixture in the non-fatal outbox test). `git diff --check` passed. Full `make check` passed (`1192 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run in either the EFS submodule or parent repo because neither has a workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no pending migrations, D1 info was reachable, and Time Travel bookmark was `0000050b-00000028-00005094-d711a439306e78783077d8c94632d00b`. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `23025fdb-6771-4184-9015-a4425b223197`. Post-deploy checks: remote D1 migrations still reported no migrations to apply; public root returned `200`; the protected admin lead dialog URL returned `303` behind Cloudflare Access.

Suggested commit message:

```txt
fix(admin): remove lead dialog open page button (#pipeline-leads)

Remove the duplicate header Open Page action from the Deal Pipeline lead dialog while keeping the footer Open Lead deep-link. Updates the pipeline regression test and PRD note, with no schema or endpoint changes.
```

Escalation: none. Confidence: high.

### Re: Lead Dialog Next Step And Deploy — gpt-5

> **Queue prompt:**
>
> 🚧 https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1. Remove the Open Lead button.


> **Queue prompt:**
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1. Remove the Open Lead button.
>
> **Queue prompt:**
> In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1, show an alert with a CTA to advance the lead to the next stage. Does it mean the lead should sign up for an account? I believe btak.dev+borrower3@gmail.com already signed in.
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed.

Answer: the next stage is not necessarily creating a new account. The admin CTA now sends or resends the borrower invite for the full-application path. If the borrower already has an account, the same invite path lets them sign in and continue rather than creating a second account.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/admin/application_lead_detail.tsx`, `lib/ui--browser--efs/portal/admin/pipeline.tsx`, `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx`, `tests/admin-application-lead-detail.test.ts`, `tests/admin-pipeline.test.ts`, and `prds/investor-pipeline-admin.md`. The Deal Pipeline lead dialog no longer shows standalone `Open Page` / `Open Lead` actions. The embedded lead detail now shows a top next-step alert with a `Send borrower invite` / `Resend borrower invite` CTA using the existing invite endpoint.

Commits: EFS submodule commit `8abe2db fix(admin): surface lead invite next step` was pushed to `origin/main`. Parent repo gitlink commit `370807f6d chore(efs): bump lead next step submodule` was committed locally only; I did not push parent `main` because it is already ahead and dirty with unrelated work.

Verification: focused `bun test tests/admin-pipeline.test.ts tests/admin-application-lead-detail.test.ts` passed (`72 pass / 0 fail`; it logs the known `d1 down` fixture in the non-fatal outbox test). `git diff --check` passed. Full `make check` passed (`1192 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run in either the EFS submodule or parent repo because neither has a workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no pending migrations, D1 info was reachable, and Time Travel bookmark was `0000050b-00000030-00005094-f2fece13b078fb4ae70ceae427111275`. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `6131143f-9cec-4323-884e-b1a3305404dd`. Post-deploy checks: remote D1 migrations still reported no migrations to apply; public root returned `200`; the protected admin lead dialog URL returned `303` behind Cloudflare Access.

Suggested commit message:

```txt
fix(admin): surface lead invite next step (#pipeline-leads)

Replace standalone lead-page actions in the Deal Pipeline lead dialog with a top next-step alert that sends or resends the borrower invite for the full-application path. Existing borrower accounts can use the same invite to sign in and continue; no schema or endpoint changes.
```

Escalation: none. Confidence: high.

### Re: Deploy Queue Residue Cleanup — gpt-5

> **Queue prompt:**
>
> ~~In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1, show an alert with a CTA to advance the lead to the next stage. Does it mean the lead should sign up for an account? I believe btak.dev+borrower3@gmail.com already signed in.~~ — auto-struck: answered this cycle (#ftstrike)


> **Queue prompt:**
> deploy

Struck the answered deploy queue residue after the deployed lead-dialog response committed. No additional app work remained.

Escalation: none. Confidence: high.

### Re: Queue Residue Cleanup — gpt-5

Cleared the already-struck queue residue after the lead dialog deploy response and deploy cleanup response were committed. No app work remained.

Escalation: none. Confidence: high.

### Re: IPC Dogfood Note — gpt-5

Acknowledged the automated IPC proof dogfood note that was appended during queue cleanup recovery. The lead-dialog work is already implemented, pushed, deployed, and the queue is stopped with no remaining head.

No actionable follow-up items are needed for this note.

Escalation: none. Confidence: high.

### Re: IPC Dogfood Note Closeout — gpt-5

Acknowledged the automated IPC proof dogfood note from queue cleanup. The queue is stopped and there is no remaining lead-dialog or deploy work.

No actionable follow-up items are needed for this note.

Escalation: none. Confidence: high.

### Re: Lead Dialog CTA Focus And Deploy — gpt-5

> **Queue prompt:**
> https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1 should focus on the alert CTA when opened and with the direct url. In this case "Resend borrower invite".
>
> **Queue prompt:**
> deploy

Implemented, pushed, and deployed.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/admin/application_lead_detail.tsx`, `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx`, `tests/admin-application-lead-detail.test.ts`, and `prds/investor-pipeline-admin.md`. The lead next-step alert CTA is now marked as the top initial dialog action, and embedded `ApplicationLeadDetail` performs a one-time post-load focus pass after the async lead detail fetch. Direct `dialog=lead` URL restores now focus the alert CTA; for the target lead state, that CTA label is `Resend borrower invite`.

Commit: EFS submodule commit `692acf4 fix(admin): focus lead invite CTA` was pushed to `origin/main`. The parent repo `HEAD` already points `src/equityfundingsource-dev` at `692acf4`; I did not push parent `main` because it is ahead/dirty with unrelated work.

Verification: focused `bun test tests/admin-application-lead-detail.test.ts tests/admin-pipeline.test.ts` passed (`73 pass / 0 fail`; it logs the known non-fatal `d1 down` outbox fixture). `git diff --check` passed. Full `make check` passed (`1193 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Deploy: pre-deploy remote D1 migrations reported no pending migrations, D1 info was reachable, and Time Travel bookmark was `0000050c-00000004-00005094-8090079220923e579c4370ba5db5164d`. `make deploy` completed successfully. Worker URL: `https://equityfundingsource.brian-takita.workers.dev`. Current Version ID: `b344beea-543d-43e9-a5ab-e1040222d2bd`. Post-deploy checks: remote D1 migrations still reported no migrations to apply; public root returned `200`; the protected admin lead dialog URL returned `303` behind Cloudflare Access.

Suggested commit message:

```txt
fix(admin): focus lead invite CTA (#pipeline-leads)

Focus the embedded Deal Pipeline lead next-step invite button after async lead detail load, including direct dialog URL restores. Marks the shared lead alert CTA as the initial dialog action and updates source coverage plus the admin pipeline PRD; no endpoint or schema changes.
```

Escalation: none. Confidence: high.
❯ **IPC proof issue dogfood log**

❯ Detected during interrupted-cycle recovery. This note is appended automatically so the session document records editor IPC issues while queue edits are being dogfooded.

❯ Issue class: `ipc_proof_insufficient`
❯ Affected component: editor IPC / writeback

```text
ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md source=file_ipc patch_id=8b947586-54b3-4eae-903e-6975bee0a257 invariant=no_ack recovery=retry_without_disk_write timeout_secs=2 patch_file=/home/brian/work/btakita/agent-loop/.agent-doc/patches/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad.json doc=equityfundingsource session=62fe1f41-886c-4f3d-b6fa-48d46106ec27 turn=cycle-1782333341357
```

### Re: Deploy Queue Residue And IPC Dogfood Note — gpt-5

> **Queue prompt:**
> deploy

The deploy queue residue is already satisfied by the preceding deployed change. No additional app work was needed after Worker version `b344beea-543d-43e9-a5ab-e1040222d2bd` went live and post-deploy checks passed.

Acknowledged the automatic IPC proof dogfood note. It recorded the transient `ipc_proof_insufficient` writeback failure from the first finalize attempt; the recovery path applied and committed the response. I also requested `agent-doc admin recycle /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md` so the stale route-owned supervisor can recycle at the next idle boundary.

Changed paths this cleanup cycle: none in app code. Commands this cleanup cycle: `agent-doc plan`, `agent-doc read --component queue`, and `agent-doc admin recycle ...`. Verification/deploy evidence remains the prior cycle: focused tests, `git diff --check`, full `make check`, pushed EFS commit `692acf4`, and production deploy version `b344beea-543d-43e9-a5ab-e1040222d2bd`.

No actionable follow-up items are needed for the IPC note.

Escalation: none. Confidence: high.

### Re: IPC Dogfood Note Closeout — gpt-5

Acknowledged the second automatic IPC proof dogfood note from cleanup recovery. The deploy queue is drained, the app change remains deployed at Worker version `b344beea-543d-43e9-a5ab-e1040222d2bd`, and there is no remaining EFS app work from this note.

The route-owned supervisor was already marked for recycle at the next idle boundary with `agent-doc admin recycle /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`. An immediate `agent-doc session restart-supervisor ...` correctly refused because this Codex turn is still the active busy owner; the recycle request is the non-disruptive path.

Changed paths this closeout cycle: none. Commands this closeout cycle: `agent-doc preflight` recovery, `agent-doc session restart-supervisor ...` probe, and `agent-doc read --component queue`.

No actionable follow-up items are needed for this IPC note.

Escalation: none. Confidence: high.

### Re: Deal Pipeline Card Meaning — gpt-5

> **Queue prompt:**
> In https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=lead&dialog_lead_id=c907d851-8870-49aa-a6fc-66d393868ce1, what is the Deal Pipeline card? I don't recognize these steps. They look like a sales pipeline.

You're reading it correctly: that card is the lead workflow/status card, not a separate EFS loan/deal underwriting step. It is titled `Deal Pipeline` because the embedded lead detail component is reused inside the Deal Pipeline dialog and shares the pipeline status controls.

For this lead dialog, the card renders:

- lead status: `New`, `Contacted`, `Converted`, `Archived`
- staff-email notification state
- `Send borrower invite` / `Resend borrower invite` CTA

Those stages are sales/CRM-ish because this record is still a prequalification lead before there is a full application or TMO loan. The backend maps open leads (`new` / `contacted`) into the board's `Draft Applications` funnel column; `converted` and `archived` leave the active lead funnel. Once the borrower has a full application, the board uses application states (`draft`, `submitted`, `in_review`, `accepted`), and once there is a TMO loan it uses loan/TMO statuses such as `Available Deals`, `Loan Estimate`, `Intent to Proceed`, and `Processing`.

So the label is misleading in this context. A clearer title would be `Lead Workflow` or `Lead Follow-up`; the current behavior is about advancing a lead toward the borrower full-application path, not managing a lender/investor deal sale.

Changed paths: none.
Commands: inspected `pipeline.tsx`, `application_lead_detail.tsx`, `lead_pipeline_controls.tsx`, and `repo.ts`; checked the live URL/API, which redirects to admin login or returns `unauthenticated` without an admin session.
Verification: no tests run because this was an explanation-only cycle.
Escalation: none. Confidence: high.

### Re: Queue Residue Cleanup — gpt-5

Cleared the answered free-text queue head after the Deal Pipeline card explanation committed. No additional EFS app work was needed.

Changed paths: none in app code.
Commands: `agent-doc preflight`, `agent-doc session-check`, `agent-doc admin inspect`, and queue component read/cleanup through `agent-doc finalize`.
Verification: explanation-only cleanup; no tests run.
Escalation: none. Confidence: high.

### Re: Remove Deal Pipeline Lead Status Dropdown — gpt-5

> **Queue prompt:**
> Remove "Deal Pipeline" card dropdown. The CRM status can be inferred by the conversation and pipeline status. The conversation is self-evident. Just have the button and a link to open the message conversation.

Implemented and committed.

Changed paths in `src/equityfundingsource-dev`: `lib/ui--browser--efs/portal/admin/application_lead_detail.tsx`, `lib/ui--browser--efs/portal/admin/messages.tsx`, `lib/ui--browser--efs/portal/admin/pipeline.tsx`, `lib/ui--browser--efs/portal/shared/lead_pipeline_controls.tsx`, `tests/admin-application-lead-detail.test.ts`, `tests/admin-pipeline.test.ts`, `tests/portal-table-dialog.test.ts`, `prds/equity-funding-source-lending-portal-prd.md`, and `prds/investor-pipeline-admin.md`.

What changed: the reused lead detail no longer renders the separate `Deal Pipeline` card or lead CRM status dropdown. The top lead next-step alert now keeps the send/resend borrower invite button and adds `Open message conversation`, linking to `/admin/messages?role=borrower&q=<lead email>`. The Messages hub now reads `q=` and feeds it into the existing preferred-user flow, so the borrower thread is searched/opened and the reply box is focused.

Commits: EFS submodule commit `7669e49 fix(admin): simplify lead follow-up card`; parent gitlink commit `987c8bd01 chore(efs): bump lead follow-up submodule`. Both are local commits; I did not push or deploy because this queue item did not request deploy.

Verification: focused `bun test tests/admin-application-lead-detail.test.ts tests/admin-pipeline.test.ts tests/portal-table-dialog.test.ts tests/application-leads.test.ts` passed (`93 pass / 0 fail`). `git diff --check` passed. Full `make check` passed (`1193 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Escalation: none. Confidence: high.

### Re: Deploy Lead Follow-up Change — gpt-5

> **Queue prompt:**
> deploy

Pushed and deployed.

The EFS submodule commit `7669e49 fix(admin): simplify lead follow-up card` is now pushed to `origin/main`. The production Worker deploy completed successfully at `https://equityfundingsource.brian-takita.workers.dev` with Current Version ID `9eff0e78-a089-4554-97b6-fac79f5e784d`.

Deploy evidence: pre-deploy remote D1 migrations reported `No migrations to apply`; D1 info was reachable for DB `280e1539-b940-4c59-8988-fbed63c12ac0`; Time Travel bookmark before deploy was `0000050c-00000020-00005094-f3536d826ff65b1bc511b111f7e755d8`. `make deploy` built the Worker, applied remote migrations (again no pending migrations), uploaded one changed browser asset, and deployed the Worker.

Post-deploy checks: remote D1 migrations still report `No migrations to apply`; public root returned `HTTP 200`; the protected admin pipeline lead-dialog URL returned `HTTP 303` to `/login?role=admin&next=...`, which is expected without an admin session.

Changed paths this deploy cycle: none. The code change was already committed in the prior cycle; this cycle pushed EFS `main` and deployed it. The EFS submodule is clean and matches `origin/main`. The parent repo still has unrelated dirty work plus local agent-doc commits; I did not push parent `main`.

Verification carried forward from the code cycle: focused Bun tests passed (`93 pass / 0 fail`), `git diff --check` passed, and full `make check` passed (`1193 pass / 0 fail`, app build, Worker build). `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`.

Escalation: none. Confidence: high.
<!-- agent:boundary:9287d082 -->
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
- ~~do [#q1w3]~~
- 🚧 deploy
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
