# Methodology Q&A crib (say-able answers)

The questions people actually ask, with answers that fit in one breath.
Use until the clarity-pass UI ships; keep using after — the UI will show
the same facts, you narrate them.

**"What does the sparkline show?"**
Hourly mention counts for that topic over the last 12 hours. A mention
is one post categorized into the topic — comments and votes are
engagement, not mentions. The evidence chart is the same series with
the 7-day baseline drawn as the dashed line.

**"What exactly is a 'mention'?"**
One post, from any of the three sources, whose title or tags match the
topic. Not comments, not upvotes — those measure engagement with a
post; we're measuring how many distinct things are being said.

**"Why is this card ranked #1?"**
rank = trend × your interest weight. Trend = recent velocity (1h/6h/24h
weighted), amplified up to 25% by how unusual the activity is for that
topic (z-score), plus a source-diversity term. Then your fader scales
it. Every number in that sentence is visible in the evidence panel.

**"Where does the z-score come from?"**
It compares this 6-hour window against the topic's own previous week —
27 six-hour buckets, mean and standard deviation. z = how many σ above
its own normal. It's self-normalizing: a quiet topic can spike on five
posts; a big topic needs a real surge. For very quiet topics we floor σ
at 1.0 so tiny baselines can't manufacture huge scores — and the UI
says so when the floor is engaged.

**"How are posts categorized?"**
Curated alias lists matched against title + tags — "wasmtime" maps to
wasm-runtimes. A post can be in several topics; unmatched posts get
keyword-derived topics. Deliberately rules, not ML: misclassification
is possible, but every match is inspectable, which is the property we
care about. (Agent research fills the depth gap for link posts.)

**"Can the AI change the ranking?"**
No — architecturally. Research annotates (verdict, summary, watch
suggestions) but never re-ranks; there's a test asserting digest order
is identical before and after any research submission. The formula is
plain math you can audit; the AI's job is context, not curation.

**"Why only five items?"**
The attention budget — the product's core invariant. You own the number
(3–10 fader), the engine enforces the ceiling, and unbounded is never
an option. Depth is unlimited (evidence, briefs, source links); it's
interruption that's budgeted.

**"Is an HN post worth the same as a Lobsters post?"**
It shouldn't be, and (once source normalization lands) it isn't: mention
weights are computed from each source's own rolling volume — a mention
from a low-traffic source counts for more, clamped so no source
dominates or vanishes. The weights are data-derived, shown live in the
methodology explainer, and votes are never compared raw across sources —
a post's points are read against its own platform's distribution.

**"How fresh is the data?"**
The header shows last ingest; live mode refreshes on an interval with a
hard 120-second politeness floor toward the free source APIs — same
philosophy as the budget, pointed outward.

**"Isn't z misleading when the baseline is tiny?"**
Yes, it can overstate — which is why the σ floor exists, the UI
discloses it inline, and the research agents are prompted to check
exactly this (one live report flagged a z=12.9 as an ingestion-batching
artifact, with citations). The score is an ordinal ranking signal, not
a probability claim.
