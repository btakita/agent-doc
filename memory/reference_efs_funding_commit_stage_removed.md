---
name: reference_efs_funding_commit_stage_removed
description: EFS lender self-serve funding-commitment stage REMOVED — selected lender wires the entire loan; selection emails the lender
metadata: 
  node_type: memory
  type: reference
  originSessionId: f2dae47f-8491-4295-bd5f-15332982ea1b
---

Deployed 2026-07-10 (EFS Worker version faa4ff98, commit e4127f5). The lender
**self-serve funding-commitment stage was removed** (`#funding-commit-stage-removed`).

- **Gone:** `POST /api/investor/deals/:id/funds` (route + handler `investor_deals_by_id_funds_POST_`) and the lender `CommitFundingForm` (amount/vesting/beneficiary entry). Lenders no longer submit an amount or create a `funds` row.
- **New lender flow:** EFS **selects** the lender → EFS collects docs off-platform → EFS **assigns the loan's wire numbers** (`loan_wire_instructions`, gated by `loan_wire_instructions_assigned_` = routing + account present) → the selected lender wires the funds for the **ENTIRE loan**.
- **Lender portal panel** = `SelectedLenderFunding` (`portal/investor/deal_detail.tsx`): if `deal.wire_instructions_ready` → reveals wire instructions UI; else → "You've been selected; EFS will send wire instructions".
- **Wire reveal** (`investor_deals_by_id_wire_instructions_reveal_POST_`) re-gated on `interest.selected_at` + wire numbers assigned (NOT on a fund row); shows the entire loan amount from per-loan `wire_fields_from_instructions_`.
- **Selection now emails the lender**: `admin_deal_interests_by_id_select_PUT_` → `notify_lender_deal_selected_` ("You've been selected to fund …") on every new selection, independent of the due-diligence package.
- **Funds records** are now created only admin/TMO-side: `PUT /api/admin/funds/:id/confirm-wire` (activates an existing fund) or the hourly TMO pull / unmatched-funding bind. See [[reference_efs_deal_card_pending_annual_requirements_stale_read]] for the `funds` table context.
- Also completed the investor side of `#lender-preview-docs`: retired admin `deal_photos` from lender preview (`investor_merged_photos_by_deal_` + deal-detail GET now use borrower property photos only).
- Follow-on (commit 895584a, Worker version 9b7e42e6): removed the **"Funding Commits" tab from the admin deal modal** (`portal/admin/deal_detail.tsx`, `ActiveTab` no longer has `'funds'`). Funds are now managed only in the **Closing tab** (confirm-wire via `ClosingWorkflowPanel`) and the standalone `/admin/funding` page (`AdminFundingCommits`). The deal modal header now shows **"Selected lender(s): <names>"** when `selected_interests` is non-empty.

Doc: `docs/funding-commit-lifecycle.md`. Deploy handled a concurrent session's uncommitted `#dpkt` WIP (deal_packet/routes.ts, admin/loan_detail.tsx) by stashing those two, committing the scoped set (incl. the coupled shared email-footer-logo template work), deploying from the main checkout, then popping the stash. See [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]], [[reference_efs_verify_deploy_via_string_grep]].
