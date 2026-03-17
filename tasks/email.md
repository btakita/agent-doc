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
btak.dev: deciding between XOAUTH2 (IMAP + OAuth) vs full Gmail API sync vs 2FA + app password.
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
<!-- /agent:output -->
