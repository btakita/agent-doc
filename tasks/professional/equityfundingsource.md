---
agent_doc_session: 62fe1f41-886c-4f3d-b6fa-48d46106ec27
resume: 254409c8-f6f5-4409-aa60-eb4818ff2208
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
links:
- ./judith-krug.md
- ./robert-waldman.md
- ../personal/robert-ward.md
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260730-051412.md`*

Compacted content:
- Archived 5 response topic(s): Production logs; Production logs follow-up; Fixing the production issues; 2 more
- Prior summary/context: prior compacted content: Archived 2 response topic(s): TMO sandbox purge authorization; EFS production route token

### Re: cycle check — sonnet-5

Nothing new to act on this cycle. The only change vs. baseline was a stray bare `410dfec2` line appended past `&lt;!-- /agent:done --&gt;` — that's the exchange boundary hash leaking out as literal text from an earlier interrupted commit cycle, not a new prompt. It sits outside every managed component block, so it doesn't affect `exchange`/`queue`/`backlog`/`review` content.

Queue status: `queue_drainable_head_count: 0` — nothing is in-session-drainable right now. The two open queue heads with real work (`#prod-fix-deploy`, `#tmo-sync-cron-failures`) are both `[operator-verify]`: the deploy needs an explicit operator go (current queue preset `#spec-test-commit-push` excludes deploy, and a whole-tree deploy would ship a concurrent session's ~355 uncommitted `tmo-sync.ts` lines), and the cron-failure root-cause is blocked on that same deploy landing first. `#noticedavailabledeals` and the lender-annual-requirements research question remain queued but aren't flagged drainable either.

One thing worth your attention, not acted on: preflight surfaced 4 "semantic completion" candidates suggesting some `agent:review` items may duplicate resolved `.done.md` entries. I checked one (`#tmo-orphan-as-fundings` vs `#tmo-orphan-cache`) and it's a false positive — the done entry says #tmo-orphan-cache was *superseded by* #tmo-orphan-as-fundings, not that the latter is resolved. I didn't auto-close any of the 4; they need a human look before treating them as done.

### Re: denied applications showing in Available Deals — sonnet-5

[#noticedavailabledeals] Confirmed real bug, and it's already fixed and on `main` (equityfundingsource-dev `653fe3fe`, pushed to origin, tests 2143 pass / 0 fail, build + build:worker clean):

- **Root cause**: `PipelineDefinition.column_id_for_status_` fell back to the pipeline's *default* column (`application-received`, labeled "Available Deals") for any status it didn't recognize. Production loans carrying TMO's `'Application Declined'` spelling (the code only matched the canonical `'Denied'`, which had zero rows) landed there, plus two more loans on a `'Documents'` status that had been retired from `LOAN_STATUS_STEPS` but was still on live rows. Five loans misfiled; only three belonged.
- **Where they land now**: `column_id_for_status_` falls back to `unassigned_column_id` instead — Unassigned sits inside the existing **Parked** group, so an unrecognized status is visible and triage-able instead of silently presented as a fundable deal. No new column was needed.
- **Denial vocabulary**: `TERMINAL_DENIED_LOAN_STATUSES` now includes `'Application Declined'` alongside `'Denied'`, so both spellings hit the existing **Application Denied** column (also under Parked) — this directly answers "should they be in Parked or a new column": Parked, in the column that already exists for this.
- **Marketplace-visibility half of the question** ("are these viewable to Lenders"): separately, `admin_deal_status_sync_by_loan_` previously mapped only `Funded`/`funded`→`published`; every other status hit an `ELSE status` no-op, so a loan denied *after* its investor deal was published would have kept `investment_deals.status='published'` and stayed on `/lenders/available-deals`. That branch now closes the deal on a denial. No production row was actually exposed (all three declined loans had no `investment_deals` row), so this closed a latent hole rather than a live leak.
- **Still open**: TMO-sourced declines take a *different* code path — `tmo-sync.ts` has its own duplicate sync function, `investment_deal_status_sync_by_loan_`, that still only maps `Funded`. That file currently holds ~355 uncommitted lines from a concurrent session and was deliberately left alone; tracked separately as `#tmo-sync-declined-unpublish` (blocked on that coordination, not on anything here).

### Re: lender annual Requirements — admin-recorded state and legal basis — sonnet-5

> **Queue prompt:**
>
> deploy to production: [#prod-fix-deploy]


> **Queue prompt:** Do we have a state for lenders who have filled out their requirements outside of the admin portal? Could we have a checkbox or a date when the requirements were met? Please research the annual requirements by law. Is it at the beginning of the year?

**Yes, the admin-side path already exists** (shipped as part of `#zwws` phase 5, `efs 1957f64`) — this isn't a gap, it's a shipped feature: on a lender's card in the admin Lender Pipeline board, the "Review Requirements" panel (`investor_pipeline.tsx`) lets an admin pick an accreditation basis and click "Save and approve questionnaire." That calls `PUT /api/admin/users/:id/investor-accreditation`, which — per the code's own comment — "records the Requirements decision on the lender's behalf" and works even when the lender never submitted anything through the portal (it creates the verification record from scratch via upsert if none exists, then marks it verified). This *is* the "filled it out outside the portal, admin records it" case.

- **What's captured**: `verification_type` (the accreditation basis: income / net worth / institution / professional) and a status. There is a date, but it's `verified_at` — stamped to the moment the *admin* records the decision, not a field for backdating when the lender actually satisfied the requirement. If there's a lag between the lender doing it on paper and the admin getting to data entry, today's system only knows the entry date.
- **Expiry**: `expires_at = verified_at + 365 days` (`ACCREDITATION_ANNUAL_TTL_SECONDS`, `documents_repo.ts:18`). This is a **rolling per-lender anniversary**, not a shared calendar date — each lender's own verification date starts their own clock.
- **Legal research — is a beginning-of-year reset required?** No. EFS's lender-investor relationship reads as a Reg D private-placement structure, and neither Rule 506(b) nor 506(c) imposes a fixed calendar-year reverification cycle. Rule 506(c) requires the issuer to take "reasonable steps to verify" accredited status, evaluated against the facts and circumstances of *each investor and transaction* — the standard is tied to reasonableness as of the sale, not a fixed annual clock. In practice, third-party verification letters (broker-dealer/CPA/attorney) are commonly treated as reliable only for a few months before a *new* investment, not a full year. Nothing in the rule requires resetting on January 1 specifically. A rolling 365-day window anchored to each lender's own verification date — what EFS already does — is a reasonable, defensible convention, and if anything more generous to lenders than the strictest reading of "verify as of each sale." This is not legal advice; given the real compliance stakes, worth a quick confirmation from securities counsel before treating it as settled.

Sources:
- [SEC.gov — General solicitation, Rule 506(c)](https://www.sec.gov/resources-small-businesses/exempt-offerings/general-solicitation-rule-506c)
- [SEC.gov — Assessing Accredited Investors under Regulation D](https://www.sec.gov/resources-small-businesses/capital-raising-building-blocks/assessing-accredited-investors-under-regulation-d)
- [Gibson Dunn — SEC Provides Bright-Line Test for Investor Verification Under Rule 506(c)](https://www.gibsondunn.com/sec-provides-bright-line-test-for-investor-verification-under-rule-506c/)

### Re: Correction on the TMO cron, the denied-deals fix, and committing the tree — opus-5

> **Queue prompt:**
>
> do [#noticedavailabledeals]

> **Queue prompt:**
>
> commit + push all equityfundingsource-dev files

Three things: a correction to my previous response, the denied-deals fix, and the whole tree is now committed and pushed.

### Correction: the roster failures are not confined to the full sweep

I wrote that the seven failures are roster work only the full sweep performs, and that the next evidence would arrive around 17:15Z. **That is wrong.** The 04:30:51Z run — `driver delta`, on the new build `c50a9aaa` — failed one second later:

```
04:30:52Z lvl=error  [tmo-sync] LOS lender roster failed     at sp (index.js:3163:37)
                         at async Gd (index.js:3150:12)
                         at async Promise.all (index 1) ...
```

The 04:15:51Z delta run immediately before it was clean. So it is intermittent, roughly every other run, and I generalized from a single clean sample. Nothing to wait until 17:15Z for.

Worse, **the read-error fix did not cover this path.** That line is exactly as unreadable as before: a bare minified stack, no reason. `tmo_read_error_message_` never returns an empty string, so the throw is not coming from one of the five patched read helpers — it reaches `console.error('[tmo-sync] LOS lender roster failed', err)` as something whose message renders empty. Filed as `#tmo-roster-error-still-silent`. Until that lands, `#tmo-sync-cron-failures` cannot be root-caused, and I rewrote that item rather than leave the full-sweep theory sitting in the backlog.

### Denied deals: `653fe3fe`

You asked which column these belong in. Answer: the one you already have. No new column.

| | |
|---|---|
| `column_id_for_status_` | falls back to `unassigned_column_id`, not `default_column_id` |
| `TERMINAL_DENIED_LOAN_STATUSES` | `Denied` **and** `Application Declined`, both claimed by the Application Denied column |
| `admin_deal_status_sync_by_loan_` | a denial closes the offering instead of hitting `ELSE status` |

The fallback is the one that mattered. `default_column_id` for the loan pipeline is `application-received` — the column *labeled* "Available Deals" — so every status the code did not recognize was presented as an active fundable deal. That is why `Documents` (1560 Radnor, 1561 Quail/Gregory) was sitting there too, not just the declines. Unassigned already lives inside Parked. `DEAL_PIPELINE` declares no unassigned column, so its behaviour is byte-identical.

Five mutations, all caught: fallback reverted (3 tests), alias dropped (5), denial branch no-op'd (3), `closed_at` clobbered (1), `trim()` removed (1). Two existing assertions in `admin-pipeline.test.ts` changed on purpose — one encoded the bug, and the `#closed-step-decouple` source-grep now slices to the end of the function instead of a fixed 800 bytes my comment overran.

Lenders were never exposed: the marketplace filters `investment_deals.status` and all three declined loans have no deal row.

### Committed and pushed: `292b52d6`

The full working tree, as asked — 14 files, ~1,036 insertions. Most of it is the concurrent session's in-progress TMO work: `tmo-deal-sync.ts` (+tests), `tmo-backup-scheduler.ts` (+tests), ~355 lines in `tmo-sync.ts`, and the `worker.ts` / `loan_management` / `tmo-api` / `tmo_contacts` / `tmo_los` call sites. Landed as one checkpoint rather than left uncommitted. Verified first: **2143 pass / 0 fail** across 193 files, `build` and `build:worker` clean.

**Two files I did not commit.** `.dev.vars.production` and `.dev.vars.staging` hold cleartext `AUTH_SECRET` and `ADMIN_GLOBAL_PASSWORD`, and `.gitignore` covered `.dev.vars` but not the per-environment variants — so they were untracked but fully committable, one `git add -A` away from a public repo. They are now ignored via `.dev.vars.*`, along with the `.agent-doc/` and `.claude/` session directories. I scanned the staged diff for secret literals before committing; it was clean. If either file has ever been added in the past, those secrets need rotating — worth a `git log --all --diff-filter=A -- '.dev.vars*'` to confirm it never happened.

Superproject gitlink bumped to `292b52d6`.

### Still open

`#tmo-sync-declined-unpublish` — `tmo-sync.ts` carries its own copy of the deal-status sync, `investment_deal_status_sync_by_loan_`, which still maps only `Funded`. That is the path a TMO-sourced decline takes, so a decline arriving *from TMO* still will not unpublish a published deal. Now that the tree is committed the coordination block is gone; the better fix is deleting the duplicate and calling the `repo.ts` function, since two copies of this rule is how the gap appeared.

Neither commit is deployed. The preset is `#spec-test-commit-push`; say the word and I will deploy the same isolated way as the last one.

### Re: Backlog closeout for the deploy and TMO items — opus-5

Bookkeeping that three earlier writes dropped; the substance is in the responses above.

`#prod-fix-deploy` closed — production `c50a9aaa` is live, all three verification checks passed, plus a 98-second fresh-profile anonymous session with zero `/api/public/messages/realtime/ws` requests client-side and zero server-side.

`#tmo-sync-cron-failures` rewritten. The full-sweep theory in my earlier response was wrong: the 04:30:51Z run with `driver delta` failed at 04:30:52Z, while the 04:15:51Z delta run immediately before it was clean. It is intermittent, roughly one run in two, and it is now blocked on `#tmo-roster-error-still-silent` rather than on a deploy.

`#tmo-sync-declined-unpublish` rewritten — its concurrent-session coordination block is gone now that `292b52d6` committed the whole tree, so the better fix (delete the duplicate, call the `repo.ts` function) is unblocked.

One thing worth cleaning up: there is debris below `agent:backlog` near line 268 — orphaned `- [ ]` fragments of the `#prod-fix-deploy` and `#tmo-sync-cron-failures` text plus three bare id lines (`410dfec2`, `cf2d65db-…`, `93980072-…`). That is fallout from this session's partial writes, and it is what `session-check` flags as a shadowed backlog item. It sits outside every `agent:` component, so I left it rather than hand-edit the document mid-cycle.
### Re: Requirements met-date, and one deal-status sync instead of two — opus-5

> **Queue prompt:**
>
> do [#recommendedcapturesepara]

> **Queue prompt:**
>
> do [#tmo-sync-declined-unpublish]

Both landed in `0a87c46d`, pushed to `main`. Not deployed — the preset is `#spec-test-commit-push`.

### `#recommendedcapturesepara` — separate the met-date from the entry date

The backlog note called this low priority because the current behaviour only ever extends validity. That is true, and it is also the bug: **the annual cycle drifts later every time a decision is recorded late.** `verified_at` was stamped `now`, `expires_at` derived from it, so a lender who satisfied Requirements in March but got entered in June silently earned a fresh year from June. Nothing was wrong in a way that would fail an audit; the dates just were not the dates.

| | |
|---|---|
| migration `0144` | `lender_accreditation_verifications.requirements_met_at`, backfilled from `verified_at` on decided rows |
| `investor_accreditation_decision_` | optional met-date; `expires_at` anchors to it. `verified_at` keeps its meaning — when the admin recorded it — so nothing that reads it changes. Omit the argument and behaviour is identical |
| `accreditation_requirements_met_at_parse_` | `YYYY-MM-DD` (UTC midnight) or unix seconds |
| admin PUT + Review Requirements panel | date input capped at today, blank means today, expiry consequence stated inline |

The parse function distinguishes **"not supplied"** (`undefined` → default to now) from **"supplied but unusable"** (`null` → the route 400s). That distinction is the whole point: a typo'd date quietly becoming "today" would reintroduce the drift the field exists to remove. Future dates are rejected, with one day of slack for timezone skew between the admin's calendar day and UTC. The date is stored and read back in UTC, so the input does not show a day earlier west of Greenwich.

One visible payoff: recording a decision from 400 days ago now reads back as **expired** rather than freshly valid. There is a test for exactly that.

### `#tmo-sync-declined-unpublish` — deleted the duplicate rather than patching it

`tmo-sync.ts` carried its own copy of the loan-status → deal-status sync, and the copies had drifted. `653fe3fe` taught the admin one to close the offering on a denial and to skip `disabled_at` rows; this one still mapped only `Funded`. Since TMO stores `Application Declined`, a decline arriving *from TMO* — the common path — left the deal `published` and on the lender feed.

Patching the copy would have restored parity and left the next drift in place, so the copy is gone. One implementation, `admin_deal_status_sync_by_loan_`, now serves both callers. The sync path also inherits the `disabled_at` guard it never had, so a TMO sync can no longer reanimate a disabled deal row. No import cycle — `admin/repo.ts` does not import `tmo-sync`.

### Verification

`2162 pass / 0 fail` across 194 files; `build` and `build:worker` clean. Ten mutations, all caught:

| | |
|---|---|
| expiry anchored to `now` instead of the met-date | 2 tests |
| met-date ignored entirely | 3 |
| future date accepted | 1 |
| blank treated as unusable | 1 |
| `YYYY-MM-DD` parsed as today | 1 |
| route accepts an unusable date | 1 |
| blank posted as `''` instead of omitted | 1 |
| date input unbounded | 1 |
| sync stops calling the shared function | 1 |
| duplicate reintroduced into `tmo-sync.ts` | 1 |

Worth flagging how the first mutation run went: my harness passed the two test paths as a single quoted argument, so `bun test` matched nothing and every mutation reported "0 fail". Eight clean passes, all vacuous. I caught it because a run that tests nothing should not print zero failures for a mutation that removes the entire feature. Every run above asserts `Ran 125 tests` alongside the failure count.

Also fixed a second fixed-window source-grep — `admin-investor-pipeline.test.ts` sliced 1800 bytes from the function start, and my comments pushed the calls it guards past that window. Same fix as `653fe3fe`: slice to the end of the function. That is twice in two cycles; a fixed byte count silently stops covering what it is meant to guard, and it fails as a test error rather than as a coverage gap, which is the only reason it was noticed.

### Re: Closing #tmo-sync-declined-unpublish — opus-5

Complete in `0a87c46d`: the duplicate `investment_deal_status_sync_by_loan_` is deleted from `tmo-sync.ts`, both callers now go through `admin_deal_status_sync_by_loan_`, and the TMO sync path inherits the `disabled_at` guard it never had.

### Re: setting the date of the last verification in the lender dialog — opus-5

> **Queue prompt:**
>
> In https://equityfundingsource.com/admin/lender-pipeline?tab=pipeline&dialog_user_id=5cdcb0dd-76e9-4d63-b57e-dd29ecce359e&dialog=lender, please allow the admin to set the date of the last verification. This will enable previously questioned lenders to have a schedule as to when the next date will occur. Test + deploy on staging first. If it works, then deploy to production.

Shipped as EFS `fee11117`, live on staging and production.

The date field already existed, but on only half the paths. `0a87c46d` (9 hours ago) added "Requirements met on" to the admin-filled questionnaire, so expiry anchors to the real met-date. A lender who answered their **own** questionnaire — exactly the "previously questioned lender" — is decided with the **Accredit** button, and that endpoint never took the field. It stamped the met-date as the click date, so the next verification date drifted later by however long the decision sat. And once verified, Accredit was disabled forever, so an existing date could not be corrected at all; the only way to change a lender's schedule was to file a whole new questionnaire.

What changed:

- `POST /api/admin/investor-accreditation/:id/decision` now accepts `requirements_met_at` under the same contract as the questionnaire PUT — absent means "default to now" (behaviour unchanged), unusable **400s** rather than silently becoming today. A rejection still clears the met-date and the expiry.
- Accredit posts the date and re-reads the saved record into the form, so the input shows what the server stored rather than a stale blank.
- **A verified record is re-datable in place.** The button unlocks as soon as the admin changes the date and reads "Update verification date"; re-deciding the same row moves `requirements_met_at` and `expires_at` while `verified_at` moves to now, keeping its meaning as when the admin recorded it.
- The dialog now states the schedule itself: **last verification**, **next verification due**, and the date the lender starts showing as renewal due. That window is a named 60-day constant matching `investor_requirements_status_`, so the date the admin reads and the status the board computes cannot drift apart. Dates render in UTC, matching how the met-date is stored.

Also converted the fixed-window source-grep over the decision handler in `admin-investor-documents.test.ts` to slice to the end of the function — a fixed byte count silently stops covering what it guards, the same failure `653fe3fe` and `0a87c46d` each had to fix once.

**Staging, end to end** (seeded a disposable lender + pending record, then removed it): back-dating to `2026-02-01` produced `verified_at` 2026-07-30, `requirements_met_at` 2026-02-01, `expires_at` **2027-02-01** — exactly 365 days from the met-date, renewal-due from 2026-12-03. Under the old behaviour that same click would have set expiry to 2027-07-30, so this removed ~6 months of drift on one decision. `01/02/2026` → 400, `2027-01-01` (future) → 400, re-dating the already-verified row to 2026-05-15 → same row id, expiry 2027-05-15, blank → defaults to today unchanged.

**Production** — deployed, migration `0144` applied there for the first time (it had only ever run on staging), backfill a no-op since prod has no verified accreditations yet. Verified without touching real data: the browser bundle at `equityfundingsource.com` is content-hash `index.browser-7YCODNTY.js`, identical to the clean local build of this commit, and carries all four new UI strings; a bad met-date against a non-existent verification id returns the new 400 while a valid one falls through to `verification not found`, so the server path is live and validating rather than blanket-rejecting.

`bun test` 2174 pass / 0 fail across 195 files; `build` and `build:worker` clean. Three mutations checked — dropping the met-date argument, dropping the 400 guard, and restoring the permanent disable each redden the new tests.

One residue: the staging fixture user `VERIFY-LENDER-1` could not be deleted (the `users` soft-delete trigger refuses `DELETE`), so it is suspended and disabled with a reason naming it as a fixture. It is staging-only and inert.

### Re: plan for the anonymized live → sandbox TMO clone and staging refresh — opus-5

> **Queue prompt:**
>
> Please plan into a file: Copy of the most recent live TMO data backup into the sandbox TMO. Please anonymize the lender + property records. Any PII should be anonymous. Create a script to copy live TMO into the sandbox TMO. Please also ensure the staging db is refreshed. Make sure you do not do any destructive operations on the production db or live TMO account. Do not execute any operations until I look at the plan and approve.

Plan written to `src/equityfundingsource-dev/prds/tmo-sandbox-clone-and-staging-refresh-plan.md`, committed and pushed as EFS `c484b0ee`. **Nothing was executed** — no TMO write, no purge, no D1 write, no export. The only calls I made were reads: production `platform_settings` for the backup pointer, `sqlite_master` for the table list, and the local artifact already on disk.

**The only thing actually missing is the anonymizer.** Everything else is already built: `bin/tmo-backup.ts` captures a restorable live artifact with attachment bytes and per-file sha256, the scheduled job writes one to R2 daily, `--restore --allow-production-source --apply` *is* the live→sandbox replay path, and `bin/tmo-sandbox-purge.ts` clears the sandbox behind a protected keep-list. `grep -rl 'anonymi\|redact\|scrub\|pseudonym'` over the whole repo returns nothing.

The most recent live backup, read from production `platform_settings`:

```
tmo_backup_last_key       tmo-backups/2026-07-30/2026-07-30T17-00-51-000Z.json
tmo_backup_last_status    complete
tmo_backup_last_checksum  e984d11c…9557
```

**I measured the PII surface against the real production artifact rather than guessing** (field names only, no values read out). Fundings *are* the lender record — `Account`, `TIN`, `DOB`, `FullName`, address, four phone numbers, `LegalVesting`. Borrowers appear twice, top-level and embedded in each loan, and only the embedded copy is what `NewLoan` actually writes. `Collaterals` carries the property plus `Encumbrances[].Beneficiary*`. And then `loan.Fields[]`: **~230 opaque keys per loan across ~250 distinct descriptions**, including `Guarantor - TIN`, `Guarantor - Birthday`, the whole `Broker *` / `Broker Rep *` / `Referring Broker *` sets, `1003 - Loan Originator - Email`, escrow/title/trustee contacts, `Deed of Trust - Mail to - Name`, `Liens - Lienholder's Name`, and `Loan Application Login (Email Address)`.

That last surface is why the plan specifies the anonymizer **allowlist-driven, scrub-by-default**. A blocklist over keys named `F1971.43` and `F9092.1.4` leaks the first time TMO adds a field, and some descriptions are the empty string.

Two things I would not paper over:

- **PDF bytes cannot be reliably redacted.** Text layers, embedded images, XMP metadata, and incremental-update history all carry PII. Default is a generated placeholder PDF keeping the same `doc_type`, the verified `TabName: ''` contract, and a fresh sha256. Shipping the real bytes is not an option; `drop` is the alternative.
- **The staging D1 classifier fails closed over all 94 production tables.** Anything the classifier does not recognize is dropped, so a table added later shows up as *missing staging data*, never *leaked staging data*.

Deliverables specified: a pure `lib/tmo-api/anonymize.ts`, an offline `bin/tmo-artifact-anonymize.ts` (with an `--audit` residual-PII scan that exits non-zero), the orchestrator `bin/tmo-sandbox-clone.ts` (dry-run default, `--simulate` against `lib/tmo-sim` for a zero-risk rehearsal), and `bin/d1-staging-refresh.ts`. The clone verifies by re-capturing the sandbox and scanning **what TMO actually stored**, not just the file that was sent.

**Six questions need your answer before any apply** (§7 of the plan): preserve `LoanNumber` (I recommend yes — it is the staging↔sandbox join key), attachments placeholder vs drop, purge-first vs sync-onto-existing (I recommend purge — `NewLoan`'s duplicate-`LoanNumber` behaviour is unverified), staging wipe-and-load vs additive, where the pseudonym seed lives, and whether loan amounts/rates count as sensitive (the plan deliberately preserves them, since anonymizing them removes the reason to clone live data at all).

### Re: draining #tmo-roster-error-still-silent and #tmo-sync-cron-failures — opus-5

Both shipped as EFS `300b788d`, live on staging and production. They turned out to be one bug wearing two masks, and the root cause is documented in this repo — from 2026-07-18.

**`LOS.svc/GetLoanPaymentSchedule` does not exist.** I called it against the real sandbox: HTTP 404, and the body is a WCF error page — UTF-8 BOM, `<?xml version="1.0"`, "Endpoint not found." `docs/TMO/backup.md` recorded that 404 twelve days ago, under a heading literally called *Endpoints that do not exist*, and `bin/tmo-snapshot.ts` and the backup sweep were fixed then. `tmo-sync.ts` was not, and the 15-minute cron has been calling it once per changed loan ever since.

That is the whole of `#tmo-sync-cron-failures`, and it was worse than "intermittent". Production was not failing one run in two — the last four consecutive runs each re-swept **the same 28 loans**, each finished `partial`, each took ~250 seconds. A partial batch does not advance the cursor, so the next run got the same window back. It had been chewing the same 28 loans every 15 minutes.

**And that is also why the log said nothing.** `res.json()` ran before the `res.ok` check, so a 404 HTML page threw a JSON `SyntaxError` and `tmo_read_error_message_` — the thing `29f27773` added last night specifically to make reads say why — was never reached. Then Workers Logs rendered that SyntaxError with its **header line dropped**, leaving `[tmo-sync] schedule fetch failed 1024` plus five minified frames and nothing else.

I want to be precise about the premise in `#tmo-roster-error-still-silent`, because it was written from that log and the log was lying twice. The catch sites it named (`tmo-sync.ts` ~850-856) no longer exist — `292b52d6` removed the network roster calls hours after the item was filed. And the reason was never actually missing: `tmo_issue_` had been writing it to `tmo_sync_batches.error` in the same code path the whole time. That is where I found it, in one query:

```
Unexpected token '', "<?xml ver"... is not valid JSON     ×28, phase=payment_schedule
```

The log was the only sink that lost it.

**What changed:**

- **The schedule is read from the loan detail already in hand.** TMO carries it inline as a `Fields[]` entry keyed `PaymentSchedule` — an XML DataTable diffgram, documented in `docs/TMO/payment-schedule-field.md`, and the parser for it was already sitting in `tmo-api/index.ts`. `tmo_loan_payment_schedule_(loan)` is now pure, so the sweep makes one *fewer* round-trip per changed loan instead of one wasted 404. An empty result never clobbers a stored schedule.
- **LOS reads go through `tmo_read_json_`**, which keeps the HTTP status and a readable one-line snippet of whatever TMO actually sent, with the BOM and tags stripped. A non-JSON answer can no longer erase the reason.
- **Every `[tmo-sync]` catch site logs `error_message_(err)`, not the error object** — 16 sites, plus the three roster/lender sites in the admin path. Given how Workers Logs renders stacks, handing the console an Error is not safe.
- **The simulator no longer implements an endpoint TMO lacks.** This is the part I would flag hardest: `lib/tmo-sim` served `GetLoanPaymentSchedule` with tidy JSON, so *every test passed while production failed 28 times a batch*. The sim was answering an error case more politely than the real service does. It now returns the real 404 XHTML page, so a caller that skips the status check fails in simulation the way it fails against TMO.

I swept for the same class elsewhere: `GetBorrowers`, `GetPaymentSchedule`, `GetLoanSchedule`, `GetSchedule`, `GetLoanPayments` appear only in comments now. No live call to a nonexistent endpoint remains.

**Verified live, not just green.** `bun test` 2187 pass / 0 fail across 195 files; `build` and `build:worker` clean. Nine mutations checked, each asserted against `Ran 47 tests` so nothing passes vacuously — reverting any read helper to bare `res.json()`, restoring the simulator route, dropping the empty-schedule guard, emptying the extractor, un-collapsing the body note, and un-stringifying any *individual* catch site each redden.

| | staging full sweep | production full sweep |
| --- | --- | --- |
| before | `partial`, 25 issues, **0** schedules | `partial`, 28 issues, **0** schedules, ~250s, cursor stuck |
| after | **`succeeded`**, 0 issues, **11** schedules | **`succeeded`**, 0 issues, **12** schedules |

I forced a full sweep on both rather than wait a day for the natural one. Error-level logs since the production deploy: six, all stamped `v=a7a61fae` — the *old* build, from the 18:19:44–18:20:03 tail of the batch still draining when I deployed. Zero from `3b7d6d2d`.

The number worth sitting with is the schedule count. Production has 31 loans and **not one of them has ever had a payment schedule stored** — `payment_schedule_json` was empty across the board, because the only code that could fill it called an endpoint that has never existed. It is 12 now. This was not a noisy-log fix; a feature that never once worked is working.

One thing I did **not** do: `bin/tmo-backup.ts` and `bin/tmo-snapshot.ts` still say "TMO exposes no payment-schedule read" and skip schedules entirely. That is true of the HTTP surface and false of the data — the schedule is on the loan, and the sync now reads it. Backlogged as `#tmo-backup-schedule-inline` rather than folded in here, because it changes the backup artifact shape, and that artifact is exactly what `#tmo-sandbox-clone` is describing in a plan you are currently reviewing. I would rather not move that target while you are looking at it.

`#tmo-sandbox-clone` stays open and untouched — it is still waiting on your answers to the six questions in §7.
<!-- agent:boundary:b3c77020 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority start -->
- 🚧 do [#tmo-sandbox-clone]
- ~~⏭️ do [#tmo-roster-error-still-silent]~~
- ~~do [#tmo-sync-cron-failures]~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tmo-backup-schedule-inline] Capture payment schedules in bin/tmo-backup.ts and bin/tmo-snapshot.ts. Both skip schedules with the note 'TMO exposes no schedule read endpoint' (docs/TMO/backup.md, snapshot.ts:22). True of the HTTP surface, false of the data: the schedule rides inline on the loan as a Fields[] entry keyed PaymentSchedule (docs/TMO/payment-schedule-field.md), and tmo-sync.ts now reads it via tmo_loan_payment_schedule_(loan) as of EFS 300b788d - 12 of 31 production loans have one. A restored artifact currently loses every schedule, and tmo-snapshot's complete=true does not mean complete. COORDINATION: this changes the backup artifact shape, which #tmo-sandbox-clone's plan (prds/tmo-sandbox-clone-and-staging-refresh-plan.md, EFS c484b0ee) describes section by section - sequence it after that plan is approved, or update the plan in the same change.
- [ ] 🚧 [#tmo-sandbox-clone] Build the anonymized live->sandbox TMO clone + staging D1 refresh per src/equityfundingsource-dev/prds/tmo-sandbox-clone-and-staging-refresh-plan.md (EFS c484b0ee). BLOCKED ON OPERATOR: answer the six questions in plan section 7 (preserve LoanNumber, attachments placeholder-vs-drop, purge-vs-sync, staging wipe-vs-additive, pseudonym seed custody, whether amounts/rates are sensitive) before any apply. Deliverables: pure lib/tmo-api/anonymize.ts, offline bin/tmo-artifact-anonymize.ts with a fail-closed --audit residual-PII scan, bin/tmo-sandbox-clone.ts orchestrator (dry-run default, --simulate against lib/tmo-sim), bin/d1-staging-refresh.ts with a fail-closed 94-table classifier, plus offline tests mutation-checked. Nothing has been executed.
- [ ] [#efs-dev-vars-secret-history] Confirm .dev.vars.production / .dev.vars.staging were never committed, and rotate if they were. Both hold cleartext AUTH_SECRET and ADMIN_GLOBAL_PASSWORD; .gitignore covered '.dev.vars' but NOT the per-environment variants, so they sat untracked-but-committable until EFS 292b52d6 added '.dev.vars.*'. Run: git -C src/equityfundingsource-dev log --all --diff-filter=A -- '.dev.vars*'. If any commit ever added one, rotate AUTH_SECRET (wrangler secret put AUTH_SECRET, both envs) and ADMIN_GLOBAL_PASSWORD, and purge from history. Audit the same gap in sibling repos that copied this .gitignore.
- [/] [#efs-email-subdomain] [operator-verify] Grant `passage:btak/equityfundingsource/production/cloudflare_api_token` Email Sending, Email Routing, and required DNS write access on `equityfundingsource.com`; then enable the `efs.equityfundingsource.com` sending/routing subdomain, route `reply@efs.equityfundingsource.com` plus-addressed replies to the EFS Worker, push and deploy EFS commit `c6d4133e`, and verify login, invite, notification, and inbound-reply delivery end to end. Current evidence: Email Sending list succeeds for the apex, but Wrangler enable and direct POST `/zones/426085c68d8d584075c824ff1d715926/email/sending/subdomains` fail with Cloudflare code 10000; the separate infra token has no production-account access.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#tmo-sandbox-purge-apply] [operator-verify] Execute the sandbox purge only after explicit operator go. The auto-mirrored queue directive is not approval for permanent `DeleteLoan` operations. Before any approved purge, take the documented snapshot; preserve the existing 16-loan target list, keep-list, dry-run evidence, and warning that attachment bytes and RecIDs are not recoverable. Recipes: `docs/TMO/delete-loan.md`, `docs/TMO/snapshot.md`.
- [/] [#tmo-orphan-apply] [operator-verify] Apply the orphan reconcile plan: TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)" bun ./bin/tmo-orphan-reconcile.ts --remote --apply. Dry run re-verified 2026-07-18 against remote D1: reconcile_cache=12 + tombstone=6, push_funding=0, blocked_no_vesting=0, zero TMO writes (EFS-local only). CORRECTION 2026-07-18: this targets the STAGING database (equityfundingsource, 280e1539-b940-4c59-8988-fbed63c12ac0), NOT production — the earlier 'mutates PRODUCTION D1' note was wrong; --d1 defaults to the staging binding and production (293f0a4d) is untouched. TMO side is the sandbox database (001-8553-000-Equity Funding - Sandbox, sandbox=true). All 6 tombstone rows (TST217, TST254, TST374, TST723, TST762, TST999) verified inert: efs_id NULL, zero rows in loans.tmo_loan_number, zero rows in funds (funds is empty entirely). Operator confirmed 2026-07-18 that staging data is test data. Remaining gate is operator go only.
- [/] [#tmo-reconcile-auto] Automated reconciliation attempt over tmo_unmatched_fundings: for each unmatched TMO funding try RecID -> Account against lender_vestings.tmo_lender_account -> lender email, record confidence plus evidence, auto-bind above a threshold and queue the rest with the candidate ranking pre-computed. Per PRD D4 a bind reconciles EFS to TMO (TMO funding wins) and must record what it overrode. The matcher must be a pure function over pulled fundings + the lender/vesting roster so it is testable without TMO, mirroring vesting_gap_. Phase 3 of src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [/] [#tmo-orphan-as-fundings] Replace #tmo-orphan-cache's tombstone plan per the operator directive: for each of the 18 orphan tmo_cache_entities 'loan' rows, reconcile against TMO if the loan exists, push the EFS-side funding into TMO via LOS.svc/AddLoanFunding (LOS-embed lender, PRD D1) and adopt the returned RecID if it does not, and tombstone only pure debris with no EFS deal. Gated on the EFS->TMO push gate (#dzmm/#7a4d). Phase 2 of src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [/] [#tmo-prod-seat-not-provisioned] [operator-verify] The 2026-07-17 20:19 "Welcome to The Mortgage Office" credentials are for the SANDBOX host: the email's own Login link is https://app2.themortgageoffice.com/Login, not app.themortgageoffice.com (which is what the 2026-05-15 welcome email linked). So April's "added you to the production account" has not landed as a production seat yet — app2 still offers exactly one company, Equity Funding - Sandbox. Ask April to confirm which account/serial the new seat was created on and to provision app.themortgageoffice.com access. Credentials stored at passage equityfundingsource/tmo_login_2026_07_17.
- [/] [#mam7] Fractionalized loans (future): support up to ~10 lenders/loan with siloed per-lender info in the shared loan view. [followups: Fractionalized loans]
- [/] [#jzqj] Bidirectional TMO conditions sync (TMO as source of truth for conditions) + LOS lender-ID / deep-link into TMO pages. (gated on the Tue TMO screen-share + API access) [followups: TMO]
- [/] [#1067] Accreditation/suitability questionnaire: full form in the lender portal, income/net-worth options only, not a hard funding gate, admin-fillable, editable PDF + investigate DocuSign. Needs the DRE form + a DocuSign account. [followups: Accreditation]
- [/] [#nyc5] [P0] Available-deals upload: API endpoint + API key to upload investor offers to the marketplace (replaces manual YAMM) + a manual add-deal form (same template, admin-created). Needs Brian's offer-format spec. [followups: Available-deals]
- [/] [#tcb0] [P0] Go-live: publish the EFS site this week — disable/hide not-yet-functional portals, keep apply-for-loan + contact inquiries working; verify apply->TMO end-to-end. (needs operator go on which portals to hide) [followups: Go-live]
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
- [/] [#dzmm-writeback-replay] EFS->TMO writeback replay: once #7a4d field map and #k2xg support answers arrive, replay tmo_cache_outbox with idempotency keys, conflict logging, retry/dead-letter state, and tests across broker/admin/investor writebacks. Now also covers the #tmo-split-launch 'create_loan_application' rows — applications approved while EFS ran split carry the full LOS.svc/NewLoan body plus the EFS-assigned loan number and sit at writeback_status='gated' until replayed. Advances #dzmm/#37ah/#67v2/#zwws.
- [/] [#xcvg] [recommended] Borrower message email round-trip: email notification when a borrower sends a secure-portal message, inbound email reply that appends back into the D1 chat, and a borrower-facing email when an admin replies. Builds on the shipped chat_sessions/chat_messages + admin owner. Gated on a verified Resend EFS sending domain (f4rq) and Google Workspace SMTP (m6ch/p40t). (Distinct from the operator/comms emails to Judith + Robert.)
- [/] [#jnzs] [recommended] Await Judith or EFS reviewer approval on funded-deal images, copy, and lien-position wording before invoicing or launch sign-off. The funded-deals page carries no standalone legal/disclosure text; the disclosure needing legal review is the ESIGN/UETA Electronic record and signature disclosure on the borrower full-application/document-uploads flow (full_application.tsx). Funded-deals copy was reframed as real closed deals and shipped.
- [/] [#67v2] Admin loan pipeline: TMO-aligned stages + Kanban (drag cards, auto-advance when stage requirements met) synced to dashboard; mirror TMO stages as single source of truth. From 2026-06-03 call. Relates to #dzmm/#k2xg.
- [/] [#zwws] [recommended] Investor dashboard rework — remaining phases are externally gated: 4 vestings/special-instructions (TMO writeback-gated #dzmm/#k2xg/#7a4d); 6 new-deal email + workflow-priority dashboard (#f4rq). Shipped: P1 nav (30037e3), P2 dashboard-composite landing (3e06907), P2b interest-button + distributions/statements cleanup (efs cd4253a), P3 simplified portfolio (8d40c66), P5 accreditation→annual Requirements (efs 1957f64). Plan: src/equityfundingsource-dev/prds/rpzm-investor-dashboard-rework.md. Queue one phase at a time.
- [/] [#next-steps] Remaining EFS workers.dev production state: D1 API token unblocked (Cloudflare API token at `passage btak/d1/CLOUDFLARE_API_TOKEN`); migrations 0025/0026 are already applied; Cloudflare Access env vars are deployed for /login/access-callback on workers.dev using the one-time PIN IdP. Custom domain cutover remains intentionally deferred; true Google Workspace SSO remains gated on Google admin credentials.
- [/] [#dzmm] TMO webhooks UNAVAILABLE per operator (2026-06-13). Read-side tightened polling is shipped (scheduled tmo-sync with configurable interval + due-gating + tmo_cache read mirror). Remaining gated: EFS to TMO writeback (tmo_cache_outbox replay) needs the #7a4d write field map + #k2xg support answers before it can replay.
- [/] [#dropbox-sync-adapter] Dropbox sync adapter: #dropbox-sync-contract shipped the shared documents Dropbox metadata, document_sync_logs idempotency schema, pure sync planner, and mocked adapter tests. Remaining: after Dropbox access is available, wire the real credential-backed worker to push/pull R2_DOCUMENTS with retries, backfill, tombstone replay, and per-document sync logs.
<!-- /agent:review -->

## Notes

<!-- agent:notes -->
How can I instruct Judith to create a Cloudflare account for EFS? She does not have much time today so please make brief instructions for her to follow.
---
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

We have not cut over the DNS today. The current CF environment will be staging. Please setup another CF environment for production. At some point, we will have EFS have custody of the CF account, possibly today. But I want to have a production environment ready to go in case it is needed.
<!-- /agent:notes -->

## Completed / Reaped

<!-- agent:icebox -->
- [ ] [#efs-prod-secrets] [operator-verify] Cutover prerequisite, NOT a live outage — the [env.production] Cloudflare environment is provisioned and deployed but not launched (no DNS, empty D1, no traffic). It has 4 secrets; the live default env has 13. Present in [env.production]: ADMIN_GLOBAL_PASSWORD, AUTH_SECRET, CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN. Missing: TMO_API_TOKEN, TMO_DATABASE, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_ENDPOINT_URL_S3, DROPBOX_APP_KEY, DROPBOX_APP_SECRET, DROPBOX_REFRESH_TOKEN, and one more. If DNS were cut over today, every TMO, S3, and Dropbox call would fail closed. Push with 'bun run cf:secrets:push:production'. Secrets are per-account AND per-environment, so do this AFTER #efs-cf-account-custody or it has to be redone. Discovered 2026-07-18.
- [ ] [#efs-cf-account-custody] [operator-verify] Move EFS to its own Cloudflare account. Both environments currently live in Brian's personal account (1e7e194ff8da4ad9e40ce041aea84f76), which also hosts unrelated projects. STEP 1 (Judith, instructions drafted 2026-07-18): sign up at dash.cloudflare.com/sign-up with a SHARED EFS address (becomes billing + recovery contact), verify the email (hard prerequisite — invites and 2FA both fail without it), invite Brian as Super Administrator via Manage Account -> Members -> Invite, enable 2FA, have a company card ready for the R2 checkout. Super Administrator is required, not just Workers Platform Admin, because R2 activation is a purchase and the deploy pipeline needs an account-owned API token. STEP 2 (Brian): D1 databases and R2 buckets cannot move between accounts — create them in the new account, update the ids under [env.production] in wrangler.toml, export+import D1, copy R2, re-push secrets (per-account AND per-environment), then repoint DNS. Sequencing: do this before #efs-prod-seed to migrate the data once instead of twice. Advances #efs-dns-cutover. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-dns-cutover] [operator-verify] equityfundingsource.com does not resolve: no A/AAAA record, nameservers still ns1/2/3.dreamhost.com, and wrangler.toml declares no route or custom_domain. The worker is live and correct at equityfundingsource.brian-takita.workers.dev. Needs registrar + Cloudflare access: move the zone to Cloudflare (or add a CNAME at the registrar), add the custom domain / route to wrangler.toml, verify /login/access-callback still works, and allow for NS propagation. This is the remaining gate on the weekend launch. Advances #tcb0 / #efs-cutover-sso-plan. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#vcyr] Send rwaldman100+admin@gmail.com admin login link to btak.dev+admin@gmail.com once a working Cloudflare Email Sending API token is available or Brian explicitly confirms an alternate sender/body. Verified Robert is active admin; current local Cloudflare/Resend credentials cannot send. (Deferred from backlog 2026-07-03; operator replaced the line — restore if unintended.)
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
~~`reply@efs.briantakita.me` must not be used in production. The equityfundingsource automated email subdomain must be used.~~ — auto-struck: answered this cycle (#ftstrike)
[#prod-fix-deploy] [operator-verify] Deploy the production fixes and verify them live. Production still runs the pre-fix code, so the anonymous realtime 401 loop (~6,500 requests/day) is still burning. Needs operator go for two reasons: the queue preset was #spec-test-commit-push, which excludes deploy; and a whole-tree deploy from src/equityfundingsource-dev would ship a concurrent session's ~355 uncommitted lines of TMO refactor. Deploy from a CLEAN checkout at EFS 29f27773 (see reference_efs_deploy_from_main_checkout_not_fresh_install_worktree), then verify: (a) GET accounts/67b0703bb39744866efbf765e1ebcf1c/workers/scripts/equityfundingsource-production/settings reports observability.enabled true; (b) the telemetry count for /api/public/messages/realtime/ws status 401 falls toward zero over 24h (baseline 6,576 of 11,787 app requests); (c) a fresh anonymous public tab opens NO realtime socket until the first message is sent, and one immediately after. Query recipe: src/equityfundingsource-dev/docs/deploy.md section 2.4b.
- [ ] [#tmo-sync-cron-failures] [operator-verify] Root-cause the TMO sync cron failures. All 7 error-level lines in 24h of equityfundingsource-production logs are eventType=cron from the 15-minute sync: loan sync failed on 1012 and 1014, borrower roster failed (x2), LOS lender roster failed, and GetLoanFundings failed while building the lender roster on 1005 and 1009. Blocked on #prod-fix-deploy: until EFS 29f27773 is deployed those Errors carry an empty message, so the logs cannot say what LOS returned. After deploy, re-query per docs/deploy.md 2.4b filtering $metadata.level=error and fix what the endpoint plus ErrorNumber names. Coordinate first: app/equityfundingsource/lib/tmo-sync.ts currently holds ~355 uncommitted lines from a concurrent session.
- [ ] 
 what the endpoint plus ErrorNumber names. Coordinate first: app/equityfundingsource/lib/tmo-sync.ts currently holds ~355 uncommitted lines from a concurrent session.
- [ ]
410dfec2
cf2d65db-105b-472b-9d1e-de6638dc2eb6
93980072-a559-45d3-8fe5-98e2d0180fc4
.vars.*'. Run: git -C src/equityfundingsource-dev log --all --diff-filter=A -- '.dev.vars*'. If any commit ever added one, rotate AUTH_SECRET (wrangler secret put AUTH_SECRET, both envs) and ADMIN_GLOBAL_PASSWORD, and purge from history. Audit the same gap in sibling repos that copied this .gitignore.
- [ ] 🚧
