---
name: reference_efs_d1_token_working
description: EFS prod D1 is queryable now — passage btak/equityfundingsource/cloudflare_api_token is D1-Edit scoped;
metadata: 
  node_type: memory
  type: reference
  originSessionId: 8e1fc0d8-3b83-4df8-bb27-e66629708ed2
---

`passage btak/equityfundingsource/cloudflare_api_token` is an **active, D1-Edit-scoped** Cloudflare token (verified 2026-06-20: token-verify active + D1 db-list + `SELECT` query all succeed against account `1e7e194ff8da4ad9e40ce041aea84f76` / DB `280e1539-b940-4c59-8988-fbed63c12ac0` = the `equityfundingsource` D1). The D1 `/query` HTTP API requires Account D1 Edit, so write scope is confirmed.

`src/equityfundingsource-dev/.env` line 7 **already** references it: `CLOUDFLARE_API_TOKEN=passage:btak/equityfundingsource/cloudflare_api_token`. So **#cftk is RESOLVED** — do NOT re-treat EFS prod D1 as blocked. The earlier #cftk-blocked memory (no D1 access, 10000/wrong-acct errors) is stale; the token was re-scoped since.

Query prod D1 safely (no token in stdout/ps): write the bearer to a temp file and `curl --config`:
```
HDR=$(mktemp); trap 'rm -f "$HDR"' EXIT
printf 'header = "Authorization: Bearer %s"\n' "$(passage btak/equityfundingsource/cloudflare_api_token)" > "$HDR"
curl -s --config "$HDR" -H "Content-Type: application/json" -X POST \
  "https://api.cloudflare.com/client/v4/accounts/1e7e194ff8da4ad9e40ce041aea84f76/d1/database/280e1539-b940-4c59-8988-fbed63c12ac0/query" \
  -d '{"sql":"SELECT ..."}' | jq '.result[]?.results[]?'
```
This unblocks D1-gated investigations like #dealtab500 and #cmtsec. Relates to [[project_efs_legacy_applications_table]].
