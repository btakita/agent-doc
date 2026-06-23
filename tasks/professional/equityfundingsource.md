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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260623-041604.md`*

Compacted content:
- Archived 15 response topic(s): production deletion logs; TMO sync delete risk + soft-disable trail; application logging + deploy; 12 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: Does the TMO sandbox have any loan records? — gpt-5

> **Queue prompt:**
>
> Does the TMO sandbox have any loan records? Or were they cleaned out?

Short answer: the public/documented TMO `API Sandbox` is not cleaned out, but the EFS-specific/private TMO sandbox state is still not directly verifiable from the local `passage equityfundingsource/TMO_API_TOKEN` token.

What I verified:

- Public documented TMO sandbox: `GET https://api.themortgageoffice.com/LOS.svc/GetLoans` with `Token: TMO` and `Database: API Sandbox` returned `37` loan records. So that sandbox has loans.
- The EFS-specific local Passage token is present, but `GetLoans` returned `401 Not authenticated` for `Database: API Sandbox` and the likely alternate database header values I tried. There is no local `passage equityfundingsource/TMO_DATABASE` entry.
- The deployed Worker has `TMO_API_TOKEN` and `TMO_DATABASE` secret names configured, but Cloudflare does not let us read secret values back. So I cannot use those production secret values for a direct local live `GetLoans` count.
- Production EFS app-owned D1 tables are cleaned out for the main loan/application surfaces: `loans=0`, `investment_deals=0`, `full_applications=0`; `application_leads=6` still exist.
- Production EFS `tmo_cache_entities` still has cached TMO loan payloads: `11` non-deleted `loan` cache rows and `1` deleted cache row. That includes old cached rows such as `TST113` / Lucas Huang and the prior `FTM631` / `GQI751` cache rows.
- Those EFS cache rows are not proof that the live TMO sandbox still contains those loans today. The current sync code loops local EFS `loans` with a `tmo_loan_number`; because EFS currently has zero local loans, recent sync batches with `loans_count=0` do not mean live TMO returned zero loans.
- The public documented sandbox does not currently contain `TST113`, `FTM631`, `GQI751`, Lucas, Huang, or stormrunner in `GetLoans`; direct `GetLoan/TST113`, `GetLoan/FTM631`, and `GetLoan/GQI751` all returned `Loan not found`. `GetLoan/1010` does exist in the public sandbox, but it is a generic sample record, not the old EFS cached `1010` payload.

So the precise read is: public TMO sandbox has records; EFS app-owned production tables are cleaned out for loans/deals/full applications; EFS has stale/cached TMO-origin rows; current live EFS-specific TMO sandbox contents are still unknown until we have a valid token/database pair or a production-safe read-only diagnostic that runs inside the Worker with its existing secrets.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- No EFS source files, production D1 rows, TMO records, deploys, or migrations were changed.

### commands — gpt-5

- `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
- `tsift status` in the workspace and EFS submodule.
- Source reads for `tmo_live_loans_R_`, `tmo_loans_R_`, `tmo_sync_all_`, and the admin TMO-loans route.
- Read-only live TMO `GetLoans` / `GetLoan` checks against the public documented sandbox and the local Passage token.
- Read-only remote D1 `SELECT` queries against `tmo_sync_batches`, `tmo_cache_entities`, `loans`, `investment_deals`, `full_applications`, and `application_leads`.
- `wrangler secret list` to verify the deployed Worker secret names.
- `passage ls` to check available local TMO credential entries without printing secret values.

### verification — gpt-5

All D1 checks reported `changed_db=false` and `rows_written=0`. The public TMO sandbox returned 37 loans; the local EFS Passage token returned authentication failures; production D1 confirmed zero app loans/deals/full applications and existing cached TMO rows.

### confidence — gpt-5

High that the public documented TMO sandbox is not empty and that EFS app-owned production tables currently have zero loans/deals/full applications. Medium-high that EFS cache rows are stale historical TMO-origin data. Low for current live EFS-specific TMO sandbox contents because the available local private token/database did not authenticate and production secret values are not readable.

### escalation — gpt-5

None.

### Re: 0099 lender role/schema — gpt-5

> **Queue prompt:**
>
> Lucas found 0099_lender_role_and_schema as the likely culprit. Can you confirm? Why were you not able to find this out?

Yes, with a narrower reading: `0099_lender_role_and_schema.sql` is very likely the culprit if the symptom Lucas found was an investor-vs-lender role/table mismatch. It is not a plausible culprit for TMO sandbox loan deletion or EFS loan-table cleanup.

What `0099` does:

- Rebuilds `users` with `role IN ('lender','borrower','broker','admin')`.
- Converts existing `users.role = 'investor'` rows to `lender`.
- Renames the persisted investor schema tables to lender names: `investor_documents`, `investor_document_requests`, `investor_accreditation_verifications`, `investor_vestings`, `investor_special_instructions`, `investor_deal_signatures`, and `investor_deal_interests` become the corresponding `lender_*` tables.
- Recreates the renamed `lender_*` indexes.

Production confirms that migration boundary is already crossed: remote migration list says only `0102_full_application_soft_delete.sql` is pending, so `0099` has already applied. Remote `sqlite_master` contains `lender_deal_interests`, `lender_documents`, `lender_vestings`, and `users`; the checked old `investor_*` table names are absent. Remote `users` role counts are `admin=5`, `borrower=4`, `broker=2`, `lender=3`, with no `investor` role rows.

So if any live code, diagnostic query, or support/debug script still expects `users.role='investor'` or old `investor_*` table names, `0099` explains the failure. Current runtime source appears mostly aligned with the rename: the direct SQL DML search for old `investor_documents` / `investor_deal_interests`-style table references found only historical migrations, not active app/lib query code. The remaining `investor` names are mostly public route/module/API names like `/api/investor/...`, TypeScript names, UI ids, comments, and backward-compatible role normalization.

Why I did not find it before: I scoped the previous pass around the TMO question and app-owned loan/application data. I checked live TMO endpoints, `tmo_cache_entities`, `loans`, `investment_deals`, `full_applications`, and `application_leads`, but I did not audit the migration boundary or compare production `sqlite_master` against the deployed code schema. That was the miss. The repo also has a noisy naming transition: routes and functions still say `investor` in many places while the persisted schema now says `lender`, so a plain text search for `investor` does not isolate the schema mismatch cleanly. The right first check should have been migration list + table-name/role counts.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- No EFS source files, migrations, production D1 rows, TMO records, deploys, or secrets were changed.

### commands — gpt-5

- `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
- `tsift status` in the workspace and EFS submodule.
- `rg` / `sed` / `git show` reads for `0099_lender_role_and_schema.sql`, commit `071a93f`, and old/new investor/lender SQL references.
- `bun run cf:d1:migrations:list:remote`.
- Read-only remote D1 queries against `sqlite_master` and grouped `users.role` counts.

### verification — gpt-5

Remote D1 checks were read-only and reported `changed_db=false`, `rows_written=0`. Migration list shows `0099` is applied in production. Remote schema and role counts match the post-`0099` lender naming state.

### confidence — gpt-5

High that `0099` explains investor/lender role or table-name mismatch symptoms. High that it does not explain TMO sandbox loan deletion or EFS loan cleanup because it does not touch the loan/TMO tables or external TMO. Medium for Lucas's exact production symptom only because the prompt did not include the specific failing request, stack, or SQL error.

### escalation — gpt-5

Used `force_disk` only after two failed IPC finalizes plus an unchanged no-ACK patch file; no source/prod escalation.

### Re: destructive migration — gpt-5

> **Queue prompt:**
>
> Why did you build the migration to be destructive?

I built it that way because I treated `0099_lender_role_and_schema.sql` as a one-time terminology cutover from `investor` to `lender`, and I used the standard SQLite/D1 workaround for changing a `CHECK` constraint on `users.role`: create a replacement `users` table, copy rows with `investor` mapped to `lender`, drop the old `users`, then rename the replacement. For the other persisted investor entities, I used direct table renames from `investor_*` to `lender_*` and rebuilt indexes under the new names.

That explains the mechanics, but it does not make the migration good enough. The mistake was treating a semantic rename as if it were safe to do as a hard schema cutover in production. It removed the old table names and old role value as compatibility surfaces in one migration, and the `users` rebuild used a `DROP TABLE users` step. Even though the script copies user rows first and is not written as a deliberate data wipe, it is still destructive in the operational sense: rollback is hard, stale deployed code or support SQL using `investor_*` breaks immediately, and a production-shaped migration safety test should have existed before this landed.

The safer version should have been additive/compatible first: keep the old names working while introducing the lender terminology, prove deployed code and diagnostics no longer depend on `investor_*`, then remove compatibility only after a soak. At minimum, this migration needed a production-shaped fixture with users plus dependent rows, `foreign_key_check` before/after, row-count assertions for every renamed table, and a rollback or alias/compatibility plan. I did not do that; I relied on text-level tests that only asserted the migration contained the intended role/table rename statements. That was the wrong bar for a production D1 migration.

So the direct answer is: I built it destructive because I optimized for completing the naming cutover and SQLite constraint change, not for production migration safety. That was my miss. The immediate fix should be a follow-up migration/safety patch that either restores compatibility aliases where D1 supports them or updates every remaining diagnostic/support path to the new schema, plus tests that execute the migration against prod-shaped data instead of only checking strings.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- No EFS source files, migrations, production D1 rows, TMO records, deploys, or secrets were changed in this cycle.

### commands — gpt-5

- `agent-doc preflight`, `agent-doc plan`, `agent-doc read`.
- `tsift status` from the workspace root.
- Source/history reads for `src/equityfundingsource-dev/app/equityfundingsource/migrations/0099_lender_role_and_schema.sql`, commit `071a93f`, and the tests that reference the migration.
- `rg` checks for migration users-table rebuilds, old/new lender table references, and foreign-key-dependent schema.

### verification — gpt-5

No code was changed, so I did not run the EFS test suite. I verified from the checked-in SQL that `0099` copies `users`, drops `users`, renames the replacement, renames the `investor_*` tables to `lender_*`, and recreates indexes; I also verified the tests I inspected are string/shape checks rather than a production-shaped migration execution test.

### confidence — gpt-5

High that the migration is operationally destructive/compatibility-breaking even if not intended as a data wipe. High that the reason was the lender terminology cutover plus SQLite/D1 constraint-rebuild pattern. Medium on the exact production blast radius without Lucas's concrete failing query/request/stack in this prompt.

### escalation — gpt-5

Used `force_disk` only after two failed IPC finalizes plus the same no-ACK patch-file closeout state; no source/prod escalation.

### Re: migration safety guardrails — gpt-5

> **Queue prompt:**
>
> How do we prevent destructive migrations from ever happening again?

We prevent this class of mistake by making destructive migrations fail closed in CI and deployment, not by relying on memory or review discipline.

For EFS, the rule should be: production migrations are expand/contract by default. First add compatible schema, adapters, views, nullable columns, or dual-write paths. Then deploy code that can read both old and new shapes. Then backfill and verify. Only after a soak do we remove old tables, old role values, old columns, or compatibility views. A one-step semantic rename like `investor_*` to `lender_*` should not directly remove the old compatibility surface in the same migration.

The concrete guardrails I would put in place:

1. Static migration gate: CI scans every new migration for destructive patterns: `DROP TABLE`, `DROP COLUMN`, table rebuilds via `CREATE ... INSERT ... DROP ... RENAME`, `ALTER TABLE ... RENAME`, `PRAGMA foreign_keys = OFF`, broad `DELETE`, and index/table drops. The default result is failure unless the migration carries an explicit waiver block with data-preservation proof, rollback plan, deploy order, and owner approval.

2. Prod-shaped migration test: new migrations must execute against a fixture that has real dependency shape: users, loans, deals, documents, messages, lender rows, borrower rows, admin references, and foreign-key dependents. The test must assert `PRAGMA foreign_key_check` is clean, key row counts/checksums are preserved, and required old compatibility names still work during a transition window.

3. Rename policy: semantic renames are not hard cuts. For role names, accept old and new values at the boundary until all callers are updated. For table names, prefer compatibility views/adapters or a dual-read/dual-write bridge before dropping old names. If D1 cannot support a compatibility mechanism cleanly, the migration must be split and explicitly reviewed as a breaking change.

4. Remote migration checklist: before `wrangler d1 migrations apply --remote`, require a backup/export or known restore point, preflight row counts, staging/preview D1 application from a prod-shaped snapshot, rollback script, and a code/migration ordering note. If any item is missing, do not apply remotely.

5. Review rule: migrations that disable foreign keys or rebuild a referenced table require a second-pass review focused only on data preservation and compatibility. That review should ignore product intent and ask one question: "What breaks or disappears if old code, support SQL, or a stale worker runs against the post-migration DB?"

I added a backlog item to implement this as an actual EFS guardrail: migration linter + prod-shaped migration execution tests + deployment checklist. That is the right next step because a written policy alone would not have stopped `0099`.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- Added one actionable EFS backlog item for destructive-migration guardrails through agent-doc pending ops.
- No EFS source files, migrations, production D1 rows, TMO records, deploys, or secrets were changed in this cycle.

### commands — gpt-5

- `agent-doc preflight` via CLI after the MCP wrapper returned a generic failure.
- `agent-doc plan`, `agent-doc read`.
- `agent-doc finalize`; first retry detected the backlog item had already been applied, so the final retry omitted the duplicate `pending_add`.

### verification — gpt-5

No code was changed, so I did not run the EFS test suite. The prevention answer is based on the already-inspected `0099` failure mode: hard role/table rename, `users` rebuild/drop, no compatibility window, and string-level tests instead of production-shaped migration execution tests.

### confidence — gpt-5

High that these guardrails would have blocked or forced explicit review of the `0099` pattern before production. Medium that this exact checklist is complete for all future D1 risks; it should evolve as migrations expose new failure modes.

### escalation — gpt-5

Used `force_disk` only after repeated IPC finalizes failed with the same no-ACK captured-response state; no source/prod escalation.
<!-- agent:boundary:aee61adb -->
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
- ~~How do we prevent destructive migrations from ever happening again?~~
- [#vjcv]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send; no active unused Robert login tokens remain.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
