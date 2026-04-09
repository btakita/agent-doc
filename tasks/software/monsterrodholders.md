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
❯ I have a dedicated Alma Linux server on liquidweb.com
❯ I want to run a wordpress site using a docker container.
❯ I want to configure a good setup with docker containers.
Should we use kubernetes or docker swarm? I don't have experience to kubernetes but I can learn. What do you suggest. 
### Re: Kubernetes vs Docker Swarm for single-server WordPress

**Use Docker Compose — not Kubernetes or Swarm.**

For a single dedicated server running one WordPress site, neither Kubernetes nor Docker Swarm adds value. They're orchestrators designed for multi-node clusters. On one machine, they add complexity with no real benefit.

Docker Compose is the right tool:
- Purpose-built for single-host multi-container apps
- Excellent WordPress + Nginx + MariaDB examples everywhere
- Simple to operate, easy to update, trivial to back up

**Typical stack:**

```
Nginx (reverse proxy + SSL termination)
  ↕
WordPress (php-fpm container)
  ↕
MariaDB (database container)
  ↕
Redis (optional, object cache)
```

Certbot or Traefik handles Let's Encrypt SSL. Traefik integrates directly with Docker Compose and auto-renews certs — less manual work than Certbot.

**When to reconsider:**
- k3s (lightweight K8s): if you add more servers later and need automated failover
- Docker Swarm: basically never — k3s has replaced it and Swarm is in maintenance mode

Want me to draft a `docker-compose.yml` + Nginx config for this setup?

❯ Yes...create a private btakita/monsterrodholders-dev repo to put our config files into. Add a submodule at src/monsterrodholders-dev.
❯ Layout the repo assuming that we will be administering wordpress with it.
### Re: Create repo + submodule

Done. Here's what was created:

**GitHub:** `btakita/monsterrodholders-dev` (private)
**Submodule:** `src/monsterrodholders-dev` (committed to agent-loop)

#### Repo layout

```
monsterrodholders-dev/
├── docker-compose.yml          # Nginx + WordPress + MariaDB + Redis + WP-CLI
├── .env.example                # Env var template (.env is gitignored)
├── nginx/
│   ├── nginx.conf
│   └── conf.d/wordpress.conf   # Site config + SSL + PHP-FPM proxy
├── wordpress/
│   └── php.ini                 # Upload limits, memory, execution time
├── mysql/
│   └── custom.cnf              # InnoDB buffer pool tuning
├── scripts/
│   ├── backup.sh               # DB + uploads backup (auto-prunes >30 days)
│   ├── restore.sh              # Restore DB from .sql.gz
│   ├── deploy.sh               # Pull images + restart + cache flush
│   ├── wp.sh                   # WP-CLI wrapper (plugin, theme, cache, etc.)
│   └── certbot-init.sh         # One-time SSL provisioning
├── certs/                      # SSL certs (gitignored)
└── backups/                    # Backup storage (gitignored)
```

#### First-time setup (on the server)

```bash
git clone git@github.com:btakita/monsterrodholders-dev.git
cd monsterrodholders-dev
cp .env.example .env    # fill in passwords + DOMAIN + CERTBOT_EMAIL
source .env && ./scripts/certbot-init.sh   # provision SSL
docker compose up -d
```

Then complete WordPress install at `https://monsterrodholders.com`.

#### Next steps to configure on the server

1. Add `CERTBOT_EMAIL` to `.env.example` (email for Let's Encrypt)
2. Set up cron for SSL auto-renewal (line in README)
3. Install Redis Object Cache plugin in WordPress admin and activate it
4. Configure daily backup cron: `0 2 * * * cd /path && source .env && ./scripts/backup.sh`

❯ commit + push
### Re: commit + push

Done.

- `btakita/monsterrodholders-dev` — committed `.gitignore` (added `*.iml`), pushed to `origin/main`
- `btakita/agent-loop` — updated submodule pointer, pushed to `origin/main`

❯ We are in the process of moving monsterrodholders.com to the new host.
❯ I have a backup zip file.
❯ There are many plugins that are unnecessary. Not sure which ones.
❯ There's currently spam on the site.
❯ Is it possible to keep the same passwords for the existing user accounts?
❯ Please give me instructions on the migration.
### Re: WordPress migration to new Docker host

**User passwords are preserved** — hashed in `wp_users.user_pass`, migrating the DB keeps all accounts intact.

#### What format is the backup zip?

The process depends on how it was created:

- **All-in-One WP Migration** (`.wpress` file) — easiest path, plugin handles everything
- **cPanel/Plesk backup** — contains separate files + SQL dump
- **Manual zip** — wp-content folder + SQL dump

Which is it? If unsure, open the zip and check for a `.wpress` file or a `.sql`/`.sql.gz` file inside.

❯ Zip file at /home/brian/work/rberkes/monsterrodholders/MonsterRodholdersNETNEWcloud-040626-backup.zip
❯ There's a backup.sql file in there.

---

#### Path A: All-in-One WP Migration backup

```bash
cd src/monsterrodholders-dev
cp .env.example .env    # fill in passwords + domain
docker compose up -d

# Install plugin
./scripts/wp.sh plugin install all-in-one-wp-migration --activate

# Copy backup into container
docker compose run --rm wordpress sh -c \
  'mkdir -p /var/www/html/wp-content/ai1wm-backups'
docker cp /path/to/backup.wpress \
  $(docker compose ps -q wordpress):/var/www/html/wp-content/ai1wm-backups/

# Import
./scripts/wp.sh ai1wm restore backup.wpress --yes

# Update URLs if domain changed
./scripts/wp.sh search-replace 'https://old-domain.com' 'https://monsterrodholders.com' --all-tables
./scripts/wp.sh cache flush
```

#### Path B: Manual zip (files + SQL dump)

```bash
cd src/monsterrodholders-dev
cp .env.example .env
source .env
docker compose up -d

# Replace wp-content
docker compose run --rm wordpress sh -c 'rm -rf /var/www/html/wp-content'
docker cp /path/to/wp-content $(docker compose ps -q wordpress):/var/www/html/
docker compose exec wordpress chown -R www-data:www-data /var/www/html/wp-content

# Import database
gunzip -c /path/to/backup.sql.gz | docker compose exec -T db mariadb \
  -u"${WORDPRESS_DB_USER}" -p"${WORDPRESS_DB_PASSWORD}" "${WORDPRESS_DB_NAME}"

# Update URLs
./scripts/wp.sh search-replace 'https://old-domain.com' 'https://monsterrodholders.com' --all-tables
./scripts/wp.sh cache flush
```

#### Clean up spam

```bash
./scripts/wp.sh comment list --status=spam --format=count
./scripts/wp.sh comment delete $(./scripts/wp.sh comment list --status=spam --format=ids) --force
```

#### Audit plugins

```bash
./scripts/wp.sh plugin list
# Share the output — I can identify which to remove
```

#### User passwords

Passwords are hashed (bcrypt/phpass) in the DB — all existing users keep their passwords after migration.
### Re: Migration steps for this backup

**Good news:** Domain is already `https://monsterrodholders.com` — no URL search-replace needed.

**Critical detail:** Non-standard table prefix `wp_nxz1mfc9cg_` — must set in `.env` before starting.

#### Step 1: Configure `.env`

```bash
cd src/monsterrodholders-dev
cp .env.example .env
# Set WORDPRESS_TABLE_PREFIX=wp_nxz1mfc9cg_
# Fill in all passwords
```

#### Step 2: Extract the backup

```bash
mkdir -p /tmp/mrh-restore
cd /tmp/mrh-restore
unzip /home/brian/work/rberkes/monsterrodholders/MonsterRodholdersNETNEWcloud-040626-backup.zip
# Result: backup.sql + files/wp-content/
```

#### Step 3: Start the stack

```bash
cd src/monsterrodholders-dev
source .env
docker compose up -d
# Wait ~30s for MariaDB to initialize
docker compose logs db --tail=5
```

#### Step 4: Import the database (519MB — takes a few minutes)

```bash
cd /tmp/mrh-restore
cat backup.sql | docker compose -f ~/work/btakita/agent-loop/src/monsterrodholders-dev/docker-compose.yml \
  exec -T db mariadb \
  -u"${WORDPRESS_DB_USER}" -p"${WORDPRESS_DB_PASSWORD}" "${WORDPRESS_DB_NAME}"
```

#### Step 5: Copy wp-content into the volume

```bash
WP_CTR=$(docker compose -f ~/work/btakita/agent-loop/src/monsterrodholders-dev/docker-compose.yml ps -q wordpress)
docker exec $WP_CTR rm -rf /var/www/html/wp-content
docker cp /tmp/mrh-restore/files/wp-content $WP_CTR:/var/www/html/
docker exec $WP_CTR chown -R www-data:www-data /var/www/html/wp-content
```

#### Step 6: Remove WP Engine-specific mu-plugins

The backup came from WP Engine — these files cause errors on self-hosted:

```bash
docker exec $WP_CTR rm -f  /var/www/html/wp-content/mu-plugins/wpe-rtc-loader.php
docker exec $WP_CTR rm -rf /var/www/html/wp-content/mu-plugins/rtc
docker exec $WP_CTR rm -f  /var/www/html/wp-content/mu-plugins/0-worker.php
docker exec $WP_CTR rm -f  /var/www/html/wp-content/mu-plugins/01-mu-TransitionFlowModule.php.php
docker exec $WP_CTR rm -f  /var/www/html/wp-content/mu-plugins/wp-migrate-db-pro-compatibility.php
docker exec $WP_CTR rm -f  /var/www/html/wp-content/advanced-cache.php
```

#### Step 7: Flush caches

```bash
cd src/monsterrodholders-dev
./scripts/wp.sh rewrite flush
./scripts/wp.sh cache flush
```

#### Plugin audit

| Plugin | Action | Reason |
|--------|--------|--------|
| `worker` (ManageWP) | **Delete** | WP Engine managed agent — not needed self-hosted |
| `wpmudev-updates` | **Delete** | WPMU DEV updater — not needed without subscription |
| `adroll-for-woocommerce-stores-dev` | **Review** | Dev suffix — likely stale/broken |
| `wctzin` | **Review** | Unknown — check what it does |
| `taxjar-simplified-taxes-for-woocommerce` | **Pick one** | Both TaxJar + AvaTax installed — keep only one |
| `woocommerce-avatax` | **Pick one** | ↑ |
| `klarna-payments-for-woocommerce` | **Keep if used** | Deactivate if not accepting Klarna |
| `revslider` | **Keep if used** | Remove if no sliders on site |
| `js_composer` (WPBakery) | **Check** | Both Elementor + WPBakery active — verify which one the theme uses |
| `woocommerce-legacy-rest-api` | **Keep if needed** | Only for old 3rd-party integrations |

```bash
./scripts/wp.sh plugin deactivate worker wpmudev-updates
./scripts/wp.sh plugin delete worker wpmudev-updates
```

#### Step 8: Clean spam

```bash
./scripts/wp.sh comment list --status=spam --format=count
./scripts/wp.sh comment delete $(./scripts/wp.sh comment list --status=spam --format=ids) --force
```

#### Go live (on the server)

```bash
source .env && ./scripts/certbot-init.sh
docker compose up -d
```

❯ Put instructions into it's own artifact md file.
❯ I am logged in via ssh.
❯ Walk me through setting up a ssh key, adding permission just to the github repo, cloning, installing necessary software on Alma Linux, etc.
❯ Step by step.
I want to test the site before switching over the DNS.
### Re: Server setup guide (HEAD)

Created `docs/server-setup.md` in the repo and pushed. Full step-by-step:

**File:** `src/monsterrodholders-dev/docs/server-setup.md`

Steps covered:

1. **Install Docker + Docker Compose** (via Docker's official RHEL repo)
2. **Install git + unzip**
3. **Generate a deploy SSH key** — scoped to this repo only, no passphrase
4. **Add deploy key to GitHub** — `btakita/monsterrodholders-dev` → Settings → Deploy keys
5. **Configure `~/.ssh/config`** — maps `github-monsterrodholders` host to the deploy key
6. **Clone the repo** to `/opt/monsterrodholders/app`
7. **Configure `.env`** — passwords, table prefix `wp_nxz1mfc9cg_`, domain, certbot email
8. **Provision SSL** — `./scripts/certbot-init.sh` (port 80 must be open first)
9. **Start services** — `docker compose up -d`
10. **Transfer + import backup** — `scp` the zip over, then follow migration steps

Start with step 1. When you get to step 4, you'll need to paste the public key into GitHub. Copy the output of:

```bash
cat ~/.ssh/monsterrodholders_deploy.pub
```
<!-- agent:boundary:cf72eda9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] **Server step 1:** Install Docker + Docker Compose
- [ ] **Server step 2:** Install git + unzip
- [ ] **Server step 3:** Generate deploy SSH key
- [ ] **Server step 4:** Add deploy key to GitHub (btakita/monsterrodholders-dev → Settings → Deploy keys)
- [ ] **Server step 5:** Configure `~/.ssh/config`
- [ ] **Server step 6:** Clone repo to `/opt/monsterrodholders/app`
- [ ] **Server step 7:** Configure `.env` (passwords + `WORDPRESS_TABLE_PREFIX=wp_nxz1mfc9cg_`)
- [ ] **Server step 8:** Provision SSL (`certbot-init.sh`)
- [ ] **Server step 9:** `docker compose up -d`
- [ ] **Server step 10:** Transfer + import backup, remove WP Engine mu-plugins
- [ ] Clean spam comments post-import
- [ ] Plugin audit: delete `worker`, `wpmudev-updates`; pick TaxJar OR AvaTax
- [ ] Set up cron jobs (SSL renewal + daily backup)
<!-- /agent:pending -->
