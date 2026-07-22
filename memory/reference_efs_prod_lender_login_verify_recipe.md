---
name: reference_efs_prod_lender_login_verify_recipe
description: "Verify EFS lender-facing prod changes by curl-logging-in as a btak.dev lender and hitting /api/investor/deals (no browser, no secret in argv)"
metadata: 
  node_type: memory
  type: reference
  originSessionId: b898c81d-532c-4a2a-93fe-2cc56c6bff22
---

To verify a lender-facing change on **live prod** end-to-end (auth + real data),
log in with curl as a real lender and call the same API the UI consumes — no
browser, no cleartext secret in argv:

```
BASE="https://equityfundingsource.brian-takita.workers.dev"
CJ=$(mktemp); PWF=$(mktemp); chmod 600 "$PWF"
passage btak/lucas-collab-password | tr -d '\n' > "$PWF"        # tr -d '\n' is REQUIRED
curl -s -c "$CJ" -L \
  --data-urlencode "email=btak.dev+lender2@gmail.com" \
  --data-urlencode "password@$PWF" "$BASE/login" -o /dev/null    # 200 + efs_session cookie set
curl -s -b "$CJ" "$BASE/api/investor/deals" | jq ...             # authenticated payload
```

**Gotchas learned (2026-07-10):**
- `--data-urlencode "password@file"` includes the file's trailing newline, so a
  `passage ... > file` (which appends `\n`) submits the wrong password → **401**
  "Email or password is incorrect." Always `| tr -d '\n'`. The stored hash is
  fine; the 401 is a test artifact.
- `POST /login` gates: wrong pw → **401**; `status='suspended'` → 403; role-query
  mismatch → 403; **`email_verified_at IS NULL` → 403** (resends verification, no
  session). Pick a **verified active** lender: `SELECT ... WHERE role='lender'
  AND status='active' AND email_verified_at IS NOT NULL`.
- The btak.dev+lender* accounts' password is the shared `passage
  btak/lucas-collab-password` (see [[feedback_efs_commit_only_your_files_via_temp_index_compare_and_swap]]
  tooling `bin/rename-accounts.ts`).

**Marketplace deal facts (for spoken-for / status checks):** a loan's marketplace
row is `investment_deals` (join on `loan_id`); its `status` ∈
published/reserved/funded/closed drives the API's `deal.status`. `loans.status`
(e.g. `submitted`) is a DIFFERENT field. `/api/investor/deals` emits `status`,
`investor_fund_status`, `selected_for_commitment`, `interested`.

**Lender dashboard "Available Deals" preview** = first 3 of the browseable deals
(`investor_fund_status != 'active'`, then not pending/selected/interested,
`.slice(0,3)`). #spoken-for badge on both the dashboard preview and
/lenders/available-deals: `spoken_for = !pending && status === 'reserved'` where
`pending = investor_fund_status === 'pending' || selected_for_commitment`.
Relates to [[reference_efs_local_portal_drive_recipe]] (local equivalent),
[[reference_efs_verify_deploy_via_string_grep]].
