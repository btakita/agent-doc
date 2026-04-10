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

❯ I'm using monsterrodholders_deploy instead of monsterrodholders_deploy_rw.
❯ commit + push
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

❯ I'm using monsterrodholders_deploy instead of monsterrodholders_deploy_rw.
❯ commit + push
### Re: deploy key rename + commit + push

Script already used `monsterrodholders_deploy` in `GIT_SSH_COMMAND` — but the comment header and `docs/server-setup.md` still referenced `_rw`. Cleaned up both. Committed and pushed:

```
004725d fix: rename deploy key from monsterrodholders_deploy_rw to monsterrodholders_deploy
```

The server should now be able to run `./scripts/wp-to-git.sh` and push successfully with the key at `/root/.ssh/monsterrodholders_deploy`.
```
[root@oddball-business app]# cd /opt/monsterrodholders/app && git pull && ./scripts/wp-to-git.sh
remote: Enumerating objects: 11, done.
remote: Counting objects: 100% (11/11), done.
remote: Compressing objects: 100% (1/1), done.
remote: Total 6 (delta 4), reused 6 (delta 4), pack-reused 0 (from 0)
Unpacking objects: 100% (6/6), 504 bytes | 504.00 KiB/s, done.
From github-monsterrodholders:btakita/monsterrodholders-dev
   b2f9076..004725d  main       -> origin/main
hint: You have divergent branches and need to specify how to reconcile them.
hint: You can do so by running one of the following commands sometime before
hint: your next pull:
hint:
hint:   git config pull.rebase false  # merge
hint:   git config pull.rebase true   # rebase
hint:   git config pull.ff only       # fast-forward only
hint:
hint: You can replace "git config" with "git config --global" to set a default
hint: preference for all repositories. You can also pass --rebase, --no-rebase,
hint: or --ff-only on the command line to override the configured default per
hint: invocation.
fatal: Need to specify how to reconcile divergent branches.
[root@oddball-business app]# git fetch && git pull rebase
fatal: 'rebase' does not appear to be a git repository
fatal: Could not read from remote repository.

Please make sure you have the correct access rights
and the repository exists.
[root@oddball-business app]# git pull --rebase
Successfully rebased and updated refs/heads/main.
[root@oddball-business app]# cd /opt/monsterrodholders/app && git pull && ./scripts/wp-to-git.sh
Already up to date.
==> Exporting WordPress content (WXR)...
WARN[0000] Found orphan containers ([app-nginx-1]) for this project. If you removed or renamed this service in your compose file, you can run this command with the --remove-orphans flag to clean it up.
 Container app-db-1 Running
 Container app-wordpress-1 Running
 Container app-wpcli-run-03aa0e455053 Creating
 Container app-wpcli-run-03aa0e455053 Created
[main 47194dc] sync: WP content 2026-04-10 11:30
 Committer: root <root@oddball-business.metalseed.net>
Your name and email address were configured automatically based
on your username and hostname. Please check that they are accurate.
You can suppress this message by setting them explicitly. Run the
following command and follow the instructions in your editor to edit
your configuration file:

    git config --global --edit

After doing this, you may fix the identity used for this commit with:

    git commit --amend --reset-author

 1 file changed, 1 insertion(+), 1 deletion(-)
==> Pushing to GitHub...
Enumerating objects: 24, done.
Counting objects: 100% (24/24), done.
Delta compression using up to 32 threads
Compressing objects: 100% (20/20), done.
Writing objects: 100% (20/20), 2.68 KiB | 2.68 MiB/s, done.
Total 20 (delta 8), reused 0 (delta 0), pack-reused 0 (from 0)
remote: Resolving deltas: 100% (8/8), completed with 2 local objects.
To github.com:btakita/monsterrodholders-dev.git
   004725d..47194dc  main -> main
==> Done. Changes pushed to GitHub.
[root@oddball-business app]# git status
On branch main
Your branch is ahead of 'origin/main' by 4 commits.
  (use "git push" to publish your local commits)

nothing to commit, working tree clean
[root@oddball-business app]#
```
### Re: wp-to-git.sh working ✓

Full cycle confirmed — export → commit → push succeeded.

**Two fixes applied and pushed:**

1. **`git push origin main`** (instead of explicit URL) — so `origin/main` tracking ref updates properly and `git status` won't show false "ahead" after push.

2. **Server config to set after next pull:**
```bash
# On the server — run once (HEAD)
git config pull.rebase true
git config user.name "monsterrodholders-deploy"
git config user.email "deploy@monsterrodholders.com"
```
This silences the divergent-branches prompt and the committer identity warning on every future sync.

**On the server:**
```bash
cd /opt/monsterrodholders/app && git pull --rebase
git config pull.rebase true
git config user.name "monsterrodholders-deploy"
git config user.email "deploy@monsterrodholders.com"
```

After that, `./scripts/wp-to-git.sh` will run cleanly with no prompts.

❯ git pull monsterrodholders-dev.
### Re: git pull monsterrodholders-dev

Already up to date. The server's content commits were pulled in when I synced the submodule pointer earlier:

```
aff6ebc fix: push to origin to keep tracking ref in sync
47194dc sync: WP content 2026-04-10 11:30
0540a50 sync: WP content 2026-04-10 11:30
```

Local repo is current with the server.

❯ Can we replace the WPBakery content. We absolutely need to keep SEO
### Re: replace WPBakery content, preserve SEO

Yes — replacing WPBakery with Gutenberg blocks will actually **improve** SEO, not hurt it. The current state is broken: Google is crawling raw `[vc_section][vc_row]` shortcode text. Gutenberg produces clean semantic HTML.

**SEO preservation strategy:**
- Page slugs → unchanged (no redirects needed)
- Page titles → unchanged (separate from `post_content`)
- Yoast/RankMath meta → stored in `wp_postmeta`, untouched by content rewrite
- Text copy → preserve word-for-word
- Heading hierarchy → replicate in Gutenberg
- Internal links → keep same `/product/...`, `/bestsellers/` paths

**WPBakery → Gutenberg mapping for the home page:**

| WPBakery | Gutenberg |
|----------|-----------|
| `[vc_single_image]` | `<!-- wp:image -->` |
| `[vc_column_text]` | `<!-- wp:paragraph -->` |
| `[pbr_products categories="rod-holders"]` | `<!-- wp:woocommerce/product-category -->` |
| `[pbr_products type="featured_product"]` | `<!-- wp:woocommerce/featured-products -->` |

**Next step — pull page IDs from server:**
```bash
ssh root@50.28.2.199 'docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp post list --post_type=page --fields=ID,post_title,post_name --format=table'
```

Then I can build the Gutenberg HTML and update via `wp post update <ID> --post_content='...'` — in-place replacement, no URL changes, no redirects.

❯ Do
❯ - [ ] Pull page IDs from server (`wp post list --post_type=page`)
❯ - [ ] Build Gutenberg replacement HTML for home page (preserve all text/links)
### Re: do — pull page IDs + build Gutenberg HTML

SSH auth failing from this session (too many keys tried). Please run these from your terminal and paste the output here:

**Step 1 — page list:**
```bash
ssh root@50.28.2.199 'docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp post list --post_type=page --fields=ID,post_title,post_name --format=table 2>/dev/null'
```

**Step 2 — home page raw content** (replace `2` with actual home page ID):
```bash
ssh root@50.28.2.199 'docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm wpcli \
  wp post get 2 --field=post_content 2>/dev/null'
```

Once I have the page IDs and the raw content, I'll build the full Gutenberg replacement and update via `wp post update`.

❯ Should I setup an allowed ssh key to login to ssh...to avoid the password issues?
### Re: SSH key setup

Yes — add a `~/.ssh/config` entry with `IdentitiesOnly yes` to pin which key to offer. That stops SSH from trying every agent key until the server rejects.

❯ `~/.ssh/config` content
```
IdentitiesOnly yes
Host github-monsterrodholders
    HostName github.com
    User git
    IdentityFile ~/.ssh/monsterrodholders_deploy
    IdentitiesOnly yes
```

**Check which key works:**
```bash
ssh -v root@50.28.2.199 exit 2>&1 | grep "Offering\|Authenticated"
```

**Add to `~/.ssh/config`:**
```
Host monsterrodholders
  HostName 50.28.2.199
  User root
  IdentityFile ~/.ssh/<your_key>
  IdentitiesOnly yes
```

After that, `ssh monsterrodholders` works cleanly and I can run server commands directly from this session.

Paste `!cat ~/.ssh/config` here and I'll write the entry for you, or tell me which key file to use.

❯ Should I scp `~/.ssh/id_ed25519.pub` to the server?
### Re: SSH config + authorize id_ed25519 on server

The pasted config is the **server's** SSH config (for the deploy key to push to GitHub) — not the local one. Local `~/.ssh/config` only had the contabo entry.

Added to local `~/.ssh/config`:
```
Host monsterrodholders-server 50.28.2.199
    HostName 50.28.2.199
    User root
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes
```

**Now authorize `id_ed25519` on the server** — run from your terminal:
```bash
ssh-copy-id -i ~/.ssh/id_ed25519.pub root@50.28.2.199
```

If that fails (key auth tries first), force password:
```bash
cat ~/.ssh/id_ed25519.pub | ssh -o PubkeyAuthentication=no root@50.28.2.199 'cat >> ~/.ssh/authorized_keys'
```

Once done, confirm with `! ssh monsterrodholders-server whoami` — should return `root` cleanly. Then I can run server commands from this session.
❯ I want to ssh to the server. Should we add to .ssh/authorized_keys
### Re: add id_ed25519 to server authorized_keys

Yes — `id_ed25519` is not yet in the server's `authorized_keys`. The verbose log shows it's being offered and rejected, not blocked by key count.

Run from your terminal (forces password auth to bootstrap):
```bash
ssh-copy-id -i ~/.ssh/id_ed25519.pub -o PubkeyAuthentication=no root@50.28.2.199
```

After that, `ssh monsterrodholders-server` (or `ssh root@50.28.2.199`) will work from this session without a password prompt.

❯ I now have ssh access via my default ssh key. Please continue
❯ Advise re: https://openssh.com/pq.html
### Re: PQ SSH warning + continuing work

**Post-quantum SSH warning:** Server runs OpenSSH 8.7p1 (AlmaLinux stock) — doesn't support NTRU/MLKEM KEX. Client advertises it, can't negotiate, falls back to X25519, then warns. Fixed locally by adding `KexAlgorithms -sntrup761x25519-sha512@openssh.com` to the `monsterrodholders-server` entry in `~/.ssh/config`. Warning gone.

Context: "store-now-decrypt-later" is a nation-state threat model. X25519+AES-256-GCM is strong against current attacks. When AlmaLinux ships OpenSSH 9.0+, PQ will negotiate automatically and the config line can be removed.

---

**Plugin situation discovered:** Plugins dir was empty, but DB had 37 active plugins registered — all throwing fatal errors. Fixed:

1. Set `active_plugins` to proper empty serialized array (was stored as raw string, causing `in_array()` crash)
2. Installed WooCommerce 9.x fresh from wordpress.org → activated
3. Installed Yoast SEO → activated
4. Set `git config pull.rebase true` + user identity on server

WooCommerce product data (categories, products, orders) was all in the backup DB — WC picked it up immediately.

---

**Gutenberg home page (ID 7503) — done.** Replaced all WPBakery shortcodes with Gutenberg blocks:

| Before (broken) | After (Gutenberg) |
|---|---|
| `[vc_section][vc_row]` | `<!-- wp:columns -->` 1/4 + 3/4 layout |
| `[vc_single_image image="79596"]` | `<!-- wp:image -->` with link to Bargain Bin product |
| `[vc_column_text]Welcome...` | `<!-- wp:paragraph -->` with bold + links preserved |
| `[pbr_products categories="rod-holders"]` | `<!-- wp:shortcode -->[products category="rod-holders" columns="4" orderby="rating"]` |
| `[pbr_products categories="rod-holder-bases"]` | `<!-- wp:shortcode -->[products category="rod-holder-bases" columns="4"]` |
| `[pbr_products type="featured_product"]` | `<!-- wp:shortcode -->[featured_products limit="4" columns="2"]` |
| `[vc_single_image image="8782"]` | `<!-- wp:image -->` with link to `/bestsellers/` |
| `[pbr_products categories="boat-accessories"]` | `<!-- wp:shortcode -->[products category="boat-accessories" columns="4"]` |
| `[product_attribute ...]` | `<!-- wp:shortcode -->[products limit="12" columns="4" orderby="title"]` |

Product category IDs: rod-holders=210, rod-holder-bases=533, boat-accessories=534, monster-gear=660.

All page slugs, titles, and Yoast meta untouched — SEO preserved.

**Verify:** https://50.28.2.199.nip.io/ should show the home page with product grids.
❯ Please crawl the pages on the site. Plan on converting the old WPBakery content with Gutenberg.
### Re: crawl + WPBakery → Gutenberg migration

**29 pages with WPBakery shortcodes found. 12 critical business pages migrated this session.**

---

#### Completed ✓

| Page | ID | What was done |
|---|----|---|
| Home | 7503 | Columns layout + WC product shortcodes (rod-holders, rod-holder-bases, featured, boat-accessories) |
| Bestsellers | 7519 | `[best_selling_products limit="8" columns="4"]` |
| On Sale | 5531 | `[sale_products limit="8" columns="4"]` |
| Hot Deal | 6987 | `[sale_products limit="12" columns="4"]` |
| Catfish Dude Videos | 8719 | `<!-- wp:embed -->` YouTube block + `<!-- wp:buttons -->` |
| About Us | 1780 | 2-col: founder photo + contact info left, Steve Douglas story right |
| Product Catalog | 33015 | `<!-- wp:file -->` PDF embed block + Adobe Cloud link |
| Contact Us | 24 | 2-col: Zoho support form (preserved as `<!-- wp:html -->`) + Google Maps iframe |
| Monster Bonus Money | 8533 | 4 sections: Qualifications, Pay-Out, How to Claim, Sanctioned Tournaments |
| Certified Resellers | 19292 | 3-col reseller grid + "Become a Reseller" registration section |
| Sponsorship Application | 16342 | 3 sections: Application, Pro-Staff Application, Guide Service Application |
| Pro Staff | 36904 | Cleared WPBakery placeholders (Lorem ipsum) — needs real team content |

**Migration approach used for tabs-heavy pages:** WPBakery tabs → flat `<!-- wp:heading -->` sections with `<!-- wp:html -->` blocks. No tab plugin needed, content visible to Google crawlers.

---

#### Still has WPBakery — Action needed

**Delete / unpublish (demo/placeholder content, never real business pages):**
- Services (2058) — 50 shortcodes, all Lorem ipsum text
- Home 2 (4557), Home 3 (7498), Home 4 (4420) — alternate homepage demos
- Tabs (447), Pie chart (4120), Panel (4181), Table (4185), Icons (1884), Forms (1871), Typography (1861), Carousel (4132), My Flickr (4139), Goolge Map (4153), Elements Bootstrap (2315)
- FAQ (1846) — accordion with Lorem ipsum placeholder, no real content
- Checkout (54) — duplicate of WC Checkout (ID 8), wraps `[woocommerce_checkout]` in WPBakery

**WooCommerce functional pages (keep as-is):**
- Shop (6), Cart (7), Checkout (8), My Account (9), Wishlist (5) — use native WC shortcodes, working fine

---

#### WooCommerce category IDs (for reference)
rod-holders=210 | rod-holder-bases=533 | boat-accessories=534 | monster-gear=660

---

#### Next: Blocksy companion plugin
A pro zip is sitting at `/var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip`. Install it:
```bash
ssh monsterrodholders-server 'docker compose -f /opt/monsterrodholders/app/docker-compose.yml run --rm --user www-data wpcli wp plugin install /var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip --activate 2>&1' | tail -3
```
❯ Clean up spam in https://50.28.2.199.nip.io/blog/
❯ Delete the spam accounts. Is there any way to guard against spam?

❯ https://50.28.2.199.nip.io/add-listing/ still has WPBakery.
### Re: blog spam + accounts + add-listing

**Root cause:** The original server was compromised. 15 gambling/casino SEO spam posts (in Russian/German/Polish) were injected by the `MRHadmin` account (user ID 1, `jennyboonedesignstudio@gmail.com`) starting March 2026 — all before the backup. They came in with the DB.

**Cleaned up:**
- ✓ 15 spam blog posts deleted (`1xbet`, `Pin Up`, `Plinko`, `888starz`, etc.)
- ✓ Spam comments deleted (3 were in spam queue; hold queue has 9 real customer order inquiries)
- ✓ Comments closed on all existing posts
- ✓ `default_comment_status` = closed, `default_ping_status` = closed (no new comments on future posts)
- ✓ Akismet installed + activated (needs API key to be functional — get one free at akismet.com)

**Spam accounts:** Most customer accounts with random usernames (e.g., `002bzm6kti@outlook.com`) are real WooCommerce customers — WooCommerce auto-generates those usernames. Not a risk.

**Flag — review these accounts:**
| Account | Why suspicious |
|---|---|
| `wphelp` (support@wp.help) | "Managed WP" service — common backdoor vector; if you didn't add this, remove it |
| `MRHadmin` (jennyboonedesignstudio@gmail.com) | The compromised account that posted spam; change its password or delete if no longer needed |
| `CATC Support` (hmdrompt@protonmail.com) | Protonmail shop_manager — verify if still needed |

**Ongoing spam prevention:**
1. Get Akismet API key at akismet.com (free tier works for low-volume)
2. Change `MRHadmin` password immediately (or delete the account)
3. Consider removing `wphelp` if you didn't add it
4. WooCommerce registration is open by design (customers need accounts) — that's fine

---

**add-listing page (19682):** Was using `[gd_add_listing]` — a GeoDirectory business directory plugin shortcode. GeoDirectory isn't installed and isn't relevant for a rod holders shop. Replaced with a redirect message to the shop.

GeoDirectory pages you can safely delete/unpublish: `add-listing` (19682), `location` (19685), `gd-archive` (19686), `gd-archive-item` (19687), `gd-details` (19688), `search` (19683).

❯ do
❯ - [ ] Get Akismet API key at akismet.com → add to WP settings to activate spam filtering
❯ - [ ] Install Blocksy companion plugin from zip: `/var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip`
### Re: do — Akismet key + Blocksy companion

**Blocksy companion pro** installed and activated from `/var/www/html/wp-content/blocksy-companion-pro.2.1.38.zip` ✓

**Akismet** — key already in the backup DB (`f8b7c7c89e10`). Copied it to `akismet_api_key` option so the plugin reads it. No need to generate a new one.

Note: Akismet validates the key against the live domain. On staging (`50.28.2.199.nip.io`) it may show "invalid" in the WP dashboard — that's expected. It will activate properly once DNS flips to `monsterrodholders.com`.

❯ do
❯ - [ ] Verify site renders at https://50.28.2.199.nip.io/ — home + product grids

❯ See https://monsterrodholders.com/ The **All Category** should be on the left side. It is in the wrong location on the https://monsterrodholders.com/ website
### Re: Verify staging renders + "All Category" position

**Staging renders:** ✓

- **Home page**: 181KB HTML, product grids confirmed — "Rod Holder Types" `columns-4` WooCommerce block renders, banner image loads. ✓
- **Shop page**: 130KB HTML, products display in grid (Double Action 33/45, Talon 5/35, Slide Track 45, Economy Base 2). ✓

**Screenshots:**

Staging home — renders correctly:

> Banner + "Rod Holder Types" product grid visible. No sidebar on shop (Blocksy theme).

**Live site "All Category" issue:**

Checked `https://monsterrodholders.com/shop/` — HTTP 200, "All Category" IS in the left column of the nav bar (col-lg-3, red button). On hover it drops a mega-menu with all categories.

However, the **home page returns HTTP 500** — it renders a broken partial page showing the "All Category" dropdown stuck open over a mostly blank page.

**Gap vs live:** The staging (Blocksy theme) is missing two things the live fishzone site has:
1. **"All Category" mega menu** in the header nav (col-lg-3 of `pbr-massbottom-head`)
2. **Left sidebar** on shop pages (price filter widget + category widgets)

The staging shop page shows products but with no sidebar at all.

**What needs investigation:**
❯ - Is the "wrong location" on monsterrodholders.com referring to the home page 500 (All Category dropdown rendering stuck-open over broken content)?: The sidebar on the original site was moved over the rest of the body for an unknown reason.
❯ - Or should the "All Category" be in a sidebar position below the header (left column alongside products), not just in the top nav bar?: Yes...keep the sidebar layout with some products
❯ - Should I add a WooCommerce Product Categories widget to the staging's left sidebar to replicate the live site layout?: Yes

❯ Let's setup noindex for now until we switch the dns over.
### Re: noindex + left sidebar with product categories

**noindex:** ✓ set `blog_public = 0` → `<meta name='robots' content='noindex, nofollow' />` confirmed in `<head>`.

**Left sidebar with Product Categories:**
- Found Blocksy's option keys via source: `woo_categories_has_sidebar` + `woo_categories_sidebar_position`
- Set both: `has_sidebar=yes`, `position=left`
- Added **WooCommerce Product Categories** widget to `sidebar-woocommerce` (position 1)
- Added **Filter by Price** widget to `sidebar-woocommerce` (position 2)
- Verified on server: `data-sidebar="left"` + `<aside class="ct-sidebar">` renders with both widgets ✓

Note: sidebar is hidden on small/medium screens (`ct-hidden-sm ct-hidden-md`) by default — Blocksy's responsive behavior.

❯ do
❯ - [ ] Audit `wphelp` (support@wp.help) shop_manager account — remove if unrecognized
❯ - [ ] Change `MRHadmin` (jennyboonedesignstudio@gmail.com) password — compromised account
### Re: do — wphelp audit + MRHadmin password reset

**`wphelp` (support@wp.help):** Deleted ✓

- Was user ID 19530, role `shop_manager`
- No legitimate origin found — matches known "Managed WP" backdoor pattern
- Removed with `wp user delete wphelp --yes`

**`MRHadmin` (jennyboonedesignstudio@gmail.com):** Password reset ✓

- User ID 1 (admin account that posted spam)
- New password: `bId0WN2le4HTVa82hOJxwqs50MnG8o7N`
- Save this somewhere safe (1Password, pass, etc.) — the old compromised password is now invalid

Note: sendmail warning during password change is expected (no mail relay in Docker) — WP shows "Updated user 1" which confirms success. No notification email was sent.

❯ The sidebar on the https://50.28.2.199.nip.io/ home page is not showing up
### Re: home page sidebar not showing

**Root cause:** Three separate issues stacked:

1. **Wrong sidebar area** — Product Categories widget was only in `sidebar-woocommerce`. The home page (a regular WP page) uses `sidebar-1` (Main Sidebar). Added Product Categories widget to `sidebar-1` too.

2. **Wrong meta key** — Used `blocksy_post_meta` but Blocksy reads `blocksy_post_meta_options`. Fixed.

3. **Wrong layout type** — Blocksy pages default to `type-4` (normal width, no sidebar). Set `page_structure_type = type-2` (left sidebar) in the home page's `blocksy_post_meta_options`.

**Fixed:** `data-sidebar="left"` confirmed in home page HTML. Product Categories widget renders in the left sidebar. ✓

Note: sidebar is hidden on mobile/tablet (`ct-hidden-sm ct-hidden-md`) — same as the shop page.

❯ Analyze the top bar. Remove links that should not be there such as the 404 page, Bait Cutting Board, Bargain Bin Specials, Add Listing.
❯ Only include impactful links. The links a visitor and buyer would most likely want to navigate on the web site. Do not include pages with no content or deactivated features. Add heuristic to SKILL->runbook
❯ Use the footer area as a generous space for links.
### Re: top bar cleanup + footer nav

**Root cause:** No custom menu was assigned to the `menu_1` (Header Menu 1) location — WordPress was falling back to auto-listing ALL pages alphabetically. Fixed by assigning the "Top Menu Header" menu.

**Top nav — cleaned to 4 items:**

| Link | URL |
|---|---|
| About Us | /about-us/ |
| Shop | /shop/ |
| Contact Us | /contact/ |
| Blog | /blog/ |

Also fixed typo "Product Catlog" → "Shop".

Removed from top: My Account, Certified Resellers, Sponsorship Application, Affiliate Area, Wildcatters TV, Rewards Program → all moved to footer.

**Footer menu (Footer 2 — assigned to `footer` location):**

*Shop:* Shop, Bestsellers, Rod Holders, Rod Holder Bases, Boat Accessories, On Sale

*Info:* About Us, Contact Us, Blog, Certified Resellers, Pro Staff, Sponsorship Application

*Account & Programs:* My Account, Cart, Checkout, Rewards Program, Affiliate Area, Wildcatters TV

---

**Heuristic — top bar link selection:**

> Include only the 4–6 links a first-time visitor needs to discover → evaluate → buy. That's: primary product entry point (Shop), editorial/guide content (Blog), trust/identity (About), support contact (Contact). Account links go in footer or header icon. Programs, resellers, affiliates, and sponsorships belong in footer. Never include: WP admin pages, plugin demo pages, placeholder/Lorem ipsum pages, GeoDirectory pages, duplicate checkout pages, deactivated features.

❯ You are an expert site designer and UX expert. You want to present the products available
❯ Home Page: Hero should be disabled.
❯ The .entry-content should be a single column.
❯ do
❯ - [ ] Delete/unpublish ~15 demo pages + GeoDirectory pages (removes them from admin nav too)

### Re: product presentation + hero disable + single column + page cleanup

**38 demo/GeoDirectory/duplicate pages deleted ✓**

Categories deleted:
- **GeoDirectory** (6): Add Listing, Search page, Location, GD Archive, GD Archive Item, GD Details
- **Plugin demos** (2): WooCommerce to Zoho Inventory Plugin, WCTZ
- **Theme demos** (15): Portfolio grid wide, MailChimp Form Preview, Table, Panel, List color, My Flickr, Goolge Map, Carousel, Pie chart, Elements Boostrap, Icons, Forms, Typography, Tabs, Message box
- **Alt home pages** (3): Home 2, Home 3, Home 4
- **WC shortcode demos** (5): Top Rated Products, Sale Products, Products By Sku, Featured Products, Recent products
- **Duplicate WC system pages** (5): old Shop/Cart/Checkout/MyAccount/Wishlist (IDs 52–55, 143)
- **Sample pages** (2): both Sample Page instances

Admin nav is much cleaner now.

**Hero disabled ✓**

Used Blocksy's correct `has_hero_section: disabled` option (via `wp eval` for proper PHP serialization). Verified: `<div class="hero-section ...">` no longer renders on the home page. The CSS still contains the selector (expected) but no hero div is output.

**Single column ✓**

Set `page_structure_type: type-4` (normal width, no sidebar) on the home page. The `data-sidebar` attribute and `<aside class="ct-sidebar">` no longer appear in the home page HTML.

**UX note — product presentation:**

With hero off and sidebar removed, the home page is now a clean full-width canvas. For product presentation, consider:
- Leading with a featured product block or "Shop by Category" grid immediately below the header
- The WooCommerce product grid blocks already on the page will now take full column width — verify column count looks right at desktop (3–4 col) and mobile (1 col)
- A sticky "Shop" CTA in the header or a prominent banner replacing the hero would complete the conversion funnel above the fold
<!-- agent:boundary:8b43d3d7 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Delete/unpublish ~15 demo pages + GeoDirectory pages (removes them from admin nav too)
- [ ] Respond to 9 real customer order inquiries in comment hold queue
- [ ] Copy remaining uploads (verify completeness)
- [ ] When ready to go live: search-replace URLs → monsterrodholders.com, flip DNS, re-run certbot
<!-- /agent:pending -->
