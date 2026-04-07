---
agent_doc_session: aace927e-3f52-4e08-8677-ae851d9d9825
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Give me a 1 page pdf of research for filevine.com for an interview.

Help me prepare for an interview with the Filevine development team.
- https://www.filevine.com/
- https://www.linkedin.com/company/filevine
- Chris Smith
- Cain Elliott
- Stuart Hiller
- Emily Lippincott
Help me prepare for an interview with the Filevine development team.
- https://www.filevine.com/
- https://www.linkedin.com/company/filevine
- Chris Smith
- Cain Elliott
- Stuart Hiller
- Emily Lippincott

### Re: Filevine Interview Prep

---

## Company Overview

**Filevine** (founded 2014, ~500 employees, HQ Salt Lake City UT) is a **legal work platform** — case management, document automation, contract lifecycle, billing, and AI — for law firms, government agencies, and in-house counsel. Scale: 5,000+ clients, 100,000 active users, 1.8B documents processed. Offices in Chicago, New York, Prague, Bratislava.

**Recent milestones:**
- Raised **$400M all-equity round** (Sept 2025)
- Achieved **FedRAMP 20x Authorization** (Oct 2025)
- Acquired **Pincites** (AI contract redlining for Word, Jan 2026)
- Launched **LOIS** (Legal Operating Intelligence System) at LEX Summit 2025

---

## Flagship Product: LOIS

LOIS is the core AI narrative right now — know it cold.

- **Built into the system of record**, not bolted on — unifies case management, docs, comms, billing, compliance, analytics, and AI
- **Intent-aware routing**: routes each query (summarize depositions, extract facts, calculate deadlines) to the best capability
- **Zero-retention AI**: no data retained by AI providers — enterprise/HIPAA/FedRAMP grade
- **Verifiable outputs**: every AI answer has source citations + excerpted evidence for attorney audit

Key tools under LOIS: **AskAI** (chat with a case file), **DraftAI** (document drafting), **AIFields** (auto-populate structured data), **Timely** (jurisdiction-specific deadline automation), **MedCron** (HIPAA-compliant medical chronology).

---

## Tech Stack

From job postings and engineering signals:

| Layer | Technologies |
|-------|-------------|
| Backend | Python, Node.js, C#/.NET |
| Frontend | React, TypeScript, Svelte |
| Mobile | React Native, Swift, Kotlin |
| Infra | AWS, Kubernetes |
| Orchestration | Temporal |
| Data/Search | MS SQL, Redis, Elasticsearch |
| Testing | Playwright, Groovy |
| Architecture | Microservices, SOC 2 / HIPAA / FedRAMP |

---

## Your Interviewers

**Chris Smith** — Principal Software Engineer
Engineering track. Expect systems design, Python/React/AWS, architecture tradeoffs. Senior IC role, so probably asking about technical depth and ownership.

**Stuart Hiller** — Full Stack Developer
Engineering track. Background: Node/Express/React/MongoDB/GraphQL. Previous work at Strategic Financial Solutions and LemonBrew. Likely assessing full-stack execution and code quality.

**Cain Elliott** — Chief Legal Futurist (PhD Philosophy, Polish Academy of Sciences)
*Not an engineer* — product/strategy/AI vision side. Former philosophy professor (UPenn, Warsaw), former CIO at Margolis Edelstein. Led the "Legal Futurists" team, spearheaded ImmigrationAI. Expect conversation about AI in legal, product philosophy, and "what does intelligent legal software look like?" Knows legal workflows deeply.

**Emily Lippincott** — Senior Legal Futurist
*Not an engineer* — JD attorney (Creighton Law 2005), 16+ years Silicon Valley, Monica Bay Women of Legal Tech award winner. Joined as Sales Engineer 2022, now Senior Legal Futurist. ABA Techshow speaker. Expect customer-centric product conversations and how tech maps to attorney workflows.

---

## Key Talking Points

1. **AI grounded in truth**: LOIS's design principle — verifiable outputs with citations. This matters in legal (malpractice risk from hallucination). Filevine differentiates on trustworthy AI.
2. **System of record first**: AI is only as good as the data it touches. Filevine owns the data layer, which is a competitive moat.
3. **Compliance as a feature**: FedRAMP, HIPAA, SOC 2 aren't afterthoughts — they're part of the product story for government/enterprise customers.
4. **Legal workflows ≠ generic SaaS**: Deadlines are jurisdictional, documents are privileged, billing is regulated. The domain complexity is why vertical SaaS wins here.

---

## Questions to Ask

- *What does a sprint cycle look like for LOIS features — how close is engineering to the legal domain experts?*
- *How does Filevine handle schema evolution for 1.8B documents across 5,000+ firm configurations?*
- *With FedRAMP authorization just landed, how is that shaping the product roadmap?*
- *What's the boundary between the Pincites acquisition and Filevine's existing DraftAI?*
- *How does the Legal Futurists team (Cain/Emily) interface with engineering on LOIS?*

---

*Sources: filevine.com, LinkedIn, LEX Summit 2025 announcements, Crunchbase, ZoomInfo, job postings (April 2026)*

How receptive would filevine be to agent-doc?
How receptive would filevine be to agent-doc?

### Re: Filevine receptivity to agent-doc (HEAD)

Short answer: **low as a product fit, moderate as a conversation piece, high with the right people.**

**Why low as a product fit:**
Filevine is building LOIS — an AI-native legal platform for attorneys, not developers. Their customers use a web app, not a CLI. agent-doc's markdown/tmux/editor workflow is a dev tool with no natural path to a law firm's desktop. They're also deep into their own AI platform investment ($400M raised) — they're building, not buying dev tooling externally.

**Why moderate as a conversation piece:**
The *concept* behind agent-doc — document as the UI, AI responding to edits in a living document, preserving context across sessions — maps interestingly onto legal work. Legal documents are living artifacts: they accumulate annotations, revisions, cited sources. Filevine's DraftAI and AIFields are solving adjacent problems (AI-assisted drafting, structured extraction from case files).

Cain Elliott in particular (PhD philosopher, "Chief Legal Futurist") would likely find the session/context model intellectually interesting — he thinks about the future of legal workflows, not just current UI patterns.

**Why high with the right people:**
Chris Smith and Stuart Hiller are developers. If you frame agent-doc as "how I work with AI on complex, multi-session engineering tasks" — using it to demonstrate your own process — that's a credibility signal, not a pitch. Engineers who see you dog-fooding a tool you built are more interested than if you sell it abstractly.

**The interview angle:**
Don't pitch agent-doc to Filevine. Use it as evidence of how you think about AI tooling, then ask: *"How does your engineering team actually work with AI day-to-day — in the codebase, in PRs, in design docs?"* That reframes the conversation toward their workflow and signals you're someone who builds for real-world AI integration, not just demos.
<!-- agent:boundary:d258e9fe -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
