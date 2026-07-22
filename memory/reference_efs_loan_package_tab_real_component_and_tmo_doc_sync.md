---
name: reference_efs_loan_package_tab_real_component_and_tmo_doc_sync
description: "EFS admin Deal Pipeline > Loan Package tab is rendered by AdminLoanDocumentsManager (loan_detail.tsx), NOT the orphan loan_package_tab.tsx; plus the #tmo-doc-sync bidirectional TMO document sync"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 6764f3ee-e720-44c8-ab4c-b986139645c6
---

**Gotcha (cost a wasted deploy 2026-07-10):** `lib/ui--browser--efs/portal/admin/loan_package_tab.tsx`
(`AdminLoanPackageTab`) is an **orphan** — nothing imports it, so esbuild tree-shakes it out and its strings
never reach `index.browser-*.js`. The REAL admin Deal Pipeline → Loan Package tab is
`DealDocumentsTab` in `portal/admin/deal_detail.tsx`, which renders **`AdminLoanDocumentsManager`** (defined in
`portal/admin/loan_detail.tsx:919`, also reused standalone in the loan detail view). Per-document cards are
`DocCard` (loan_detail.tsx ~549) inside `AdminLoanUploadedDocumentsSection`. Edit those, not the orphan.
Verify a UI change actually ships by grepping the served `index.browser-<hash>.js` for a unique string BEFORE
declaring done ([[reference_efs_verify_deploy_via_string_grep]]) — a matching content-hash with your string absent = you edited dead code.

**#tmo-doc-sync — bidirectional Loan Package ↔ TMO LOS document sync (shipped 2026-07-10):**
- Migration `0129`: `loan_documents.tmo_attachment_rec_id` + `tmo_synced_at` (idempotency map to the TMO attachment RecID).
- `lib/tmo-documents/sync.ts` `tmo_documents_sync_for_loan_(env,{loan_id,loan_number,direction})`:
  PULL = TMO attachments whose RecID isn't mapped → download + store to R2 + insert `loan_documents` (source `tmo_pull`,
  owner = loan's borrower `user_id` since it's NOT NULL/FK). PUSH = docs with no RecID + R2 content → `AddAttachment`, stamp RecID.
  Additive only — deletes never propagate. Never throws (counts errors).
- API client additions in `tmo-api/index.ts`: `tmo_loan_attachments_R_`, `tmo_loan_attachment_content_R_`,
  `tmo_loan_attachment_add_`, `tmo_bytes_to_base64_`/`tmo_base64_to_bytes_` (chunked). See [[reference_tmo_los_attachment_document_api]].
- Wiring: push-on-upload (waitUntil) in `admin_loans_by_id_documents_POST_`; manual
  `POST /api/admin/loans/:id/documents/sync-tmo` ("Sync with TMO" button) `admin_loans_by_id_documents_sync_tmo_POST_`;
  per-loan reconcile inside `tmo_sync_all_` (bounded to changed loans on delta sweeps). UI: "Sync with TMO" button +
  per-doc "In TMO"/"EFS only" badge in `AdminLoanDocumentsManager`.
- `admin_loan_documents_R_` / `admin_loan_document_by_id_R_` now SELECT the two new columns (added to `admin_loan_document_T`).
- Admin Settings "Sync with TMO Now" toast reports the attachment count: `tmo_sync_counters_T.documents` (pulled+pushed), rendered as "... N loan attachments" (settings.tsx). Verified live 2026-07-10 via chromium-bridge: all EFS loan docs already carry `tmo_attachment_rec_id` (mapped==count per loan), so a steady-state run correctly shows "0 loan attachments" (idempotent — nothing new).
- Tests: `lib/tmo-documents-sync.test.ts` (base64 round-trip, attachment parse/decode, push/pull/idempotent with FakeD1+fake R2+mocked fetch).
  NOTE `loan_documents` has `file_type` not `content_type` — the sync query must not select `content_type` (the test caught this).
