# Haiven SDK-as-Contract Structure — DRAFT (hold until Craig engages)

Status: NOT sent, NOT referenced in detail in the decline. The decline's door-open line
is the hook; this document is the follow-up proposal if Craig picks it up. Purpose:
answer "how could Brian do the engine-SDK work as a contractor where Haiven keeps
control + transparency, and the project actually succeeds."

## Why this is credible for Haiven (their side of the table)

- **The SDK is the ideal contract shape.** Unity/Unreal/etc. integrations are bounded
  wrappers around Haiven's stable API surface: defined deliverables, objective
  acceptance criteria, little need for deep product/roadmap context. Companies resist
  contracting their *core* (the realtime backend); platform SDKs are routinely
  contracted.
- **It converts a thin hiring market into a procurement decision.** Even Brian doesn't
  know a great SDK-fit candidate offhand... that thinness cuts for the contract: waiting
  for the perfect FTE risks the October launch.
- **It is cash-friendlier than the FTE package.** No benefits, no equity, no severance
  exposure; spend is gated milestone-by-milestone... the same milestones (August beta,
  October launch) their remaining funding is gated on.

## Structure (control + transparency by construction)

1. **Milestone-based SOW, one engine at a time.**
   - Phase 0 (small, paid): integration design doc + conformance test plan for engine #1
     (recommend Unity first: fastest iteration loop, largest indie base).
   - Then per-engine milestones: alpha (compiles + core chat/presence flows in a sample
     game) → beta (full API surface + moderation hooks) → launch-ready (docs + samples +
     conformance suite green).
   - Haiven only ever commits to the *next* milestone. Either party exits at a milestone
     boundary; they keep everything paid for.
2. **All work in Haiven's repos from day one.** Their GitHub org, their CI, PRs reviewed
   by their team, issues in their tracker. Work-for-hire assignment for the
   deliverables. Zero lock-in: if Brian disappears tomorrow, they hold reviewed code,
   docs, and a green test suite.
3. **Bounded cadence.** Committed hours band (10-15 hrs/wk), weekly live demo, async
   standup notes. Overlap window already offered (12-5pm ET).
4. **Objective acceptance criteria in the SOW.** Sample-game integration passes the
   conformance suite on target engine versions; API docs shipped per milestone. Payment
   on acceptance, not on hours claimed.
5. **Rate:** $110-140/hr, or fixed-fee per milestone (preferred by cash-tight seed
   companies since it caps exposure). Anchor per-milestone pricing off the hourly band.

## The honest constraint on Brian's side (resolve BEFORE offering)

The entire Acadian decision was purchased with evenings for patchboard/lazily. A
10-15 hr/wk SDK contract spends exactly those evenings. Options, pick deliberately:
- **Don't offer it** (protect the bet-on-myself hours; leave the door-open line as pure
  politeness);
- **Offer Phase 0 + engine #1 only** (bounded ~6-10 weeks, real cash + relationship,
  patchboard resumes after); or
- **Offer referral-first**: help Craig scope the SOW + interview SDK contractors, take
  no build role. Costs almost nothing, high goodwill.

## If bringing it up proactively (optional decline-email line)

Replace the door-open sentence with the stronger variant:

> If a milestone-scoped contract on the engine SDKs would help you hit the launch...
> your repos, your review, spend gated per milestone... I'd genuinely consider that,
> and I'm happy to sketch what it could look like.

Only use this variant if Brian has already resolved the evenings conflict and would
actually say yes... Craig is thin on SDK candidates and is likely to follow up.
