---
agent_doc_session: 9f4d7cf6-4c6c-4c61-b52f-12e9c5908e20
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
links:
  - ../personal/robert-ward.md
  - ./bob-remeika.md
  - ./robert-waldman.md
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
March 9 – April 2, 2026: Judith Krug / Equity Funding Source project kickup.

Key decisions:
- Built equityfundingsource.com on Cloudflare Pages (25 pages, 18 funded deals, forms)
- Three proposal options: (1) TMO+Custom+Ragie ~$700/mo, (2) MA+Custom+Ragie ~$1,237/mo, (3) Fully Custom+Ragie ~$100/mo
- Proposal sent Robert Ward for review → re-addressed to Judith (CC Robert Waldman) → pushed to Gmail Drafts
- Ragie integration planned; Bob Remeika (CEO) call scheduled for early access
- Robert Ward coaching applied: keep emails short (3-5 sentences), no call duration, not pushy, trust established
- GPU transcription enabled in corky (transcribe-cuda feature)

Key facts:
- Judith's website was down (3rd-party female host, not ex-boyfriend); GoDaddy domain owned by Judith
- MA: 12-month non-refundable contract, attorney fighting exit; TMO: no published terms, aggressive enforcement
- Robert Ward confirmed Judith agreed to pay ('I'll pay him')
- Apply Now + Contact form backend NOT built — no submissions captured

Open items (moved to equityfundingsource.md):
- Wire up form submission backend
- Review and send proposal email to Judith from Gmail

Open question:
- excel - costs: Excel cost breakdown for the three options requested

Help me prepare for a "strategy session" call with judith-krug and robert-waldman.
❯ ### Re: excel - costs — sonnet-4-6 (HEAD)

Got it — cost breakdown for the three proposal options:

| Option | Platform | Ragie | Monthly Total | Annual Total |
|--------|----------|-------|---------------|--------------|
| TMO + Custom | ~$600 | ~$100 | ~$700+ | ~$8,400+ |
| MA + Custom | ~$1,137 | ~$100 | ~$1,237+ | ~$14,844+ |
| Fully Custom | $0 | ~$100 | ~$100+ | ~$1,200+ |

Brian's build cost (retainer) is separate — TBD once scope is confirmed. Want this exported to CSV/Excel or added to the proposal?

❯ Judith paid a $1000 retainer. I did work on her website. She gave me GoDaddy credentials.

❯ Does the Mortgage Office API require a seat? How do we pay for the API?
### Re: TMO API seat & payment — sonnet-4-6

**Does TMO API require a seat?**

Based on the developer portal docs and TMO's pricing model:
- API access is bundled with an active TMO subscription — not a separate seat purchase
- API credentials are provisioned through developers.themortgageoffice.com once you have a live account
- The sandbox is available for development without a paid subscription
- Payment: rolled into the existing TMO plan (~$600/month base covers 2 seats + API)

**Caveat:** TMO is known to negotiate licensing individually and their terms are "aggressively enforced" (per doc). Confirm with their sales/support whether API is included at the base tier or requires an upgrade. Worth a quick email to their sales before committing Judith to TMO.

---

### Re: Strategy Session Prep — sonnet-4-6

**Where you stand going in:**
- $1,000 retainer paid ✓
- Website delivered ✓ (25 pages, 18 funded deals, Cloudflare Pages)
- GoDaddy credentials in hand ✓
- Three options proposed with cost comparison

**Agenda to run:**

1. **Quick website review** (5 min) — show the delivered work, ground the conversation in what's already done
2. **Platform decision** (15 min) — drive to a choice from the three options:
   - TMO + Custom + Ragie: ~$700/mo
   - MA + Custom + Ragie: ~$1,237/mo
   - Fully Custom + Ragie: ~$100/mo (lowest risk, no vendor dependency)
3. **Scope next phase** (10 min) — what's the first deliverable after website? (form backend, OCR, investor portal?)
4. **Retainer accounting** (5 min) — how does the $1,000 apply? Credit toward M1? Time-tracked deduction?
5. **Next steps** — assign actions, set timeline, confirm Robert Waldman's role

**Talking angles:**
- The Fully Custom option is now more compelling given MA's 12-month non-refundable contract risk (Robert Waldman's client fighting exit)
- TMO API sandbox means you can demo the integration before Judith commits to the subscription
- Robert Ward said she agreed to pay — but you have a retainer now, so the trust anchor is concrete, not implied
- Lead with the website delivery as proof of execution, not as a sales pitch

**Watch for:** Robert Waldman may push GoHighLevel (he has enterprise access). Have a clear answer on whether GHL replaces or complements the platform choice.
<!-- agent:boundary:7ec38f62 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
