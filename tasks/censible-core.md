---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** AUTH0_AUDIENCE set ✓. Contabo investigated — no PostgreSQL, no CSVs. Finegar DB is on Heroku (503/down). Need: Heroku PG credentials or pg_dump backup for D1 import.
<!-- /agent:status -->

<!-- agent:exchange -->

### Contabo Investigation Results

**SSH'd into 207.244.234.186** — findings:

- **No PostgreSQL** on Contabo. Only 3 Docker containers: `traefik`, `mi-www_site-1`, `mi-redis`
- **No `esg_mi_*.csv` files** found anywhere in the container
- mi-www uses **Contabo Object Storage (S3)** for file data (`USE_S3=1`, endpoint: `usc1.contabostorage.com`)
- Data files found in container match what's already in `root-web/public/`: `wil_zero_US-region.json`, `women_in_leadership_sp1500_CEOs.json`, `esg_ratings_keys.json`

### Finegar Database Location

The finegar database is **on Heroku PostgreSQL**, not Contabo:
- `CENSIBLE_SEARCH_URL=https://censible-search.herokuapp.com` (currently returning **503 — maintenance mode**)
- `FINEGAR_PG_URL` is blank in all `.env` files — needs Heroku credentials
- The finegar-api Flask app ran as a Heroku dyno with Heroku Postgres addon

### What's Needed for D1 Import

To port finegar search data to SQLite/D1, I need access to the PostgreSQL data. Options:

**Option A: Heroku CLI access**
- Install `heroku` CLI and login
- `heroku pg:pull DATABASE_URL finegar_local -a censible-search` (pulls DB locally)
- Then I can dump the tables and create D1 migrations

**Option B: Existing pg_dump backup**
- If you have a PostgreSQL dump file (`.sql` or `.dump`) from finegar, provide the path

**Option C: Heroku database credentials**
- Go to https://dashboard.heroku.com/apps/censible-search/resources
- Click the Heroku Postgres addon → Settings → View Credentials
- Provide the connection string and I'll dump it remotely

### Completed This Round

- ✅ `AUTH0_AUDIENCE` secret set to `https://censible.auth0.com/api/v2/`
- ✅ SSH'd into Contabo — confirmed no PostgreSQL, no CSV data files
- ✅ Analyzed finegar-api schema (40+ tables, all SQL queries documented)
- ✅ Confirmed search API is down (Heroku 503)

I have a usb drive which may contain the csv files:
Please help me track the usb drive to mount
```
❯ lsusb
Bus 001 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
Bus 001 Device 002: ID 0db0:0076 Micro Star International MYSTIC LIGHT
Bus 001 Device 003: ID 6655:a583 Best Buy NS-PCNVMEHDE(-C)
Bus 002 Device 001: ID 1d6b:0003 Linux Foundation 3.0 root hub
Bus 003 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
Bus 003 Device 002: ID 0489:e10a Foxconn / Hon Hai
Bus 004 Device 001: ID 1d6b:0003 Linux Foundation 3.0 root hub
Bus 005 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
Bus 006 Device 001: ID 1d6b:0003 Linux Foundation 3.0 root hub
Bus 007 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
Bus 008 Device 001: ID 1d6b:0003 Linux Foundation 3.0 root hub
Bus 009 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
Bus 009 Device 002: ID 05e3:0610 Genesys Logic, Inc. Hub
Bus 009 Device 003: ID 05e3:0610 Genesys Logic, Inc. Hub
Bus 009 Device 004: ID 03f0:0b8b HP, Inc HyperX SoloCast
Bus 009 Device 005: ID 05ac:0265 Apple, Inc. Magic Trackpad
Bus 009 Device 008: ID 3938:1390 Sonix Technology Co., Ltd. onn 4K Webcam
Bus 009 Device 009: ID 05ac:0265 Apple, Inc. Magic Trackpad
Bus 009 Device 010: ID 1d50:615e OpenMoko, Inc. SliceMK ErgoDox
Bus 010 Device 001: ID 1d6b:0003 Linux Foundation 3.0 root hub
Bus 010 Device 002: ID 05e3:0626 Genesys Logic, Inc. Hub
Bus 010 Device 003: ID 05e3:0625 Genesys Logic, Inc. USB3.2 Hub
Bus 011 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub
```
<!-- /agent:exchange -->
