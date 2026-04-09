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
### Re: Server setup guide

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

❯ commit + push
### Re: commit + push

Done — `btakita/agent-loop` pushed to `origin/main`.

❯ I am the root user. Is that ok?
### Re: Running as root

Fine for a dedicated server. Adjustments to the setup guide:

- **Skip** `usermod -aG docker $USER` and the logout/login — root can already run Docker
- **Drop all `sudo`** — run commands directly
- SSH key lands at `/root/.ssh/monsterrodholders_deploy` (same as `~/.ssh/` when you're root)

Step 1 as root:

```bash
dnf -y install dnf-plugins-core
dnf config-manager --add-repo https://download.docker.com/linux/rhel/docker-ce.repo
dnf -y install docker-ce docker-ce-cli containerd.io docker-compose-plugin
systemctl enable --now docker
docker run --rm hello-world
docker compose version
```

Rest of the steps are identical — just omit `sudo` everywhere.

❯ update server-setup.md
### Re: update server-setup.md

Updated and pushed. Changes:

- Removed all `sudo` — root runs everything directly
- Paths changed: `~/.ssh/` → `/root/.ssh/` (explicit)
- Step 6 (clone): removed `chown` step — not needed as root
- Step 1: removed `usermod` and logout/login instructions
- Added note at top: "Running as root — no `sudo` needed"
- Step 10 `scp`: changed `user@` → `root@` and clarified it runs on local machine

❯ Should I use the pass command or is cleartext ok for the .env files. On step 7 btw
### Re: cleartext vs pass for `.env` (step 7)

**Cleartext in `.env` is fine.**

The `.env` file is gitignored, only readable by root on the server, and never leaves the machine. That's the standard Docker Compose credential pattern.

Lock down permissions when you create it:

```bash
touch .env && chmod 600 .env
nano .env   # fill in values
```

`chmod 600` ensures only root can read or write it — no further protection needed on a single-admin server. `pass` would add GPG key setup and subprocess substitution complexity with no practical benefit here.

❯ How do I generate a secure password on my machine?
### Re: generate a secure password

Run this on your local machine (or the server):

```bash
openssl rand -base64 32
```

Run it twice — one output for `WORDPRESS_DB_PASSWORD`, the other for `MARIADB_ROOT_PASSWORD`. Copy each directly into `.env`. No need to remember them.

❯ ron-berkes is managing this project for steve-douglas (save him as a contact). steve-douglas is the catfish dude. See tasks/research/monsterrodholders.md 
❯ > CERTBOT_EMAIL=<your email for Let's Encrypt>
How do I setup test domain for the WP site?
Can DOMAIN have 2 values?
### Re: steve-douglas contact + CERTBOT_EMAIL

**Contact saved:** `mail/contacts/steve-douglas/AGENTS.md` created with info from Ron's file and the research doc (founder background, health status, YouTube channels, business contact, the Monster Energy trademark dispute).

**CERTBOT_EMAIL:** Use the business email that makes most sense for SSL expiration notices. Options:

- `support@monsterrodholders.com` — already public, someone checks it
- Steve's personal email if Ron has it — more reliable given business support responsiveness issues

In `.env`, set:
```
CERTBOT_EMAIL=rberkes@gmail.com
```

Let's Encrypt sends renewal warnings 30 and 7 days before expiry. Whoever monitors that inbox needs to act if auto-renewal fails.

❯ monsterrodholders.com is using ssl.com. Can we remove it? If we keep ssl.com, do we still have CERTBOT_EMAIL?
### Re: ssl.com vs certbot

**ssl.com is WP Engine's certificate** — not yours. WP Engine auto-provisions SSL using various commercial CAs (often ssl.com or DigiCert). You don't install, pay for, or control it. When the domain moves off WP Engine, that cert goes with it.

**On the new server**, you're using certbot (Let's Encrypt). They're completely separate:

| | WP Engine (current) | New server |
|---|---|---|
| CA | ssl.com (WP Engine manages) | Let's Encrypt (certbot) |
| Cost | Included in WP Engine | Free |
| Auto-renew | WP Engine handles it | certbot cron in Docker |
| CERTBOT_EMAIL | N/A | Required |

**So:** nothing to remove — ssl.com disappears when DNS flips to the new server. You do still need `CERTBOT_EMAIL`.

`rberkes@gmail.com` is fine for that — Ron gets the "your cert expires in 30 days" emails if auto-renewal breaks.

❯ How can we have a test DOMAIN and monsterrodholders.com. I want the test domain first so we can verify the site works.
### Re: test domain first

**Easiest option: nip.io** — no registration, no DNS setup, works immediately.

nip.io is a wildcard DNS service: `<ip>.nip.io` resolves to `<ip>`. If your server is `1.2.3.4`, then `1.2.3.4.nip.io` is a real public domain pointing at it. Let's Encrypt will issue a cert for it.

**Setup:**

1. Get your server's IP:
   ```bash
   curl ifconfig.me
   ```

2. In `.env`, set:
   ```
   DOMAIN=<server-ip>.nip.io
   WORDPRESS_SITEURL=https://<server-ip>.nip.io
   WORDPRESS_HOME=https://<server-ip>.nip.io
   ```

3. Run certbot and bring up the stack as normal

4. Verify: browse to `https://<server-ip>.nip.io`, import the WP backup, test everything

5. When ready to go live, update `.env` to:
   ```
   DOMAIN=monsterrodholders.com
   WORDPRESS_SITEURL=https://monsterrodholders.com
   WORDPRESS_HOME=https://monsterrodholders.com
   ```
   Then flip DNS, re-run certbot, and `docker compose up -d`

**Alternative if you want something more readable:** DuckDNS (duckdns.org) gives you a free subdomain like `mrhtest.duckdns.org` — takes 2 minutes to set up and lets you choose the name.

nip.io is simpler (zero setup), DuckDNS is easier to remember. Either works.
❯ Will Docker + wordpress serve pages with the nip.io domain?
### Re: Docker + WordPress with nip.io

Yes — with one WordPress-specific step.

**Docker/nginx:** The container just listens on port 80/443. It doesn't care what domain points at it, as long as the nginx config has the right `server_name`. When you set `DOMAIN=<ip>.nip.io` in `.env` before first boot, the nginx config is generated with that domain. certbot issues the cert for it. All good.

**WordPress:** Stores its base URL in the database (`wp_options`: `siteurl` and `home`). On a fresh install these are set from `WORDPRESS_SITEURL`/`WORDPRESS_HOME` env vars — which you'll set to the nip.io domain. Pages will serve correctly.

**The catch — importing the WP Engine backup:** The backup DB has `https://monsterrodholders.com` hardcoded everywhere. After importing, WordPress will redirect all requests to that domain. You need a search-replace:

```bash
docker exec wordpress wp search-replace \
  'https://monsterrodholders.com' \
  'https://<ip>.nip.io' \
  --all-tables
```

Run that once after import. Then the site serves on nip.io cleanly.

When switching to production:
```bash
docker exec wordpress wp search-replace \
  'https://<ip>.nip.io' \
  'https://monsterrodholders.com' \
  --all-tables
```

Then flip DNS.

❯ Can we have multiple domains pointing to the wordpress site?
### Re: multiple domains pointing to WordPress

**Short answer:** WordPress has one primary domain. Multiple domains can point at the server, but they all redirect to that one domain — not serve separate content.

**What works:**
- `www.monsterrodholders.com` → redirects to `monsterrodholders.com` (nginx handles the redirect, WordPress never sees the www)
- Any other domain → can also redirect to `monsterrodholders.com` via nginx 301

**What doesn't work without plugins:** Two domains serving the same WordPress content simultaneously. WordPress stores one `siteurl` and redirects anything that doesn't match.

**For the test→production workflow:** nip.io is temporary — once you `wp search-replace` to `monsterrodholders.com`, the nip.io URL stops working. That's fine; it was just for verification. You don't keep both alive.

**If Ron wants a permanent staging URL after launch:** easiest is a separate server (or subdomain like `staging.monsterrodholders.com`) with its own WordPress install — that's a future problem, not this migration.

For now: test on nip.io → verify → flip to `monsterrodholders.com` → done. One domain, one site.

❯ Should I call `chmod a+x ./scripts/certbot-init.sh` or more strict permissions?
### Re: certbot-init.sh permissions (HEAD)

Use `chmod 700`:

```bash
chmod 700 ./scripts/certbot-init.sh
```

- `a+x` (755) — world-executable, no reason for that
- `700` — only root reads or runs it, which is all you need on a single-admin root server

The script touches Docker and certbot as root. No other user needs to see or execute it. `700` is the right habit for any script that runs infrastructure commands, even when there are no other users.
<!-- agent:boundary:2d74bf35 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] **Server step 7:** `touch .env && chmod 600 .env`, fill in values — use `openssl rand -base64 32` for passwords, `CERTBOT_EMAIL=rberkes@gmail.com`, set `DOMAIN=<server-ip>.nip.io` for test first
- [ ] **Server step 8:** `chmod 700 ./scripts/certbot-init.sh && ./scripts/certbot-init.sh`
- [ ] **Server step 9:** `docker compose up -d`
- [ ] **Server step 10:** Transfer + import backup, then run `wp search-replace 'https://monsterrodholders.com' 'https://<ip>.nip.io' --all-tables`; remove WP Engine mu-plugins
- [ ] Verify site on nip.io test domain
- [ ] `wp search-replace` back to `monsterrodholders.com`, update `.env`, flip DNS, re-run certbot
- [ ] Clean spam comments post-import
- [ ] Plugin audit: delete `worker`, `wpmudev-updates`; pick TaxJar OR AvaTax
- [ ] Set up cron jobs (SSL renewal + daily backup)
<!-- /agent:pending -->
