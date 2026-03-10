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

### Checking accounts associated with your phone

I can't look that up — Google doesn't expose a "list all accounts by phone number" API. But you can check yourself:

- [myaccount.google.com](https://myaccount.google.com) → **Personal info** → **Phone** — shows which account the phone is linked to
- [accounts.google.com](https://accounts.google.com) → **"Forgot email?"** → enter your phone number — Google shows associated accounts (masked)
- Or try creating a new account — Google will tell you if you've hit the limit

### Setting up Google Voice

1. Go to [voice.google.com](https://voice.google.com)
2. Sign in with any Google account
3. **"Get Google Voice"** → choose **Personal**
4. Search for a number by area code → select one
5. **Verify** with your existing phone number (one-time link)
6. Done — free permanent US phone number for calls/texts via web or app

Once you have the Voice number, use it to verify additional Gmail accounts. The Voice number is free and permanent. Requires an existing US phone to set up initially.

I have a shared gmail account with contact lucas-huang.
btak.dev@gmail.com

I want to route text messages going to this google voice number to a common place where lucas & I can see it. Perhaps btak.dev@gmail.com?
(508) 731-4882

### Routing Google Voice SMS to shared Gmail

Google Voice has built-in SMS→email forwarding. Options for routing (508) 731-4882 texts to `btak.dev@gmail.com`:

**Option 1: Direct forwarding (if Voice is on btak.dev)**
- [voice.google.com/settings](https://voice.google.com/settings) → Messages → **"Forward messages to email"**
- Texts arrive as emails in `btak.dev@gmail.com` — both you and Lucas see them
The phone number is linked to brian.takita@gmail.com
- See screenshot

**Option 2: Cross-account forwarding (if Voice is on a different account)**
- Enable SMS→email on the owning account
- Gmail Settings → Forwarding → add `btak.dev@gmail.com`
- Filter: `from:voice-noreply@google.com` → Forward to `btak.dev@gmail.com`

**Option 3: Share the Voice account directly (recommended)**
- If Voice is on `btak.dev@gmail.com`, both of you sign into the Google Voice app/web with that account
- Both see texts in real-time, both can reply
- No forwarding needed

**Recommendation:** If (508) 731-4882 is already on `btak.dev@gmail.com`, Option 3 is simplest. If it's on a different account, use Option 2 to pipe it to the shared inbox.

Can I switch the email address or do I need to make a new Google Voice number?
<!-- /agent:exchange -->
