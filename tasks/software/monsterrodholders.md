---
agent_doc_session: a9421282-fd96-4943-9af5-3561ed5cb799
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
WordPress migration to Docker on InterWorx/Alma Linux (50.28.2.199). Stack: wordpress:apache on 127.0.0.1:8080, host Apache reverse-proxies via /etc/httpd/conf.d/00-monsterrodholders-proxy.conf. SSL via certbot, real cert files in ./certs/. DB imported, table prefix wp_nxz1mfc9cg_ set in docker-compose env. siteurl+home = https://50.28.2.199.nip.io. HTTPS 200 ✓, HTTP→HTTPS redirect ✓. Browser may cache old 301 to monsterrodholders.com — test in incognito. Orphan app-nginx-1 crash-loops harmlessly. Pending: incognito verify, remove orphan nginx, copy uploads, install new theme.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Verify in incognito window: `https://50.28.2.199.nip.io/` should load WordPress directly
- [ ] Fix crash-looping `app-nginx-1` container (or remove from docker-compose)
- [ ] `wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables`
- [ ] Copy uploads into wordpress container (if not done)
- [ ] Install + activate new theme
<!-- /agent:pending -->
