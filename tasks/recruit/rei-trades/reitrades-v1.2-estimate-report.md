# REItrades V1.2 — Work Breakdown & Estimate

**Prepared for:** Artur Muller, Founder & CEO, REItrades
**Engineer:** Brian Takita — senior full-stack (Rust / Vue / Postgres)
**Rate:** $100/hr
**Basis:** Itemized from the V1.2 brief (`reitrades-v1.2.md` / V1.2 PDF), priced as **agentic-implementation hours** — one senior AI-native engineer using coding agents for search, scaffolding, migration/test drafts, and parallel review.

---

## 1. How to read this estimate

- **These are agentic hours, not conventional solo hours.** They already assume heavy use of coding agents. They still include human review, integration, targeted tests, and production QA on a **live** marketplace — the parts agents do not compress.
- **Ranges are real.** The low end is hit when existing patterns are clean and reusable; the high end when data models are inconsistent or production migration risk is high. With my tooling I bias toward the **low end of each range** — that is the competitive lever, not cutting scope.
- **Rows are incremental within a shared implementation.** Admin grid columns/filters/inline-edit, bulk-action plumbing, audit logging, import/export, role gates, and listing-data mapping are built **once** and reused. Per-row numbers assume those primitives already exist, so they do not sum naively — see the de-duped roll-up.
- **What agents compress vs. what they don't:**
  - Admin table/UI work: ~25-40% faster.
  - Rust/Postgres permission & data-model work: ~15-30% faster.
  - External integrations, production migrations, PDF/OM output, AI extraction accuracy: **least** — validation dominates.
- **"100% accurate" is scoped as a deterministic core with an AI assist** (see §3b), **not** a pure-AI promise. Deterministic code owns every number, validation rule, structured-data lookup, and document assembly — those are *provably* correct. The model only *proposes* values for the genuinely fuzzy work (extraction from messy source docs, generated narrative), and every proposal passes deterministic validators + confidence scoring + the A–D provenance rubric (§3a) + admin review before it ships. I will not present AI extraction as inherently 100% accurate without that verification path, especially for buyer-facing materials.

---

## 2. Scope at a glance

Three workstreams against a launched (post-beta, 10/14/2025) production platform:

| Workstream | What it covers |
|---|---|
| **1. Admin DBMS** | Listings/user/company management: columns, inline edit, filters, bulk actions, MailerLite status, import/export, audit trails, deletes, NDA/LOI visibility, activity analytics. |
| **2. Broker segment** | New account type, application + approval workflow, separate broker admin views, role-gated pages/permissions, per-listing access codes, Broker/Principal tags. |
| **3. Listing creation + OM rebuild** | Rust/Tera rebuild, step gating, asset-class schemas, teaser/OM data derivation + production migration, AI/BatchData marketing materials, OM polish, broker OM upload, old-OM extraction, Stripe listing tiers. |

---

## 3. Itemized task breakdown

### 0. Setup / technical audit

| Task | Hours | Notes |
|---|---:|---|
| Repo/data-model audit, route/module map, milestone plan | 16-28 | Agents parallelize codebase mapping; final architecture & sequencing need senior review. |
| Production-safety plan for live beta data | 8-16 | Migrations, rollback, delete policy, regression-risk mapping. |
| **Subtotal** | **24-44** | |

### 1.1 Listings Management

| Task | Hours | Notes |
|---|---:|---|
| Delete listing from action dropdown; cascade related data | 16-28 | Prefer soft-delete/archive + cleanup jobs unless hard delete is explicitly required; hard cascade through NDA/LOI/files/analytics is riskier. |
| View NDAs and LOIs signed on each listing | 8-16 | Mostly query/UI if documents are stored consistently. |

### 1.2 User Management

| Task | Hours | Notes |
|---|---:|---|
| Update user summary-table columns exactly as listed | 10-18 | Add/remove columns, enum/multi-select display, sort. |
| View/edit complete user profile pages (incl. analytics) | 14-26 | More if user/contact/company data is split across inconsistent models. |
| Email status Active / Not Active / bounced from MailerLite | 10-20 | Depends on MailerLite webhook/API access. |
| Search/filter users by all data points | 12-24 | Needs query-builder/index review so production filters stay fast. |
| Inline edit on summary table (no dropdown/popup) | 16-30 | Save/validation/error states need careful review. |
| 50 contacts per page | 2-4 | Small if pagination exists. |
| Bulk platform invitations to selected users | 10-18 | Selection state, confirmation, background job/throttle, audit. |
| Send teasers/flyers to selected users from user mgmt | 14-28 | Reuses teaser path with new recipient semantics; must prevent accidental broad sends. |
| Bulk import/export users with audit trail | 18-34 | CSV/schema validation, dry-run/errors, audit records. |
| Remove "Invite network" button | 1-2 | UI removal + route/action check. |
| New bulk-actions button for selected users | 6-10 | Menu/actions plumbing reusing bulk framework. |
| Delete users individually and in bulk | 10-20 | Soft-delete policy, account-access effects, company/contact relations. |

### 1.2 User Analytics / Activity

| Task | Hours | Notes |
|---|---:|---|
| Last signed in | 3-8 | Small if auth/event table records it. |
| NDAs signed through platform | 4-8 | Relationship display if NDA records normalized. |
| LOIs submitted through platform | 4-8 | Same pattern as NDA. |
| Property-teaser engagement from MailerLite | 14-28 | Harder — depends on campaign/message-ID mapping to MailerLite engagement API. |
| History of messages sent through platform | 8-18 | Depends on whether outbound sends are centrally logged. |

### 1.3 Company Management

| Task | Hours | Notes |
|---|---:|---|
| Update company summary-table columns exactly as listed | 8-16 | Simpler than user table. |
| View/edit full company profiles | 12-22 | Edit form, validation, related records. |
| View users/contacts per company | 6-12 | Straightforward if every user has a company relation. |
| Search/filter companies | 10-20 | Reuse user-management filter/query primitives. |
| 50 companies per page | 2-4 | Small if pagination exists. |
| Bulk import/export companies with audit trail | 16-30 | Slightly simpler than user import/export. |
| Delete company individually and in bulk | 10-20 | Define behavior for attached contacts/users first. |
| New bulk-actions button for companies | 4-8 | Reuse bulk-action menu pattern. |

### 2.1 Broker Database

| Task | Hours | Notes |
|---|---:|---|
| Separate broker DB management under admin | 22-40 | Assumes logical separation in same Postgres app. **Physical separate DB/schema: +16-40.** |
| Broker user table (Name/Title/Email/Company/License#/State/LinkedIn/Activated/Created/Email Status) | 6-12 | Incremental after admin-table primitive. |
| Broker company table (Name/License#/State/Website/LinkedIn/Contact Count) | 6-12 | Same pattern as company admin table. |

### 2.2 Approval Workflow

| Task | Hours | Notes |
|---|---:|---|
| Broker applies free, shows CRE experience | 8-16 | Application fields + validation. |
| Broker application flow | 16-30 | Public/app flow, persistence, admin visibility, notifications. |
| Approval gating before platform entry | 14-28 | Role/status gating, route guards, denied-access tests. |

### 2.3 Broker Pages / Permissions

| Task | Hours | Notes |
|---|---:|---|
| Broker pages/capabilities differ from principals | 10-18 | Shared role-gate framework; removals below are incremental. |
| Remove User Acquisition Criteria box (brokers) | 2-6 | Keep code-added listings. |
| Remove Browse page for brokers | 6-12 | Nav guard + direct-URL protection. |
| Remove Browse/Acquisition/Request-BOV buttons from broker dashboard | 2-6 | UI + server-side permission check. |
| Engaged Listings = code-accessed + NDA-signed | 10-20 | Event/source-of-truth query. |
| Saved Listings includes code-accessed listings | 8-16 | Depends on saved/accessed data model. |
| Remove Company Peers (broker profile) | 2-5 | Small UI/route change. |
| Remove Company Activities box (broker profile) | 2-5 | Small UI/route change. |
| Per-listing access code, any logged-in user incl. brokers | 18-34 | Code generation, uniqueness, access rules, audit, anti-guessing protections. |
| Broker tag / Principal tag on listings | 6-12 | Data field, UI badges, optional filters. |

### 3. Listing Creation & OM Rebuild (first asset class)

**Confirmed wizard scope (from the V1.2 PDF "Listing Creation Steps" screenshots).** The current flow is a **5-step wizard** with a **separate 5-state listing lifecycle** running alongside it:

- **Wizard steps:** (1) **Location** — asset class, address, city, state/province, postal code, entrance-location details; (2) **Property Overview** — building name, asset-class sub-category, main use, submarket, brand/flag, year built/renovated, total square footage, and asset-class-specific fields (the hotel example shows Number of Keys, Room size, Parking, Amenities); (3) **Financials** — an "Investment Snapshot" with ~16 fields (Asking Price, Cap Rate, Historical Occupancy, ADR, RevPAR, Gross Income, Vacancy, Effective Gross Income, Opex, NOI, LTV, Interest Rate, Amortization Period, Annual Debt Service); (4) **Attachments** — categorized document upload (Expenses Breakdown, CapEx, Rent Roll, Property Video) plus additional documents and a property-photo gallery, all drag-and-drop; (5) **Marketing Materials** — generated teaser/OM output.
- **Lifecycle status machine:** Draft → Under Review → Active → Pending → Closed, with the seller only able to submit to **Under Review** until REItrades approves (matches the brief's gating rule).
- **Per-step custom attributes:** every step exposes an "+ Add Attribute" control.

**What this confirms for the estimate:** the field density is real and asset-class-driven (the hotel example alone carries ~12 property-overview + ~16 financial fields, several hotel-specific), so the per-asset-class schema work sits at the **mid-to-high end** of the ranges below rather than the low end. It also surfaces one item the markdown-only pass under-counted — the categorized **Attachments/media step** — now broken out explicitly. The screenshots **validate** the prior ranges; they do not blow up the top line.

| Task | Hours | Notes |
|---|---:|---|
| Reimplement 5-step listing create/edit wizard in Rust/Tera, match current UI, consolidate modules | 40-72 | Core rebuild before feature bullets; 5 steps (Location, Property Overview, Financials, Attachments, Marketing Materials), visual parity + backend structure need review. |
| Step locking + 5-state lifecycle (Draft→Under Review→Active→Pending→Closed) + Under-Review submission gating | 22-40 | Two coupled state machines: per-step unlock gating and permission-gated listing status transitions. |
| Asset-class required/optional + custom attributes (first class) | 30-58 | Field-dense per screenshots (~28 fields for the hotel example). **+12-24 per additional asset class** by field complexity. |
| Attachments step: categorized doc upload (Expenses/CapEx/Rent Roll/Video) + additional docs + photo gallery, drag-and-drop | 14-24 | Newly broken out from the PDF; upload categories, validation, gallery ordering, storage wiring. |
| Teaser/OM data derivation + **production migration** | 30-65 | Major production-data risk; migration script + regression checks vs. existing listings. |
| Marketing Materials from BatchData + AI fallback; generate-once / regenerate | 26-55 | Idempotency, source tracking, failure states, manual regeneration. |
| Modular one-asset-class-at-a-time PR structure | 8-14 | Process/architecture overhead that lowers later review risk. |

### 3. Additional Comments / Requests

| Task | Hours | Notes |
|---|---:|---|
| Improve AI-generated content (demographics, data accuracy) | 32-75 | Source-backed + validation/confidence/admin review, **graded against the accuracy rubric below** (§3a) — replaces the unmeasurable "100% accurate" ask with a defined acceptance bar. |
| Polish OM generator output | 20-50 | Depends on design expectations and PDF/rendering stack. |
| Ensure additional attributes flow into generated OM | 6-14 | Small if listing-data→OM mapping is clean. |
| Broker-only OM upload, NDA/email workflow compatible; principals use generator | 14-30 | Permission, file handling, NDA-triggered delivery, attachment/link rules. |
| Old OM/flyer upload → extract → autofill listing + OM | 45-100 | **Largest AI item:** document ingestion, OCR/PDF parsing, schema extraction, confidence flags, review UI, sample-doc tests scored on the **field-accuracy rubric** (§3a). |
| **AI accuracy rubric + evaluation harness** | 14-28 | New line item (per "include rubrics in AI accuracy work"): per-field grading rubric, a labeled ground-truth sample set, and an automated scoring harness that gates release of extraction/generation changes. This is what makes "accurate" measurable and defensible to buyers. |
| 3 listing tiers (Free / Premium $199 / Elite $499) as **new first step** before Location; Stripe links | 18-38 | The tier step is inserted ahead of the confirmed 5-step wizard, so it also shifts step-index/gating logic. Tier selection, state persistence, Stripe link flow, entitlement behavior, admin visibility. |

### 3a. AI accuracy rubrics (acceptance criteria, not a "100% accurate" promise)

Every AI-touched field — extracted from an uploaded OM/flyer, or generated (demographics, marketing copy) — is graded on a **provenance rubric** that decides whether it can auto-publish or must be reviewed:

| Grade | Meaning | Handling |
|---|---|---|
| **A — Source-exact** | Value copied verbatim from an authoritative source (BatchData, uploaded rent roll/financials, structured listing field) | Auto-fill, no review |
| **B — Derived-verified** | Computed from source fields by a deterministic rule (e.g. NOI = EGI − Opex) and reconciles within tolerance | Auto-fill, flagged for spot-check |
| **C — AI-inferred** | Produced by the model without a directly citable source | **Blocked from external send** until an admin approves; shown with its confidence and rationale |
| **D — Missing/low-confidence** | No source and confidence below threshold | Requires manual entry; never auto-published |

**Acceptance thresholds (the "done" bar for an AI change, measured on the labeled sample set):**

| Field class | Target precision | Notes |
|---|---|---|
| Financial (Asking Price, Cap Rate, NOI, Opex, LTV…) | **≥ 99%** on Grade A/B; zero silent Grade-C financials sent externally | Money fields are buyer-facing; wrong numbers are the worst failure mode |
| Structured facts (address, sq ft, year built, keys/units) | **≥ 97%** | Mostly source-exact |
| Demographics / market data | **≥ 95%**, every value carrying a cited source | "much better demographics" from the brief, made testable |
| Narrative/marketing copy | Human-approved before external send | Graded for factual claims only, not style |

The harness runs on a labeled ground-truth set (a handful of representative OMs per asset class), scores each release of the extraction/generation code, and **gates merge** on the thresholds above. That turns Artur's "100% accurate" request into a concrete, auditable acceptance test instead of an open-ended promise — and gives admin review a queue driven by grade, not guesswork.

### 3b. Deterministic core vs. AI assist (where "100%" is honest)

The strongest, most defensible version of the accuracy story is to split the pipeline into a **deterministic layer** and a **generative layer**, and let deterministic code own everything it possibly can. Then "100% accurate" attaches to the parts that can actually carry it, and the fuzzy parts are described as *validated + reviewed* — a claim that holds up on a call and in a contract.

**Deterministic software is provably correct — this is where "100%" is literal:**

| Concern | Why it's deterministic (100%) |
|---|---|
| **Calculations** — cap rate, NOI, price/unit, price/SF, GRM, loan sizing, LTV | Formulas, not predictions. Computed in code from validated inputs; the model never does arithmetic. |
| **Schema / field validation** — types, ranges, required fields, unit normalization, enum membership (asset class, state, zoning) | A validator passes or fails; correct by construction. |
| **Structured data retrieval** — reading a known field (year built, SF, ownership) from BatchData / rent rolls / structured listing fields | Deterministic lookup. Only *locating* a value inside a messy source doc is fuzzy. |
| **Template / OM rendering** — Tera templates | Identical inputs produce identical output. Document assembly is deterministic; only the prose inside is generated. |

**The AI layer only proposes — it never publishes unchecked.** For the genuinely probabilistic work — extraction from unstructured source (old OM PDFs, flyers, scanned rent rolls) and generated narrative (listing copy, market/demographic write-ups) — the model *proposes* values, and every proposal runs the gauntlet already defined above:

1. Deterministic code owns every number, validation rule, and document assembly — provably correct and auditable.
2. The model only proposes values for fuzzy extraction/generation.
3. Every model output passes deterministic validators + confidence scoring + the A–D provenance rubric (§3a) before it is shown.
4. Anything money-facing or buyer-facing gets admin review before it goes out.

**Why lead with this:** it is the strongest trust argument, it is cheaper and more testable than trying to make the AI itself infallible, and it fits a bootstrapped budget — deterministic checks are hours of code, not an open-ended model-tuning bill. If a *learned prediction* on tabular data is wanted later (valuation/cap-rate estimate, lead scoring, asset-class classification), that is a phase-2, data-dependent option requiring a labeled outcome set — and the right tool there is gradient-boosted trees (XGBoost/LightGBM), which beat plain neural nets on tabular data. It is out of scope for V1.2.

---

## 4. Roll-up (de-duped, shared primitives built once)

| Scope | Agentic hours | At $100/hr |
|---|---:|---:|
| Discovery / safety plan | 24-44 | $2.4k-$4.4k |
| Admin DBMS workstream | 190-320 | $19k-$32k |
| Broker segment workstream | 95-170 | $9.5k-$17k |
| Listing creation + OM rebuild (first asset class) | 260-480 | $26k-$48k |
| Cross-workstream QA / release hardening | 40-80 | $4k-$8k |
| **Full V1.2 (every listed item, production-grade)** | **545-850** | **$54.5k-$85k** |
| **First scoped post-launch tranche** | **270-400** | **$27k-$40k** |

*Adjustments: (1) after reviewing the PDF wizard screenshots the listing-creation workstream rose ~15-20h (explicit Attachments/media step + confirmed asset-class field density); (2) the AI accuracy rubric + evaluation harness (§3a) adds ~14-28h. The rubric harness is front-loaded — it belongs in the first tranche so the AI-accuracy bar is enforced from day one, not retrofitted. The round-number $52k-$80k / $26k-$38k headline still holds for the bid; these are refinements, not scope expansion.*

---

## 5. Recommended phasing (competitive bid)

I recommend leading with a scoped first tranche rather than a single fixed bid on the whole list — it controls Artur's spend and lets the team validate the new code structure on live data before committing the full budget.

**Phase 0 — Discovery (1 week)**
Repo/data-model audit + production-safety plan + tight milestone plan. **24-44h → $2.4k-$4.4k.** Credited toward Phase 1 if we proceed.

**Phase 1 — First production tranche (260-380h → $26k-$38k)**
Highest-value admin improvements + broker application/approval flow + one asset class of the listing/OM rebuild. Weekly demos and review points.

**Phase 2+ — Remaining V1.2 (balance to 520-800h total)**
Additional asset classes, full document extraction, OM polish, remaining integrations, production migrations, and hardening — sequenced after Phase 1 proves the structure.

**Engagement options (pick one):**
- **Hourly at $100/hr with a weekly cap** — lowest risk for both sides; Artur controls burn week to week.
- **Fixed price per phase** — e.g. Phase 1 fixed in the **$30k-$34k** band (mid-range) against a frozen scope, change-orders for additions. Predictable for budgeting; I hold the velocity risk.

**Competitive note:** $100/hr is at the lower-competitive end for senior Rust+Vue contract work ($100-175/hr market). Combined with agentic delivery biasing toward the low end of each range, this is a competitive bid without underpricing the production-QA and migration risk that a live marketplace actually carries.

---

## 6. Assumptions & open questions (confirm before locking a fixed price)

1. How many asset classes must ship in the first release?
2. Is "separate broker database" a hard **physical** separation, or role/account-segment separation in the same Postgres system? (Physical adds 16-40h.)
3. Are MailerLite, DocuSign/e-signature, Stripe, BatchData, and the AI provider already integrated in production with accessible credentials?
4. Hard deletes or soft deletes with audit retention? I default to soft delete for transaction records unless told otherwise.
5. Are approved OM/teaser templates per asset class already designed, or is template/design work in scope?
6. What review is expected on AI-generated fields before emails/OMs are sent externally?
7. ~~Do the PDF "Listing Creation Steps" screenshots define fields/steps beyond the markdown brief?~~ **Resolved** — the PDF confirms a 5-step wizard (Location / Property Overview / Financials / Attachments / Marketing Materials) plus a 5-state lifecycle (Draft→Under Review→Active→Pending→Closed) and dense asset-class fields; folded into §3. Remaining sub-question: **which single asset class ships first** (the screenshots show a hospitality/hotel example — is hotel the pilot, or another class?).

---

## 7. Sources

- Feature brief: `tasks/recruit/rei-trades/reitrades-v1.2.md` (and V1.2 PDF)
- Platform demo: https://www.youtube.com/watch?v=1Q8RdnQwYLw
- Site: https://reitrades.com/ · Sellers: https://reitrades.com/sellers/ · Buyers: https://reitrades.com/buyers/
- Artur Muller: https://www.linkedin.com/in/mullerartur/
- REItrades company page: https://www.linkedin.com/company/reitradesmarketplace
- REJournals (Artur Muller, 2025-08-20): https://rejournals.com/streamlined-commercial-real-estate-sales-ai-powered-and-automated/
