---
agent_doc_session: 64316602-b77c-48ce-b990-92fc72425f26
agent_doc_format: template
agent_doc_write: crdt
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Royce Stewart / Greenlight Guru — sonnet-4-6

**Corky contact created:** `mail/contacts/royce-stewart/AGENTS.md`

**Royce Stewart**
- Role: Talent Acquisition Manager at Greenlight Guru (Chicago, IL)
- Credentials: SHRM-SCP (Senior Certified Professional in HR)
- LinkedIn: https://www.linkedin.com/in/stewartroyce/
- Prior employers: Stats Perform, MAT Holdings Inc., CultureFit
- Education: Evangel University

---

**Greenlight Guru** — Medical Device QMS SaaS

Greenlight Guru is the leading eQMS (electronic Quality Management System) platform built specifically for medical device companies. Founded 2013, HQ in Indianapolis, IN.

**Products & Services:**
- **Quality Management Suite** — document control, CAPA, nonconformance, audit management, supplier oversight, training
- **Product Development Module** — design controls, requirements management, software release lifecycle, AI-powered traceability
- **Clinical Evidence Platform** — electronic data capture (EDC) for clinical trials, post-market follow-up (PMCF), ePRO/eCOA
- **Implementation Services** — consulting + 80+ audit-tested SOP templates

**Target Market:** Medical device companies of all sizes (startups through enterprise), CROs, pre- and post-market compliance teams. 1,100+ customers actively use the platform.

**Business Model:** Subscription SaaS (tiered by module) + professional services.

**Company Scale:**
- ~146 employees across 5 continents
- $121M+ total funding raised
- Acquired by Enzyme in July 2025 (LBO/buyout)
- Positioned as #1 QMS purpose-built for medtech; built around FDA and ISO compliance

### Re: Contributing value to Greenlight Guru — sonnet-4-6

> **Queue prompt:** Tell me more about what is needed to contribute value to this company.

Greenlight Guru's value proposition sits at the intersection of **regulated-industry domain knowledge** and **modern SaaS product execution**. The areas where contributors create the most leverage:

**Regulatory / Domain expertise**
- Deep familiarity with FDA 21 CFR Part 820 (Quality System Regulation) and ISO 13485 (medical device QMS standard) — these are the compliance rails their entire product is built on
- Understanding of design controls, CAPA workflows, risk management (ISO 14971), and audit-readiness in medtech
- Post-market surveillance and clinical follow-up (PMCF) knowledge supports their Clinical Evidence Platform

**Engineering / Product**
- SaaS platform engineering: the product is document-control, workflow, and audit-trail heavy — think compliance-grade data integrity, versioning, and traceability at scale
- AI/ML: they're actively investing in AI-powered traceability and analytics — connecting requirements → design → verification across the device lifecycle
- Integration work: medical device companies use PLM (Windchill, Teamcenter), ERP, and lab systems — interop matters
- Full-stack or backend engineers comfortable with high-compliance, audit-log-everything environments

**Go-to-Market / Customer Success**
- Selling/implementing into medtech (regulatory affairs, quality, R&D teams) requires knowing their vocabulary — QMS buyers are skeptical of generic software
- Implementation consultants who can map existing SOP libraries into the platform and run validation (IQ/OQ/PQ) for FDA-regulated customers
- Customer success that understands device lifecycle stages (pre-submission, 510(k), PMA, post-market)

**Post-PE considerations (Enzyme acquisition, July 2025)**
- PE-backed growth phase typically means pressure on ARR growth, net revenue retention, and operational efficiency — people who can execute fast and measure impact clearly will be valued
- Likely headcount discipline + ROI focus; strong individual contributors who don't need large teams tend to do well in this environment

**Summary:** The highest-leverage profile combines regulated-industry fluency (medtech or adjacent: pharma, diagnostics, IVD) with modern SaaS skills (product, engineering, or go-to-market). Pure-tech generalists without QMS/regulatory context face a steep onboarding curve; pure-domain experts without product/software comfort will struggle on the execution side.

### Re: Domain + engineering depth, and your tie-in — opus-4-8

#### Domain expertise — the deep version

Greenlight Guru sells into a world where *the document IS the product* as much as the device is. To contribute you need fluency in the regulatory spine their software automates:

- **The two core standards** — **ISO 13485** (QMS for medical devices) and **FDA 21 CFR Part 820**. As of **Feb 2, 2026** the FDA's **QMSR final rule** harmonizes Part 820 *with* ISO 13485 — so the whole industry is mid-transition right now. Knowing the QSR→QMSR shift is current, high-value context.
- **Design controls (820.30 / 13485 §7.3)** — design inputs → outputs → verification → validation, the **Design History File (DHF)**, design reviews, and the **traceability matrix** that proves every requirement was verified and validated. This is the heart of their Product Development module.
- **Risk management — ISO 14971** — hazard analysis, risk/benefit, risk controls traced to requirements. Inseparable from design controls.
- **IEC 62304** — medical device *software* lifecycle (since so many devices are now SaMD — Software as a Medical Device). Directly relevant to how their own customers build, and to how Greenlight builds.
- **Quality events** — CAPA, nonconformance, complaint handling, **post-market surveillance**, MDR/EU MDR reporting. The "quality" half of the platform.
- **Records discipline** — DHF, **DMR** (Device Master Record), **DHR** (Device History Record), controlled documents with versioning + approval routing.
- **21 CFR Part 11** — electronic records & **electronic signatures**: audit trails, immutability, attributability. This is a *software requirement*, not just a policy — it shapes the data model.
- **Data integrity / ALCOA+** — Attributable, Legible, Contemporaneous, Original, Accurate (+ Complete, Consistent, Enduring, Available). The mental model regulators apply to every record their software stores.
- **Clinical side** — GCP, EDC, PMCF, ePRO/eCOA for the Clinical Evidence platform.

The non-obvious point: their customers must **validate** the software itself (CSV / GAMP 5) before using it in a regulated process. So Greenlight's engineering bar is higher than normal SaaS — a bug isn't just a bug, it can invalidate a customer's compliance posture.

#### Engineering expertise required

- **Compliance-grade data architecture** — immutable audit trails, e-signature flows, versioned controlled documents, segregation-of-duties RBAC. Append-only / event-sourced thinking.
- **Traceability graphs** — the core technical asset: requirement → design output → verification → validation → risk control, queryable and rendered as live matrices. This is a graph problem.
- **Workflow / state machines** — approval routing, CAPA lifecycles, change-control gates with enforced transitions and full history.
- **Multi-tenant SaaS** — tenant isolation, configurability per customer's SOPs, performance at audit-trail-everything write volume.
- **AI in a *validated* context** — their AI-powered traceability push is the hard part: suggestions must be **auditable, bounded, and citeable**. You cannot ship a hallucinated CAPA. AI output needs provenance and human-in-the-loop sign-off to survive an audit.
- **Integrations** — PLM (Windchill, Teamcenter), ERP, EDC, eventually QMS-to-QMS supplier links.

#### How your experience ties in

Your portfolio is an unusually direct match — not because you've worked in medtech, but because you've **already built the exact engineering primitives a compliant QMS is made of:**

1. **Traceability graphs ≈ tsift's graph substrate.** Your provider-neutral graph substrate (nodes/edges/**provenance/freshness**, dependency DAGs, conflict matrices, multi-grammar AST extraction) is structurally the same machine as a requirements→design→V&V traceability matrix. This is your single strongest angle — you've shipped the hard part.

2. **Controlled documents + audit trail ≈ agent-doc.** A QMS is document control with approval workflows, versioning, and an immutable audit trail. agent-doc is **git-native** (every change committed, attributable, diff-tracked) with **CRDT conflict-free co-editing** — i.e. you've built data-integrity-by-construction and concurrent-edit safety, which is precisely the Part 11 / ALCOA+ posture. "Git-native = immutable, attributable audit trail" is a compelling line for them.

3. **Provenance propagation ≈ corky.** 3-way merge, content hashing (FNV-1a), per-contact/per-topic context propagating across collaborators — that's controlled-document distribution and supplier-context fan-out.

4. **Auditable AI is your wheelhouse.** You ship production AI (Presence AI real-time multimodal pipeline) *and* you build agent tooling whose whole purpose is turning noisy output into **bounded, citeable evidence for smaller models** (tsift digests). That "AI you can audit" framing is exactly what's needed to add AI to a validated QMS — the thing most AI engineers *can't* speak to.

5. **Regulated-workflow SaaS, already shipped.** Equity Funding Source is the closest analog: deal-pipeline state machines, document management, multi-file upload, needs-action dashboards, **74 production migrations, 741+ tests** on Cloudflare Workers + D1. Structurally that's a controlled-workflow document platform — swap "loan deal" for "CAPA" and the engineering is the same shape.

6. **Spec-driven discipline mirrors their process culture.** Your "lightweight living specs refined through closed-loop agent auditing + human-in-the-loop" methodology maps onto QMS process discipline: documented, reviewed, verified, change-controlled. You already think in spec → verification loops, which is the cultural language of a quality org.

7. **Stack overlap is clean.** Their SaaS ↔ your Next.js/React/GraphQL/Postgres (Presence AI) + TypeScript/Bun/Workers (EFS). No retooling needed.

**Honest gap to name proactively** (in your voice — tactful, reason-first): you haven't worked inside a 13485-certified shop or run formal CSV/Part 11 implementation. But frame it as: *the regulatory vocabulary is learnable in weeks; the engineering instincts a QMS demands — immutable audit trails, traceability graphs, provenance, conflict-free versioning — are things I've already designed and shipped.* That's a credible, non-defensive close.

**Suggested positioning one-liner for Royce:** *"I build systems where every change is tracked, attributable, and conflict-free — and I ship auditable AI on top of them. That's the same discipline a QMS lives or dies on."*

Want me to draft the actual outreach message to Royce, or tailor one of your resume variants (the `ai-agents` or `backend` cut looks closest) toward this role?

### Re: Job description + Greenlight Guru architecture — opus-4-8

I pulled the posting and cross-checked it against public sources on the platform. Summary first, then the architecture, then the resource list.

#### The role — Sr. Software Engineer I

- **Shape:** Remote (US/Canada), 4+ years, full-stack leaning toward modern front-end ownership. You lead multiple concurrent initiatives, own features end-to-end, and mentor juniors.
- **Stated stack expectations:** React (or similar modern JS); **fluency with REST, RPC, *and* GraphQL APIs**; cloud-native architecture; unit + integration test suites; CI pipelines; production observability (monitors, alerts, performance benchmarks) and incident response.
- **The signal that matters:** they explicitly want *"hands-on experience with large language models (LLMs), generative AI APIs, and intelligent automation."* This is not a buzzword line — see the AI section below; they are actively shipping an AI layer and hiring to extend it.
- **Culture tell:** the posting asks you to *"list your favorite seasoning for steak at the top of your resume."* It's a real filter for attention-to-instructions + personality. Actually do it.

#### What architecture Greenlight Guru runs

No single source dumps the full stack, so this is assembled from their HTEC modernization case study, the Built In company profile, their AI features page, and backend job postings:

- **Cloud-native on AWS.** Java (backend), JavaScript/TypeScript across the stack, **React** front-end. Tableau for analytics/BI. (Built In tech profile.)
- **Modular, decoupled platform reached via the Strangler pattern.** HTEC wrapped the legacy monolith behind an intermediary interface and carved out independent modules (the decoupled **Risk Management** module is the documented example). So it's a monolith → modular/microservices migration that is *still in progress* — meaning real legacy-code diagnosis work, which the JD calls out directly ("diagnosing performance issues in legacy code").
- **Service-oriented backend.** Backend roles describe building **micro-services, an analytics-platform layer, and 3rd-party integrations** — consistent with the decoupled direction above and with the REST/RPC/GraphQL surface in this JD.
- **A real, shipping AI layer — 13 features.** AI Brainstorming, AI Verifiability Check, **Suggested Links (AI traceability)**, AI Style Guide, AI Search with generated summaries, AI Chat over QMS data, AI Summaries, Quiz Builder, change/document-history summaries, and AI Training Analytics. Most pointed for you: the **AI Connector is built as a Model Context Protocol (MCP) server** exposing QMS data to external AI tools. (LLM provider/model isn't publicly disclosed.)

**Net:** a regulated-SaaS document/workflow platform on AWS (Java + TS/React, GraphQL), mid-Strangler toward modular services, with an MCP-exposed generative-AI layer they're investing in.

#### Why this maps to you (short — the deep portfolio tie-in is in the response above)

- **The MCP server is the single sharpest hook.** Their AI Connector is an MCP server; you build MCP/agent tooling daily (tsift, agent-doc). That's a *literal, current* overlap — name it explicitly in outreach, not generically as "AI experience."
- **"Auditable, bounded, citeable AI" is exactly the JD's hard part.** Their AI must survive a regulated audit; that's the whole thesis of tsift digests (bounded citeable evidence) and agent-doc's git-native audit trail.
- **Strangler/modular + GraphQL + AWS** lines up with EFS (Workers/D1, 74 migrations, modular workflow platform) and Presence AI (React/GraphQL/Postgres). The traceability-graph angle (tsift's graph substrate ≈ their Suggested-Links / requirement→V&V matrix) is still your strongest structural match.
- **Honest gap:** their backend is Java; you lean TS/Bun/Rust. Minor here — the role is front-end-and-AI weighted and the rest of the stack is TS/JS-heavy — but worth a one-line, non-defensive acknowledgment.

#### Resources found

- [The posting itself](https://job-boards.greenhouse.io/greenlightguru/jobs/5160112007) and [all GG openings](https://job-boards.greenhouse.io/greenlightguru) (Greenhouse)
- [HTEC modernization case study](https://htec.com/insights/success-stories/greenlight-guru-advancing-quality-management-system/) — Strangler pattern + decoupled modular architecture
- [Built In company profile](https://builtin.com/company/greenlight-guru) — Java / JS / TS / React / AWS / Tableau
- [Greenlight Guru AI features](https://www.greenlight.guru/blog/greenlight-guru-ai-features) — the 13 AI features incl. the MCP-server AI Connector
- [Crunchbase tech details](https://www.crunchbase.com/organization/greenlight-guru/technology) (403 on automated fetch — open in a browser)

Want me to turn this into a tailored cover note / outreach that leads with the MCP-server + auditable-AI overlap, or fold the architecture findings into your resume's GG-specific cut?
❯ Will my Martin Investments/Censible experience help me succeed at Greenlight Guru? I made a ESG portfolio rebalancer and Screener tools.

### Re: Does your Martin Investments / Censible (ESG) experience transfer? — opus-4-8

Short answer: **yes — not the *domain* (finance ≠ medtech), but the *engineering primitives* transfer almost one-for-one, and the ESG angle specifically hands you the "regulated, disclosure-driven scoring" vocabulary GG cares about.** Here's the mapping.

#### What you actually built (the transferable shapes)

- **A screener is a configurable rule-engine over a governed dataset.** Inclusion/exclusion criteria, thresholds, and policy filters applied to a universe of securities is *structurally the same machine* as a QMS applying acceptance criteria and compliance gates to design records. Their **AI Verifiability Check** (flags non-measurable/untestable requirements) and **AI Style Guide** (enforces writing rules across contributors) are literally automated screeners — you've built that class of system.
- **A rebalancer is constraint-satisfaction + state reconciliation.** Compute a target allocation and the ordered transitions to reach it under constraints — that's the same shape as GG's **CAPA / change-control state machines and approval-routing lifecycles** (compute desired end-state, enforce the legal transitions to get there). This is one of the harder problems on their platform and you've shipped its analog.
- **Heterogeneous-data aggregation + defensible scoring.** ESG scoring pulls noisy, multi-source data and produces a score you must be able to *explain*. Maps directly onto GG's analytics platform / Tableau BI and **AI Search summaries**, and onto the broader "every number must trace to its inputs" requirement.
- **Decision-support React UI over complex data.** Screener tools are exactly that — and the Sr. SWE I role is weighted toward **modern front-end ownership (React)**. This is a clean, immediate stack match.

#### The non-obvious advantage: ESG is itself a regulated, audit-driven domain

This is the part most candidates can't claim. ESG investing lives under **disclosure regulation and greenwashing scrutiny** (SEC ESG disclosure rules, methodology-transparency expectations). So you've already worked where **provenance, methodology transparency, and a defensible/auditable score** are non-negotiable — that's the *same mindset* as ALCOA+ / 21 CFR Part 11 / "you cannot ship a hallucinated CAPA." You can say, credibly: *"I've built scoring systems whose methodology had to survive a regulator's and a skeptic's audit"* — which is precisely the bar GG's AI features must clear.

#### How to position it (one line for outreach)

> *"At Censible I built ESG screening and rebalancing — configurable rule engines and constraint-based state reconciliation over data whose scoring had to be transparent and defensible. That's the same engineering a QMS runs on: acceptance criteria, change-control workflows, and auditable scoring."*

#### Honest gap

It's finance-regulated, not medical-device-regulated — no ISO 13485 / 21 CFR 820 exposure. But frame it as transfer, not equivalence: *the regulatory vocabulary differs; the discipline (transparent rules, constraint-driven workflows, auditable scoring) is identical and already shipped.*

❯ **Bottom line:** Censible is a genuine asset for this role — lead with the rule-engine/constraint-solver and the "auditable scoring in a regulated domain" framing, and treat the medtech-domain gap as a few weeks of vocabulary, not a capability gap. Want me to fold this into the GG-specific resume cut alongside the EFS / tsift / agent-doc angles?
❯ Will my real-time experience help me succeed at Greenlight Guru? Are the skills transferrable?
### Re: Does your real-time experience transfer? — opus-4-8

Honest answer up front: **real-time is a real strength here, but it's a *supporting* one, not the headline.** A QMS is fundamentally transactional SaaS — document control, workflow, audit trails — not a hard-real-time/low-latency product like Presence AI. So don't lead a GG pitch with "real-time streaming." That said, three specific slices of your real-time work transfer directly and create genuine value:

#### Where it genuinely transfers

1. **Concurrency + state synchronization + conflict resolution — your strongest real-time-adjacent asset.** Real-time multi-user systems force you to solve concurrent state mutation, ordering, and consistency. That's exactly what **collaborative controlled-document editing** needs: multiple contributors editing the same design record, approval routing with concurrent reviewers, no lost updates. Your **agent-doc CRDT work is literally conflict-free concurrent editing** — name it specifically; it's the cleanest bridge from "real-time" to "QMS."
2. **Streaming / low-latency LLM integration → responsive AI UX.** Their AI Chat and AI Search (generated summaries above results) live or die on feeling fast. Your real-time multimodal pipeline experience — streaming inference, latency budgets, backpressure, graceful degradation — is exactly what makes those features responsive instead of janky. The JD's explicit LLM/generative-AI requirement is where this lands.
3. **Latency discipline + observability under load.** Real-time engineers instrument everything — p99 latency, backpressure, failure modes. The JD calls out **"monitors, alerts, performance benchmarks"** and **"diagnosing performance issues in legacy code"** directly. That instinct is baked into anyone who's shipped real-time, and it's a daily-contribution differentiator on a maturing platform.

#### Where it doesn't (say this, don't hide it)

The core platform isn't streaming or sub-100ms; most of the surface is request/response CRUD over a compliance-grade data model. So real-time is a *transferable engineering discipline* here, not a literal product-skill match. Treat it as a strong secondary, behind your headline angles (traceability graphs ≈ tsift, auditable AI, rule-engine/state-machine work).

#### How to position it (one line)

> *"My real-time work gave me concurrency, state-synchronization, and latency discipline — which I'd apply to collaborative document editing (conflict-free co-authoring) and to making your AI features genuinely responsive — even though the core QMS isn't a streaming product."*

❯ **Bottom line:** transferable, yes — lead with the **CRDT/concurrency** and **streaming-AI-UX** slices, frame the rest as engineering discipline, and keep it behind the auditable-AI / traceability-graph angles that are your sharpest fits. Want me to assemble all of these (architecture, Censible, real-time) into a single prioritized "why me for GG" narrative for the resume top / cover note?
❯ Are State Machines + Lean useful?
### Re: Are state machines + Lean useful at Greenlight Guru? — opus-4-8

Both yes — but for different reasons. **State machines are a direct, technical bullseye; Lean is domain/culture vocabulary that makes you credible.**

#### State machines — a direct hit, arguably your sharpest engineering match

A QMS *is* a collection of enforced state machines. Their own architecture calls it out: **"approval routing, CAPA lifecycles, change-control gates with enforced transitions and full history."** Every controlled object moves through a regulated lifecycle:

- **Documents:** Draft → In Review → Approved → Effective → Obsolete (with e-signature gates).
- **CAPA / quality events:** Open → Investigation → Root Cause → Action Plan → Verification of Effectiveness → Closed.
- **Change orders:** Requested → Impact Assessment → Approval → Implementation → Verification.
- **Design controls:** input → output → verification → validation, each a gated transition.

The hard part isn't drawing the diagram — it's **enforcing legal transitions, preventing illegal ones, capturing full history for audit, and handling concurrent actors.** That's exactly what you've built repeatedly: corky's state machines, EFS's deal-pipeline state machines, and agent-doc's own cycle state machine (preflight → respond → finalize → committed with enforced recovery transitions). You can speak to *enforced, auditable transitions* — not just "I've used a state machine library." Lead with this; it's as strong as the traceability-graph angle.

#### Lean — useful as the *language* of their domain and their moment

"Lean" lands two ways at GG, both helpful:

- **Lean / Lean Six Sigma is core quality-domain vocabulary.** Their customers live in continuous-improvement, waste-reduction, root-cause (5 Whys, fishbone), and CAPA-effectiveness thinking. Knowing Lean means you speak the quality org's language — it signals you understand *why* the workflows exist, not just how to code them. That's the domain-fluency gap most pure engineers have.
- **Lean as execution philosophy fits the post-PE moment.** Post-Enzyme (PE-backed), the environment rewards "execute fast, measure impact, cut waste" — which is Lean startup / Lean software (build-measure-learn, small batches, flow). The earlier note about ROI-focus and headcount discipline is exactly where a Lean operating instinct reads as a fit.

Caveat: Lean is *supporting* — it strengthens your domain credibility and culture fit, but it isn't a hard technical requirement. Don't over-index on it; use it to show you understand the quality world your code serves.

#### How to position (one line)

> *"A QMS is enforced, auditable state machines — CAPA, change control, document lifecycles — which is exactly the kind of system I've built (corky, EFS pipelines, agent-doc's own cycle engine). And I think in Lean terms, so I understand the continuous-improvement workflows those state machines exist to serve."*

**Bottom line:** state machines = put it near the top of your technical pitch (peer to traceability graphs and auditable AI); Lean = weave in as domain-fluency + culture-fit signal. Want me to now assemble the full prioritized "why me for GG" narrative (state machines, traceability graphs, auditable AI, Censible, real-time, Lean) for the resume top / cover note?
<!-- agent:boundary:c45e5b68 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
