# REItrades — OM Sync Engine

**Fixed-bid first slice** · Brian Takita · 2026-07-03

## What it is

The layer that keeps the OM and teaser in sync with listing data. When a listing's numbers change after generation, the affected sections update; a buyer never sees a stale figure.

## Deliverables

| # | Deliverable | Notes |
|---|-------------|-------|
| 1 | Field-level dependency map: listing data → OM/teaser sections | One asset class (your call which) |
| 2 | Change detection + staleness state on generated documents | Any post-generation edit marks affected sections stale |
| 3 | Selective deterministic re-render | Only affected sections regenerate; numbers, calculations, and template assembly are code-owned, never AI |
| 4 | Stale guard | A stale OM/teaser cannot be served silently: deterministic fields auto-refresh; anything else is flagged and held |
| 5 | Admin "out of sync" indicator + one-click regenerate | In the existing admin |
| 6 | Change audit log | What changed, what regenerated, when, by what |

**Open source:** the generic dependency-tracking / incremental-recompute engine is released open source and maintained by me. You pay only for the REItrades-specific work: the field mapping, template integration, and admin touches. Your schemas, templates, and business rules stay yours.

## Price

| Option | Scope | Fixed price |
|--------|-------|-------------|
| **Core** | Deliverables 1–6 above | **$3,500** |
| Core + AI prose regen | Adds: AI-generated narrative sections regenerate only when their source fields change, routed through a review gate before publishing | **$5,500** |

Half at kickoff, half on acceptance. If we later proceed to the larger V1.2 scope, this work is credited against Phase 1 — it directly covers part of the "generate-once / regenerate" and OM line items already estimated.

## Timeline

~2 weeks from repo access. I can start next week.

## Acceptance test

Change a listing's financial field (e.g. Asking Price or NOI). The OM and teaser reflect the change without a full regeneration, the audit log records it, and at no point could a buyer-facing document have served the old number.

## Assumptions

- Repo + staging access at kickoff
- One asset class template in scope (more are follow-on, same pattern)
- Current generation flow as discussed (wizard step 5 / generate-once)

## Out of scope (unchanged from the V1.2 estimate)

Extraction from old OMs/flyers, additional asset classes, broker segment, listing tiers/Stripe, demographics sourcing. Each is a candidate next slice if this one works.

## Off-ramp

If the slice doesn't earn a next step, you keep working infrastructure and the open-source engine keeps getting maintained. We shake hands.
