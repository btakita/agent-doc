---
name: reference_efs_document_type_catalogue_locations
description: Adding an EFS loan document type (dt_*) requires a document_types seed migration + 4 client catalogue lists
metadata: 
  node_type: memory
  type: reference
  originSessionId: ef640b33-022a-498b-a05d-cc012bacbe57
---

EFS loan document types (`dt_appraisal`, `dt_photos`, …) are FK-constrained:
`loan_documents.document_type_id REFERENCES document_types(id)` (migration
`0010_document_types.sql`). Adding a new category (e.g. `dt_additional`
"Additional Documents", 2026-07-08 `0118`) means updating **the DB seed + four
client catalogue mirrors** — they are NOT generated from one source:

1. **Migration** `app/equityfundingsource/migrations/NNNN_*.sql`:
   `INSERT OR IGNORE INTO document_types (id, name, category, sort_order) VALUES (...)`.
   `category` ∈ `required` | `optional` | `executed` (borrower-facing types are
   `optional` per `0109`; only `optional`/`required` show in the borrower step).
2. **Borrower** `lib/ui--browser--efs/portal/borrower/doc_uploader.tsx` →
   `export const DOC_TYPES` (id/name/**icon** Material-Symbol). Single-slot per
   type (`taken_ids`/`existing = docs.find(...)`), so one instance per type.
3. **Admin loan uploads** `lib/ui--browser--efs/portal/admin/loan_detail.tsx` →
   `DOC_TYPE_NAMES` (drives `DOC_TYPE_OPTIONS` selector; admin uploads allow
   multiple of any type).
4. **Deal-packet reclassify** `lib/ui--browser--efs/portal/admin/deal_packet_tab.tsx`
   → `DOC_TYPE_OPTIONS`.
5. **Admin settings default** `lib/ui--browser--efs/portal/admin/settings.tsx` →
   `DEFAULT_SETTINGS.document_types` (id/name/description/category/sort_order).

Only `dt_photos` (`PROPERTY_PHOTOS_TYPE_ID`) is image-restricted
(`file_is_image_`, `accept="image/*"`); every other type accepts any file, so a
new catch-all type needs no upload-guard change. The worker server treats
`document_type_id`/`name` as free-form (no enum) apart from the `dt_photos`
special case. Deploy with migration (`deploy:worker`, not `:no-migrate`) so the
seed applies to prod D1. See [[reference_efs_isolated_deploy_clean_worktree]].

**Admin-defined types + per-deal required docs (#deal-req-docs, 2026-07-08,
migration `0120`, deployed version 97519954).** The client catalogues are no
longer the only source — the deal modal now fetches the LIVE catalogue so
custom types appear everywhere without editing the four mirrors:
- `0120` adds `document_types.default_required` (1 for all seeded except
  `dt_additional`=0), `is_custom`, `created_at`; new table
  `deal_required_document_types(deal_id, document_type_id, required, updated_at)`
  holds per-deal overrides. Effective required = override if present else
  `default_required`. Missing = required type with no non-deleted `loan_documents`
  row of that type for the deal's loan.
- Repo: `app/equityfundingsource/lib/admin/document_requirements/repo.ts`
  (`deal_document_requirements_R_`, `deal_missing_required_documents_`,
  `deal_required_document_type_set_`, `active_deals_document_alerts_R_`,
  `stale_deals_R_` — STALE_DEAL_DAYS=10). `document_type_create_` +
  extended `document_type_T` (`default_required`/`is_custom`) live in
  `application-documents.ts`; the settings delete-guard now spares `is_custom`
  and deal-referenced types.
- Endpoints (admin/routes.ts): `GET|POST /api/admin/document-types`,
  `GET /api/admin/needs-action`, `GET|PUT /api/admin/deals/:id/document-requirements[/:type_id]`.
- UI: deal Documents tab renders `DealDocumentsTab` (Required Documents checklist
  + add-type) via `loan_detail.tsx` `AdminLoanDocumentsManager` (now takes
  `doc_types`/`on_docs_changed`; selects fetch `/api/admin/document-types`).
  Deal dialog shows `DealDocumentsAlert` banner on open when docs missing. The
  pipeline "Next Step" tab is renamed **"Needs Action"** and is now an executive
  dashboard (overview tiles + missing-docs/stalled-deal drill-down) in
  `pipeline.tsx`. Tests: `tests/deal-required-documents.test.ts` (bun:sqlite FakeD1).

**The deal modal "Deal Packet" tab = `active_tab==='documents'` → `DealDocumentsTab`
in `deal_detail.tsx`** (label is "Deal Packet" but tab key/URL `deal_tab` is
`documents`). `deal_packet_tab.tsx` `AdminDealPacketTab` is **DEAD CODE** (defined,
imported nowhere) — do not edit it expecting UI changes.

**Search-first document-type picker (#dpkt-type-search, 2026-07-08, deployed
version f0a0b0d1, bundle ISQT5LX5).** `DocumentTypeCombobox` in `deal_detail.tsx`
replaced the plain "+ Add document type" form in `DealRequiredDocuments`. It
searches the live catalogue (`requirements`, which already carries premade +
custom types with `is_custom`/`required`): pick an existing match → `on_toggle(id,
true)` requires it for the deal; when no exact-name match → inline category select
+ Add → `POST /api/admin/document-types` (existing endpoint) persists a custom type
(`is_custom=1`) reused across every deal. No new migration/backend — storage +
GET/POST already shipped with `0120`. Verified live end-to-end (create→persist→
search-reuse) with a test type deleted from prod D1 after.

**dt_additional dropped from the checklist (#dpkt-drop-additional, deployed
version 3528e83e, bundle FMPWJL5P).** `deal_document_requirements_R_`
(document_requirements/repo.ts) now `.filter(row => row.document_type_id !==
'dt_additional')` — so "Additional Documents" no longer shows in the Required
Documents checklist OR the combobox (both driven by `requirements`). The type
stays in `document_types`, so the upload/reclassify selectors (fed by
`/api/admin/document-types`, the full catalogue) still list it and existing docs
classified under it remain valid. `tests/deal-required-documents.test.ts` updated
(expects `dt_additional` undefined in the checklist).

**CSS gotcha: `.fi { width:100% }`.** In `deal_detail.tsx` the combobox's
"Not found — add" category `<select class="fi">` inherited width:100% and, being
`flex:0 0 auto`, consumed the whole row (text squished to one char/line, Add
button overflowed). Fix: inline `width:auto;min-width:118px` on the select +
`flex-wrap:wrap` on the row + `overflow-x:hidden` on the dropdown. Any `.fi` used
as a flex item needs an explicit width override.
