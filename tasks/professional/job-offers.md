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
queue: start
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

### Re: do #haivenask — Haiven compensation reply drafted — fable

> **Queue prompt:**
>
> do [#haivenask]


Executed `#haivenask`: the compensation reply draft is written and committed at **`tasks/recruit/haiven/compensation-reply-draft.md`** (commit `833663a`). Nothing is sent... it's a draft for your review. (`[#reassessopusanswers]` was also closed out; its analysis is committed above.)

**The package it proposes** (addressed to Craig; swap the greeting if the conversation is with Luke):

- **Base: $185,000** — one deliberate calibration: I opened at $185k rather than the $190k anchor from the earlier analysis, because Craig's original job posting banded the role at $80-100k before it evolved into the Founder Engineer conversation; $190k risks reading off-scale, while $185k still anchors the top of your believed range. Edit up to $190k in the draft if you want the aggressive anchor.
- **Signing: $20,000**, framed explicitly as the one-time bridge against a competing offer "annualizing above $205k in cash," with the plain statement that you're not asking them to match cash... you're asking for the equity to make the gap rational.
- **Founder package: 1.0%**, 4-year vest / 1-year cliff, with the written-terms request: instrument, fully-diluted basis, and revenue-share mechanics (percentage of what, what cadence) if Haiven stays private.
- **Offer-letter items**: IP carve-out for your pre-existing open-source (list to follow via `#ipcarveout`), and 12-5pm ET core-hours overlap.

The draft file also carries negotiation notes for when they counter: floor at $180k + $15k + 0.75% written; the $175k-for-1.0%+$25k trade if they're base-constrained; and the tell to watch — specific answers to the written-terms request means good faith, vagueness means weight the equity at ~$0 and re-run the decision gate.

Review the draft, edit numbers to taste, and it's ready to paste into email or LinkedIn. `#ipcarveout` remains the next actionable item and should be produced as soon as Haiven engages on the framework.

### Re: #ipcarveout — future side-work carve-outs, and which company makes it easier — fable

> **Queue prompt:**
>
> [#ipcarveout]: I plan on working on side projects. Can I have future side-work as a carve-out? Would it be easier with Acadian than with Haiven?


Short answers: **yes, future side-work can be carved out, but it requires a specific clause beyond the standard pre-existing-IP exhibit... and it's meaningfully easier with Acadian than with Haiven.** I also went ahead and produced the deliverable: the carve-out exhibit is committed at **`tasks/recruit/ip-carveout-exhibit.md`** (commit `0d51caf`), ready to attach to either agreement.

**Why the standard exhibit isn't enough for you.** The usual "Exhibit A" only protects what exists *before* your start date. Your plan is ongoing side projects, so the thing that matters is the *future-inventions* clause. Default employment IP agreements assign anything "related to the company's business or anticipated R&D"... and one more thing that matters for you specifically: **Virginia has no statutory invention-assignment protection** (no California §2870 equivalent), so the contract text is your entire protection. The exhibit I drafted therefore has two parts: the pre-existing list (lazily family, agent-doc, tsift, patchboard, tmux-router, corky, and the rest, each with "and successors" language), plus two tiers of future-works clause: Tier 1 (company claims nothing built on your own time without company resources, *even if related to their business*) and Tier 2 fallback (a tightly-scoped "Excluded Field" definition that expressly excludes developer tooling, reactive-state, and CRDT/sync libraries). Plus a red-line list of clauses to strike from their draft.

**Acadian: structurally easy.** Two reasons. First, it's a 12-month *contract* engagement through PGC... market-standard contractor terms assign only the work product delivered under the engagement, not everything you invent during the term. Second, zero domain overlap: Acadian is financial services; your projects are developer infrastructure. Nobody at a lender is going to claim a reactive-signals library. Action items are just hygiene: read PGC's assignment clause, confirm it's deliverables-scoped, strike any "all inventions during the term" language, attach the exhibit as a precaution.

**Haiven: negotiable but genuinely harder, and higher-stakes.** A Founder Engineer PIIA will default broad, and here the domain overlap is real: lazily (reactive/CRDT), agent-doc (realtime multi-replica sync), and patchboard sit *adjacent to Haiven's actual product* (realtime chat/presence SDK infrastructure). Under a default "related to the Company's business" clause, next year's lazily release or patchboard iteration is arguably assignable... which is exactly why the future-works clause, not just the exhibit list, is the non-negotiable part with them. Open with Tier 1; fall back to Tier 2 with a narrow Excluded Field ("AI chat moderation and player-safety systems for multiplayer games", not "realtime infrastructure"). Their response to this ask is another good-faith tell, same as the equity terms.

**One non-legal reality check:** the carve-out protects your *ownership*, not your *hours*. Haiven's expected overtime reduces side-project time no matter what the contract says; Acadian gives you both the cleaner legal position and the actual evenings. If ongoing side-work is a first-class goal for 2026, that compounds the earlier "Acadian is the stronger default" conclusion.

`#ipcarveout` is done: exhibit drafted, committed, and applicable to both offers. Recommend a one-hour employment-attorney review of whichever final agreement you sign... cheap insurance given Virginia's lack of a statutory floor.
Should I mention the IP carve-out in my Haiven offer? I told the recruiter for Arcadian that I will have an answer by Sunday evening. What would happen if they choose to fire me early?

### Re: carve-out timing, the Sunday deadline, and early-termination risk — fable

Three questions, three answers.

**1. Should you mention the IP carve-out in the Haiven reply? Yes... it's already in the draft, at the right weight.** The compensation draft mentions it as one of "two smaller items for the offer letter," which is exactly the correct posture: surface it early so the PIIA doesn't arrive as a surprise fight, but don't lead with it or paste legal language into a compensation email. The detailed two-tier clause work stays in your back pocket (`tasks/recruit/ip-carveout-exhibit.md`) until they send the offer letter. I made one wording upgrade to the draft (commit `d5241ee`): it now says "pre-existing open-source projects **and future independent side work**"... six extra words that put the future-works ask on the table from day one, so when Tier 1 language shows up later it's a follow-through, not an escalation. Mentioning it early is also cheap signal-gathering: a company that flinches at a routine carve-out mention was going to be a hard IP negotiation anyway, and you want to know that before Sunday, not after.

**2. The Sunday-evening deadline: send the Haiven email today and put the deadline in it.** Today is Saturday; the ask email hasn't gone out, so the clock is the real risk, not the negotiation. I added a timing paragraph to the draft: "I owe my other party an answer by Sunday evening. If you can respond on the framework above by then... even informally... I can hold my decision open for you." A deadline you name honestly is leverage, not rudeness... it forces a fast good-faith signal from Haiven, which is exactly the tell your decision gate needs. Then the branches: **(a)** Haiven engages by Sunday with real numbers → run the decision gate as planned. **(b)** Haiven asks for a few more days → call the Acadian recruiter Monday morning and ask for 2-3 days; recruiters grant short extensions routinely, especially on contract seats, and "another offer is finalizing terms" is a normal reason. **(c)** Haiven goes quiet → that silence *is* the answer; take Acadian on schedule. One thing not to do: don't accept Acadian Sunday and keep negotiating Haiven... reneging on an accepted contract burns the recruiter relationship and the agency channel behind it.

**3. What happens if Acadian fires you early: the honest answer is the 12 months is an expectation, not a guarantee.** Standard staffing/EOR engagements are terminable for convenience with short notice... typically 1-2 weeks, sometimes zero... and your W-2 through PGC is at-will. If they cut you at month 3, you keep what you billed, there's no severance, PGC benefits end (COBRA-style continuation or marketplace), and as a W-2 employee you'd generally qualify for Virginia unemployment while you re-search. So the $208-225k figure should be read as "rate x months they actually keep you." Three mitigations: **(a)** read the actual termination clause before accepting and ask the recruiter directly what the notice period is... this is a normal question that costs nothing; **(b)** if it's bare (immediate/for-convenience), ask for a 2-4 week notice period in the SOW... modest, often granted; **(c)** weigh the realistic risk: a stable, profitable company that just ran a hiring process rarely cuts a productive contractor early... the real risk is a project cancellation or finance-sector budget freeze, not performance whim. Also remember it's symmetric: the same clause is your exit door if Haiven-like opportunities reappear or the engagement sours. And for calibration, Haiven is at-will too, with startup-survival risk on top... neither offer guarantees duration; they just fail differently. Added `#acadianterms` to backlog [recommended]: get the notice clause plus the >40hr-billing and renewal confirmations in writing from the recruiter before you accept.

Practical sequence for the next 30 hours: send the Haiven email today (the draft is ready at `tasks/recruit/haiven/compensation-reply-draft.md`), ask Acadian's recruiter the notice-period question in parallel, and let Sunday evening resolve the tree.
I meant, what happens if Haiven fires me early in regards to the offer?

### Re: what happens if Haiven fires you early — fable

Got it... the question was about Haiven, not Acadian. Here's what early termination does to each component of the offer as currently shaped, then the protections worth negotiating... because the honest answer is that **the current shape leaves you badly exposed in exactly the window where startup firings happen.**

**Component by component, if they terminate you at month N:**

- **Salary:** you keep what you earned through the termination date. Virginia is at-will with no statutory notice or severance... absent a contract clause, the default is zero severance, effective immediately. As a W-2 employee you'd generally qualify for Virginia unemployment.
- **Signing bonus ($20k ask):** depends entirely on the clawback clause. The common default is "repay if you leave within 12 months"... make sure it reads "repay only on *voluntary* resignation or termination *for cause*." If they fire you without cause at month 4 under a bare clawback, you could owe the bonus back while unemployed. No clawback clause at all = you keep it.
- **Equity (the big one):** with a standard 1-year cliff, **fired at month 11 = zero equity. Nothing vests before the cliff.** After the cliff you keep the vested fraction (fired at month 18 on a 4-year schedule = ~37.5% of the grant) and forfeit the rest. If the instrument is options, there's a second trap: the typical 90-day post-termination exercise window means you must pay the strike price within 3 months of being fired to keep even the vested part... while job-hunting.
- **Revenue share:** the component most likely to simply *evaporate*. A contractual revenue share tied to employment almost certainly ends with employment unless the paper says otherwise. Since the revenue share is the piece most likely to ever pay you (per the $100M analysis... the Richeys' pattern is private-and-profitable, not exit), "does the revenue share survive termination without cause, at least for the vested fraction?" belongs on the written-terms question list.
- **Benefits:** end at termination; COBRA-style continuation at full cost or marketplace.

**The asymmetry to hold in your head:** if Acadian cuts you early, you lose future billings and walk with everything you earned. If Haiven cuts you before month 12, you lose future salary *and* the entire equity package *and* likely the revenue share... after having given up a $208-225k alternative. And the realistic early-firing scenario at a startup isn't performance... it's a runway crunch or pivot, exactly when severance promises are hardest to collect.

**What to negotiate (all standard asks for a founder-title hire):**

1. **Vesting: monthly with no cliff, or cliff waiver on termination without cause.** The cliff protects the company from bad hires who quit; it should not let them fire you at month 11 for free. "Founder Engineer" is exactly the title that justifies founder-style vesting. Fallback: prorated cliff credit if terminated without cause.
2. **Signing-bonus clawback scoped to voluntary exit / for-cause only.** One sentence; costs them nothing if they intend to keep you.
3. **Modest severance without cause: 2 months salary.** Frame it plainly: "I'm walking away from a $208k contract seat; I need downside symmetry."
4. **Extended post-termination exercise window** if options (ask 5-10 years; even 12 months beats the 90-day default).
5. **Revenue-share survival language** for the vested fraction.

Their reaction to #1 and #3 is the strongest good-faith tell yet: a company that expects to keep you has no reason to defend a free-firing window. If they refuse all downside protection *and* stay vague on equity terms, the Haiven package is "at-will salary below your alternative plus a lottery ticket they can void"... and the decision gate resolves to Acadian on its own.

### Re: do #acadianterms — recruiter questions drafted — fable

> **Queue prompt:**
>
> do [#acadianterms]


Executed the agent-side of `#acadianterms`: the recruiter message is drafted and committed at **`tasks/recruit/acadian/recruiter-questions-draft.md`** (commit `69f3a4c`)... three written-confirmation questions (notice/termination clause, >40hr billing mechanics, renewal + rate renegotiation) framed around your Sunday-evening answer, with a counter ready if notice comes back as "immediate" (ask for 2-4 weeks in the SOW). The item is now gated rather than done because its completion condition is external: you send it, and the recruiter's written answers land. Unblock = paste their reply into `tasks/recruit/acadian/` and the decision gate updates.
❯ I would like to ask Acadian for either a small signing bonus or an advance on the first paycheck.
❯ Should we include the termination protection (severence + clawback protection). Craig mentioned a Founder's Package...not necessarily a Founding Engineer. Please interpret this email.

```
Brian,

Thanks for your time on the phone and your continued candor about the offers you are considering. It came to mind that might be useful to understand that since January we have raised ~half of our target Seed Series, with additional funding coming in as we hit our August Beta, initial Founder's Pack revenue generation, and October launch milestones. 

I believe it is important to underscore that we are funded through launch and beyond. Our VC and Angel Investors, plus anticipated industry partner funding, will continue providing us with a solid runway as we launch and ramp.

I'm standing by to hear about where Haiven needs to be regarding base compensation and options, along with any questions you have on the offered benefits. We are flexible, including the possibility of offering you a one-time signing bonus.

Thanks very much,

Craig Fryar, CTO
Austin, Texas
```

### Re: Craig's email interpreted + revised asks for both companies — fable

Craig's email is the best development yet... and it contains one detail that quietly reframes the equity conversation. Both drafts are revised and committed (`9e140ba`). Point by point:

**Interpreting the email.** Four signals:

1. **This is strong good-faith engagement.** Unprompted funding transparency, an explicit invitation to name your number, signing bonus volunteered before you asked, and "we are flexible." That's the tell the decision gate was watching for... he's negotiating, not stalling.
2. **"Raised ~half of our target Seed" cuts both ways.** The honest read: the other half is *not raised*, and the remainder is tied to milestones (August beta, Founder's Pack revenue, October launch). "Funded through launch and beyond" leans partly on *anticipated* industry-partner funding... anticipated is not committed. So: real runway confidence, but conditional. This is exactly the profile where the cliff-waiver and clawback protections matter... and now you can justify them from *his own disclosure* rather than generic startup caution.
3. **"Base compensation and options"** ... he said options. That likely answers the instrument question (stock options, not restricted stock or a bare contractual rev-share). Good: options with defined terms is a real thing you can evaluate.
4. **The detail that matters: "Founder's Pack revenue generation" is a *product*.** A Founder's Pack in gaming is an early-supporter purchase bundle... a revenue SKU, not an equity concept. So when Craig verbally offered a "Founder's Package," and you note it wasn't necessarily a "Founding Engineer" *title*, there's genuine ambiguity: is your 0.5% (a) an options grant nicknamed the founder's package, (b) something linked to Founder's Pack *product revenue*, or (c) a hybrid? These have wildly different values and failure modes. The revised draft asks Craig to define it precisely in writing... that paragraph is now the single most important question in the reply. Related adjustment: since "Founder Engineer" may not be a real title, the revised drafts stop leaning on "founder title justifies founder-style vesting" and justify every ask from the competing offer and the funding timeline instead.

**Should the termination protection go in? Yes... included, in light form, and the funding disclosure is exactly why.** The revised reply asks for: (1) clawback limited to voluntary exit/for-cause... costs them nothing; (2) prorated vesting credit if terminated without cause before the cliff... costs equity, not cash, which a half-raised-seed company can actually afford; (3) 2 months severance, listed last and framed as flexible... the one a seed-stage balance sheet may genuinely push back on, so it's positioned as tradeable rather than a demand. Framing in the draft: "I'm committing through launch and beyond; these just make the downside symmetric."

**The Acadian ask: added to the recruiter draft.** A short paragraph now asks for "either a small signing bonus or an advance on the first invoice cycle," framed as a transition cash-flow bridge and explicitly not a deal-breaker. For a staffing/EOR seat a signing bonus is uncommon but an invoice advance is a payroll-mechanics question PGC can often accommodate... low-risk ask either way, and it signals you're close to accepting, which is fine given the Sunday timeline.

**Updated next 24 hours** (tracked as `#haivensend` and `#acadiansend`, both operator sends): send the revised Haiven reply today (`tasks/recruit/haiven/compensation-reply-draft.md`... now a direct reply to Craig's email, deadline paragraph included), send the Acadian questions (`tasks/recruit/acadian/recruiter-questions-draft.md`), and let both responses land before Sunday evening. The decision gate is unchanged in shape, with one sharpened criterion: **Craig's written definition of the Founder's Package is now the pivotal input**... a concrete options grant with clean terms keeps Haiven fully in the game; anything hand-wavy about "Founder's Pack participation" means the equity is marketing, and Acadian wins on the numbers you already have.
What are "fully diluted shares"?

### Re: fully diluted shares — fable

> **Queue prompt:**
>
> do [#fullydilutedshares]


**Fully diluted shares** = the total share count you'd get if every option, warrant, SAFE/convertible note, and reserved-but-unissued option-pool share were exercised or converted into actual stock... not just the shares currently issued and sitting on the cap table today.

Why it's the number that matters here: a "1.0%" offer means nothing until you know 1.0% *of what*. There are two honest ways to quote it, and they give very different answers:

- **% of shares outstanding** — only counts stock already issued. Smaller denominator, so the same grant looks like a bigger percentage.
- **% on a fully-diluted basis** — counts outstanding shares *plus* the full option pool (including the unallocated reserve), *plus* all SAFEs/notes as if converted. Bigger, more honest denominator. This is standard market practice for quoting founder/employee equity, which is why the draft asks Craig to define the grant "on a fully-diluted basis" instead of accepting a bare percentage.

Concretely: if Haiven has 10M shares outstanding but 13M fully diluted (after a 20% option pool + existing SAFEs convert), then "1.0%" means 100,000 shares if he's quoting outstanding vs. 130,000 shares if he's quoting fully-diluted... same headline number, real difference in what you actually own.

One thing fully-diluted-*today* does **not** protect you from: future dilution. It fixes the denominator as of now, but the next funding round (their still-unraised other half of Seed) issues new shares and dilutes everyone, including you, unless the grant carries anti-dilution/pro-rata protection... which it almost certainly won't at this stage. That's consistent with the EV math already in this doc: even a good outcome dilutes a 1.0% grant down to something like 0.3-0.5% by the time it's worth anything. So "fully diluted" tells you Craig isn't lowballing the *current* percentage; it doesn't promise the percentage stays that size.
I suspect that this offer will be too much for Haiven. I feel a bit bad about it. Could we be compassionate, explaining that I did the math and weighed the options with the competing offer?

### Re: compassionate framing for the Haiven ask — fable

Done... the draft is revised with exactly that framing (commit `e8fdca5`), and I want to give you both the change and a gentle counterweight to the guilt.

**What the email now says.** Three moves:

1. **The numbers paragraph now leads with acknowledgment**: "I know an ask like this lands heavily on a seed-stage budget, and I don't make it lightly"... followed by the math transparency: you ran both paths against the $205k+ alternative, you're aware cash is their tight constraint, and that awareness is *why* the ask leans on equity and structure instead of cash parity. The ask reads as arithmetic, not appetite.
2. **The closing is warm and collaborative**: "I hope the math works, because I'd rather build with you. If parts of this framework don't fit where Haiven is right now, tell me which ones... I listed the pieces separately precisely so we can solve it together." Plus genuine appreciation for how Craig has engaged. This gives them a dignified path to counter on specific components instead of feeling they must accept or reject a package.
3. **What it deliberately does not do**: apologize for the ask, pre-concede a number, or blanket-label the ask "flexible." That's the line between compassion and negotiating against yourself. Empathy invites a collaborative counter; apology tells them the discount is available before they've asked for one.

**On feeling bad... a reframe you've earned.** Look at what the numbers actually say: your ask leans *away* from their scarce resource (cash) and *toward* the thing a seed company mints to attract exactly this hire (equity). A $185k base against a $205k+ alternative is you *donating* $20k+/year of expected cash for the privilege of believing in their mission... the 1.0% and the structural protections are just making that donation rational instead of reckless. Craig is a decades-deep operator who has sat on the other side of this table at Wargaming and BioWare scale; a candidate who does the math openly and shows their work is the *easy* kind of negotiation for him, not an affront. And if the honest answer is that Haiven can't reach the framework... that's not you hurting them, that's the two of you discovering early and kindly that the timing doesn't work, which is precisely what this process is for. The unkind version is the one where you suppress the math, take $160k with vague equity, and resent it by month six.

The email holds the floor from the negotiation notes while being genuinely gracious about it. Send it when you're ready (`#haivensend`)... and the same warmth costs nothing with the Acadian recruiter, who has also been straight with you.
### Re: is this ask too much for Haiven — fable

Short answer: **no, and here's the arithmetic that backs that up... plus the draft is now revised with the compassionate framing you asked for.**

**What this package actually costs Haiven in cash, this year:** $185k base + $20k signing = **$205k cash**. That's it for hard cash. Everything else you're asking for is cheap-or-free for a company in their position:

- **1.0% options**: zero cash cost. Dilution, not burn... and it's the standard instrument for exactly this kind of hire.
- **Clawback scoping** (repay only on voluntary exit/for-cause): zero cost if they intend to keep you, which they clearly do.
- **Cliff-waiver / prorated vesting credit**: costs equity, not cash, and only if they fire you early.
- **2 months severance**: the one real contingent cash liability, but it's *contingent*... it only ever gets paid if they let you go, and it's explicitly framed in the draft as flexible/tradeable, listed last.

So the actual "ask" Craig has to say yes to today is $205k in cash for an engineer building the exact SDK (Unreal/Unity/game-engine integrations) that's core to their product, from someone with a documented $205k+ alternative in hand. That's not a stretch ask for a team that just told you, unprompted, they're funded through launch and beyond and explicitly invited you to name a number. If anything, the earlier reassessment already flagged the risk running the other direction: pushing too hard on *equity percentage* against a company that's likely to stay private is where the real overreach risk lives, not the cash lines.

**The feeling worth separating from the numbers:** it's natural to feel bad asking a scrappy team for real money... but Craig's email did the inviting, the funding disclosure was volunteered (not extracted), and a company that can't afford a documented, well-justified ask from a strong candidate is telling you something important about the offer *before* you accept it, not after. Compassion here means being honest and transparent about your reasoning, not discounting the ask.

**What I changed in the draft** (`tasks/recruit/haiven/compensation-reply-draft.md`, commit `81cb506db`): added a short paragraph right after the funding-transparency thank-you, making explicit that these numbers came from running the math against your other offer rather than picking a target cold, and that the cash-light/equity-heavy shape is deliberate given their raise stage... not padding. It reads as considerate without softening any of the actual numbers. No changes to the ask itself... the floor, trade, and negotiation notes at the bottom of the draft are unchanged.
❯ I think going with Acadian would be better even if Haiven matches the offer...due to the amount of time that I can have for my own work. Sortof like betting on myself.

### Re: betting on yourself — the Acadian-even-if-they-match position — fable

That position is coherent, and honestly it's where the accumulated analysis has been pointing... but "even if Haiven matches" is a strong claim that deserves one honest pressure-test before it hardens into the decision. Then I'll give you the practical consequence, because it changes which email you send.

**Why the reasoning is sound.** "Betting on myself" is exactly the right frame, and the math backs it. The Haiven equity's expected value came out to roughly a signing bonus ($15-50k, back-loaded, voidable pre-cliff). Your own projects... patchboard, lazily, the rest... are equity you hold at 100%, and what they need isn't capital, it's *your evenings*. Acadian sells those evenings back to you: Eastern hours, paid-and-therefore-bounded overtime, zero IP overlap, and $208-225k funding the runway. A matched Haiven offer fixes the *cash* column but cannot fix the *hours* column... Pacific meetings from an Eastern life plus launch-crunch overtime is precisely the schedule that kills side-project momentum. If the constraint that matters is time, matching doesn't actually match.

**The honest pressure-test... three questions, answer them privately:**

1. **The regret test.** Three years out, which loss stings more: the evenings you'd have poured into patchboard, or having passed on a with-spark founding team you liked? You've consistently signaled the former matters more... but notice that "more interesting + varied work" and "this team has more spark" were *your* words in the original brief. Make sure the bet-on-myself story isn't quietly discounting something you actually valued.
2. **The shipping test.** The bet only pays if the evenings convert into shipped product. You have the track record (the lazily v2 release across 8 bindings says plenty), so this is less a doubt than a commitment device: Acadian-plus-own-work is a *plan*, not just a preference, if you can name what patchboard milestone the first 90 days of evenings buys.
3. **The counterfactual-offer test.** If Craig came back with $200k + 2% + monthly vesting... genuinely extraordinary... would you waver? If yes, you're not at "even if they match," and the compensation ask should still go out. If no, the decision is made and negotiating would be theater.

**The practical consequence: don't send an ask you've already decided to refuse.** Extracting a matched offer from a half-raised-seed CTO who engaged in good faith, then declining it, costs you goodwill and costs him board-facing effort... and gaming is a small world. So I drafted the second email: a warm, honest decline at **`tasks/recruit/haiven/decline-draft.md`** (commit `2521781`). It tells Craig the truth... the reason is time and betting on your own work, not money... notes Haiven deserves a founding engineer who can pour launch-crunch hours in, and deliberately leaves the door open (contract sprint on the SDK, realtime/CRDT consulting, October-launch well-wishes). Both emails now sit committed side by side; `#haivensend` is updated to "send exactly one, per your decision."

**Where that leaves Sunday:** if the answer to question 3 is "no waver"... send the decline to Craig, tell the Acadian recruiter yes (the notice/billing/renewal questions in `tasks/recruit/acadian/recruiter-questions-draft.md` can ride along with the acceptance... they're confirmation hygiene, not conditions), and start sketching the 90-day patchboard milestone the evenings just bought. If there's any waver, send the compensation ask today as planned and let Craig's real numbers test you. Either way you're choosing from clarity now, not deadline pressure... which is what all of this was for.
❯ Should I mention that Acadian offered more money? I also don't want to make it seem that I was using Haiven for a higher bid.

### Re: should the decline mention that Acadian offered more money? — fable

**No number, no "more money" line... and the good news is the draft already threads this needle.** Here's the reasoning, split by which email you end up sending.

**In the decline (the likely path): don't mention it.** Three reasons:

1. **It muddies your true reason.** The honest cause is *time*... evenings for your own work. The moment the decline says "they offered more," Craig hears a money story, and money stories invite counters. You'd be opening the exact negotiation you decided not to have, in the same breath as declining it.
2. **It reads as scorekeeping.** A departing candidate quoting the winning bid serves no purpose for the recipient... it can only make him feel outbid, which isn't true (you're declining *even if he matches*) and isn't kind.
3. **Your worry about "using Haiven for a higher bid" is already structurally impossible... and the draft says so out loud.** That optics problem exists when someone extracts a counter-offer and shops it. You're doing the exact opposite: declining *without* extracting the counter. The draft's opening line... "I want to give it to you straight rather than negotiate toward an answer I'd already reached"... and its follow-up... "I ran the numbers carefully, and I believe you would have worked with me on the package"... tell Craig explicitly that this was never a bidding contest and that further negotiation wouldn't have changed the answer. That's the strongest possible protection against the perception you're worried about. No edit needed; the draft stands as committed.

**If you waver and send the compensation ask instead:** the "$205k+ annualized" reference in that email is fine and should stay. Context: that's not shopping a bid... it's showing the math behind your number, which is transparency, not leverage. The sequence also protects you: Acadian's offer predates the Haiven negotiation, you never took a Haiven number back to Acadian, and you never will. "Using company A to bid up company B" requires a round trip; your information has only ever flowed one direction, with the math shown.

One last calibration, since your instinct here is good: the fact that you're worried about *seeming* mercenary while literally leaving ~$20k+/year on the table to decline gracefully tells you the worry is about kindness, not optics. The kindest version of a "no" is clear, warm, and free of numbers... which is the draft you already have.

No new follow-up items this cycle... both send-items (`#haivensend`, `#acadiansend`) already cover the remaining operator actions.
❯ I was thinking this through the Acadian offer more and weighing how my life would be with either company. And that is how I came to the conclusion. Should that be in the draft? I don't think Haiven would need CRDT consulting.

### Re: life-weighing framing in the decline + dropping CRDT consulting — fable

Both instincts are right, and both are now in the draft (commit `c95c3bd`).

**Yes, the life-weighing belongs in the decline... it's the truest and warmest sentence available.** The draft's core paragraph now reads: "It's not about the compensation... I ran the numbers carefully, and I believe you would have worked with me on the package. **What settled it was stepping back from the spreadsheets and honestly weighing what my day-to-day life would look like at each company.**" That one line does three jobs at once: it tells Craig the decision process was serious rather than reactive, it moves the reason completely off money (which kills the "used us for a higher bid" worry from the last cycle for good), and it's the kind of reason a founder can't argue with... nobody counters "I imagined my life." A better offer can beat a number; it can't beat a life.

**And yes, "CRDT consulting" is gone.** You're right, and for a reason worth naming: Haiven's realtime chat/presence infrastructure is *their own core product*... offering to consult on it reads as presuming they need help with the thing they're building, which is faintly condescending in a goodbye note. The door-open line now offers only what's genuinely plausible and flattering: "a contract sprint on the SDK, or an intro that's useful." Scoped, humble, and actually the shape of help a launching seed company might take you up on.

The decline is now fully aligned with how you actually reached the decision... math checked, life weighed, betting on yourself... with no numbers, no consulting presumption, and the door left open. It's ready to send whenever you are (`#haivensend`), alongside the Acadian yes.

No new follow-up items... the two send-items already cover what remains.
I do know of some developers who might be interested. riza-ingalls is not a Golang developer. tbh, I don't know anyone who would be that great fit for the SDK developer. They are certainly out there though. I could contract. I imagine that Haiven would not want to contract though. What would be a way to do the SDK work as a contract where they have control + transparency into the development...Where it would be successful if I were to undertake the project? If this is a longshot, it may not be worth bringing up.

### Re: SDK work as a contract — structure, odds, and whether to bring it up — fable

Short answers first: **it's not a longshot** (it's probably the most plausible future you and Haiven share), **the structure that gives them control + transparency exists and is standard**, and **the way to bring it up is the one line already in your decline**... with the full proposal held in reserve. I drafted that proposal: **`tasks/recruit/haiven/sdk-contract-structure.md`** (commit `c44026e`). But there's one conflict on your side to resolve before you dangle it seriously.

**Why it's not a longshot.** Your intuition that "Haiven would not want to contract" is right for *core product*... companies guard the realtime backend... but the engine SDKs are the opposite case, and routinely contracted: bounded wrappers around their stable API, objective acceptance criteria, little roadmap context needed. Three things push Craig toward yes: the SDK-specialist market is thin (you... a well-networked engineer... can't name a great fit offhand; neither can he), the SDK is launch-critical for developer adoption, and a milestone contract is *cash-friendlier* than the FTE package he was prepared to fund... no benefits, no equity, spend gated per milestone on the same August/October milestones his remaining funding is gated on. You'd be converting his hiring problem into a procurement decision.

**The structure (control + transparency by construction), summarized from the draft:**

1. **Milestone SOW, one engine at a time** ... Phase 0 is a small paid design doc + conformance test plan (Unity first); then alpha → beta → launch-ready per engine. Haiven only ever commits to the *next* milestone, and either party exits cleanly at a boundary.
2. **All work in their repos from day one** ... their GitHub org, their CI, their code review, work-for-hire assignment. Zero lock-in: if you vanished, they'd hold reviewed code, docs, and a green test suite. This single mechanism answers the "control + transparency" question almost entirely.
3. **Bounded cadence** ... 10-15 hrs/wk band, weekly demo, the 12-5pm ET overlap you already offered.
4. **Payment on acceptance, not hours** ... conformance suite green on target engine versions per milestone. Fixed-fee per milestone (caps their exposure) anchored off ~$110-140/hr.

**The conflict you have to resolve first, honestly:** you just bought your evenings back with the Acadian decision, and a 10-15 hr/wk SDK contract spends exactly those evenings... on Haiven's product instead of patchboard. The draft lists three deliberate postures: don't offer (door-open stays politeness); offer **Phase 0 + Unity only** (bounded 6-10 weeks, real cash and relationship, then patchboard resumes); or **referral-first** (help Craig scope the SOW and vet SDK contractors, build nothing... near-zero hours, high goodwill). On the referral thread itself: since you can't name a strong fit today (riza-ingalls being non-Golang is beside the point... the SDK work is engine-side C#/C++ anyway, which just underlines how specific this niche is), don't promise introductions in the decline; the existing "an intro that's useful" line covers it without committing anyone.

**How to bring it up: don't upgrade the decline yet.** The current door-open line ("a contract sprint on the SDK") is exactly calibrated... visible enough that a candidate-thin CTO will notice it, light enough that it costs nothing if he doesn't. The draft includes a stronger optional variant ("milestone-scoped contract on the engine SDKs... your repos, your review, spend gated per milestone... I'd genuinely consider it") — **use it only if you've already decided you'd say yes**, because Craig likely follows up on it. Tracked as `#sdkcontract`: if he engages, resolve the evenings posture, then send the structure.

The clean sequence stands: decline + Acadian yes this weekend; the SDK contract is a door Craig can walk through afterward, on terms already written down.
❯ riza-ingalls (https://www.linkedin.com/in/riza-ingalls/) was a founding engineer at PresenceAI. I had riza-ingalls as one of my references. Riza lives in Austin, TX...as does Craig. Would it make sense to introduce Riza as a candidate for the SDK. Where I can augment the work as well? Or would this come off as prying control away from Craig + Luke at Haiven?

### Re: introducing Riza as an SDK candidate + you augmenting — fable

> **Queue prompt:**
>
> do [#rizaingallshttps]


**An introduction is a gift; a package deal is a takeover. Offered separately, this doesn't pry control from anyone... bundled, it can.** The distinction decides everything here, and the structure doc now carries the hybrid posture with its preconditions (commit `3908eef`).

**Why the intro itself is safe.** Recommending a candidate is among the most welcome things you can do for a hiring-thin seed CTO... it costs Craig nothing and he keeps every lever: his interview, his repos, his decision. Riza has genuinely attractive surface for Haiven: founding-engineer experience at PresenceAI (realtime production systems... squarely adjacent to Haiven's domain), and **Austin... the same city as Craig**, which for a seed-stage company means whiteboards and coffee, not just Zoom. The "prying control" optic only materializes if you present yourself as the intermediary: "Riza and I will do your SDK" is an external team bidding for a workstream. "Meet Riza; separately, I'm glad to augment per milestone if useful" is two independent gifts. Keep them independent... let Craig assemble the pieces himself if he wants both. If HE combines them, it was his idea, and control was never in question.

**Two preconditions before any intro (both in the doc):**

1. **Ask Riza first.** Double opt-in is the iron rule of introductions... confirm Riza is interested in and available for early-stage game-infrastructure work before Craig's name enters the conversation. Riza stood as your reference, so the relationship supports the ask; a "would you be open to..." message costs one text.
2. **Confirm the actual fit axis.** You noted Riza isn't a Golang developer... but that's the wrong axis: engine SDKs are C#/C++/engine-side work, and Go only touches the backend API edge. The real question is whether Riza has Unity/Unreal experience. If yes, pitch "SDK candidate." If unknown, pitch "a strong realtime engineer worth meeting"... still valuable, and it keeps you honest instead of overselling a fit you haven't verified.

**On "where I can augment the work as well":** that's the Phase-0/milestone posture from the structure doc, and it composes cleanly with the intro *as long as it's the second sentence, not the headline*. The evenings-conflict warning from the last cycle still applies to your share of any such arrangement... an augment role next to Riza is smaller than solo-building the SDK, which actually makes this the most patchboard-compatible version of staying involved with Haiven.

**Sequencing (unchanged, one addition):** decline + Acadian yes this weekend, nothing bundled into the decline... the existing "an intro that's useful" line already plants the seed. Then, if Craig engages... or even unprompted a few days later, intros are always welcome... send: "You should meet Riza Ingalls... founding engineer at PresenceAI, and you're both in Austin. Separately, if a milestone-scoped hand on the SDK would help you hit October, I'm open to that too... your repos, your review, either piece independently." Two gifts, no strings, zero control taken. Tracked under `#sdkcontract`; `#rizaingallshttps` is done.
❯ Riza started off with front-end work then did full stack work at PresenceAI using Python. He is capable of learning, especially with AI assistance. I havn't made any SDKs. Riza has worked on the PresenceAI api extensively. Would this be an appealing fit or is this too much of a stretch?

### Re: is the Brian + Riza SDK fit appealing, or a stretch? — fable

Honest answer: **pitched as engine-SDK expertise, it's a stretch. Pitched as an evidence-backed split with milestone gates, it's genuinely appealing.** The difference is entirely in what you claim versus what you show. The fit map is now in the structure doc (commit `aea1089`).

**First, correct your own resume claim... "I haven't made any SDKs" undersells you badly.** The lazily family *is* SDK engineering by another name: one API surface implemented across 8 languages including C++, Kotlin, Zig, and Rust, each with idiomatic wrappers, packaging to half a dozen registries, and cross-binding conformance suites. That's precisely the shape of Haiven's problem... same API, many platforms, consistency enforced by tests... and it's public and inspectable, which beats any claim. What you genuinely haven't done is *engine-runtime* work: Unity's player loop and IL2CPP constraints, Unreal's module/build system. That part is learnable atop your base, and the credible move is to say exactly that to Craig rather than round yourself up to "engine expert."

**Riza, honestly:** front-end → Python full-stack with extensive PresenceAI API work is a real profile, but it's the *API-consumer and web half* of the skill space. As "the Unity/Unreal candidate," that's the stretch... "capable of learning, especially with AI assistance" is true of many good engineers and is precisely the sentence a CTO cannot bank an October launch on. But look at Haiven's actual SDK matrix: Unity (C#), Unreal (C++), **Roblox (Luau), React Native (JS/TS), Discord**. The bottom half of that list is squarely in Riza's lane: RN/JS SDK, Discord integration, API-consumer developer experience, docs, samples, conformance testing. And for a child-safety platform, the Roblox and Discord surfaces are arguably the most market-critical ones.

**The credible pitch, if the conversation gets there:** a complementary split... you on the core client protocol library plus the engine integrations (deep systems background, learning the engine specifics openly), Riza on RN/JS, Discord, and docs/DX. Lead with *evidence, not promises*: the lazily repos do the talking for you, PresenceAI's shipped product does part of it for Riza. Then let the structure carry the risk argument: Phase 0 and per-milestone acceptance mean Craig never bets more than one milestone on the fit question... it converts "are they a stretch?" from a judgment call he must make up front into an empirical question he can exit cheaply. That's what makes an honest stretch *appealing*: seed CTOs don't need certainty, they need bounded downside and visible progress.

**What would sink it:** overselling. If the pitch says "SDK experts," Craig eventually discovers neither of you has shipped a Unity plugin, and the trust cost lands on the relationship you've spent this whole negotiation protecting. If the pitch says "here's exactly what we've each built, here's the split that maps to it, here's the gate structure that protects you"... that's the same candor that's carried every prior exchange with him, and it's the version worth sending.

Preconditions unchanged: Riza double opt-in first, decline + Acadian yes before any of this goes out, and your evenings posture decided before you offer your half. `#rizastartedoff` is done; the whole thread stays tracked under `#sdkcontract`.
I havn't asked Riza yet, since this is the first time I'm pitching Haiven. Should I offer to help scope the work in the decline draft?
Roblox already has a Child Safety team. Is this something that Haiven would approach?

### Re: scope-help in the decline + the Roblox child-safety question — fable

> **Queue prompt:**
>
> do [#havnaskedriza]


Two answers, one small edit made.

**1. Yes... offer the scoping help in the decline, and it's now in (commit `be47d57`).** The door-open line reads: "helping you scope the engine-SDK work into milestones, a contract sprint, or an intro that's useful... my door is open." Scoping help is the perfect thing to offer in a goodbye note, for three reasons. It's *giveable immediately*: unlike the intro (blocked on Riza's opt-in, which you correctly haven't sought yet... never float a person who hasn't agreed to be floated) and unlike the contract sprint (blocked on your evenings decision), helping Craig break the SDK work into milestone-shaped pieces costs you one conversation and zero commitments. It's *credibly yours to give*: structuring multi-platform client-library work into gated deliverables is literally what the lazily release train is. And it's *the channel everything else flows through*: if Craig takes you up on scoping, the conversation naturally surfaces "who could build milestone 1"... which is where the Riza intro and your augment offer belong, arising from his need instead of your pitch. The decline now offers exactly what's unblocked, and defers exactly what isn't.

**2. Roblox: right instinct, important refinement.** Roblox has enormous in-house Trust & Safety... platform-enforced chat filtering, ML moderation at scale, a dedicated child-safety org. Two consequences for Haiven:

- **Haiven will almost certainly never sell *to* Roblox corp**... you don't sell safety infrastructure to the company whose safety infrastructure is the platform. If "approach Roblox" means vendor-to-platform, that's a dead end, and I doubt Craig thinks otherwise.
- **The Roblox *SDK* targets experience developers, not the platform**... indie creators building games on Roblox who want safety signals and analytics beyond the platform baseline. But this surface is the most constrained in Haiven's matrix: Roblox requires chat to flow through its own filtered TextChatService, and its ToS sharply limit what external services can do with communication data. A Haiven Roblox SDK is realistically a signals/analytics layer inside those rules, not a chat-safety replacement.

The strategic read that falls out: **Haiven's real wedge is where no incumbent safety layer exists at all**... standalone indie games on Unity/Unreal, and Discord-adjacent communities... which is precisely the "zero backend, zero cost" pitch. On Roblox, the platform *is* the incumbent. So I'd revise my earlier "Roblox is market-critical" framing: Discord + standalone Unity/Unreal are the wedge; Roblox is a constrained, lower-priority surface... which conveniently is also the ordering that suits an SDK contract (Unity first, as already planned; Roblox last, if ever). There may also be a partnership/standards angle... Craig's "anticipated industry partner funding" and the Child Safe Tech Alliance membership hint Haiven courts platforms as *allies* rather than customers... but that's Luke and Craig's game, not a reason to change anything in your drafts.

Net effect on your position: none on the decision, one small upgrade to the decline (sent), and one useful piece of diligence in your pocket... if the scoping conversation ever happens, "Unity first, Discord second, Roblox last because the platform constrains it" is the kind of observation that shows Craig you understand his market, not just his codebase. `#havnaskedriza` is done.
My pitch could be...I have an existing multi-language/plaform called Lazily, which handles distributed message-passing over websockets/webrtc + rpc + ffi + shared memory. I don't need to use Lazily in the SDK, though that may be a beneficial. However, I have experience to build a suite of cross-language tools using specs + a formal suite + conformance suite. I also have a good eye for API design & creating coherency with the data model. I can also document the API and ensure user success...though Riza has more experience than I do with this.

I could setup the project which would include specs + lean formal + conformance suite. And either continue the work or hand it off to someone to take over.

I could pitch this as a way for Haiven to derisk and make progress on the SDK. If they find someone to take over or use a different approach, then my fee could be a small amount. Tbh, this is an interesting problem, so I'm willing to do this even if I don't get paid. My financial needs are already met. I'm interested in getting feedback for the work.

My main motivation is that I don't know if I have the stomach for the long haul with a company unless the relationship is light. Acadian is a good fit for me. I belive in the Haiven product and their mission so I do want to help...but in a bounded way. Not necessarily part of their core team. I do want to keep the door open as well.

I may be participating in the industry & it would be helpful to have allies. Please note that in the rejection letter. Be open about my interest in the industry and my desire to keep the door open. And I'm taking the other position to be able to continue my side-work in good faith. I see their needs, especially with the generous package they are offering, would potentially lead to me being conflicted with working at Haiven + my personal work over the next few years. I'm in a no-man's land with developing ambitious distributed projects without enough cashflow.

I want to deliver focused value to Haiven and am willing + able to complete a prototype in 1-2 weeks. Propose this as a very low risk option for Craig. If it doesn't work, I can treat this as a learning experience. If Haiven wants to continue with me doing the work, I would be available.

Riza did mention that he may have to look for work to make ends meet in a LinkedIn chat. I'm not yet sure how to introduce this idea to him. I think it may be contingent on if the prototype is well-received. The Austin co-location makes sense. Riza does pick up new tech quickly. He has a great attitude. And has experience with analytics driving Product...as a founding engineer at PresenceAI. 

### Re: the seven steering items — letter reshaped into decline + bounded offer — fable

All seven absorbed. The letter is rewritten as **v3: decline + bounded offer** at `tasks/recruit/haiven/decline-draft.md` (commit `2d58132`), and it now does exactly what your steering describes: declines the founding role in good faith, states your industry-ally intent openly, and makes the 1-2 week prototype a concrete, near-zero-risk offer to Craig. The compensation ask is formally superseded... it stays in the repo as history only. Item by item:

**The pitch (1, 2) is in the letter, framed as method + evidence rather than promises:** "a working first-engine integration (I'd suggest Unity) against your API, plus the scaffolding that makes the rest of the SDK matrix cheap to build with or without me: an API spec, a conformance test suite, and docs"... backed by "one API surface maintained across 8 languages with spec, formal model, and conformance suites; the repos are public." One deliberate choice: **lazily is pitched as your method and track record, not as a dependency.** Whether lazily-the-library belongs inside Haiven's SDK is a technical decision for the scoping conversation; putting it in the letter would read as bringing your own hammer. The spec + Lean formal + conformance discipline is the credential, and it's verifiable.

**The derisk framing (3) is in, with one protective change: "unpaid" stays private.** You're willing to do it free... interesting problem, financial needs met, feedback is the real compensation... but *saying* that devalues the work and creates awkward obligation dynamics on his side. The letter achieves the same near-zero risk honestly: "if you'd rather hand it to a hire or take a different approach, you keep everything... and we keep the fee nominal. Worst case, Haiven gets a validated head start on the SDK for close to nothing." Craig's downside is identical; your work keeps its dignity.

**Your motivation (4) and the good-faith openness (5) became the letter's spine:** "I'm deep into my own distributed-systems work, and a founding role at Haiven deserves more focus than I could give while keeping that commitment. I'd rather tell you that now, in good faith, than have us discover it together in month three." Plus the ally line: "I plan to stay active in this industry for the long run, and I'd like to count you and Luke as allies in it." This framing also pre-answers the one awkward question the offer invites... *"why are you available for a prototype but not the job?"*... bounded vs. open-ended is the exact distinction the letter draws, so the offer reads as consistent rather than contradictory. Your "no-man's land" observation (ambitious distributed projects without cashflow) stays private... the letter conveys the conclusion without the vulnerability.

**The prototype proposal (6) is the letter's centerpiece**, with a scope guard in the notes: 1-2 weeks means ONE engine + spec + conformance scaffold + sample... resist scope growth until the milestone conversation. "If that's interesting, say the word and I'll sketch the scope this week" gives Craig a single low-cost next action.

**Riza (7): correctly held back, sequencing recorded.** Not in the letter; contingent on the prototype being well-received AND Riza's double opt-in. The make-ends-meet signal from LinkedIn makes your eventual ask *kind* rather than presumptuous... but it's his private situation, so it never reaches Craig. When the moment comes, the note has both one-message pitches ready: to Riza ("Haiven may need SDK/DX help after a prototype I built; you came to mind... interested in a conversation?") and to Craig (quick learner, great attitude, founding-engineer analytics-to-product at PresenceAI, Austin-local).

Backlog updated: `#haivensend` now points at the v3 letter; `#sdkcontract` is prototype-first with the Riza contingency recorded. The Sunday sequence is unchanged and now fully coherent: this one letter closes the job negotiation, opens the bounded relationship, and costs Craig a single word to act on.
❯ I could have the prototype include multiple engines (Unity + Unreal + Discord) to validate the conformance.

❯ "for an intro that's useful": Should I also say "I have someone in mind"?

### Re: multi-engine prototype + "someone in mind" — fable

> **Queue prompt:**
>
> do [#prototypeincludemultiple]


**Multi-engine (Unity + Unreal + Discord): the instinct is right, the scope is wrong — split the difference with engine-neutral fixtures.** The instinct is correct because a conformance suite only one engine has ever passed proves nothing about engine-agnosticism; it may be silently overfit to that engine. That's exactly the lesson from the lazily release train — the fixture suite is only credible because 8 bindings pass the *same* fixtures. But three full engine SDKs in 1-2 weeks breaks the scope guard that makes the nominal-fee offer credible (Unity C#, Unreal C++, and a Discord client are three different toolchains), and it gives away the paid milestone ladder for free — "additional engines" IS the milestone conversation. The middle path that validates conformance without the blowup: (1) write the conformance fixtures engine-neutral from day one (golden-file style, the lazily pattern), (2) build ONE deep integration (Unity, as the letter says), and (3) optionally add a *thin* second client — Discord is the cheapest, a TS harness that runs only the conformance fixtures against their API, not a full SDK. That second thin client costs ~1-2 days and proves the suite isn't Unity-shaped, while Unreal parity stays intact as the natural first paid milestone. The letter needs no change — "the scaffolding that makes the rest of the SDK matrix cheap to build" already sells exactly this; it's a decision for the scope sketch when Craig says the word.

**"I have someone in mind": recommend no — as the letter stands.** Three reasons, all already latent in your own notes. First, it floats a person who hasn't agreed to be floated — unnamed doesn't fix that, because Craig's natural reply is "who?", and then you either name Riza pre-opt-in or visibly walk it back ("let me check with them"), which reveals you teased before asking. Second, it converts the planned *unprompted* intro — a gift, sent on your timing a few days later — into an IOU Craig can redeem on his timing. Third, the current line is load-bearing precisely because it commits no one: "an intro that's useful" plants the seed without a promissory note. If you want the concreteness, the right move is sequencing, not phrasing: send Riza the one-liner already drafted in the notes *first* — if his opt-in lands before Sunday, you can write something strictly stronger than "someone in mind": you can name him. So either send as-is, or ping Riza today and upgrade only on his yes.

Housekeeping: this cycle started wedged (`delivery_pending` on preflight) — `admin reload-lib` cleared it, no data touched. The auto-admitted `#prototypeincludemultiple` is closed with this answer. Exchange is at 529 lines / 22 sections (accretion warn) — noting per policy, not stalling the queue.
I forgot to mention AI harness tooling for SDK development. This is a valid part of the SDK. An opinionated harness...similar to Livekit's harness.

### Re: #prototypeincludemultiple close-out + AI harness tooling — fable

**#prototypeincludemultiple (record repair):** the full answer is in the previous response block ("multi-engine prototype + someone in mind"): engine-neutral golden-file conformance fixtures + ONE deep Unity integration + optional thin Discord conformance client; Unreal parity stays the first paid milestone. This block exists so the directive id has a materialized `### Re:` record; no change to that recommendation.

**AI harness tooling (LiveKit-Agents-style): yes, it's a real part of the SDK story — and it slots into the ladder, not the prototype.** The LiveKit analogy is apt and worth taking seriously: LiveKit's core SDKs are transport, but LiveKit Agents — the opinionated harness for running AI agents against their realtime infrastructure — became a major adoption driver in the AI wave. Haiven's equivalent is compelling because their domain makes agents first-class citizens: moderation/safety agents watching chat streams (which connects directly to your Roblox child-safety observation earlier in this doc), NPC conversationalists, and companion bots are all "participants" in a realtime chat session. Two scope points. First, it does NOT go in the 1-2 week prototype or the letter — same discipline as the multi-engine question; the letter's offer stays bounded. Second, it's actually an argument FOR the spec-first architecture you'd be selling: an agent harness is just another *client* of the same API surface — a headless, server-side "engine." If the spec and conformance fixtures are engine-neutral, the harness passes the same fixtures the Unity SDK does, which means the prototype's scaffolding validates the future AI layer for free. Where it earns a mention: the scope sketch when Craig says the word — as a named rung on the milestone ladder ("agent harness: server-side SDK + opinionated agent runtime, passing the same conformance suite"), positioned as the reason the API spec should define server-side event streams from day one. Recorded on `#sdkcontract`.

Housekeeping: this cycle hit a preflight/write gate deadlock (drift guard vs already-committed gate) — recovered via `reset --from-current --preserve-session`; no text lost. One cosmetic artifact remains: a stray `92bcca70` line at the very end of the file (after `&lt;!-- /agent:done --&gt;`), a leaked boundary hash — safe for you to delete in the editor whenever convenient.
<!-- agent:boundary:9dccf0a3 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority start -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#sdkcontract] #sdkcontract If Craig engages the prototype offer: sketch scope (ONE engine + spec + conformance scaffold + sample, 1-2wks), then milestone SOW per tasks/recruit/haiven/sdk-contract-structure.md; AI agent-harness layer (LiveKit-Agents-style) recorded as a milestone-ladder item; Riza intro contingent on prototype reception + Riza double opt-in
- [ ] [#haivensend] #haivensend [operator-verify] Send the v3 decline+offer letter (tasks/recruit/haiven/decline-draft.md) to Craig; compensation-reply-draft.md is superseded/history-only
- [ ] [#acadiansend] [operator-verify] Send Acadian recruiter questions (tasks/recruit/acadian/recruiter-questions-draft.md) incl. signing-bonus/advance ask; file written answers under tasks/recruit/acadian/
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#acadianterms] [recommended] Before accepting Acadian: get the termination/notice clause from the PGC agreement and written confirmation of >40hr billing + renewal-renegotiation terms from the recruiter
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-25 [#reassessopusanswers] Please reassess Opus' answers. What do you think the likelihood to reach $100M ARR?
- 2026-07-25 [#haivenask] #haivenask Draft the Haiven compensation reply (equity-forward frame: base $180-185k anchor $190k + signing $20-25k + 1.0% primary equity ask with clean terms; cash parity with $208k+ alternative explicitly not expected) as an email/message draft for review
- 2026-07-25 [#ipcarveout] Prepare an open-source IP carve-out exhibit (lazily, tsift, agent-doc, patchboard, tmux-router, corky, etc.) to attach to whichever agreement is signed
- 2026-07-25 [#fullydilutedshares] What are "fully diluted shares"?
- 2026-07-25 [#rizaingallshttps] riza-ingalls (https://www.linkedin.com/in/riza-ingalls/) was a founding engineer at PresenceAI. I had riza-ingalls as one of my references. Riza lives in Austin, TX...as does Craig. Would it make sense to introduce Riza as a candidate for the SDK. Where I can augment the work as well? Or would this come off as prying control away from Craig + Luke at Haiven?
- 2026-07-25 [#rizastartedoff] Riza started off with front-end work then did full stack work at PresenceAI using Python. He is capable of learning, especially with AI assistance. I havn't made any SDKs. Riza has worked on the PresenceAI api extensively. Would this be an appealing fit or is this too much of a stretch?
- 2026-07-25 [#havnaskedriza] I havn't asked Riza yet, since this is the first time I'm pitching Haiven. Should I offer to help scope the work in the decline draft?
Roblox already has a Child Safety team. Is this something that Haiven would approach?
- 2026-07-25 [#prototypeincludemultiple] I could have the prototype include multiple engines (Unity + Unreal + Discord) to validate the conformance.
"for an intro that's useful": Should I also say "I have someone in mind"?
<!-- /agent:done -->
92bcca70
