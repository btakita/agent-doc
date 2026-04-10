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
### Re: certbot-init.sh permissions

Use `chmod 700`:

```bash
chmod 700 ./scripts/certbot-init.sh
```

- `a+x` (755) — world-executable, no reason for that
- `700` — only root reads or runs it, which is all you need on a single-admin root server

The script touches Docker and certbot as root. No other user needs to see or execute it. `700` is the right habit for any script that runs infrastructure commands, even when there are no other users.

```
[root@oddball-business app]# source .env
[root@oddball-business app]# ./scripts/certbot-init.sh
./scripts/certbot-init.sh: line 8: DOMAIN: Set DOMAIN in .env
```
### Re: DOMAIN not set

`DOMAIN` isn't in your `.env` yet. The script checks for it on line 8 with `${DOMAIN:?...}` — fails fast if unset.

Fix:

1. Get your server's public IP:
   ```bash
   curl ifconfig.me
   ```

2. Add to `.env` (open with `nano .env`):
   ```
   DOMAIN=<ip>.nip.io
   ```

3. Re-source and retry:
   ```bash
   source .env
   ./scripts/certbot-init.sh
   ```
   
```
[root@oddball-business app]# echo $DOMAIN
50.28.2.199.nip.io
[root@oddball-business app]# ./scripts/certbot-init.sh
./scripts/certbot-init.sh: line 8: DOMAIN: Set DOMAIN in .env
```
### Re: DOMAIN set but script still fails

`$DOMAIN` is in the shell but not exported — child processes (the script) don't inherit it. `source .env` sets variables in the current shell only.

Fix — export everything from `.env` before running the script:

```bash
export $(grep -v '^#' .env | xargs)
./scripts/certbot-init.sh
```

Or just export the one variable since it's already set:

```bash
export DOMAIN
./scripts/certbot-init.sh
```

To avoid this every time, add `export` to the variables in `.env` that need to reach subprocesses:

```
export DOMAIN=50.28.2.199.nip.io
export CERTBOT_EMAIL=rberkes@gmail.com
```

Note: Docker Compose handles `KEY=VALUE` (no export) natively, so adding `export` doesn't break it.

```
[root@oddball-business app]# ./scripts/certbot-init.sh
Unable to find image 'certbot/certbot:latest' locally
latest: Pulling from certbot/certbot
1799ce2dd17e: Pull complete
de19b7a249e5: Pull complete
4bd2d3896265: Pull complete
bde296103a2e: Pull complete
0a9a5dfd008f: Pull complete
61535b7ba0cb: Pull complete
d4954f48dcc7: Pull complete
1d7a63b094ee: Pull complete
eac39555de92: Pull complete
b2ee67ef6cca: Pull complete
8be8a68a3e49: Pull complete
Digest: sha256:78a7eaa40af657301e25731297b6e73646c695370d17e9bb03189fa82966d65c
Status: Downloaded newer image for certbot/certbot:latest
docker: Error response from daemon: failed to set up container networking: driver failed programming external connectivity on endpoint eager_sutherland (723fadfe9d4fdaf1c632badf72d20eaa919e2d46e37b161d337a20f8af156c5b): failed to bind host port 0.0.0.0:80/tcp: address already in use

Run 'docker run --help' for more information
```
### Re: port 80 already in use

Something else is holding port 80. Find it:

```bash
ss -tlnp | grep :80
```

Or:

```bash
docker ps
```

**Most likely:** a Docker container from a previous test run is still up. Stop all running containers:

```bash
docker stop $(docker ps -q)
```

Then retry:

```bash
./scripts/certbot-init.sh
```

If `ss` shows something non-Docker (like nginx or apache installed on the host), stop that service:

```bash
systemctl stop nginx   # or apache2
```

Certbot needs port 80 temporarily to complete the ACME HTTP challenge. Once the cert is issued, port 80 goes to the nginx container instead.

```
[root@oddball-business app]# ss -tlnp | grep :80
LISTEN 0      511                     *:80               *:*    users:(("httpd",pid=998683,fd=4),("httpd",pid=993496,fd=4),("httpd",pid=993441,fd=4),("httpd",pid=993398,fd=4),("httpd",pid=993397,fd=4),("httpd",pid=993391,fd=4))
```
### Re: httpd on port 80

Apache (`httpd`) — stop it:

```bash
systemctl stop httpd
systemctl disable httpd   # prevent it from restarting on reboot
```

Then retry certbot:

```bash
./scripts/certbot-init.sh
```

The Docker nginx container will take over port 80 after the stack is up, so Apache isn't needed.

```
[root@oddball-business app]# ./scripts/certbot-init.sh
docker: Error response from daemon: failed to set up container networking: driver failed programming external connectivity on endpoint hungry_ride (f8e5ec74ae53de492638286b6f0a5971e6acc7efe0b257693a691b9cb2e53c3b): Unable to enable DNAT rule:  (iptables failed: iptables --wait -t nat -A DOCKER -p tcp -d 0/0 --dport 80 -j DNAT --to-destination 172.17.0.2:80 ! -i docker0: iptables: No chain/target/match by that name.
 (exit status 1))

Run 'docker run --help' for more information
```
### Re: iptables DOCKER chain missing

Docker's NAT chains are out of sync — likely because stopping `httpd` or firewalld triggered an iptables reload that wiped Docker's rules.

Fix:

```bash
systemctl restart docker
./scripts/certbot-init.sh
```

Restarting Docker reinitializes its iptables chains. Should clear the error.

If it fails again, check whether `firewalld` is running:

```bash
systemctl status firewalld
```

If it's active, firewalld and Docker are fighting over iptables. Either stop firewalld (`systemctl stop firewalld`) or configure Docker to use `iptables=false` and manage rules via firewalld — but stopping it is simpler for a single-admin server.

❯ http://50.28.2.199.nip.io/ is not serving the wordpress site
### Re: site not serving

Check the container state first:

```bash
docker ps
docker compose logs --tail=50
```

**Also try HTTPS** — if certbot ran successfully, nginx may be redirecting HTTP → HTTPS:

```
https://50.28.2.199.nip.io/
```

If `docker ps` shows no containers (or exited ones), the stack isn't up — run:

```bash
docker compose up -d
```

Then check logs for errors. What does `docker ps` show?
```
[root@oddball-business restore]# docker ps
CONTAINER ID   IMAGE                         COMMAND                  CREATED       STATUS       PORTS      NAMES
343f0a86a4a2   wordpress:php8.3-fpm-alpine   "docker-entrypoint.s…"   2 hours ago   Up 2 hours   9000/tcp   app-wordpress-1
8cfe407b1414   redis:alpine                  "docker-entrypoint.s…"   2 hours ago   Up 2 hours   6379/tcp   app-redis-1
63f95dcdc125   mariadb:11                    "docker-entrypoint.s…"   2 hours ago   Up 2 hours   3306/tcp   app-db-1
```
### Re: nginx missing

Three containers running (wordpress-fpm, redis, mariadb) — but **no nginx**. The FPM container handles PHP, not HTTP. Without nginx there's nothing serving port 80/443.

Check what happened to it:

```bash
docker compose ps --all          # show ALL containers including stopped/failed
docker compose logs nginx        # nginx-specific logs
```

Most likely cause: nginx is failing to start because the SSL cert from certbot doesn't exist yet. If `certbot-init.sh` didn't complete successfully, the cert files aren't there, and nginx exits on startup.

Confirm by checking:

```bash
ls /etc/letsencrypt/live/
```

If that directory is empty or doesn't have `50.28.2.199.nip.io/`, certbot didn't finish. You'd need to stop the stack, fix the certbot issue (restart docker, rerun certbot-init.sh), then bring everything back up.

If certbot DID succeed, share the `docker compose logs nginx` output.

❯ Diagnose server + docker issues
❯ MONSTERRODHOLDERS_IP_ADDRESS="50.28.2.199"
❯ MONSTERRODHOLDERS_SSH_PASSWD="$(pass monsterrodholders/SSH_PASSWD)"

### Re: remote diagnosis

Can't connect via SSH — `pass monsterrodholders/SSH_PASSWD` is failing auth (both password and keyboard-interactive). Password may have changed or the pass entry may be stale.

Run these on the server:

```bash
# 1. Check all containers (including stopped/failed)
docker ps --all

# 2. Check nginx logs specifically
cd /root/app && docker compose logs nginx

# 3. Check if certbot completed
ls /etc/letsencrypt/live/

# 4. Check firewalld
systemctl is-active firewalld
```

Paste the output back. Most likely nginx is stopped because certbot didn't finish (no cert files), or there's a config error in the nginx container.

Also — update the pass entry if the server password changed:
```bash
pass edit monsterrodholders/SSH_PASSWD
```

```
[root@oddball-business restore]# docker ps --all
CONTAINER ID   IMAGE                         COMMAND                  CREATED       STATUS                   PORTS      NAMES
0ad9e51a3770   nginx:alpine                  "/docker-entrypoint.…"   3 hours ago   Exited (1) 3 hours ago              app-nginx-1
343f0a86a4a2   wordpress:php8.3-fpm-alpine   "docker-entrypoint.s…"   3 hours ago   Up 3 hours               9000/tcp   app-wordpress-1
8cfe407b1414   redis:alpine                  "docker-entrypoint.s…"   3 hours ago   Up 3 hours               6379/tcp   app-redis-1
63f95dcdc125   mariadb:11                    "docker-entrypoint.s…"   3 hours ago   Up 3 hours               3306/tcp   app-db-1
```

❯ see /home/brian/work/btakita/agent-loop/tasks/software/monsterrodholders/docker-compose.log

```
[root@oddball-business app]# ls /etc/letsencrypt/live/
ls: cannot access '/etc/letsencrypt/live/': No such file or directory
```
### Re: nginx exiting + no letsencrypt

Confirmed: nginx is crashing because it expects SSL certs at `/etc/nginx/certs/fullchain.pem` but `/etc/letsencrypt/live/` doesn't exist. Certbot was never successfully completed — it got blocked by httpd + iptables, then docker was restarted but certbot-init.sh was never rerun.

Fix sequence — run on the server:

**1. Stop the stack:**
```bash
cd /root/app
docker compose down
```

**2. Verify port 80 is free and docker is clean:**
```bash
systemctl is-active firewalld && systemctl stop firewalld
ss -tlnp | grep :80   # should show nothing
```

```
[root@oddball-business app]# ss -tlnp | grep :80
LISTEN 0      511                     *:80               *:*    users:(("httpd",pid=1006953,fd=4),("httpd",pid=1006393,fd=4),("httpd",pid=1006336,fd=4),("httpd",pid=1006290,fd=4),("httpd",pid=1006286,fd=4),("httpd",pid=1006186,fd=4))
```

**3. Run certbot to get the cert:**
```bash
./scripts/certbot-init.sh
```

**4. Confirm certs are there:**
```bash
ls /etc/letsencrypt/live/
```

**5. Bring the stack up:**
```bash
docker compose up -d
docker compose ps
```

nginx should come up green. Paste any errors if it doesn't.
### Re: httpd still on port 80

`systemctl stop firewalld` ran but httpd is still up. Stop it:

```bash
systemctl stop httpd
systemctl disable httpd
ss -tlnp | grep :80   # should now be empty
```

Then run certbot:

```bash
./scripts/certbot-init.sh
```

```
[root@oddball-business app]# ./scripts/certbot-init.sh
Saving debug log to /var/log/letsencrypt/letsencrypt.log
Certificate not yet due for renewal

- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
Certificate not yet due for renewal; no action taken.
- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
==> Certs provisioned. Start services with: docker compose up -d
[root@oddball-business app]# ls /etc/letsencrypt/live/
ls: cannot access '/etc/letsencrypt/live/': No such file or directory
```
### Re: certbot "not yet due for renewal"

That's actually a success path — certbot found a cert from a previous attempt and it's still valid (doesn't need renewal). The script printed "Certs provisioned. Start services with: docker compose up -d" which is the success signal.

The `ls /etc/letsencrypt/live/` check was on the **host** filesystem — certbot-init.sh likely runs certbot in a Docker container and stores certs in a named Docker volume, not on the host. That's why the host path is empty.

Bring up the stack:

```bash
docker compose up -d
docker compose ps
docker compose logs nginx --tail=20
```

nginx should now find the cert via the volume mount.

❯ Site cannot be reached. Why?
❯ http://50.28.2.199.nip.io/

```
[root@oddball-business app]# docker compose up -d
[+] up 5/5
 ✔ Network app_wp_net        Created                                                                                                                                                                                                       0.0s
 ✔ Container app-redis-1     Started                                                                                                                                                                                                       0.2s
 ✔ Container app-db-1        Started                                                                                                                                                                                                       0.3s
 ✔ Container app-wordpress-1 Started                                                                                                                                                                                                       0.4s
 ✔ Container app-nginx-1     Started                                                                                                                                                                                                       0.5s
[root@oddball-business app]# docker compose ps
NAME              IMAGE                         COMMAND                  SERVICE     CREATED         STATUS                                  PORTS
app-db-1          mariadb:11                    "docker-entrypoint.s…"   db          4 seconds ago   Up 3 seconds                            3306/tcp
app-nginx-1       nginx:alpine                  "/docker-entrypoint.…"   nginx       3 seconds ago   Restarting (1) Less than a second ago
app-redis-1       redis:alpine                  "docker-entrypoint.s…"   redis       4 seconds ago   Up 3 seconds                            6379/tcp
app-wordpress-1   wordpress:php8.3-fpm-alpine   "docker-entrypoint.s…"   wordpress   3 seconds ago   Up 3 seconds                            9000/tcp
[root@oddball-business app]# docker compose logs nginx --tail=20
nginx-1  | /docker-entrypoint.sh: /docker-entrypoint.d/ is not empty, will attempt to perform configuration
nginx-1  | /docker-entrypoint.sh: Looking for shell scripts in /docker-entrypoint.d/
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
nginx-1  | 2026/04/10 02:04:07 [emerg] 1#1: cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
nginx-1  | nginx: [emerg] cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
nginx-1  | /docker-entrypoint.sh: /docker-entrypoint.d/ is not empty, will attempt to perform configuration
nginx-1  | /docker-entrypoint.sh: Looking for shell scripts in /docker-entrypoint.d/
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
nginx-1  | 2026/04/10 02:04:09 [emerg] 1#1: cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
nginx-1  | nginx: [emerg] cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
```
### Re: nginx still can't find certs

The certs aren't landing where nginx expects (`/etc/nginx/certs/fullchain.pem`). "Certificate not yet due for renewal" suggests certbot found stale state from a previous run but the actual cert files aren't accessible to nginx.

Need to see where the certs are and how nginx mounts them:

```bash
# What volumes exist?
docker volume ls

# What does the compose config say about nginx mounts?
cat /root/app/docker-compose.yml
```

Also check if there's actually anything in the certbot volume:

```bash
# Try to list whatever certbot volume exists
docker run --rm -v app_certbot_certs:/certs alpine ls -la /certs/ 2>/dev/null \
  || docker run --rm -v certbot_certs:/certs alpine ls -la /certs/ 2>/dev/null \
  || echo "volume not found"
```

The certbot script claimed certs are provisioned but nginx is still failing — the cert path or volume mount is mismatched.

```
[root@oddball-business app]# docker volume ls
DRIVER    VOLUME NAME
local     app_db_data
local     app_wordpress_data
[root@oddball-business app]# cat /root/app/docker-compose.yml
cat: /root/app/docker-compose.yml: No such file or directory
[root@oddball-business app]# cat docker-compose.yml
services:
  nginx:
    image: nginx:alpine
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./nginx/nginx.conf:/etc/nginx/nginx.conf:ro
      - ./nginx/conf.d:/etc/nginx/conf.d:ro
      - wordpress_data:/var/www/html:ro
      - ./certs:/etc/nginx/certs:ro

  db:
    image: mariadb:11
    restart: unless-stopped
    environment:
      MARIADB_DATABASE: ${WORDPRESS_DB_NAME:-wordpress}
      MARIADB_USER: ${WORDPRESS_DB_USER:-wordpress}
      MARIADB_PASSWORD: ${WORDPRESS_DB_PASSWORD}
      MARIADB_ROOT_PASSWORD: ${MARIADB_ROOT_PASSWORD}
    volumes:
      - db_data:/var/lib/mysql
      - ./mysql:/etc/mysql/conf.d:ro
    networks:
      - wp_net

  redis:
    image: redis:alpine
    restart: unless-stopped
    networks:
      - wp_net

[root@oddball-business app]# docker run --rm -v app_certbot_certs:/certs alpine ls -la /certs/ 2>/dev/null \
  || docker run --rm -v certbot_certs:/certs alpine ls -la /certs/ 2>/dev/null \
  || echo "volume not found"
total 8
drwxr-xr-x    2 root     root          4096 Apr 10 02:07 .
drwxr-xr-x    1 root     root          4096 Apr 10 02:07 ..
```
### Re: certs mount is a host bind, certbot volume is empty

Now it's clear:
- nginx mounts `./certs:/etc/nginx/certs:ro` — host directory `/root/app/certs/`
- No certbot named volume exists (only `app_db_data` and `app_wordpress_data`)
- `certbot_certs` volume is empty (just created, no files)
- certbot said "not yet due for renewal" — it found a prior cert registration but **never copied files to `./certs/`**

The cert was issued at some point but the files didn't land where nginx expects them.

Check:

```bash
ls -la /opt/monsterrodholders/app/certs/
cat /opt/monsterrodholders/scripts/certbot-init.sh
```

Most likely fix: force certbot to re-issue and have it write to `/root/app/certs/`. Or, if certs exist in `/etc/letsencrypt/live/` from a previous certbot container run, copy them manually:

```bash
# Find where they are:
find /etc/letsencrypt /root/app -name "fullchain.pem" 2>/dev/null
```

```
[root@oddball-business app]# ls -alh certs/fullchain.pem certs/privkey.pem
lrwxrwxrwx 1 root root 54 Apr  9 22:12 certs/fullchain.pem -> /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem
lrwxrwxrwx 1 root root 52 Apr  9 22:12 certs/privkey.pem -> /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem
[root@oddball-business app]# ls /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem
ls: cannot access '/etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem': No such file or directory
```
### Re: broken symlinks — certbot container filesystem is gone

The symlinks in `./certs/` point to `/etc/letsencrypt/live/50.28.2.199.nip.io/` on the HOST, but that path doesn't exist. Certbot ran inside a temporary container and the cert files lived only in that container's ephemeral filesystem. Container exited → certs gone.

The cert IS valid on Let's Encrypt's side (that's why "not yet due for renewal"), but the files need to land on the host.

Fix: run certbot directly on the host, then copy real files (not symlinks) into `./certs/`:

```bash
# Stop stack to free port 80
docker compose down

# Install certbot on the host (Rocky/RHEL/CentOS)
dnf install -y certbot

# Issue cert — standalone mode binds port 80 temporarily
certbot certonly --standalone -d 50.28.2.199.nip.io --non-interactive --agree-tos -m admin@monsterrodholders.com

# Copy actual cert files (replacing the broken symlinks)
cp /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem /opt/monsterrodholders/app/certs/fullchain.pem
cp /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem /opt/monsterrodholders/app/certs/privkey.pem

# Restart the stack
docker compose up -d
docker compose logs nginx --tail=10
```

Note: certbot will say "not yet due for renewal" again — add `--force-renewal` if it skips issuance without writing files. But since the host `/etc/letsencrypt/` directory is empty, it should issue fresh.

```
[root@oddball-business app]# docker compose logs nginx --tail=10
nginx-1  | /docker-entrypoint.sh: /docker-entrypoint.d/ is not empty, will attempt to perform configuration
nginx-1  | /docker-entrypoint.sh: Looking for shell scripts in /docker-entrypoint.d/
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
nginx-1  | 2026/04/10 02:15:01 [emerg] 1#1: cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
nginx-1  | nginx: [emerg] cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
```
### Re: same error — certbot steps not run yet

Same nginx failure — the certbot/copy steps haven't been executed yet. Run the full sequence on the server:

```bash
# 1. Stop the stack
docker compose down

# 2. Install certbot on the host
dnf install -y certbot

# 3. Issue the cert (certbot binds port 80 temporarily)
certbot certonly --standalone -d 50.28.2.199.nip.io \
  --non-interactive --agree-tos \
  -m admin@monsterrodholders.com

# 4. Copy real cert files into the certs dir (replace broken symlinks)
cp /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem \
   /opt/monsterrodholders/app/certs/fullchain.pem
cp /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem \
   /opt/monsterrodholders/app/certs/privkey.pem

# 5. Restart
docker compose up -d
docker compose logs nginx --tail=10
```

Paste the output from step 3 if certbot fails.
```
[root@oddball-business app]# docker compose down
[+] down 5/5
 ✔ Container app-nginx-1     Removed                                                                                                                                                                                                       0.0s
 ✔ Container app-redis-1     Removed                                                                                                                                                                                                       0.2s
 ✔ Container app-wordpress-1 Removed                                                                                                                                                                                                       0.1s
 ✔ Container app-db-1        Removed                                                                                                                                                                                                       0.3s
 ✔ Network app_wp_net        Removed                                                                                                                                                                                                       0.1s
[root@oddball-business app]# dnf install -y certbot
Last metadata expiration check: 0:25:37 ago on Thu 09 Apr 2026 09:58:24 PM EDT.
Package certbot-3.1.0-1.el9.noarch is already installed.
Dependencies resolved.
Nothing to do.
Complete!
[root@oddball-business app]# certbot certonly --standalone -d 50.28.2.199.nip.io \
  --non-interactive --agree-tos \
  -m admin@monsterrodholders.com
Saving debug log to /var/log/letsencrypt/letsencrypt.log
Certificate not yet due for renewal

- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
Certificate not yet due for renewal; no action taken.
- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
[root@oddball-business app]# cp /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem \
   /opt/monsterrodholders/app/certs/fullchain.pem
cp: '/etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem' and '/opt/monsterrodholders/app/certs/fullchain.pem' are the same file
[root@oddball-business app]# cp /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem \
   /opt/monsterrodholders/app/certs/privkey.pem
cp: '/etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem' and '/opt/monsterrodholders/app/certs/privkey.pem' are the same file
[root@oddball-business app]# docker compose up -d
[+] up 4/5
 ✔ Network app_wp_net        Created                                                                                                                                                                                                       0.0s
 ✔ Container app-db-1        Started                                                                                                                                                                                                       0.2s
 ✔ Container app-redis-1     Started                                                                                                                                                                                                       0.3s
 ✔ Container app-wordpress-1 Started                                                                                                                                                                                                       0.4s
 ⠸ Container app-nginx-1     Starting                                                                                                                                                                                                      0.5s
Error response from daemon: failed to set up container networking: driver failed programming external connectivity on endpoint app-nginx-1 (e2af03870dac727f00dda47f2eed57bf0a2facbf352d5a80c67b0ba7be6bfda3): failed to bind host port 0.0.0.0:80/tcp: address already in use
[root@oddball-business app]# docker compose down
[+] down 5/5
 ✔ Container app-redis-1     Removed                                                                                                                                                                                                       0.2s
 ✔ Container app-nginx-1     Removed                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Removed                                                                                                                                                                                                       0.1s
 ✔ Container app-db-1        Removed                                                                                                                                                                                                       0.3s
 ✔ Network app_wp_net        Removed                                                                                                                                                                                                       0.1s
[root@oddball-business app]# systemctl stop httpd
[root@oddball-business app]# systemctl disable httpd
[root@oddball-business app]# systemctl stop httpd
[root@oddball-business app]# docker compose up -d
[+] up 5/5
 ✔ Network app_wp_net        Created                                                                                                                                                                                                       0.0s
 ✔ Container app-redis-1     Started                                                                                                                                                                                                       0.3s
 ✔ Container app-db-1        Started                                                                                                                                                                                                       0.3s
 ✔ Container app-wordpress-1 Started                                                                                                                                                                                                       0.4s
 ✔ Container app-nginx-1     Started                                                                                                                                                                                                       0.5s
[root@oddball-business app]# docker compose logs nginx --tail=10
nginx-1  | /docker-entrypoint.sh: /docker-entrypoint.d/ is not empty, will attempt to perform configuration
nginx-1  | /docker-entrypoint.sh: Looking for shell scripts in /docker-entrypoint.d/
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
nginx-1  | 2026/04/10 02:24:55 [emerg] 1#1: cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
nginx-1  | nginx: [emerg] cannot load certificate "/etc/nginx/certs/fullchain.pem": BIO_new_file() failed (SSL: error:80000002:system library::No such file or directory:calling fopen(/etc/nginx/certs/fullchain.pem, r) error:10000080:BIO routines::no such file)
```
### Re: symlinks inside Docker bind mount don't resolve in the container

Good progress — certs now exist on the host (`cp` said "same file" = the symlink resolves correctly). But the problem is the symlinks themselves. When Docker mounts `./certs:/etc/nginx/certs:ro`, the symlinks come along, and inside the nginx container they point to `/etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem` — a path that doesn't exist in the container's filesystem. nginx follows the symlink, gets a dead path, crashes.

Fix: remove the symlinks, replace with actual file copies:

```bash
cd /opt/monsterrodholders/app
rm certs/fullchain.pem certs/privkey.pem
cp /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem certs/fullchain.pem
cp /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem certs/privkey.pem

# Verify: should show -rw-r--r-- regular files, not lrwxrwxrwx symlinks
ls -la certs/

docker compose up -d
docker compose logs nginx --tail=5
```

(httpd is now disabled — port 80 is free, stack came all the way up. Just the cert content issue remains.)
```
[root@oddball-business app]# ls -la certs/
total 36
drwxr-xr-x 7 root root 4096 Apr  9 22:40 .
drwxr-xr-x 9 root root 4096 Apr  9 19:09 ..
drwx------ 3 root root 4096 Apr  9 19:13 accounts
drwx------ 3 root root 4096 Apr  9 19:13 archive
-rw-r--r-- 1 root root 2860 Apr  9 22:40 fullchain.pem
drwx------ 3 root root 4096 Apr  9 19:13 live
-rw------- 1 root root  227 Apr  9 22:40 privkey.pem
drwxr-xr-x 2 root root 4096 Apr  9 22:01 renewal
drwxr-xr-x 5 root root 4096 Apr  9 19:13 renewal-hooks
[root@oddball-business app]# docker compose up -d
[+] up 4/4
 ✔ Container app-redis-1     Running                                                                                                                                                                                                       0.0s
 ✔ Container app-db-1        Running                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Running                                                                                                                                                                                                       0.0s
 ✔ Container app-nginx-1     Started                                                                                                                                                                                                       0.1s
[root@oddball-business app]# docker compose logs nginx --tail=5
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
```
### Re: nginx is up

No certificate error in the logs — "Configuration complete; ready for start up" is the clean exit. All 4 containers running. The cert fix worked.

Test the site:
- `https://50.28.2.199.nip.io` — should load WordPress
- `http://50.28.2.199.nip.io` — should redirect to HTTPS

If WordPress loads (even if it looks like a fresh install or shows a DB error), the stack is working. Next step is the backup import.

❯ http://50.28.2.199.nip.io/ is still running apache

Same with https://50.28.2.199.nip.io/
### Re: http still hitting Apache

Docker compose logs show nginx started clean — but HTTP is still hitting Apache. Something is still holding port 80. Diagnose on the server:

```bash
# Who owns port 80?
ss -tlnp | grep ':80'

# Is httpd actually dead?
systemctl status httpd
ps aux | grep -E 'httpd|apache'
```

```
[root@oddball-business app]# ss -tlnp | grep ':80'
LISTEN 0      511                     *:80               *:*    users:(("httpd",pid=1145018,fd=4),("httpd",pid=1142304,fd=4),("httpd",pid=1142249,fd=4),("httpd",pid=1142203,fd=4),("httpd",pid=1142199,fd=4),("httpd",pid=1142170,fd=4))
[root@oddball-business app]# systemctl status httpd
● httpd.service - The Apache HTTP Server
     Loaded: loaded (/usr/lib/systemd/system/httpd.service; disabled; preset: disabled)
    Drop-In: /etc/systemd/system/httpd.service.d
             └─php-fpm.conf, php74-php-fpm.conf, php81-php-fpm.conf, php82-php-fpm.conf
     Active: active (running) since Thu 2026-04-09 22:25:02 EDT; 36min ago
       Docs: man:httpd.service(8)
   Main PID: 1142170 (httpd)
     Status: "Total requests: 190; Idle/Busy workers 100/0;Requests/sec: 0.0875; Bytes served/sec: 440 B/sec"
      Tasks: 231 (limit: 406328)
     Memory: 188.0M (peak: 191.0M)
        CPU: 1.570s
     CGroup: /system.slice/httpd.service
             ├─1142170 /usr/sbin/httpd -DFOREGROUND
             ├─1142198 /usr/sbin/httpd -DFOREGROUND
             ├─1142199 /usr/sbin/fcgi- -DFOREGROUND
             ├─1142203 /usr/sbin/httpd -DFOREGROUND
             ├─1142249 /usr/sbin/httpd -DFOREGROUND
             ├─1142304 /usr/sbin/httpd -DFOREGROUND
             └─1145018 /usr/sbin/httpd -DFOREGROUND

Apr 09 22:25:01 oddball-business.metalseed.net systemd[1]: Starting The Apache HTTP Server...
Apr 09 22:25:02 oddball-business.metalseed.net httpd[1142170]: Server configured, listening on: port 443, port 80
Apr 09 22:25:02 oddball-business.metalseed.net systemd[1]: Started The Apache HTTP Server.
[root@oddball-business app]# ps aux | grep -E 'httpd|apache'
root      191515  0.0  0.0  19244 12120 ?        Ss   Apr07   0:01 /home/interworx/bin/iworx-web -f /home/interworx/etc/httpd/httpd.conf -DSSL
iworx-w+  191518  0.0  0.0  19340  4664 ?        S    Apr07   0:00 /home/interworx/bin/fcgi-pm   -f /home/interworx/etc/httpd/httpd.conf -DSSL
root     1142170  0.0  0.2 149996 136520 ?       Ss   22:25   0:00 /usr/sbin/httpd -DFOREGROUND
apache   1142198  0.0  0.1 149556 122184 ?       S    22:25   0:00 /usr/sbin/httpd -DFOREGROUND
apache   1142199  0.0  0.1 149652 121724 ?       S    22:25   0:00 /usr/sbin/fcgi- -DFOREGROUND
apache   1142203  0.0  0.2 2550924 136124 ?      Sl   22:25   0:00 /usr/sbin/httpd -DFOREGROUND
apache   1142249  0.0  0.2 2616396 150908 ?      Sl   22:25   0:00 /usr/sbin/httpd -DFOREGROUND
apache   1142304  0.0  0.2 2485324 142316 ?      Sl   22:25   0:00 /usr/sbin/httpd -DFOREGROUND
apache   1145018  0.0  0.2 2813004 146560 ?      Sl   22:28   0:00 /usr/sbin/httpd -DFOREGROUND
iworx-w+ 1161802  0.0  0.0  19460  6104 ?        S    22:51   0:00 /home/interworx/bin/iworx-web -f /home/interworx/etc/httpd/httpd.conf -DSSL
iworx-w+ 1166129  0.0  0.0  19460  6104 ?        S    22:57   0:00 /home/interworx/bin/iworx-web -f /home/interworx/etc/httpd/httpd.conf -DSSL
root     1169157  0.0  0.0   6416  2588 pts/2    S+   23:01   0:00 grep --color=auto -E httpd|apache
```

If httpd is truly stopped, docker's port mapping may not have taken effect. Check the nginx container is actually bound:

```bash
docker compose ps
docker port app-nginx-1
```

If httpd is still running despite `systemctl stop`, force-kill it:

```bash
pkill -f httpd
# then restart nginx container to pick up port 80:
docker compose restart nginx
```

And if there's a socket unit keeping it alive:

```bash
systemctl stop httpd.socket 2>/dev/null
systemctl disable httpd.socket 2>/dev/null
```

❯ Nothing loads at https://50.28.2.199.nip.io
### Re: InterWorx is managing httpd — it keeps restarting Apache

Two things jump out from this output:

1. **httpd is still running** despite `systemctl stop httpd` — because **InterWorx** (`/home/interworx/bin/iworx-web`) is managing httpd and restarting it automatically. InterWorx is a server control panel that owns the Apache process. `systemctl stop` won't stick — InterWorx brings it back.

2. **Nothing loads at HTTPS** — nginx may have exited after Docker started, because `iworx-web` may also hold port 443. Need to verify.

Check current state:

```bash
docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps
docker compose -f /opt/monsterrodholders/app/docker-compose.yml logs nginx --tail=20
ss -tlnp | grep -E ':80|:443'
```

Likely: nginx crashed trying to bind 443 (held by iworx), container exited, port 80 released, InterWorx restarted httpd.

**The right fix for an InterWorx server:** Don't fight InterWorx for ports 80/443. Instead, run Docker nginx on internal ports and have Apache reverse proxy to it.

Change `docker-compose.yml` nginx ports from `80:80 / 443:443` to internal-only (no host port binding), and add an Apache VirtualHost in InterWorx that proxies to the Docker container:

```apache
# /etc/httpd/conf.d/monsterrodholders.conf
<VirtualHost *:80>
    ServerName 50.28.2.199.nip.io
    ProxyPreserveHost On
    ProxyPass / http://127.0.0.1:8080/
    ProxyPassReverse / http://127.0.0.1:8080/
</VirtualHost>
```

With nginx inside Docker exposed only on `127.0.0.1:8080:80` (no SSL in Docker — Apache/certbot handles SSL at the host level).

Does this server have an InterWorx control panel URL you can log into, or is direct config editing the only option?

❯ https://appendix.interworx.com/current/faqs/general-faqs.html
❯ > InterWorx does not support containerized environments, including Docker or LXC.

❯ Does this mean I cannot use Docker at all with InterWorx? Have people successfully used Docker with InterWorx?
### Re: InterWorx "no Docker" — support disclaimer, not a technical block

That FAQ statement means InterWorx won't help you if Docker breaks something — not that Docker is prevented from running. Your Docker containers are already running on this server.

The port conflict is the only real issue: InterWorx's Apache owns ports 80/443 and will always restart httpd to reclaim them. The solution is don't compete with Apache — run Docker on internal ports and proxy through Apache. This is a well-known and working pattern on InterWorx servers.

**Revised architecture: Apache handles SSL + reverse proxies to Docker**

**docker-compose.yml** — change nginx ports to internal only:
```yaml
nginx:
  ports:
    - "127.0.0.1:8080:80"   # internal only, no 443 — Apache handles SSL
```

**`/etc/httpd/conf.d/monsterrodholders.conf`**:
```apache
<VirtualHost *:80>
    ServerName 50.28.2.199.nip.io
    RewriteEngine On
    RewriteRule ^ https://%{HTTP_HOST}%{REQUEST_URI} [R=301,L]
</VirtualHost>

<VirtualHost *:443>
    ServerName 50.28.2.199.nip.io
    SSLEngine on
    SSLCertificateFile /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem
    SSLCertificateKeyFile /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem

    ProxyPreserveHost On
    ProxyPass / http://127.0.0.1:8080/
    ProxyPassReverse / http://127.0.0.1:8080/
</VirtualHost>
```

Result:
- nginx inside Docker serves HTTP only on `127.0.0.1:8080` (no SSL config needed in nginx at all)
- Apache handles SSL termination and proxies to Docker
- InterWorx remains happy — Apache still owns 80/443

Want me to write out the full revised `docker-compose.yml` and nginx config for this architecture?

❯ Is this safe or can this cause problems?
### Re: Is this safe or can this cause problems?

Yes, this is safe — it's a standard reverse proxy pattern. A few things to watch:

**Safe:**
- Binding to `127.0.0.1:8080` means the port is unreachable from the internet — only Apache on the same host can connect
- HTTP internally (loopback) is fine; traffic never leaves the server
- Apache owns SSL, certbot manages certs on the host — clean separation

**Watch for:**

**1. WordPress HTTPS detection** — Apache terminates SSL then proxies plain HTTP to Docker, so WordPress won't see `$_SERVER['HTTPS'] = 'on'`. Fix with:

In Apache config:
```apache
RequestHeader set X-Forwarded-Proto "https"
```

In `wp-config.php`:
```php
if (isset($_SERVER['HTTP_X_FORWARDED_PROTO']) && $_SERVER['HTTP_X_FORWARDED_PROTO'] === 'https') {
    $_SERVER['HTTPS'] = 'on';
}
```

❯ Do I need to add this to all of my wordpress sites? Can I just have `$_SERVER['HTTPS'] = 'on';` always set?

Without this, WordPress generates `http://` links on a page served over HTTPS → mixed content errors.

**2. mod_proxy must be enabled** — verify:
```bash
httpd -M | grep proxy
```
Should show `proxy_module`, `proxy_http_module`. If missing: `LoadModule proxy_module ...` in httpd.conf.

**3. InterWorx may regenerate Apache configs** — if InterWorx manages vhosts via its panel, it could overwrite `/etc/httpd/conf.d/monsterrodholders.conf`. Either add the domain through the InterWorx panel (if it has a reverse proxy option) or put the config somewhere InterWorx doesn't touch (check what directories it manages).

Otherwise the architecture is solid — this is exactly how most WordPress + Docker + existing Apache setups work.

❯ Is this docker-compose setup safe to use with InterWorx?
### Re: Is this docker-compose setup safe to use with InterWorx?

The docker-compose setup with `127.0.0.1:8080:80` is safe with InterWorx. Here's the breakdown:

**Why it's safe:**
- Docker daemon runs independently — InterWorx doesn't manage it and can't interfere with it
- Binding to `127.0.0.1:8080` avoids any port conflict with InterWorx's Apache (80/443)
- Docker's internal networking (172.17.x.x bridge) doesn't overlap with anything InterWorx manages

**One thing to watch — firewall:**
InterWorx typically runs CSF (ConfigServer Firewall) or APF, which also manages iptables. Docker adds its own `DOCKER` iptables chain. These usually coexist fine, but if the site is unreachable after setup, check:
```bash
csf -s   # if CSF is installed — it may need to flush/reload after Docker starts
iptables -L DOCKER -n  # verify Docker's chain is intact
```

**The main risk is the old config** — if `docker-compose.yml` still has `80:80` or `443:443` bindings from before, restarting Docker will fail to bind (InterWorx's Apache holds those ports). Make sure the ports are updated to `127.0.0.1:8080:80` before running `docker compose up`.

Simplified check before launch:
```bash
# Verify no port conflicts
ss -tlnp | grep ':80\|:443\|:8080'
# Docker should only own 127.0.0.1:8080, Apache owns 80/443
```

Otherwise: Docker + InterWorx on the same host is a common setup and works reliably when Docker stays off the controlled ports.
### Re: Do I need this on all WordPress sites? Can I just always set `$_SERVER['HTTPS'] = 'on'`?

**Do you need it on all WordPress sites?** Only sites behind a reverse proxy that terminates SSL — i.e., where PHP/WordPress receives plain HTTP while Apache upstream serves HTTPS. WordPress sites where Apache directly handles SSL with mod_ssl don't need it.

In this setup (all Docker WordPress sites proxied through Apache), yes — each Docker WP site needs it.

**Can you just always set `$_SERVER['HTTPS'] = 'on'`?** For this specific setup it would work (all external traffic comes in via Apache HTTPS, nothing hits port 8080 directly from outside). But it's a bad habit:
- Breaks local dev or staging running on plain HTTP
- Breaks internal health checks over HTTP
- The conditional check is zero cost — just use it

**Cleaner approach if you have multiple WordPress sites:** Put it in an mu-plugin so you don't touch every `wp-config.php`:

```php
// /path/to/wordpress/wp-content/mu-plugins/proxy-https.php
<?php
if (isset($_SERVER['HTTP_X_FORWARDED_PROTO']) && $_SERVER['HTTP_X_FORWARDED_PROTO'] === 'https') {
    $_SERVER['HTTPS'] = 'on';
}
```

And in Apache config (once, covering all vhosts):
```apache
RequestHeader set X-Forwarded-Proto "https"
```

That way it's configured once at the Apache layer and the mu-plugin handles the PHP side for every WP site automatically.

❯ How do I ensure that Interworx does not overwrite my apache configs?
### Re: How do I ensure InterWorx doesn't overwrite my Apache configs?

InterWorx regenerates Apache vhost configs when you change domain settings, add/renew SSL, etc. Any file it manages gets overwritten. There are two safe approaches:

**Option 1 (recommended): Use InterWorx's built-in custom httpd directives**

SiteWorx (per-domain control panel) has a "Extra httpd Configuration" or "Custom httpd Directives" field — the exact label varies by version. Directives entered there get merged into InterWorx's generated vhost config and survive regeneration.

Log into SiteWorx for monsterrodholders.com and look for something under Hosting Features → Custom httpd Configuration (or similar). Add the ProxyPass directives there rather than directly editing the .conf file.

**Option 2: Separate include file + reference from InterWorx config**

If InterWorx's generated vhost includes an `IncludeOptional` or similar directive pointing to a file you control, put your custom config there. Check the generated vhost to see if there's an include hook:

```bash
grep -i include /etc/httpd/conf.d/50.28.2.199.nip.io*.conf
```

**Option 3: Separate .conf file for the internal-only proxy (safest fallback)**

If the ProxyPass config is in a *separate* file that InterWorx didn't generate, it typically won't touch it:

```bash
# Put proxy config in a file InterWorx doesn't own
/etc/httpd/conf.d/monsterrodholders-proxy.conf
```

InterWorx manages its own files (usually named after the domain or account). A distinctly named file it didn't create is generally left alone — but this is not guaranteed across panel updates, so option 1 is more durable.

**Practical advice:** Try option 1 first via SiteWorx. If the UI doesn't expose it or the directives don't take effect, fall back to the separate .conf file and monitor after any InterWorx domain or SSL changes.

❯ I want everything to run with scripts. I want to avoid the control panel as much as possible.
### Re: I want everything to run with scripts — avoid the control panel

Good call — the separate `.conf` file approach is fully scriptable and the right path. The key is managing the conflict risk:

**The main concern:** If InterWorx already created a vhost for this domain, you'll have duplicate `VirtualHost` definitions. Apache picks one (alphabetical order in conf.d), which can cause unexpected behavior.

**Scriptable setup:**

```bash
# 1. Check if InterWorx owns a vhost for this domain
grep -r "50.28.2.199.nip.io\|monsterrodholders" /etc/httpd/conf.d/ --include="*.conf" -l

# 2. Write your proxy config (named to load BEFORE InterWorx's files)
cat > /etc/httpd/conf.d/00-monsterrodholders-proxy.conf << 'EOF'
<VirtualHost *:80>
    ServerName 50.28.2.199.nip.io
    RewriteEngine On
    RewriteRule ^ https://%{HTTP_HOST}%{REQUEST_URI} [R=301,L]
</VirtualHost>

<VirtualHost *:443>
    ServerName 50.28.2.199.nip.io
    SSLEngine on
    SSLCertificateFile /etc/letsencrypt/live/50.28.2.199.nip.io/fullchain.pem
    SSLCertificateKeyFile /etc/letsencrypt/live/50.28.2.199.nip.io/privkey.pem

    RequestHeader set X-Forwarded-Proto "https"
    ProxyPreserveHost On
    ProxyPass / http://127.0.0.1:8080/
    ProxyPassReverse / http://127.0.0.1:8080/
</VirtualHost>
EOF

# 3. Test config then reload (safe — doesn't stop Apache)
httpd -t && systemctl reload httpd
```

The `00-` prefix ensures your file loads before InterWorx's files alphabetically — Apache uses the first matching VirtualHost it encounters, so yours wins.

**If InterWorx has its own vhost for this domain:** You'll want to either remove it from InterWorx (via its CLI/API if available: check `nodeworx --help` or `/home/interworx/bin/`) or rename/disable the generated file. InterWorx typically stores these under names like `domain.conf` or `account-domain.conf` in `/etc/httpd/conf.d/`.

**Certbot for SSL (scriptable, no control panel):**
```bash
# Use webroot or standalone — don't use the Apache plugin (it modifies Apache configs)
certbot certonly --webroot -w /path/to/webroot -d 50.28.2.199.nip.io
# OR if the site isn't serving files yet:
certbot certonly --standalone -d 50.28.2.199.nip.io
```

This keeps certbot from touching your Apache config at all.

❯ http://50.28.2.199.nip.io/ did not win. https://50.28.2.199.nip.io/ has Service Unavailable
### Re: http didn't redirect, https is Service Unavailable

"Service Unavailable" on HTTPS means Apache's proxy is active but can't reach the backend on port 8080. Let's isolate:

```bash
# 1. Is Docker running and binding 8080?
docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps
ss -tlnp | grep 8080

# 2. Can Apache reach the backend directly?
curl -v http://127.0.0.1:8080/ 2>&1 | head -30

# 3. What is HTTP actually returning?
curl -v http://50.28.2.199.nip.io/ 2>&1 | head -30

# 4. Check Apache loaded your config and what vhosts are active
httpd -t
httpd -S 2>&1 | grep -E "50.28.2.199|8080|namevhost"

# 5. Check Apache error log for proxy errors
tail -30 /var/log/httpd/error_log
```

Most likely causes:
- **Docker container not running** — `docker compose ps` will show it exited
- **nginx still binding to `80:80` or `443:443`** in old docker-compose.yml — container fails to start
- **Port 8080 not bound to 127.0.0.1** — check the ports line in docker-compose.yml is `127.0.0.1:8080:80`
- **mod_proxy not enabled** — `httpd -t` would show a directive error

Run those and share the output — the error log especially will pinpoint it.

❯ Running on port 9000
```
[root@oddball-business app]# docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps
NAME              IMAGE                         COMMAND                  SERVICE     CREATED       STATUS                          PORTS
app-db-1          mariadb:11                    "docker-entrypoint.s…"   db          2 hours ago   Up 2 hours                      3306/tcp
app-nginx-1       nginx:alpine                  "/docker-entrypoint.…"   nginx       2 hours ago   Restarting (1) 11 seconds ago
app-redis-1       redis:alpine                  "docker-entrypoint.s…"   redis       2 hours ago   Up 2 hours                      6379/tcp
app-wordpress-1   wordpress:php8.3-fpm-alpine   "docker-entrypoint.s…"   wordpress   2 hours ago   Up 2 hours                      9000/tcp
```
### Re: nginx is Restarting — crash loop

nginx is crash-looping — that's why you get 503. The other containers are fine. Need to see why:

```bash
# See what's killing nginx
docker compose -f /opt/monsterrodholders/app/docker-compose.yml logs nginx --tail=30

# Also show current nginx config inside the container
docker compose -f /opt/monsterrodholders/app/docker-compose.yml exec nginx cat /etc/nginx/conf.d/default.conf 2>/dev/null || \
  docker compose -f /opt/monsterrodholders/app/docker-compose.yml exec nginx cat /etc/nginx/nginx.conf

# And the current docker-compose.yml ports section
grep -A5 'nginx:' /opt/monsterrodholders/app/docker-compose.yml | grep -A3 'ports:'
```

The most likely cause: **nginx config inside Docker still references SSL cert files that don't exist inside the container**, or it's trying to bind a port that's failing. Since WordPress is running PHP-FPM on port 9000, nginx inside Docker also needs to be configured as a FastCGI proxy (not just HTTP proxy).

Share the logs output and I can pinpoint exactly what's wrong.

❯ Why are we using nginx if InterWorx uses Apache?

```
nginx-1  | 2026/04/10 04:35:59 [emerg] 1#1: host not found in upstream "wordpress" in /etc/nginx/conf.d/wordpress.conf:27
nginx-1  | nginx: [emerg] host not found in upstream "wordpress" in /etc/nginx/conf.d/wordpress.conf:27
nginx-1  | /docker-entrypoint.sh: /docker-entrypoint.d/ is not empty, will attempt to perform configuration
nginx-1  | /docker-entrypoint.sh: Looking for shell scripts in /docker-entrypoint.d/
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/10-listen-on-ipv6-by-default.sh
nginx-1  | 10-listen-on-ipv6-by-default.sh: info: /etc/nginx/conf.d/default.conf is not a file or does not exist
nginx-1  | /docker-entrypoint.sh: Sourcing /docker-entrypoint.d/15-local-resolvers.envsh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/20-envsubst-on-templates.sh
nginx-1  | /docker-entrypoint.sh: Launching /docker-entrypoint.d/30-tune-worker-processes.sh
nginx-1  | /docker-entrypoint.sh: Configuration complete; ready for start up
```
### Re: Why nginx inside Docker? And the crash log

**Why nginx inside Docker:** WordPress is running as PHP-FPM (`php8.3-fpm-alpine`) — it only speaks FastCGI, not HTTP. Something inside Docker must handle HTTP requests and proxy PHP files to FastCGI port 9000. That's nginx's role. The stack is:

```
Internet → Apache (SSL termination) → nginx:8080 (HTTP→FastCGI) → WordPress PHP-FPM:9000
```

**The crash cause:** `host not found in upstream "wordpress"` — nginx can't DNS-resolve the `wordpress` service name inside Docker. This means nginx and wordpress are either on different Docker networks, or the network isn't configured yet.

Check the docker-compose.yml:
```bash
cat /opt/monsterrodholders/app/docker-compose.yml
```

The fix is usually adding an explicit shared network:
```yaml
networks:
  default:
    name: app-network

services:
  nginx:
    # no explicit networks: needed — uses default
  wordpress:
    # no explicit networks: needed — uses default
```

Or verify all services use the same network. If docker-compose has explicit `networks:` keys that put nginx and wordpress on different networks, they can't reach each other.

The nginx config at `/etc/nginx/conf.d/wordpress.conf:27` likely has `fastcgi_pass wordpress:9000;` — that syntax is correct. The problem is purely DNS resolution within Docker.

**Alternative to simplify:** Replace `wordpress:php8.3-fpm-alpine` with `wordpress:apache` — then you don't need nginx inside Docker at all, just Apache inside Docker serving WordPress directly. Host Apache proxies to Docker Apache on 8080. Removes one moving part.

❯ Why not just proxy pass to port 9000?
❯ Or have Nginx use port 8080
### Re: proxy to port 9000 / nginx on 8080

**Why not proxy-pass to port 9000?**

Port 9000 is FastCGI — not HTTP. Apache can't `ProxyPass` to it with standard HTTP proxy. FastCGI is a binary protocol for PHP processes, not a web server. You'd need `mod_proxy_fcgi` + `SetHandler "proxy:fcgi://..."`, which forces Apache to also handle the static-file / PHP routing logic nginx handles now. Possible, but you'd be duplicating nginx's job.

Also, port 9000 is currently only reachable inside the Docker network — it's not published to the host. You'd need to expose it first.

**Or have Nginx use port 8080** — that's already what the setup does: nginx inside Docker listens on container port 80, mapped to host port 8080. The crash is purely the DNS bug (`host not found in upstream "wordpress"`) — nginx can't resolve the `wordpress` service name. Cause: the two containers are on different Docker networks (or networks aren't configured). One look at `docker-compose.yml` will confirm.

Fix: ensure both services share a default network (or an explicit shared one):
```yaml
networks:
  default:
    name: app-network
```

**Simplest path if you want to skip debugging nginx:** Switch to `wordpress:apache` image. Apache inside Docker handles PHP + static files, listens on 80, map to host 8080, host Apache proxies to it. Eliminates FastCGI and the nginx DNS issue entirely.

❯ How does it perform compared to the nginx solution?
### Re: `wordpress:apache` vs nginx + PHP-FPM performance

**nginx + PHP-FPM — faster at scale:**
- Event-driven, low memory per connection
- PHP workers only invoked for PHP requests — static files bypass PHP entirely
- Better under high concurrency (hundreds of simultaneous requests)

**`wordpress:apache` (mod_php) — adequate for this site:**
- Apache prefork embeds PHP in every worker, including static file requests → higher per-worker memory
- At low traffic, the difference is imperceptible
- Zero FastCGI config, no nginx DNS debugging

**For monsterrodholders:** Use `wordpress:apache`. The performance gap only matters at scale — thousands of concurrent visitors. A rod holders shop isn't in that range. The nginx + PHP-FPM setup is correct for high-traffic WooCommerce, but here it's complexity with no measurable return.

❯ do
❯ - [ ] Switch docker-compose to `wordpress:apache` (drop nginx service)
### Re: switch docker-compose to wordpress:apache

Done. `docker-compose.yml` updated and pushed.

**Changes:**
- Removed `nginx` service entirely
- Changed `wordpress:php8.3-fpm-alpine` → `wordpress:apache`
- Added `ports: - "127.0.0.1:8080:80"` to wordpress service
- Removed nginx volumes, certs volume mount, nginx depends_on

**To apply on the server:**
```bash
cd /opt/monsterrodholders/app
git pull
docker compose down
docker compose up -d
docker compose ps
```

The wordpress container will now serve HTTP on host port 8080. Host Apache/InterWorx proxies to it. Verify with:
```bash
curl -s http://localhost:8080/ | head -5
```

❯ Looks like nginx would be better?
```
[root@oddball-business app]# docker compose up -d
[+] up 28/28
 ✔ Image wordpress:apache Pulled                                                                                                                                                                                                           8.6s
 ✘ Network app_wp_net     Error Error response from daemon: Failed to Setup IP tables: Unable to enable ACCEPT OUTGOING rule:  (iptables failed: iptables --wait -t filter -A DOCKER-FORWARD -i br-48f62a99c14d -j ACCEPT: iptables: N...  0.0s
failed to create network app_wp_net: Error response from daemon: Failed to Setup IP tables: Unable to enable ACCEPT OUTGOING rule:  (iptables failed: iptables --wait -t filter -A DOCKER-FORWARD -i br-48f62a99c14d -j ACCEPT: iptables: No chain/target/match by that name.
 (exit status 1))
```
### Re: iptables error — not a nginx vs apache issue

That error is a Docker/iptables problem, unrelated to the image choice. The `DOCKER-FORWARD` chain doesn't exist in the kernel's iptables, so Docker can't create the `app_wp_net` bridge network. This would fail identically with nginx.

**Cause:** firewalld (which InterWorx uses) cleared Docker's iptables chains — happens after a firewalld reload or kernel update.

**Fix — restart Docker:**
```bash
systemctl restart docker
docker compose -f /opt/monsterrodholders/app/docker-compose.yml up -d
docker compose -f /opt/monsterrodholders/app/docker-compose.yml ps
```

**If that fails — reload iptables modules:**
```bash
modprobe ip_tables iptable_filter iptable_nat
systemctl restart docker
docker compose -f /opt/monsterrodholders/app/docker-compose.yml up -d
```

❯ Do I need to install wordpress?
❯ https://50.28.2.199.nip.io/wp-admin/install.php
### Re: WordPress installer at nip.io

Docker is running and host Apache is proxying correctly — good.

**Whether to run the installer depends on backup format:**

**All-in-One WP Migration (`.wpress` file):**
Yes — run the installer first (any temp credentials, any title). Then:
1. Install + activate the All-in-One WP Migration plugin
2. Go to All-in-One WP Migration → Import → upload the `.wpress` file
3. The import overwrites the database, wp-content, and wp-config credentials entirely

**Raw SQL dump + wp-content folder:**
Skip the installer. Import the SQL directly, copy wp-content, done.

What format is the backup?

❯ zip with a *.sql file. I want to use a new theme. Have it exclude all plugins.
### Re: zip with SQL — skip installer, imports only

Raw SQL + new theme + no plugins → only two things to import: **database** and **uploads**.

**Step 1 — get the zip onto the server:**
```bash
# From your local machine:
scp /path/to/backup.zip root@50.28.2.199:/opt/monsterrodholders/app/
```

**Step 2 — extract and check contents:**
```bash
cd /opt/monsterrodholders/app
unzip backup.zip -d /tmp/backup
ls /tmp/backup/          # confirm .sql file location
ls /tmp/backup/wp-content/uploads/ 2>/dev/null  # check if uploads are included
```

**Step 3 — import the database:**
```bash
source .env
docker compose exec -T db mariadb \
  -u"${WORDPRESS_DB_USER}" -p"${WORDPRESS_DB_PASSWORD}" "${WORDPRESS_DB_NAME}" \
  < /opt/monsterrodholders/restore/backup.sql
```
Replace `your-backup.sql` with the actual filename.

**Step 4 — copy uploads only (skip plugins + themes):**
```bash
WP_CTR=$(docker compose ps -q wordpress)
docker compose exec wordpress mkdir -p /var/www/html/wp-content/uploads
docker cp /tmp/backup/wp-content/uploads/. ${WP_CTR}:/var/www/html/wp-content/uploads/
docker compose exec wordpress chown -R www-data:www-data /var/www/html/wp-content/uploads
```
If the zip doesn't include a wp-content folder, skip this — uploads can be added later.

**Step 5 — fix URLs (what's the old domain?):**
```bash
docker compose run --rm wpcli wp search-replace 'https://OLD-DOMAIN.com' 'https://50.28.2.199.nip.io' --all-tables
```

What's the old domain on the SQL dump?
❯ Can I switch the domain back to monsterrodholders.com once we test?
### Re: domain switching + backup path confirmed

Yes — switching back to `monsterrodholders.com` is the standard flow:

1. **Import SQL** → DB has `monsterrodholders.com` as siteurl
2. **search-replace → nip.io** for testing on the new server
3. **Verify** everything looks correct on nip.io
4. **Flip DNS** to the new server IP (`50.28.2.199`)
5. **Run certbot** for a real SSL cert on `monsterrodholders.com`
6. **search-replace → monsterrodholders.com** (switch back from nip.io)

The site is only on nip.io temporarily — users on the live site never see it.

**Confirm old domain before import:**
```bash
grep -m1 'siteurl' /opt/monsterrodholders/restore/backup.sql
```
Expected output: something like `'https://monsterrodholders.com'` — paste it and I'll fill in the exact search-replace command.
❯ Can I copy the container. Do the switch in the current container...then restore the old container?
### Re: no need to snapshot — domain switch is reversible

The domain is just two rows in `wp_options` (`siteurl` and `home`). `wp search-replace` rewrites them — run it in either direction:

```bash
# Switch to nip.io for testing
docker compose run --rm wpcli wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables

# Switch back when ready to go live
docker compose run --rm wpcli wp search-replace 'https://50.28.2.199.nip.io' 'https://monsterrodholders.com' --all-tables
```

**If you want a rollback point for the full import** (wrong content, bad SQL, etc.), snapshot the DB volume before importing:

```bash
# Before import — save DB volume to disk
docker run --rm \
  -v app_db_data:/source \
  -v /opt/monsterrodholders:/backup \
  alpine tar czf /backup/db_before_import.tar.gz -C /source .

# Restore if needed
docker compose down
docker run --rm \
  -v app_db_data:/target \
  -v /opt/monsterrodholders:/backup \
  alpine tar xzf /backup/db_before_import.tar.gz -C /target
docker compose up -d
```

```
Image wordpress:cli Pulled
Container app-wpcli-run-be2a32ab87ee Creating
Container app-wpcli-run-be2a32ab87ee Created
Error: The site you have requested is not installed.
Your table prefix is 'wp_'. Found installation with table prefix: wp_nxz1mfc9cg_.
Or, run `wp core install` to create database tables.
```
### Re: table prefix mismatch

Good news: the SQL imported successfully — the tables exist with prefix `wp_nxz1mfc9cg_`. The problem is `wp-config.php` defaults to `wp_`, so WP-CLI can't find the tables.

**Fix — update table prefix in `.env`:**
```bash
# Edit /opt/monsterrodholders/app/.env — change or add:
WORDPRESS_TABLE_PREFIX=wp_nxz1mfc9cg_
```

❯ The .env variable was already set. How do I get the docker env to use .env when it loads?

Then restart to regenerate `wp-config.php`:
```bash
cd /opt/monsterrodholders/app
docker compose down && docker compose up -d

# Now search-replace will work:
docker compose run --rm wpcli wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables
```
### Re: `.env` doesn't flow into containers automatically

Two things are needed:

**Immediate fix — edit `wp-config.php` directly (already-running container):**
```bash
docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
# fix it:
docker compose exec wordpress sed -i "s/\$table_prefix = 'wp_';/\$table_prefix = 'wp_nxz1mfc9cg_';/" /var/www/html/wp-config.php
# verify:
docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
```

**Why `.env` alone isn't enough:**
Docker Compose reads `.env` for *compose file variable interpolation* (`${VAR}` syntax in the YAML). But that doesn't pass vars into containers — only `environment:` entries or `env_file:` do.

**Permanent fix — make the compose file pass it through:**

In `docker-compose.yml`, find the `wordpress:` service's `environment:` block and add:
```yaml
      WORDPRESS_TABLE_PREFIX: ${WORDPRESS_TABLE_PREFIX}
```

Or add `env_file: .env` to the wordpress service to pass all `.env` vars into the container automatically.

❯ `env_file: .env` seems simplest. Correct?

After patching `wp-config.php`, WP-CLI should work without a restart. Then proceed with the search-replace.
### Re: `env_file: .env` — yes, and one caveat

Yes, simplest. Add it to the wordpress service:

```yaml
  wordpress:
    env_file: .env
    environment:
      # ... existing vars still work, env_file fills in the rest
```

❯ Do this in the monsterrodholders-dev repo...correct?

Caveat: `env_file` passes **all** `.env` vars to the container (DB passwords, etc.) — fine for a private server, just broader than a targeted `environment:` entry.

**But `wp-config.php` already exists** — the WordPress entrypoint only generates it on first boot, so the env var won't take effect automatically even after `docker compose up -d`. You still need one of:

1. **Patch directly** (no restart, fastest):
   ```bash
   docker compose exec wordpress sed -i "s/\$table_prefix = 'wp_';/\$table_prefix = 'wp_nxz1mfc9cg_';/" /var/www/html/wp-config.php
   docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
   ```

2. **Delete + regenerate** (restarts wordpress container):
   ```bash
   docker compose exec wordpress rm /var/www/html/wp-config.php
   docker compose restart wordpress
   docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
   ```

Recommend option 1 (sed) — it's immediate and doesn't require a restart.
### Re: monsterrodholders-dev already handles this

Good news — the local `monsterrodholders-dev/docker-compose.yml` **already has** `WORDPRESS_TABLE_PREFIX: ${WORDPRESS_TABLE_PREFIX:-wp_}` in the wordpress service's environment block. No change needed there.

The production server's compose file at `/opt/monsterrodholders/app/docker-compose.yml` is what matters right now. Check if it matches:
```bash
grep -A 20 "wordpress:" /opt/monsterrodholders/app/docker-compose.yml
```

If the server's compose file is missing `WORDPRESS_TABLE_PREFIX`, copy the local one up:
```bash
scp src/monsterrodholders-dev/docker-compose.yml root@50.28.2.199:/opt/monsterrodholders/app/docker-compose.yml
```

**But the `sed` fix on `wp-config.php` is still required regardless** — the env var only takes effect when the container first generates `wp-config.php`. Since it already exists, patch it directly:
```bash
docker compose exec wordpress sed -i "s/\$table_prefix = 'wp_';/\$table_prefix = 'wp_nxz1mfc9cg_';/" /var/www/html/wp-config.php
docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
```

```
[root@oddball-business app]# docker compose exec wordpress sed -i "s/\$table_prefix = 'wp_';/\$table_prefix = 'wp_nxz1mfc9cg_';/" /var/www/html/wp-config.php
[root@oddball-business app]# docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
$table_prefix = getenv_docker('WORDPRESS_TABLE_PREFIX', 'wp_');
[root@oddball-business app]# docker compose run --rm wpcli wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables
WARN[0000] Found orphan containers ([app-nginx-1]) for this project. If you removed or renamed this service in your compose file, you can run this command with the --remove-orphans flag to clean it up.
[+]  2/2t 2/22
 ✔ Container app-db-1        Running                                                                                                                                                                                                       0.0s
 ✔ Container app-wordpress-1 Running                                                                                                                                                                                                       0.0s
Container app-wpcli-run-8fc99f9ab543 Creating
Container app-wpcli-run-8fc99f9ab543 Created
Error: The site you have requested is not installed.
Your table prefix is 'wp_'. Found installation with table prefix: wp_nxz1mfc9cg_.
Or, run `wp core install` to create database tables.
```
### Re: `getenv_docker` — env var not set in container

The `sed` changed nothing because the pattern didn't match. The file already uses:
```
$table_prefix = getenv_docker('WORDPRESS_TABLE_PREFIX', 'wp_');
```

If it already uses `getenv_docker('WORDPRESS_TABLE_PREFIX', 'wp_');`, why do we need to hard code the prefix?

`getenv_docker` reads `WORDPRESS_TABLE_PREFIX` from the container's environment. Neither the `wordpress` nor `wpcli` container has it set → falls back to `wp_`.

**Option 1 — Hardcode in `wp-config.php` (fastest, no restart):**
```bash
docker compose exec wordpress sed -i "s/getenv_docker('WORDPRESS_TABLE_PREFIX', 'wp_')/'wp_nxz1mfc9cg_'/" /var/www/html/wp-config.php
docker compose exec wordpress grep table_prefix /var/www/html/wp-config.php
```

**Option 2 — Fix the production compose file + recreate (durable fix):**

Edit `/opt/monsterrodholders/app/docker-compose.yml` on the server — add `WORDPRESS_TABLE_PREFIX` to both `wordpress` AND `wpcli` services:
```yaml
  wordpress:
    environment:
      WORDPRESS_TABLE_PREFIX: wp_nxz1mfc9cg_
      # ...

  wpcli:
    environment:
      WORDPRESS_TABLE_PREFIX: wp_nxz1mfc9cg_
      # ...
```
Then: `docker compose up -d --force-recreate wordpress`

Note: the local `monsterrodholders-dev` compose has `WORDPRESS_TABLE_PREFIX: ${WORDPRESS_TABLE_PREFIX:-wp_}` for wordpress but is also missing it on wpcli — fix both when you update.

Recommend Option 1 now to unblock, then Option 2 to clean up the compose files.

❯ Do Option 2
### Re: why hardcode, and doing Option 2

**Re the question:** `getenv_docker` would work if the env var was set in the container — but the production compose file's `wordpress` and `wpcli` services don't include `WORDPRESS_TABLE_PREFIX` in their `environment:` blocks, so the containers start without it and fall back to `wp_`. Option 2 fixes the root cause by properly passing the var through.

**Done — local `monsterrodholders-dev/docker-compose.yml` updated:** Added `WORDPRESS_TABLE_PREFIX: ${WORDPRESS_TABLE_PREFIX:-wp_}` to the `wpcli` service (it was already on `wordpress`).

Now deploy and recreate on the server:

```bash
# Copy updated compose file to server
scp src/monsterrodholders-dev/docker-compose.yml root@50.28.2.199:/opt/monsterrodholders/app/docker-compose.yml

# Verify .env has WORDPRESS_TABLE_PREFIX set
ssh root@50.28.2.199 "grep WORDPRESS_TABLE_PREFIX /opt/monsterrodholders/app/.env"

# Recreate wordpress container so it picks up the new env var
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && docker compose up -d --force-recreate wordpress"

# Verify wp-config.php resolves to correct prefix
ssh root@50.28.2.199 "docker compose -f /opt/monsterrodholders/app/docker-compose.yml exec wordpress php -r \"require '/var/www/html/wp-config.php'; echo \\\$table_prefix;\""

# Run the search-replace
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && docker compose run --rm wpcli wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables"
```

The verify step should print `wp_nxz1mfc9cg_` — if it does, the search-replace will work.
❯ Make certbot-init.sh have 600 permissions.
### Re: certbot-init.sh permissions

Git can only track two modes: `644` (non-executable) or `755` (executable). It cannot store `600`. The file is currently `644` in git.

Options:

1. **`chmod 600` on the server after deploy** — protects from group/other read; call it with `bash scripts/certbot-init.sh` (no execute bit):
   ```bash
   ssh root@50.28.2.199 "chmod 600 /opt/monsterrodholders/scripts/certbot-init.sh"
   ```
   
❯ Problem is I have a dirty git tree on production. Can the change be ignored on the server so I can pull?

2. **Add to deploy script or Makefile** so it's applied automatically on every deploy.

3. **`chmod 700`** — owner-only execute; git stores as `755` in the index but `700` is applied locally. Same limitation: git doesn't round-trip 700 cleanly.

If the goal is protecting sensitive content from other server users, option 1 is quickest. Want a Makefile `deploy` target that does the `chmod` automatically?
### Re: dirty git tree on production

Easiest options:

**Option A — Skip git pull, just `scp`** (cleanest for production servers with local tweaks):
```bash
scp src/monsterrodholders-dev/docker-compose.yml root@50.28.2.199:/opt/monsterrodholders/app/docker-compose.yml
```
No git state to manage — file lands exactly where needed.

**Option B — Stash, pull, pop:**
```bash
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && git stash && git pull && git stash pop"
```

**Option C — Discard dirty changes and pull** (only if the dirty changes are throwaway):
```bash
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && git checkout -- . && git pull"
```

Recommend Option A — production configs often have local tweaks that shouldn't be in git. The `scp` gives explicit control over what changes without touching git state.

❯ commit and push monsterrodholders-dev
### Re: commit and push monsterrodholders-dev

Done. Committed and pushed:
- `docker-compose.yml` — added `WORDPRESS_TABLE_PREFIX: ${WORDPRESS_TABLE_PREFIX:-wp_}` to `wpcli` service
- `docs/server-setup.md` — minor line-wrap fix (already in the working tree)

Also updated the `agent-loop` submodule pointer.

❯ Can certbot-init.sh be a 755 permission?
### Re: certbot-init.sh → 755

Yes — `755` is correct for scripts. Git tracks the executable bit, so this is the right way to do it (unlike `600` or `700` which git can't store).

Done: `chmod 755 scripts/certbot-init.sh`, committed, pushed, and submodule pointer updated.

On the server after deploying: `./scripts/certbot-init.sh` will work directly.

❯ commit + push
### Re: commit + push

Done — pushed `agent-loop` to remote. Had ~20 unpushed commits (all the agent-doc monsterrodholders session commits + two submodule pointer updates).

❯ https://50.28.2.199.nip.io/ redirects to https://monsterrodholders.com/
❯ http://50.28.2.199.nip.io/ is back to the apache server.

❯ Please help me fix.
### Re: HTTP → Apache + HTTPS → redirect (HEAD)

Two separate issues:

**1. HTTP → Apache:** Apache (host service) is still running on port 80 and intercepting requests before Docker nginx. Stop it:
```bash
ssh root@50.28.2.199 "systemctl stop apache2 && systemctl disable apache2"
```

**2. HTTPS → redirects to monsterrodholders.com:** WordPress `siteurl`/`home` are still set to `monsterrodholders.com` — the `wp search-replace` was still pending. Run it:
```bash
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && docker compose run --rm wpcli wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables"
```

Also set the options directly (belt + suspenders):
```bash
ssh root@50.28.2.199 "cd /opt/monsterrodholders/app && docker compose run --rm wpcli wp option update siteurl 'https://50.28.2.199.nip.io' && docker compose run --rm wpcli wp option update home 'https://50.28.2.199.nip.io'"
```

After both — verify `http://50.28.2.199.nip.io/` hits nginx and `https://50.28.2.199.nip.io/` loads without redirect.
<!-- agent:boundary:f40782e9 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Stop Apache on host: `systemctl stop apache2 && systemctl disable apache2`
- [ ] `wp search-replace 'https://monsterrodholders.com' 'https://50.28.2.199.nip.io' --all-tables`
- [ ] Update `siteurl` and `home` wp options to `https://50.28.2.199.nip.io`
- [ ] Verify `http://50.28.2.199.nip.io/` loads via nginx (not Apache)
- [ ] Verify `https://50.28.2.199.nip.io/` loads without redirect
- [ ] Copy uploads into wordpress container (if not done)
- [ ] Install + activate new theme
<!-- /agent:pending -->
