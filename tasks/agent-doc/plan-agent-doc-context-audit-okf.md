# Agent-Doc Context Audit and OKF Plan

Date: 2026-06-27

## Goal

Make agent-doc context layered, deduplicated, and measurable so OKF becomes a
real dynamic-context primitive instead of another documentation folder.

## Current Position

The current OKF integration gives agent-doc a canonical place for durable
concepts such as session cycle, instruction surface, and dynamic context. It
does not yet mechanically prevent duplicate runtime context or prove token
reduction. The next step is an audit that assigns ownership, measures context
cost, and defines runtime deduplication.

## Plan

### 1. Inventory Current Context

List every instruction and context source:

- `SKILL.md`
- installed harness surfaces
- `AGENTS.md`
- `runbooks/*.md`
- `okf/*.md`
- generated preflight, plan, session-review, and tsift packs
- project-local instruction files

Classify each source as one of:

- hot-path
- procedure
- concept
- generated-state
- project-local

Record approximate word, byte, and token cost per source.

### 2. Define Ownership Rules

Use these ownership rules as the audit target:

- `SKILL.md`: mandatory cycle invariants and routing only.
- `runbooks/*.md`: procedural workflow branches.
- `okf/*.md`: durable vocabulary and short concept definitions.
- generated packs: current state, never durable policy.
- project/root instructions: local overrides and repo-specific routing.

Each repeated rule or term should have one owner and all other surfaces should
route to that owner.

### 3. Find Duplication

Search for repeated definitions and repeated procedural prose around:

- closeout boundary
- session cycle
- dynamic context
- instruction surface
- harness install
- pending mutation
- preflight contract
- plan contract
- generated context packs

For each duplicate:

- choose the owning source
- replace repeated prose with a short route or link
- keep hot-path text only when it is required every cycle

### 4. Design A Loaded-Context Ledger

Define a runtime record that can prove a source was loaded once per cycle:

```text
source_id
source_kind
path
content_hash
concept_id
loaded_at
expansion_reason
```

Use the ledger to suppress duplicate expansions across OKF concepts, runbooks,
and generated packs.

### 5. Tighten OKF Structure

Keep OKF files short and concept-only.

Add concepts only when they are durable terms referenced from multiple
surfaces. Avoid procedural instructions in OKF.

Add stable frontmatter where useful:

```yaml
concept_id: agent-doc.session-cycle
type: concept
tags: [agent-doc, session, closeout]
```

### 6. Update Audit Tooling

Extend `audit-docs` to check:

- broken OKF links
- stale installed OKF bundles
- OKF files referenced from instruction surfaces but not bundled
- duplicate managed concept definitions
- concepts duplicated directly in `SKILL.md` or runbooks
- installed harness OKF directories that drift from the running binary

### 7. Measure Token Impact

Capture before/after measurements for:

- default loaded context size
- branch-specific loaded context size
- worst-case loaded context size
- duplicated words removed
- OKF concepts loaded per cycle
- duplicate context expansions suppressed by the ledger

Do not claim token reduction until there are before/after measurements.

### 8. Implement In Small Phases

1. Inventory and ownership report.
2. Move duplicated durable vocabulary into OKF.
3. Add `audit-docs` checks for OKF and context ownership.
4. Add the loaded-context ledger.
5. Enforce selective loading and measure token impact.

## Acceptance Criteria

- `SKILL.md` gets smaller or stays flat while routing improves.
- OKF contains only durable concepts, not procedures.
- `audit-docs` catches stale, missing, or duplicated OKF context.
- Runtime context loading can prove a source was loaded once.
- Token usage has before/after numbers.
- Installed harness surfaces and OKF bundles stay aligned across Claude Code,
  Codex, OpenCode, Cursor, and generic installs.

## Implementation Report

Implemented and measured in
[`tasks/agent-doc/agent-doc-context-audit-okf-report.md`](agent-doc-context-audit-okf-report.md).
