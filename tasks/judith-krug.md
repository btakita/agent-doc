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

Robert Ward is going to reach out to Judith. We may have a conversation. Robert told me that Judith gets busy at times. She is deciding on what platform to go with. Her ex-husband & business partner recommended hubspot crm. It has a great price at $9/mo/seat. MA has Hubspot integration.

## Assistant

**She hasn't decided yet** — "Judith gets busy at times" from Robert corrects my earlier read. Silence ≠ rejection. She's still evaluating.

**HubSpot CRM at $9/mo/seat is compelling** — cheapest option by far. But HubSpot alone doesn't do mortgage-specific workflows (loan origination, RE 885 disclosures, fractionalized notes). It's a CRM, not a LOS.

**The landscape now:**

| Option | Cost | What it does | What it doesn't |
|---|---|---|---|
| HubSpot CRM | $9/mo/seat | Contacts, pipeline, follow-ups, email | Mortgage compliance, doc gen, servicing, investor pooling |
| TMO | ~$600/mo | Loan origination, doc gen, compliance | Modern UI, AI, investor outreach |
| MA | ~$1,137/mo | Full LOS + HubSpot integration + servicing | Price (she doesn't want servicing anyway) |

**HubSpot + TMO could be her sweet spot** — CRM for investor relationships ($9/mo) + TMO for loan mechanics ($600/mo). MA's main advantage over TMO was the HubSpot integration, but if she runs HubSpot independently, TMO gets cheaper.

**Your angle sharpens:** A platform-agnostic intelligence layer that bridges HubSpot (investor pipeline) ↔ TMO or her bespoke system (loan operations). Deal sourcing feeds into HubSpot contacts. Investor pitch decks pull from loan data. This is exactly the gap between CRM and LOS that neither vendor fills.

**For the conversation with Robert:** Position yourself as the person who can connect these tools. "Judith's choosing between platforms — I can build the glue between whatever she picks and her investor outreach." That's something TMO and MA don't offer.

## User

Judith is going with the Mortgage Automator. What are some next steps?

## Assistant

**Judith chose MA.** Platform decision made — now the question is where you add value on top.

**What MA covers:** Full LOS, HubSpot integration, portals, doc gen, RE 855, notifications. She's well-served on loan management.

**Your opportunities (what MA doesn't do):**

1. **Deal sourcing automation** — Agent loops scanning county records, NODs, probate, expired listings for properties matching her parameters (LTV caps, residential $75K+, commercial $100K+). No LOS does this.
2. **Investor matching & pitch decks** — Auto-generated decks per investor risk profile. Pool orchestration for fractionalized notes (up to 10 per loan). MA has portals but not intelligent outreach.
3. **Website modernization** — equityfundingsource.com is dated. Modern site with anonymized deal listings, intake forms, analytics funnels.
4. **Data migration** — Bespoke system → MA. One-time project, clear scope, immediate need.
5. **MA API customization** — Custom workflows or integrations beyond MA's defaults.

**Recommended next steps:**

1. **Congratulate the choice** — "MA is solid. Smart move."
2. **Ask about onboarding timeline** — When does MA go live? Data migration needs?
3. **Pitch deal sourcing** — "Now that loan management is covered, the bottleneck becomes deal flow."
4. **Start small** — Website refresh or a migration assist to build trust first.

## User

What are some improvements that we can make to http://www.equityfundingsource.com/
How is the seo and the llm integration?

## Assistant

**Site audit of equityfundingsource.com:**

Stack: WordPress (Twenty Twenty-Four theme) + Elementor Pro + WPForms + Bookly + Infusionsoft CRM. Google Forms for loan applications.

**SEO scorecard:**

| Area | Grade | Notes |
|---|---|---|
| Schema.org | B+ | Organization, WebPage, BreadcrumbList present |
| Meta descriptions | B | Basic coverage, not keyword-optimized |
| Heading hierarchy | B | h1/h2/h3 used properly |
| FAQ schema | F | Missing — huge opportunity |
| Sitemap + robots.txt | F | Not referenced at all |
| Page speed | C- | Bloated (Elementor + Twenty Twenty-Four CSS, multiple JS libraries) |
| Internal linking | D | Minimal cross-page links |
| Blog utilization | D | Exists ("Private Lending Blog") but underutilized |
| Local SEO | D | DRE/NMLS shown but no Google Business Profile optimization |

**LLM discoverability:**

| Signal | Status |
|---|---|
| Structured business data | Good — name, address, phone, email, licenses in schema |
| Service descriptions | Moderate — general, not granular |
| FAQ content | Missing — LLMs pull heavily from FAQ sections |
| llms.txt | Missing — no machine-readable site summary |
| Funded deals as text | Missing — details are in images, not indexable text |

**Improvements by priority:**

**Quick wins (hours):**
1. **robots.txt + XML sitemap** — basic SEO hygiene, currently missing entirely
2. **FAQ page with schema** — "What is a hard money loan?", "What LTV ratios do you offer?", "What are your rates?", "How fast can you close?" — massive for both Google snippets and LLM citation
3. **llms.txt at /llms.txt** — emerging standard for LLM discoverability. Plain text summary of business, services, rates, service area
4. **Text-ify funded deals** — add structured text (location, amount, type, LTV, close time) alongside images so search engines and LLMs can index them

**Medium effort (days):**
5. **Page speed** — Elementor + Twenty Twenty-Four is heavy. Defer unused JS (Bookly loads on every page), purge unused CSS, or migrate to a lighter theme
6. **Replace Google Forms** — loan application links to Google Forms. Embed WPForms (already installed) or build a proper intake flow. Keeps users on-site.
7. **Testimonials** — none visible. Add investor and borrower testimonials with Review schema
8. **Blog content strategy** — target long-tail: "hard money loans Los Angeles", "private money lender California", "bridge loan vs hard money", "how to invest in trust deeds"
9. **Investor portal** — protected area for qualified investors to see deal details, RE 885 docs, opt in

**Larger projects (weeks):**
10. **Full redesign** — modern landing pages, loan calculator, application tracking, MA integration for live deal status
11. **Local SEO campaign** — Google Business Profile, review generation, local citations
12. **Content hub** — market reports, rate comparisons, borrower/investor guides (feeds both SEO and LLM training data)

**Biggest ROI for your pitch to Judith:** Items 1-4 are quick, tangible, and demonstrate immediate value. The FAQ + llms.txt combo is particularly compelling — "your competitors aren't doing this yet, and it's how AI assistants will recommend lenders."

## User

If Judith is going to have ownership...what technology would be the best for her hosting?

Wordpress? Astro? Something else?

I can use relysjs, but it's my own library. I wonder if there is a well designed efficient library that I can use.

## Assistant

**Key constraint: Judith needs to update content herself** — funded deals, blog posts, rates. She's not a developer.

She doesn't need to update the content herself. But I promised that she will have custody of all of her services.

| Stack | She can self-maintain? | Performance | Your DX |
|---|---|---|---|
| **WordPress (keep, optimize)** | Yes — familiar admin | Better with caching/lighter theme | Painful (Elementor) |
| **Astro + headless CMS** | Yes — via CMS admin panel | Excellent (static) | Excellent |
| **Astro + markdown only** | No — needs you | Excellent | Excellent |

**Recommendation: Astro + Keystatic**

Astro is the modern standard for content-heavy marketing sites — pre-renders to static HTML, island architecture for interactive components only where needed. Keystatic (by Thinkmill) gives Judith a clean web UI for editing content that commits to git. Free, first-class Astro integration.

**Other CMS options if Keystatic doesn't fit:**
- **Decap CMS** (formerly Netlify CMS) — free, git-based, proven
- **Tina CMS** — visual editing, git-backed
- **WordPress as headless** — keep her existing WP admin, fetch via REST API into Astro frontend. She keeps her familiar UI, you get a modern frontend.

**Re: relysjs alternatives —** For this site, Astro's built-in features cover most needs. For interactive islands (loan calculator, intake form, investor portal), Astro supports any framework. **Preact** or **Solid** would be the efficient choices — tiny bundles, fast hydration, just for the few interactive widgets.

**Hosting:** Netlify or Cloudflare Pages (free tier handles this traffic level easily). ~$0/mo.

**The practical path:**
1. Astro + Keystatic + Preact (for interactive islands)
2. Deploy on Cloudflare Pages or Netlify
3. Migrate existing content (5 pages + blog + funded deals)
4. Add FAQ, llms.txt, sitemap as part of migration
5. Judith edits content via Keystatic admin UI → auto-deploys

## User

Does Keystatic mean editing markdown files directly?

## Assistant

No — Keystatic provides a **web-based admin UI** (form fields, rich text editor) that stores data as markdown/JSON/YAML in git. Judith would see a browser-based editor, not raw markdown. Under the hood it commits to git, so she owns everything with full history.

**But given your correction** — she doesn't need self-service editing, just custody — **you can skip the CMS entirely.**

A CMS doesn't hurt and may be beneficial. So I"ll keep it in mind.

**Revised stack: Astro + markdown + Preact islands**
- Content in markdown files (blog, FAQ, funded deals)
- Astro renders static HTML
- Preact for interactive islands (calculator, forms)
- Deploy on Cloudflare Pages (she owns the account)
- She owns the GitHub repo
- Any developer can pick it up — no vendor lock, no database

If she ever wants self-service editing later, Keystatic or Decap layers on top of the existing markdown files without changing the codebase.

I'll use relementjs + rmemo for the components.

**Custody checklist:**
- Git repo in her GitHub/GitLab account (you're a collaborator)
- Hosting account in her name (Cloudflare Pages, Netlify, or Vercel)
- Domain DNS she controls
- All credentials documented and handed over

## User



## Assistant

Noted both corrections:
- **CMS**: Keystatic stays on the radar — easy to add later since it layers on top of markdown content collections
- **Components**: relementjs + rmemo instead of Preact for interactive islands

**Updated stack: Astro + markdown + relementjs/rmemo + optional Keystatic**

## User

What is a good host for Astro? Judith is in LA

## Assistant

**Cloudflare Pages** — best fit.

| Host | Free tier | Why / Why not |
|---|---|---|
| **Cloudflare Pages** | Unlimited bandwidth, 500 builds/mo | Fastest edge (LAX PoP), free SSL + custom domain, Judith owns the account. Workers available if you need server-side later (auth, API proxy). |
| **Netlify** | 100GB bandwidth, 300 build min/mo | Good DX, built-in form handling. Close second. |
| **Vercel** | 100GB bandwidth | Works but optimized for Next.js, not Astro's focus |
| **GitHub Pages** | Unlimited (public repos) | Simplest, but no build pipeline or edge functions |

LA location doesn't matter for static sites — all major CDNs have LA PoPs. A mortgage brokerage site won't come close to any free tier limits.

**If you ever need SSR** (investor portal, MA API bridge): Cloudflare Workers on the same account. No migration needed.

## User

Judith has somewhat technically skilled employees. Not exactly programmers but more administrator level...with limited if any git skills.

Cloudflare Pages could serve the website and do email routing.

I will recommend Claude Code as well for her staff.
Would the CRM workflow well with Claude Code + Cloudflare Pages? Can we make this somewhat error proof given that we are using git?

Perhaps a cli tool for the staff that works with Claude Code?

## Assistant

**Yes — Claude Code becomes the git interface.** Staff never touch git directly.

**Workflow:** Staff talks to Claude Code → Claude commits to a branch → pushes → Cloudflare Pages auto-deploys (or preview-deploys for review).

**Error-proofing layers:**
1. **Protected `main`** — no direct pushes. All changes go through PRs.
2. **Claude Code creates branches** — "add a funded deal for $400K in Pasadena" → Claude makes the branch, adds markdown, opens PR.
3. **Cloudflare preview deploys** — every PR gets a preview URL. Staff checks before going live.
4. **CI validation** — GitHub Actions checks required fields, valid rates, proper images before merge.
5. **Auto-merge or your review** — simple content auto-merges after CI. Structural changes need you.
6. **Git revert** — any mistake is undone in seconds.

**CLI tool (`efs`):**
```bash
efs deal add          # guided prompts for funded deal
efs blog draft        # create blog post draft
efs blog publish      # move draft → published
efs site preview      # open preview URL
efs site deploy       # merge PR → auto-deploy
```

Abstracts git completely. Validates content. Works alongside Claude Code — staff can use either. Claude Code is the "smart" path, CLI is the structured path.

**Cloudflare email routing** — `contact@equityfundingsource.com` → Judith's Gmail. Free. No separate email hosting.

**CRM fit:** Staff uses Claude Code to draft investor outreach, pulling deal data from the markdown content. MA handles loans. Claude helps with everything around MA — website, investor materials, follow-ups. If they use HubSpot, its API is accessible to Claude Code too.

**Pitch to Judith:** "Your staff talks to Claude in plain English. Claude updates your website, drafts investor materials, manages content. Everything is version-controlled — nothing breaks permanently, you own all of it."

## User

If I use Cloudflare pages with Hono, are there any advantages to using Astro?
