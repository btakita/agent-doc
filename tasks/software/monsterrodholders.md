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

❯ Why is https://50.28.2.199.nip.io an empty site? I just restored the wordpress site without the theme. Just the data and the uploads.
❯ Can I extract the basic theme without extra plugins.
❯ Backup is at /home/brian/work/rberkes/monsterrodholders/MonsterRodholdersNETNEWcloud-040626-backup.zip
### Re: empty site + extracting theme from backup

**Why empty:** The theme wasn't copied — only DB + uploads were restored. WordPress has no active theme files, so it shows an empty/broken site. The DB still has `fishzone` set as the active theme, but the files aren't in the container.

**Themes in the backup:** `fishzone` (original), `astra`, `twentyfifteen`.

Since you want a new theme — `astra` is already in the backup and is a clean lightweight WooCommerce theme. No extra plugins needed. Or keep `fishzone` if you want the original look (may depend on WPBakery/Elementor).

Extract and deploy:

```bash
# 1. Extract theme(s) from backup (local machine)
mkdir -p /tmp/mrh-themes
unzip /home/brian/work/rberkes/monsterrodholders/MonsterRodholdersNETNEWcloud-040626-backup.zip \
  "files/wp-content/themes/*" -d /tmp/mrh-themes

# 2. Copy theme to server
scp -r /tmp/mrh-themes/files/wp-content/themes/astra root@50.28.2.199:/tmp/astra

# 3. On server — copy into container and fix permissions
WP_CTR=$(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress)
docker cp /tmp/astra $WP_CTR:/var/www/html/wp-content/themes/
docker exec $WP_CTR chown -R www-data:www-data /var/www/html/wp-content/themes/astra

# 4. Activate via WP-CLI
docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp theme activate astra
```

Replace `astra` with `fishzone` if you prefer the original. Do you want both or just astra?

❯ I just got the theme. How do I use this theme instead?
❯ /home/brian/work/rberkes/monsterrodholders/blocksy-companion-pro.2.1.38.zip
### Re: blocksy-companion-pro.2.1.38.zip

That's the **Blocksy Companion Pro plugin** — not the theme itself. The Blocksy theme is separate and free on WordPress.org. The companion plugin adds pro features on top of the theme.

**Install order:**

**Step 1 — Install the Blocksy theme** (free, from WordPress.org):
```bash
docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp theme install blocksy --activate
```

**Step 2 — Copy companion plugin to server:**
```bash
scp /home/brian/work/rberkes/monsterrodholders/blocksy-companion-pro.2.1.38.zip \
  root@50.28.2.199:/tmp/
```

**Step 3 — Install + activate companion plugin on server:**
```bash
WP_CTR=$(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress)
docker cp /tmp/blocksy-companion-pro.2.1.38.zip \
  $WP_CTR:/var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip
docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp plugin install /var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip --activate
```

**Step 4 — Enter license key** in WP Admin → Blocksy → License (unlocks pro features). Do you have the license key?

❯ Take a look at https://50.28.2.199.nip.io/
### Re: site check — https://50.28.2.199.nip.io/ (HEAD)

Site is up and working:

- HTTP 200 — no redirect to `monsterrodholders.com`
- **Blocksy theme is active** (`blocksy/static/bundle/main.min.css?ver=2.1.38` loading)
- Title: "Monster Rod Holders – We are the reference for Heavy-Duty Fishing Rod Holders..."
- All URLs correctly use `50.28.2.199.nip.io`

Blocksy theme is already installed and serving content. If you want to also install the Blocksy Companion Pro plugin (step 2–4 from above), that unlocks pro customization features — but the site is functional without it.

Next steps (when ready to go live):
1. Run `wp search-replace 'https://50.28.2.199.nip.io' 'https://monsterrodholders.com' --all-tables`
2. Flip DNS to `50.28.2.199`
3. Run certbot for `monsterrodholders.com` SSL
<!-- agent:boundary:e9c8f575 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] (Optional) Install blocksy-companion-pro plugin + license key
- [ ] Verify products/WooCommerce pages load correctly
- [ ] Remove crash-looping `app-nginx-1` orphan container
- [ ] When ready to go live: search-replace URLs → monsterrodholders.com, flip DNS, re-run certbot
<!-- /agent:pending -->
