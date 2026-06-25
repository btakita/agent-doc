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
<!-- agent:boundary:5ece03c3 -->
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
