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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260622-234611.md`*

Compacted content:
❯ - Archived 4 response topic(s): Available Deals missing; audit rows + due-diligence packet; were the two Available Deals deleted?; 1 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b...

### Re: production deletion logs — gpt-5

> **Queue prompt:**
>
> Look in the production logs. Are there any deletion events that occurred in the production logs?

Yes, but only one clear domain-level deletion event showed up, and it is older than the two missing Available Deals: production `tmo_cache_change_logs` has one `operation='deleted'` row for `entity_type='loan'`, `source_id='TST042'`, `mode='efs'`, created at `2026-06-08 15:08:43 UTC`.

For the two missing Available Deals we have been tracing (`FTM631` and `GQI751`), I did not find a production log record proving that either one was deleted. The stronger evidence is still that their EFS-local `loans` rows are gone while TMO cache still has active `Application Received` loan records for them.

What I found in production logs/audit sources:

- App DB fallback logs: `loan_activity_logs` has no rows matching delete text/kinds. The newer durable `audit_logs` table is not present in production yet (`no such table: audit_logs`), so it could not have preserved the missing-row deletion evidence.
- TMO cache change log: exactly one delete event exists in the current cache log set: `TST042` at `2026-06-08 15:08:43 UTC`.
- Cloudflare account audit logs: in the `2026-06-20T00:00:00Z` to `2026-06-22T23:30:00Z` window, `raw_method=DELETE` returned one Cloudflare control-plane event: `Delete script secret` for the Worker, not a D1/app row deletion.
- Cloudflare D1 audit logs: cursor pagination over `resource_product=d1` scanned 350 D1 API query audit rows from `2026-06-20T02:31:27.650Z` through `2026-06-22T23:06:24.279Z`. Thirteen were write-bearing (`changed_db`/`changes`/`rows_written`), but Cloudflare audit payloads do not include the SQL body, so those cannot be classified as delete vs insert/update from audit logs alone. These are Cloudflare API/D1 query audit records, not necessarily Worker-internal D1 calls.
- Worker logs: live `wrangler tail` works and captured a read-only `GET /admin/pipeline` request, but historical persisted Worker log queries through Workers Observability returned zero events for the checked windows. Cloudflare docs distinguish real-time logs from stored Workers Logs: real-time logs do not store history, while Query Builder searches stored Workers Logs. Sources: https://developers.cloudflare.com/workers/observability/logs/real-time-logs/ and https://developers.cloudflare.com/workers/observability/query-builder/ . Cloudflare Audit Logs are account/API/dashboard action logs, not app-domain audit logs: https://developers.cloudflare.com/fundamentals/account/account-security/audit-logs/ .

So the direct answer is: yes, production has one recorded loan deletion event, `TST042` on June 8. No, I did not find a production deletion log for the two missing Available Deals (`FTM631`, `GQI751`). The absence is not definitive because the durable app audit table is not deployed in production and the historical Worker log query path is returning no stored invocation events.

Changed paths: none outside the agent-doc session document; no EFS code or production data was changed.

Commands: `agent-doc reset --from-current ...`; `agent-doc preflight ...`; `agent-doc plan`; `tsift status`; `tsift status --fix`; `tsift --envelope session-review ...`; Cloudflare Observability API checks via `bun -e`; Cloudflare Audit Logs API checks via `bun -e`; `bun ./bin/cf-wrangler.ts tail -c ../../wrangler.toml --format json`; read-only `curl` to `/admin/pipeline`; read-only `bun ./bin/cf-wrangler.ts d1 execute ...` queries against `audit_logs`, `loan_activity_logs`, `tmo_cache_change_logs`, and `sqlite_master`; targeted `rg`/`sed` review of the delete handlers and TMO delete helper.

Verification: read-only production D1 queries returned one `tmo_cache_change_logs.operation='deleted'` row (`TST042`, `2026-06-08 15:08:43 UTC`), zero delete-matching `loan_activity_logs` rows, no production `audit_logs` table, one Cloudflare control-plane `DELETE` event for a Worker secret, and 13 D1 write-bearing account audit events that lack SQL bodies. Live Worker tail was verified with a fresh `/admin/pipeline` request; historical persisted Worker log queries returned zero rows.

Confidence: medium. The observed log evidence is concrete, but Cloudflare's historical Worker log query path did not return stored app request events, and production lacks the durable `audit_logs` table that would make this attribution high-confidence.

Escalation: none.

### Re: TMO sync delete risk + soft-disable trail — gpt-5

> **Queue prompt:**
>
> Could the delete event have occured from a TMO sync bug?
>
> **Queue prompt:**
>
> No record should be deleted, only disabled. Create a migration. The durable app audit table should be deployed. Do we have sufficient logging to track deletion events? The delete borrower should not cascade. No deleted records. Only disabled records. We need a trail.

A scheduled TMO sync bug is unlikely to have hard-deleted the missing EFS rows. The read-side TMO sync updates local loan/deal fields and cache records; the cache delete helper only marks `tmo_cache_entities.deleted_at` and logs `tmo_cache_change_logs.operation='deleted'`. The real hard-delete risks were explicit app paths: admin loan delete, admin TMO-loan delete/bulk-delete, borrower loan delete, and admin user delete with the old borrower loan cascade chain.

I changed those paths so records are disabled, not deleted:

- Added `0101_soft_delete_guards.sql`, an additive D1 migration that adds `disabled_at`, `disabled_by_user_id`, and `disabled_reason` metadata, plus `disabled_route` on loans.
- Added D1 triggers that abort hard deletes on `users`, `loans`, and `investment_deals`, so deleting a borrower cannot cascade into loans/deals.
- Changed admin loan, TMO-loan, borrower loan, and admin user delete handlers to write audit rows and update disable metadata/status instead of issuing hard deletes or TMO `DeleteLoan` calls.
- Changed deal rows to disable by `disabled_at` metadata, while active deal lists filter disabled deals out without changing the existing checked deal status values.
- Kept app audit insertions durable now that `0100_audit_logs.sql` is deployed; audit insert failures are logged instead of masking the primary disable operation.
- Updated the admin UI copy from delete to disable and updated docs/spec tests to require row preservation and audit trails.

Logging is sufficient going forward for these app paths: loan disables write `loan_disabled` / `borrower_loan_disabled`, user disables write `user_disabled`, TMO-cache orphan detection still writes app audit rows, and production now has the durable `audit_logs` table. It is not retroactive for the previously missing rows because that table was not deployed at the time.

Deployed production changes:

- Applied remote D1 migrations `0100_audit_logs.sql` and `0101_soft_delete_guards.sql`.
- Deployed Worker version `3c6745a6-d281-458e-a937-d25c203c5cc9` to `https://equityfundingsource.brian-takita.workers.dev`.
- Verified remote schema contains `audit_logs`, `prevent_users_hard_delete`, `prevent_loans_hard_delete`, and `prevent_investment_deals_hard_delete`.

EFS commit: `aa4cf71 fix: disable loans and users instead of deleting`, pushed to `origin/main`.

Changed paths:

- `app/equityfundingsource/migrations/0101_soft_delete_guards.sql`
- `app/equityfundingsource/lib/admin/repo.ts`
- `app/equityfundingsource/lib/admin/loan_management/index.ts`
- `app/equityfundingsource/lib/admin/user_management/index.ts`
- `app/equityfundingsource/lib/borrower/loans/index.ts`
- `lib/ui--browser--efs/portal/admin/loans.tsx`
- `lib/ui--browser--efs/portal/admin/pipeline.tsx`
- `docs/business-logic.md`
- `docs/current_progress.md`
- `tests/admin-pipeline.test.ts`
- `tests/portal-table-dialog.test.ts`

Verification:

- `bun test tests/admin-pipeline.test.ts --test-name-pattern "loan disable and TMO-cache orphaning"`
- `bun test tests/admin-pipeline.test.ts --test-name-pattern "disable actions|loan disable"`
- `bun test tests/portal-table-dialog.test.ts --test-name-pattern "admin loan dialogs use PortalModal"`
- `bun test tests/admin-deal-publish-gate.test.ts`
- `bun run cf:d1:migrate:local`
- `make check` — 1,142 tests passed, app build passed, worker build passed.
- `make deploy` — final deploy succeeded with no pending migrations.
- `bun run cf:d1:migrations:list:remote` — no migrations to apply.
- `curl /` returned `200`; `curl /admin` returned `303` through Access.
- Remote D1 schema query confirmed the audit table and delete-prevention triggers.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for this repo.

Confidence: high for preventing future hard deletes through these app/DB paths. It does not prove the historical missing rows were caused by one specific actor because the durable app audit table was not present then.

Escalation: none.

### Re: application logging + deploy — gpt-5

> **Queue prompt:**
>
> We should log all web requests, api calls, migrations, etc. in the application logs.
>
> **Queue prompt:**
>
> deploy

Implemented and deployed structured application logging for the production Worker paths we can control directly.

What changed:

- Every Worker web request now emits a structured `[app-log]` `web_request` line with method, path, route kind (`api`, `auth`, `portal`, `public`, etc.), status, duration, Cloudflare ray, and user agent. Errors include exception fields and use `warn`/`error` severity based on response status.
- Unhandled Worker errors now emit a structured `[app-log]` `web_request_error` line before returning the existing error response.
- Scheduled Worker runs now emit `[app-log]` `scheduled_event`, `scheduled_task_started`, `scheduled_task_done`, and `scheduled_task_failed` lines for TMO sync and auth-token cleanup.
- D1 migration wrapper commands now emit `[app-log]` `migration_command` entries for `d1 migrations ...`, including whether the command is local or remote.
- The local dev Worker log sidecar now captures `[app-log]` alongside existing `[tmo-api]` and `[dropbox-api]` API logs.
- Added a regression test that keeps request logging, scheduled task logging, migration logging, and API log capture wired.

I kept the logs metadata-oriented: no request bodies, authorization headers, tokens, or secrets are logged.

Deployed production changes:

- Deployed Worker version `8bc3cffa-e2d8-403c-9344-7ebd02f30c48` to `https://equityfundingsource.brian-takita.workers.dev`.
- Remote D1 migration check reported `No migrations to apply`; the migration command itself emitted the new `[app-log]` `migration_command` entry.

EFS commit: `f0ccec3 feat: add structured application logging`, pushed to `origin/main`.

Changed paths:

- `app/equityfundingsource/lib/app-log.ts`
- `app/equityfundingsource/worker.ts`
- `app/equityfundingsource/bin/cf-wrangler.ts`
- `app/equityfundingsource/bin/dev-worker.ts`
- `tests/app-logging.test.ts`
- `tests/email-login.test.ts`

Verification:

- `bun test tests/email-login.test.ts --test-name-pattern "scheduled worker purges stale auth token rows"`
- `bun test tests/app-logging.test.ts`
- `make check` — 1,145 tests passed, app build passed, worker build passed.
- `make deploy` — succeeded; deployed Worker version `8bc3cffa-e2d8-403c-9344-7ebd02f30c48`.
- `bun run cf:d1:migrations:list:remote` — no migrations to apply, and the wrapper logged the migration command.
- Production smoke checks: `/` returned `200`; `/admin` returned `303`; `/api/admin/pipeline` returned `401` unauthenticated.
- `gh run list --workflow CI --limit 1` could not find a workflow named `CI`; `gh workflow list` returned no workflows for the EFS repo.

Confidence: high for Worker request logging, scheduled task logging, migration command logging, and the existing TMO/Dropbox API log capture path. This does not retroactively create historical request logs before this deployment.

Escalation: none.

### Re: deploy — gpt-5

> **Queue prompt:**
>
> ~~No record should be deleted, only disabled. Create a migration. The durable app audit table should be deployed. Do we have sufficient logging to track deletion events? The delete borrower should not cascade. No deleted records. Only disabled records. We need a trail.~~ — auto-struck: answered this cycle (#ftstrike) 


> **Queue prompt:**
>
> deploy

The logging changes are deployed.

Deployment details:

- Worker URL: `https://equityfundingsource.brian-takita.workers.dev`
- Worker version: `8bc3cffa-e2d8-403c-9344-7ebd02f30c48`
- EFS commit deployed: `f0ccec3 feat: add structured application logging`
- Remote D1 migrations: no pending migrations to apply.

Changed paths: none in this follow-up cycle; the deployed code paths were recorded in the prior logging response.

Commands:

- `make deploy`
- `bun run cf:d1:migrations:list:remote`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/admin`
- `curl -s -o /dev/null -w '%{http_code}\n' https://equityfundingsource.brian-takita.workers.dev/api/admin/pipeline`

Verification:

- Deployment succeeded.
- Migration list reported `No migrations to apply`.
- Public home returned `200`.
- `/admin` returned `303` through Access/login flow.
- `/api/admin/pipeline` returned `401` unauthenticated.

Confidence: high.

Escalation: none.

### Re: Claude/Codex session-history delete attribution — gpt-5

> **Queue prompt:**
>
> Please look through the Claude + Codex session history. Did the deletions occur from one of these sessions?

I do not see evidence that the missing EFS loans/deals were deleted by a Claude or Codex session.

What I found in the recorded agent history is consistent with investigation, smoke checks, deployments, and later guardrail/logging work, not destructive production data operations. The relevant sessions include GET requests against protected loan/admin pages, read-only D1/log/schema queries, source-code searches for delete paths, and deploys for the auto-publish, soft-disable, and logging fixes. I did not find an actual recorded `curl -X DELETE`, inline `fetch(... method: 'DELETE' ...)` execution, remote D1 `DELETE FROM loans/users/investment_deals`, or TMO `DeleteLoan` operation against the missing production records.

Important caveats:

- This answers what appears in the local Claude/Codex command/session history. It cannot rule out a manual browser action, an external user, a Cloudflare-dashboard action, an older unrecorded session, or a path that predated durable app-side deletion logging.
- The prior production-log finding still stands: the only clear production-domain delete event found was `TST042` in `tmo_cache_change_logs` at `2026-06-08 15:08:43 UTC`. I still do not have a production delete log tying `FTM631` or `GQI751` to Claude/Codex.
- The historical app did have hard-delete routes that could remove loans and cascade `investment_deals`; those paths were the risk that the later soft-disable guards addressed. I just did not find evidence that the agents invoked those paths for these records.

**Changed Paths**

- Session document only: `tasks/professional/equityfundingsource.md`
- No EFS app/source files changed for this answer.

**Commands**

- `tsift status`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- Targeted `rg` searches across `/home/brian/.claude/projects/-home-brian-work-btakita-agent-loop`, `/home/brian/.codex/sessions`, `/home/brian/.codex/history.jsonl`, `.agent-doc/archives`, `.agent-doc/ack-content`, `.agent-doc/op-capture`, and this session document for `FTM631`, `GQI751`, `45ddae53`, `7a0c1463`, `TST042`, delete routes, `DELETE FROM`, `bulk-delete`, and `DeleteLoan`.
- `jq` extraction of actual Codex `exec_command` calls and Claude `Bash` tool calls, followed by destructive-command filters for `curl -X DELETE`, remote D1 `DELETE FROM loans/users/investment_deals`, app loan delete endpoints, bulk delete, and TMO `DeleteLoan` patterns.
- `git status --short`

**Verification**

- The broad destructive-pattern searches initially found false positives: code searches, read-only audit queries containing `raw_method: "DELETE"` as a filter, prior written analysis, and unrelated local code-edit text.
- The refined checks over actual Claude/Codex executed command records completed with no matches for real destructive execution shapes: no `curl -X DELETE`, no inline DELETE fetch script, and no remote D1 hard delete against `loans`, `users`, or `investment_deals`.
- I inspected the relevant EFS agent-session context around the June 2026 work. The agent activity I found was GET smoke checks, code review/search, read-only production checks, migrations/deploys, and logging/soft-disable changes.

**Confidence**

Medium-high. The recorded agent command history is a strong negative signal for Claude/Codex-caused deletions. The remaining uncertainty is that the production app lacked durable deletion logging before the later fixes, and local transcripts cannot prove what happened outside recorded agent tool calls.

**Escalation**

None.

### Re: stormrunner email and D1 state — gpt-5

> **Queue prompt:**
>
> I see stormrunner06106@gmail.com as user emails. We previously chanegd the user emails. Did the database revert to a previous state?

No, the production EFS app database does not look reverted.

Current remote D1 `users` state shows the renamed rows are still present and the old source emails are absent:

- `btak.dev+admin2@gmail.com` exists, role `borrower`, row id `48acd458-c531-4833-9cc1-017528acf48d`.
- `btak.dev+broker2@gmail.com` exists, role `broker`, row id `37b88820-2a05-443c-85e7-7146660dc8b7`.
- `btak.dev+lender2@gmail.com` exists, role `lender`, row id `68b25f71-9208-43ba-ab3f-047f3f8b767d`.
- Exact lookup for `stormrunner06106@gmail.com`, `test2@broker.com`, and `test2@investor.com` returned zero rows.

So if you are seeing `stormrunner06106@gmail.com`, it is probably not coming from the EFS `users` table. The likely source is Cloudflare Email Routing destination addresses: a June 22 session captured the Cloudflare dashboard destination list for `Brian.takita@gmail.com's Account`, and that list included `stormrunner06106@gmail.com` alongside `brian.takita@gmail.com`, `btak.dev+efs@gmail.com`, `dev-slack@briantakita.me`, and `info@briantakita.me`. Those are Cloudflare account/email-routing destinations, not EFS application users.

Other checks:

- Remote D1 has 14 total users; the oldest `created_at` is `2026-04-27 07:12:50 UTC`, and the newest user update is `2026-06-22 22:53:08 UTC`. That does not look like a wholesale restore to an old pre-rename database snapshot.
- `audit_logs` exists now, but it only has 3 rows, all created `2026-06-23 00:33:43 UTC`, and none mention `stormrunner` or `btak.dev+admin2`. That matches the earlier finding that durable audit logging was deployed after the rename, so it cannot prove the June 17 rename history.
- Remote D1 migrations report `No migrations to apply`.
- I attempted to query Cloudflare Email Routing destination addresses through the Cloudflare API, but the current token returned `403 Authentication error` for that endpoint. That limits current API confirmation of the email-routing list, but it does not affect the D1 result.

Changed paths: session document only; no EFS code or production data was changed.

Commands: `tsift status`; `tsift status --fix`; `tsift index --workspace .`; `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc read ...`; targeted `rg`/`sed` over the session archives and EFS rename code; read-only remote D1 `SELECT` queries against `users`, `sqlite_master`, and `audit_logs`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 migrations list ... --remote`; Cloudflare Email Routing API read attempt via `bun -e`.

Verification: remote D1 query directly confirmed the renamed app-user rows and confirmed zero rows for the three old source emails. No D1 write occurred (`changed_db=false`, `rows_written=0` on the read queries).

Confidence: high that the EFS app database did not revert. Medium on the exact current Cloudflare Email Routing destination list because the API token lacks access to that endpoint, but the local session history strongly points to that as the `stormrunner` source.

Escalation: none.

### Re: D1 backups — gpt-5

> **Queue prompt:**
>
> Do we have database backups?

Yes, for the production D1 database we have Cloudflare D1 Time Travel / point-in-time recovery. I verified the live `equityfundingsource` D1 database responds to `wrangler d1 time-travel info` and returned a current bookmark:

`000004cd-0000002e-00005093-712f588e6a4644f31de6c58da8484d1c`

I also retrieved a timestamp bookmark for `2026-06-22T00:00:00Z`:

`00000487-0000001a-00005092-96db74e790c4f0a3f31f27f3e46f9311`

Cloudflare's current D1 docs say Time Travel is D1's backup/point-in-time recovery mechanism, is always on, automatically creates bookmarks, and can restore to a minute-level point in the retention window. The limit is 30 days on Workers Paid or 7 days on Free. Sources: https://developers.cloudflare.com/d1/reference/time-travel/ and https://developers.cloudflare.com/d1/platform/limits/ .

Important caveat: restoring Time Travel is destructive in-place. Cloudflare's docs say restore overwrites the database and cancels in-flight queries. I did not run a restore. If we need to recover specific missing rows, the safer first move is to identify the timestamp/bookmark, export or inspect what we can, and only restore production with an explicit recovery plan and current bookmark captured for rollback.

What we do not appear to have yet: a separate long-retention D1 export backup job. I searched the EFS repo and this session history for `d1 export`, `time-travel`, backup jobs, and R2 database exports. I found prior Time Travel inspection notes and Cloudflare's `wrangler d1 export` support, but no scheduled Worker/Workflow/R2/offsite SQL export implemented for this project. Cloudflare's docs explicitly mention D1-to-R2 exports as useful when you want to keep database state longer than the Time Travel retention window.

Changed paths: session document only; no EFS code or production data was changed.

Commands: `agent-doc plan ...`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 --help`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 info equityfundingsource -c ../../wrangler.toml --json`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 time-travel info equityfundingsource -c ../../wrangler.toml`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 time-travel info equityfundingsource -c ../../wrangler.toml --timestamp 2026-06-22T00:00:00Z --json`; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 export --help`; targeted `rg` for backup/export jobs.

Verification: live D1 metadata returned database `280e1539-b940-4c59-8988-fbed63c12ac0`, 80 tables, size 1,683,456 bytes, and Time Travel returned valid bookmarks. No restore, export, or write operation was run.

Confidence: high that Cloudflare Time Travel backups are available for the live D1 database; medium that no separate long-retention export job exists, based on repo/session search rather than Cloudflare dashboard configuration.

Escalation: none.

### Re: Time Travel check — gpt-5

> **Queue prompt:**
>
> Did we use Timetravel incorrectly? It seems that the deal data is from the past.

I do not see evidence that we used D1 Time Travel incorrectly or restored production to a past point.

The Time Travel commands I found were `d1 time-travel info` calls and help text checks. `time-travel info --timestamp ...` only returns a bookmark for a timestamp; it does not query through the application, change current reads, or mutate the database. I did find `time-travel restore ... --help` / `time-travel restore --help` command history, but no executed restore against `equityfundingsource` and no restore-by-bookmark command.

The current production D1 state also does not look like a whole-database rollback:

- `users` still has recent state: 14 rows, with `MAX(updated_at)=2026-06-23 01:09:10 UTC`.
- `audit_logs` exists and has rows at `2026-06-23 00:33:43 UTC`.
- latest remote migrations include `0100_audit_logs.sql` and `0101_soft_delete_guards.sql`.
- `tmo_cache_entities` for loans still has 12 cached loan records, 11 active, with `MAX(updated_at)=2026-06-22 20:00:23 UTC`.
- `loans` is still empty and `investment_deals` is still empty.

So the “deal data is from the past” signal is more likely old TMO/cache/business data surfacing, not a Time Travel restore. The old TMO cache rows can carry historical loan/application values even while the local EFS `loans` and `investment_deals` tables are empty. That matches the earlier diagnosis: local EFS deal rows are gone/orphaned, while TMO cache still has active-looking older records.

I did not run any restore, export, or write operation in this cycle. I only ran read-only production D1 `SELECT` checks and command-history searches.

Changed paths: session document only; no EFS code or production data was changed.

Commands: `tsift status`; `tsift index --workspace .`; `agent-doc preflight ...`; `agent-doc plan ...`; `agent-doc read ...`; `tsift --envelope context-pack ...`; targeted `rg`/`jq` searches for Time Travel restore usage; `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 time-travel info ... --json`; read-only remote D1 `SELECT` queries against `audit_logs`, `users`, `loans`, `investment_deals`, `d1_migrations`, and `tmo_cache_entities`.

Verification: current D1 bookmark lookup succeeded; all production D1 queries reported `changed_db=false` and `rows_written=0`; current D1 has recent user/audit/migration/cache evidence but still has zero `loans` and zero `investment_deals`; command-history search found Time Travel info/help usage, not a production restore command.

Confidence: high that no agent-run Time Travel restore caused the current state. Medium-high that the past-looking deal data is stale/orphaned TMO cache rather than D1 rollback; proving the original row-loss cause remains limited by the missing pre-fix app audit trail.

Escalation: none.

### Re: lucas-huang/stormrunner application deletion — gpt-5

> The loan application records were deleted by lucas-huang. The strange thing about this is they had his old stormrunner email address. I changed the user email to a btak.dev+something email. Any other thoughts as to why this happened?

Yes. The old `stormrunner06106@gmail.com` value does not require the current `users` row to have reverted. Current production D1 still has the mapped rows as `btak.dev+admin2@gmail.com`, `btak.dev+broker2@gmail.com`, and `btak.dev+lender2@gmail.com`; I did not find a current `stormrunner` user row. The account rename spec also only updates `users.email` (`stormrunner06106@gmail.com` -> `btak.dev+admin2@gmail.com`). It does not rewrite denormalized application data.

The application flow has at least one denormalized email field: `application_personal.email`. Admin feedback code also prefers `sections.personal?.email` over the borrower user email. So an application created before the rename could still carry the old `stormrunner` email inside its application section payload, even while the login/user record now shows `btak.dev+admin2@gmail.com`. If the deletion view, payload, request log, or feedback/audit snapshot came from application section data, that explains the mismatch.

The more important finding is that full applications still have a hard-delete surface. `DELETE /api/admin/full-applications/:id` calls `application_admin_delete_`, which explicitly deletes section rows (`application_personal`, addresses, signatures, employment, income, assets, liabilities, REO, subject property, declarations, etc.), then deletes `full_applications`, and also deletes the linked lead if present. This is separate from the prior soft-delete work on users/loans/investment deals.

That matches current production state: `full_applications` is now `0`; checked section tables such as `application_personal`, `application_addresses`, `application_broker_info`, `application_change_logs`, `application_documents`, `application_feedback_reports`, and `application_subject_property` are also `0`; `application_leads` still has `6` rows. Loans and investment deals are still `0` too. So this looks like an actual full-application tree deletion, not just a UI filter or cache issue.

The current app `audit_logs` do not prove the original deleter. The only recent rows I found are `tmo_cache_loan_orphaned` entries for `FTM631`, `GQI751`, and `TST042` at `2026-06-23 00:33:43`, created by `btak.dev+admin3@gmail.com`; those record orphan detection after the fact, not the delete action itself. If Cloudflare/request logs show `lucas-huang` invoking the delete, the code path above makes that plausible, but the D1 `audit_logs` table by itself does not establish it.

My recommended next fix is to treat full applications as part of the same “no destructive production deletes” surface: replace admin full-application hard delete with soft-delete/archive semantics, preserve section rows, write a durable audit log with actor/id/reason, and add regression tests for admin deletion and user-cascade cases. I added that as a backlog item.

Proof:

- Changed paths: `tasks/professional/equityfundingsource.md` only; no code or production data writes.
- Commands: `agent-doc preflight`, `agent-doc plan`, `tsift status`; read-only production D1 queries through `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 execute ... --remote --command ...`; local source reads with `rg`, `sed`, and `nl`.
- Verification: confirmed mapped current users; confirmed current app/audit counts; confirmed hard-delete route and repository implementation in source. Two broad `UNION ALL` count attempts failed with D1 `too many terms in compound SELECT`, then narrower multi-statement read-only counts succeeded.
- Confidence: medium-high for the old-email explanation and hard-delete gap; medium for the exact actor because current app audit rows do not include the original deletion event.
- Escalation: none. I did not use Time Travel restore, mutate D1, deploy, or modify EFS source.
❯ The deal pipeline did not display the stormrunner email address when the email address was changed.

### Re: deal pipeline did not show stormrunner — gpt-5

> The deal pipeline did not display the stormrunner email address when the email address was changed.

That correction makes sense. It means my prior `application_personal.email` explanation is probably too broad for the specific thing you observed in the Deal Pipeline. The Deal Pipeline card path does not primarily read `users.email`: loan cards use `loans.borrower_email`, open lead cards use `application_leads.email`, and incomplete full-application cards use a `COALESCE(ap.email, al.email, borrower.email, bi.email, broker.email)` query. If the board was already showing the `btak.dev+...` address after the rename, then the visible Deal Pipeline surface was not the stale `stormrunner` source.

I found a more concrete source: current production `tmo_cache_entities` contains a raw TMO loan cache row for `source_id='TST113'` with `ShortName='Huang, Lucas'` and `EmailAddress='stormrunner06106@gmail.com'`. That row has `efs_id=NULL`, `sync_status='fresh'`, was created `2026-06-08 14:56:10 UTC`, and last synced `2026-06-08 14:58:51 UTC`. There is no matching current EFS `loans` row for `TST113`, and exact current-D1 searches found no `stormrunner` rows in `users`, `application_leads`, `application_feedback_reports`, `audit_logs`, or `tmo_cache_change_logs`.

So the better explanation is: the stale `stormrunner` email is coming from an external/source cache record, not from the current EFS user account or necessarily from the Deal Pipeline UI. The EFS rename only updates app-owned `users.email`; it does not rewrite historical TMO loan payloads already cached from TMO. If the deletion evidence named `lucas-huang` with the stormrunner address, it may have been reading from that TMO payload or another external identity/cache surface, while the Deal Pipeline display was already clean.

This does not change the destructive-delete finding. Full applications still have a hard-delete admin path, and production currently has zero `full_applications` plus zero rows in the application section tables I checked. But the email mismatch should now be read as a cross-source identity/cache artifact, not evidence that the Deal Pipeline reverted or that the EFS `users` rename failed.

Proof:

- Changed paths: `tasks/professional/equityfundingsource.md` only; no code or production data writes.
- Commands: `agent-doc preflight`, `agent-doc plan`, `tsift status`; source reads with `rg`, `sed`, and `nl`; read-only production D1 queries through `bun ./app/equityfundingsource/bin/cf-wrangler.ts d1 execute ... --remote --command ...`.
- Verification: confirmed Deal Pipeline source paths; confirmed `TST113` TMO cache row contains `EmailAddress='stormrunner06106@gmail.com'` / `ShortName='Huang, Lucas'`; confirmed no current matching EFS loan row and no current `stormrunner` match in the obvious app-owned email/audit tables.
- Confidence: high that the current proven stale source is `tmo_cache_entities` for `TST113`; medium on whether that exact row is the same evidence you saw, because I did not inspect the external UI/log that reported the deletion.
- Escalation: none. I did not use Time Travel restore, mutate D1, deploy, or modify EFS source.
## full-app-soft-delete #ktqk — gpt-5

Implemented the full-application delete replacement in `src/equityfundingsource-dev`; submodule commit `68346f4` is `Archive full applications instead of deleting`.

### changed_paths — gpt-5

- `app/equityfundingsource/migrations/0102_full_application_soft_delete.sql`: adds archive metadata, active-draft partial indexes scoped to `archived_at IS NULL`, and a guard trigger that rejects hard deletes from `full_applications`.
- `app/equityfundingsource/lib/borrower/applications/repo.ts`: replaces admin hard delete with archive semantics, durable `audit_logs` writes, optional linked-lead archival, and active-only reads/updates for borrower/admin flows.
- Admin, borrower, dashboard, pipeline, nav-count, investor-activity, and publish-eligibility queries now exclude archived full applications while preserving existing child section rows.
- Regression tests updated for admin archive behavior, audit rows, preserved child data, active-only counts, and the migration hard-delete guard.

### commands — gpt-5

- `bun run check` in `src/equityfundingsource-dev`: passed (`1146 pass`, `0 fail`; build and worker build passed).
- `git -C src/equityfundingsource-dev diff --check`: passed.
- `make check` at `/home/brian/work/btakita/agent-loop`: passed.

### verification — gpt-5

The admin delete route now archives the full application instead of deleting the root or child application-section rows. Current pipeline/list/count/activity/publication queries only consider `full_applications.archived_at IS NULL`, so archived applications stop showing in active admin and borrower surfaces without losing the application history.

### confidence — gpt-5

High. The full EFS test suite/builds and the repo-level check passed after the migration and query updates.

### escalation — gpt-5

No escalation or external services were required.
## Re: Lucas Huang TMO sandbox vs EFS DB — gpt-5

Lucas Huang is not present in the current app-owned EFS tables I checked, but there is a TMO-origin cache row in EFS D1.

The concrete row is `tmo_cache_entities` with `entity_type='loan'`, `source_id='TST113'`, `ShortName='Huang, Lucas'`, `FullName='Lucas Huang'`, `EmailAddress='stormrunner06106@gmail.com'`, `LoanStatus='Application Received'`, and TMO `RecID='89B3B9CD0A6E47DF81161AD805897B5C'`. It was created at `2026-06-08 14:56:10 UTC` and last synced at `2026-06-08 14:58:51 UTC`.

That is not the same as a current EFS user/application/loan row. Current production D1 counts returned zero matches for Lucas/stormrunner/TST113 across `users`, `application_leads`, `loans`, and `full_applications`; the renamed `btak.dev+admin2@gmail.com` user still exists separately.

What this proves: Lucas/TST113 was in the EFS TMO cache, and that cache row was created by TMO sync. The sync batches for the row succeeded: auto sync `83a6c0c6-f8da-4da2-a554-6bf1b8b63103` at `2026-06-08 14:56:10-14:56:35 UTC`, then manual sync `50a2950d-9201-4d69-8d40-d7b4e522141a` at `2026-06-08 14:58:51-14:59:02 UTC`, each with `loans_count=1`. The source code path for that sync calls live TMO `GetLoan` and stores the returned payload, so this is strong evidence the row came from TMO sandbox/API data at that time, not from an EFS-only user record.

What I cannot prove from this machine: whether Lucas/TST113 still exists in the live TMO sandbox right now. The repo `.env` does not contain `TMO_API_TOKEN` or `TMO_DATABASE`, so a direct local `GetLoan('TST113')` / `GetLoans` check returned `TMO not configured`. Production does have `TMO_API_TOKEN` and `TMO_DATABASE` configured as Worker secrets, but Cloudflare only lets us list secret names, not read their values.

So the accurate answer is: not only the ordinary EFS app database. Lucas is currently proven in EFS's cached copy of a TMO loan payload, and not in the current app-owned EFS user/lead/loan/application rows. Current live TMO sandbox presence needs a live TMO lookup through the Worker environment or the TMO portal to be certain.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only for this answer.
- No EFS code or production data was changed in this question cycle.

### commands — gpt-5

- Read-only remote D1 `SELECT` queries against `tmo_cache_entities`, `tmo_cache_change_logs`, `tmo_sync_batches`, `users`, `application_leads`, `loans`, and `full_applications`.
- `wrangler secret list` through `app/equityfundingsource/bin/cf-wrangler.ts` to confirm production has TMO secret bindings by name.
- Local Bun script calling `tmo_live_loan_R_('TST113')` and `tmo_live_loans_R_` with repo `.env`, which failed closed with `TMO not configured`.
- Source reads of `app/equityfundingsource/lib/tmo-api/index.ts` and `app/equityfundingsource/lib/tmo-sync.ts` to verify the cache population path.

### verification — gpt-5

All remote D1 checks were read-only (`changed_db=false`, `rows_written=0`). The cache row and sync batch metadata were returned from production D1; app-owned table checks returned zero Lucas/stormrunner/TST113 matches.

### confidence — gpt-5

High that Lucas/TST113 is in the EFS TMO cache and not in current app-owned EFS tables. Medium-high that the cache row originated from TMO sandbox/API data on June 8, based on the sync logs and code path. Medium on current live TMO sandbox presence because the live TMO secret values are not locally readable.

### escalation — gpt-5

No escalation. No Time Travel restore, D1 write, deploy, or TMO mutation was performed.

### Re: Is Lucas Huang in the TMO sandbox? Or only in the EFS database? — gpt-5

> **Queue prompt:**
>
> Is Lucas Huang in the TMO sandbox? Or only in the EFS database?


Direct answer: Lucas Huang is not in the current app-owned EFS tables I checked. He is in EFS D1 only as a cached TMO loan payload for `TST113`, and that cache row was populated by TMO sync on `2026-06-08`.

That means this is not just an EFS user/application row. It is a cached copy of TMO-origin data: `TST113`, `ShortName='Huang, Lucas'`, `EmailAddress='stormrunner06106@gmail.com'`, `LoanStatus='Application Received'`, and TMO `RecID='89B3B9CD0A6E47DF81161AD805897B5C'`. The live app-owned tables returned zero matches for Lucas/stormrunner/TST113 in `users`, `application_leads`, `loans`, and `full_applications`.

I cannot prove from this machine whether `TST113` still exists in the live TMO sandbox right now, because the local repo `.env` does not include TMO credentials. Production has `TMO_API_TOKEN` and `TMO_DATABASE` configured as Worker secrets, but Cloudflare does not expose secret values. So the current proven state is: in EFS's TMO cache, not in current app-owned EFS tables; live TMO sandbox still needs a direct TMO portal/API lookup through credentials.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.

### commands — gpt-5

- Read-only production D1 `SELECT` checks for TMO cache, sync logs, sync batches, and app-owned EFS tables.
- `wrangler secret list` to confirm TMO secret bindings by name.
- Local TMO API wrapper check, which failed closed with `TMO not configured`.

### verification — gpt-5

Remote D1 query metadata reported `changed_db=false` and `rows_written=0`; no production data was changed.

### confidence — gpt-5

High for the EFS D1/cache conclusion; medium for current live TMO sandbox presence because a live TMO lookup was not possible with locally available credentials.

### escalation — gpt-5

None.

### Re: Look in the TMO sandbox. Is Lucas Huang in the TMO sandbox? — gpt-5

I could not verify current live TMO sandbox presence from this machine. The live TMO API checks were blocked by authentication, so the answer is not a reliable yes/no.

What I did verify:

- The current EFS D1 cache still has the TMO-origin loan payload for `TST113`: `ShortName='Huang, Lucas'`, `FullName='Lucas Huang'`, `EmailAddress='stormrunner06106@gmail.com'`, `LoanStatus='Application Received'`, and TMO `RecID='89B3B9CD0A6E47DF81161AD805897B5C'`.
- That cache row was populated by TMO sync on `2026-06-08`, so Lucas was represented in data returned from TMO at that time.
- A direct read-only call to live TMO `GetLoan/TST113` using the local password-store token and `Database: API Sandbox` returned `401 Not authenticated`.
- A direct read-only live `GetLoans` call with the same token/database also returned `401 Not authenticated`.
- I tried likely alternate database header values (`Equity Funding Source`, `EquityFundingSource`, `EFS`, `equityfundingsource`) and they all returned `401 Not authenticated`; an empty database header returned `400 Database is missing`.
- Production Worker secrets include `TMO_API_TOKEN` and `TMO_DATABASE` by name, but Cloudflare does not expose secret values. Wrangler remote-dev also used local `.dev.vars`, did not expose those TMO secrets, and did not serve the temporary diagnostic entrypoint because the project config requires Durable Object exports.

So the most precise answer is: Lucas Huang is proven in EFS's cached TMO payload for `TST113`, and that cache came from TMO sync on June 8. I cannot prove whether he is still in the live TMO sandbox today without a valid readable TMO token/database pair or a production-safe read-only diagnostic route that runs inside the Worker environment using its existing TMO secrets.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- The temporary diagnostic Worker file was created and removed; the EFS submodule is clean and has no code changes from this answer.

### commands — gpt-5

- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md`
- `tsift status`
- Source reads for TMO API/admin route behavior.
- Password-store key checks for TMO credentials without printing secret values.
- Direct read-only TMO API checks against `GetLoan/TST113` and `GetLoans`.
- `wrangler secret list` / `wrangler dev --remote` capability checks.
- `git status --short tasks/professional/equityfundingsource.md src/equityfundingsource-dev`

### verification — gpt-5

The only live TMO responses available from this machine were authentication failures, so they do not prove absence from TMO. They prove the locally available TMO token/database combination is not sufficient to query the live sandbox. The EFS submodule working tree is clean after removing the temporary diagnostic file.

### confidence — gpt-5

High that the cache evidence exists and came from prior TMO sync. Low for current live TMO sandbox membership because valid live TMO credentials were not available to the local process.

### escalation — gpt-5

None. No production D1 writes, TMO mutations, deploys, or Time Travel restore were performed.

### Re: Look in the TMO sandbox. Is Lucas Huang in the TMO sandbox? Use `passage equityfundingsource/TMO_API_TOKEN`. — gpt-5

I used `passage equityfundingsource/TMO_API_TOKEN` for the live TMO checks. With that token, TMO did not authenticate the read-only sandbox requests, so I still cannot truthfully answer yes or no for the current live TMO sandbox.

The exact live checks were:

- `GET https://api.themortgageoffice.com/LOS.svc/GetLoan/TST113` with `Database: API Sandbox`: `401 Not authenticated`.
- `GET https://api.themortgageoffice.com/LOS.svc/GetLoans` with `Database: API Sandbox`: `401 Not authenticated`.
- The same `GetLoan/TST113` check with likely alternate database names (`Equity Funding Source`, `EquityFundingSource`, `EFS`, `equityfundingsource`) also returned `401 Not authenticated`; no database header returned `400 Database is missing`.

So the live TMO sandbox lookup was attempted with the specified passage token, but that token is not sufficient for the sandbox API from this environment. The existing positive evidence remains the EFS D1 TMO cache row for `TST113` (`Huang, Lucas`, `stormrunner06106@gmail.com`), created by TMO sync on `2026-06-08`; that proves prior cached TMO-origin data, not current live sandbox presence today.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.
- No EFS code changes remain; the temporary diagnostic file was removed and the submodule is clean.

### commands — gpt-5

- `passage equityfundingsource/TMO_API_TOKEN` used inside shell command substitution without printing the token.
- Read-only live TMO `GetLoan/TST113` and `GetLoans` checks.
- Alternate read-only database-header checks.
- `wrangler dev --remote` diagnostic attempt; it used `.dev.vars`, did not expose production TMO secrets, and did not serve because the entrypoint lacked configured Durable Object exports.
- `git status --short tasks/professional/equityfundingsource.md src/equityfundingsource-dev`.

### verification — gpt-5

The TMO API returned authentication failures for the live reads, so it did not prove Lucas is absent. It proves only that the specified local passage token cannot currently query the sandbox API from this environment.

### confidence — gpt-5

High that the `passage` token was tried and failed authentication. Low for current live TMO sandbox membership because the live lookup did not authenticate.

### escalation — gpt-5

None. No production D1 writes, TMO mutations, deploys, or Time Travel restore were performed.

### Re: Look in the TMO sandbox. Is Lucas Huang in the TMO sandbox? Use `passage equityfundingsource/TMO_API_TOKEN`. — gpt-5

> **Queue prompt:**
>
> Look in the TMO sandbox. Is Lucas Huang in the TMO sandbox? Use `passage equityfundingsource/TMO_API_TOKEN`.


I used `passage equityfundingsource/TMO_API_TOKEN` for the live TMO sandbox lookup. The token was present, but TMO rejected the read-only requests, so I could not verify whether Lucas Huang is currently in the live sandbox.

Results:

- `GetLoan/TST113` with `Database: API Sandbox`: `401 Not authenticated`.
- `GetLoans` with `Database: API Sandbox`: `401 Not authenticated`.
- `GetLoan/TST113` with likely alternate database names also returned `401 Not authenticated`; no database header returned `400 Database is missing`.

The strongest verified fact remains the EFS D1 TMO cache row for `TST113`, populated by TMO sync on `2026-06-08`, with `Huang, Lucas` and `stormrunner06106@gmail.com`. That proves Lucas existed in cached TMO-origin data then. It does not prove current live TMO sandbox presence today because the live API lookup did not authenticate.

### changed_paths — gpt-5

- `tasks/professional/equityfundingsource.md` only.

### commands — gpt-5

- `passage equityfundingsource/TMO_API_TOKEN` used inside shell command substitution without printing the token.
- Read-only live TMO `GetLoan/TST113` and `GetLoans` checks.
- Alternate database-header checks.
- `git status --short tasks/professional/equityfundingsource.md src/equityfundingsource-dev`.

### verification — gpt-5

The live TMO API returned authentication failures, not data. No TMO mutation, D1 write, deploy, or Time Travel restore was performed.

### confidence — gpt-5

High that the specified passage token was tried and failed authentication. Low for current live TMO sandbox membership because the live lookup did not authenticate.

### escalation — gpt-5

None.
<!-- agent:boundary:b6a84b64 -->
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
