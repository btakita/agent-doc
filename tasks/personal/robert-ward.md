---
agent_doc_session: aaca1487-6409-4c94-88d6-97b579485190
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

Robert Ward is a long time friend who lives in LA.
He has a Chinese Medicine, acupressure, therapeutic massage practice.

He did not complete the Chinese Medicine program

robert-ward introduced me to judith-krug.

He often works from his phone and rarely uses his laptop.
He often needs help looking for housing on CraigsList.
Also looking for a storage unit.

I have been thinking of building a tool to scrape CraigsList `rooms / shared` category for specific areas in  Santa Monica, Venice, Palisades.

Please transcribe and diarize phone calls with robert-ward.
Brian Takita and Robert Ward are in the phone calls.
## Phone Call Transcripts (Mar 7–9)

Transcribed and diarized 5 phone calls with Robert Ward (310-968-4542). Whisper (small model); speaker attribution by conversational context. Full transcripts archived at `mail/contacts/robert-ward/recordings/`.

### Call 1: 2026-03-07 10:15 PM ET (~8 min)
**Topic:** Judith Krug proposal follow-up. Brian sent $8K proposal but no response. Robert offers to call Judith directly. Coaches Brian: be willing to compromise, don't mention other work, only talk about her business.
### Call 2: 2026-03-07 10:26 PM ET (~65 min)
**Topics:** Judith at daughter's birthday party, agreed to talk Sunday. Robert's storage garage — landlord demanding he leave by 3/12. Brian researches CA tenant law live: retaliatory eviction (CA Civil Code 1942.5) applies since Robert complained about water leak and landlord relisted at higher rent. However, standalone storage = commercial tenancy, so 60-day notice doesn't apply. Brian drafts legal letter.

### Call 3: 2026-03-08 3:27 PM ET (~14 min)
**Topic:** Strategy before Judith call. Robert coaches: stage payments ($2K increments), be concise, she's emotional about big numbers. "Build is the new buy" pitch — Robert loves it as a call opener.

### Call 4: 2026-03-08 8:31 PM ET (~15 min)
**Topic:** Judith confirmed 100% Brian doing website. Wants Brian to study MA and replace it. Brian's pitch: MA $13K/yr + $5K setup vs his $8K flat. Phase 2: replace TMO ($700/mo). Custom integrates everything — CRM, county records, all her business. Sunday call scheduled 10 AM Pacific.

### Call 5: 2026-03-09 12:46 AM ET (~17 min)
**Topics:** Judith confirmed Sunday call with Robert Waldman. DST confusion. Garage: 30-day notice valid for non-residential but retaliatory defense still applies. Brian: negotiate deposit return + moving help. Perplexity vs Claude Code discussion.

---

## Key Takeaways Across All Calls

**Judith Krug / Equity Funding Source:**
- Brian sent an $8K proposal (80 hours) to replace Mortgage Automator
- Judith confirmed Brian will do the website
- Judith thinks the $8K price is fair ("no, that's a fair price")
- Sunday call scheduled: Brian, Judith, and Robert Baldwin (Judith's business partner/ex)
- Strategy: "build is the new buy" — custom software vs SaaS lock-in
- Phase 1: Replace Mortgage Automator ($13K/yr + $5K setup → Brian's $8K)
- Phase 2: Replace Mortgage Office ($700/mo = $8.4K/yr)
- Robert advises: stage payments, be concise, focus only on her business

**Robert's Storage Garage (Santa Monica):**
- 4-year tenant, $325/mo, $325 deposit
- Landlord gave 30-day notice (terminate by 3/12/26)
- Robert complained about water leak → landlord immediately relisted at higher rent
- This is textbook retaliatory eviction (CA Civil Code 1942.5)
- However: standalone storage = commercial tenancy, so 60-day notice (1946.1) likely doesn't apply
- Retaliatory eviction defense still applies even for commercial
- Landlord threatened to arrest Robert for trespassing if he doesn't notify before accessing — this is illegal
- Strategy: Send certified letter citing retaliatory eviction, negotiate for deposit return + moving help
- Brian drafted and emailed the legal letter to Robert

## User

Note the changes to the frontmatter.

<!-- agent:exchange -->

### Storage Unit Update

Robert reserved 2 public storage units, moved items into 1, and is canceling the 2nd. PS wants to charge a $40 admin fee for the cancellation.

**Can his bank reverse the $40?**
Yes — Robert can file a chargeback/dispute. For $40, banks typically issue a provisional credit quickly. Strength of the dispute depends on whether the admin fee was disclosed upfront:
- **Not disclosed** → strong chargeback case
- **Agreed to in terms** → bank may rule in PS's favor after investigation

**Would Public Storage retaliate?**
For the unit he's keeping: PS *could* flag his account or claim an outstanding balance, restricting access until resolved. Large storage companies generally don't retaliate over $40, but disputing while being an active customer creates friction.

**Recommended approach:**
1. Call PS corporate (not local facility) and request the fee be waived — he's canceling promptly and keeping the other unit
2. If they refuse and Robert has no remaining business with PS, then dispute with the bank
3. If he still has the active unit, weigh whether $40 is worth the risk of PS complicating access on the unit he's keeping

Robert & I are thinking of having a business.
Robert has many connections in LA.
I want to make a website and have a "company name" for my agent services.

Please help with a marketing plan + a company name.

### Business Partnership: Brian + Robert

**What you're selling:** AI agent services — custom automation, document workflows, correspondence tools, web development. Brian builds, Robert connects.

**Complementary strengths:**
- **Brian:** Full-stack developer, AI agent tooling (corky, agent-doc), Rust/TypeScript, infrastructure
- **Robert:** LA network, relationship builder, sales/coaching instinct (demonstrated in Judith Krug deal)

---

### Company Name Ideas

Names that signal AI/automation + personal service:

| Name | Domain availability likely | Vibe |
|------|---------------------------|------|
| **Ward & Takita** | wardandtakita.com | Professional services, trust |
| **Agentcraft** | agentcraft.io | Technical, builder-oriented |
| **Relay Works** | relayworks.co | Automation + human handoff |
| **Signal Loop** | signalloop.io | Feedback systems, AI agents |
| **Automaton Partners** | automatonpartners.com | Classic + technical |
| **Build Side** | buildside.co | "Build is the new buy" — your pitch |
| **Takita Ward** | takitaward.com | Names-first, consultancy feel |

**My top pick:** **Build Side** — it directly references the "build is the new buy" pitch Robert already loves, it's memorable, and it works for both technical and non-technical audiences.

So far buildside.co is the best.

---

### Marketing Plan (Phase 1: Launch)

**1. Positioning**
- "We replace your expensive SaaS subscriptions with custom software that you own."
- Target: Small/mid businesses paying $500+/mo for tools they've outgrown
- Proof point: Equity Funding Source deal (replacing $13K/yr MA + $8.4K/yr TMO)

**2. Website (Week 1-2)**
- Single-page site: hero, services, case study, contact form
- Services listed: Custom web apps, AI automation, SaaS replacement, data integration
- Stack: Static site (Astro or plain HTML), Cloudflare Pages (free hosting)

**3. Robert's Role: Referral Engine**
- Robert works his LA network — every small business owner he knows who complains about software costs is a lead
- Script: "My partner builds custom software for less than what you're paying monthly for [tool]. Want an intro?"
- Commission structure: % of first contract for referred clients

**4. Initial Channels**
- **Word of mouth** (Robert's network) — highest conversion, zero cost
- **LinkedIn** (Brian's profile) — post case studies, technical credibility
- **Local business meetups** (Robert attends, hands out cards)
- **Craigslist services section** — Robert already lives on CL, post in computer/gigs

**5. Pricing Model**
- Project-based: $5K-$15K for SaaS replacement projects
- Retainer option: $1K-$3K/mo for ongoing maintenance + new features
- "ROI guarantee" angle: "If we don't save you money vs. your current tools within 6 months, we'll refund the difference"

**6. First 3 Clients**
- Judith Krug / Equity Funding Source (already in progress)
- Robert's other contacts who need web/tech work
- One cold outreach win from LinkedIn or CL

---

**Next steps:**
1. Pick a name and register the domain
2. Brian builds a one-page site (can be done in a day)
3. Robert starts soft-pitching to 5 contacts this week
4. File a simple LLC once revenue starts (CA LLC = $800/yr minimum franchise tax, so wait until you have a paying client)

Want me to draft the website copy or dig into any of these areas?

Draft an email to Robert Ward robertward3@yahoo.com. Including:
- `Marketing Plan (Phase 1: Launch)`
- Other steps for robert to take

### Email Drafted

Email to Robert (robertward3@yahoo.com) pushed to Gmail Drafts (btak.dev@gmail.com).

**Subject:** Build Side — Marketing Plan + Next Steps

**Contents:**
- Marketing Plan (Phase 1): website, network referrals, channels, pricing, first clients
- Steps for Robert: identify 5 contacts, soft-pitch, business cards, LLC timing

**Status:** In Gmail Drafts — open Gmail to review and send.

Added context about Robert:

Mother:
- Dr Barbara Korsch
- Children's Hospital in LA
  - Head of Children's Hospital teaching program
    - Taught interns and residents
- Professor of pediatrics
- 1st female Dr who conducted a Kidney Transplant on a child
  - On the team. Not a lead Physician
- Documentary made about her
  - Portrait of a Pioneer
- Author of a multi volume publication on Doctor <-> Patient Communication

Father:
- Dr Robert Ward II
- Physician in Child in Children's Hospital in LA
- Surgeon
- Wrote books
- Author
  - Infectious Diseases of Children

Grandfather:
- Helped thousands of people escape from Nazi Germany
- Karl Korsch
  - https://en.wikipedia.org/wiki/Karl_Korsch

### Robert's Family — Summary

**Mother — Dr. Barbara Korsch**
- Professor of Pediatrics, Children's Hospital Los Angeles
- Head of the teaching program (trained interns and residents)
- On the team that performed one of the first kidney transplants on a child (not lead physician)
- Subject of documentary *Portrait of a Pioneer*
  - https://www.imdb.com/title/tt1329423/
  - https://www.portraitofapioneer.com/about.html
- Author of multi-volume publication on doctor-patient communication — pioneered this field in medical education
  - We Are All Patients
    - https://www.amazon.com/Are-Patients-Barbara-Maria-Korsch/dp/1932800387

**Father — Dr. Robert Ward II**
- Physician/surgeon at Children's Hospital Los Angeles
- Author of *Infectious Diseases of Children* — foundational pediatric textbook

**Grandfather — Karl Korsch**
- Helped thousands escape Nazi Germany
- Notable Marxist theorist and philosopher

Updated Robert's contact file (`mail/contacts/robert-ward/CLAUDE.md`) with family details, practice info, and Build Side partnership note.

Updated contact file with the additional details:
- *Portrait of a Pioneer* — IMDB (tt1329423) and portraitofapioneer.com links added
- *We Are All Patients* — book title and Amazon link added
- "1st female Dr on a team that conducted a kidney transplant on a child" — added to contact file

Let's extract out buildside.md
I registered buildside.co

Created `tasks/buildside.md` with:
- Business overview (domain: buildside.co, partners, tagline)
- Full marketing plan (Phase 1)
- Next steps checklist (domain registration marked done)
- Template mode with exchange component for future sessions

Run `/agent-doc tasks/buildside.md` to continue planning there.

Please transcribe and diarize these phone calls:
[phone_20260327-040231_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-040231_3109684542.amr)
[phone_20260327-020734__13109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-020734__13109684542.amr)
[phone_20260322-022648_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260322-022648_3109684542.amr)
[phone_20260321-051112_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260321-051112_3109684542.amr)
[phone_20260327-001518_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-001518_3109684542.amr)
[phone_20260326-043254_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-043254_3109684542.amr)
[phone_20260326-005855_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-005855_3109684542.amr)
[phone_20260324-214531_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260324-214531_3109684542.amr)
[phone_20260324-005235_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260324-005235_3109684542.amr)
[phone_20260321-051012_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260321-051012_3109684542.amr)
[phone_20260321-051012_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260321-051012_3109684542.amr)
[phone_20260321-051112_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260321-051112_3109684542.amr)
[phone_20260322-022648_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260322-022648_3109684542.amr)
[phone_20260323-183219_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260323-183219_3109684542.amr)
[phone_20260323-211012_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260323-211012_3109684542.amr)
[phone_20260324-005235_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260324-005235_3109684542.amr)
[phone_20260324-214531_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260324-214531_3109684542.amr)
[phone_20260325-130005_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260325-130005_3109684542.amr)
[phone_20260326-005855_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-005855_3109684542.amr)
[phone_20260326-034516_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-034516_3109684542.amr)
[phone_20260326-040656_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-040656_3109684542.amr)
[phone_20260326-043254_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260326-043254_3109684542.amr)
[phone_20260327-001518_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-001518_3109684542.amr)
[phone_20260327-020734__13109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-020734__13109684542.amr)
[phone_20260327-040231_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-040231_3109684542.amr)
[phone_20260327-212919_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260327-212919_3109684542.amr)
[phone_20260328-000319_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260328-000319_3109684542.amr)
[phone_20260328-001315_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260328-001315_3109684542.amr)

Note that [phone_20260328-000319_3109684542.amr](../../mail/contacts/robert-ward/recordings/phone_20260328-000319_3109684542.amr) is a 3 way call with judith-krug, robert-ward, brian-takita

### Transcription in Progress (Mar 21–28 Calls) (HEAD)

Noted the frontmatter changes (`agent_doc_format: template`, `agent_doc_write: crdt`).

Transcribing 18 unique phone calls from Mar 21–28. Using Whisper large-v3-turbo model (CPU-only, no GPU detected). Diarization not available — speaker attribution by conversational context.

**Completed (4/18):**
- ✅ phone_20260321-051012 (~12s call)
- ✅ phone_20260326-034516
- ✅ phone_20260327-001518 (~39s call)
- ✅ phone_20260328-000319 (3-way call with Judith Krug — 318 segments)

**Processing:**
- ⏳ phone_20260321-051112 (7.4M — largest file, ~80+ min audio)
- ⏳ phone_20260326-040656 (2.0M)
- Plus 12 more files queued

Summaries will be added once all transcriptions complete. Run `/agent-doc tasks/personal/robert-ward.md` again after ~30 min to get the full results.
<!-- agent:boundary:af7d04fe -->
<!-- /agent:exchange -->
