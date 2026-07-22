# Community Pulse — Solution Brief

Case Study 2 (TrendForge) · Brian Takita

TrendForge's 10,000 users can't watch everything. Community Pulse is my
answer to what they should see instead: **the five things moving in their
community right now, with the evidence to judge each one — and nothing
else.**

## The decision the case asks for

"You need to decide what Community Pulse actually means." I chose
**topic-level trends with per-trend evidence**, over the alternatives:

- *Trending posts* — too raw; ten posts about one story is one signal.
- *Most active users* — a popularity contest, not insight.
- *Sentiment analysis* — opaque; a user can't audit a sentiment score.

Topics + evidence is the only option a user can **verify**: every card
shows its velocity, its 7-day baseline, its z-score, and the source posts
behind it — each linking out to the original thread. The pulse routes you
outward to the community; it is not a walled garden.

## The organizing principle: an attention budget

The digest is capped — **five cards by default, and the user owns the
number** (a master fader on the mix, 3–10). The invariant is not "five";
it is that **a budget always exists, the user sets it, and unbounded is
never the default** — enforced in the engine (`limit.clamp(1,
MAX_BUDGET)`), not the UI, so no surface (app, chat agent, or external
tool client) can exceed the ceiling. When a trend earns a slot, another
loses one. Scarcity forces ranking quality and converts "checking the
pulse" into a 90-second ritual instead of a scroll session.

Wanting to go deeper is a different job with different physics: **push is
budgeted, pull is not.** Evidence panels, source links, and the chat
agent give unlimited research depth on demand — the budget only governs
what the app pushes at you.

We treat **low time-in-app as a success metric**. Engagement comes from
returning tomorrow (delta-on-return, tracked-topic alerts that fire only
on a z-score threshold, one daily digest notification), not from staying
today. Full loop design + metrics: `engagement-hooks.md`.

## The four challenges, answered

### 1. Surfacing insight without flooding users

Structurally, not editorially: the attention budget above, an interest
mixer the user controls (boost/mute per topic — mutes are absolute),
deltas instead of feeds on return, and alerts that are rare by
construction. The cap is a line of code I can point to.

### 2. Holding up at 100,000+ users

The load-bearing observation: **trend computation scales with content,
not users.** A 100k-user community produces perhaps 5–20k posts/day —
topic counting and z-scores over that corpus stay trivially cheap no
matter how the user count grows. What scales with users:

- **Personalized rerank**: top-N shared candidates × per-user interest
  affinity — a dot product at read time, microseconds; interest vectors
  in a KV store. No per-user trend computation, ever.
- **Notification fan-out**: 100k daily digests is a batch job (~30
  sends/sec spread over an hour). Threshold alerts scale with *topics
  crossing thresholds* × subscribers — capped by construction.
- **Storage**: ~20k posts/day ≈ 20 MB/day normalized. SQLite → Postgres,
  streaming topic-window counters replace recompute. Single region,
  boring by design — at this scale the interesting limit is topics ×
  windows, not users.

Abuse at scale: the trend formula already weights **source diversity**,
so a coordinated pump on one platform under-scores; add per-account
ingest rate limits and heavily-damped community terms for any future
voting. Cost: the ranking needs no LLM — math only — so the marginal
cost of a reader is ~zero; LLM spend is opt-in chat, per active user.

### 3. Recommended architecture, and the constraints that matter

One engine, N lenses — running today, not proposed:

```
       Slint UI (desktop + phone-frame)      chat agent      MCP clients
                    └──────────────┬──────────────┴───────────────┘
                          shared tool bridge (4 tools)
                     get_pulse · set_interests · explain_trend · subscribe_topic
                                   │
                pulse engine — SQLite, velocity windows, z vs 7-day baseline
                                   │
                 ingesters: Hacker News · Lobsters · Product Hunt
```

Every lens — UI clicks, the chat agent's tool calls, and external agents
over MCP — writes the same reactive state cells, so surfaces can never
disagree. Production swaps the edges (web frontend, Postgres, precompute
workers, LiveKit if voice ships) and keeps the shape.

**Client/server boundary in production — distill centrally, personalize
locally.** The prototype is deliberately client-only (zero infra, offline
demo). Production splits on the same line as the scaling argument: the
service layer owns everything content-scaled — ingestion, topic
extraction, window counters, z-scores — computed once per window and
published as a user-independent **candidate set** (cacheable, CDN-able).
The client owns everything user-scaled: interest vector, rerank, budget,
tracked topics, deltas. Two consequences worth noting: the interest
mixer never has to leave the device (personalization is private by
architecture), and the migration is an extraction, not a rewrite — the
engine crate has no UI dependencies, so the service layer is the
prototype's engine deployed out-of-process.

**Agent research at scale** follows the same pattern: the MCP tool
surface the prototype exposes IS the service API contract. Server-side,
a research-request queue feeds a worker pool of API-key-billed agent
runs against the same nine tools (the product absorbs metered inference
cost — the mass-market tier), while power users keep local CLI
delegation and BYO-subscription users connect their own Claude/ChatGPT
apps via a remote MCP connector — three billing tiers, one tool surface.
Per-user state (mix, budget, tracked topics, reports) syncs across a
user's devices through the same contract, so desktop↔mobile is state
sync, not a bespoke bridge.

**Connectivity as a service (the relay tier).** Between "pure local" and
"vendor computes everything" sits a third deployment shape: the desktop
stays the user's personal compute node (local data, subscription-billed
CLI agents), and the vendor runs only a **zero-knowledge relay** so
mobile reaches the desktop with no user networking setup — desktop holds
an outbound connection (no port forwarding), devices pair via QR with
their own keys, traffic is end-to-end encrypted (the relay moves
ciphertext), and same-LAN traffic bypasses the relay entirely. The
vendor sells convenience bytes, not access to data or inference. This
extends "personalization is private by architecture" across the network
hop, and gives users a spectrum — local → relay-connected personal node
→ full service — along one unchanged tool contract. In short: the
prototype's in-process seams become the production network boundaries —
nothing is re-architected, only re-deployed.

**AI access tiers (who pays for inference, and how).** The ranking needs
no LLM, so AI is opt-in cost. Four compliant tiers behind one billing
abstraction: (1) *power users* — local CLI delegation (Claude Code /
Codex) bills their own subscriptions and grants local-data access; the
right fit for a developer community, but a technical barrier for mass
market. (2) *mass market, service-metered* — the service runs inference
on its own API keys and prices it into the product; users need no AI
account at all. (2b) *mass market, BYOK* — the user supplies their own
provider API key and pays the provider at cost; the service provides
only orchestration. This is the sanctioned third-party path under both
vendors' ToS, and the prototype already implements it (the chat layer
takes any key against any OpenAI-compatible endpoint) — productizing it
is key custody (encrypted at rest, or client-held so the key never
touches the service) plus per-key spend caps. (3) *bring-your-own-
subscription without a CLI* — expose the pulse as a remote MCP
connector, so users' own Claude/ChatGPT apps (mobile included) become
lenses, with inference billed to their subscription inside first-party
surfaces — the only ToS-clean subscription-BYO path today. Tiers can
shift as vendor programs mature; the abstraction is the point.

Constraints, ranked by how much they shaped the design:

1. **Attention budget** — the product invariant; engine-enforced.
2. **Auditability** — transparent scores, visible formula, evidence
   posts, outbound links. No black-box ranking.
3. **Determinism** — fixture dataset + replay agent; every behavior is
   testable and the demo survives a dead network.
4. **LLM-optional** — the pulse is fully functional with the agent off;
   AI is a lens, never a dependency.
5. **Abuse resistance** — source diversity in the score; damped social
   inputs.
6. **Privacy** — the interest model is explicit and user-owned, not
   inferred surveillance.

### 4. The simplest version that tests the hypothesis

The hypothesis: *velocity × baseline-deviation × explicit interest,
capped at five, yields a digest a user would trust.* The simplest test
is a terminal command, and it was milestone 1:

```
pulse --fixture top --json
```

Everything after it — UI, chat, mobile frame, MCP — was an additive
demo beat on an already-validated core; each milestone was non-blocking
by construction (`community-pulse-plan.md` § Implementation status).

## What I'll demo

A 90-second core: ask the pulse → five cards; retune the mix by chat and
by fader — same state; expand a card to its evidence and click through
to the source thread; track a topic. Encores as interest allows: the
phone-frame mobile view sharing live state with desktop; an external
Claude Code session driving the visible window over MCP; the design
mockups this was built against.

## Organizations (roadmap, not demoed)

The same primitives compose into a team tier. **Declared interests
become a coverage map**: when teammates can see that someone already
tracks wasm-runtimes, the team's collective attention budget divides
instead of duplicating — alerts route to the person who watches that
topic, and gaps ("nobody is covering databases") become visible. Shared
artifacts follow naturally: research reports and article briefs land in
a team library instead of a personal drawer, annotatable notes attach to
trends, and mixes become named team assets ("the platform-team mix")
that onboard a new member's lens in one click. The mechanics are the
existing ones — interests, `submit_research`, budgets — with an org
scope and permissions added; nothing about the engine or the tool
surface changes. This is also where the business likely lives: teams
pay for shared attention coverage, the individual product stays cheap
or free.

## Beyond this case

The same engine pointed at one person's captured thoughts and tracked
intentions — with news/life signals as the ingest — is Case 1's Spark:
a personal pulse under the same attention budget. One slide, same
architecture, different lens.

## Artifact index

- Working app: `community-pulse` repo (Rust · Slint · SQLite · lazily) —
  `make demo`, `make demo-mobile`, tests green via `make check`.
- Design mockups: desktop + mobile (in-repo under `docs/design/`).
- Engagement & viral loops: `engagement-hooks.md`.
- Build/status plan: `community-pulse-plan.md`; mobile: `mobile-demo-plan.md`.

## Panel lens map (private prep note — not part of the shared brief)

- **Technical interviewer** → architecture + scaling math (§3, §2), MCP
  beat, engine internals, honest-data rules (no fabricated sparklines).
- **Product-leaning interviewer** → §"decision" + attention budget +
  engagement-hooks metrics; mockups; the 90-second core only.
- **Collaboration/process lens** → the workflow itself: specs written
  for an async implementing agent, de-risking milestone order,
  deterministic fixtures, status-tracked plan. The process is the
  exhibit; say it explicitly.
