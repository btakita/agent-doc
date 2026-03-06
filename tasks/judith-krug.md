---
agent_doc_session: 9f4d7cf6-4c6c-4c61-b52f-12e9c5908e20
tmux_session: '1'
---



# Judith Krug — Equity Funding Source

**Contact:** `mail/contacts/judith-krug/CLAUDE.md`
**LinkedIn:** https://www.linkedin.com/in/judith-krug-a4b71b15/
**Company:** http://www.equityfundingsource.com/4
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

## Corky Bugs Found & Fixed

1. **Missing subject field in drafts** — `EmailDraftMeta` had no `subject` field. Fixed.
2. **Gmail filter AND vs OR** — multi-field filters now use `query` criteria with OR logic. Fixed.
3. **Reply threading heuristics** — documented in `corky/docs/guide/commands.md` and `corky/CLAUDE.md`.

## Key People

- **Robert Ward** — referral who connected Brian to Judith
- **Robert Waldman** — Judith's tech/marketing consultant, family taught Judith the mortgage business, based in Texas, has enterprise GoHighLevel access

## Meeting Recording (2026-03-04)

- **Audio:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-.amr`
- **Diarized transcript:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-waldman-diarized.txt`
- **Notes:** `mail/contacts/judith-krug/recordings/20260304-judith-krug-robert-waldman-notes.md`
- **Shareable meeting notes:** `mail/contacts/judith-krug/recordings/20260304-meeting-notes-for-sharing.md`

## Session Log (2026-03-05)

### Platform Decision

Judith chose **Mortgage Automator** initially but had second thoughts when the $1,800 pending charge hit. She texted asking for a comparison if she went with TMO instead. Robert Ward confirmed she was still deciding (silence ≠ rejection — she gets busy). Robert Waldman is advising on tech stack.

### Proposal Development

Full proposal at: `mail/contacts/judith-krug/20260305-tmo-proposal.md` (PDF: `20260305-tmo-proposal.pdf`)

**Key decisions made during drafting:**

1. **Pricing:** Started at $5K → $6K → $7K/70hrs → $8K/80+hrs. Final: **$8,000 flat for 80+ hours**, two $4,000 milestone payments. $100/hr for additional work.
2. **Milestones:** M1 = website + document upload portal. M2 = OCR/TMO integration + notification system.
3. **Monthly costs:** ~$670–845/mo (TMO ~$600, Google Workspace 5 seats $35, AI/hosting $10-200+, Cloudflare $0-5, domain $0).
4. **Support:** 1 month free post-completion, then $100/hr as needed.
5. **Licensing:** Royalty-free perpetual license (non-exclusive). Optional exclusive reseller rights for mortgage market available.

### Technical Stack Decisions

- **Framework:** Hono + Cloudflare Workers/Pages (Astro considered but skipped — Hono is simpler, no framework overhead)
- **Components:** relementjs + rmemo (Brian's libraries)
- **CMS:** Keystatic (GitHub mode) for staff content editing via browser
- **Hosting:** Cloudflare Pages (free tier, LAX PoP, email routing)
- **Office suite:** Google Workspace Business Starter (5 seats, $35/mo)
- **CRM:** GoHighLevel (Robert has free enterprise access) or HubSpot — in bonus scope

### Development Phases (63-90 hours, ~5 weeks)

| Phase | Deliverable | Hours |
|-------|------------|-------|
| 1 | Website migration to Cloudflare (Hono + Workers) | 15-20 |
| 2 | Document upload portal (secure links, Drive/Dropbox) | 12-16 |
| 3 | AI-powered OCR + TMO API integration | 16-26 |
| 4 | Notification system (Google Calendar-based) | 10-14 |
| Cross-cutting | Event logging, testing, documentation, handoff | Inline |

### TMO API

- **Developer Portal:** https://developers.themortgageoffice.com/
- **Sandbox:** Token "TMO", Database "API Sandbox"
- **Key endpoints:** NewLoan, UpdateLoan, GetLoan, NewLender, AddFunding, GetLoanAttachments, NewConversation
- **OCR pipeline:** Upload → Claude vision extracts fields → staff reviews → push to TMO via API

### Bonus Scope (if core finishes early)

- Phone recording + transcripts (telephony — separate plan in `tasks/telephony.md`)
- County record scraping for lenders/investors (beneficiaries)
- GoHighLevel or HubSpot CRM integration
- Additional website features
- Investor portal (minimal: 2-4 hrs; medium with TMO API: 8-14 hrs)

### AI Cost Tiers

| Usage level | Est. cost/mo |
|-------------|-------------|
| Light (OCR only, 1-5 docs/day) | $30-80 |
| Moderate (+ phone transcription, 5-10 calls/day) | $80-200 |
| Heavy (high call volume) | $200-500 |

### Website Audit (equityfundingsource.com)

- Stack: WordPress + Elementor Pro + WPForms + Bookly + Infusionsoft
- SEO gaps: no robots.txt/sitemap, no FAQ schema, no llms.txt, funded deals in images not text
- Quick wins identified: robots.txt, FAQ page, llms.txt, text-ify funded deals
- Site structure plan: main site (borrowers + brokers), separate investor portal, separate broker portal

### Staff Tooling

- **Content editing:** Keystatic browser UI (no git knowledge needed)
- **Layout/structural changes:** Claude Code in plain English
- **Text editor (if needed):** VS Code recommended
- **Obsidian:** potential for internal knowledge base; agent-doc integration feasible but needs robustness work
- **CLI tool (`efs`):** custom business tooling planned for structured workflows

### Corky Feature: `corky doc build`

Implemented during this session:
- `corky doc build <file.md>` → PDF via pandoc + weasyprint
- `corky doc build --format docx <file.md>` → DOCX via pandoc
- Template selection via frontmatter (`template: proposal`)
- Files: `corky/src/doc/{mod,build}.rs`
- Proposal template: `mail/templates/proposal.css` + `mail/templates/build-pdf.sh`

### Final Deliverable

**Email drafted and pushed to Gmail:**
- To: judithkrug@gmail.com
- CC: rwaldman100@gmail.com (Robert Waldman)
- Subject: Proposal — TMO + Custom Automation for Equity Funding Source
- Attachment: 20260305-tmo-proposal.pdf
- Draft file: `mail/drafts/20260305-judith-proposal.md`

## Open Items

- [ ] Judith reviews proposal and decides MA vs TMO + custom
- [ ] Schedule Zoom call with Judith and Robert Waldman for deeper requirements
- [ ] Talk to MA about website integration specs (if she stays with MA)
- [ ] Telephony plan details (see `tasks/telephony.md`)
- [ ] `corky filter apply` — retroactive filter application via Gmail Messages API
- [ ] Auto-derive `Re: <original subject>` when `in_reply_to` is set
- [ ] Proofreading check — flag factual claims against contact file before sending
- [ ] `corky doc upload` — Google Docs upload feature (stubbed)

## User

Judith has not gotten back to me. Should I follow up to see what she decided to do?

## Assistant

**Wait until tomorrow or Friday.** The proposal was sent earlier today — same-day follow-up reads as pushy. From earlier context: Robert Ward confirmed "silence ≠ rejection — she gets busy." She'll want to review it and likely discuss with Robert Waldman first.

When you do follow up, keep it light — confirm receipt, don't ask for a decision:

> "Just wanted to make sure the proposal came through OK — happy to jump on a call whenever you're ready to discuss."

Want me to draft that follow-up for tomorrow?

## User

