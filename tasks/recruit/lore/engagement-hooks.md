# Community Pulse — Engagement & Viral Hooks

The case prompt asks for engagement *without overwhelm*. That constraint is
the strategy: every hook below preserves the 5-card attention budget and
no-infinite-scroll rule. The pitch line for the panel: **"we make leaving
easy and returning valuable — retention through scarcity, not capture."**
(Lore is a health company; anti-doomscroll mechanics are on-brand.)

## Return hooks (habit)

1. **The Daily Pulse ritual** — one push notification per day at a
   user-chosen time: "Your pulse is ready — local-first is spiking." Never
   per-event spam; the hard cap makes it a ritual (open, absorb 5 cards,
   done in 90 seconds). The mobile answer to "a compelling reason to check
   in daily."
2. **The delta, not the feed** — on return, the app leads with what changed
   since *your* last visit: "2 trends entered your mix · rust cooled −1.1σ ·
   iroh still climbing (day 3)." The reward for coming back is a diff, not
   a fear of missing out. Cheap to build: store last-seen digest, render a
   delta chip row. **Demoable.**
3. **Tracked-topic alerts** — `subscribe_topic` pays off: an interrupt ONLY
   when a tracked topic crosses a z threshold ("WASM runtimes just spiked
   +3σ"). Rare by construction → each alert stays high-signal. Already
   wired to existing engine state. **Demoable** (trigger with fixture).
4. **Voice check-in** — 30-second spoken briefing ("give me the pulse")
   for the commute/coffee habit slot. Mobile is where this earns its place.

## Viral loops (acquisition)

5. **Shareable trend cards** — one tap renders a branded card image
   (headline + sparkline + z-score + "spotted on Community Pulse") for
   X/LinkedIn/Slack. People share taste signals; the card is the ad.
   **Demoable**: render the card to PNG from the engine data.
6. **Early-spotter credit** — "You tracked iroh 12 days before it hit #1."
   Timestamped `subscribed_at` already exists in the schema; surfacing it
   creates the collectible people screenshot. Weekly "early spotters"
   recognition per community. Taste-flex is the share motive (cf. Product
   Hunt hunters, Spotify Wrapped).
7. **Shareable mixes** — export/clone interest mixer configs: "Follow
   Brian's systems-programming mix." Curators become distribution; a
   cloned mix is a pre-onboarded new user.
8. **Weekly Wrapped** — "Your week in the pulse": top trend you caught
   early, your mix drift, one stat. Shareable recap, one notification.

## Contribution loop (ownership)

9. **Signal/noise vote** — one-tap "signal" / "noise" on each card feeds
   ranking (a light personalization + community prior). After voting, show
   the community split — the curiosity gap ("am I with the crowd?") is the
   comeback hook. Guardrail: votes tune, never override, the transparent
   z-score ranking.

## Growth: 0 → 100k (channel ranking + sequencing)

Ranked honestly: **viral loops > content flywheel > agent-native
distribution > organizations > advertising.**

- **Loops lead** because every loop ships an artifact that carries a real
  insight — the artifact IS the ad. Cloned mixes are the strongest:
  a respected curator's shared mix pre-onboards every cloner with a
  working interest model on day zero. Curators become distribution.
- **Content flywheel, nearly free**: a public weekly "state of the
  pulse" digest — the engine already produces it; SEO + newsletter. The
  router posture (we send readers TO the communities) buys legitimacy in
  the exact places the first 1,000 users live.
- **Agent-native distribution (novel)**: MCP connector directories are
  new app stores with early-mover discovery — "the trend-research
  connector your agent already knows" is a wedge that didn't exist 18
  months ago; the architecture is already compatible.
- **Organizations = revenue + retention, not acquisition** — B2B follows
  individual love (bottom-up motion). Exception: team invites are a
  loop; one convert pulls a team for the coverage map. Orgs enter the
  10k→100k leg, monetizing what the loops built.
- **Advertising: mostly no.** Dev audiences are ad-skeptical, CAC will
  embarrass a cheap/free tier's LTV, and paid spend before K-factor is
  measured is buying ignorance. Narrow later use: trusted dev
  newsletters/podcasts once unit economics exist.

Sequence: **0→1k** genuinely-useful launches in the source communities →
**1k→10k** share cards / early-spotter / cloned mixes + content
flywheel (tune K-factor, share→install) → **10k→100k** curator
ecosystem + connector directories + bottom-up org expansion.

## Metrics slide (say the numbers out loud)

- Habit: D1/D7/D30 retention, median time-in-app (target: LOW — under two
  minutes is a *feature*, argue it explicitly), notification opt-in rate.
- Viral: K-factor from card/mix shares, share→install conversion,
  % of new users arriving via a cloned mix.
- Health: alert precision (alerts opened / alerts sent), mute rate on
  notifications as the overwhelm early-warning signal.

## What to actually demo vs. slide

- **Demo (cheap, high impact):** delta chips on open (#2), tracked-topic
  alert firing from fixture data (#3), share-card PNG render (#5).
- **Slides only:** daily notification (#1 — OS plumbing, zero interview
  value), early-spotter (#6 — show the SQL that already supports it),
  mixes (#7), Wrapped (#8), voting (#9 — mention the ranking-integrity
  guardrail; interviewers probe gaming/abuse here).

Abuse note for Q&A: share cards and cloned mixes carry no user-generated
free text (headline comes from source posts), which closes most spam/brand
-abuse vectors by construction; votes are rate-limited per account and
shape only the voter's own personalization prior plus a heavily-damped
community term.
