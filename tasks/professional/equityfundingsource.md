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
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share
  '#commit-push-deploy': commit all uncommitted files + push in claudescore-plugin, claudescore.com, and session-share. deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
EFS workers.dev production: `/apply` public prequalification + D1-backed `application_leads` admin review is deployed in Worker version `2819a1cf-3091-495f-b9ab-72434d5cafdd` as of 2026-05-28; public homepage rate-estimator dollar prefixes now use `-translate-y-1/4` from EFS commit `2e18870`, and investor commitment/funding amount fields use shared grid-aligned dollar-prefix styling from EFS commit `a9b0405`. Local `bun dev` on port 4200 mounts `/apply`; public site/footer email is `info@equityfundingsource.com`; logged-out navigation exposes Borrower Login, Apply for a Loan, Broker Portal, and Investor Portal with Broker Portal placed between Apply and Investor. Cloudflare Access protects /login/access-callback with ACCESS_TEAM_DOMAIN/AUD. `/portal-admin/users` lets existing admins create admin/borrower/broker/investor accounts without reopening public `/admin/signup`; production D1 has active admin rows for `brian.takita+admin@gmail.com` and `btak.dev+admin@gmail.com`. Application lead notifications still route through Resend testing-mode config `APPLICATION_LEAD_NOTIFICATION_TO=brian.takita@gmail.com` and `APPLICATION_LEAD_NOTIFICATION_FROM=Equity Funding Source <onboarding@resend.dev>` until an EFS-owned sending domain is verified. Custom-domain cutover is deferred; continue using https://equityfundingsource.brian-takita.workers.dev until further notice.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: transfer full exchange — gpt-5

Transferred this document's exchange content into `tasks/professional/equityfundingsource-demo.md` using `agent-doc transfer ... exchange --bypass-claim`.

The transfer created commit `ea44de165` for the demo document. The binary also transferred the source backlog and icebox along with the exchange, so the main EFS document now keeps the current status/review surface while the demo document owns the transferred exchange/backlog/icebox context.

Closeout evidence: changed documents are `tasks/professional/equityfundingsource-demo.md`, `tasks/professional/equityfundingsource.md`, and the source done archive `tasks/professional/equityfundingsource.done.md`. Commands used included `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight/read/plan/template-info`, `git diff/status/log`, `agent-doc transfer`, and this `agent-doc finalize`. Verification is document/source review only; no code changed and no tests were run. Confidence is high that the full source exchange was moved into the demo document because `agent-doc template-info` shows the transfer block in the target exchange and an empty source exchange before this closeout. No escalation was used.
Are there ways for an existing admin to create new admins in the admin dashboard? If not, please create an account management section in the admin portal.
Please add brian.takita+admin@gmail.com and btak.dev+admin@gmail.com as admins to https://equityfundingsource.brian-takita.workers.dev/
#spec-test-commit-push-deploy

### Re: admin account management — gpt-5

Yes: existing admins can already create new admins in the admin dashboard at `/portal-admin/users`. The section is labeled User Management, is behind an authenticated admin session, and the create-user modal includes the `admin` role, so I did not add a duplicate account-management section.

I added both requested production admin users directly to the remote D1 database for `https://equityfundingsource.brian-takita.workers.dev/`:

- `brian.takita+admin@gmail.com` — `role=admin`, `status=active`
- `btak.dev+admin@gmail.com` — `role=admin`, `status=active`

They were inserted with empty personal password hashes, so admin login can use the configured global admin credential, and Cloudflare Access identity will link on first successful Access login.

Changed paths in `src/equityfundingsource-dev`:

- `prds/equity-funding-source-lending-portal-prd.md` — documents post-bootstrap admin provisioning through `/portal-admin/users`
- `docs/current_progress.md` — records that existing admins can create admin/staff/user accounts there
- `tests/admin-dashboard.test.ts` — pins the admin-only user-management route, role selector including `admin`, password requirement, and self-lockout guards
- `tests/borrower-messages.test.ts` — rebased stale assertion from the latest remote so the full suite stays green

Repo/deploy closeout:

- Committed and pushed EFS source repo commit `c57821c858f8122474c9d666b4e869c8601880d5` to `origin/main`.
- Deployed Worker version `00778168-53b8-4aca-9daa-b54db9f980b9` to `https://equityfundingsource.brian-takita.workers.dev`.
- Verified the remote D1 query returns both requested emails as active admins.

Verification:

- `bun test tests/admin-dashboard.test.ts` passed.
- `make check` passed after rebase: Bun reported `195 pass, 0 fail`, then build and worker build completed. The tsift digest still flags the expected logged `limiter down` string from upload-guard coverage, but the command exited 0 and Bun reported no failing tests.
- Live smoke checks after deploy: `/admin/login` returned 200, `/admin/signup` returned 404, and unauthenticated `/api/admin/users` returned 401.
- EFS repo GitHub Actions check: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` had no matching workflow to inspect.

Confidence: high. Escalation: none.
What is the password for the brian.takita+admin@gmail.com and btak.dev+admin@gmail.com admin accounts?

### Re: admin account passwords — gpt-5

There is no unique per-account password for `brian.takita+admin@gmail.com` or `btak.dev+admin@gmail.com` right now. I created both production rows with empty personal password hashes.

For `/admin/login`, that means these accounts can authenticate only with the configured shared `ADMIN_GLOBAL_PASSWORD` while their personal password hash is empty. I am not going to print that secret into this committed session document.

To give either account its own password, sign in as an existing admin and update the user through `/portal-admin/users`, or rotate/set a temporary password through the production admin-password path and share it out-of-band.

Closeout evidence: no repo files changed in this cycle. Commands run: `tsift status`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, and this `agent-doc finalize`. Verification is source/session knowledge from the prior D1 insert and auth behavior; no tests were needed. Confidence is high. Escalation: none.
Is the global password the same as my machine?

### Re: global admin password — gpt-5

No, it is not automatically the same as your machine password. `ADMIN_GLOBAL_PASSWORD` is a Cloudflare Worker secret used by `/admin/login`; it is independent of your local OS or machine login password unless you intentionally set the secret to that same value.

I would not recommend reusing a machine password for this. If you do not know the current value, check the private secret store or rotate the Worker secret and then share the new temporary credential out-of-band, not in this committed markdown session.

Closeout evidence: no repo files changed in this cycle. Commands run: `tsift status`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, and this `agent-doc finalize`. Verification is based on the deployed auth configuration: `ADMIN_GLOBAL_PASSWORD` is a Worker secret, not a machine-login integration. Confidence is high. Escalation: none.

### Re: store admin global password — gpt-5

Done. I stored the production admin global password in passage at:

`btak/equityfundingsource/admin_global_password`

I did not print the secret value into the terminal or this committed session document.

Verification:

- `passage find admin_global_password` shows the new entry.
- The passage value matches the local `.dev.vars` `ADMIN_GLOBAL_PASSWORD` source used by the project secret-push flow.
- Cloudflare lists `ADMIN_GLOBAL_PASSWORD` as a production Worker secret.
- A live `/admin/login` POST for `brian.takita+admin@gmail.com` using that value returned `303` to `/portal-admin`, confirming the stored value works against production.

Closeout evidence: no EFS source repo files changed; `src/equityfundingsource-dev` stayed clean. Commands run included `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `passage find`, `passage insert`, `bun run cf:secrets:list`, a live login verification script, and this `agent-doc finalize`. Confidence is high. Escalation: none.
❯ In /portal-admin/dashboard, please ensure the numbers are data driven. Please provide links to the admin pages for the following to see the detail data:
❯ - Active users
❯ - Open applications
❯ - New application leads
❯ - Pending lender requests
❯ - Pending funding commits
❯ - Uploads (last 7 days)
❯ - Open document requests

❯ #spec-test-commit-push

### Re: admin dashboard metric drilldowns — gpt-5

Done. `/portal-admin/dashboard` now keeps the dashboard numbers D1-backed and makes the requested metrics clickable into filtered admin detail pages:

- Active users -> `/portal-admin/users?status=active`
- Open applications -> `/portal-admin/loans?application_status=Application%20Received`
- New application leads -> `/portal-admin/application-leads?status=new`
- Pending len4der requests -> `/portal-admin/lenders?status=pending`
- Pending funding commits -> `/portal-admin/funding?status=pending`
- Uploads, last 7 days -> `/portal-admin/documents?kind=uploads&window=7d`
- Open document requests -> `/portal-admin/documents?kind=requests&status=open`

I added two admin detail pages that were missing for exact aggregate drilldowns: `/portal-admin/funding` for global investor funding commitments and `/portal-admin/documents` for global borrower uploads plus investor document requests. Existing admin pages now honor the query-string filters from the dashboard links.

Changed paths in `src/equityfundingsource-dev` include the admin dashboard UI, admin portal routing/layout, admin API/repo helpers, new `documents.tsx` and `funding_commits.tsx` admin pages, query-filter support in users/loans/leads/lenders, PRD/current-progress docs, and `tests/admin-dashboard.test.ts`.

Commit/push:

- EFS repo commit `928ecdc` (`feat: link admin dashboard metrics`) pushed to `origin/main`.

Verification:

- `bun test tests/admin-dashboard.test.ts` passed: 17 pass, 0 fail.
- `make check` passed: Bun reported 198 pass, 0 fail; app build and worker build completed.
- GitHub Actions inspection: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` both reported no matching workflow.

Confidence: high. Escalation: none.
On development, the localhost:4200/admin/login goes to localhost:4200/admin/login with an error "Database not configured". #spec-test-commit-push
### Re: local admin login preview — gpt-5

Fixed. In the lightweight `bun dev` server on port 4200, `/admin/login` now behaves like the regular login pages:

- GET `/admin/login` renders the admin form with `action="http://127.0.0.1:8787/admin/login"` when `auth_action_origin` is configured, so local preview submissions go to the Worker dev server with D1 bindings instead of posting back to port 4200.
- A direct POST to `localhost:4200/admin/login` now re-renders the admin login page with the local-preview setup message instead of returning the raw `Database is not configured.` 500.
- Admin signup direct POSTs get the same friendly local-preview fallback when bootstrap is enabled but D1 is unavailable.
- `admin_signup_enabled_` now tolerates a missing local `c.env`, which prevented a pure local Hono preview from 500ing on admin login rendering.

I also saw the earlier exchange text edit from `lender` to `len4der`; that looks like a prior-response typo edit, so I did not treat it as a product-code requirement.

Changed paths in `src/equityfundingsource-dev`:

- `app/equityfundingsource/lib/auth/routes.ts`
- `lib/ui--server--efs/auth/admin_login.ts`
- `app/equityfundingsource/dev-routes.test.ts`
- `prds/equity-funding-source-lending-portal-prd.md`
- `docs/deploy.md`
- `docs/current_progress.md`

Commit/push:

- EFS repo commit `ee616e6` (`fix: route admin login to worker dev auth`) pushed to `origin/main`.

Verification:

- `bun test app/equityfundingsource/dev-routes.test.ts` passed: 11 pass, 0 fail.
- `make check` passed: Bun reported 201 pass, 0 fail; app build and worker build completed.
- GitHub Actions inspection: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` both reported no matching workflow.

Confidence: high. Escalation: none.
❯ - http://127.0.0.1:8787/portal-admin/application-leads shows a 500 in red.
❯ - http://127.0.0.1:8787/portal-admin/messages shows "Could not load users" in red
❯ - http://127.0.0.1:8787/portal-admin/lenders show "Failed to load lender requests" in red

### Re: local admin detail pages — gpt-5

Fixed. The immediate localhost 500s were caused by the local D1 sandbox being behind the current migrations; `wrangler d1 migrations list --local` showed 0027 through 0032 still pending. I applied the local migrations, and the three reported authenticated API calls now return 200:

- `/api/admin/application-leads?status=new` -> `{"leads":[]}`
- `/api/admin/messages?message_limit=1&limit=10` -> `{"sessions":[],"session":null,"messages":[],"has_more":false}`
- `/api/admin/lender-requests?status=pending` -> `{"requests":[]}`

I also changed the dev worker startup so this does not recur after future pulls: `bun run dev:worker` applies pending local D1 migrations before building/starting Wrangler, and `dev:worker:once` now does the same.

Changed paths in `src/equityfundingsource-dev`:

- `app/equityfundingsource/bin/dev-worker.ts`
- `app/equityfundingsource/dev-routes.test.ts`
- `app/equityfundingsource/package.json`
- `docs/current_progress.md`
- `docs/deploy.md`
- `prds/equity-funding-source-lending-portal-prd.md`

Commit/push:

- EFS repo commit `6a06b44` (`fix: migrate local d1 before worker dev`) pushed to `origin/main`.

Verification:

- `bun run cf:d1:migrate:local` applied the pending local migrations.
- `wrangler d1 migrations list equityfundingsource --local` now reports no migrations to apply.
- Authenticated local smoke checks for the three reported APIs returned 200.
- `bun test app/equityfundingsource/dev-routes.test.ts` passed: 12 pass, 0 fail.
- `make check` passed: Bun reported 202 pass, 0 fail, then app build and worker build completed.
- GitHub Actions inspection: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` both reported no matching workflow.

Confidence: high. Escalation: none.
### Re: local admin detail pages closeout repair — gpt-5

Closeout repair: the local admin detail page fix is recorded in the response immediately above. No additional repo changes were made in this repair write.
deploy
### Re: deploy — gpt-5

Deployed the current EFS Worker from `src/equityfundingsource-dev` commit `6a06b44` to workers.dev.

Deploy result:

- Worker URL: `https://equityfundingsource.brian-takita.workers.dev`
- Current Version ID: `9a289db0-4793-4004-a234-757aa27acdfe`
- Trigger deployed: scheduled cron `9 19 * * *`

Changed paths: none in the EFS source repo for this deploy cycle; the repo was clean before and after deployment. The session status was updated with the new Worker version.

Commands run:

- `tsift status`, `tsift status --fix`
- `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`
- `git status --short`, `git log -1 --oneline`
- `bun run deploy:worker`
- live smoke checks with `curl`
- this `agent-doc finalize`

Verification:

- Deploy command completed successfully after building the app and worker bundle.
- Live `/` returned 200.
- Live `/admin/login` returned 200.
- Live `/apply` returned 200.
- EFS source repo remained clean after deploy.

Confidence: high. Escalation: none.
What are #next-steps to complete /portal-broker/pre-qual-tool?
No document changes were detected in `/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`, so I stopped after `agent-doc preflight` as required.

Preflight did auto-commit a queued dispatch snapshot first, then reported `no_changes: true`. It also warned that the exchange is getting large and suggested compacting only with explicit approval.
What are #next-steps to complete /portal-broker/pre-qual-tool?
No new document changes were detected in `/home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`, so I stopped after preflight.

Preflight did auto-commit a queued dispatch snapshot first, then returned `no_changes: true`. The document is also large enough that `agent-doc` recommends compacting only with explicit approval.
Add items to the top of the backlog.
### Re: Add items to the top of the backlog. — gpt-5

Added the broker pre-qual completion work to the top of the backlog in priority order.

Top backlog item: `#bpq-api`.

The current `/portal-broker/pre-qual-tool` page is static, so the first item is the backend calculation/API contract; UI wiring, submission handoff, and test/docs coverage follow it. No product code changed in this cycle.

Closeout evidence: changed path is `tasks/professional/equityfundingsource.md` only. Commands run: `tsift status`, `tsift status --fix`, `agent-doc --version`, `agent-doc preflight`, `agent-doc plan`, `git status --short`, `git diff`, and this `agent-doc finalize`. Verification is the binary-owned finalize/session-check plus document review; no tests were run because this cycle only mutates the session backlog. Confidence is high. Escalation: none.
### Re: broker pre-qual API — gpt-5

Done. `#bpq-api` is implemented and pushed in `src/equityfundingsource-dev` commit `05e3aaf` (`feat: add broker prequal api`).

What changed:

- Added a pure broker pre-qual calculator in `app/equityfundingsource/lib/broker/prequal.ts`.
- Added broker-authenticated `POST /api/broker/pre-qual`.
- The endpoint accepts `property_value`, `loan_amount`, `property_type`, and `loan_purpose`, with aliases for the existing static UI field names.
- The response returns calculated LTV, indicative rate, eligibility status, commission estimate, reasons, and the active April 2026 Bridge Lending rate-sheet reference.
- Added browser API types/helper for the later UI wiring item, without changing the static `/portal-broker/pre-qual-tool` UI yet.
- Updated PRD, formula docs, current progress, and broker portal tests.

Rules now captured in code/spec:

- 65% max LTV guideline.
- $100,000 minimum loan guideline.
- `eligible`, `review_required`, and `outside_guidelines` statuses.
- Construction/commercial/land scenarios require underwriter review.
- 1% broker commission estimate.
- Example scenario `$750,000` value / `$450,000` request returns `60%` LTV, `10.25%` indicative rate, and `$4,500` commission.

Verification:

- `bun test tests/broker-portal.test.ts` passed: 23 pass, 0 fail.
- `make check` passed: Bun reported 206 pass, 0 fail; app build and worker build completed. The suite still prints the expected upload limiter and local missing-D1 log lines while passing.
- `gh workflow list` returned no workflows.
- `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
- `gh run list --workflow tmux-test --limit 1` reported no workflow named `tmux-test`.

Closeout evidence: changed EFS paths are `app/equityfundingsource/lib/broker/prequal.ts`, `app/equityfundingsource/lib/broker/routes.ts`, `lib/ui--browser--efs/portal/broker/api.ts`, `tests/broker-portal.test.ts`, `prds/equity-funding-source-lending-portal-prd.md`, `docs/formula.md`, and `docs/current_progress.md`. EFS repo is clean after push. Confidence is high. Escalation: none.
### Re: broker pre-qual UI — gpt-5

Done. `#bpq-ui` is implemented and pushed in `src/equityfundingsource-dev` commit `14b900f` (`feat: wire broker prequal UI`).

What changed:

- Replaced the static read-only `/portal-broker/pre-qual-tool` fields with editable scenario inputs.
- The page now auto-runs the default scenario against `POST /api/broker/pre-qual` and reruns on the broker's form submit.
- Added loading, error, retry, reset, and result states using the existing broker portal UI patterns.
- Result rows now render from the API payload: LTV, indicative rate, eligibility status, commission estimate, rate sheet, loan request, and reasons.
- Kept the follow-on submit-deal handoff out of this UI-only item; that remains separate queued work.
- Updated PRD/current-progress docs and broker portal tests for the live UI contract.

Changed EFS paths:

- `lib/ui--browser--efs/portal/broker/pre_qual_tool.tsx`
- `tests/broker-portal.test.ts`
- `prds/equity-funding-source-lending-portal-prd.md`
- `docs/current_progress.md`

Verification:

- `bun test tests/broker-portal.test.ts` passed: 24 pass, 0 fail.
- `make check` passed: Bun reported 207 pass, 0 fail; app build and worker build completed. The suite still prints the expected upload limiter and local missing-D1 log lines while passing.
- `gh workflow list` returned no workflows.
- `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
- `gh run list --workflow tmux-test --limit 1` reported no workflow named `tmux-test`.

Closeout evidence: EFS repo is clean after push; changed files were staged exactly before commit. Confidence is high. Escalation: none.
### Re: broker pre-qual handoff — gpt-5

Done. `#bpq-prefill` is implemented and pushed in `src/equityfundingsource-dev` commit `ffdcd33` (`feat: prefill broker deal from prequal`).

What changed:

- Added a `Continue to submission` action on `/portal-broker/pre-qual-tool` for in-guidelines pre-qual results.
- The handoff URL carries the live estimate payload into `/portal-broker/submit-a-deal` with `source=prequal` query params.
- Submit Deal now initializes from those query params, pre-filling loan amount, LTV, combined property/purpose deal type, and underwriter notes with rate/status/commission/rate-sheet context.
- Outside-guideline scenarios still link to Submit Deal without prefill params.
- Updated PRD/current-progress docs and broker portal tests for the handoff contract.

Changed EFS paths:

- `lib/ui--browser--efs/portal/broker/pre_qual_tool.tsx`
- `lib/ui--browser--efs/portal/broker/submit_deal.tsx`
- `tests/broker-portal.test.ts`
- `prds/equity-funding-source-lending-portal-prd.md`
- `docs/current_progress.md`

Verification:

- `bun test tests/broker-portal.test.ts` passed: 25 pass, 0 fail.
- `make check` passed: Bun reported 208 pass, 0 fail; app build and worker build completed. The suite still prints the expected upload limiter and local missing-D1 log lines while passing.
- `gh workflow list` returned no workflows.
- `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
- `gh run list --workflow tmux-test --limit 1` reported no workflow named `tmux-test`.

Closeout evidence: EFS repo is clean after push; changed files were staged exactly before commit. Confidence is high. Escalation: none.

### Re: broker pre-qual coverage — gpt-5

Done. `#bpq-tests` is implemented and pushed in `src/equityfundingsource-dev` commit `bcd9db5` (`test: cover broker prequal workflow`).

What changed:

- Added explicit broker pre-qual coverage for authenticated endpoint behavior: the `POST /api/broker/pre-qual` route must call `require_broker_` before parsing or calculating.
- Added docs/progress drift coverage for the pre-qual rules, outside-guidelines status, UI API wiring, and submit-deal handoff documentation.
- The cumulative broker pre-qual coverage now includes rule boundaries, validation errors, auth gate behavior, UI API wiring, handoff wiring, and PRD/current-progress/formula documentation.

Changed EFS path:

- `tests/broker-portal.test.ts`

Verification:

- `bun test tests/broker-portal.test.ts` passed: 27 pass, 0 fail.
- `make check` passed: Bun reported 210 pass, 0 fail; app build and worker build completed. The suite still prints the expected upload limiter and local missing-D1 log lines while passing.
- `gh workflow list` returned no workflows.
- `gh run list --workflow CI --limit 1` reported no workflow named `CI`.
- `gh run list --workflow tmux-test --limit 1` reported no workflow named `tmux-test`.

Closeout evidence: EFS repo is clean after push; changed file was staged exactly before commit. Confidence is high. Escalation: none.
### Re: investor currency prefix alignment — gpt-5

Fixed, committed, pushed, and deployed. The shared investor `CurrencyAmountInput` now uses a two-column grid with a fixed `$` prefix track, a 12px column gap, and matching 32px text boxes/line heights for the prefix and amount. That keeps the dollar sign vertically centered on the same row as the amount and adds visible spacing between them in both investor places that reuse the component.

Changed EFS paths:

- `lib/ui--browser--efs/portal/investor/available_deals.tsx`
- `tests/investor-portal-e2e.test.ts`

Commit / push / deploy:

- EFS repo commit: `a9b0405 fix: align investor currency prefix`
- Pushed `a9b0405` to `origin/main`
- Deployed Worker version `b937e33b-8992-4801-bca6-b1897569fe77` at `https://equityfundingsource.brian-takita.workers.dev`

Verification:

- `bun test tests/investor-portal-e2e.test.ts` passed: 9 pass, 0 fail.
- `git diff --check` passed.
- `make check` passed: 211 pass, 0 fail, then app build and worker build completed.
- Headless Chromium screenshot of the isolated control showed the `$` and `250,000` on the same row with spacing.
- Live smoke checks: `/` returned 200, `/portal-investor` returned 303 to `/login?role=investor`, and the deployed `/index.browser-4VKUCFZ6.js` asset contains the new `grid-template-columns:max-content minmax(0,1fr)` currency field styling.
- GitHub Actions inspection: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` both reported no matching workflow.
- Tmux CI inspection: `make -n tmux-ci` has no `tmux-ci` target in the EFS repo.

Escalation: none. Confidence: high.
❯ For the $ span, use `-translate-y-1/4` instead of `-translate-y-1/2`
### Re: rate estimator dollar prefix translate — gpt-5

Done. I changed the two public homepage rate-estimator dollar-prefix spans from `-translate-y-1/2` to `-translate-y-1/4`, and added the matching critical CSS utility for `translateY(-25%)` so the class works during first paint.

Changed EFS paths:

- `lib/ui--server--efs/home/index.ts`
- `lib/ui--server--efs/styles/index.ts`
- `tests/public-header.test.ts`

Commit / push / deploy:

- EFS repo commit: `2e18870 fix: adjust estimator currency prefix`
- Pushed `2e18870` to `origin/main`
- Deployed Worker version `2819a1cf-3091-495f-b9ab-72434d5cafdd` at `https://equityfundingsource.brian-takita.workers.dev`

Verification:

- `bun test tests/public-header.test.ts` passed: 7 pass, 0 fail.
- `git diff --check` passed.
- `make check` passed: 212 pass, 0 fail, then app build and worker build completed.
- Live `/` returned 200.
- Live homepage HTML contains both `-translate-y-1/4` dollar-prefix spans and links `/index.server-EGLIFSVJ_TTQJXFP6.css`.
- The deployed CSS asset contains the `translateY(-25%)` utility for the new class.
- GitHub Actions inspection: `gh workflow list` returned no workflows; `gh run list --workflow CI --limit 1` and `gh run list --workflow tmux-test --limit 1` both reported no matching workflow.
- Tmux CI inspection: `make -n tmux-ci` has no `tmux-ci` target in the EFS repo.

Escalation: none. Confidence: high.
In the input, use `pl-10` instead of `pl-7`.
#spec-test-commit-push
<!-- agent:boundary:b86ee83d -->
<!-- /agent:exchange -->
###
<!--
Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrow sends a message? Can we support an email respons back into the chat? Can the borrower receive emails whenever an admin responds with a message?
-->

<!-- agent:queue auto -->
---
In the input, use `pl-10` instead of `pl-7`.
#spec-test-commit-push
---
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xt4d] Inventory TMO usage once access is restored: screenshots, exports, required fields, statuses, document flows, notifications, reports, integrations, and workflows EFS actually uses.
- [ ] [#0e1p] Draft phased TMO replacement roadmap after the demo: EFS-first MVP, TMO sync/cutover plan, document intelligence/search, borrower/broker/investor/admin portals, and tenant-ready resale packaging.
- [ ] [#f4rq] Verify an EFS-owned sending domain in Resend, then switch APPLICATION_LEAD_NOTIFICATION_FROM and APPLICATION_LEAD_NOTIFICATION_TO from testing-mode values to EFS production addresses.
- [ ] [#k2xg] Send Robert the six-question TMO outbound webhook prompt in src/equityfundingsource-dev/docs/tmo-webhooks-question.md for him to forward to Mortgage Office, then record the response and decide push vs. tightened polling.
- [ ] [#qr72] [recommended] By May 27, 2026, send Lucas a written bridge plan: $500 now from personal funds if available, ask for the remaining $500 to be paid after EFS payment clears or by an agreed near-term date, and invoice EFS immediately for $5,000 with a smaller immediate continuation retainer accepted if Judith pushes back.
- [ ] [#s62t] [recommended] Prepare EFS invoice with truthful hours/value; Lucas cost is $4,150 (83h @ $50), so floor is 42h/$4,200 and recommended ask is 50-60h/$5,000-$6,000 with any concession as an explicit courtesy credit.
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
<!-- /agent:icebox -->

<!-- agent:done archive=tasks/professional/equityfundingsource.done.md -->

<!-- completed work archived in tasks/professional/equityfundingsource.done.md -->
<!-- /agent:done -->
