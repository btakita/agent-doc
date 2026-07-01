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
- **"100% accurate" AI content is scoped as** source-backed extraction + validation + confidence scoring + admin review, **not** a pure-AI promise. I will not present AI extraction as inherently 100% accurate without a verification path, especially for buyer-facing materials.

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
| Improve AI-generated content (demographics, data accuracy) | 32-75 | Source-backed + validation/confidence/admin review. No pure "100% accurate" promise. |
| Polish OM generator output | 20-50 | Depends on design expectations and PDF/rendering stack. |
| Ensure additional attributes flow into generated OM | 6-14 | Small if listing-data→OM mapping is clean. |
| Broker-only OM upload, NDA/email workflow compatible; principals use generator | 14-30 | Permission, file handling, NDA-triggered delivery, attachment/link rules. |
| Old OM/flyer upload → extract → autofill listing + OM | 45-100 | **Largest AI item:** document ingestion, OCR/PDF parsing, schema extraction, confidence flags, review UI, sample-doc tests. |
| 3 listing tiers (Free / Premium $199 / Elite $499) as **new first step** before Location; Stripe links | 18-38 | The tier step is inserted ahead of the confirmed 5-step wizard, so it also shifts step-index/gating logic. Tier selection, state persistence, Stripe link flow, entitlement behavior, admin visibility. |

---

## 4. Roll-up (de-duped, shared primitives built once)

| Scope | Agentic hours | At $100/hr |
|---|---:|---:|
| Discovery / safety plan | 24-44 | $2.4k-$4.4k |
| Admin DBMS workstream | 190-320 | $19k-$32k |
| Broker segment workstream | 95-170 | $9.5k-$17k |
| Listing creation + OM rebuild (first asset class) | 245-450 | $24.5k-$45k |
| Cross-workstream QA / release hardening | 40-80 | $4k-$8k |
| **Full V1.2 (every listed item, production-grade)** | **530-820** | **$53k-$82k** |
| **First scoped post-launch tranche** | **265-390** | **$26.5k-$39k** |

*Adjustment after reviewing the PDF wizard screenshots: the listing-creation workstream rose ~15-20h (explicit Attachments/media step + confirmed asset-class field density), nudging the top line up by roughly one point. The prior $52k-$80k / $26k-$38k headline still holds as a round-number bid — the screenshots validated the estimate rather than expanding it.*

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
