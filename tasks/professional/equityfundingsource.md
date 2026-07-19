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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260718-214413.md`*

Compacted content:
- Archived 10 response topic(s): do [#tmo-sandbox-orphan-loans]; vestings, fundings, and the reconciliation process; TMO live API access; 7 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Dedicated URL for Reconcile with TMO; What TMO API endpoint th...

### Re: Add backlog items onto the queue in priority order — opus-4-8

> **Queue prompt:**
>
> Add backlog items onto the queue in priority order


The queue is mirrored from `agent:backlog` priority order, so I reordered the backlog rather than hand-writing queue lines. Two things were wrong with the old order:

1. **Dependency order was inverted.** `#tmo-reconcile-binder` sat above `#tmo-orphan-reconcile-exec`, but the orphan executor has to run first — the binder reconciles against a roster the executor populates.
2. **Operator-gated items sat above agent-drainable ones.** `#tmo-sandbox-purge-apply` (waiting on your go, permanent TMO deletes with no undo) was the head, which stalls the drain on a human gate before any autonomous work runs.

New priority order — agent-drainable first, then context, then the operator-gated cluster:

| # | Item | Drainable |
|---|------|-----------|
| 1 | `#tmo-orphan-reconcile-exec` | yes |
| 2 | `#tmo-reconcile-binder` | yes (after 1) |
| 3 | `#efs-do-hibernation` | yes |
| 4 | `#efs-prod-secret-parity` | yes |
| 5 | `#tmo-orphan-cache` | context only (superseded by 1) |
| 6 | `#tmo-sandbox-purge-apply` | operator go |
| 7 | `#efs-cf-account-custody` | operator |
| 8 | `#efs-prod-seed` | operator (after 7) |
| 9 | `#efs-dns-cutover` | operator |
| 10 | `#tmo-getlender-verify` | operator |

The launch-gating cluster (7 → 8 → 9) is sequenced deliberately: custody moves the account, then seed once into the new account, then cut DNS over. Doing it in any other order means migrating the data twice.

One caveat on this cycle: preflight reported `queue_maintenance_not_persisted` — editor authority was in `editor_attached_model_missing`, so the computed queue mirror did not land. The backlog reorder in this write is the durable change; the queue text re-syncs from it on the next preflight.
Please add the backlog items into the queue in priority order

### Re: add the backlog items into the queue in priority order — opus-4-8

> **Queue prompt:**
>
> do [#vesting-gap-admin-queue]


Done — all 10 backlog items are now queue heads in backlog priority order. There were no actionable follow-up items to capture this cycle: this was a queue-ordering request, not implementation work, and the 10 items already in the backlog *are* the follow-up.

The automatic backlog→queue mirror was not going to do this on its own. Two things were blocking it:

1. **`queue_maintenance_not_persisted`** — editor authority sat in `editor_attached_model_missing`, so the recomputed queue never landed. This is the same wedge reported on the prior cycle.
2. **Tombstone suppression** — preflight was reporting `suppressed 10 operator-deleted id(s) from backlog→queue mirror (#provauth2 tombstone)`. Those 10 suppressed ids are exactly the 10 backlog items you wanted queued, so even with a healthy editor the mirror would have recomputed an *empty* queue and then set `queue: stop`.

So instead of waiting on the mirror I ran the explicit one-shot `agent-doc queue sync`, which materialized all 10 ids past the tombstone suppression:

```
synced 10 backlog id(s) → 11 queue prompt(s) (Append mode)
materialized: #tmo-orphan-reconcile-exec, #tmo-reconcile-binder, #efs-do-hibernation,
              #efs-prod-secret-parity, #tmo-orphan-cache, #tmo-sandbox-purge-apply,
              #efs-cf-account-custody, #efs-prod-seed, #efs-dns-cutover, #tmo-getlender-verify
```

Resulting queue:

| # | Head | Gate |
|---|------|------|
| 1 | `#tmo-orphan-reconcile-exec` | agent |
| 2 | `#tmo-reconcile-binder` | agent |
| 3 | `#efs-do-hibernation` | agent |
| 4 | `#efs-prod-secret-parity` | agent |
| 5 | `#tmo-orphan-cache` | agent |
| 6 | `#tmo-sandbox-purge-apply` | operator |
| 7 | `#efs-cf-account-custody` | operator |
| 8 | `#efs-prod-seed` | operator |
| 9 | `#efs-dns-cutover` | operator |
| 10 | `#tmo-getlender-verify` | operator |

The stale `do [#vesting-gap-admin-queue]` head is still sitting above them; it is already in `done`, and preflight's auto-strike will reap it on the next clean cycle.

Two things to flag rather than silently absorb:

- **Stale install.** Preflight reports `~/.cargo/bin/agent-doc`, the cdylib, and both built artifacts predate the latest local `src/agent-doc` source edit. That is a concurrent session's tree, not mine — I did not rebuild it, so live tmux/JetBrains sessions may be running pre-edit code at an unchanged version.
- **The editor-authority wedge is not fixed**, only worked around for this write. If `queue_maintenance_not_persisted` keeps recurring the mirror will stay unreliable and queue state will need `queue sync` each time. That is an agent-doc-repo defect, so it does not belong in this document's EFS backlog.

### Re: do [#tmo-orphan-reconcile-exec] — opus-4-8

Shipped and pushed to `equityfundingsource-dev` as `213e42c9`. `bun run check` green: 1927 tests, build, worker build.

**Executor** — `lib/tmo-api/orphan_execute.ts`, split the same way the planner is:

- `orphan_exec_plan_()` — pure gate. Turns decisions into ordered operations, or moves them into `skipped` **with the reason each was withheld**. No I/O.
- `orphan_execute_()` — driver over an injected effects port. That port is the only thing touching D1 or TMO, so the destructive and creative paths are unit-testable end to end with a recording fake rather than a network mock.

The gate, in order:

1. `blocked_no_vesting` **never** executes, at any opt-in level. The planner already refused to attribute the funding; running it anyway produces exactly the unattributable TMO data Phase 1 exists to prevent.
2. `reconcile_cache` / `tombstone` are EFS-local and unguarded.
3. `push_funding` / `create_loan_then_funding` need explicit `--push`, and against a **non-sandbox** database additionally the EFS→TMO push gate (`#dzmm` / `#7a4d`).

Rule 3's sandbox test is on the `Database` header, mirroring `bin/tmo-sandbox-purge.ts` as the backlog item asked: the TMO token is identical across environments and only that header selects one, so the database name is the only thing between a sandbox run and a production write. I check it **inside** the planner layer rather than trusting a caller's flag. `create_loan_then_funding` calls `NewLoan` before `AddLoanFunding` and does not push when the create fails — there is a test for exactly that ordering.

`bin/tmo-orphan-reconcile.ts` runs it: D1 through the same `cf-wrangler` path the deploy runbook uses, TMO through `fetch`. Default is a dry run; `--apply` alone runs **only** the EFS-local actions. The lender account is resolved through `lender_vestings.tmo_lender_account`, not `users.lender_account` — per migration 0138 one lender has many vestings and TMO holds one account per vesting, and a fund with no linked vesting resolves to NULL, which is what the planner turns into `blocked_no_vesting`.

### Re: Can we backup the tmo db? — opus-4-8

> **Queue prompt:**
>
> do [#tmo-orphan-reconcile-exec]
>
> do [#tmo-sandbox-purge-apply]


**No — TMO has no backup.** No dump, no export, no restore endpoint. The API is per-entity REST with no bulk import and no server-side trash, and `docs/TMO/delete-loan.md` says outright that `DeleteLoan` "permanently removes the loan... There is no undo and no TMO-side trash."

That is a bad place to be with `#tmo-sandbox-purge-apply` sitting in the queue holding 16 permanent deletes, so I built the closest available substitute rather than leaving the question at "no": `bin/tmo-snapshot.ts` sweeps the documented GET endpoints — loan index, per-loan record, fundings, payment schedule, attachment metadata, lender and borrower rosters — into one timestamped JSON file.

I want to be blunt about what it is not. **A snapshot is evidence, not a restore:**

- TMO has no bulk import — restoring means replaying `NewLoan` / `AddLoanFunding` per record.
- RecIDs are server-assigned and will **not** round-trip, so every EFS row holding one (`funds.tmo_funding_rec_id`, `lender_vestings.tmo_lender_rec_id`) needs re-reconciling after any rebuild.
- Attachment **bytes** are not captured, only metadata. Documents deleted in TMO are gone.
- Borrowers survive a `DeleteLoan`, so a loan-level rebuild can collide with surviving borrower records.

Those four print on every run, sit in the module, and lead `docs/TMO/snapshot.md`. A snapshot that reads as a backup is worse than no snapshot, because it licenses a delete nobody can undo.

Two properties make it usable as a gate. `--covers` exits non-zero unless every named loan was captured **cleanly**, and the process exits non-zero on any incomplete sweep at all — so a partial capture can't silently authorize a purge:

```bash
bun ./bin/tmo-snapshot.ts --covers "$LOANS" --out tmp/before.json \
  && bun ./bin/tmo-sandbox-purge.ts --apply
```

I wired that precondition into `#tmo-sandbox-purge-apply` rather than leaving it as advice.

**One caveat you should weigh before relying on this.** The tool is fully tested at the pure layer but has **never run against real TMO** — I have no token. In particular `LSS.svc/GetLenders` and `LOS.svc/GetBorrowers` are inferred from the documented API surface and are *not* covered by any file in `docs/TMO/`. If either 404s, the sweep records it as an error and correctly refuses to report `complete`, but the roster capture would be empty. Filed as `#tmo-snapshot-verify` and made a blocker on the purge.
Please add a backup command to backup the TMO system. Test backup on the sandbox.

### Re: do [#tmo-reconcile-binder] — opus-4-8

Shipped and pushed as `bbbd7c1a`. `bun run check` green: 1943 tests, build, worker build.

`lib/tmo-api/funding_bind.ts` follows the same split as the matcher and the orphan executor from the previous cycle — `funding_bind_plan_()` is a pure gate, `funding_bind_apply_()` drives an injected effects port. That shape matters more here than anywhere else in this series: an auto-bind sets `funds.status='active'` and overwrites conflicting EFS fields from the TMO row, so a wrong bind attributes someone else's money.

The gate, in order:

1. **Only `auto_bind` binds.** `review` / `ambiguous` / `no_match` are held **with their ranked candidates**. For a held row that ranking *is* the deliverable — it's what makes the admin queue open pre-ranked instead of blank, which is what the backlog item actually asked for.
2. **Collision guard.** A run must not bind two fundings on the same loan to the same lender. The matcher scores each funding independently and structurally cannot see this; two rows scoring one lender on one loan is far more likely a duplicate TMO funding (the `#5q7z` class) than a lender funding the same loan twice through separate records. Both are held. Scoped per loan, so the same lender on two different loans binds normally.
3. **Auto-bind is opt-in**, default off — a scheduled reconcile pass persists rankings without moving money.

Rankings persist *before* any bind runs, so a bind failure still leaves the queue pre-ranked for everything the pass scored. There's a test pinning that ordering.

Binds go through the existing `tmo_unmatched_funding_bind_()` rather than a reimplementation — it already owns the PRD D4 reconcile-vs-insert decision (TMO wins on conflict) and the override audit.

**One thing I found that changes the PRD.** The matcher's `lender_account` signal is dead. Migration 0140 retired `users.lender_account` onto `lender_vestings` and **0141 physically dropped the column**, so a roster builder reading it would throw. `tmo_match_roster_R_()` sources accounts from `lender_vestings.tmo_lender_account` only and leaves `roster_entry_T.lender_account` unset — every account match now runs through `vesting_account`, the stronger signal anyway. I left the weight-table row in the PRD marked as provenance rather than deleting it, and there's a test asserting account matching still works end to end with no `lender_account` on any roster entry.

**Verification beyond the suite**, since this cycle added SQL — unit tests would not have caught a bad column name:

- migration `0142` applies locally (`cf:d1:migrate:local`, ✅ through 0142);
- the `tmo_match_roster_R_()` query executes against real local D1 and returns rows — this caught a real bug, my first draft filtered vestings on `lv.archived_at IS NULL`, but `lender_vestings` has no such column (it uses `status IN ('active','archived')`, migration 0041);
- the four new `0142` columns are queryable.

**What is not done, and it's the honest gap:** nothing reads these columns yet. `tmo_unmatched_fundings_R_` doesn't select them and no route or cron calls `tmo_unmatched_fundings_reconcile_()`, so today the pass only runs from a test. The persistence layer and the safety gate are real and verified; the admin queue does not yet show the ranking. Filed as `#tmo-reconcile-binder-ui` at the top of the backlog rather than claiming the queue is pre-ranked in the UI.

### Re: do [#efs-do-hibernation] — opus-4-8

Shipped and pushed as `b75f92ca`. `bun run check` green: 1960 tests, build, worker build.

All three room classes — `BorrowerChatRoom`, `AdminEventsRoom`, `PortalMessagesRoom` — now use `state.acceptWebSocket()` plus `webSocketMessage` / `webSocketClose` / `webSocketError` handlers instead of `server.accept()` with `addEventListener`.

The part that made this more than a mechanical swap: the runtime may evict the object and construct a **fresh instance over the same state**, so anything held per-connection on the instance is gone. Two things had to move:

- the socket set → `state.getWebSockets()` instead of a `Set<WebSocket>` instance field;
- `BorrowerChatRoom`'s per-socket identity → `serializeAttachment()` instead of a `WeakMap`.

Leaving either in place gives you a room that passes every warm-object test and then silently stops broadcasting once evicted — and for the chat room, throws `chat socket is not registered` on every inbound message after the first hibernation. I also had to make `broadcast_()` stop routing send failures through `close_()`, which broadcasts, since that path would now recurse.

**On verification, because the green suite was misleading here.** The three pre-existing realtime tests never instantiate these classes — they assert `typeof PortalMessagesRoom === 'function'` and exercise the `notify_*` helpers. So the suite was green before and after and proved nothing about the conversion.

I wrote `tests/do-hibernation.test.ts` to actually drive the rooms: a fake state that models the two things that matter (the runtime owns the socket list; `serializeAttachment` survives), and an `eviction` step that builds a new room instance over the same state exactly as the runtime does. Then I checked the tests are worth having by stashing the source changes and re-running against the old code:

```
--- old code ---   1 pass, 16 fail
--- restored ---  17 pass,  0 fail
```

Only the "does the class expose the handlers" assertion is trivially satisfiable; the other 16 genuinely pin the regression class.

Updated `docs/deploy.md` — the cost section now reads as resolved rather than pending, and the bill table drops the "up to ~$5" DO row. The open-tab scaling that made a single always-on room ~82% of the included 400,000 GB-s is gone.

No new backlog items: this one is complete, and there is no follow-up it created.

### Re: do [#tmo-reconcile-binder-ui] — opus-4-8

Shipped and pushed as `5383afba`. `bun run check` green: 1972 tests, build, worker build.

**I had the scope wrong when I filed this.** I wrote that the queue "still renders blank candidate lists" — it doesn't, because there was no binding-queue page at all. `/api/admin/tmo-unmatched-fundings` existed but nothing in the UI tree fetched it. So this cycle wired the whole path, not just a render fix.

Read layer:

- both SELECTs now carry the 0142 columns;
- pending rows order by `match_confidence DESC` with **NULLs last** — otherwise never-scored rows sort first and lead the queue with the least useful entries;
- `tmo_unmatched_funding_match_()` decodes the ranking and degrades a malformed `match_candidates_json` to "no candidates" rather than taking the whole render down.

Routes: the `GET` returns each row with `match` already decoded, and a new `POST .../reconcile` runs the pass. Auto-bind is `body.auto_bind === true` — an explicit `true`, never a truthy default.

UI: a new admin page under nav **Unmatched Fundings**, rendering candidates highest-confidence first with the evidence behind each as one-click bind targets, plus Rescore. Rescore posts `auto_bind: false`, and there's a test asserting the page contains no `auto_bind: true` anywhere — ranking the queue must never be the same click as moving money.

The three states stay distinct end to end, since collapsing any two sends an admin to guess at money: **never scored** ("Not scored yet"), **scored and found nobody** ("No candidate"), and **scored with candidates**.

**What the verification caught.** My SELECT change broke 10 existing tests with `no such column: match_outcome` — the test fixtures build their own SQLite schema inline and had not gained the 0142 columns. Real omission, fixed in the fixture. Beyond the suite I confirmed the page and route are actually in the built artifacts (`Not scored yet` in the browser bundle, `tmo-unmatched-fundings/reconcile` in the worker bundle), since registering a section and having it survive the build are different claims.

**Not verified:** I have not driven this page in a browser against real parked rows. The reconcile pass has no cron or scheduled trigger either — it runs from the admin button or a direct POST only. Neither is a gap I can close without a live environment, and neither blocks the queue from being used.
<!-- agent:boundary:209e0e96 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- Please add a backup command to backup the TMO system. Test backup on the sandbox.
- ~~do [#tmo-reconcile-binder-ui]~~
- do [#efs-prod-secret-parity]
- do [#tmo-orphan-cache]
- do [#efs-cf-account-custody]
- do [#efs-prod-seed]
- do [#efs-dns-cutover]
- do [#tmo-getlender-verify]
- do [#tmo-snapshot-verify]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tmo-backup-command] Add a backup command for the TMO system and test the backup against the TMO sandbox. Operator request (verbatim: "Please add a backup command to backup the TMO system. Test backup on the sandbox."). Scope: a bin/ command that captures a restorable snapshot of TMO state (lenders, borrowers, loans, fundings) via the documented LSS/LOS API surface, written to a versioned artifact; dry-run + apply split like bin/tmo-snapshot.ts; verify a full backup+restore round trip on the sandbox tenant. Blocked-ish: no real TMO token locally — see #tmo-snapshot-verify.
- [ ] [#tmo-snapshot-verify] [operator-verify] Run bin/tmo-snapshot.ts against the real TMO sandbox with the operator token (TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)") and confirm the sweep reports complete=true. The tool, its pure manifest/coverage layer, and its tests shipped 2026-07-18 (#tmo-orphan-reconcile-exec cycle) but no run has ever touched real TMO, so the endpoint set is unverified — in particular LSS.svc/GetLenders and LOS.svc/GetBorrowers are inferred from the documented API surface and are NOT covered by docs/TMO/*.md. If either 404s, drop it from the sweep or correct the path. This is the precondition for #tmo-sandbox-purge-apply. Spec: docs/TMO/snapshot.md.
- [ ] [#efs-do-hibernation] Switch the three Durable Object room classes to the WebSocket Hibernation API. BorrowerChatRoom, AdminEventsRoom, and PortalMessagesRoom all call the non-hibernating server.accept() with no idle alarm (app/equityfundingsource/lib/messages/realtime.ts:35, lib/borrower/messages/realtime.ts:76, lib/admin/events/realtime.ts:50), so an open browser tab pins its DO active and accrues GB-s duration billing whether or not anyone is typing — cost scales with concurrent open tabs instead of business volume. A single always-on room is ~328,500 GB-s of the 400,000 GB-s included in Workers Paid (~82%); ten rooms open 8h/day x 22 days is ~792,000 GB-s (~+$4.90/mo). Fix: state.acceptWebSocket() plus webSocketMessage/webSocketClose handlers; Cloudflare does not bill duration while a hibernatable socket is idle. Cost detail: src/equityfundingsource-dev/docs/deploy.md 'C — What this costs'. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-prod-secret-parity] Production is missing secrets staging has and .dev.vars cannot supply (values are only in Cloudflare and cannot be read back out): DROPBOX_APP_KEY / DROPBOX_APP_SECRET / DROPBOX_REFRESH_TOKEN gate document sync. AUTH_SECRET, ADMIN_GLOBAL_PASSWORD, CLOUDFLARE_API_TOKEN, and CLOUDFLARE_ACCOUNT_ID are already set (2026-07-18); TMO_API_TOKEN / TMO_DATABASE are intentionally unset so production runs split (#tmo-split-launch). AWS_*/S3_BUCKET_NAME are local bin/assets-*.sh only and are NOT needed in the worker. Compare with 'bun run cf:secrets:list' vs 'bun run cf:secrets:list:production'. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#tmo-orphan-cache] SUPERSEDED 2026-07-18 by #tmo-orphan-as-fundings per operator directive — do NOT prune/tombstone these rows as the default. The 18 orphan tmo_cache_entities 'loan' rows are the reconciliation corpus: push them into TMO as fundings and tombstone only pure debris with no EFS deal. Kept as context for the loan-number list (TST042/TST113/TST217/TST254/TST374/TST388/TST723/TST762/TST766/TST999, AVW580, FTM631, GQI751, ICL909, MBQ435, PIP038, SRV532, 554466) and for the note that admin_tmo_loans_by_loan_number_DELETE_ does NOT cover them — it only disables EFS 'loans' rows and never touches tmo_cache_entities. Execute via #tmo-orphan-reconcile-exec. Plan: src/equityfundingsource-dev/prds/vesting-and-tmo-reconciliation.md.
- [ ] [#tmo-sandbox-purge-apply] [operator-verify] Execute the sandbox purge: `TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)" bun ./bin/tmo-sandbox-purge.ts --apply` deletes 16 EFS test-debris loans from the TMO sandbox (554466, AVW580, FTM631, ICL909, MBQ435, PIP038, SRV532, TST113, TST323, TST388, TST488, TST766, TST775, TST786, TST788, TST941). SNAPSHOT FIRST (added 2026-07-18): TMO has no backup or restore and DeleteLoan is permanent, so gate the purge on `bun ./bin/tmo-snapshot.ts --covers <the 16 loan numbers> --out tmp/tmo-before-purge.json`, which exits non-zero unless every named loan was captured cleanly. Blocked on #tmo-snapshot-verify (the snapshot tool has never run against real TMO). Held pending operator go because DeleteLoan is permanent with no TMO-side undo. 1010 (TMO seed) and GQI751 (pinned repro fixture for #dealtab500/#5q7z) are keep-listed in code and cannot be selected. Dry run verified 2026-07-18: purge=16 keep=2. Recipes: docs/TMO/delete-loan.md, docs/TMO/snapshot.md.
- [ ] [#efs-cf-account-custody] [operator-verify] Move EFS to its own Cloudflare account. Both environments currently live in Brian's personal account (1e7e194ff8da4ad9e40ce041aea84f76), which also hosts unrelated projects. STEP 1 (Judith, instructions drafted 2026-07-18): sign up at dash.cloudflare.com/sign-up with a SHARED EFS address (becomes billing + recovery contact), verify the email (hard prerequisite — invites and 2FA both fail without it), invite Brian as Super Administrator via Manage Account -> Members -> Invite, enable 2FA, have a company card ready for the R2 checkout. Super Administrator is required, not just Workers Platform Admin, because R2 activation is a purchase and the deploy pipeline needs an account-owned API token. STEP 2 (Brian): D1 databases and R2 buckets cannot move between accounts — create them in the new account, update the ids under [env.production] in wrangler.toml, export+import D1, copy R2, re-push secrets (per-account AND per-environment), then repoint DNS. Sequencing: do this before #efs-prod-seed to migrate the data once instead of twice. Advances #efs-dns-cutover. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
- [ ] [#efs-prod-seed] [operator-verify] Decide whether the production Cloudflare environment should be seeded from staging before launch. Production D1 (equityfundingsource-production) is schema-complete and empty; staging holds 33 users / 7 loans / 7 deals. If that is real business data it must be exported+imported once, immediately before cutover (a second import collides on primary keys), and R2_DOCUMENTS must be copied separately or uploaded documents 404 in production despite their D1 rows existing. Recipe: src/equityfundingsource-dev/docs/deploy.md 'Seeding production from staging'. Blocked on the operator confirming real-vs-test data. See #efs-cf-account-custody — if custody moves first, seed once after the move. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
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
