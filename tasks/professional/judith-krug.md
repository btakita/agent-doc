---
agent_doc_session: 9f4d7cf6-4c6c-4c61-b52f-12e9c5908e20
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
links:
  - ../personal/robert-ward.md
  - ./bob-remeika.md
---

# Judith Krug — Equity Funding Source

**Contact:** `mail/contacts/judith-krug/CLAUDE.md`
**LinkedIn:** https://www.linkedin.com/in/judith-krug-a4b71b15/
**Company:** http://www.equityfundingsource.com/
**Email:** judithkrug@gmail.com
**Referral:** Robert Ward (`mail/contacts/robert-ward/`)

## Background

**Judith Krug** — CEO & Broker, Equity Funding Source, Inc. Private mortgage brokerage in Los Angeles specializing in non-conforming, equity-based loans funded by private investors. Not a bank — she brokers between borrowers and private lenders.

- Hard money, bridge, swing, and home equity loans
- California-focused (Southern + Northern)
- LTV caps: 1st ≤65%, 2nd ≤60% combined, 3rd ≤55% combined
- Residential minimum $75K-$100K, commercial $100K+
- No residential rehab; commercial rehab OK
- Fractionalized note investments (up to 10 investors per loan)
- RE 885 disclosure (interest-only on 1-4 unit residential)

## Current System

- **Bespoke/custom system** (not TMO, not MA)
- Her family was involved in creating TMO, but her company doesn't use it
- Evaluating both TMO and Mortgage Automator
- Current workflow: manual entry ("fill in the blanks"), office server, Dropbox
- Does NOT want loan servicing (auditing burden)

## Meeting Notes (2026-02-26)

Interested in investor outreach with private individuals to expand business (doctors, lawyers, etc.).

**Mortgage Automator pricing:**
- $18,000 Y1 (includes setup + coding)
- $13,000/year ongoing ($1,137/month)
- Unlimited seats, notifications, portals, Hubspot, servicing, RE 855 disclosure

**The Mortgage Office (TMO) pricing:**
- ~$600/month (2 seats, $400/month per extra)
- Doc generation, manual entry, borrower doc upload
- Industry incumbent in LA, half the price of MA

**Also mentioned:** Kalix Software (likely Calyx Software) integration

## Strategy: Platform-Agnostic Intelligence Layer

Instead of duplicating TMO's or MA's core operations, build a **complementary intelligence layer** that works with any platform:

### Value Propositions

1. **Deal sourcing** — Agent loops scanning county records, NODs, probate, expired listings
2. **Investor pitches & pooling** — Custom pitch decks per investor risk profile, pooling orchestration
3. **Website integration** — Tiered deal marketplace (public anonymized → trusted investor access)
4. **Scheduled reminders** — Deadlines, follow-ups, milestones
5. **Eliminating manual entry** — AI extraction from docs → auto-populate via API
6. **Website modernization** — Redesign, full account custody, analytics-driven funnels

## Email Timeline

### Pitch Email (2026-02-26)
- **Draft:** `mail/contacts/judith-krug/20260226-tmo-pitch.md` → status: `sent`
- **Issues:** No subject line (corky bug, fixed). Factual errors (implied she uses TMO).

### Correction Email (2026-02-27)
- **Draft:** `mail/contacts/judith-krug/20260227-correction.md` → status: `sent`
- **Threading failed** — Gmail broke thread due to subject change from empty to "Quick correction"

### Follow-up Email (2026-03-06)
- **Draft:** `mail/drafts/20260306-judith-followup.md` → status: `sent`
- Light follow-up confirming receipt, offering call. CC'd Robert Waldman.

### Proposal Email (2026-03-05)
- **Draft:** `mail/drafts/20260305-judith-proposal.md` → status: `sent`
- **To:** judithkrug@gmail.com, **CC:** rwaldman100@gmail.com
- **Subject:** Proposal — TMO + Custom Automation for Equity Funding Source
- **Attachment:** `20260305-tmo-proposal.pdf`

### Onboarding Email (2026-03-11)
- **Draft:** `mail/drafts/20260311-judith-onboarding.md` → status: `sent`
- Onboarding next steps: TMO signup, domain access, branding assets, call scheduling. CC'd Robert Waldman.

## Key People

- **Robert Ward** — referral who connected Brian to Judith
- **Robert Waldman** — Judith's tech/marketing consultant, family taught Judith the mortgage business, based in Texas, has enterprise GoHighLevel access

## Meeting Recording (2026-03-04)

- **Audio:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-.amr`
- **Diarized transcript:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-waldman-diarized.txt`
- **Notes:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-waldman-notes.md`
- **Shareable meeting notes:** `mail/contacts/judith-krug/recordings/20260304-meeting-notes-for-sharing.md`

## Proposal (2026-03-05)

Full proposal at: `mail/contacts/judith-krug/20260305-tmo-proposal.md` (PDF: `20260305-tmo-proposal.pdf`)

**Pricing:** $8,000 flat for 80+ hours, two $4,000 milestone payments. $100/hr for additional work.

**Milestones:**
- M1: Website + document upload portal
- M2: OCR/TMO integration + notification system

**Monthly costs:** ~$670–845/mo (TMO ~$600, Google Workspace $35, AI/hosting $10-200+)

**Technical stack:** Hono + Cloudflare Workers/Pages, relementjs + rmemo, Keystatic CMS, Google Workspace

**Development Phases (63-90 hours, ~5 weeks):**

| Phase | Deliverable | Hours |
|-------|------------|-------|
| 1 | Website migration to Cloudflare (Hono + Workers) | 15-20 |
| 2 | Document upload portal (secure links, Drive/Dropbox) | 12-16 |
| 3 | AI-powered OCR + TMO API integration | 16-26 |
| 4 | Notification system (Google Calendar-based) | 10-14 |

**TMO API:** Developer portal at https://developers.themortgageoffice.com/, sandbox available.

**Bonus scope:** Telephony, county record scraping, CRM integration, investor portal.

## Cost Comparison

| | Mortgage Automator | TMO + Brian (custom) |
|---|---|---|
| Year 1 | ~$18K | ~$15.2K ($8K dev + $7.2K TMO) |
| Year 2+ | ~$13K/yr | ~$7.2K/yr (TMO only) |
| Phase 2 (replace TMO) | N/A | +$8-12K dev, then $0/yr |

Year 1 savings vs MA: ~$2.8K. Real win is year 2+: ~$5.8K/yr savings. With TMO replacement (Phase 2), breaks even by end of year 2, saves ~$7-13K/yr after.

## Talking Points for Call

**Opening — "Build is the new buy"** (Robert Ward coaching)
- Custom software fits like a glove vs. SaaS one-size-fits-all
- No vendor lock-in, no annual contracts, no per-seat pricing
- She owns the code, perpetual license

**Staged payments** (Judith is emotional about big numbers):
- M1 ($2-3K): Website + document upload portal
- M2 ($2-3K): AI-powered OCR/document extraction
- M3 ($2-3K): TMO integration + notification system
- She pays as she sees results — low risk

**What custom does that MA can't:**
- OCR for ALL her document types (MA only handles their templates)
- County record scraping for investor/lender sourcing
- Custom investor pitch decks per risk profile
- GoHighLevel integration (Robert Waldman has enterprise access)

**Communication style** (Robert Ward's coaching):
- Be concise, professional — get to the point
- Only talk about HER business
- Don't re-pitch $8K as a lump — let the stages frame it
- She already called you a "genius" — trust is established
- Ask: "What would you like to start with?"

**If she pushes back on price:**
- "What would you be comfortable starting with?"
- Offer website-only Phase 1 for $2-3K as foot-in-the-door
- MA is $18K year one — even at $8K + TMO she's saving money

## Open Items

- [ ] Judith reviews proposal and decides MA vs TMO + custom
- [ ] Schedule Zoom call with Judith and Robert Waldman for deeper requirements
- [ ] Talk to MA about website integration specs (if she stays with MA)
- [ ] Telephony plan details (see `tasks/telephony.md`)

## Session Log (2026-03-09)

- Judith expected to call at 10 PT (per Robert Ward). DST just happened — may be confused about time.
- Brian has calls 3:30-5:00 ET — free until 12:30 PT for Judith.
- Robert Ward set up the call — Brian feels strange reaching out directly.
- Corky calendar integration moved to `tasks/corky.md`.

<!-- agent:exchange mode=append -->
### Exchange Archive (March 9 – March 17, 2026)

---

#### Re: Text message draft (2026-03-09)
Drafted text to Judith re: scheduling a call. Brian available until 12:30 PT, then after 5 ET. DST confusion noted.

---

#### Re: Mortgage Automator Contract (2026-03-15)
- **MA ToS found:** [mortgageautomator.com/terms-of-service](https://www.mortgageautomator.com/terms-of-service)
- 12-month minimum, non-refundable, auto-renews unless 45-day written notice
- Stopping use ≠ cancellation — full annual fee owed regardless
- No early termination, no partial refunds
- **Critical:** When did Judith sign? 45-day email cancellation before renewal is the escape hatch

#### Re: MA Cancellation Sentiment (2026-03-15)
- One Capterra user reported $10K threatening letter for multi-device login (later corrected: this was about TMO, not MA)
- Bryt Software markets against MA's lock-in ("cancel anytime")
- No BBB profile (Canadian company)
- Recommendation: get Judith's contract, check timeline, don't panic — cross-border SaaS enforcement is expensive
- Judith's attorney (Canada-based) is confident he can get her out

#### Re: TMO Contract Research (2026-03-15–16)
- TMO does NOT publish terms, pricing, or contracts publicly
- TMO claims "no long-term contract" but Capterra reviews describe $10K licensing fees for device violations
- Multiple reviewers: small company, remote access triggers "breach" claim, TMO demands $10K+ and threatens data lockout
- **Both platforms have aggressive enforcement** — long-term play is custom software
- Capterra blocks datacenter IPs — reviews confirmed via search engine snippets

---

#### Re: Adjust plan for platform-independent Phase 1 (2026-03-17)
- Judith verbally confirmed: "100% Brian's going to work on the website and other stuff"
- MA enforcing 12-month contract — Judith's attorney fighting it
- Robert Ward: just start working, build a demo, get paid
- **Revised Phase 1** (16-24 hours): website migration + borrower intake + investor section. Zero platform dependency.

#### Re: Website Kickoff Email — Sent (2026-03-17)
- Draft: `mail/drafts/20260317-judith-website-kickoff.md`
- To: judithkrug@gmail.com, CC: rwaldman100@gmail.com, BCC: robertward3@yahoo.com
- Subject: Re: Proposal — TMO + Custom Automation for Equity Funding Source
- Asks about document upload portal scope (defer vs build now)
- Asks about GoHighLevel CRM (Robert Waldman's enterprise access)
- **Sent** via `corky draft push --send`

#### Re: Corky HTML Email (2026-03-17)
- Email sent as plain text — `**bold**` shows as literal asterisks
- Feature request added to `tasks/corky.md`: markdown-to-HTML via `pulldown-cmark`, `multipart/alternative`

Did you get Judith's response?
> I am out of town on a life transformed coaches retreat so I can’t do anything this week. Please let’s reconvene about middle of next week to discuss everything more. Thank you for your time and efforts.

Robert Ward had some comments about this email.


#### Re: Judith's Response (2026-03-17)

Judith replied to the kickoff email:
> I am out of town on a life transformed coaches retreat so I can't do anything this week. Please let's reconvene about middle of next week to discuss everything more. Thank you for your time and efforts.

**Analysis:** Positive signal — she replied promptly, confirmed interest, and set a concrete timeline (mid-next week = ~Mar 24-26). The retreat aligns with her pattern of spiritual/coaching events that Robert Ward described.

#### Re: Robert Ward Recording Transcriptions

**Completed transcriptions:**
- `phone_20260317-144243` — ring only (23s, no answer)
- `phone_20260317-164000` — 3:45 pre-email call. Robert coaching Brian not to push Judith, will call her himself
- `phone_20260317-165018` — 18:56 post-email callback. KEY: Robert's specific email tone feedback (see below)
- `phone_20260314-035658` — 5:12 check-in. Robert confirming Judith is "done deal"
- `phone_20260314-221922` — 5:11 coaching call. "Remind her politely, don't bother her"
- `phone_20260315-114203` — 0:02 (empty)
- `phone_20260315-225612` — 6:30 MA contract research call
- `phone_20260316-014818` — 9:15 post-Judith call. "She likes you, thinks you're intelligent"

**Still transcribing (background):** Mar 7-9 (5 files), Mar 18 (3 files)

Note: Diarize feature not compiled in corky — transcripts lack speaker labels. Used `--speakers` flag for metadata only.

#### Re: Robert Ward's Email Tone Feedback — Summary

From phone_20260317-165018 (the callback after Robert read the kickoff email):

1. **Email was too long** — Robert confirmed: "Yeah, my email was a little long. I agree. It was too long." Keep future emails to 3-5 sentences max.
2. **60-minute call was a mistake** — From the earlier proposal email. Robert: "I think it's not good to have an emotional woman believe that she has to dial into a 60 minute phone call." Never specify duration — just say "a quick call."
3. **Not pushy, but serious** — Robert's exact words for the right tone. Show competence and eagerness without creating pressure.
4. **Don't jump the gun** — When Brian said he'd have the website ready by next week, Robert pushed back: "I think that is jumping into the gun." Don't promise deliverables before scope is confirmed.
5. **Don't come across as needy** — Despite financial pressure, keep communications professional.
6. **Let Robert be the intermediary** — Brian preferred Robert deliver the "I'm getting started" message rather than emailing Judith directly during her retreat.
7. **She needs reminding, not pushing** — She overbooks and lets things go. Gentle, polite reminders work. Pressure doesn't.
8. **Trust is already established** — She called Brian a "genius," told Robert she's "100% in." Don't re-sell.

**Applied to `mail/contacts/judith-krug/CLAUDE.md`** — updated the Tone section with Robert's coaching points for all future email drafts.

#### Re: Draft Response to Judith

Applying Robert's feedback — short, warm, no pressure, no time commitments:

---

**Draft:** `mail/drafts/20260319-judith-retreat-reply.md`

> Hi Judith,
>
> Enjoy the retreat — sounds like a great reset. I'll be here whenever you're ready next week. Looking forward to it.
>
> Brian

---

**Why this works:**
- 2 sentences (Robert said keep it short)
- No call duration mentioned
- No deliverable promises (no jumping the gun)
- Warm and supportive, not pushy
- Acknowledges her retreat positively (empowerment > instruction)
- Ball in her court for timing

Want me to create the draft file and send via corky?
Please use corky to transcribe and diarize all of my conversations with Robert Ward
/home/brian/work/btakita/agent-loop/mail/contacts/robert-ward/recordings

Robert had some comments about the tone of the emails. Please summarize the comments. And let's apply the suggestions to our communication AGENTS.md with Judith.

Please help me with a draft response.

Please have corky transcribe use --gpu on this machine by default. Set CLAUDE.local.md or a local skill.


#### Re: Reply Sent (2026-03-19)

**Sent** via `corky draft push --send`:
- **Draft:** `mail/drafts/20260319-judith-retreat-reply.md`
- **To:** judithkrug@gmail.com
- **Subject:** Re: Proposal — TMO + Custom Automation for Equity Funding Source
- **Body:** Short, warm acknowledgment of retreat + confirmation for next week. 2 sentences per Robert's coaching.

#### Re: GPU Transcription Config

- This machine has NVIDIA GPU (CUDA 13.2)
- Corky had no `--gpu` CLI flag — GPU is a compile-time feature: `transcribe-cuda`
- **Rebuilt corky** with `cargo install --path . --features transcribe-cuda` — future transcriptions will use GPU automatically (~10x faster)
- Created `src/corky/CLAUDE.local.md` with build instructions and transcription defaults
- No skill needed — it's a build-time setting, not a runtime flag
The reply looks good. Full send.

Please read the transcript for the last phone call with robert-ward. Come up with a plan for today.
I will start with Judith's website. Also need to plan Ragie integration with Judith's at equity-fundng-source.
### Re: Bob Remeika Link Added (HEAD)

Linked `./bob-remeika.md` — Bob Remeika (CEO, Ragie) is now a related contact for this document. This ties into the Ragie RAG integration planned for equityfundingsource.com. When ready to scope that work, the bob-remeika doc and `tasks/software/ragie-web-doc.md` have the Ragie context.
<!-- agent:boundary:473dc36a -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] Ragie integration scoping for equityfundingsource.com (linked via bob-remeika.md)
- [ ] Judith reviews proposal and decides MA vs TMO + custom
- [ ] Schedule Zoom call with Judith and Robert Waldman
- [ ] Website Phase 1 kickoff (pending Judith's return from retreat)
<!-- /agent:pending -->
