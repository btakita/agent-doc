---
session: 6c57ef6d-db13-43df-a68d-0e9c4372110b
resume: null
agent: null
model: null
branch: null
---

## User

Set my gmail filters from the latest edits

## Assistant

No recent edits to `mail/filters.toml` — the file is unchanged and working tree is clean.

Current filters (2):
1. **for-lucas** — forwards job platform emails (Upwork, LinkedIn, Wellfound, etc.) to `dev@briantakita.me`, stars, marks important
2. **correspondence** — marks personal correspondence as important (from/to match with family/business contacts)

To push current filters to Gmail:
```
corky filter push --dry-run    # preview first
corky filter push              # apply to Gmail
```

Or edit `mail/filters.toml` with your changes and re-submit — I'll push them for you.

## User

We don't have filters.toml anymore correct? It should be in .corky.toml

## Assistant

**Correct — `corky filter push` reads from `.corky.toml`, not `mail/filters.toml`.**

The active config is `/home/brian/work/btakita/mail/.corky.toml` with **uncommitted edits** — the entire `[gmail]` section (filters, OAuth creds) plus contacts, topics, and LinkedIn config.

Differences from the old `mail/filters.toml`:
- **correspondence**: +4 addresses (`*@onesynergee.org`, `berkes06@yahoo.com`, `rberkes@gmail.com`, `feedback@slack.com`)
- **for-lucas**: +1 address (`*@mercor.com`)
- Addresses sorted alphabetically

Ready to push:
```
corky filter push --dry-run    # preview
corky filter push              # apply
```

## User

push them
