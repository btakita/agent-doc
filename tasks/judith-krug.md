---
session: 9f4d7cf6-4c6c-4c61-b52f-12e9c5908e20
resume: null
agent: null
model: null
branch: null
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
- $18,000 Y1 + $3,000 coding = $21K
- $13,000/year ongoing ($1,137/month)
- Unlimited seats, notifications, portals, Hubspot, servicing, RE 855 disclosure

**The Mortgage Office (TMO) pricing:**
- ~$600/month (2 seats, $400/month per extra)
- Doc generation, manual entry, borrower doc upload
- Industry incumbent in LA, half the price of MA

**Also mentioned:** Kalix Software (likely Calyx Software) integration

## Strategy: Platform-Agnostic Intelligence Layer

Instead of duplicating TMO's or MA's core operations, build a **complementary intelligence layer** that works with any platform:

**TMO's hiring signals** (React UI modernization, AI engineers in San Mateo, aggressive sales push) — they're building what we'd duplicate. Complement instead.

### Value Propositions

1. **Deal sourcing** — Agent loops scanning county records, NODs, probate, expired listings for properties matching Judith's conforming parameters
2. **Investor pitches & pooling** — Custom pitch decks per investor risk profile, pooling orchestration for up to 10 investors per commercial mortgage
3. **Website integration** — Tiered deal marketplace (public anonymized listings → trusted investor access with RE 885 docs)
4. **Scheduled reminders** — Deadlines, follow-ups, milestones (MA has this, TMO doesn't)
5. **Eliminating manual entry** — AI extraction from docs → auto-populate via API
6. **Website modernization** — Redesign, full account custody, analytics-driven sales funnels

### Why TMO First

| Factor | TMO | MA |
|---|---|---|
| Judith's status | Evaluating (family created TMO) | Evaluating |
| Partner ecosystem | Open — developer API, Savvior, Lightning Docs | More closed/proprietary |
| Modernization need | Actively hiring to catch up | Already modern, VC-funded |
| Your opportunity | Fill gaps they can't staff fast enough | Less room |

Tooling is platform-agnostic by design — works with TMO, MA, or her bespoke system.

## Email Timeline

### Pitch Email (2026-02-26)

- **Draft:** `mail/contacts/judith-krug/20260226-tmo-pitch.md` → status: `sent`
- **Synced:** `mail/conversations/untitled-4.md`
- **Issues found after send:**
  - No subject line (corky bug — `EmailDraftMeta` had no `subject` field). **Fixed** in `corky/src/draft/mod.rs`.
  - Factual errors: implied Judith uses TMO ("system that already handles your compliance and servicing", "given your existing investment in TMO"). She uses a bespoke system.

### Correction Email (2026-02-27)

- **Draft:** `mail/contacts/judith-krug/20260227-correction.md` → status: `sent`
- **Synced:** `mail/conversations/quick-correction.md`
- **Sent as threaded reply** with `in_reply_to: <69a0c4df.0c0a0220.e7ca9.c626@mx.google.com>`
- **Threading failed** — Gmail broke the thread because subject changed from empty to "Quick correction". Headers were correct (`In-Reply-To` + `References` both set). Root cause: cascading from the no-subject bug on the original email.
- Added one-liner about developing workflow tools

## Corky Bugs Found & Fixed

### 1. Missing subject field in drafts
- `corky/src/draft/mod.rs` — `EmailDraftMeta` had no `subject` field
- YAML `subject:` key was ignored, only `# Heading` in body was used
- **Fixed:** Added `subject: Option<String>`, YAML takes precedence over body heading

### 2. Gmail filter AND vs OR
- `match = ["from", "to"]` set both criteria on same filter — Gmail AND's them → never matches
- **Fixed:** Multi-field filters now use `query` criteria with OR logic: `from:(addrs) OR to:(addrs)`
- Files: `corky/src/filter/push.rs`, `corky/src/filter/build.rs`
- Filters pushed to Gmail, 168 tests pass

### 3. Reply threading heuristics
- Documented in `corky/docs/guide/commands.md` (Reply Threading Heuristics section)
- Documented in `corky/CLAUDE.md` (Drafting Emails section)
- Rule: follow-ups → threaded reply; new topics → new thread; ambiguous → default reply and ask
- Gmail requirement: subject must match (adding `Re:` is fine, changing subject breaks threading)

## Open Items

- [ ] `corky filter apply` — retroactive filter application via Gmail Messages API (currently web UI only)
- [ ] Auto-derive `Re: <original subject>` when `in_reply_to` is set in draft system
- [ ] Proofreading check — flag factual claims against contact file before sending
- [ ] Follow up with Judith on her response to the pitch and correction

## Assistant

Cleaned the document from ~970 lines to a structured summary. Preserved:

- All contact info and meeting notes
- Strategy and value propositions
- Complete email timeline with issues encountered
- All corky bugs found and fixed
- Open items as a checklist

Removed verbose back-and-forth while keeping every decision, outcome, and lesson learned. The full conversation history is preserved in git.

## User

It's been 2 days. Judith texted me that she will look it over. No response. I think she made her decision.

## Assistant

**Realistic read:** Two days after "I'll look it over" with no follow-up likely means she's going with TMO or MA. The no-subject original email + factual errors weakened the first impression, and established vendors have polished sales processes.

**Options:**
1. **Brief follow-up in 2-3 days** — "Hi Judith, just checking if you had a chance to review. Happy to hop on a call if useful." Low pressure, keeps the door open.
2. **Let it go** — Respect the silence. If she's interested, she'll reach out.
3. **Add value** — Send something useful (TMO vs MA comparison from your research) without asking for anything. Positions you as knowledgeable.

The pitch itself was solid — platform-agnostic intelligence layer is genuinely differentiated from TMO/MA. The delivery just had friction. Lessons captured in the Corky bugs section above. If she passes, the strategy and tooling transfer to the next prospect.

## User

