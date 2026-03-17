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
btak.dev IMAP auth failed. Needs Gmail app password and/or IMAP enabled.
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

<!-- /agent:output -->
