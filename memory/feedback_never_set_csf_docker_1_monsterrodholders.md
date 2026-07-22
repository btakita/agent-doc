---
name: feedback_never_set_csf_docker_1_monsterrodholders
description: "NEVER set CSF DOCKER=\"1\" on the monsterrodholders VPS — it flushes Docker's NAT chain and takes the site down"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cc6ea9b2-ffc0-493f-a3ef-23445fc7dbbe
---

On the monsterrodholders Liquidweb VPS (`ssh monsterrodholders-server`, `50.28.2.199`), **never set `DOCKER = "1"` in `/etc/csf/csf.conf`.** It must stay `DOCKER = "0"`.

**Incident (2026-07-14 11:33:19 ET):** a Claude session set `DOCKER="1"` in csf.conf → outage. Evidence: `/etc/csf/csf.conf.bak.20260714-1535-docker1` (content mtime 11:33:19, only diff from current is the DOCKER line). Reverted to `DOCKER="0"`; site recovered. This was a *different* session than the concurrent one that fixed the expired TLS cert — two independent outage causes the same day. [[reference_monsterrodholders_origin_cert_expiry_outage]]

**Why:** with `DOCKER="1"`, CSF/LFD takes ownership of the iptables NAT ruleset and rebuilds/flushes it on `csf -r`, wiping Docker's `DOCKER` NAT chain and the `127.0.0.1:8080 → container:80` DNAT rule the Apache reverse proxy depends on. Origin becomes unreachable = same mechanism as the documented May-4 RCA. Docker must own its own NAT.

**How to apply:** The site-specific CLAUDE.md already mandates `DOCKER="0"` — obey it. Treat any CSF/firewall edit on this box as production-critical and confirm before running. Verify with `cd /opt/monsterrodholders/app && ./scripts/check-firewall-stack.sh` after any firewall/Docker/host-security change; it validates `DOCKER="0"` and the Docker NAT chain. Concurrent Claude sessions operate this workspace — assume another session may be touching the same host. [[reference_monsterrodholders_ssh]] [[feedback_concurrent_session_clobbers_efs_worker_deploy]]

**Residual egress damage (diagnosed 2026-07-15):** a DOCKER=1 flush wipes ALL Docker iptables rules, but recovery often only rebuilds the INBOUND `DOCKER` DNAT chain (site serves) while leaving `FORWARD` policy `DROP` with no Docker ACCEPT rules and no POSTROUTING MASQUERADE for `172.18.0.0/16`. Symptom: containers reach the host (`50.28.2.199`/nip.io) but NOT the internet — `docker exec app-wordpress_green-1 curl https://www.google.com` times out. This silently breaks WordPress self-loopback (wp-cron/Site Health) → admin.php hangs to the 60s proxy `Timeout` → **wp-admin 500s**; also breaks WooCommerce outbound (PayPal/Stripe/ShipStation). `check-firewall-stack.sh` does NOT catch it (it only checks the inbound chain). Fix = restore Docker egress: `iptables -t nat -A POSTROUTING -s 172.18.0.0/16 ! -o <br-bridge> -j MASQUERADE` + 3 `FORWARD` ACCEPTs (`-i br ! -o br`, `-o br ctstate RELATED,ESTABLISHED`, `-i br -o br`). Persisted durably in `/etc/csf/csfpost.sh` (idempotent, bridge derived from the `172.18.0.1` gateway). Canonical alt: `systemctl restart docker` (rebuilds all Docker rules, but restarts containers = brief outage). **The csf-drift auto-revert flips DOCKER back to 0 but does NOT rebuild NAT — a DOCKER=1 event still needs this egress repair.**
