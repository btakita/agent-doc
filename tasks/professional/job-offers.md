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
<!-- agent:boundary:7bcca397 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#haivenask] #haivenask Draft the Haiven compensation reply (year-1 target ~$200k: base anchor $190k / land $180-185k + signing $20-25k; equity term questions from analysis) as an email/message draft for review
- [ ] [#ipcarveout] Prepare an open-source IP carve-out exhibit (lazily, tsift, agent-doc, patchboard, tmux-router, corky, etc.) to attach to whichever agreement is signed
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
