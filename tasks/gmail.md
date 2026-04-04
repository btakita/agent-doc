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

### Transfer Google Voice number to another account

Yes — you can transfer the number. No need for a new one.

**Steps:**
1. Sign into [voice.google.com](https://voice.google.com) with `brian.takita@gmail.com`
2. Settings → **Transfer number**
3. Enter destination: `btak.dev@gmail.com`
4. Confirm via email sent to `btak.dev@gmail.com`
5. Sign into voice.google.com with `btak.dev@gmail.com` to complete

After transfer, (508) 731-4882 lives on `btak.dev@gmail.com`. Both you and Lucas use it directly — no forwarding needed.

Will (508) 731-4882 still forward to my phone number 424.249.2350?
Can I make calls & texts from my phone?
Can Lucas make texts from the imac?

**Caveat:** Transfer removes the number from `brian.takita@gmail.com` entirely. If you want to keep Voice on brian.takita for other purposes, use the forwarding approach (Option 2) instead.

### Answers to forwarding & device questions

**1. Will (508) 731-4882 still forward to 424.249.2350?**
Yes, but re-link after transfer. The linked phone stays with the old account. After transfer to `btak.dev@gmail.com`:
- Open Google Voice app → sign in as `btak.dev@gmail.com`
- Settings → Linked numbers → Add 424.249.2350
- Calls/texts to (508) 731-4882 ring your phone again

**2. Can you call & text from your phone?**
Yes. Google Voice app on your phone, signed into `btak.dev@gmail.com`. Make/receive calls and texts as (508) 731-4882. Your personal number (424.249.2350) stays separate.

**3. Can Lucas text from the iMac?**
Yes. [voice.google.com](https://voice.google.com) in any browser, signed into `btak.dev@gmail.com`. Full texting — no app install needed. Optional: Google Voice Chrome extension for desktop notifications.

**Summary:** After transfer, both of you have full access — you via phone app, Lucas via web on the iMac. Just re-link 424.249.2350 after the transfer completes.

Does re-linking 424.249.2350 mean it will no longer be associated with brian.takita@gmail.com? I want it to be associated with brian.takita@gmail.com.

### Re-linking does NOT affect brian.takita@gmail.com

No — these are two separate things:

- **Google Voice "linked number"** = a forwarding destination (tells Voice "ring this phone"). One phone can be linked to multiple Voice accounts.
- **Google account phone** = recovery/verification number at myaccount.google.com. Completely independent.

424.249.2350 stays as `brian.takita@gmail.com`'s account phone regardless of what you do with Voice.

**Simpler alternative — skip the transfer entirely:**

Given you want to keep everything on `brian.takita@gmail.com`, use **Option 2** instead:

1. Keep Voice on `brian.takita@gmail.com` (no transfer)
2. Turn ON "Forward messages to email" (the toggle in your screenshot)
3. Gmail filter on `brian.takita@gmail.com`: From `voice-noreply@google.com` → Forward to `btak.dev@gmail.com`

Is voice-noreply@google.com guaranteed to be the google voice phone number only?
What if I have another Google voice number that I don't want to route?
Can the filter distinguish between the phone numbers?

Result: Voice texts arrive at both your Gmail and the shared `btak.dev@gmail.com`. Nothing changes about your account setup.

### Filter specificity for Voice emails

**`voice-noreply@google.com`** sends ALL Voice notifications (calls, texts, voicemails) for the account. But:

**Personal Voice = one number per account.** You can only have one Voice number on `brian.takita@gmail.com`. So `from:voice-noreply@google.com` already targets only (508) 731-4882. No ambiguity.

**If you ever had multiple numbers** (requires Workspace Voice plan), you could filter by subject/body — the emails include the phone number:
- `from:voice-noreply@google.com subject:508` — matches only that number

**Bottom line:** For your setup, `from:voice-noreply@google.com` is sufficient and specific. One account = one Voice number = one filter catches exactly what you need.

Please update .corky.toml with the filter and forwarding rule.

### .corky.toml updated — one manual step needed

Added to `.corky.toml`:

```toml
[[gmail.filters]]
label = "voice-sms"
match = ["from"]
forward_to = "btak.dev@gmail.com"
addresses = [
    "voice-noreply@google.com",
]
```

Also added `voice-sms` to the personal account's sync labels.

**Issue:** `corky filter push` failed because the `voice-sms` label doesn't exist in Gmail yet. Corky can't create Gmail labels via the API (only reads them).

**Manual step:** Create the label in Gmail:
1. Open [Gmail](https://mail.google.com) → Left sidebar → scroll down → **"+ Create new label"**
2. Name: `voice-sms`
3. Then run: `corky filter push`

Can I create the filter via the api?

Alternatively, if you'd rather skip the label and just use the forwarding, you can create the filter manually in Gmail (Settings → Filters → Create: From `voice-noreply@google.com` → Forward to `btak.dev@gmail.com`). But having it in `.corky.toml` keeps it tracked and reproducible.

mail/.corky.toml

**Also needed:** Add `btak.dev@gmail.com` as a forwarding address in Gmail settings first (Settings → Forwarding → Add). Google requires confirmation from the receiving account.

### Auto-label creation implemented

Yes — implemented auto-label creation in corky. Running `corky filter push` now auto-creates missing Gmail labels via the Labels API. The `voice-sms` label was created successfully.

**However**, the filter push still failed with **"Unrecognized forwarding address"** — Gmail requires `btak.dev@gmail.com` to be verified as a forwarding address before the Filters API can reference it.

**Manual step (one-time):**
1. Gmail Settings (brian.takita@gmail.com) → **Forwarding and POP/IMAP** → **Add a forwarding address** → enter `btak.dev@gmail.com`
2. Google sends a confirmation email to `btak.dev@gmail.com` — click the link
3. Then re-run: `corky filter push`

Done

After that, the filter will be created via the API with both the label and forwarding action.

### Filter push — two forwarding addresses need verification

Re-ran `corky filter push` — still failing. The error is on filter [2] (`for-lucas`, forwards to `dev@briantakita.me`). The voice-sms filter (forwards to `btak.dev@gmail.com`) also needs its forwarding address confirmed.

**Both forwarding addresses must be verified** in Gmail Settings for `brian.takita@gmail.com`:

| Address | Status |
|---------|--------|
| `btak.dev@gmail.com` | Added? Check if confirmation email was clicked |
Now Confirmed
| `dev@briantakita.me` | Needs to be added as forwarding address |

**Steps:**
1. Gmail Settings → Forwarding → confirm both addresses show as "verified"
2. If `dev@briantakita.me` isn't listed, add it → confirm via email
It has already been forwarding. Did it go away?
3. Re-run: `corky filter push`

Note: `corky filter push` deletes all existing filters and recreates them, so if ANY filter's forwarding address is unverified, the entire push fails mid-way (filters 1 gets created, but 2 and 3 fail).

### All filters pushed successfully

`corky filter push` — all 3 filters created:
1. **correspondence** — label only
2. **for-lucas** — label + forward to `dev@briantakita.me`
3. **voice-sms** — label + forward to `btak.dev@gmail.com`

The previous failure was because `btak.dev@gmail.com` wasn't confirmed yet — the push fails on the first bad forwarding address and stops. `dev@briantakita.me` was still verified; it just never got reached.

**Remaining manual step:** Turn ON "Forward messages to email" in Google Voice settings ([voice.google.com/settings](https://voice.google.com/settings)) so texts actually generate the emails that the filter will catch and forward.

I sent 2 test sms messages. But I don't see either going to brian.takita@gmail.com

### SMS not arriving — check Voice forwarding toggle

The Gmail filter only catches emails — it doesn't generate them. Two things to check:

1. **"Forward messages to email" toggle** — your earlier screenshot showed this was OFF. Go to [voice.google.com/settings](https://voice.google.com/settings) → Messages → turn **ON** "Forward messages to email". This makes Voice email `brian.takita@gmail.com` for each incoming text.

2. **Check spam** — Voice notification emails from `voice-noreply@google.com` sometimes land in spam initially.

**The flow:**
```
SMS to (508) 731-4882
  → Google Voice receives it
  → (toggle ON) emails brian.takita@gmail.com
  → Gmail filter matches from:voice-noreply@google.com
  → Labels "voice-sms" + forwards to btak.dev@gmail.com
```

Without the toggle, no email is generated and the filter has nothing to act on.

The toggle is on. Please check the brian.takita@gmail.com for the sms emails.
![img_1.png](img_1.png)

### Toggle confirmed ON — no emails yet

Screenshot confirms "Forward messages to email" is ON. Ran `corky sync` — no messages in the `voice-sms` label yet.

**Likely causes:**
1. **Pre-toggle messages don't get emailed** — the 2 test SMS sent before enabling the toggle won't be retroactively forwarded. Only new messages after the toggle generate emails.
2. **Delay** — Google Voice can take a few minutes to start forwarding after the toggle is enabled.
3. **Spam** — first-time Voice emails sometimes land in spam.

**Next steps:**
- Send a **new** test SMS to (508) 731-4882 now (after toggle is confirmed on)
- Wait 2-3 minutes
- Check Gmail: search `from:voice-noreply@google.com` in All Mail
- Check spam folder
- Then run `corky sync` again

I don't see the sms messages going to the Google Voice messages either

![img_2.png](img_2.png)

### SMS not reaching Google Voice at all

Your Google Voice messages tab shows the most recent texts are from **2015**. The test SMS messages aren't arriving at Google Voice — this is upstream of the email forwarding issue.

**Most likely cause: Google reclaimed the number.** Google Voice reclaims numbers after ~6 months of inactivity. Your last Voice activity was 2015 — over 10 years ago. The number (508) 731-4882 may no longer be yours.

**To verify:**
1. In Google Voice, click the **gear icon** (Settings) → check what number is listed under "Account"
2. Or try sending a text FROM Google Voice (click "Send new message") to your phone — see if it goes through
3. Check: [voice.google.com/settings](https://voice.google.com/settings) → look for your number in the "Phone number" section

**If the number was reclaimed:**
- You'll need to claim a new Google Voice number (free)
- The old (508) 731-4882 number is gone — someone else may have it now
- Your test texts went to whoever currently has that number (or nowhere)

**If the number is still yours:**
- Try texting (508) 731-4882 from a completely different phone (not 424.249.2350) to rule out linked-number routing issues
<!-- agent:boundary:ab6ddfee -->
<!-- /agent:exchange -->
