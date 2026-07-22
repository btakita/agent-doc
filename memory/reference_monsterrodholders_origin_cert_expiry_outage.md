---
name: reference_monsterrodholders_origin_cert_expiry_outage
description: "monsterrodholders.com \"down\" root cause = expired origin Let's Encrypt cert; certbot-renew.timer was enabled-but-never-started; how to diagnose + fix"
metadata: 
  node_type: memory
  type: reference
  originSessionId: cc6ea9b2-ffc0-493f-a3ef-23445fc7dbbe
---

**Symptom (2026-07-14):** monsterrodholders.com "down" — browser/HTTPS hangs or 500/503 via Cloudflare, plain HTTP works.

**Architecture:** Cloudflare (proxy, edge cert = Google Trust Services, fine) → origin `50.28.2.199` (`ssh monsterrodholders-server`), Apache reverse proxy (`/etc/httpd/conf.d/00-monsterrodholders-proxy.conf`, DocumentRoot `/var/www/html`). Cloudflare talks to origin over 443; Full(-strict) mode rejects a bad origin cert → site down even though the box is healthy.

**Root cause:** origin Let's Encrypt cert (`monsterrodholders.com` + `www`, ECDSA, webroot authenticator) **expired**. Deeper cause: `certbot-renew.timer` was **enabled but never *started*** — `systemctl is-active` = inactive/dead, `journalctl -u certbot-renew.service` = "No entries" for the whole 98-day uptime. Enabled-only units auto-start at *boot*; with no reboot since it was enabled, it never fired once, so the 90-day cert lapsed.

**Diagnosis path:** `openssl s_client -connect 50.28.2.199:443 -servername monsterrodholders.com` → `certificate has expired / notAfter=...`. Confirm with `certbot certificates` (shows INVALID: EXPIRED). Origin port 80 (`curl -H Host:` ) returning 301 while 443 fails = origin-cert leg, not the app.

**Fix applied:** `certbot renew --cert-name monsterrodholders.com --no-random-sleep-on-renew` (webroot; Cloudflare exempts `/.well-known/acme-challenge/`), then `systemctl reload httpd`. Then `systemctl start certbot-renew.timer` (already enabled → now active, next ~05:40 daily) + added deploy hook `/etc/letsencrypt/renewal-hooks/deploy/reload-httpd.sh` (webroot has no installer so it won't reload Apache otherwise). Validated with `certbot renew --dry-run` = all simulated renewals succeeded incl. hook.

**Gotchas:** `certbot renew` over non-TTY ssh inserts a random sleep (up to ~8 min) — use `--no-random-sleep-on-renew` for urgent runs; a hung ssh call can leave certbot holding its lock ("Another instance already running"). The `50.28.2.199.nip.io` self-cert was also expired (2026-07-09) but is not public-facing; the timer renews it automatically. See [[reference_monsterrodholders_ssh]], [[reference_monsterrodholders_waf_933120]].
