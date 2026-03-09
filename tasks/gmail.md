---
agent_doc_session: 53c928b3-2ad9-4007-9f13-f5035e35aafa
tmux_session: '0'
agent_doc_mode: template
---


# Gmail & Calendar Integration

## Gmail Filters

- Synced via `corky filter check` / `corky filter push`
- Filters: `for-lucas` label includes `*@*.mission.dev`, `*@mission.dev`, `*@us.greenhouse-jobs.com`

## Email Drafting

- Drafted and sent reply to Jonathan Martinez (Cast & Crew) re: Senior Python Software Engineer position
- Availability: Thu/Mon 1-5 PM ET, Fri 4-5 PM ET
- Note: `in_reply_to` was empty (corky doesn't store Message-IDs during sync) — Gmail threads by subject, so this works fine
- Draft location: `~/Documents/mail/` is outside credential bubbling path — had to move to `~/work/btakita/mail/drafts/` for `corky draft push`

## Calendar Management (corky v0.13.1)

Implemented `corky cal` module with 3 subcommands:

```bash
corky cal auth                          # OAuth2 (reuses Gmail credentials, calendar:* token keys)
corky cal list [--limit N] [--query Q]  # list upcoming events
corky cal delete "query" [--all] [--dry-run]  # delete events (--all for recurring series)
```

**Files:** `src/cal/{mod,auth,list,delete}.rs`, CLI wiring in `src/{cli,main,lib}.rs`

**Bugs fixed during implementation:**
- 403: Calendar API must be enabled separately in Google Cloud Console
- 400: `timeMin` RFC3339 `+00:00` not URL-encoded (`+` → space) — fixed with `urlencode_pub()`
- Recurring series: `--all` now deduplicates by `recurring_event_id` and deletes series root event (one API call per series)

**Released v0.13.1** — crates.io, PyPI, GitHub Releases: https://github.com/btakita/corky/releases/tag/v0.13.1

## User

commit and push mail and submodules
