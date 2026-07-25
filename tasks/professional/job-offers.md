---
agent_doc_session: a7cd84b1-f38d-4554-92b9-1fbe207b6f9e
resume: c00f9a95-29de-4ded-8b3c-3df9edebe6f3
agent: claude
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
I have two job offers. Acadian Financial Services for $100/hr with a 1 year contract. Haiven where I need to get back with my salary requirements. I estimate that $180k/year could be reached, so let's say $170k-$180k a year. Part of Haiven's offering is 0.5% founder's package which would likely involve revenue share if the company remains private.

Both teams seem great. Haiven has a more fun environment. Acadian is more professional.

Acadian Financial Services:
- Eastern Time
- $100/hr
- 12 month contract
- Probably less overtime work than Haiven
- Stable profitable company
- More time to do open source
- benefits: tasks/recruit/acadian/acadian-benefits-summary.md
- There is the possibility to work on other aspects of the business.

Haiven:
- Pacific Time
- $150k-$180k/yr
- Signing bonus is on the table. Perhaps ~$10k?
- Founder Engineer
- Likely more overtime work than Acadian Financial Services
- Potential for revenue share if the company remains private
- Possibly more flexible work hours
- Less time to do open source
- More interesting + varied work
- SDK work involves Unreal + Unity + other major game engine platforms
- benefits tasks/recruit/haiven/haiven-benefits-summary.md
- I think this team has more spark.
- There is definately opportunity to work on multiple aspects of the business.

What should I ask Haiven for a s/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.mdkeyReturnalary + signing bonus + options/equity?
Please help me analyze the job offers and make a decision.

### Re: Acadian vs Haiven offer analysis — fable-5

(Reading the garbled line as: "What should I ask Haiven for a salary + signing bonus + options/equity?" ... a file path got pasted mid-word.)

**The year-1 money is close to a wash. The decision is really about time horizon and what 2026 is for.**

**Cash comparison (year 1)**

- **Acadian**: $100/hr through PGC (employer of record). $208k at 2,080 hrs, but hourly means unpaid vacation/holidays... realistic ~$188-196k. The PGC benefits sheet shows no employer premium subsidy: worker-only Gold is ~$773/mo (~$9.3k/yr), full family ~$2,358/mo (~$28.3k/yr) out of your pocket. No mention of 401k match or PTO.
- **Haiven**: $180k base + ~$10k signing = $190k year 1. Benefits are subsidized; even the family-tier contribution (~$906-$1,431/mo) beats PGC's family rates. Plus 0.5% founder package.

Net-of-benefits, single coverage: Acadian ~$180-190k vs Haiven ~$185k. Family coverage: Haiven likely wins outright. Then Acadian expires in 12 months with renewal risk, while Haiven is ongoing with equity upside. On pure economics at $180k, Haiven is the stronger offer... **which means the real trade is lifestyle/strategy, not money.**

**The non-money trade**

- Acadian: Eastern time (matches yours), lighter load, bounded 12-month commitment, more open-source time. This is the "fund the runway while building your own products" option... it aligns with your stated patchboard-first sequencing.
- Haiven: Pacific team from an Eastern life means your meeting block starts at noon and the day stretches into the evening, plus more overtime. Less time for your own work. In exchange: more interesting work, founder title, equity, team spark, game-engine SDK work.

Be honest with yourself about one thing: "Founder Engineer + overtime + more spark" and "keep shipping patchboard/lazily nights-and-weekends" don't coexist. Haiven is a choice to make *their* product your main product for a few years.

**What to ask Haiven**

Present it as one package, anchored by the competing offer ("my alternative annualizes at $208k"):

1. **Base: anchor $185k, land at $180k.** You already believe $180k is reachable; anchoring at the top of your range guarantees you land mid-range. Don't open at $170k... that becomes the ceiling.
2. **Signing: ask $20k, accept $15k.** Signing bonuses are the easiest concession for a startup (one-time, no ongoing burn). Frame it explicitly as bridging the year-1 gap vs the $208k-rate contract. Ask about clawback terms (12-month prorated is standard).
3. **Equity: get the 0.5% defined in writing before you weight it at all.** "Likely revenue share if private" is a red flag for vagueness. Ask:
   - Instrument: stock options (ISO), restricted stock, or a contractual profit-share? These are wildly different things.
   - 0.5% of what... fully diluted shares today? What's the strike/409A valuation?
   - Vesting: 4-year/1-year cliff is standard. Early exercise available?
   - Acceleration on acquisition (ask for double-trigger).
   - Revenue-share mechanics: % of what (net? gross?), distribution cadence, who decides, does it survive dilution/new investors?
   - Given the Founder Engineer title, asking for **0.75-1.0%** is reasonable. A good trade to offer: $170k base in exchange for 1.0% + $20k signing, if you believe in the upside.
4. **Non-cash asks** (cheap for them, valuable to you): written **open-source IP carve-out** listing your projects by name (this is critical... a standard invention-assignment agreement at a "founder" role can cloud ownership of patchboard/lazily/tsift), core-hours agreement (e.g., overlap 12-5pm ET, flex the rest), 12-month comp review, and confirm the signing bonus in the offer letter not verbally.

**Recommendation**

Negotiate Haiven fully first... it costs a week and nothing else. Then let the answers decide:

- If Haiven lands at **$180k + $15k+ signing + real equity terms on paper + IP carve-out**, take Haiven. At that package they've beaten Acadian economically and shown they negotiate in good faith, and you've said yourself the team has more spark.
- If they stall at $150-160k, get vague about the equity instrument, or refuse the IP carve-out, take Acadian. The $208k rate, matching timezone, and bounded commitment fund exactly the patchboard-first year you already planned, and a 12-month contract keeps the option to re-enter the market in 2027.

One more diligence item either way: ask Acadian whether the contract is renewable/extendable and whether the rate is renegotiable at renewal... that changes how much weight the "expires in 12 months" risk deserves.
❯ Is $100/hr more like >$200k/year? Please reassess what I should ask for.

### Re: $100/hr annualization reassessment — fable-5

**Yes, you're right... >$200k/yr is the fair way to read it, and I was over-discounting.**

$100/hr x 40 hrs x 52 weeks = $208k. My earlier ~$188-196k assumed 3 weeks vacation plus all holidays unpaid. But a contractor who works steadily and takes ~2 weeks off still bills 50 weeks = $200k. Two things push it even higher:

1. **Hourly pays for every hour.** If the contract lets you bill actual hours (confirm this... some cap at 40), any week over 40 is paid at $100/hr. At Haiven, "likely more overtime" is *unpaid* by construction. A $180k salary at a realistic 48-50 hr founder-eng week is an effective $69-72/hr. That asymmetry is worth naming in your own head even if you don't say it to Haiven.
2. **Your time off is under your control.** The discount only materializes if you actually take unpaid weeks; in a 12-month contract sprint, many people don't.

So the honest year-1 comparison is:

- **Acadian**: ~$200-208k cash, minus ~$9.3k unsubsidized single-tier health (Gold via PGC) → **~$195-199k effective** (single coverage; family coverage knocks off ~$28k instead).
- **Haiven** (at my previous ask): $180k + $10k signing + subsidized benefits → **~$185-190k effective**, plus 0.5%.

That's a real ~$10-20k year-1 gap in Acadian's favor, wider if you bill any overtime. Haiven's compensating assets are the equity and the fact that the job doesn't end in month 12.

**Revised ask for Haiven** — raise the anchors so the package reaches ~$200k year-1 value:

1. **Base: anchor $190k, land $180-185k.** The justification line writes itself: "My alternative pays a $208k annual rate. I'd rather build with you, but I need the package to get close."
2. **Signing: ask $25k, accept $20k.** Size it explicitly as closing the year-1 gap. One-time money is the concession startups grant most easily, so let the signing bonus do the bridging work rather than pushing base past their ceiling.
3. **Package math for them:** $185k + $20k = $205k year 1, or $180k + $25k = $205k. Either lands at parity with Acadian, and the 0.5% + ongoing role is then genuine upside instead of a consolation.
4. **Equity asks unchanged** (instrument, fully-diluted basis, vesting, acceleration, revenue-share mechanics in writing; 0.75-1.0% is still a reasonable counter given the Founder Engineer title). If they can't move cash, trade: $175k base for 1.0% + $25k signing.
5. **Keep the IP carve-out and core-hours asks** ... they're free for Haiven and matter most to you.

And the mirror-image diligence question for Acadian gains weight: **can you bill above 40 hrs, and is the rate renegotiable at renewal?** If both answers are yes, Acadian's effective ceiling is meaningfully above $208k and the bar Haiven must clear rises again.

Decision gate stays the same shape, with updated numbers: Haiven at **$180k+ base, $20k+ signing, real equity paper, IP carve-out** → take Haiven. Haiven stalling below ~$195k year-1 total or getting vague on equity → Acadian funds your patchboard year at the better rate.
❯ I can bill above 40 hours and the rate is renegotiable at renewal.

### Re: billable overtime + renegotiable renewal — fable-5

Those two answers upgrade Acadian from "good 12-month stopgap" to "renewable seat with a rising ceiling":

- **Paid overtime**: 45 hrs/wk x 50 weeks = **$225k**; 50 hrs = $250k. Every extra hour is compensated, versus Haiven where extra hours *reduce* your effective rate. (Counterweight: only count hours you'd actually choose to work. At a clean 40 you're still at $208k... don't let billable overtime become an obligation to work it.)
- **Renegotiable renewal**: the "expires in month 12" risk I was pricing in mostly evaporates. If the engagement goes well you have a built-in rate-raise checkpoint at month 12, with a year of demonstrated value as leverage. The realistic Acadian trajectory is $208k+ year 1, higher after renewal.

**What this means for the Haiven ask: stop trying to reach cash parity and make it equity-forward.** Haiven almost certainly can't pay $210-225k base, and pushing them there sours the negotiation. The honest frame flips to: "I'm not asking you to match my alternative's cash. I'm asking for a package where the equity makes the gap rational."

Revised structure:

1. **Base: $180-185k, anchor $190k** (unchanged). This is already near the top of the range you believe exists; squeezing harder buys little.
2. **Signing: $20-25k** (unchanged). Still the easy one-time bridge.
3. **Equity: 1.0% becomes the *primary* ask, not the fallback.** The cash gap vs Acadian is now structural (~$25-45k/yr depending on your hours), so the equity has to carry real weight to justify it. Say it plainly: "Given my alternative annualizes above $208k with paid overtime, I need the founder package at 1.0% with standard terms (4-yr vest, 1-yr cliff, double-trigger acceleration, terms on paper) for this to work." Settle at 0.75% if base lands at $185k.
4. **Everything else holds**: instrument/vesting/revenue-share mechanics in writing, IP carve-out naming your projects, core-hours overlap for the ET-PT split.

**Updated decision gate:**

- Take **Haiven** only if you get: $180k+ base, $20k+ signing, **~1.0% with clean written terms**, and the IP carve-out... *and* you affirmatively want the multi-year startup bet with evening-shifted hours. The package now needs the equity to be real, because the cash case is gone.
- Otherwise **Acadian**: $208-225k, your timezone, paid overtime, a month-12 raise checkpoint, and the open-source time your patchboard-first plan calls for.

Bluntly: with billable overtime and a renegotiable rate, Acadian is now the stronger default. Haiven has to win on conviction... team, equity terms, and the kind of work... not on money. That's still a winnable case, but it's theirs to make, and their willingness to put 1.0% on paper is the cleanest test of it.
❯ Please assess Haiven...including the founder Luke Richey + the CTO Craig Fryar. I believe they can make $10M a year within a few years. Obviously their business model is up in the air. But please give me an assessment of the likelihood of achieving $1M revenue and $10M revenue.

### Re: Haiven company + founder assessment ($1M / $10M revenue odds) — fable-5

I researched the company and both principals. Quick grounding on what Haiven actually is, then the founders, then honest probability ranges. Caveat up front: this is a judgment call on a pre-revenue-model startup, not a forecast... treat the numbers as calibrated opinion, not fact.

**What Haiven is**

"The Indie Game Platform That Pays." Two things bolted together:
1. **A trust-&-safety SDK** — real-time AI moderation, grooming/predator detection, chat + media filtering, mental-health signal detection, analytics. Drops into Unity, Unreal, Roblox, React Native, Discord. "Zero backend, zero cost" to the developer. This is the game-engine SDK work you'd be doing.
2. **An ecosystem subscription marketplace** — players subscribe once at the ecosystem level and get benefits across participating indie titles; developers share that subscription revenue based on player time/engagement. Explicitly child-safety and faith-mission framed (Jesus.net partnership, Child Safe Tech Alliance).

The important structural fact: **these two halves have very different odds.** The SDK is a proven category; the ecosystem-subscription marketplace is the moonshot.

**The founders — genuinely experienced, not first-timers**

- **Luke + Jennifer Richey** (CEO/COO, married co-founders): previously founded **Gravity Jack** (2009), an AR company that did **$22M+ cumulative revenue** over its life, ~$23M valuation, with Fortune 500 + government clients (Ford, Intel, NASA) plus a patent-licensing division and a crowdfunded game. Read that carefully: $22M *cumulative over ~15 years* is a solid, durable services/IP business... it is **not** a venture rocket or a big exit. It tells you they can build a real company and survive, and that they lean agency/IP/mission rather than hypergrowth. That's a meaningful signal about the *shape* of outcome to expect.
- **Craig Fryar** (CTO): data/analytics leader, ex-**Tableau** (Global Director, Enterprise Strategy) and ex-**Wargaming.net** (World of Tanks — one of the most successful F2P gaming companies) as Chairman/CDO. This is a genuinely strong, credible gaming-data hire. He knows F2P monetization and analytics at scale, which is exactly the hard part of the ecosystem model.

Net read on the team: **above-average execution capability, credible domain fit, but a track record that points to a mid-size durable outcome rather than a $100M rocket.** That's not a knock... it's actually well-matched to the safety-infra opportunity.

**Market context (matters a lot)**

Trust & safety in gaming is a real, funded, regulation-driven market right now: UK Online Safety Act, EU DSA, KOSA, age-verification mandates. Comparable companies are real businesses: Modulate (ToxMod), ActiveFence (acquired Spectrum Labs), GGWP, Hive, Two Hat (acquired by Microsoft/Xbox). So the SDK half sits in a category that *demonstrably* produces multi-million-dollar companies. The headwind: it's **crowded**, and Discord/Roblox/Xbox increasingly build safety in-house.

**Likelihood of $1M revenue (~2-3 yr horizon): I'd put it around 45-60%.**
Reasoning: if they sell the safety SDK B2B (per-seat / per-MAU to developers and platforms), $1M ARR is a very reachable milestone in a tailwind market with a credible team. The main thing that *lowers* it: if they over-index on the novel ecosystem-subscription marketplace, which has a brutal two-sided cold-start problem (need indie devs *and* paying players simultaneously), the road to first $1M gets much longer. Founders who've survived one company before are more likely to find the sellable wedge. Call it a coin-flip-plus.

**Likelihood of $10M revenue (few-year horizon): I'd put it around 8-18%.**
Reasoning: $10M ARR is a genuine venture-scale outcome that the large majority of startups never reach. Two paths get there: (a) the ecosystem subscription model actually works at scale — low probability, unproven model, hard consumer-subscription sell across indie titles whose own monetization is famously weak; or (b) they become a category-leading B2B safety vendor — possible but against well-funded incumbents and platform in-housing. No disclosed large VC round means they likely lack the capital to buy the growth, which caps the fast path. Not impossible, especially with Fryar's F2P chops and regulatory tailwind... but "within a few years" makes it a low-teens bet, not a base case.

**What this means for your equity — the part that actually hits your bank account**

Your instinct that they *could* reach $10M/yr isn't crazy; the issue is that **$10M revenue ≠ your payout**, and the two ways you'd get paid are very different:

- **Equity (illiquid, exit-only):** at $10M revenue, a safety SaaS might carry a ~3-5x revenue enterprise value = ~$30-50M. **0.5% = ~$150-250k on paper; 1.0% = ~$300-500k** — *before* dilution from future raises and *before* liquidation preferences, and only realized on an acquisition/IPO that may never come. At $1M revenue, the equity is essentially nominal.
- **Revenue-share (cash, if it stays private):** this is the more tangible near-term value and the one you flagged. But "0.5% founder package → likely revenue share if private" is doing a *lot* of undefined work. Revenue-share on *what* — gross, net, distributed how often, decided by whom, does it survive new investors? At $10M revenue, even a 0.5% *revenue* share = ~$50k/yr; that's real, but it must be written down precisely or it's worth zero.

So the assessment *reinforces* the negotiation posture from the last cycle rather than changing it: **the expected value of the equity is real but modest and heavily back-loaded, so it cannot be vague.** Push for 1.0%, and — given the Richeys' history points to a "stays-private, profitable, mid-size" outcome more than an acquisition — **weight the revenue-share definition at least as heavily as the equity percentage**, because in the most likely good scenario (private + profitable, no exit) the revenue-share is the only thing that pays you. If they'll define the revenue-share mechanics crisply and put 1.0% on paper, that's genuine upside on top of a credible team. If they keep it hand-wavy, treat the equity as ~$0 and let Acadian's $208-225k cash win.

*Sources: [haiven.gg](https://haiven.gg/), [Child Safe Tech Alliance profile](https://www.childsafetechalliance.org/blog/designing-safety-from-the-start-how-haiven-is-reimagining-gaming-for-kids), [Luke Richey LinkedIn](https://www.linkedin.com/in/lukerichey/), [Craig Fryar LinkedIn](https://www.linkedin.com/in/craigfryar/), [Gravity Jack / Spokane Journal](https://www.spokanejournal.com/articles/15619-gravity), [Gravity Jack Crunchbase](https://www.crunchbase.com/organization/gravity-jack). Company financials/funding are not publicly disclosed; revenue-model details are drawn from Haiven's own and the Child Safe Tech Alliance's descriptions.*
❯ Please reassess Opus' answers. What do you think the likelihood to reach $100M ARR?

### Re: reassessment + $100M ARR odds — fable

> **Queue prompt:** do [#reassessopusanswers]

I re-examined the prior assessment line by line. Most of it holds; two numbers deserve adjustment, one detail was under-weighted, and then the $100M question has a pretty crisp answer.

**Where the prior analysis holds up**

- Founder read: correct and well-calibrated. Gravity Jack ($22M *cumulative* over ~15 years, no exit) is the single most predictive datapoint on the table... it says "durable mid-size builder," not "rocket." Fryar at Wargaming/Tableau is a genuinely strong CTO signal.
- $10M at **8-18%**: I'd keep this range. It brackets the honest uncertainty between "credible team in a tailwind category" and "unproven two-sided model without disclosed venture capital."
- The equity/revenue-share framing (paper equity is exit-gated; revenue-share is what pays in the most-likely good scenario) is the right decision lens and I re-endorse it as the core of the negotiation posture.

**Where I'd revise**

1. **$1M odds: trim from 45-60% to ~40-55%.** The prior analysis leaned on "sell the safety SDK B2B" as the reachable wedge, but under-weighted Haiven's own positioning: the SDK is marketed as **"zero cost" to developers**. If the SDK is genuinely free, the B2B wedge isn't the plan... the *ecosystem subscription* is the only revenue line, and that's the moonshot half with the cold-start problem. The 40-55% still assumes they'd pivot to charging platforms/studios if subscriptions stall (teams this experienced usually do), but the stated model starts revenue later than the prior estimate implied. **This is resolvable with one interview question: "Who pays, and when does the first dollar arrive?"** Their answer moves this number more than any research can.
2. **Valuation multiple: 3-5x is conservative.** A growing safety-infra SaaS could fetch 5-8x. Bumps the 1.0%-at-$10M paper value toward $500k-800k pre-dilution. Directionally friendlier to Haiven, but it doesn't change any conclusion because it's still exit-gated.
3. **Under-weighted factor: the faith-mission positioning cuts both ways.** Jesus.net partnership + Child Safe Tech Alliance gives real distribution into faith/family networks and brand trust with parents — a genuine wedge for the subscription model. It also niches the brand in a way that may cap mainstream studio adoption of the SDK. It widens the variance in both directions more than the prior text acknowledged.

**$100M ARR likelihood: ~0.5-2%. Call it ~1% over a decade, and well under 1% "within a few years."**

Reasoning from base rates and category evidence, not vibes:

- **Category ceiling:** no gaming trust-&-safety pure-play has publicly reached $100M ARR. The category's best-funded leaders (ActiveFence, Modulate, GGWP, Hive's gaming slice) are estimated in the $10-50M range, and the biggest buyers (Microsoft, Roblox, Discord) keep acquiring or building in-house rather than paying at scale. Hitting $100M via B2B safety means becoming bigger than everyone who has ever tried in this category, against incumbents with 9-figure war chests.
- **The consumer path:** $100M ARR via the ecosystem subscription means ~1-1.5M paying households at ~$6-8/mo across *indie* titles — effectively building a "Netflix of safe indie gaming." Consumer subscription platforms at that scale are ~1%-conditional-success bets even *with* major VC backing, which Haiven has not disclosed.
- **Capital reality:** $100M ARR companies almost universally burn $50-150M getting there. A company with no disclosed institutional round doesn't have the fuel; reaching it would require multiple large future raises (diluting your 0.5-1.0% substantially along the way, likely 2-3x) *and* the model working.
- **Founder-prior:** the Richeys' revealed style (profitable, patient, mission-first, 15 years to $22M cumulative) is close to the opposite of the burn-to-$100M playbook.

**Does the $100M scenario change your decision? No — and here's the EV math that proves it.** Suppose $100M ARR happens: valuation perhaps $500M-1B; your 1.0% diluted through the raises that funded the run is realistically 0.3-0.5%, worth ~$1.5-5M. Multiply by ~1%: **expected value ~$15-50k.** That's roughly one signing bonus. The tail is real enough to want the equity *on paper with clean terms*, but nowhere near big enough to accept a cash discount for it, work unpaid overtime for it, or skip the revenue-share definition. Every prior conclusion survives the reassessment: equity-forward ask (1.0%, written terms, revenue-share mechanics precisely defined), signing bonus does the cash bridging, and Acadian remains the stronger default that Haiven must beat on conviction.

One framing note for your own head: you don't need to *believe* in the $100M case to take Haiven, and you shouldn't take Haiven *because* of it. The bet you'd actually be making is the 40-60% world where they build a real $1-10M mission business you enjoy working in, with a revenue-share that pays something and a small chance of more. Price the offer against that world, not the tail.
<!-- agent:boundary:8d396312 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority start -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] 🚧 [#reassessopusanswers] Please reassess Opus' answers. What do you think the likelihood to reach $100M ARR?
- [ ] [#haivenask] #haivenask Draft the Haiven compensation reply (equity-forward frame: base $180-185k anchor $190k + signing $20-25k + 1.0% primary equity ask with clean terms; cash parity with $208k+ alternative explicitly not expected) as an email/message draft for review
- [ ] [#ipcarveout] Prepare an open-source IP carve-out exhibit (lazily, tsift, agent-doc, patchboard, tmux-router, corky, etc.) to attach to whichever agreement is signed
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
