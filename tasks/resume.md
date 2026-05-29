---
agent_doc_session: db08a891-c339-4a49-9f49-2d8a1f30ed10
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
agent: claude
opencode_model: GLM-5.1
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
codex_network_access: enabled
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#corky-bug': Please create a plan for corky to fix this issue. Add to the backlog of agent-loop/tasks/software/corky.md
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-commit': update spec + tests. commit
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---


# Resume Session

<!-- agent:status -->
**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)
**Profile Statements**: `resume/profile-statements.md` — LinkedIn headlines, tagline, summary
**agent-resume**: [btakita/agent-resume](https://github.com/btakita/agent-resume) — complete: build, render, validate, search-index commands; search.html for GitHub Pages
**Tests**: `make test` — 12 tests (search index validation + LightPanda DOM tests)
**Apex CVS**: `profiles/apex-cvs.toml` — custom profile for CVS Health via Deepak Rana (Apex Systems). PDF: `BrianTakita-apex-cvs.pdf`
<!-- /agent:status -->

## Completed

### Resume Content
- Markdown resume from LinkedIn PDF + briantakita.me portfolio/open-source mining
- Career Highlights section (pages 1-2): 8 one-liners spanning 20-year career
- 5 rounds proofread (casing, tense, corporate names, punctuation)
- 83+ highlight phrases tuned for: agentic loops, reactive programming, event-driven, ML, real-time systems, context engineering

### Build System
- `build.py` + Makefile → pandoc (fragment mode) + weasyprint with `resume.css`
- `profiles/default.toml`: phrase highlighting, skill bolding, section filtering
- GitHub Action: builds PDF, commits, deploys to GitHub Pages

### Key Decisions
- **Career Highlights** always shown (Option A) — benefits every reader, front-loads signal
- **Single profile** (default) — startup-founder profile removed; Career Highlights makes it redundant
- **LinkedIn headline**: Option 3 hybrid (212 chars) for recruiter/founder scanning
- **"multimodal"** not "multi-modal" (standard ML/AI literature)
- **BDD Pioneer** added — connects RSpec/rr to the BDD movement narrative
- **RR proxy innovation** — pioneered proxy-based test doubles, now standard as `patch` in pytest

### Foxley Talent — Full Stack Engineer
- **Role:** Full Stack Engineer (SaaS infrastructure), fully remote US, $150K-$180K + equity
- **Stack:** Python/Django backend, React/TypeScript frontend, API/distributed systems, CI/CD
- **Calendly notes:** Honest framing — Rails ↔ Django mapping, no Django overclaim, "days not weeks" framework pickup
- **Revised notes sent:** Presence AI pipeline, FastAPI/Flask/Graphene/SQLAlchemy, 10+ years Rails as Django equivalent

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/98e0966f0f923652f73812054e6cd6c829ccef13e7b897464d74658860d0278e-20260529-192019.md`*

Compacted content:
- Archived 11 response topic(s): Arunprakash S contact created; WBD Rust Backend Developer; Resume updated + email sent to Arunprakash; 8 more
- Prior summary/context: ### Session Summary **agent-resume Rust crate** ([btakita/agent-resume](https://github.com/btakita/agent-resume)) - H...
❯ What would you recommend to show my skills for:
❯ - Dreamable (redacted or private to follow IP in my separation agreement). See tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.md
❯ - Monsterrodholders
❯ - Equity Funding Source

❯ I will add to LinkedIn and my resume.

### Re: Skills to show — Dreamable / Monsterrodholders / Equity Funding Source — sonnet-4-6

#### Dreamable (confidentiality-constrained)

Section 5 of the settlement bars disclosure of non-public business info (technical data, trade secrets, product plans). Section 3 assigned all work product IP to Dreamable. You can list the engagement and your own skills; you cannot describe what you built for them.

**Safe framing:**

> **Dreamable, Inc. — Software Development Consultant** *(Apr–May 2026)*
> Short-term consulting engagement; work product IP assigned to company per agreement.

Better angle: your **Prior Inventions from the same period are yours**. Per the addendum, tsift and homebrew-tap (built Apr 6 – May 26) are Consultant property. Lead with those instead:

> **tsift** — Built and shipped a token-efficient CLI search plugin for Claude Code during this period: hybrid BM25 + vector (Sift) search, AST-aware code navigation, native tool routing. Used as primary code-navigation tool across projects.

Recommendation: if the Dreamable entry adds little signal (7 weeks, settled, can't describe the work), omit it from the resume and LinkedIn Experience, and let tsift + homebrew-tap appear under Open Source / Tools.

Can I leave the company blank and give a general description of what I did?

---

#### Monsterrodholders — What to Show

**LinkedIn position:**
> **Freelance E-commerce & WordPress Consultant** — MonsterRodHolders.com *(2026)*

**Resume bullets:**

- Identified critical security compromise (admin account hijacked) on WooCommerce site; documented remediation path and evaluated platform migration options (WordPress vs Shopify)
- Conducted competitive analysis across a 57-SKU fishing-tackle catalog: 4 competitors analyzed, 22 resellers mapped across 15 states, pricing intelligence gathered
- Configured Cloudflare CDN for performance and DDoS protection; restored Google Analytics tracking pipeline
- Advised on shipping rule logic for rod holders, landing nets, and river anchors

**What you can show publicly:** monsterrodholders.com is live — but attribution requires Ron's sign-off before putting it in a portfolio. Flag him to confirm.

---

#### Equity Funding Source — What to Show

This is the strongest portfolio entry of the three — greenfield fintech platform on modern Cloudflare edge stack.

**LinkedIn position:**
> **Software Engineer / Tech Lead** — Equity Funding Source *(2026)*
> Full-stack loan origination platform — Cloudflare Workers, D1, R2, Cloudflare Access, React, TypeScript

**Resume bullets:**

- Architected and built a full-stack loan origination platform from scratch: public prequalification form, D1-backed application lead pipeline, email notification via Resend
- Implemented multi-role authentication (admin / borrower / broker / investor) using Cloudflare Access with one-time PIN — no external auth vendor required
- Built invite-only admin portal: lead review queue, user account creation, protected route management
- Designed OCR/document intelligence pipeline for automated loan document extraction (field extraction → reviewed writeback → TMO integration)
- Planned integration with The Mortgage Office (TMO) via outbound webhooks for loan-lifecycle sync
- Stack: Cloudflare Workers · D1 · R2 · Cloudflare Access · React · TypeScript · Hono · Resend · Wrangler

**Portfolio artifact:** https://equityfundingsource.brian-takita.workers.dev (live deployment — link on LinkedIn until custom domain is live)

---

#### Skills to add to LinkedIn / Resume Skills section

From these three engagements, new additions worth surfacing:

- **Cloudflare Workers, D1, R2, Cloudflare Access** (from EFS — edge-native full-stack)
- **Hono** (Workers-native web framework)
- **WooCommerce, Cloudflare CDN** (from MRH)
- **Fintech / Loan Origination** (domain — EFS)
- **Security Auditing** (WordPress — MRH)

### Re: Leave company blank for Dreamable? — sonnet-4-6

Yes — legally fine per the agreement. Nothing in the settlement prohibits omitting the company name or listing it as "Confidential." The confidentiality clause (§5) covers *their business information*, not the existence of the engagement. You're free to describe your own skills in generic terms.

**Practical options:**

**Option A — "Confidential client" framing**
```
Confidential AI Startup — Software Development Consultant (Apr–May 2026)
• AI/ML systems engineering, Rust; short-term engagement; all work product IP assigned to client.
```
This is a recognized resume/LinkedIn convention for NDA-covered roles. LinkedIn accepts any company name string — type "Confidential" in the company field.

**Option B — Skills-forward, no employer named**
```
Independent Contractor — AI/Systems Engineering (Apr–May 2026)
• Rust, AI/ML — [brief generic description of skill area, no product details]
```
Appears under your own name as a contractor period; no company entry needed.

**Practical note on LinkedIn:** LinkedIn expects a company name for each experience entry. "Confidential" or "Private Company" in the company field works fine — recruiters see it routinely for NDA engagements. If you add a public description, keep it to skill/tech-category level (e.g., "AI infrastructure development in Rust") without business-domain specifics.

**Recommended:** Given it was only 7 weeks and the work can't be shown, Option A with minimal description is cleaner than a gap. Or skip it entirely and let tsift (your own IP, same period) carry the narrative for that time.
❯ I cannot describe that I used Livekit?

### Re: LiveKit and Dreamable — sonnet-4-6

**You can claim LiveKit as a skill — but don't attribute it specifically to Dreamable.**

The distinction:

- **"I know LiveKit" (your Skills section)** — fine. Skills you've gained are yours regardless of where you used them.
- **"At [Confidential Client], I built X using LiveKit"** — edges into revealing their technical stack, which is "technical data" under §5 of the settlement.

**But it doesn't matter for the resume:** LiveKit is already attributed to **Presence AI** in your current resume:

> *"…frame sequencing on NVIDIA H100 GPUs via WebRTC using LiveKit"*

That's the right attribution and it's clean. No need to also attribute it to a confidential engagement. Keep LiveKit under Presence AI and in your Skills list — that's sufficient and accurate.

If you used LiveKit at Dreamable too, you can mentally count it toward your depth of experience, but there's no resume reason to add a second attribution when the first one already demonstrates it.
❯ Can I have general skills used or does everything need to be confidential?

### Re: General skills in a Confidential entry — sonnet-4-6

**Yes — skills are yours to claim. Only Dreamable's business information is off-limits.**

The confidentiality clause (§5) covers their non-public *business* information: product plans, customers, trade secrets, financials, operations. It does not cover the general technologies you used to do the work — that's your own skill set.

Safe for a "Confidential Client" entry:
```
Confidential AI Startup — Software Development Consultant (Apr–May 2026)
• Real-time AI systems engineering — Rust, LiveKit, WebRTC
```

**The test:** would a Dreamable employee read "Rust, LiveKit, WebRTC" and say "that reveals our trade secrets"? No — those are widely used tools, not their product or architecture secrets.

**What's NOT safe:** describing what you built with those tools at that company — the specific system, its purpose, the architecture decisions, the customer use case. That's product/business information.

So the format to use:
- Skills: ✅ `Rust, LiveKit, WebRTC, AI/ML engineering`
- What you built: ❌ `built a real-time video AI avatar system for [use case]`

The skills list for the entry can be as specific as the technology names. Just don't pair them with a description of the product.
❯ Python, Typescript, Realtime asset generation?

### Re: Python, TypeScript, Realtime asset generation — sonnet-4-6

All fine. Same rule applies — these are skill/domain descriptors, not Dreamable's business information.

- **Python, TypeScript** — technology names, clearly yours to claim
- **Realtime asset generation** — describes a work domain/capability area, same as "AI/ML engineering" or "WebRTC media pipelines." Not a product description.

So the Confidential entry could read:

```
Confidential AI Startup — Software Development Consultant (Apr–May 2026)
Skills: Rust, Python, TypeScript, LiveKit, WebRTC, realtime asset generation
```

That's clean, accurate, and reveals nothing about their product or business. The key is you're describing *your* capabilities, not their system.
❯ Code/Harness analysis?

### Re: Code/Harness analysis as a skill — sonnet-4-6

Yes — same rule. "Code analysis" and "agent harness analysis/development" are capability descriptors, not Dreamable's business information.

Safe to add:

```
Confidential AI Startup — Software Development Consultant (Apr–May 2026)
Skills: Rust, Python, TypeScript, LiveKit, WebRTC, realtime asset generation, code analysis, agent harness development
```

The pattern holds for any skill you want to add: if it describes *what you can do*, it's yours to claim. Only the *what you built for them* is off limits.

---

**Note:** The exchange has grown to 165 lines across 5 response sections. Would you like to compact it? Run `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/resume.md --commit` to archive and reset.
❯ Should I consolidate all work into a single entry or have 3 separate jobs?

### Re: 1 consolidated entry vs 3 separate — sonnet-4-6

**3 separate entries** — for LinkedIn. **Consolidated section** — for resume.

#### LinkedIn: 3 separate

LinkedIn shows each entry independently with its own date range and company name. Three entries in 2026 reads as active/in-demand, not fragmented. Keeping them separate makes each searchable by company name and tech.

```
Equity Funding Source — Software Engineer (2026–present)
  Cloudflare Workers · D1 · R2 · React · TypeScript · Hono · Cloudflare Access · Resend

MonsterRodHolders.com — Freelance E-commerce Consultant (2026)
  WordPress · WooCommerce · Cloudflare CDN · Google Analytics · competitive analysis

Confidential AI Startup — Software Development Consultant (Apr–May 2026)
  Rust · Python · TypeScript · LiveKit · WebRTC · realtime asset generation
```

EFS is the anchor — give it the most detail. Dreamable stays brief with just skills (no description needed).

#### Resume: single consulting section

Resume format is more flexible. A "**Independent Consulting (2026)**" section with 3 sub-entries lets you control emphasis and prevents the Dreamable entry from looking like a gap-filler:

```
Independent Consulting                                           2026

  Equity Funding Source — Software Engineer
    • [full bullets]

  MonsterRodHolders.com — E-commerce & WordPress Consultant
    • [3-4 bullets]

  Confidential AI Startup — Software Development Consultant (Apr–May 2026)
    Skills: Rust, Python, TypeScript, LiveKit, WebRTC, realtime asset generation,
            code/harness analysis
```

This groups them visually as one "consulting period" while still distinguishing each client. EFS leads since it's the strongest.
❯ Please create the Independent Consulting role using the opus model.

### Re: Created the Independent Consulting role — opus-4-8

Added the **Independent Consulting** entry to `resume/BrianTakita.md`, placed at the top of Experience (most recent, 2026 – Present, above Presence AI). It groups all three engagements as one consulting period with EFS leading:

```
### Independent Consulting — _Software Engineer / Consultant_

<small>2026 – Present</small>

- **Equity Funding Source** — Architected and built a full-stack loan origination
  platform on Cloudflare's edge stack: public prequalification intake, D1-backed
  application-lead pipeline, multi-role auth via Cloudflare Access. Built an
  invite-only admin portal and designed an OCR document-intelligence pipeline.
  Cloudflare Workers, D1, R2, Cloudflare Access, Hono, React, TypeScript, Resend, Wrangler.
- **MonsterRodHolders.com** — E-commerce/WordPress consulting for a WooCommerce
  storefront (57-SKU catalog). Documented an admin-account security compromise +
  remediation, evaluated WordPress-vs-Shopify migration, configured Cloudflare CDN,
  restored analytics. WordPress, WooCommerce, Cloudflare CDN, Google Analytics.
- **Confidential AI Startup** — Short-term AI systems engineering consultant
  (Apr–May 2026). Rust, Python, TypeScript, LiveKit, WebRTC, realtime asset
  generation, code/harness analysis.
```

I followed the existing freelance bullet pattern (bold company prefix), matching the `### Brian Takita — _Full-Stack Engineer_` section already in the resume.

**Verified:** `make` builds cleanly — `BrianTakita.pdf` regenerated.

**Not committed.** The change is staged in the `resume/` repo (its own git remote, `btakita/resume`) but not committed or pushed, since you asked me to *create* the role, not deploy it. Tell me `commit + push` when you've reviewed it.

**One flag:** the MonsterRodHolders bullet is in now, but I'd confirm attribution with Ron before this goes live publicly (LinkedIn/published PDF) — added a backlog reminder.
❯ Add Google Ads to monsterrodholders.com.

### Re: Google Ads work in src/boost-client (monsterrodholders) — opus-4-8

Investigated `src/boost-client` (read-only). The Google Ads work is substantial and lives under the monsterrodholders site, not here. Summary of what's done vs. still open:

#### Done

- **Order attribution** (`#gadsglk`, deployed) — every WooCommerce order now captures `gclid`, `utm_*`, and `_gcl_aw` as order meta via `mrh-google-ads.php` v1.3/v1.4. This is the click→revenue join key.
- **Root-caused the "$1 conversions" problem** — the canonical issue was that **Google for WooCommerce was never installed**; the flat $1 came from the codeless gtag action `1069392929`. GfW orphaned actions `7058230965`/`7058446455` recorded 0.
- **GfW v3.7.0 deployed + Merchant Center `5768131521` connected** (May 29).
- **`#nsvaltag` (keystone, done)** — configured the canonical Purchase conversion tag in GTM-MWKP3BR8 to send dynamic `value`/`currency`/`transaction_id` from the dataLayer.
- **Google Ads API confirmed working** (`#gadsapiver`) against production customer `1189088403` on v21/v20 — Basic access effectively already granted.
- **New MCC `900-767-5798`** created for the manager-level developer token.
- **Shopping-waste RCA** — $284 (19%) of $1,481 wasted; 21 of 34 products had clicks + 0 sales. Paused an over-spending campaign, blocked zero-sale search terms.
- **`scripts/google-ads-conversion-backfill.mjs`** — ConversionAdjustment uploader (v18→v21, dual-identifier, 55-day window).
- **Budget guardrail** (`#adsbudget`) — hold $50–75/day until tracking is trustworthy.

#### In review / open (tracked in `boost-client/tasks/monsterrodholders.md`)

- `#gfwconnect` — finish GfW onboarding: select the **existing** MC `5768131521` (a new one orphans the feed), link Ads `1189088403`, enable conversion tracking. Blocked by `#gfwhandshake`.
- `#gadsconsolidate` [in review] — keep one GfW dynamic-value purchase action as Primary, demote the rest; repoint the `passage` conversion-action so `#nsbfrun` restatements stop getting rejected (`CANNOT_RESTATE…DEFAULT_VALUE`).
- `#nsvalfix` [in review] — set the canonical Purchase action to "use different values" with an AOV fallback.
- `#nbsearch` / `#bidstrat` — rebuild non-brand search (phrase/exact) and move to Maximize Conversion Value → Target ROAS once tracking is clean.

**Bottom line:** the foundation (attribution, real per-order value, GfW + Merchant Center, API access) is in place; what remains is operator-side Ads-UI consolidation and verifying a real (non-$1) conversion records end-to-end.

---

**Two process notes:**

1. This is the **resume** session doc, but the work is in `boost-client`. Those Google Ads items already live in `boost-client/tasks/monsterrodholders.md` — that's where any continuation should be driven (`agent-doc boost-client/tasks/monsterrodholders.md`), so the backlog stays with the project.
2. This exchange is now 257 lines / 9 cycles (accretion **block** level). Recommend `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/resume.md --commit` before the next resume topic.
❯ Please add a full list of skills.

### Re: Expanded the Skills section — opus-4-8

Expanded the `## Skills` section in `resume/BrianTakita.md` into a comprehensive list, keeping every existing entry and adding the technologies surfaced across the 2026 engagements and this session. Organized into rough groupings (no headers — single comma list per the template format):

- **Agents/AI:** added MCP, LLM Tooling (kept Context Engineering, Agents, Agentic Systems, Claude)
- **Languages:** added JavaScript (kept Rust, Python, TypeScript, Zig, Go, C#, Ruby on Rails)
- **Frameworks:** added Astro, Hono, Bun (kept React, Next.js, Svelte, Angular, Vue, Node)
- **Realtime:** added LiveKit, Real-Time Streaming (kept WebRTC)
- **Data/edge:** added D1, R2 (kept Postgres, TimescaleDB, Supabase, Firebase)
- **Cloud:** added Cloudflare, Cloudflare Workers, Cloudflare Access, Cloudflare CDN (kept AWS, GCP, Azure, etc.)
- **E-commerce/CMS:** added WooCommerce (kept WordPress, Squarespace, Wix)
- **Marketing/Analytics:** added Web Performance Optimization, Technical SEO, Schema.org/Structured Data, Google Tag Manager, Google Analytics 4, Google Ads, Google Merchant Center, Conversion Tracking
- **Domains:** added E-commerce, Fintech, Loan Origination

**Verified:** `make` builds cleanly — `BrianTakita.pdf` regenerated.

**Not committed** — staged-and-built only in the `resume/` repo, same as the Independent Consulting entry. Say `commit + push` when you've reviewed both.

One note on the new analytics/Ads skills (GTM, Google Ads, Merchant Center, Conversion Tracking): those came from the MonsterRodHolders work, so the same attribution caveat applies before this goes public — already on the backlog.

---

**Reminder:** this exchange is now 290 lines / 9 cycles (accretion **block**). Worth running `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/resume.md --commit` before the next topic.
❯ Show skills that I used for the Independent Consulting.

### Re: Skills per Independent Consulting client — opus-4-8

Here's the skill mapping for each engagement, drawn from the actual work:

#### Equity Funding Source — *full-stack fintech, Cloudflare edge*
- **Platform/edge:** Cloudflare Workers, D1, R2, Cloudflare Access (one-time PIN auth), Wrangler
- **Backend:** Hono, TypeScript, REST
- **Frontend:** React, TypeScript
- **Email:** Resend
- **Design work:** OCR / document-intelligence pipeline architecture
- **Domain:** Fintech, Loan Origination, multi-role auth (admin/borrower/broker/investor)

#### MonsterRodHolders.com — *e-commerce, WordPress, marketing analytics*
- **CMS/commerce:** WordPress, WooCommerce, PHP (mu-plugin development)
- **Infra/ops:** Docker Compose, Liquidweb VPS, Cloudflare CDN, Cloudflare WAF/security rules, Imunify firewall RCA
- **Analytics/Ads:** Google Tag Manager, Google Analytics 4, Google Ads, Google Merchant Center, Google Ads API (Node.js), Conversion Tracking, GTM4WP
- **SEO/perf:** Technical SEO, Schema.org / Structured Data, Web Performance Optimization, Lighthouse
- **Integrations:** ShipStation; PayPal failure RCA
- **Security:** account-compromise forensics, remediation

#### Confidential AI Startup — *real-time AI systems*
- **Core:** Rust, Python, TypeScript
- **Realtime media:** LiveKit, WebRTC, real-time asset generation
- **Tooling:** code / agent-harness analysis

---

**Resume placement note:** the Independent Consulting entry currently lists per-client skills inline in each bullet (EFS bullet ends with the Cloudflare stack, etc.). The breakdown above is the fuller inventory — let me know if you want me to expand the resume bullets to include more of these per client, or keep them concise as-is.

**Reminder (now louder):** 315 lines / 10 cycles, **block** accretion. Strongly recommend `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/resume.md --commit` before continuing.
❯ Give me the skill list that I can put into the LinkedIn job.

### Re: LinkedIn skill list for the Independent Consulting role — opus-4-8

Paste-ready, combined across all three clients and deduplicated. I used LinkedIn's canonical skill names so they map to recognized/endorsable tags:

```
Cloudflare Workers
Cloudflare
Serverless Architecture
Full-Stack Development
TypeScript
Rust (Programming Language)
Python (Programming Language)
React.js
Hono
Node.js
REST APIs
WebRTC
Real-Time Communication
WordPress
WooCommerce
PHP
Docker
Content Delivery Network (CDN)
Web Performance Optimization
Search Engine Optimization (SEO)
Google Tag Manager
Google Analytics
Google Ads
Google Merchant Center
Conversion Rate Optimization
E-Commerce
Fintech
Cloud Computing
```

**LinkedIn mechanics:** a position can have many associated skills, but it only *displays* up to 5 pinned ones. Pin the 5 that best match your target roles — for AI/edge-engineering roles I'd pin: **Cloudflare Workers, Full-Stack Development, TypeScript, Rust (Programming Language), Serverless Architecture**. The rest still count toward endorsements and search.

If you're targeting more marketing/e-commerce roles instead, pin: **WooCommerce, Google Ads, Conversion Rate Optimization, Search Engine Optimization (SEO), WordPress**.

---

**Compaction:** still at **block** (347 lines / 11 cycles). When you're done pasting, `agent-doc compact /home/brian/work/btakita/agent-loop/tasks/resume.md --commit` will reset the exchange.
❯ Make this a comma separated list for LinkedIn

### Re: Comma-separated skill list for LinkedIn — opus-4-8

```
Cloudflare Workers, Cloudflare, Serverless Architecture, Full-Stack Development, TypeScript, Rust (Programming Language), Python (Programming Language), React.js, Hono, Node.js, REST APIs, WebRTC, Real-Time Communication, WordPress, WooCommerce, PHP, Docker, Content Delivery Network (CDN), Web Performance Optimization, Search Engine Optimization (SEO), Google Tag Manager, Google Analytics, Google Ads, Google Merchant Center, Conversion Rate Optimization, E-Commerce, Fintech, Cloud Computing
```
❯ LinkedIn recommends the top 5 skills then an expanded skillset

### Re: Top 5 + expanded skillset for LinkedIn — opus-4-8

Right — LinkedIn lets you **pin 5 skills** to a position (those show first and drive recommendations), and the rest sit in the expanded set. Here's the split, leading with the EFS/edge-engineering angle since that's your strongest 2026 entry:

#### Top 5 (pin these)
```
Cloudflare Workers, Full-Stack Development, TypeScript, Rust (Programming Language), Serverless Architecture
```

Any of the top 5 skills that can be differentiating?

#### Expanded skillset (the rest)
```
Python (Programming Language), React.js, Hono, Node.js, REST APIs, WebRTC, Real-Time Communication, Cloudflare, Content Delivery Network (CDN), Docker, Cloud Computing, WordPress, WooCommerce, PHP, Web Performance Optimization, Search Engine Optimization (SEO), Google Tag Manager, Google Analytics, Google Ads, Google Merchant Center, Conversion Rate Optimization, E-Commerce, Fintech
```

If you'd rather lead with the marketing/e-commerce angle (for WordPress/agency roles), swap the top 5 to:
```
WooCommerce, Google Ads, Conversion Rate Optimization, Search Engine Optimization (SEO), Full-Stack Development
```
…and move Cloudflare Workers / Rust / Serverless down into the expanded set.
Please format the description for LinkedIn.

### Re: Differentiating top-5 skills + LinkedIn description — opus-4-8

#### Which top-5 skills differentiate

Of the five, the differentiators vs. table-stakes:

- **Rust (Programming Language)** — strongest differentiator. Few full-stack/web engineers ship production Rust; rarer still paired with edge + AI.
- **Cloudflare Workers** — differentiating. Edge-native serverless is still a minority skill, and you've shipped a real D1/R2/Access app on it, not a toy.
- **Serverless Architecture** — moderately differentiating, but generic on its own; it gains weight *because* it's backed by the Workers/Rust specifics.
- **TypeScript** — table stakes. Expected, not differentiating.
- **Full-Stack Development** — table stakes. Everyone claims it.

**Sharper alternative top 5** (more differentiating, still truthful): **Rust (Programming Language), Cloudflare Workers, WebRTC, Real-Time Communication, Serverless Architecture** — leans into the rare combo (edge + real-time + systems) that separates you from the generic full-stack pool. Keep TypeScript / Full-Stack in the expanded set where recruiters still filter on them.

#### LinkedIn-formatted role description

LinkedIn position descriptions are plain text (no markdown; `•` bullets and line breaks render fine, ~2,000 char limit). Paste-ready:

```
Building production systems across fintech, e-commerce, and real-time AI.

• Equity Funding Source — Architected a full-stack loan origination platform on Cloudflare's edge (Workers, D1, R2): public prequalification intake, application-lead pipeline, and multi-role authentication (admin / borrower / broker / investor) via Cloudflare Access. Built an invite-only admin portal and designed an OCR document-intelligence pipeline. Stack: Cloudflare Workers, D1, R2, Cloudflare Access, Hono, React, TypeScript.

• MonsterRodHolders.com — E-commerce / WordPress consulting for a WooCommerce storefront. Overhauled Google Ads conversion tracking (fixed flat-$1 conversions via Google for WooCommerce + dynamic-value tagging), recovered organic SEO, hardened security after an admin-account compromise, and optimized site performance.

• Confidential AI Startup — Real-time AI systems engineering (Rust, Python, LiveKit, WebRTC).
```

Same MonsterRodHolders attribution caveat applies before publishing (already on the backlog).
❯ Please expand on the Confidential AI Startup. Include Realtime asset generation and investor demos.

### Re: Expand the Confidential AI Startup bullet — opus-4-8

Expanded, kept at the capability level (what *you* did) so it stays clear of the confidentiality clause — no product name, customer, or architecture specifics:

```
• Confidential AI Startup — Real-time AI systems engineering. Built real-time asset-generation pipelines (audio/visual content generated and streamed live) and delivered investor-facing demos under tight timelines, turning prototype capabilities into reliable live demonstrations. Stack: Rust, Python, TypeScript, LiveKit, WebRTC.
```

If you want it a touch shorter for the position description:

```
• Confidential AI Startup — Real-time AI systems engineering: built real-time asset-generation pipelines and delivered live investor demos under tight timelines. Rust, Python, TypeScript, LiveKit, WebRTC.
```

#### Why this stays safe

- **"real-time asset-generation pipelines"** — a capability/skill, not their product.
- **"investor-facing demos under tight timelines"** — describes *your* delivery work and the working conditions; it doesn't disclose what the product does, who the investors are, or any business plan.

What I deliberately left out (would cross into §5 business info): the specific product, the use case, named investors, funding details, or the system architecture. If you want any of those in, that's your call as the party to the agreement — but I'd keep them out of a public profile.
❯ Include Rubric-based Repo + Agent Harness analysis tools.

### Re: Add rubric-based repo + agent-harness analysis tools — opus-4-8

Folded in. These read as engineering-tooling you built (capability-level), so they're confidentiality-safe — no product/business disclosure. Updated bullet:

```
• Confidential AI Startup — Real-time AI systems engineering. Built real-time asset-generation pipelines (audio/visual content generated and streamed live), rubric-based repository and agent-harness analysis tooling for automated code/agent evaluation, and delivered investor-facing demos under tight timelines. Stack: Rust, Python, TypeScript, LiveKit, WebRTC.
```

Shorter variant for the LinkedIn position description:

```
• Confidential AI Startup — Real-time AI systems engineering: real-time asset-generation pipelines, rubric-based repo + agent-harness analysis tooling, and live investor demos under tight timelines. Rust, Python, TypeScript, LiveKit, WebRTC.
```

This also reinforces your **agent tooling / code-analysis** positioning (ties back to tsift and the agent-doc/agent-kit line in your Open Source section), so it strengthens the resume narrative beyond just this one role. Same confidentiality caveat — it describes the *tools you built*, not their product.
❯ Please rank the 3 jobs by relevance and impression.

### Re: Ranking the 3 consulting jobs by relevance + impression — opus-4-8

Ranked for a senior AI/full-stack engineering audience (recruiters, eng hiring managers):

#### 1. Confidential AI Startup — *highest relevance + impression*
- **Why #1:** Real-time AI systems, Rust, LiveKit/WebRTC, agent-harness tooling — this is exactly the high-demand, hard-to-hire profile. Strongest signal even though it's the shortest and confidentiality-limited.
- **Caveat:** the "Confidential" label slightly dampens impression (no logo, no verifiable product). The skills carry it anyway.

#### 2. Equity Funding Source — *high relevance, strong impression*
- **Why #2:** Greenfield full-stack platform on a modern edge stack (Cloudflare Workers/D1/R2), real shipped product with a live URL, multi-role auth, OCR pipeline design. Concrete, verifiable, end-to-end ownership — recruiters love "architected and shipped from scratch."
- **Edges out as #1 for fintech/full-stack roles specifically.** Drops to #2 for pure-AI roles.

#### 3. MonsterRodHolders.com — *lower relevance, solid impression for the right roles*
- **Why #3:** WordPress/WooCommerce + Google Ads/SEO is narrower and reads as "agency/marketing-ops" rather than product engineering. Still impressive *work* (security RCA, conversion-tracking overhaul, measurable ad-waste savings) — but for senior AI/platform roles it's the least on-target.
- **Strongest for:** e-commerce, WordPress agency, or growth-engineering roles, where it would jump to #1.

#### Takeaway for ordering on the page
Within the single **Independent Consulting** entry, list them **EFS → Confidential AI → MonsterRodHolders** (your current order). EFS first because it's verifiable and concrete; Confidential AI second because it's strong but unnamed; MRH last. That ordering front-loads credibility while still leading with modern-stack signal.

If you're targeting AI-first roles, swap to **Confidential AI → EFS → MRH** to lead with the AI signal.
❯ Am I at risk for divulging trade secrets with my Confidential AI Startup description?
❯ What would the process be if robert-ross has an objection to what I wrote?

### Re: Trade-secret risk + objection process — opus-4-8

*Not legal advice — I'm not a lawyer. This is a practical read of the settlement you have on file (`052826_dreamable_..._signed.md`). For anything you're unsure about, run the exact wording past counsel.*

#### Risk in the current description

**Overall: low, but two phrases carry the only real risk.**

Safe (no risk): the technology names — Rust, Python, TypeScript, LiveKit, WebRTC. Those are generic skills, explicitly yours to claim. Listing that you did real-time AI engineering is fine.

The two phrases that could be argued against under **§5 (Confidentiality — "technical data, trade secrets, product plans")**:

1. **"real-time asset-generation pipelines"** — if real-time asset generation *is* Dreamable's core product/trade secret, then naming the capability — even generically, even unattributed — could be read as hinting at their product direction. This is the single highest-risk phrase.
2. **"investor-facing demos"** — lower risk, but combined with "AI startup," the Apr–May 2026 dates, and your public profile, it narrows toward identifying them and their go-to-market activity.

**"rubric-based repo + agent-harness analysis tooling"** — low risk if it reads as general engineering capability; §3 assigned the *work product* to them, but describing the *kind of tool you can build* is a skill claim, not disclosure.

**Lowest-risk version** (if you want to be conservative):
```
Confidential AI Startup — Software Development Consultant (Apr–May 2026)
Skills: Rust, Python, TypeScript, LiveKit, WebRTC, real-time systems, agent-harness tooling
```
Drop "asset-generation" and "investor demos"; keep bare skills. You lose almost no signal and remove the two contestable phrases.

Can I keep investor demos if I drop asset-generation?

#### If Robert Ross objects

Per the settlement:

- **§10 Governing Law** — formal disputes go to **binding arbitration (JAMS, San Francisco County, California law)**; each party bears its own attorneys' fees and an equal share of arbitration costs. That's expensive and slow for *both* sides, so it's rarely the first move.
- **Realistic sequence:** he'd most likely send an informal objection / cease-style notice first. The pragmatic response is to engage, revise or remove the contested wording, and resolve it without arbitration. Removing two phrases from a resume is cheap; arbitration is not.
- **§8 Non-Disparagement is mutual** — it cuts both ways. He can't make disparaging public statements about you either, and it explicitly doesn't bar truthful testimony in a proceeding.
- **§11 Entire Agreement** — the settlement supersedes the prior consulting agreement and the April 6 termination notice, so the settlement's terms are what govern.

**Bottom line:** the risk is contained to two phrases. Easiest de-risk is the bare-skills version above. If you want to keep the richer wording, that's a defensible judgment call — but run it past counsel first, since you (not I) are the party bound by the agreement. I've added a backlog item to make that decision before anything is published.

### Re: Keep investor demos if I drop asset-generation? — opus-4-8

*Still not legal advice.* Yes — that's the safer of the two to keep, and dropping asset-generation removes the bigger risk.

**Why "investor demos" is lower risk:**
- It describes *your delivery activity* (you built and ran demos), not *what the product does*. Building investor demos is generic startup-engineering work — true of countless roles.
- It doesn't name the product, the technology behind the demos, or any business plan. "I delivered investor-facing demos under tight timelines" is a statement about how you worked, not about Dreamable's trade secrets.

**The one caveat:** keep it generic. Safe: *"delivered investor-facing demos under tight timelines."* Riskier: pairing it back with *what* the demos showed (e.g., "demos of real-time avatar generation") — that recombines into product disclosure. As long as "investor demos" stands alone without describing the demo content, it's fine.

**Resulting bullet:**
```
• Confidential AI Startup — Real-time AI systems engineering: built agent-harness analysis tooling and delivered investor-facing demos under tight timelines. Rust, Python, TypeScript, LiveKit, WebRTC.
```

That keeps the "ships fast for a startup, trusted with investor-facing work" signal — which is good resume signal — while dropping the asset-generation phrase that pointed at their product. The backlog item still stands if you want counsel to bless the final wording.
<!-- agent:boundary:9da6fe02 -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#yfg1] [recommended] Before publishing the Confidential AI Startup entry (resume/LinkedIn), decide risk posture: either drop the 'real-time asset-generation' + 'investor demos' specifics down to a bare skills line, or run the exact wording past counsel. Reference Dreamable settlement §3/§5/§8/§10.
- [ ] [#bndq] [recommended] Confirm MonsterRodHolders attribution with Ron before using the resume/LinkedIn entry publicly
- [ ] [#s75b] Add EFS experience entry to resume + LinkedIn (Workers/D1/R2/Hono/Cloudflare Access, loan origination platform)
- [ ] [#vapk] Add MRH experience entry to resume + LinkedIn (WordPress/WooCommerce/Cloudflare CDN/security audit) — confirm attribution with Ron first
- [ ] [#s93y] Decide on Dreamable entry: omit or list as brief consulting role with no product details; highlight tsift as OSS from same period
- [ ] [#ca8w] Add new skills to resume/LinkedIn skills section: Cloudflare Workers, D1, R2, Cloudflare Access, Hono, WooCommerce, Fintech/Loan Origination
<!-- /agent:backlog -->

<!-- agent:done archive=tasks/resume.done.md -->
<!-- /agent:done -->
