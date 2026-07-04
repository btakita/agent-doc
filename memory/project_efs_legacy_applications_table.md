---
name: project_efs_legacy_applications_table
description: "equityfundingsource-dev prod D1 has a legacy `applications` table that collides with new migrations; apply4 uses full_applications"
metadata: 
  node_type: memory
  type: project
  originSessionId: 3a21b72b-215c-4346-bb3c-22f199b00869
---

The production D1 for `equityfundingsource-dev` (DB `equityfundingsource`, id `280e1539-b940-4c59-8988-fbed63c12ac0`) contains a **legacy `applications` table** that is NOT created by any repo migration and is not referenced by current code (the live app uses `application_leads` + `loans`). Its schema is the old public-intake shape: `id INTEGER AUTOINCREMENT, borrower_name, email, phone, address…, loan_amount, occupancy, lien_position, status IN (pending/reviewed/approved/declined)`.

A new migration that does `CREATE TABLE applications` fails on `cf:d1:migrate:remote` with `table applications already exists [code: 7500]` — even though it applies cleanly on a fresh local D1. The apply4 full-application build therefore uses table name **`full_applications`** (migration 0036, with `application_personal/addresses/employment/income/signatures` FK-ing it).

**How to apply:** never reuse the bare name `applications` for a new EFS table; don't DROP the legacy table (not ours, may hold old data) — work around it. When adding EFS migrations, prefer prefixed/unique names and confirm against prod `sqlite_master` if unsure. Query prod via the wrapper: `bun ./bin/cf-wrangler.ts d1 execute equityfundingsource --remote -c ../../wrangler.toml --command "<sql>"` (raw `bunx wrangler` mis-formats the API-token header; the wrapper loads `.env` and works). Relates to [[reference_resume_build_source]]-style EFS ops.
