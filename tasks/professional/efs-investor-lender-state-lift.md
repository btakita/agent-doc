# EFS — Lift lender-application state across the dashboard ↔ deal-dialog boundary

## Problem

Submitting the lender account application inside the **dashboard** Available-deal
dialog does not transition the dashboard's outer `LenderApplicationNotice`
("incomplete" → "awaiting EFS review") or the deal cards. The transition only
works on the standalone `/portal-investor/available-deals` page.

Root cause: there is no shared client-side model of `InvestorLenderProfile`.
Three components each hold an **independent** `lender`/`investor` `useState`,
loaded once from `/api/investor/deals` (or `/api/investor/deals/<id>`):

- `available_deals.tsx::AvailableDeals` — `lender` (notice + gate share it → works here)
- `dashboard.tsx::AvailableDealsPreview` — `lender` (notice + cards; never updated after mount)
- `deal_detail.tsx::InvestorDealDetail` — `investor` (dialog gate; `on_change={set_investor}` updates only this copy)

On the dashboard the dialog's `InvestorDealDetail` flips its own copy to
`pending`, but nothing lifts that change to `AvailableDealsPreview`, and closing
the dialog does not refetch.

Note: `lender_request_status` (account-level) is distinct from per-deal card
state (`interested`, `investor_fund_status`). The lender application is
account-global, so there is currently **no** per-deal "applied" card state — the
notice is the thing that transitions, not the card. If the user wants a card-level
indicator, that is a new state to design, not a wiring fix.

## Options

1. **Lift state up (smallest fix).** `InvestorDealDetail` accepts an optional
   `on_lender_change?: (next: InvestorLenderProfile) => void`; `AvailableDealDialog`
   threads it; `AvailableDealsPreview` passes `set_lender`. Then the dialog's
   `on_change` calls both `set_investor` and `on_lender_change`. Dashboard notice
   reacts immediately. (Cards still won't change — there's no card-level lender state.)
2. **Refetch-on-close.** `AvailableDealsPreview` re-runs its `/api/investor/deals`
   fetch when the dialog closes (or on `visibilitychange`). Simpler, but a network
   round-trip and a flash.
3. **Shared store (preact signal / context).** One `InvestorLenderProfile` signal
   consumed by all three components. Most robust, biggest change; fixes every
   future divergence.

Recommended: Option 1 for the immediate notice transition; consider Option 3 if
more cross-component lender-derived state appears.

## Acceptance

- Submitting the application in the dashboard dialog transitions the dashboard's
  `LenderApplicationNotice` to "awaiting EFS review" without a page reload.
- Test in `tests/lender-account-gate.test.ts` (or a dashboard test) asserting the
  lifted callback fires and the dashboard notice heading updates.
