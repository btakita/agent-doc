# Plan — TMO tightened-polling sync (#tmopoll)

Operator decision (2026-06-13): TMO (The Mortgage Office) exposes no webhook or
streaming API, so EFS will **poll** TMO instead of waiting on push. This is the
keystone that unblocks the writeback-derived pipeline work (#vd8s Active-lane
AddLoanFunding sync, #67v2 auto-advance-on-requirements, #zwws phase 4
vestings/special-instructions, #7a4d OCR/TMO field writeback).

## Scope

A scheduled Cloudflare Worker cron that reconciles TMO → EFS D1:

1. **Poller** — cron-triggered handler that calls the TMO read endpoints EFS
   already uses (loans, funding/AddLoanFunding state, payment schedule reads).
2. **Diff** — compare each fetched record against its D1 mirror; detect new,
   changed, and (where TMO supports it) removed records.
3. **Reconcile** — upsert changed records into D1; record a per-run sync log
   (records scanned, changed, errors) for admin visibility.
4. **Backoff/observability** — tighten the interval where freshness matters,
   widen it elsewhere; surface last-sync time + drift count in the admin UI.

## Phases (one backlog id per phase when queued)

- **P1** Poller + sync-log table + cron wiring (read-only mirror; no writeback to TMO).
- **P2** Diff/reconcile into the loan + funding D1 tables; admin last-sync surface.
- **P3** Wire the reconciled funding state into the Investor Pipeline Active lane (#vd8s) and loan-pipeline auto-advance (#67v2).

## Gates / open questions

- Confirm the exact TMO read endpoints + auth available to EFS (the six-question
  prompt #k2xg is deferred — proceed on documented read endpoints).
- Poll interval vs TMO rate limits.
- TMO → EFS is read/mirror only here; EFS → TMO writeback (AddLoanFunding) is a
  separate gate (#dzmm/#k2xg) and out of scope for this plan.

## Verification

- SimWorld/unit coverage for the diff/reconcile logic against fixture TMO payloads.
- `bun run check` green; smoke-test the cron handler locally before deploy.
