---
name: reference_efs_production_logs_admin_token_and_telemetry_query
description: "EFS production Workers Logs ARE queryable — only `btak/equityfundingsource/cloudflare_admin_api_token` has the scope; all three deploy tokens return Cloudflare 10000 on the telemetry endpoints."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9dd22c19-7622-43b4-bf89-4cd9f14119a3
  modified: 2026-07-30T03:46:32.506Z
---

Verified 2026-07-30 against production account `67b0703bb39744866efbf765e1ebcf1c`
(worker `equityfundingsource-production`).

**Token matters, config does not.** On
`POST /accounts/<acct>/workers/observability/telemetry/query`:

| passage path | result |
|---|---|
| `btak/equityfundingsource/cloudflare_admin_api_token` | **works** |
| `btak/equityfundingsource/production/cloudflare_api_token` | code `10000 Authentication error` |
| `btak/equityfundingsource/cloudflare_api_token` | code `10000` |
| `btak/equityfundingsource/cloudflare_infra_api_token` | code `10000` |

An audit that concludes "historical logs are unavailable" has almost certainly used a deploy
token. Check the token before blaming `wrangler.toml`.

**Logs persisted even while the deployed worker reported top-level
`observability.enabled: false`**, because `[observability.logs] enabled/persist = true` was
honored. That contradiction is now fixed and pinned by
`tests/efs-production-environment.test.ts`; the query recipe lives in
`src/equityfundingsource-dev/docs/deploy.md` §2.4b.

**`view: events` is sampled/capped — never count from it.** A `limit: 1000` events query over 7h
returned starts without their matching dones and read like a broken cron. The same window via
`view: calculations` with `{"operator":"count"}` gave 192 vs 192. Counts and group-bys come from
`calculations`; `events` is only for reading individual lines.

Useful `parameters` keys: `datasets: ["cloudflare-workers"]`, `calculations`,
`groupBys: [{"type":"string","value":"$metadata.message"}]` (also `$metadata.level`,
`$metadata.error`, `$workers.eventType`, `$workers.scriptName`), and
`filters: [{"key":"$metadata.level","operation":"eq","type":"string","value":"error"}]`. Note the
`eq` filter works for `calculations` but returned 0 rows under `view: events` — group-by the
message instead to read error text.

Related: [[reference_efs_repo_has_no_ci_workflow]], [[reference_efs_d1_token_working]].
