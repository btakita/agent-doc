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

completed
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260622-173510.md`*

Compacted content:
❯ - Archived 1 response topic(s): admin in-app notification endpoint
❯ - Prior summary/context: prior compacted content: Archived 8 response topic(s): post-compaction state + the "queue all gated items" directive;...

### Re: hide wiring instructions until lender selected — gpt-5

> **Queue prompt:**
>
> Do not show "Wiring Instructions (Secure Link)" in a deal until a lender is selected.

Done. The admin deal Loan Detail tab now hides the deal/loan-specific `Wiring Instructions (Secure Link)` card until that deal has at least one selected lender. The standalone loan detail path uses the selected lender state from its interested-lender fetch, and the embedded deal tab passes the already-loaded selected-lender count through while keeping the Interested Lenders tab itself suppressed.

Platform Settings can still generate the platform-wide secure share link; this change only gates the deal/loan-specific card.

Changed paths:
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/loan_detail.tsx`
- `src/equityfundingsource-dev/tests/wire-instruction-links.test.ts`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/admin-pipeline.test.ts`
- `src/equityfundingsource-dev/prds/equity-funding-source-lending-portal-prd.md`
- `src/equityfundingsource-dev` parent submodule pointer

Commands:
- `bun test tests/wire-instruction-links.test.ts tests/admin-investor-pipeline.test.ts`
- `bun test tests/wire-instruction-links.test.ts tests/admin-investor-pipeline.test.ts tests/admin-pipeline.test.ts`
- `git diff --check`
- `make check`
- `gh run list --workflow CI --limit 1`
- `git push origin main` in `src/equityfundingsource-dev`
- `git push origin main` in the parent repo

Verification: focused suites passed. Full `make check` passed after rebasing on the latest EFS `origin/main`; the digest flags the intentionally logged `d1 down` from the passing non-fatal outbox test, while the command exit code is 0. GitHub still has no workflow named `CI`, so there was no CI run to inspect.

Commits:
- EFS submodule: `23338d7` (`fix(admin): gate wire links on lender selection`) pushed to `origin/main`.
- Parent repo: `37b2981a8` (`chore: update efs submodule for wire link gate`) committed locally. Parent push was rejected as non-fast-forward because parent `main` was already divergent (`ahead 50, behind 19`) with unrelated dirty work present; I did not merge/rebase that broader parent history.

Escalation: none.

Next queue head remains `deploy`; I did not deploy in this cycle.

Confidence: high.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> deploy

Deployed the current EFS `main` (`23338d7`) with `make deploy`. The deploy built the Worker, confirmed no remote D1 migrations were pending, uploaded the changed browser asset, and activated Worker version `0cbb834a-b221-435b-a5f5-ba980ec5a834` at `https://equityfundingsource.brian-takita.workers.dev`.

Changed paths: none in this deploy cycle.

Commands:
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` before deploy
- `make deploy`
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` after deploy
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`

Verification: pre-deploy and post-deploy remote D1 checks both reported `No migrations to apply`; the live Worker root returned HTTP `200` after deploy. The EFS working tree is clean and synced with `origin/main`.

Escalation: none.

Confidence: high.

### Re: wiring instructions still visible + deploy — gpt-5

> **Queue prompt:**
>
> I still see "Wiring Instructions (Secure Link)" in https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=d6b78994-0970-498f-9e10-6f0035902c0d&deal_tab=loan which is an Available Deal with no lender selected. Did you implement hiding Wiring Instructions until a lender is selected?

> **Queue prompt:**
>
> deploy

Yes, the hiding implementation is present in current EFS `main`: the embedded deal Loan Detail tab passes `show_wire_links={selected_interests.length > 0}`, and `selected_interests` is derived from non-null `investor_deal_interests.selected_at`.

The reason this live URL still shows `Wiring Instructions (Secure Link)` is that production D1 does not match the "no lender selected" premise. Deal `d6b78994-0970-498f-9e10-6f0035902c0d` currently has a selected interest row `cc15d2fa-a98d-43c2-8451-d389c77cde9c` for `btak.dev+investor@gmail.com` / Dude Lender, `lender_account='123'`, `proposal_amount=100000`, and `selected_at=1782012995` (`2026-06-20T23:36:35-04:00`). The deal row is also `status='reserved'`.

There is a separate status drift: the linked loan is back to `tmo_loan_status='Application Received'`, so the admin pipeline can still make the deal look like an Available Deal even though EFS has selected-lender data. I added a backlog item for that reconciliation instead of changing behavior in this cycle.

Deployed current clean EFS `main` (`23338d729ed094829bcaa540a9ed2e88393c444b`) through the documented `make deploy` path. The deploy applied no D1 migrations, uploaded no changed assets, and activated Worker version `469232ce-28b2-41fd-be76-85d1b2c97d65` at `https://equityfundingsource.brian-takita.workers.dev`.

Changed paths: none in `src/equityfundingsource-dev`.

Commands:
- `bun run --cwd app/equityfundingsource cf:d1:query:remote -- "SELECT ... FROM investor_deal_interests ..."`
- `bun run --cwd app/equityfundingsource cf:d1:query:remote -- "SELECT ... FROM investment_deals ... LEFT JOIN loans ..."`
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`
- `git rev-parse HEAD`
- `git ls-remote origin main`
- `make deploy`
- post-deploy `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote`
- post-deploy `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`
- post-deploy `git status --short --branch`

Verification: EFS submodule is clean and synced with `origin/main`; remote D1 has no pending migrations; Worker root returned HTTP `200` after deploy. I did not run the full test suite because there were no source changes; the deploy build succeeded.

Escalation: none.

Confidence: high.

### Re: available deal guidance + deploy — gpt-5

> **Queue prompt:**
>
> Why is https://equityfundingsource.brian-takita.workers.dev/admin/pipeline?dialog=deal&dialog_deal_id=d6b78994-0970-498f-9e10-6f0035902c0d&deal_tab=loan still an Available Deal? We need to have messaging in the dialog/page to instruct the admin on how to advance the deal. If it is due to a lender not having their accredidation approved by the admin, then provide a message with a button to approve the lender accredidation.

> **Queue prompt:**
>
> deploy

The live reason is data drift, not an unapproved lender. Production D1 shows deal `d6b78994-0970-498f-9e10-6f0035902c0d` is `reserved` with selected interest `cc15d2fa-a98d-43c2-8451-d389c77cde9c` for `btak.dev+investor@gmail.com` / Dude Lender, lender account `123`, and accreditation verification `732ad783-91fd-43eb-a694-2dc186eb14f0` already `verified`. The loan is still in the Available Deals lane because loan `FTM631` remains `tmo_loan_status='Application Received'`.

Implemented and deployed an admin guidance panel at the top of the Available Deal Loan Detail tab. When the loan is still `Application Received`, it now explains the next action:
- no selected lender: review Interested Lenders and select a funding proposal;
- under-review accreditation: show an `Approve accreditation` button wired to the existing admin accreditation decision endpoint;
- selected lender with old status drift: show `Move to Loan Estimate`, wired to the existing loan status endpoint.

I did not mutate the production loan status directly; the deployed UI now gives the admin the explicit action to advance it.

Changed paths:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/investor_management/index.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/shared.ts`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/prds/equity-funding-source-lending-portal-prd.md`
- `src/equityfundingsource-dev` parent submodule pointer

Commands:
- `bun run --cwd app/equityfundingsource cf:d1:query:remote -- "SELECT ... WHERE d.id = 'd6b78994-0970-498f-9e10-6f0035902c0d' ..."`
- `bun test tests/admin-investor-pipeline.test.ts`
- `git diff --check`
- `make check`
- `gh run list --workflow CI --limit 1`
- `git push origin main` in `src/equityfundingsource-dev`
- `git push origin main` in the parent repo
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` before and after deploy
- `make deploy`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`

Verification: focused admin-investor tests passed; full `make check` passed (1117 tests plus app build and worker build). The passing suite still logs the intentional `d1 down` non-fatal outbox test. GitHub has no workflow named `CI`, so there was no CI run to inspect. Remote D1 has no pending migrations. The deployed Worker root returned HTTP `200`.

Commits:
- EFS submodule: `4a1211c` (`fix(admin): explain available deal advancement`) pushed to `origin/main`.
- Parent repo: `f0539492f` (`chore: update efs submodule for deal guidance`) pushed to `origin/main`.

Deploy: activated Worker version `e7a2b801-1a89-44e4-857f-d0db5da38aa5` at `https://equityfundingsource.brian-takita.workers.dev`.

Escalation: none.

Confidence: high.

### Re: invite link + due diligence notes — gpt-5

> **Queue prompt:**
>
> rwaldman100+admin@gmail.com clicked the invite link but was not logged in and prompted to set password.

That is expected for an invite opened from an unauthenticated browser: the invite flow should let the invited admin set a password before entering the admin portal. The thing to verify is the post-password path: after setting the password, `rwaldman100+admin@gmail.com` should land in the admin portal with the intended admin role. If it loops back to password setup, loses the invite, or lands without admin access, that is the bug to fix.

> **Queue prompt:**
>
> Once a lender is selected, EFS sends a "Due Diligence Package" to the lender. The lender will request documents.

Captured as product workflow context. I added backlog coverage for the selected-lender Due Diligence Package flow: the selected lender should receive the package, then lender document requests should become visible workflow state in the deal dialog with status and audit history rather than ad hoc messages.

I also added a backlog item to verify the admin invite/password setup path for `rwaldman100+admin@gmail.com`.

Changed paths: none.

Commands:
- `tsift status`
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc repair /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc admin inspect /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc admin detect`
- `agent-doc admin recycle /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc finalize ... --force-disk`

Verification: preflight and planning passed. No app tests were run because no source files changed. The normal editor IPC path repeatedly failed with `recovery=retry_without_disk_write`; the final closeout used agent-doc's explicit `--force-disk` path because the response was otherwise captured but not visible or committed. Preflight still reports the exchange is large while the queue is active; I did not compact without explicit opt-in.

Escalation: none.

Confidence: medium; the product interpretation is straightforward, but closeout required the force-disk recovery path after repeated IPC no-ack failures.

### Re: admin invite password reset + duplicate sign-in links — gpt-5

> **Queue prompt:**
>
> rwaldman100+admin@gmail.com forgot password "did not work". Gave an account already existed error.

> **Queue prompt:**
>
> Forgot password email was not sent when rwaldman100+admin@gmail.com clicked "Forgot password" link.

> **Queue prompt:**
>
> Email me a sign in link flow sent 2 emails. It should only send 1 email. 1 of the 2 emails had an expired link. The first one worked.

> **Queue prompt:**
>
> [#admin-invite-password-flow]

> **Queue prompt:**
>
> deploy

Fixed and deployed both auth issues.

Root cause for forgot password: invite-provisioned admins can be active users with an empty `password_hash`. The forgot-password route only emailed reset links when `password_hash` already existed, and reset submission also rejected empty-hash users. That made the flow silently no-op for a first-password admin invite. The reset flow now allows any active user to receive a reset token and set the first password.

Root cause for duplicate sign-in-link emails: each POST to `Email me a sign-in link` minted a new token and superseded the previous unused token. A double submit could therefore send two emails while making one link immediately invalid. The route now suppresses immediate duplicate sends when a recent unused token already exists, keeping the first link valid and sending only one email.

Changed paths:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/auth/routes.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/auth/email_login.ts`
- `src/equityfundingsource-dev/tests/auth.test.ts`
- `src/equityfundingsource-dev/tests/email-login.test.ts`
- `src/equityfundingsource-dev` parent submodule pointer

Commands:
- `bun test tests/auth.test.ts`
- `bun test tests/email-login.test.ts tests/auth.test.ts`
- `git diff --check`
- `make check` after each auth fix
- `gh run list --workflow CI --limit 1`
- `git push origin main` in `src/equityfundingsource-dev`
- `git push origin main` in the parent repo
- `bun run --cwd app/equityfundingsource cf:d1:migrations:list:remote` before and after deploy
- `make deploy`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`

Verification: focused auth/email-login suites passed. Final full `make check` passed with 1119 tests plus app and worker builds; the suite still logs the intentional non-fatal `d1 down` outbox test. GitHub still has no workflow named `CI`, so there was no CI run to inspect. Remote D1 reported `No migrations to apply`, and the live Worker root returned HTTP `200` after deploy.

Commits:
- EFS submodule: `3423507` (`fix(auth): allow invite password reset`) pushed to `origin/main`.
- EFS submodule: `31487e0` (`fix(auth): suppress duplicate email login links`) pushed to `origin/main`.
- Parent repo: `c68d3c49b` (`chore: update efs submodule for auth reset fix`) pushed to `origin/main`.
- Parent repo: `f06cca831` (`chore: update efs submodule for email login fix`) pushed to `origin/main`.

Deploy: latest Worker version `b5683da6-1389-4aaf-8eeb-94eb82e7513c` is active at `https://equityfundingsource.brian-takita.workers.dev`.

Escalation: none.

Confidence: high.

### Re: closeout recovery — gpt-5

> **Queue prompt:**
>
> ~~Forgot password email was not sent when rwaldman100+admin@gmail.com clicked "Forgot password" link.~~ — auto-struck: answered this cycle (#ftstrike)


This cycle only reconciles the editor/bookkeeping drift after the prior closeout. No additional product or code work was performed after the admin invite password reset and duplicate email-login fixes already recorded above.

Changed paths: none in this recovery-only cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`.

Verification: confirmed the prior closeout response is already committed in `HEAD`; no new code verification was required for this recovery-only cycle.

Escalation: none.

Confidence: high.

### Re: forgot-password email queue head — gpt-5

This queue head is already covered by the auth reset fix recorded above: invite-provisioned active admins with an empty `password_hash` can now request a forgot-password email and set their first password through the reset link.

Changed paths: none in this acknowledgment cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`.

Verification: the previously recorded focused auth test and full `make check` remain the verification for this item; no new code verification was required for this queue-head acknowledgment.

Escalation: none.

Confidence: high.

### Re: forgot-password email queue head — gpt-5

This is already covered by the earlier auth reset fix: invite-provisioned active admins with an empty `password_hash` can request the forgot-password email and use the reset link to set their first password.

Changed paths: none in this acknowledgment cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `rg -n "Available Deal|dialog_deal_id=d6b78994|advance the deal|accredidation|accreditation|Remove Google|Google" tasks/professional/equityfundingsource.md src/equityfundingsource-dev`.

Verification: confirmed the Available Deal prompt had already been answered in the document and that this forgot-password email queue head is covered by the prior auth tests/full `make check`; no new code verification was required for this acknowledgment.

Escalation: none.

Confidence: high.

### Re: auto-struck queue items + due diligence package — gpt-5

> **Queue prompt:**
>
> Please explain #ftstrike. I don't see it anywhere in this document or in equityfundingsource.done.md


I reviewed the auto-struck queue items. The admin invite/reset items, duplicate sign-in-link item, Available Deal guidance, Google login removal, and deploy were all covered by code changes/deploys in this run or earlier recorded cycles. The unworked item was `#due-diligence-package`, so I completed that rather than leaving it struck/reaped.

Implemented `#due-diligence-package`: when an admin newly selects a lender for a deal, EFS now stamps the workflow package/processing timestamps, creates a lender-thread message titled `Due Diligence Package`, emails the selected lender through the existing lender deal message notification path, and tells the lender to reply in that thread with due-diligence document requests. The admin deal dialog now shows a selected-lender Due Diligence Package status panel explaining that borrower upload requests are issued and tracked through the existing Borrower communication/document-request flow.

Also completed the Robert/Google-login item: the signup page no longer renders the remaining `Continue with Google` button/divider or `/login/google` link. The login page removal was already covered; the signup surface was the lingering one.

Changed paths:
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/investor_management/index.ts`
- `src/equityfundingsource-dev/app/equityfundingsource/lib/admin/repo.ts`
- `src/equityfundingsource-dev/lib/ui--browser--efs/portal/admin/deal_detail.tsx`
- `src/equityfundingsource-dev/lib/ui--server--efs/auth/signup.ts`
- `src/equityfundingsource-dev/tests/admin-investor-pipeline.test.ts`
- `src/equityfundingsource-dev/tests/auth-login-ui.test.ts`

Verification:
- `bun test tests/auth-login-ui.test.ts` passed.
- `bun test tests/admin-investor-pipeline.test.ts` passed.
- `git -C src/equityfundingsource-dev diff --check` passed.
- `make check` passed after the Google change: 1120 tests plus app/worker builds.
- `make check` passed after the due-diligence change: 1121 tests plus app/worker builds. The known intentional `d1 down` log appears inside a passing non-fatal outbox test.
- `gh run list --workflow CI --limit 1` still reports no workflow named `CI` for the EFS repo.
- Deployed EFS Worker `d9435fc8-4aae-4e8d-a115-6a66e0a1ef90`; remote D1 had no migrations to apply.
- Live smoke: `/` returned HTTP 200; `/signup?role=admin` returned HTTP 200 and contains no `Continue with Google`, `/login/google`, or `google_url` strings.

Commits:
- EFS `dd6165b fix(auth): remove google signup button`
- EFS `ff0d565 feat(deals): send due diligence package on lender selection`
- parent `471d25a5f chore: update efs submodule for google login removal`
- parent `909a0d4b1 chore: update efs submodule for due diligence package`

Escalation: none.

Confidence: high.

### Re: #ftstrike — gpt-5

`#ftstrike` is an internal agent-doc queue marker, not an EFS backlog id and not a completed-work anchor. It abbreviates the free-text queue auto-strike path: agent-doc believed a free-text queue line had already been answered by the current response, so it struck the queue line and appended `— auto-struck: answered this cycle (#ftstrike)` as bookkeeping.

That is why you do not see `#ftstrike` in this document's backlog or in `equityfundingsource.done.md`: it is diagnostic metadata explaining why a queue line was struck, not a user-tracked task.

In this case, the marker exposed a real problem: the due-diligence item had been auto-struck/reaped before it was implemented. I corrected that by implementing and deploying `#due-diligence-package`, then marking that tracked item done.

Changed paths: none in this explanation-only cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`.

Verification: explanation-only; no code verification required beyond the due-diligence verification recorded in the previous response.

Escalation: none.

Confidence: high.

### Re: stale struck queue heads — gpt-5

The remaining queue heads are already answered/struck residue. No additional EFS product work is required for this cycle: the forgot-password email issue was covered by the auth reset fix, and the Due Diligence Package item has now been implemented, deployed, and reaped to done.

Changed paths: none in this cleanup-only cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`; `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`.

Verification: queue-state cleanup only; product verification is recorded in the prior implementation responses.

Escalation: none.

Confidence: high.

### Re: due-diligence queue residue — gpt-5

> **Queue prompt:**
>
> ~~Once a lender is selected, EFS sends a "Due Diligence Package" to the lender. The lender will request documents.~~ — auto-struck: answered this cycle (#ftstrike)


This remaining struck queue head is already handled. The Due Diligence Package behavior was implemented, verified, deployed, and `#due-diligence-package` was reaped to done in the prior cycle.

Changed paths: none in this cleanup-only cycle.

Commands: `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`.

Verification: queue-state cleanup only; product verification is recorded in the due-diligence implementation response.

Escalation: none.

Confidence: high.
<!-- agent:boundary:0145d544 -->
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
- ~~complete [#msg-unify3]~~
- ~~complete [#msg-unify4]~~
- deploy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
