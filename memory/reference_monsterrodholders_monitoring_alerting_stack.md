---
name: reference_monsterrodholders_monitoring_alerting_stack
description: "monsterrodholders downtime + firewall-drift alerting — boost-health worker (email/SMS via CF send_email), /alert relay, csf-drift-check cron"
metadata: 
  node_type: memory
  type: reference
  originSessionId: cc6ea9b2-ffc0-493f-a3ef-23445fc7dbbe
---

Downtime + firewall-drift alerting for monsterrodholders.com (built 2026-07-14). Fills the gap where outages (cert expiry, DOCKER=1) happened silently — see [[reference_monsterrodholders_origin_cert_expiry_outage]] [[feedback_never_set_csf_docker_1_monsterrodholders]].

**boost-health worker** (`src/boost-client/src/monsterrodholders-dev/workers/health-check/`, account Monsterrodholder74 `1962d02a63d8fea0c675a0db608f81e4`, URL `boost-health.monsterrodholder74.workers.dev`):
- `scheduled()` cron `*/5 * * * *` — probes ORIGIN (`50.28.2.199.nip.io/shop/`+`/wp-json/`, bypasses CF) AND PUBLIC (`monsterrodholders.com/`+`/shop/`) with secret header `x-mrh-monitor: <MONITOR_SECRET>`; re-verifies failures once (preserving the header) before paging. **A Worker CANNOT fetch its own public zone bare** — the zone Bot/WAF Managed Challenge returns 403 (caused a false-positive "MRH DOWN" storm 2026-07-14, fixed same day). Fix = WAF skip rule (see below) gated on the secret header. NOTE: scheduled alerts are NOT rate-limited — real outages re-page every 5 min until resolved.
- **WAF skip rule** (zone `18b38f6bc5d1e24e5a3a6e762f0b1c7a`, custom ruleset `b56e149fffa34614be367b2c6b066101`, rule `a680f4da70894845bd37d05fe976cf76`, at top): skips all bot/challenge phases+products ONLY for `GET` + `host=monsterrodholders.com` + path in {`/`,`/shop/`} + `any(http.request.headers["x-mrh-monitor"][*] eq <secret>)`. Secret = `passage btak/monsterrodholders/mrh_monitor_secret` (also worker secret `MONITOR_SECRET`). Verified: with header→200, without→403 (secret-gated). Edit rulesets with `passage btak/monsterrodholders/cloudflare/CLOUDFLARE_WAF_TOKEN` (FULL token lacks ruleset perm). Rotate: update passage + `wrangler secret put MONITOR_SECRET` + the rule expression.
- **On-box cert-expiry check** `scripts/cert-expiry-check.sh`, cron `/etc/cron.d/mrh-cert-check` daily 06:17, state `/var/lib/mrh-cert-check/`: reads SERVED cert via `openssl s_client 127.0.0.1:443 -servername monsterrodholders.com`, warns (via /alert) if <20 days (LE renews at 30, so <20 = renewal failing). Defense-in-depth vs [[reference_monsterrodholders_origin_cert_expiry_outage]].
- `fetch` handler unchanged (monit still calls it); added `GET /?selftest=1` (send a test alert) and `POST /alert {subject,message}` authenticated relay (Bearer `AUTH_TOKEN`) so on-box jobs page through it.
- Alerts via Cloudflare **Email Routing `send_email` binding** (`[[send_email]] name="SEB"`). Recipients = `ALERT_EMAIL`/`ALERT_SMS` **secrets**, comma-separated (multi-recipient). `ALERT_FROM=alerts@monsterrodholders.com` var.

**Key fact: send_email works with ZERO MX change.** Email Routing stayed `enabled:false`; only *verified destination addresses* are needed — no MX/receiving setup — so Google Workspace inbound mail (`aspmx.l.google.com`) is untouched. Never point MX at Cloudflare for this.

**Recipients** (verify via API `GET /accounts/1962d02a.../email/routing/addresses`): brian email `passage btak/email` (auto-verified, it's the wrangler OAuth login); brian SMS `passage btak/phone`@vtext.com (Verizon, delivering even though API shows verified:false); Ron SMS `passage rberkes/phone`@txt.att.net (AT&T, **pending Ron tapping the CF verification text**). SMS = carrier email-to-SMS gateway (vtext.com / txt.att.net / tmomail.net).

**Auth to configure:** wrangler is OAuth-logged-in as brian.takita@gmail.com with `email_routing:write`+`email_sending:write` (had to `wrangler login` to add those scopes; token at `~/.config/.wrangler/config/default.toml` → `oauth_token`, usable for CF API email-routing calls). Passage CF tokens LACK email-routing scope. Deploy: `wrangler deploy` from the worker dir (account pinned in wrangler.toml).

**csf-drift-check.sh** (`scripts/csf-drift-check.sh`, cron `/etc/cron.d/mrh-csf-drift` every 10 min, state in `/var/lib/mrh-csf-drift/`): (1) auto-reverts `DOCKER="1"→"0"` in csf.conf **file-only, never `csf -r`** (can't itself flush NAT); (2) runs `check-firewall-stack.sh` read-only and pages `POST /alert` on drift, rate-limited hourly. Reads worker URL+token from `/etc/sysconfig/mrh-healthcheck`.

**Test:** `curl -H "Authorization: Bearer <AUTH_TOKEN>" 'https://boost-health.monsterrodholder74.workers.dev/?selftest=1'` (token from `/etc/sysconfig/mrh-healthcheck` on box).

**Uncommitted as of 2026-07-14:** worker src + wrangler.toml + csf-drift-check.sh are deployed/installed but not yet committed to the monsterrodholders-dev submodule; `operations-monitoring.md` still describes the worker as origin-only. Commit + doc-update owed.
