---
name: reference_efs_pipeline_unknown_status_falls_into_available_deals
description: "EFS admin pipeline silently routes ANY unrecognized loan status into the 'Available Deals' column; production stores 'Application Declined' but the code only knows 'Denied'."
metadata:
  node_type: memory
  type: reference
---

Found 2026-07-30 answering "why do some Available Deals show an Application Denied flag?"

`PipelineDefinition.column_id_for_status_` in
`src/equityfundingsource-dev/app/equityfundingsource/lib/admin/repo.ts` ends with
`return this.default_column_id` — which for `LOAN_PIPELINE` is
`'application-received'`, **labeled "Available Deals"**. So any loan status that no
column claims lands in the entry sales lane instead of failing visibly.

Two production statuses are unclaimed:

- **`Application Declined`** — what TMO actually stores. `LOAN_STATUS_SPECIAL` and
  the `denied` column (label "Application Denied") match only the literal
  `'Denied'`, which has **zero rows** in production D1.
- **`Documents`** — deliberately removed from `LOAN_STATUS_STEPS`, but rows still
  carry it.

Net effect on prod (D1 `7f5c1518-48fe-4ba2-8829-ead63c2a1a3f`, account
`67b0703bb39744866efbf765e1ebcf1c`): 5 of the 8 Available Deals cards are
misrouted (1005/1007/1568 declined, 1560/1561 Documents); only 59, 60, 1011 belong.

**"Available Deals" names two different things.** The admin pipeline column, and the
lender feed `/lenders/available-deals`. The lender feed (`deal_viewable_` /
`deal_visible_to_investor_` in `lib/investor/deal_marketplace/index.ts`) filters on
`investment_deals.status` (`published|reserved|funded|closed`) and **never reads loan
status** — so a pipeline misroute is not lender exposure. Confirm which one is meant
before treating it as a data leak.

Latent hole: `admin_deal_status_sync_by_loan_` maps only `Funded`→`funded` and
`funded`→`published`; every other status hits `ELSE status`. A loan declined AFTER
its deal is published would stay on the lender feed. Not currently triggered — all
declined loans have no `investment_deals` row, because the (untracked, concurrent)
`tmo-deal-sync.ts` defers non-`MARKETPLACE_LOAN_STATUSES` at deal-creation time.

Fix direction: no new column — Parked already contains `on-hold`/`unassigned`/`denied`.
Change the fallback to `unassigned_column_id`, teach the vocabulary the TMO spelling,
and add the terminal-negative unpublish branch.
