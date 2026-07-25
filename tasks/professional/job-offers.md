---
agent_doc_session: a7cd84b1-f38d-4554-92b9-1fbe207b6f9e
resume: c00f9a95-29de-4ded-8b3c-3df9edebe6f3
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
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
❯ 🚧 Is $100/hr more like >$200k/year? Please reassess what I should ask for.

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
❯ 🚧 I can bill above 40 hours and the rate is renegotiable at renewal.

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
<!-- agent:boundary:e0770874 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#haivenask] #haivenask Draft the Haiven compensation reply (equity-forward frame: base $180-185k anchor $190k + signing $20-25k + 1.0% primary equity ask with clean terms; cash parity with $208k+ alternative explicitly not expected) as an email/message draft for review
- [ ] [#ipcarveout] Prepare an open-source IP carve-out exhibit (lazily, tsift, agent-doc, patchboard, tmux-router, corky, etc.) to attach to whichever agreement is signed
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
