---
agent_doc_session: 6c57ef6d-db13-43df-a68d-0e9c4372110b
tmux_session: '1'
agent_doc_format: template
agent_doc_write: crdt
---

# Email Management

## Session History

- Pushed 2 Gmail filters: **correspondence** (17 addresses → importance) and **for-lucas** (14 addresses → forward to `dev@briantakita.me` + star)
- Synced mail + committed (1474 files)
- Discussed multi-account Gmail: IMAP sync supported, filters are single-account
- Proposed `btak-dev` account config (not yet applied)

## Status

<!-- agent:status -->
- [x] btak.dev account added to .corky.toml
- [ ] Gmail API sync provider (Option B) — planning
<!-- /agent:status -->

## Exchange

<!-- agent:output mode=append -->
Converted to template mode and compacted conversation history. The document now uses `<!-- agent:input -->` / `<!-- agent:output -->` components.

Pending from last session: adding `btak-dev` (btak.dev@gmail.com) account to `mail/.corky.toml`. Want me to proceed with that?

Yes. Add btak-dev
Added `btak-dev` account to `mail/.corky.toml`:

```toml
[accounts.btak-dev]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak-dev"
labels = ["INBOX"]
```

**Assumptions:**
- Password stored at `pass email/btak-dev` — adjust if your pass path differs
- `labels = ["INBOX"]` — add more labels if you want specific label sync
- No `default = true` since the personal account is already default

**Next steps:**
- Ensure `pass email/btak-dev` exists (Gmail app password)
- Run `corky sync` to verify the new account connects
- Add Gmail API OAuth for btak-dev if you want filter management on this account too

Want me to adjust anything?

Change btak-dev to btak.dev

`pass email/btak.dev` is available
Renamed account key from `btak-dev` to `btak.dev` in `mail/.corky.toml`:

```toml
[accounts."btak.dev"]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak-dev"
labels = ["INBOX"]
```

Note: TOML keys with dots need quoting (`"btak.dev"`), so the key is `accounts."btak.dev"`. The `password_cmd` path (`pass email/btak-dev`) still uses a hyphen since pass paths don't need to match the account key — adjust if you prefer `pass email/btak.dev`.
Updated `password_cmd` to match the available pass path:

```toml
[accounts."btak.dev"]
provider = "gmail"
user = "btak.dev@gmail.com"
password_cmd = "pass email/btak.dev"
labels = ["INBOX"]
```

Ready for `corky sync` whenever you want to test it.

```
=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting to imap.gmail.com:993 as btak.dev@gmail.com
  Error syncing btak.dev: No Response: [AUTHENTICATIONFAILED] Invalid credentials (Failure)
```
IMAP auth failed — the password in `pass email/btak.dev` isn't accepted by Gmail. This usually means one of:

1. **App password needed**: Gmail with 2FA requires an [App Password](https://myaccount.google.com/apppasswords), not your regular password. Generate one under "Security → App passwords" and update:
   ```bash
   pass edit email/btak.dev   # paste the 16-char app password
   ```

I logged into btak.dev@gmail.com...wen to the /apppasswords link.
`The setting you are looking for is not available for your account.`

2. **IMAP not enabled**: Go to Gmail Settings → "See all settings" → "Forwarding and POP/IMAP" → enable IMAP for `btak.dev@gmail.com`.

3. **Wrong password stored**: Verify the stored value:
   ```bash
   pass email/btak.dev   # check it looks right (no trailing newlines/spaces)
   ```

Most likely it's #1 — app password. Generate one and update the pass entry, then retry `corky sync`.
App Passwords require 2-Step Verification to be enabled first. For a personal Gmail account:

1. **Enable 2FA first**: Go to [Google 2-Step Verification](https://myaccount.google.com/signinoptions/two-step-verification) and turn it on for `btak.dev@gmail.com`
2. **Then generate App Password**: After 2FA is active, [App Passwords](https://myaccount.google.com/apppasswords) will become available. Create one for "Mail" and save the 16-char code:
   ```bash
   pass edit email/btak.dev   # replace with the app password
   ```
3. **Retry sync**: `corky sync`

Alternatively, if you don't want 2FA on this account, corky could use **OAuth2 (Gmail API)** instead of IMAP password auth — but that requires `provider = "gmail-api"` support in corky. The current `provider = "gmail"` uses IMAP which needs either the account password (with "less secure apps" — deprecated by Google) or an app password (requires 2FA).

**TL;DR**: Enable 2FA → generate app password → update pass → retry.

Let's do OAuth2 (Gmail API)
Corky already has Gmail OAuth2 infrastructure — but only for **filter management**, not email sync. Here's the current state:

**What exists:**
- Full OAuth2 flow in `src/corky/src/filter/gmail_auth.rs` (token exchange, refresh, storage)
- Gmail API client for filters and labels
- OAuth credentials already configured in `.corky.toml` (`[gmail]` section)

**What's missing for Gmail API sync:**
- Gmail Messages API (`messages.list`, `messages.get`) — fetching email bodies
- `historyId`-based incremental sync (replacing IMAP UID tracking)
- A `gmail-api` provider type alongside the existing `gmail` (IMAP) provider

This is a meaningful feature to build in corky. Two options:

**Option A — XOAUTH2 for IMAP** (smaller change): Use the existing OAuth2 tokens with IMAP's `AUTHENTICATE XOAUTH2` mechanism. The `imap` crate supports this. This keeps the IMAP sync code intact, just swaps the auth method. No app password needed.

**Option B — Full Gmail API sync** (larger): Add `messages.list`/`messages.get` endpoints, build a new sync provider. More work but eliminates IMAP dependency entirely for Gmail accounts.

**Recommended: Option A** — it's the smallest path to getting btak.dev syncing without 2FA/app passwords. The OAuth2 tokens from filter auth can be reused with expanded scopes (`https://mail.google.com/`).

Which option do you prefer? Or would you rather just enable 2FA and use the app password approach?

Which one is better long term. Option A or Option B?
**Option B (Gmail API sync) is better long-term.** Here's why:

| | Option A (XOAUTH2 + IMAP) | Option B (Gmail API) |
|---|---|---|
| **Auth** | OAuth2 token → IMAP XOAUTH2 | OAuth2 token → REST API |
| **Threading** | Manual (subject-slug heuristic) | Native `threadId` from Gmail |
| **Incremental sync** | IMAP UIDs (fragile across expunge) | `historyId` (Google-guaranteed) |
| **Labels** | IMAP folder mapping (limited) | Full label API (multi-label per message) |
| **Rate limits** | Gmail throttles IMAP aggressively | API quota is generous (250 units/sec) |
| **Attachments** | IMAP FETCH BODY[] (full download) | Separate attachment endpoint (lazy fetch) |
| **Push notifications** | IMAP IDLE (connection-heavy) | `watch()` via Pub/Sub (webhook) |
| **Multi-account** | Separate IMAP connections | Same HTTP client, different tokens |
| **Effort** | ~1 day | ~3-5 days |

**Key advantages of Option B:**
- Gmail is deprecating IMAP in favor of their API — XOAUTH2 is a band-aid on a shrinking platform
- Native threading (`threadId`) eliminates the fragile subject-based dedup corky currently does
- `historyId` incremental sync is more reliable than IMAP UID tracking
- The OAuth2 infra, HTTP client (`ureq`), and token management already exist in corky
- Labels map 1:1 to corky's label routing — no folder→label translation needed

**Option A still has a role** as a quick unblock: implement XOAUTH2 now to get btak.dev syncing today, then build Gmail API sync as the strategic replacement. They're not mutually exclusive.

**Recommendation:** Start with Option A to unblock btak.dev immediately (~1 day), then build Option B as the proper Gmail provider over the next sprint. Option A becomes the fallback for edge cases.

Do Option B

Why am I seeing API Error?
```
✻ Churned for 45s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9EFFLSce5VaUHumS5bT"}

✻ Worked for 1m 3s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9ES9UHCz94sPU3sxjuA"}

✻ Churned for 34s

❯ /agent-doc tasks/email.md
  ⎿  API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server
     error"},"request_id":"req_011CZ9EX92dLTE1f49c1ajvC"}

✻ Cooked for 36s
```
### Re: API Errors

The 500 errors are from Anthropic's API (the Claude backend), not from your code. These are transient server-side issues — nothing you did wrong. The repeated `/agent-doc` invocations queued up while the API was having issues, then all came through once it recovered. No action needed on your end.

### Re: Option B — Gmail API Sync

Got it. I'll build the Gmail API sync provider in corky. This is a multi-file feature — I'll create a task document for it so we can track progress properly.

**Scope for `corky sync` Gmail API provider:**
1. **Auth scope expansion** — Add `https://mail.google.com/` scope to OAuth flow (currently only has `gmail.settings.basic` + `gmail.labels`)
2. **Gmail messages client** — `messages.list` (with label filter + date range) and `messages.get` (full RFC822 or metadata+body)
3. **Provider type** — New `gmail-api` provider in accounts config, reusing existing `[gmail]` OAuth credentials
4. **Sync logic** — `historyId`-based incremental sync with `.sync-state.json` integration
5. **Message conversion** — Gmail API response → same maildir-style files corky currently produces from IMAP
6. **Label routing** — Map Gmail label IDs to corky's label-based routing (already partially exists for filters)

**Not in initial scope:** Pub/Sub push notifications, draft management via API, attachment lazy-fetch (keep it simple for v1).

Want me to start building this now, or should I write a detailed plan first in a separate task doc?

Detailed plan
<!-- /agent:output -->
