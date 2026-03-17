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
Added btak-dev account to `.corky.toml`. Pending: verify `pass email/btak-dev` exists, then `corky sync`.
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
<!-- /agent:output -->
