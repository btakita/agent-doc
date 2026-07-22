---
name: reference_efs_accreditation_denormalization_and_closing_gate_removed
description: EFS closing/closed lender-annual-requirements gate REMOVED (2026-07-08); root cause was stale denormalized users.accreditation_status.
metadata: 
  node_type: memory
  type: reference
  originSessionId: 4b3f00a0-a245-4f2e-888b-17890c01ee7f
---

**Closing/Closed lender-annual-requirements gate REMOVED** (2026-07-08, deployed version 1aca286a). Brian: "no blockers" moving a deal to Closing/Closed. Removed `LOAN_CLOSING_GATED_STATUSES`, `loan_closing_blocked_by_lender_requirements_`, `loan_closing_lender_requirements_block_R_`, `LoanClosingLenderRequirementsError`, `is_loan_closing_lender_requirements_error_`, the choke-point throw in `admin_loan_status_update_` (repo.ts), and 4 callsites in loan_management/index.ts (status PUT, close PUT, closing-doc auto-advance, batch auto-advance catch). KEPT `deal_selected_lender_requirements_R_` + `investor_requirements_*` (used by email nudge + dialog display). admin-pipeline.test.ts gate block replaced with a regression guard asserting the gate is gone.

**Root cause of "requirements met but state didn't propagate"** (deal 29351553-...): the gate and the dialog read the lender's requirements status from DIFFERENT sources:
- Gate `deal_selected_lender_requirements_R_` read **`users.accreditation_status`** (a DENORMALIZED mirror) → stale `'pending'`.
- Dialog `deal_interest_admin_responses_by_deal_R_` read **`acc.status` from the latest `lender_accreditation_verifications`** (normalized/live) → `'verified'`.
So dialog showed "Current/met" while the gate blocked on the stale `'pending'`. Both feed `investor_requirements_status_()` but with different inputs. The denormalized `users.accreditation_status` was never updated when the verification went `verified`. Also a tie-break bug: that lender has 2 verification rows with identical `created_at` (one verified, one pending), so `ORDER BY created_at DESC LIMIT 1` is nondeterministic — add a secondary key (id/rowid) if reintroducing.

**N-query question:** no N+1 exists. Board (`admin_dashboard_*`) doesn't compute per-deal requirements at all; the gate read one loan (never N); dialog batches all interests for a deal in one query via a correlated latest-verification subquery. Any future per-deal requirements badge on the board should be a single JOIN with that subquery, not a per-deal call.

**Deeper latent fix (not done):** `users.accreditation_status` going stale vs the latest verification affects anything else reading that column; canonical source is the latest `lender_accreditation_verifications` row.

Relates to [[reference_efs_deploy_from_main_checkout_not_fresh_install_worktree]], [[project_efs_legacy_applications_table]].
