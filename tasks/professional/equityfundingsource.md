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
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
queue: go
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260718-204742.md`*

Compacted content:
- Archived 7 response topic(s): Dedicated URL for Reconcile with TMO; What TMO API endpoint the dropdown acts on; Jeff Blake and Lucas Huang; 4 more
- Prior summary/context: prior compacted content: Archived 4 response topic(s): Production Cloudflare environment; Brief Cloudflare account in...

### Re: do [#tmo-sandbox-orphan-loans] — opus-4-8

Built the teardown path and made the purge decision. Held the actual deletion — see the question at the end.

**Live sandbox state (LOS.svc/GetLoans, 2026-07-18):** 18 loans.

- `1010` — created 5/19/2026, `Unassigned`, $0, no borrower. TMO-seeded demo loan, predates the EFS integration. **Keep**, permanently — it is in `SANDBOX_PROTECTED_LOAN_NUMBERS`.
- The other 17 are EFS test debris created 6/6–7/1/2026 with synthetic borrowers (`Brian Takita`, `Test2 Borrower`, `Tester4 Borrower`, `Borrower 11`, `James Blake`, …).
- `GQI751` — **pinned, not purged**. It is the only live repro fixture for `#dealtab500` (loan-tab `Internal Error`) and `#5q7z` (duplicate-loan writeback on approve). Deleting it would destroy the evidence both review items depend on. Purge it with `--apply --only GQI751` once those close.
- The remaining **16 are purgeable**.

**Teardown path shipped** (EFS `a56b1b8b`, pushed):

- `app/equityfundingsource/lib/tmo-api/sandbox_purge.ts` — pure, keep-list-driven planner. No network, so the destructive decision is unit-testable.
- `bin/tmo-sandbox-purge.ts` — CLI over `LOS.svc/DeleteLoan`.
- `docs/TMO/delete-loan.md` — endpoint reference (it was missing from `docs/TMO/`), teardown recipe, and the purge decision above.
- `tests/tmo-sandbox-purge.test.ts` — 14 tests. Full suite green: 1801 pass / 0 fail; `build` + `build:worker` OK.

Safety properties, all enforced in code rather than by the caller's flags:

- Dry run is the default; deletion needs `--apply`.
- `--apply` is refused unless the `Database` header contains `Sandbox`. This matters more than it looks: **the TMO token is identical for the sandbox and live databases** — the environment is selected *only* by the `Database` header — so this guard is the one thing standing between a teardown pass and production.
- Protected and pinned loans are enforced by the planner. `--only` can narrow the purge set but can never select `1010` or `GQI751`.
- The token is read from `TMO_API_TOKEN`; nothing is committed or echoed.

Dry run against the live sandbox: `purge=16 keep=2`, matching the decision above.

Two things worth knowing about `DeleteLoan`, both now in the doc:

- It is keyed on `LoanNumber`, unlike `DeleteBorrower` which needs the borrower `RecId`.
- Deleting a loan does **not** delete its borrowers, and does **not** touch EFS state. The 18 orphan `tmo_cache_entities` rows are a separate problem (`#tmo-orphan-cache`) — several of those cached loan numbers (TST042, TST217, TST254, TST374, TST723, TST762, TST999) do not exist in TMO at all, so purging TMO will not clear them.

**Question before I run it:** `--apply` permanently deletes those 16 loans with no TMO-side undo, and your note says you are in TMO tonight after 20:30 for the sync work. Do you want them gone now, or would you rather look at the sandbox first? Queued as `#tmo-sandbox-purge-apply`; one command when you say go.

On the other queue head — `#tmo-prod-seat-not-provisioned` is operator-gated (April has to confirm which account/serial the new seat landed on and provision `app.themortgageoffice.com`). Nothing I can drain there.

### Re: vestings, fundings, and the reconciliation process — opus-4-8

> **Queue prompt:**
>
> do [#tmo-prod-seat-not-provisioned]: I was not able to login to the live system. I don't need to though. Just need api access and I believe you confirmed the live key is working.


Agreed on the reframe, and I think it is the right call for a reason worth stating: tombstoning the orphan cache rows throws away the only realistic reconciliation corpus EFS has. Pushing them into TMO as fundings turns dead rows into a live exercise of the path that actually has to work at launch.

Wrote the whole thing up as `prds/vesting-and-tmo-reconciliation.md` and shipped Phase 1 (EFS `18f06c17`, pushed).

**Why vestings are the bottleneck.** TMO models one lender *account per vesting* (Mark Leonard = MARKDBPP / MARKL / MARKPERSON / MARKROTHIR); EFS models one `users` row with many `lender_vestings`. A TMO funding can only reach an EFS lender through `lender_vestings.tmo_lender_account`. So a lender with **no vesting** is unreconcilable, and a lender whose vestings carry **no TMO account** is equally unreconcilable. Both were completely silent — the lender saw a passive empty state, the admin saw nothing at all.

**Phase 1, shipped:**

- **`vesting_gap.ts`** — one pure derivation of the gap (`no_vestings` / `unlinked` / `partial_link` / `none`) that every surface renders from, so the lender prompt and the admin warning cannot disagree.
- **`vesting_input.ts`** — one shared validator. The lender portal and the admin card now write vestings through the same code; previously only the portal path existed, and a second one would have drifted immediately.
- **`#admin-add-vesting`** — `POST` / `PATCH` / `DELETE /api/admin/lenders/:id/vestings`, plus add / edit / remove in the admin Lender Vestings card. This was the real gap: most vestings arrive by phone or email during onboarding, and there was no way to record one without the lender logging in and filling out their portal profile. `DELETE` archives rather than hard-deletes, since a vesting may already carry a TMO mapping or back a funding.
- **`#vesting-gap-banner`** — lender prompt on the portal **dashboard** (next to the requirements alert, so it is seen without navigating to Vestings) and on the Vestings page; admin warning on the Lender Vestings card, red when it blocks reconciliation, amber when it is partial.

One deliberate asymmetry: the lender surfaces never mention TMO account linking. That is back-office state a lender cannot act on, so `vesting_gap_` returns lender-facing copy only for `no_vestings`. There is a test pinning that the lender components never render `admin_message`.

24 new tests; full suite 1826 pass / 0 fail; `build` + `build:worker` clean.

**Queued the rest as three phases** rather than one parent item:

- `#tmo-orphan-as-fundings` — per-row decision over the 18 orphans: reconcile if the loan exists in TMO, `AddLoanFunding` + adopt the RecID if it does not, tombstone only pure debris with no EFS deal. Gated on the EFS→TMO push gate (`#dzmm`/`#7a4d`) — that gate is real, not me deferring.
- `#tmo-reconcile-auto` — the automated attempt you asked for, over `tmo_unmatched_fundings`: RecID → `Account` → email, with a confidence score and recorded evidence, auto-binding above a threshold and queueing the rest with candidates pre-ranked. Per the existing PRD D4 a bind reconciles EFS *to* TMO (the TMO funding wins) and records what it overrode. The matcher will be pure, like `vesting_gap_`, so it is testable without TMO.
- `#vesting-gap-admin-queue` — Phase 1 warns per lender on that lender's card; an admin still has to visit each one. A roster-level queue closes that, and both counts it needs (`investor_vesting_count`, `investor_vesting_linked_count`) are already in the lender-pipeline query.

Also moved `#tmo-prod-seat-not-provisioned` to review — it is blocked on April confirming which account/serial the new seat landed on, which is not something I can drain, and it was holding the queue head.
### Re: TMO live API access — opus-4-8

> **Queue prompt:** Instead of doing [#tmo-orphan-cache], the loans should be added as fundings into the tmo database. The admin may need to reconcile the funder/lender/vesting. We can use this to drive out the reconciliation process. We should also have an automated tool to attempt a reconciliation. We also should do work on lender portal vestings, with a banner to ask the lender to add their vestings. Also the admin should be able to add lender vestings. A lender may not have any vestings but we should have admin banners to ensure vestings are added.

Answered in full in the previous response — `prds/vesting-and-tmo-reconciliation.md`, Phase 1 shipped as EFS `18f06c17`, Phases 2–4 queued as `#tmo-orphan-as-fundings` / `#tmo-reconcile-auto` / `#vesting-gap-admin-queue`. This entry is the queue echo so the head clears.

On your note about the seat — you are right that the portal login is not the blocker, and I want to correct one thing I said. **The TMO token is not environment-scoped at all.** There is one token, it is byte-identical in `passage equityfundingsource/tmo/live/api` and `equityfundingsource/tmo/sandbox/api`, and the environment is selected *only* by the `Database` header. So API access to production was never gated on the seat; it was already there.

That is worth sitting with, because it cuts both ways: nothing but a header string separates a sandbox call from a production one. It is exactly why `bin/tmo-sandbox-purge.ts` refuses `--apply` unless the `Database` value contains `Sandbox` — a fat-fingered header on a delete pass would otherwise hit live records with no undo.

I re-gated `#tmo-prod-seat-not-provisioned` to review rather than closing it. The portal seat still matters for anything you or April need to do by hand in the TMO UI on the production company, and it was blocking the queue head where it was.

### Re: do [#tmo-orphan-as-fundings] — opus-4-8

> **Queue prompt:**
>
> do [#tmo-orphan-as-fundings]


Planner shipped (EFS `9908ed1c`, pushed). `lib/tmo-api/orphan_reconcile.ts` takes the three sides — the cached rows, what TMO actually returns from `GetLoans`/`GetLoanFundings`, and what EFS has behind each row — and emits one action per row with its evidence:

| Action | When |
| --- | --- |
| `reconcile_cache` | TMO has the loan (stale row, not an orphan), or TMO already has the funding |
| `push_funding` | TMO has the loan, EFS has an unfunded vesting-linked funding → `AddLoanFunding` |
| `create_loan_then_funding` | TMO lacks the loan but EFS has a fundable deal → `NewLoan`, **then** `AddLoanFunding` |
| `blocked_no_vesting` | EFS has a fundable deal but the lender has no linked vesting |
| `tombstone` | Nothing behind the row on either side |

Three things I want to flag, because they changed the shape of the thing:

**`AddLoanFunding` cannot create a loan.** Your directive said add the loans as fundings, but a funding attaches to an existing TMO loan. For the 17 rows TMO does not have, it is `NewLoan` then `AddLoanFunding` — two calls, and the plan says so per row rather than failing at execution time.

**The planner refuses to push a funding with no TMO lender account.** That is Phase 1's vesting gap arriving on the reconciliation side. Pushing anyway would put an unattributable funding into TMO — worse than the orphan row we started with, because now it is TMO's problem too. So the reconciliation run *surfaces* vesting gaps as `blocked_no_vesting` rather than papering over them. That is the loop you were after: the orphan corpus drives out exactly the reconciliation work that has to exist.

**It never duplicates a funding TMO already has**, checked both by `tmo_funding_rec_id` and by `GetLoanFundings` — the same class of bug as `#5q7z` (approve re-sending `NewLoan` for a loan already in TMO and getting `Duplicate loan number`).

12 tests; full suite 1838 pass / 0 fail; builds clean.

Gated the parent and queued `#tmo-orphan-reconcile-exec` for the executor. To be precise about what is actually blocked: `reconcile_cache` and `tombstone` are EFS-local and not gated at all, and **sandbox execution of the writes is not gated either** — only production writes ride `#dzmm`/`#7a4d`. So the next cycle can run this end-to-end in the sandbox against the clean slate `bin/tmo-sandbox-purge.ts` leaves, which is the right place to prove it before anything touches live.
> **Queue prompt:**
>
> do [#tmo-orphan-as-fundings]



### Re: do [#tmo-reconcile-auto] — opus-4-8

Matcher shipped (EFS `9412be52`, pushed). `lib/tmo-api/funding_match.ts` scores each `tmo_unmatched_fundings` row against the lender/vesting roster and returns `auto_bind` / `review` / `ambiguous` / `no_match` with ranked candidates, a confidence, and the evidence behind each one.

| Signal | Weight |
| --- | --- |
| `tmo_funding_rec_id` (EFS already stamped this funding) | 1.0 |
| `vesting_account` (`Account` == `lender_vestings.tmo_lender_account`) | 1.0 |
| `lender_account` (`Account` == `users.lender_account`, legacy single-valued) | 0.9 |
| `email` | 0.6 |
| `full_name` | 0.25 |

The scoring rule is the part worth arguing about, so here is the reasoning:

**Only an exact identifier can bind.** Corroborating signals add 10% of their weight rather than summing, so two weak signals can never reach an exact-identifier score — email + name still lands in `review`. I held email below the 0.9 threshold on purpose: lenders share household addresses, and EFS has already seen `+alias` test accounts collide. An email match is strong evidence for a human and insufficient evidence for a machine.

**A tie at the top returns `ambiguous`, never a bind.** Two lenders scoring identically means a coin flip on real money. The admin gets both, ranked, rather than a 50% chance of a silently wrong attribution that later has to be unwound in TMO.

That leaves auto-binding to the genuinely mechanical cases — the ones where TMO's `Account` *is* a vesting you already mapped — which is the whole point: Phase 1 made vestings mappable, and this converts each mapping into binds that stop reaching the admin queue at all.

18 tests, including the household-email and duplicate-vesting collisions; full suite 1856 pass / 0 fail; builds clean.

Gated the parent and queued `#tmo-reconcile-binder` for the wiring: read the roster, run `funding_match_all_`, apply `auto_bind` through the existing `admin_tmo_unmatched_funding_bind_POST_` path, and persist the `review`/`ambiguous` rankings so the admin queue opens pre-ranked instead of blank.

One note on this cycle: the closeout hit an editor-authority/disk divergence after the `#tmo-orphan-as-fundings` commit and again here. Both settled through the supervisor's automatic replica re-registration — no forced write, no repair, nothing lost — but it is why the queue advanced a beat ahead of the responses.
### Re: do [#vesting-gap-admin-queue] — opus-4-8

Shipped (EFS `e4cfca4d`, pushed). Phase 4 closes the loop Phase 1 opened.

- **`GET /api/admin/lender-vesting-gaps`** — every active lender who cannot be reconciled against TMO, worst gap first, with `blocked_count` (cannot be reconciled at all) separated from partial-link warnings. Lenders with nothing to fix are dropped; a fully-linked lender is not a queue item.
- **`vesting-gaps` nav badge** in `/api/admin/nav-counts` — active lenders with zero TMO-linked vestings.

The design choice worth naming: the endpoint calls the same `vesting_gap_` derivation the per-lender card renders rather than re-deriving the gap in SQL. Two implementations of "is this lender blocked" would drift the first time the rule changed, and the failure mode is quiet — a queue that disagrees with the card it links to. The badge query and the roster filter agree by construction too: `no_vestings` and `unlinked` both have zero linked vestings and both set `blocks_reconcile`; `partial_link` has neither, so it stays a warning rather than inflating the badge.

8 tests; full suite 1864 pass / 0 fail; builds clean.

Queued `#vesting-gap-queue-ui` for the screen itself — the API and count ship, the portal worklist is the follow-up.

**Where the four phases leave things.** The reconciliation path now exists end to end as decision logic: vestings are authorable from both sides and their absence is visible (Phase 1), the orphan corpus has a per-row plan (Phase 2), unmatched fundings score themselves against the roster (Phase 3), and the gaps that block all of it are a worklist (Phase 4). What is *not* built is execution — three items, all wiring rather than design: `#tmo-orphan-reconcile-exec`, `#tmo-reconcile-binder`, `#vesting-gap-queue-ui`. Worth doing in the sandbox first, in that order, since the executor is what generates the unmatched fundings the binder then has to resolve.

### Re: do [#vesting-gap-queue-ui] — opus-4-8

> **Queue prompt:**
>
> do [#vesting-gap-queue-ui]


Shipped (EFS `073e466e`, pushed). **Vesting Gaps** is now a left-nav section at `/admin/vesting-gaps`: blocked/warning counts up top, one row per lender with the gap kind, and a per-row deep link into that lender's Lender Vestings card — where the gap is actually fixable, rather than making an admin navigate there themselves.

Each row renders the API's `admin_message` rather than restating the rule in the UI. That keeps the property the whole feature rests on: the lender prompt, the admin card, and this queue all trace back to a single `vesting_gap_` call, so none of them can start disagreeing about who is blocked.

Wired end to end — section id, URL, portal route, nav item, icon, and the badge. The badge is worth a note: it binds by name (`counts[item.id]`), so a nav id that does not match the `nav-counts` key renders nothing at all, silently. There is a test pinning that they match, because a missing badge is exactly the kind of bug nobody files.

4 tests; full suite 1868 pass / 0 fail; builds clean.

**All four phases plus both UI surfaces are done.** What is left is three execution items, and only one of them is genuinely gated:

- `#tmo-orphan-reconcile-exec` — run the Phase 2 planner for real. **Not blocked in the sandbox**; only production writes ride `#dzmm`/`#7a4d`.
- `#tmo-reconcile-binder` — apply Phase 3's `auto_bind` decisions and persist the rankings. Not blocked.
- `#tmo-sandbox-purge-apply` — waiting on you, since it is 16 permanent deletions.

The order matters: the executor generates the fundings the binder then resolves, so running them out of order means the binder has nothing to work on.

### Re: do [#tmo-contact-enumeration] — opus-4-8

Re-probed the premises before building anything, and two of the three were stale. The item said TMO exposes no contact enumeration at all. Against the live sandbox today:

| Endpoint | | |
| --- | --- | --- |
| `LOS.svc/GetBorrowers` | 404 | still true — no direct borrower endpoint |
| `LSS.svc/GetBorrowers` | 404 | still true |
| `LSS.svc/GetLenders` | **200** | **returns data** — it was empty when the item was filed |
| `LOS.svc/GetLoan/:n` | **200** | **carries `Borrowers[]`, each with a `RecID`** |

That last row is the one that matters. `Borrowers[].RecID` is exactly what `DELETE /LOS.svc/DeleteBorrower/:borrowerRecId` takes. So the planned workaround — "wire an operator-supplied-RecId delete path rather than guessing record ids" — is unnecessary. Borrower enumeration is available, just indirectly: walk `GetLoans`, fetch each loan, collect its borrowers.

Built that instead (EFS `a990618d`, pushed):

- **`contact_roster.ts`** — pure aggregation across `GetLoan` payloads into a roster deduped on `RecID`. Dedup is the point: a borrower on four loans would otherwise read as four contacts, and an admin could not tell which RecId to delete.
- **`GET /api/admin/tmo-contacts`** — both rosters, fetched independently so a failing LSS read does not lose the borrower roster; only 502s when both fail.
- **Bounded**: the walk is O(loans) round trips, so it is capped (default 200, max 500) and belongs behind an explicit admin action, never a page render.

**Verified end to end against the sandbox: 18 loans → 16 distinct borrowers, 1 lender.** Not a mocked result.

One thing I found while building it that was not in the item. There are two orphan shapes and they are inverses:

- **Orphan borrower** — on no loan. Safe to delete; removing one cannot orphan a loan.
- **Borrowerless loan** — a loan with *no borrower*. This is the one that actually bit us: TST113 became one when Lucas Huang was deleted, the loan stayed, and nothing surfaced it. It cannot be serviced or contacted against, yet TMO considers it valid. Now flagged as `borrowerless_loans`.

Also had to reconcile a guard: `#tmo-getlender-confirm` had a test forbidding `LSS.svc/GetLenders` outright, from when it returned nothing and nothing called it. I narrowed it to what it actually protects — the no-LSS-*write* rule — rather than deleting it, since `GetLenders` is a read. Flagging that explicitly because loosening an existing guard deserves your eyes.

19 tests; full suite 1888 pass / 0 fail; builds clean.

**What is left for ABS support** shrank to one question, and it is efficiency rather than a blocker: is there a direct contact-enumeration endpoint, so the roster is not O(loans)? That still rides `#k2xg`. The delete path no longer waits on anyone.

Queued `#tmo-contacts-ui` for the screen.
<!-- agent:boundary:8638d80b -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- ~~do [#vesting-gap-queue-ui]~~
- ~~do [#tmo-reconcile-auto]~~
- do [#vesting-gap-admin-queue]
- ~~do [#tmo-contact-enumeration]~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tmo-reconcile-binder] Binder for the #tmo-reconcile-auto matcher: read tmo_unmatched_fundings + the lender/vesting roster (users.lender_account, lender_vestings.tmo_lender_account, funds.tmo_funding_rec_id), run funding_match_all_, apply auto_bind decisions through the existing admin_tmo_unmatched_funding_bind_POST_ path, and persist review/ambiguous candidate rankings so the admin queue opens pre-ranked instead of blank. Per PRD D4 the bind reconciles EFS to TMO (the TMO funding wins on conflict) and must record what it overrode. Plan: src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [ ] [#tmo-orphan-reconcile-exec] Executor for the #tmo-orphan-as-fundings planner: read the orphan tmo_cache_entities rows + LOS.svc/GetLoans + GetLoanFundings + the EFS funds/vesting roster, run orphan_reconcile_plan_, and apply it. reconcile_cache and tombstone are EFS-local and unguarded; push_funding and create_loan_then_funding (isolated by orphan_plan_writes_) need explicit opt-in and, in production, the EFS->TMO push gate (#dzmm/#7a4d). Sandbox execution is NOT gated — run it there first against the clean slate bin/tmo-sandbox-purge.ts leaves. Mirror the sandbox guard from bin/tmo-sandbox-purge.ts: the TMO token is identical across environments and only the Database header selects one. Plan: src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [ ] [#tmo-orphan-cache] Prune/reconcile orphan tmo_cache_entities rows: 18 of 19 cached TMO 'loan' rows (TST042/TST113/TST217/TST254/TST374/TST388/TST723/TST762/TST766/TST999, AVW580, FTM631, GQI751, ICL909, MBQ435, PIP038, SRV532, 554466) do not exist in the TMO database EFS is pointed at — GetLoans returns 24 loans and only '1010' overlaps. They are EFS test debris that renders as if it were TMO data (this is where 'Lucas Huang' shows up). Add an admin prune action or a sync tombstone pass so a cached row whose source_id is absent from GetLoans is marked deleted instead of shown as a live TMO record. Note admin_tmo_loans_by_loan_number_DELETE_ does NOT cover this — it only disables EFS 'loans' rows and never touches tmo_cache_entities.
- [ ] [#efs-do-hibernation] Switch the three Durable Object room classes to the WebSocket Hibernation API. BorrowerChatRoom, AdminEventsRoom, and PortalMessagesRoom all call the non-hibernating server.accept() with no idle alarm (app/equityfundingsource/lib/messages/realtime.ts:35, lib/borrower/messages/realtime.ts:76, lib/admin/events/realtime.ts:50), so an open browser tab pins its DO active and accrues GB-s duration billing whether or not anyone is typing — cost scales with concurrent open tabs instead of business volume. A single always-on room is ~328,500 GB-s of the 400,000 GB-s included in Workers Paid (~82%); ten rooms open 8h/day x 22 days is ~792,000 GB-s (~+$4.90/mo). Fix: state.acceptWebSocket() plus webSocketMessage/webSocketClose handlers; Cloudflare does not bill duration while a hibernatable socket is idle. Cost detail: src/equityfundingsource-dev/docs/deploy.md 'C — What this costs'. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-prod-seed] [operator-verify] Decide whether the production Cloudflare environment should be seeded from staging before launch. Production D1 (equityfundingsource-production) is schema-complete and empty; staging holds 33 users / 7 loans / 7 deals. If that is real business data it must be exported+imported once, immediately before cutover (a second import collides on primary keys), and R2_DOCUMENTS must be copied separately or uploaded documents 404 in production despite their D1 rows existing. Recipe: src/equityfundingsource-dev/docs/deploy.md 'Seeding production from staging'. Blocked on the operator confirming real-vs-test data. See #efs-cf-account-custody — if custody moves first, seed once after the move. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-cf-account-custody] [operator-verify] Move EFS to its own Cloudflare account. Both environments currently live in Brian's personal account (1e7e194ff8da4ad9e40ce041aea84f76), which also hosts unrelated projects. STEP 1 (Judith, instructions drafted 2026-07-18): sign up at dash.cloudflare.com/sign-up with a SHARED EFS address (becomes billing + recovery contact), verify the email (hard prerequisite — invites and 2FA both fail without it), invite Brian as Super Administrator via Manage Account -> Members -> Invite, enable 2FA, have a company card ready for the R2 checkout. Super Administrator is required, not just Workers Platform Admin, because R2 activation is a purchase and the deploy pipeline needs an account-owned API token. STEP 2 (Brian): D1 databases and R2 buckets cannot move between accounts — create them in the new account, update the ids under [env.production] in wrangler.toml, export+import D1, copy R2, re-push secrets (per-account AND per-environment), then repoint DNS. Sequencing: do this before #efs-prod-seed to migrate the data once instead of twice. Advances #efs-dns-cutover. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-prod-secret-parity] Production is missing secrets staging has and .dev.vars cannot supply (values are only in Cloudflare and cannot be read back out): DROPBOX_APP_KEY / DROPBOX_APP_SECRET / DROPBOX_REFRESH_TOKEN gate document sync. AUTH_SECRET, ADMIN_GLOBAL_PASSWORD, CLOUDFLARE_API_TOKEN, and CLOUDFLARE_ACCOUNT_ID are already set (2026-07-18); TMO_API_TOKEN / TMO_DATABASE are intentionally unset so production runs split (#tmo-split-launch). AWS_*/S3_BUCKET_NAME are local bin/assets-*.sh only and are NOT needed in the worker. Compare with 'bun run cf:secrets:list' vs 'bun run cf:secrets:list:production'. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-dns-cutover] [operator-verify] equityfundingsource.com does not resolve: no A/AAAA record, nameservers still ns1/2/3.dreamhost.com, and wrangler.toml declares no route or custom_domain. The worker is live and correct at equityfundingsource.brian-takita.workers.dev. Needs registrar + Cloudflare access: move the zone to Cloudflare (or add a CNAME at the registrar), add the custom domain / route to wrangler.toml, verify /login/access-callback still works, and allow for NS propagation. This is the remaining gate on the weekend launch. Advances #tcb0 / #efs-cutover-sso-plan. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#tmo-getlender-verify] [operator-verify] Confirm the #tmo-getlender-confirm LSS path against real TMO data: nothing in production is currently in_lss && !in_los, so GET /LSS.svc/GetLender/:Account has never returned a record in prod — the confirmed_no_funding state is covered by tests and degrades safely on failure, but its success path is unobserved. Needs an operator to name a TMO lender account created but never funded; map a vesting to it, open the reconcile modal, and confirm it reads 'In TMO · no funding yet' rather than 'Unconfirmed in TMO'. Spec: src/equityfundingsource-dev/docs/TMO/get-lender.md. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
