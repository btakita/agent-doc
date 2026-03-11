---
agent_doc_session: 9f4d7cf6-4c6c-4c61-b52f-12e9c5908e20
tmux_session: '0'
agent_doc_mode: template
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

<!-- agent:exchange -->
Please draft a quick text message to send to Judith. Note my schedule today.
If we don't have a call before 3 ET, then I can have a call after 5 ET...or tomorrow.
I have a call from 1-1:20 ET tomorrow.

Corky v0.13.3 just released. It has Google Calendar support.
`corky cal create`
`corky cal check`

**Calendar check (via `corky cal check`):**
- 3:30-4:00 ET — Interview (Cast & Crew)
- 4:30-4:50 ET — Brian & Kevin
- Free: now until 3:30 ET, 4:00-4:30 ET gap, after 5:00 ET

**Draft text to Judith:**

> Hi Judith, it's Brian. Robert mentioned you might be calling today — just wanted to touch base. I'm free until 12:30 Pacific or after 2:00 Pacific today. Tomorrow works too, I'm open most of the day. Let me know what works best!

Warm, low-pressure, clear time slots. Doesn't mention the proposal — just availability.

Got this text message from Judith...help me with a reply:

> It was a big decision but I sent a cancellation email to mortgage Automator. I'll join the mortgage office and get things going with u soon. Thx for your intelligence time and patience. I try to reach out tmrw sometime! 😊

**Draft text reply to Judith:**

> That's wonderful news, Judith — thank you for your trust! I'm excited to get started. Whenever you're ready tomorrow, just give me a call and we'll map out next steps. Talk soon!

**Why this works:**
- Acknowledges the weight of her decision ("wonderful news")
- Reinforces trust without being sycophantic
- "Excited to get started" — forward momentum
- Puts the ball in her court for timing ("whenever you're ready")
- Brief, warm, professional — matches text message tone

I sent the previous response. We need a way to integrate corky to my text messages.

Suggest a reply:
> Mortgage Automator is tripping on me with emails today. Omg


**Draft reply to Judith's MA frustration:**

> Ha, they're not making it easy to leave! That's a good sign you made the right call. Don't stress about it — once you're set up with TMO, you won't look back. Talk soon!

Note my edit. Today is Tuesday..."tomorrow" relative to Judith's opening text.

I will be proposing that she have a custom system to replace TMO. But that can happen later.

**Why this works:**
- Light humor validates her frustration without dwelling
- Reframes annoyance as confirmation she made the right decision
- "Don't stress" — reassuring, protective
- Short — she's venting, not asking for a solution

**Re: Corky text integration** — tracked for `tasks/corky.md`. Natural extension: `corky text send` / `corky text draft`. Main question: SMS gateway (Twilio, Google Messages API, or iMessage bridge).

Good catch on the "Talk soon!" edit — Judith's "tmrw" was relative to Sunday 3/9, so "tomorrow" would be stale by Tuesday. "Talk soon!" is timeless and works regardless of when she reads it.

Noted — TMO is the stepping stone. Phase 2 (custom replacement) is already in the proposal's cost comparison table, so the seed is planted without needing to pitch it now. Let her get comfortable with TMO + your custom layer first, then the "why pay $600/mo for TMO when we can do it all?" conversation writes itself.

Let's figure out the plan to get started with the work.

How do we onboard Equity Funding Source (Judith's company)?
What do we need from her side to get started with Phase 1?
And Phase 2?

/agent-doc run

**Onboarding Plan — Equity Funding Source**

### What We Need From Judith (Before Phase 1 Starts)

**Accounts & Access:**
1. **TMO account** — She needs to sign up with The Mortgage Office. Once active, request API/developer access via https://developers.themortgageoffice.com/
2. **Domain access** — DNS credentials for `equityfundingsource.com` (currently on Weebly). We'll point it to Cloudflare
3. **Google Workspace** — Either her existing Gmail/Google setup or we create a new Google Workspace ($35/mo for 5 seats). Needed for Calendar (notifications), Drive (document storage), and business email
4. **Current website content** — Any pages, images, branding assets, or copy she wants preserved from the existing site
5. **Cloudflare account** — We'll create one in her name (free tier)
Does Judith need to sign up far a Cloudflare account? And add me as an admin?
Can I transfer ownership to her? I expect we will need to use workers.
I could host her site on my account, but one of my selling points is that she owns the accounts.

**Business Requirements (can gather on Zoom with Robert Waldman):**
1. **Document types** — List of all document types she regularly collects (loan apps, bank statements, tax returns, construction budgets, etc.)
2. **Workflow walkthrough** — How a loan flows from inquiry → closing in her current system
3. **Staff roles** — Who does what? How many people need access?
4. **Investor communication** — How she currently pitches deals to investors, what info they need
5. **Notification preferences** — What reminders does she want automated? Current pain points with follow-ups?

### Phase 1: Website Migration (~15-20 hrs, Week 1-2)

**From Judith:** Domain credentials, branding assets, existing site content, desired pages
**Deliverable:** New site live on Cloudflare — public-facing pages for borrowers, brokers, investors
**Payment trigger:** M1 ($4,000) due at start

### Phase 2: Document Upload Portal (~12-16 hrs, Week 2-3)

**From Judith:** Document type list, sample documents (redacted), staff email addresses for admin access
**Deliverable:** Secure upload links, document tracking dashboard, Google Drive/Dropbox integration

### Phase 3: OCR + TMO Integration (~16-26 hrs, Week 3-4)

**From Judith:** TMO API credentials (sandbox first), sample filled documents for OCR testing
**Deliverable:** Upload → AI extract → staff review → push to TMO pipeline
**Payment trigger:** M2 ($4,000) due at start of Phase 3

### Phase 4: Notification System (~10-14 hrs, Week 4-5)

**From Judith:** Notification rules (what triggers, frequency, recipients)
**Deliverable:** Automated reminders via Google Calendar, drip sequences

### Immediate Next Steps

1. **Schedule Zoom** with Judith + Robert Waldman — requirements gathering + TMO signup walkthrough
2. **Judith signs up for TMO** — she can start using it immediately for loan management while we build the custom layer
3. **Judith provides domain credentials** — so we can begin Cloudflare migration
4. **Agree on payment structure** — confirm the two $4K milestones work, or break into 3 smaller ones per the talking points ($2-3K each)

Let's begin with an onboarding email. Just to lay it out what we know needs to be done in writing. We can have a call for the requirements gathering.

**Re: Cloudflare Account Ownership**

Best approach: **Create the account in Judith's name from the start.** She signs up with her business email, then adds you as a member (Cloudflare supports multi-user access on all plans, including free). This way she owns the account from day one — no transfer needed.

If you start on your account, you *can* transfer individual zones (domains) between Cloudflare accounts later, but it's cleaner to just set it up right from the start. Full account transfers aren't supported — only zone-level moves.

**Workers:** Free plan includes 100K requests/day (more than enough to start). If she needs more, Workers Paid is $5/mo for 10M requests/month. Either way, negligible cost.

**Practical setup:** On the Zoom call, walk her through creating the Cloudflare account (takes 2 minutes), then she adds you as an admin right there. Same session, you point her domain's nameservers to Cloudflare.

---

**Onboarding Email Draft**

CC's Robert Waldman so he's in the loop. Keeps it warm, actionable, and doesn't overwhelm with technical details — saves that for the call.

**To:** judithkrug@gmail.com
**CC:** rwaldman100@gmail.com
**Subject:** Getting Started — Next Steps for Your New System
**In-Reply-To:** (thread with proposal email)

---

Hi Judith,

Great to hear you're moving forward — I'm excited to get started!

Here's a quick rundown of what we'll need to kick things off. Nothing urgent, just so you know what's coming:

**Before we begin:**
1. **Sign up for The Mortgage Office (TMO)** — You can start using it right away for loan management while I build the custom tools on top. Their team can walk you through setup.
2. **Domain access** — I'll need the login for wherever equityfundingsource.com is managed (likely Weebly or your domain registrar). This lets me set up your new site on Cloudflare.
If we cannot get the Weebly login, I can crawl the existing site and appropriate the design + content. We can also redesign the website & change the content.
3. **Branding assets** — Your logo, any photos or content from the current site you want to keep. If you have a style preference (colors, fonts), let me know — otherwise I'll work from what's on the current site.

**On our first call together (you, me, and Robert):**
- Walk through your current workflow so I understand exactly how loans flow through your office
- List out the document types you collect from borrowers and investors
- Set up your Cloudflare account (takes 2 minutes) and point your domain to it
The existing website will remain pointed to the existing host until the new site is ready.
We will need the Cloudflare account to develop the new site.
- Discuss notification preferences — what reminders would save you the most time?

**How payment works:**
- Milestone 1 ($4,000): Website + document upload portal — you'll see it live before we move to the next phase
- Milestone 2 ($4,000): AI-powered document extraction + TMO integration + notification system
- You pay as you see results, and you own everything we build

When would be a good time for that first call? I'm flexible — happy to work around your schedule. A 45-60 minute Zoom should cover everything.

Looking forward to it!

Brian

---

Ready to save as a corky draft when you approve. Let me know if you want to adjust tone, add/remove anything, or change the payment framing.

**Updated Onboarding Email Draft (v2)**

Incorporated both inline notes:
- Domain access is now "if you have it, great — if not, I'll work around it"
- Cloudflare bullet now clarifies the existing site stays live during development
- Removed the parenthetical about "likely Weebly" — she knows where her site is hosted

**To:** judithkrug@gmail.com
**CC:** rwaldman100@gmail.com
**Subject:** Getting Started — Next Steps for Your New System
**In-Reply-To:** (thread with proposal email)

---

Hi Judith,

Great to hear you're moving forward — I'm excited to get started!

Here's a quick rundown of what we'll need to kick things off. Nothing urgent, just so you know what's coming:

**Before we begin:**
1. **Sign up for The Mortgage Office (TMO)** — You can start using it right away for loan management while I build the custom tools on top. Their team can walk you through setup.
2. **Domain access** — If you have the login for wherever equityfundingsource.com is managed, that's helpful. If not, no worries — I can work from the existing site's design and content, or we can do a fresh redesign if you'd prefer a new look.
3. **Branding assets** — Your logo, any photos or content you want to keep. If you have a style preference (colors, fonts), let me know — otherwise I'll work from what's on the current site.

**On our first call together (you, me, and Robert):**
- Walk through your current workflow so I understand exactly how loans flow through your office
- List out the document types you collect from borrowers and investors
- Set up your Cloudflare account (takes 2 minutes) — this is where the new site gets built. Your current site stays live until the new one is ready, so nothing changes for visitors until you're happy with the result.
- Discuss notification preferences — what reminders would save you the most time?

**How payment works:**
- Milestone 1 ($4,000): Website + document upload portal — you'll see it live before we move to the next phase
- Milestone 2 ($4,000): AI-powered document extraction + TMO integration + notification system
- You pay as you see results, and you own everything we build

When would be a good time for that first call? I'm flexible — happy to work around your schedule. A 45-60 minute Zoom should cover everything.

Looking forward to it!

Brian

---

Ready to save as corky draft when you approve.

Yes save as draft

Saved as corky draft: `mail/drafts/20260311-judith-onboarding.md`
- **To:** judithkrug@gmail.com
- **CC:** rwaldman100@gmail.com
- **Subject:** Re: Proposal — TMO + Custom Automation for Equity Funding Source
- **Status:** draft

Send with `corky send mail/drafts/20260311-judith-onboarding.md` when ready.
<!-- /agent:exchange -->
