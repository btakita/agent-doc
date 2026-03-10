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

## Calendar Management (corky v0.13.1–v0.13.3)

Implemented `corky cal` module with 5 subcommands:

```bash
corky cal auth                          # OAuth2 (reuses Gmail credentials, calendar:* token keys)
corky cal list [--limit N] [--query Q]  # list upcoming events
corky cal delete "query" [--all] [--dry-run]  # delete events (--all for recurring series)
corky cal create SUMMARY --start DT --end DT  # create events (v0.13.3)
corky cal check --start DT --end DT     # check availability (v0.13.3)
```

**Bugs fixed:** 403 (Calendar API not enabled), 400 (`+` not URL-encoded in timeMin), recurring series dedup

## Session History

- Committed and pushed all submodules: mail (`cbbfaee`), censible-core (`28bed29`), agent-doc (`771274d`), agent-kit (`a909d54`), agent-loop parent (`196a82c`)

<!-- agent:exchange -->
Converted to template mode. Updated calendar section to include v0.13.3 commands (`cal create`, `cal check`). Session history compacted.

How can I create additional gmail accounts after running into a limit with the number of email accounts allowed by google using a single phone number?

### Gmail account limits — workarounds

Google limits one phone number to **~4 Gmail accounts**.

**No second phone needed:**

1. **Google Workspace** ($7/mo per user) — unlimited accounts under your own domain. No phone verification for additional users. Cleanest long-term solution.
2. **Gmail aliases (free)** — `brian.takita+project1@gmail.com` (plus addressing) or dot variations. All deliver to the same inbox. Use filters to auto-label by alias.
3. **Skip phone verification** — sometimes works in incognito. Inconsistent.

Can you see what email addresses I have associated with my phone number?

**With a second number:**

4. **Google Voice** — free US number, can verify additional Gmail accounts.
How can I setup a google voice?
5. **Prepaid SIM** — cheap throwaway for one-time verification.
6. **VoIP** — TextNow/TextFree (Google blocks some VoIP ranges).

**Recommendation:** For truly separate inboxes → Google Workspace on your domain. For different addresses routing to one inbox → Gmail `+` aliases with corky filter rules.
<!-- /agent:exchange -->
