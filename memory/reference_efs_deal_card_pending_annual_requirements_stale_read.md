---
name: reference_efs_deal_card_pending_annual_requirements_stale_read
description: "Deal Pipeline card badge showed false 'N pending annual requirements' — deal-side compliance reads used stale users.accreditation_status; fix = COALESCE(acc.status, u.accreditation_status)"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 43117879-0f37-445e-a145-df43f9b0b444
---

**Symptom:** `/admin/pipeline` deal cards showed "N pending annual requirements" for lenders whose requirements were actually complete in the Lender Pipeline (e.g. btak.dev+lender3@gmail.com).

**Root cause (same class as [[reference_efs_accreditation_denormalization_and_closing_gate_removed]]):** the deal-side lender-compliance reads used the STALE denormalized column `u.accreditation_status`, while the Lender Pipeline board reads the LIVE latest verification via `COALESCE(acc.status, u.accreditation_status)`. When a lender's latest `lender_accreditation_verifications` row is `verified` but `users.accreditation_status` was never updated, the deal side computed `under_review`/`not_started` → `investor_requirements_blocks_funding_` → phantom hold badge.

**Fix (deployed 2026-07-09, worker Version 6e9f06cb):** switched three deal-side reads to `COALESCE(acc.status, u.accreditation_status) AS accreditation_status`:
- `deal_lender_compliance_by_loan_R_` (both UNION halves) — `app/equityfundingsource/lib/admin/loan_management/index.ts`
- `deal_selected_lender_requirements_R_` — `app/equityfundingsource/lib/admin/repo.ts`

Note: the board COLUMN "Pending Annual Requirements" is a *different* thing — a real persisted `loans.tmo_loan_status = 'On Hold'`, set only by manual drag, with no auto-exit. The user's complaint was the per-card BADGE, not the column.

Same session added the lender-scoped `POST/GET /api/admin/users/:id/lender-requirements-email` ("Request Lender Requirements by Email" + last-sent) backed by `users.lender_requirements_email_sent_at` (migration 0123), a deal-modal notification that deep-links to `/admin/lender-pipeline?tab=pipeline&dialog=lender&dialog_user_id=<id>`, and `selected_lenders` on pipeline cards. Deployed via stash-in-main isolation per [[reference_efs_isolated_deploy_clean_worktree]].
