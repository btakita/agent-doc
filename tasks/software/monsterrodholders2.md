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
### Re: site check — https://50.28.2.199.nip.io/

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

❯ The page is not being properly rendered. Look at the home page content.

I see
```
[vc_section][vc_row css=”.vc_custom_1715354722310{padding-left: 30px !important;}”][vc_column width=”1/4″][/vc_column][vc_column width=”3/4″][vc_single_image image=”79596″ img_size=”full” onclick=”custom_link” link=”https://50.28.2.199.nip.io/product/talon-5-35-lh-bargin-bin-sale/”][vc_column_text]Welcome to Monster Rod Holders, the leader in American-made heavy-duty fishing rod holders and catfishing rod holders since 2007. We designed the original 33/45 Rod Holders and many others to suit your fishing style. Discover a wide range of heavy-duty fishing rod holders, rod holder bases, and catfishing accessories.[/vc_column_text][pbr_products title=”Rod Holder Types” categories=”rod-holders” type=”top_rate” columns_count=”4″][pbr_products title=”Rod Holder Bases” categories=”rod-holder-bases” columns_count=”4″ number=”8″ view_more_page=”bestsellers”][pbr_products type=”featured_product” columns_count=”2″][/vc_column][/vc_row][vc_row][vc_column][vc_single_image image=”8782″ img_size=”full” alignment=”center” onclick=”custom_link” css_animation=”effect-v3″ link=”/bestsellers/”][pbr_products title=”Other Monster Rod Holder Products” categories=”boat-accessories” columns_count=”4″ number=”8″ view_more_page=”bestsellers”][product_attribute per_page=”12″ columns=”4″ orderby=”title” order=”ASC” filter=””][/vc_column][/vc_ro[/vc_section]
```
### Re: home page not rendering — WPBakery shortcodes

Found the cause. The home page `entry-content` is all raw WPBakery shortcodes:

```
[vc_section][vc_row][vc_column width="1/4"][/vc_column]
[vc_column width="3/4"][vc_single_image ...][vc_column_text]...[/vc_column_text]
[pbr_products title="Rod Holder Types" categories="rod-holders" ...]
[pbr_products title="Rod Holder Bases" categories="rod-holder-bases" ...]
...
[/vc_section]
```

Without WPBakery plugin, the browser shows the raw shortcode text.

**What's in the backup:**
- `js_composer` (WPBakery) plugin — can be extracted and installed
- `[pbr_products]` shortcode — part of the **fishzone theme's** vc_templates, won't work in Blocksy

**Two options:**

**Option A — Rebuild home page in Blocksy** (recommended — you want a new theme anyway)

The WPBakery content structure is clear: hero image, product grids by category (rod holders, rod holder bases, boat accessories). Rebuild using Gutenberg + WooCommerce product blocks. Clean, no WPBakery dependency.

**Option B — Install WPBakery temporarily**

```bash
# Extract WPBakery from backup (local machine)
mkdir -p /tmp/mrh-plugins
unzip /home/brian/work/rberkes/monsterrodholders/MonsterRodholdersNETNEWcloud-040626-backup.zip \
  "files/wp-content/plugins/js_composer/*" -d /tmp/mrh-plugins

# Copy to server and install
scp -r /tmp/mrh-plugins/files/wp-content/plugins/js_composer root@50.28.2.199:/tmp/
# Then docker cp into container and activate
```

`[vc_*]` layout blocks would render, but `[pbr_products]` product grids still won't (fishzone-specific). So even with WPBakery, products won't show without fishzone.

**Recommendation:** Rebuild with Blocksy/Gutenberg. The page structure is simple — product category blocks replace `[pbr_products]`. Which approach?
❯ Can you access the page source code using the WP api?
❯ How can we get the WP site content into git?
### Re: WP API + content into git

**WP REST API — page source:**

Yes. The REST API returns raw shortcode content. To see page raw content:

```bash
curl -s 'https://50.28.2.199.nip.io/wp-json/wp/v2/pages?per_page=100' | \
  jq '.[] | {id, slug, title: .title.rendered, content: .content.raw}'
```

`content.raw` requires auth (`?context=edit` + application password):

```bash
curl -s -u 'admin:APPLICATION_PASSWORD' \
  'https://50.28.2.199.nip.io/wp-json/wp/v2/pages?context=edit' | \
  jq '.[] | {slug, raw: .content.raw}'
```

**Getting WP content into git:**

WP content = database (posts/pages/settings) + theme files + uploads.

```bash
# 1. Export all content as WXR (WordPress eXtended RSS)
docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp export --dir=/var/www/html/wp-content/exports --filename_format=mrh-export.xml

# 2. Copy export out
docker cp $(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress):/var/www/html/wp-content/exports/mrh-export.xml \
  /home/brian/work/rberkes/monsterrodholders/exports/

# 3. Export DB (for full restore capability)
docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp db export /var/www/html/wp-content/exports/mrh-db.sql

# 4. Theme files
docker cp $(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress):/var/www/html/wp-content/themes/blocksy \
  /home/brian/work/rberkes/monsterrodholders/themes/

# 5. Uploads (use rsync; git-lfs if you want them tracked)
rsync -avz root@50.28.2.199:/opt/monsterrodholders/uploads/ \
  /home/brian/work/rberkes/monsterrodholders/uploads/
```

Suggested git repo layout:
```
monsterrodholders/
├── docker-compose.yml       # infra
├── exports/mrh-export.xml   # WP content (posts, pages, menus)
├── exports/mrh-db.sql       # full DB dump
├── themes/blocksy/          # theme customizations
└── uploads/                 # .gitignore or git-lfs
```

The WXR is re-importable via `wp import mrh-export.xml --authors=create`. DB dump is ground truth for full restore.

Do you want me to set up the git repo structure for `rberkes/monsterrodholders`?

❯ The git repo is at src/monsterrodholders-dev
❯ Can we have git in sync with wordpress?
### Re: git sync with WordPress (src/monsterrodholders-dev)

Repo exists with infra code but no content sync yet. `wordpress/` only has `php.ini`.

**Two directions to sync:**

**1. WP → git (pull content + theme files):**

```bash
# On server: copy active theme out of container
WP_CTR=$(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress)
rsync -avz root@50.28.2.199:/opt/monsterrodholders/app/wp-content/themes/blocksy/ \
  src/monsterrodholders-dev/wordpress/themes/blocksy/
```

Add `scripts/sync-from-wp.sh` to export content:
```bash
#!/usr/bin/env bash
set -e
SERVER=root@50.28.2.199
COMPOSE=/opt/monsterrodholders/app/docker-compose.yml

ssh $SERVER "docker compose -f $COMPOSE run --rm wpcli \
  wp export --dir=/var/www/html/wp-content/exports \
  --filename_format=mrh-export.xml --skip_comments"
scp $SERVER:/opt/monsterrodholders/app/wp-content/exports/mrh-export.xml \
  wordpress/exports/mrh-export.xml

git add wordpress/exports/mrh-export.xml
git diff --staged --quiet || git commit -m "chore: sync WP content export $(date +%Y-%m-%d)"
```

**2. git → WP (deploy theme changes):**

Add `scripts/deploy-theme.sh`:
```bash
#!/usr/bin/env bash
set -e
SERVER=root@50.28.2.199
rsync -avz wordpress/themes/blocksy/ \
  $SERVER:/opt/monsterrodholders/app/wp-content/themes/blocksy/
ssh $SERVER "docker exec \$(docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps -q wordpress) \
  chown -R www-data:www-data /var/www/html/wp-content/themes/blocksy"
```

**Recommended `wordpress/` layout:**
```
wordpress/
├── php.ini                  # already there
├── themes/blocksy/          # git-tracked theme files
├── plugins/blocksy-companion/
└── exports/mrh-export.xml   # WP content WXR
```

Add to `.gitignore`:
```
wordpress/exports/*.sql
wordpress/uploads/
```

Want me to scaffold the scripts and updated `.gitignore` in `src/monsterrodholders-dev` now?
❯ yes
### Re: scaffolded git↔WP sync

Done. Changes in `src/monsterrodholders-dev`:

**`docker-compose.yml`** — added bind mounts for themes/plugins (auto-sync):
```yaml
- ./wordpress/themes:/var/www/html/wp-content/themes
- ./wordpress/plugins:/var/www/html/wp-content/plugins
```
Changes to theme files in the container appear directly in git-tracked dirs and vice versa.

**`scripts/export-content.sh`** — dumps WXR from WP to `wordpress/exports/mrh-export.xml` (run on server when content changes)

**`scripts/import-content.sh`** — imports WXR back into WP (after `git pull` on server)

**`.gitignore`** — excludes `*.sql` dumps and `uploads/`

**`wordpress/themes/`, `wordpress/plugins/`, `wordpress/exports/`** — directories created

**One-time bootstrap on the server** — pull blocksy files out of the volume into the bind-mount path:
```bash
cd /opt/monsterrodholders/app
WP_CTR=$(docker compose ps -q wordpress)
docker cp $WP_CTR:/var/www/html/wp-content/themes/blocksy ./wordpress/themes/
docker compose restart wordpress
```

After that, `wordpress/themes/blocksy/` is git-tracked and live in the container simultaneously.
❯ commit + push
### Re: commit + push

Done. Committed to `btakita/monsterrodholders-dev` and updated parent submodule pointer.

Commit: `feat: add git↔WP sync (bind-mount themes/plugins, export/import scripts)`

Next: bootstrap the theme files on the server so the bind mount picks them up.

❯ The ssh deployment uses a read only gh token.
❯ I want to have the ssh deployment commit all of the pages + any other content that should be committed such as scripts.
❯ Please create a bi-directional process where updates can occur from local git pushed to production or from the wordpress admin back into git.
If wordpress has hooks for commit + push, use those.
Pull + merge or rebase must be explicitly run by script or manually.
### Re: bi-directional sync system

Scaffolded and pushed. Two new scripts + server-setup.md updated.

**`scripts/wp-to-git.sh`** — WP admin → GitHub:
- Exports WXR → `wordpress/exports/mrh-export.xml`
- `git commit` + `git push` using a write deploy key at `/root/.ssh/monsterrodholders_deploy_rw`
- Theme/plugin file changes already appear via bind mount — no extra step needed

**`scripts/git-to-wp.sh`** — GitHub → WP:
- `git pull` (read-only key, works as-is)
- `docker compose restart wordpress` (picks up bind-mounted theme/plugin changes)
- Optional `--import-content` flag to also import WXR

**One-time server setup required:**

1. Generate the write deploy key and add to GitHub with write access:
```bash
ssh-keygen -t ed25519 -C "monsterrodholders-deploy-rw" \
  -f /root/.ssh/monsterrodholders_deploy_rw -N ""
cat /root/.ssh/monsterrodholders_deploy_rw.pub
# → add to github.com/btakita/monsterrodholders-dev/settings/keys with write access checked
```

2. Bootstrap theme files (after `git pull` on server):
```bash
cd /opt/monsterrodholders/app
git pull
WP_CTR=$(docker compose ps -q wordpress)
docker cp $WP_CTR:/var/www/html/wp-content/themes/blocksy ./wordpress/themes/
docker compose restart wordpress
```

**Optional cron** (auto-sync WP → git every 15 min):
```
*/15 * * * * cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh >> /var/log/wp-sync.log 2>&1
```

Full docs in `docs/server-setup.md` § Write deploy key + Bi-directional sync.

❯ How do I make the WP deployment write the files into the git repo?
### Re: making WP deployment write to git

**The challenge:** WordPress runs inside Docker as `www-data` — it can't directly call `git push` with the deploy key. The script must run on the host. Three approaches:

**Option 1 — Cron (already scaffolded, simplest)**
Already in `docs/server-setup.md`. Every 15 min:
```
*/15 * * * * cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh >> /var/log/wp-sync.log 2>&1
```
No WP hook needed. Good enough for most content workflows.

**Option 2 — WP `save_post` hook → signal file → host watcher**

Create a mu-plugin that writes a signal file when content is saved, then a host-side daemon watches it:

```php
# wordpress/mu-plugins/git-sync-trigger.php
<?php
function mrh_signal_git_sync($post_id) {
    if (wp_is_post_revision($post_id) || wp_is_post_autosave($post_id)) return;
    // Signal file in bind-mounted dir, visible on host
    file_put_contents('/var/www/html/wp-content/git-sync-needed', time());
}
add_action('save_post', 'mrh_signal_git_sync');
```

Host-side watcher (systemd service):
```bash
# /opt/monsterrodholders/app/scripts/watch-sync-signal.sh
while true; do
    inotifywait -e close_write /opt/monsterrodholders/app/wp-content/git-sync-needed
    sleep 5  # debounce for rapid saves
    ./scripts/wp-to-git.sh >> /var/log/wp-sync.log 2>&1
done
```

**Option 3 — WP REST API webhook → host endpoint**
WP plugin POSTs to `http://host:PORT/sync` on content save, tiny server runs `wp-to-git.sh`. Overkill for this use case.

**Recommendation:** Start with cron (Option 1) — already documented, just needs `crontab -e` on the server. Add the mu-plugin + signal watcher (Option 2) if real-time sync matters later. Want me to scaffold Option 2?

❯ I edited `crontab -e`.
❯ Can I run the script from the host system to see if the file are committed + pushed?
### Re: running wp-to-git.sh manually

Yes — run it from local via SSH:

```bash
ssh root@50.28.2.199 'cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh'
```

**Prerequisite:** The script needs the write deploy key at `/root/.ssh/monsterrodholders_deploy_rw`. Check if it exists:

```bash
ssh root@50.28.2.199 'ls -la /root/.ssh/monsterrodholders_deploy_rw 2>/dev/null && echo "key exists" || echo "key MISSING"'
```

If missing, generate + add to GitHub:
```bash
# Generate
ssh root@50.28.2.199 'ssh-keygen -t ed25519 -C "monsterrodholders-deploy-rw" \
  -f /root/.ssh/monsterrodholders_deploy_rw -N ""'
# Print pub key → add to github.com/btakita/monsterrodholders-dev/settings/keys (write access)
ssh root@50.28.2.199 'cat /root/.ssh/monsterrodholders_deploy_rw.pub'
```

Then run the script — it'll print each step: export WXR, git add/commit, git push.
```
[root@oddball-business app]# 'cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh'
-bash: cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh: No such file or directory
[root@oddball-business app]# cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh
==> Exporting WordPress content (WXR)...
WARN[0000] Found orphan containers ([app-nginx-1]) for this project. If you removed or renamed this service in your compose file, you can run this command with the --remove-orphans flag to clean it up.
[+]  2/2t 2/22
 ✔ Container app-db-1        Running                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Running                                                                                                                                                                                                       0.0s
Container app-wpcli-run-a3a653647a2d Creating
Container app-wpcli-run-a3a653647a2d Created
Error: The directory '/var/www/html/wp-content/exports' does not exist.
```
### Re: exports directory missing

The exports dir doesn't exist inside the container. Fixed `scripts/wp-to-git.sh` to create it automatically — committed and pushed.

**Immediate fix on the server:**
```bash
cd /opt/monsterrodholders/app
git pull
./scripts/wp-to-git.sh
```

`git pull` gets the updated script (which now runs `mkdir -p` before `wp export`), then re-run.

```
[root@oddball-business app]# cd /opt/monsterrodholders/app && ./scripts/wp-to-git.sh
==> Ensuring exports directory exists...
==> Exporting WordPress content (WXR)...
WARN[0000] Found orphan containers ([app-nginx-1]) for this project. If you removed or renamed this service in your compose file, you can run this command with the --remove-orphans flag to clean it up.
[+]  2/2t 2/22
 ✔ Container app-db-1        Running                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Running                                                                                                                                                                                                       0.0s
Container app-wpcli-run-315d553ee878 Creating
Container app-wpcli-run-315d553ee878 Created
Error: The directory '/var/www/html/wp-content/exports' is not writable.
```
### Re: exports directory not writable

`mkdir -p` created the dir but owned by root — wpcli (running as `www-data`) can't write it. Fixed: script now does `mkdir -p` + `chown www-data:www-data`. Committed and pushed.

On the server:
```bash
cd /opt/monsterrodholders/app && git pull && ./scripts/wp-to-git.sh
```
```
[root@oddball-business app]# cd /opt/monsterrodholders/app && git pull && ./scripts/wp-to-git.sh
Already up to date.
==> Ensuring exports directory exists...
==> Exporting WordPress content (WXR)...
WARN[0000] Found orphan containers ([app-nginx-1]) for this project. If you removed or renamed this service in your compose file, you can run this command with the --remove-orphans flag to clean it up.
[+]  2/2t 2/22
 ✔ Container app-db-1        Running                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Running                                                                                                                                                                                                       0.0s
Container app-wpcli-run-d90191904c15 Creating
Container app-wpcli-run-d90191904c15 Created
Error: The directory '/var/www/html/wp-content/exports' is not writable.
```
### Re: still not writable

Both containers share the `wordpress_data` named volume, so the chown should have worked — but the write check kept failing. Root cause is likely that each `docker compose run` spawns a fresh container, and the writability check in WP-CLI inspects it differently. Simpler fix: export to `/tmp` and pipe stdout out in a single container run — no cross-container volume dependency:

```bash
docker compose run --rm -T wpcli \
  bash -c "wp export --dir=/tmp --filename_format=mrh-export.xml --skip_comments --allow-root && cat /tmp/mrh-export.xml" \
  > wordpress/exports/mrh-export.xml
```

Both export and cat happen in the same container, `/tmp` is always writable. Committed + pushed.

On the server:
```bash
cd /opt/monsterrodholders/app && git pull && ./scripts/wp-to-git.sh
```
<!-- agent:boundary:46f10b2d -->
<!-- /agent:exchange -->

## Pending / Not Built (HEAD)

<!-- agent:pending patch=replace -->
- [ ] On server: `git pull` + re-run `./scripts/wp-to-git.sh` (single-container export fix)
- [ ] Verify commit + push succeeds (need write deploy key — see server-setup.md)
- [ ] On server: bootstrap theme files into wordpress/themes/blocksy/
- [ ] (Optional) Set up mu-plugin + inotify watcher for real-time sync
- [ ] Decision: rebuild home page in Blocksy, or install WPBakery?
- [ ] When ready to go live: search-replace URLs → monsterrodholders.com, flip DNS, re-run certbot
<!-- /agent:pending -->
