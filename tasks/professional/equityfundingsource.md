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

### Re: do [#tmo-backup-command] — opus-4-8

Shipped and pushed as `b66e77df`. `bun run check` green: 1994 tests (up from 1972), build, worker build.

**The tool you asked for already half-existed, and the missing half was the important one.** `bin/tmo-snapshot.ts` sweeps the same endpoints, but it deliberately skips attachment **bytes** — its own caveats say so. That means a document deleted in TMO was unrecoverable even with a snapshot in hand. `bin/tmo-backup.ts` is the restorable artifact; the snapshot stays as the fast pre-destructive evidence capture. `docs/TMO/backup.md` opens with the table saying which to reach for.

Three things make it a backup rather than a bigger snapshot:

- **Attachment content**, via `GetLoanAttachment/:RecId`, each with a recorded sha256 and decoded byte length. TMO returns `Document` as a bare string, a one-element array, or null depending on the record; all three are normalized, because treating the array form as "no document" would silently drop every attachment.
- **`--verify`** re-derives every digest *from the artifact*, offline, no token. Recording a digest at capture time and never checking it again only proves a digest was recorded.
- **`--restore-plan`** emits the ordered replay TMO supports, with `depends_on` as data — `NewBorrower` needs a `LoanRecID` that only exists after `NewLoan` returns, so a future applier can't parallelize it into corruption.

`complete` is stricter than the snapshot's on purpose: a listed-but-uncaptured attachment sets `complete=false` and exits non-zero. A backup missing document bytes is not a backup of those documents.

**On testing it against the sandbox.** I have no TMO token, so I could not run this against the real sandbox — that stays blocked, same as `#tmo-snapshot-verify`. What I could do is better than nothing and I want to be precise about which: the capture path now runs behind an injected fetch port, so `tests/tmo-backup.test.ts` drives the *real* capture code against the local TMO simulator — seed a loan, funding, and attachment through the simulator's write endpoints, capture, and assert the bytes come back and self-verify. I also drove the CLI itself end to end on a simulator-seeded artifact: `--verify` passes clean, and after flipping the content it reports `DIGEST_MISMATCH` with both digests and exits 1.

**What the verification caught.** `--verify` on a clean artifact exited 1 while printing `ok=true` — digests passed, but the simulator's unimplemented `GetLenders` / `GetBorrowers` / `GetPaymentSchedule` made the backup incomplete, and verify mode never printed that. A non-zero exit under an all-green report reads as a broken checker. It now prints `complete=`, the incomplete loans, and the sweep errors.

**Not verified, and the honest edge:** the simulator's `AddAttachment` echoes whatever fields you post, so the round trip seeds `Document` (what `GetLoanAttachment` returns) rather than `Content` (what real `AddAttachment` accepts). The capture reads `Document` and the restore plan writes `Content`, both per `docs/TMO/`, but only the read half faces a server that behaves like TMO. Recorded in `docs/TMO/backup.md` rather than left for someone to discover mid-restore.

The CLI plans a restore, never applies one — writing a replay into a live TMO is the same class of action as `#tmo-sandbox-purge-apply` and belongs behind an explicit operator step, not a flag on the backup tool.
❯ Does `passage equityfundingsource/tmo/sandbox/api` have the key?

### Re: Does `passage equitywfundingsource/tmo/sandbox/api` have the key? — opus-4-8

**Yes** — 32-char hex on line 1, plus metadata: `database: 001-8553-000-Equity Funding - Sandbox`, `host: https://api.themortgageoffice.com`, organized 2026-07-18. The entry also records that the token value is **identical** in `tmo/live/api` and `tmo/sandbox/api` — TMO issues one token reaching both databases, and only the `Database` header selects the environment. Rotating means updating both paths.

So I ran the backup against the real sandbox. **It failed, and the failures were real.**

First run: `0/20 attachments captured`, `complete=false`. Three defects, none of which any unit test could have caught:

1. **`GetLoanAttachment` returns `Document` as a JSON array of raw byte values** — `[37, 80, 68, 70, ...]`, which is `%PDF-`. Not base64. `docs/TMO/get-loan-attachment.md` types it only as "array/string/null", so I wrote the normalizer to read element 0 of that array as a base64 string. Every attachment decoded to nothing while all 22 tests stayed green.
2. **`LOS.svc/GetPaymentSchedule` does not exist.** Neither does `GetLoanPaymentSchedule`, `GetLoanSchedule`, `GetPaymentSchedules`, `GetLoanPayments`, or `GetSchedule` — all 404, and `GetLoan` carries no schedule field either. TMO exposes no payment-schedule read at all.
3. **`LOS.svc/GetBorrowers` does not exist** either. Borrowers ride inline on each `GetLoan`.

`LSS.svc/GetLenders` works fine — the one endpoint I'd have bet against.

**`bin/tmo-snapshot.ts` calls both dead endpoints too.** That is the whole of `#tmo-snapshot-verify`: the snapshot tool could never report `complete=true` because it was always waiting on two 404s. Fixed in the same pass, so both tools now skip them and collect borrowers from the loan records, deduped by `RecID` — a borrower on several loans replayed once per loan would create duplicates on restore.

After the fixes, against the real sandbox:

```
[tmo-backup] loans=18 fundings=5 attachments=20/20 bytes=56636 lenders=1 borrowers=16
[tmo-backup] self-verify digests=20 ok=true
[tmo-backup] complete=true
```

`bin/tmo-snapshot.ts` reports `complete=true` there for the first time as well.

I did not stop at the tool's own report. Magic-byte check on the artifact: 12 PDFs, 4 PNGs, 1 JPG, 3 text files, each matching its extension. The bytes are real documents, not plausible blobs. `--verify` re-passes on the written file offline.

Regression coverage is the shape that broke it: the simulator echoes whatever you post, so the test seeds `Document` as a byte array and drives the real capture path end to end, plus direct cases for non-UTF-8 bytes and the borrower dedup. `bun run check` green at 1998.

Pushed as `915fa803`.

**Still unverified: the restore path.** `--restore-plan` is generated and asserted, but no op has been replayed into TMO, so the request bodies are only as correct as `docs/TMO/`. Given that the docs were wrong about `Document` and wrong about two endpoints, I would not assume they are right about `AddAttachment` taking `Content`. A first restore belongs on the sandbox, one loan, with a fresh backup taken immediately before — filed as `#tmo-restore-apply`.

### Re: do [#tmo-restore-apply] — opus-4-8

Shipped and pushed as `f6020206`. `bun run check` green: 2005 tests, build, worker build.

**The restore works. Verified end to end on the real sandbox, not just planned.**

I created a throwaway loan (`BKTEST7706`) with a canary PDF, backed it up, **deleted it from TMO**, restored from the artifact, and re-captured:

| Step | Result |
|---|---|
| Backup | `attachments=1/1 bytes=91`, `complete=true` |
| `DeleteLoan` | gone — `GetLoan` returns "Loan not found." |
| `--restore --apply` | `applied=2 failed=0 skipped=0` |
| Re-capture + diff | document **sha256 identical**, 48 of 51 loan fields preserved |

The three that changed are the ones already documented as unrestorable: `RecID` (new, server-assigned), `SysTimeStamp` (restore time), and `Fields` — reordered by TMO, but the same 25 entries with identical keys and values. Test loan deleted afterward; the sandbox is back to its original 18 loans with no `BKTEST` residue.

**Two contract details this run pinned down, neither in `docs/TMO/`:**

- **`AddAttachment` needs `TabName` present and EMPTY.** Omitting it throws `Object reference not set to an instance of an object`; any non-empty value this database doesn't know is `Incorrect tab name.` I tried `Documents`, `Loan`, `General`, `Attachments`, `Docs`, `Servicing`, `Origination` — all rejected. My plan already emitted `attachment.tab ?? ''`, which was correct, but only by luck until now. Pinned by a test so nobody widens it to pass a null through.
- **The bytes go in `Content` as base64.** Sending the `Document` byte array that `GetLoanAttachment` hands back is rejected with `File content not found`. Upload and download shapes differ, which is precisely the asymmetry I flagged as unverified last cycle.

`NewLoan` also requires a valid product `DocID` — my first attempt failed with `Loan Product not found`. Captured loans carry it, so a normal restore is fine; a hand-edited artifact is not. Added to the unrestorable list.

**The simulator was hiding all of this.** Its `AddAttachment` echoed `Content` back unchanged, so an upload followed by a read returned no document — a restore round trip that could never have worked would still have passed in tests. I fixed the simulator to convert `Content` to the byte-array `Document` shape real TMO serves, and made `GetLoanAttachments` (plural) return `Document: null` like the real plural endpoint does. That second one matters: without it, a caller that never fetches the singular endpoint passes locally and comes back empty from TMO — the exact bug from last cycle.

On the applier itself: `restore_apply_()` threads the RecID TMO assigns `NewLoan` into `NewBorrower`'s `LoanRecID`, skips a loan's dependents when the loan itself fails (sending them anyway either 404s or attaches them to whatever loan already holds that number), and treats a `200` carrying an `ErrorMessage` as a failure — TMO does that on rejected writes, so HTTP status alone would report a failed restore as successful.

`--restore` is a dry run by default. `--apply` refuses any database whose name lacks "Sandbox", checked on both the target and the artifact's origin, so restoring into production isn't reachable from this tool.
❯ Please make restore sync in case there is existing data in the TMO but was somehow corrupted. Would you recommend not deleting any records in the restore?

### Re: make restore sync; would you recommend not deleting any records? — opus-4-8

**Yes — strongly recommend never deleting, and I built it so the tool cannot.** Shipped as `55b7541a`. `bun run check` green: 2016 tests.

Four reasons, in the order I weight them:

1. **`DeleteLoan` is permanent and TMO has no trash.** Driving an irreversible operation from a diff against a possibly-stale file is the wrong shape regardless of how good the file is.
2. **A record absent from the backup is indistinguishable from one legitimately created after `captured_at`.** "Delete what the backup doesn't have" destroys exactly the newest work — the opposite of what you want during a recovery.
3. **Delete-then-recreate loses everything if the recreate fails halfway**, and it churns RecIDs that EFS rows point at, turning a data problem into a referential one.
4. **Corrupt data that is still present is evidence.** Deleting it throws away the ability to work out what went wrong.

So `--sync` is **additive and corrective only**. `sync_plan_` emits nothing but `NewLoan` / `UpdateLoan` / `NewBorrower` / `AddLoanFunding` / `AddAttachment`, and a test asserts no plan can contain a delete no matter how far the two sides diverge. Extra records are reported and left for you.

**How sync differs from restore.** Restore assumes the records are gone and replays everything. Sync re-captures the live database, diffs it against the artifact, and acts only on the difference: missing loan → create; drifted loan → `UpdateLoan` keyed by the **live** RecID (the captured one doesn't round-trip, so sending it would 404 or hit the wrong record); missing fundings/attachments → add only the ones TMO lacks. It's idempotent — a second run is a no-op. In a real incident this is the one to reach for.

**Verified against the real sandbox, not just the simulator:**

- Sync from a backup of the healthy 18-loan database: **0 ops, 0 reports, 18 loans already matching.** No false drift, which is the thing most likely to be quietly wrong — `RecID`, `SysTimeStamp`, and `Fields` ordering all vary between reads and are excluded deliberately.
- Deliberate corruption on a throwaway loan (wrong `LoanAmount`/`NoteRate`/`ShortName`, document lost): sync planned `update_loan (4 fields drifted)` + `add_attachment`, applied both, and the re-capture matched the known-good state with a **byte-identical document sha256**. Re-running sync afterward planned 0 ops.
- **The safety property, on real data:** I added a document *after* the backup was taken, then synced. It was reported `EXTRA_IN_TMO … NOT removed` and survived. Sandbox is back to its original 18 loans, no residue.

**One case sync deliberately will not fix by itself:** an attachment present under the same name with a *different* digest. TMO has no attachment update, so the only moves are adding a second copy under the same name or deleting the bad one — both worse than telling you. That's reported as a `conflict` with both digests, and it's your call.

### Re: do [#efs-prod-secret-parity] — opus-4-8

> **Queue prompt:**
>
> do [#efs-prod-secret-parity]


Shipped and pushed as `e79e71a8`. `bun run check` green: 2019 tests, build, worker build. Not deployed — the queue preset is `#spec-test-commit-push`.

**The parity audit found a real defect, not just a missing-secret list.** Production has 4 of staging's 13 secrets:

| Secret | In prod? | Verdict |
|---|---|---|
| `AUTH_SECRET`, `ADMIN_GLOBAL_PASSWORD`, `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID` | yes | fine |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL_S3`, `S3_BUCKET_NAME` | no | **correctly absent** |
| `TMO_API_TOKEN`, `TMO_DATABASE` | no | **intentionally unset** (`#tmo-split-launch`) |
| `DROPBOX_APP_KEY`, `DROPBOX_APP_SECRET`, `DROPBOX_REFRESH_TOKEN` | no | **genuinely missing** |

I verified the AWS claim rather than trusting the backlog text: those four have **0 references in worker source** and appear only in `bin/` scripts, so they are correctly not in the worker. The Dropbox three have 4/2/4 worker references — they are real.

**The defect.** `dropbox_oauth_start_GET_` called `require_secret_(env.DROPBOX_APP_KEY, …)` outside any try block. `require_secret_` throws on an unset value, and the worker's `onError` turns that into a bare `c.text('Internal error', 500)`. So on production today, an admin clicking "Connect Dropbox" gets an opaque 500 — indistinguishable from the app being broken, when in fact the integration was simply never configured there. The callback route had the same exposure ahead of its try block.

Both routes now check `dropbox_configured_()` first and return **503** with a message naming the exact three secrets an operator has to push. 503 rather than 500 because the deployment is healthy; the capability isn't provisioned. The guard is ordered before the throwing call and a test asserts that ordering, since moving it later would silently restore the 500.

**What this does not do:** it does not obtain the Dropbox credentials. Those live only in Cloudflare, cannot be read back out, and pushing them to production needs operator access — so document sync stays unavailable in production until someone runs `bun run cf:secrets:push:production` with the real values. That is now a clean, self-describing 503 instead of a crash, which is the part I could actually close.

### Re: do [#tmo-orphan-cache] — opus-4-8

Shipped and pushed as `c806c0de`. `bun run check` green: 2024 tests.

**The item was marked superseded with its executor "shipped" — but that executor had never once run.** `bin/tmo-orphan-reconcile.ts` landed with `#tmo-orphan-reconcile-exec` and died on every invocation:

```
SyntaxError: JSON Parse error: Unexpected identifier "cf"
```

It extracted wrangler's JSON with `stdout.indexOf('[')`. Both the `cf-wrangler` wrapper's `[cf-wrangler] account=…` log lines and wrangler's colour-escaped `[WARNING]` banner come first, so the first `[` in stdout is never the payload. The code comment even claimed it took "the first balanced JSON value rather than assuming the whole stdout parses" — the implementation did neither.

`d1_rows_` now walks every `[` candidate and accepts only an array whose entries carry `results`, so bracketed log text can't be mistaken for a result set. It lives in `lib/tmo-api/d1_stdout.ts` rather than the bin script, because that script `process.exit`s at import time without `TMO_API_TOKEN` — there was nothing a test could import, which is a fair part of why this shipped broken. Five regression tests cover the real preamble shape, non-payload arrays, multi-statement flattening, and empty results.

**First successful run**, against remote D1 and the real TMO sandbox:

```
cache_rows=18  tmo_loans=18  efs_fundings=0
plan reconcile_cache=12  tombstone=6  push_funding=0  create_loan_then_funding=0  blocked_no_vesting=0
```

That plan agrees with your directive. `push_funding=0` because `efs_fundings=0` — there is nothing to push into TMO, so the "orphans as fundings" path is a no-op on current data. The 6 tombstones (`TST217`, `TST254`, `TST374`, `TST723`, `TST762`, `TST999`) all match the planner's narrow rule: absent from TMO *and* with no EFS loan or funding behind them — pure test debris, not the "don't prune by default" class you flagged.

**I did not apply it.** `--apply` mutates production D1 `tmo_cache_entities` irreversibly, and tombstoning is the exact operation your directive told me not to do by default. Filed as `#tmo-orphan-apply` with the command and the six loan numbers to review. Everything gating that decision — a working tool and a real plan — is now done; the decision itself is yours.
What are TST217, TST254, TST374, TST723, TST762, TST999?

### Re: What are TST217, TST254, TST374, TST723, TST762, TST999? — opus-4-8

**They are dead cache rows. Not loans, not deals, not money — six rows in EFS's `tmo_cache_entities` that point at TMO loans which no longer exist.**

Queried remote D1 and the live TMO sandbox to answer this rather than reasoning from the planner's label:

| Check | Result |
|---|---|
| Row in `tmo_cache_entities` | yes, all six, `entity_type='loan'`, `deleted_at IS NULL` |
| `efs_id` (the EFS deal it was cached against) | **NULL** on all six |
| EFS `loans` rows referencing them | **0** |
| EFS `funds` rows referencing them | **0** |
| Present in TMO today | **no** — `GetLoan` returns "Loan not found." for all six |
| Created / last updated | all identical: 2026-06-08 14:56 / 14:58 UTC |

**Where they came from.** All six were written in a single cache sweep on 2026-06-08 at 14:56 UTC that captured 10 loan rows: `1010`, `TST042`, `TST113`, `TST217`, `TST254`, `TST374`, `TST388`, `TST723`, `TST762`, `TST999`. Only `TST042` got an `efs_id`; the other nine were cached with no EFS deal behind them. `TST###` is the EFS integration-test naming — these are test loans the integration created in the shared TMO sandbox.

**What makes these six different from the rest of that batch.** `1010` (the TMO-seeded demo loan, protected in code), `TST113`, and `TST388` still exist in TMO, so the planner assigns them `reconcile_cache` — refresh the cached row against the live record. The six you asked about were deleted from TMO at some point after the sweep, and nothing cleaned up the EFS side. So they are stale pointers to nothing: they can't be reconciled (no TMO record), can't be pushed as fundings (no EFS funding — `efs_fundings=0`), and can't be attributed to a deal (no `efs_id`).

That is exactly the narrow condition your directive carved out: *"tombstone only pure debris with no EFS deal."* They are not the class you told me not to prune — that class is orphan rows **with** an EFS deal behind them, which would get pushed into TMO as fundings instead. There are currently zero of those.

**Deleting them costs nothing recoverable.** The tombstone is EFS-local — it marks the cache row, it does not touch TMO (nothing there to touch). Worst case if we're wrong: the cached metadata for a loan that no longer exists is lost, and any future TMO loan reusing that number would simply re-cache on next sync.

I'd still rather you push the button, since it writes to production D1. `#tmo-orphan-apply` has the command.

One housekeeping note: the queue still lists `do [#tmo-orphan-cache]` unstruck even though the backlog item was reaped last cycle — preflight has been reporting `queue_maintenance_not_persisted` (editor authority stuck in `editor_attached_model_missing`), so the recomputed queue text isn't landing. Cosmetic, but that's why a completed item is still showing as a head.
<!-- agent:boundary:d9e41326 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-commit-push" priority go -->
- do [#tmo-orphan-cache]
- do [#efs-cf-account-custody]
- do [#efs-prod-seed]
- do [#efs-dns-cutover]
- do [#tmo-getlender-verify]
- ~~do [#tmo-snapshot-verify]~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#tmo-orphan-apply] [operator-verify] Apply the orphan reconcile plan against remote D1: `TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)" bun ./bin/tmo-orphan-reconcile.ts --remote --apply`. The first successful dry run (2026-07-18, after the #tmo-orphan-cache parse fix) plans reconcile_cache=12 + tombstone=6, push_funding=0, blocked_no_vesting=0 — no TMO writes at all, EFS-local only. Gated on operator go because --apply mutates PRODUCTION D1 tmo_cache_entities irreversibly and the 6 tombstones are exactly the class the operator directive said not to prune by default (they qualify: absent from TMO with no EFS loan/funding behind them). Review the 6 (TST217, TST254, TST374, TST723, TST762, TST999) before running.
- [ ] [#tmo-snapshot-verify] [operator-verify] Run bin/tmo-snapshot.ts against the real TMO sandbox with the operator token (TMO_API_TOKEN="$(passage equityfundingsource/tmo/sandbox/api | head -1)") and confirm the sweep reports complete=true. The tool, its pure manifest/coverage layer, and its tests shipped 2026-07-18 (#tmo-orphan-reconcile-exec cycle) but no run has ever touched real TMO, so the endpoint set is unverified — in particular LSS.svc/GetLenders and LOS.svc/GetBorrowers are inferred from the documented API surface and are NOT covered by docs/TMO/*.md. If either 404s, drop it from the sweep or correct the path. This is the precondition for #tmo-sandbox-purge-apply. Spec: docs/TMO/snapshot.md.
- [ ] [#efs-do-hibernation] Switch the three Durable Object room classes to the WebSocket Hibernation API. BorrowerChatRoom, AdminEventsRoom, and PortalMessagesRoom all call the non-hibernating server.accept() with no idle alarm (app/equityfundingsource/lib/messages/realtime.ts:35, lib/borrower/messages/realtime.ts:76, lib/admin/events/realtime.ts:50), so an open browser tab pins its DO active and accrues GB-s duration billing whether or not anyone is typing — cost scales with concurrent open tabs instead of business volume. A single always-on room is ~328,500 GB-s of the 400,000 GB-s included in Workers Paid (~82%); ten rooms open 8h/day x 22 days is ~792,000 GB-s (~+$4.90/mo). Fix: state.acceptWebSocket() plus webSocketMessage/webSocketClose handlers; Cloudflare does not bill duration while a hibernatable socket is idle. Cost detail: src/equityfundingsource-dev/docs/deploy.md 'C — What this costs'. KEPT OPEN 2026-07-18: not started — this cycle was preempted by the operator's live Jeff Blake / Lucas Huang TMO investigation prompt.
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
