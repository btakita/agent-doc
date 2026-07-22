---
name: reference_efs_per_lender_deal_packet_and_message_lender
description: EFS Loan Package per-lender document visibility was REMOVED 2026-07-10 (all interested lenders see all Required Documents); Message Lender button + admin unread cursor
metadata: 
  node_type: memory
  type: reference
  originSessionId: 587e3e07-ca71-4826-8108-182e00f91b9c
---

**RENAMED 2026-07-10 (commit c312d0f, deployed):** the "Deal Packet" feature is now **"Loan Package"** in all UI text, code identifiers, the module dir (`lib/deal_packet` → `lib/loan_package`), the admin route (`/api/admin/deal-packets` → `/api/admin/loan-packages`), and the DB table (`deal_packet_links` → `loan_package_links` via forward migration 0128). UNCHANGED: the opaque public path `/dp/<token>`, the `?tab=packet` query value (emailed deep links depend on it), applied migrations, and `#dpkt` annotation tags. STILL PENDING (distinct "deal package" send-workflow term, NOT yet renamed): `package_sent_at` DB columns (investor_deal_interests + lender_deal_interests), event kind `deal_package_sent`, and user-facing strings "Send deal package" / "Deal package sent" / the Settings "Deal Package Template" feature — a separate pass with schema implications. Below refs use the OLD names; substitute `loan_package_links` and `AdminLoanPackageTab`.

**PER-LENDER VISIBILITY REMOVED 2026-07-10 (commit 4dab80c, deployed version 042be228).** All interested lenders who can see a deal now see **every** Required Document. Property photos still gated by `loan_documents.lender_visible` (marketplace preview flag). What changed:
- Investor read gate (`investor_deals_by_id_documents_GET_` + `..._zip_GET_`, deal_marketplace/index.ts): now `doc.document_type_id !== BORROWER_PROPERTY_PHOTO_TYPE_ID || doc.lender_visible === 1`. The `deal_document_lender_visible_ids_R_` / `packet_visible_ids` path is GONE — do NOT re-add it.
- Admin UI (deal_detail.tsx): removed the Required Documents row expand chevron + `DealRowVisibilityPanel` + `DealLenderVisibilitySwitch`, and the Message Lender modal's per-file `LoanPackageFileChecklist`.
- STILL PRESENT but now UNUSED/dead (not read by the portal): table `deal_document_lender_visibility` (mig 0124), table `deal_document_type_lender_hidden` (mig 0127), route `GET/PUT /api/admin/deals/:id/document-visibility`, repo `deal_document_lender_visibility_*` fns, and the grant-on-interest call in investor/shared.ts. Left in place to avoid schema churn; safe to remove in a later cleanup.
- **Property photos (`document_type_id='dt_photos'`) stay marketplace-default** via `loan_documents.lender_visible` (trigger from mig 0115).

**Message Lender** per interested-lender row (#msg-lender-unread): button opens the global Messages dialog scoped by lender email (works for non-selected lenders), and marks read via `POST /api/admin/deal-interests/:id/lender-messages/read`. Unread badge = `unread_lender_message_count` (lender-authored messages newer than `lender_deal_interests.admin_lender_messages_read_at`, mig 0125), computed in `deal_interest_workflow_summary_`.

See [[reference_efs_funding_commit_stage_removed]] and [[reference_efs_deploy_from_main_checkout_not_fresh_install_worktree]].
