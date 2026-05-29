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
<!-- agent:boundary:6e9e331e -->
<!-- /agent:exchange -->
###
<!--
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#s75b] Add EFS experience entry to resume + LinkedIn (Workers/D1/R2/Hono/Cloudflare Access, loan origination platform)
- [ ] [#vapk] Add MRH experience entry to resume + LinkedIn (WordPress/WooCommerce/Cloudflare CDN/security audit) — confirm attribution with Ron first
- [ ] [#s93y] Decide on Dreamable entry: omit or list as brief consulting role with no product details; highlight tsift as OSS from same period
- [ ] [#ca8w] Add new skills to resume/LinkedIn skills section: Cloudflare Workers, D1, R2, Cloudflare Access, Hono, WooCommerce, Fintech/Loan Origination
<!-- /agent:backlog -->

<!-- agent:done archive=tasks/resume.done.md -->
<!-- /agent:done -->
