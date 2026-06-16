---
agent_doc_session: ebec48ed-d3ed-40f6-bcd0-c37938c43e8b
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/40d32c683160fb33757db1c143cf685f41718275d7e4c0d1ef254d55233f6679-20260616-164423.md`*

Compacted content:
- Archived 8 response topic(s): contacts + call transcript; contacts + call transcript; LinkedIn note for Rafael; 5 more
- Prior summary/context: ❯ Please add contacts for: ❯ - rafael-onso https://www.linkedin.com/in/rafaalonso/ ❯ - michael-kai https://www.linked...
- Where the 30% loss really happens: underwriting delay, missing menu data, hardware shipping, install scheduling, call no-shows, owner cold feet, staff training, connectivity, or post-install first transaction.
- The exact funnel definitions: signed, approved, scheduled, menu-ready, hardware-ready, installed, activated, first payment, retained.
- Source of truth and data quality: Salesforce, internal implementation tooling, POS/dashboard events, call/text/email logs, support tickets, and whether status is real-time or manually maintained.
- Segment differences: full-service restaurants with onsite contractors, lower-GPV remote installs, simple retail/unboxing installs, new openings, existing POS replacements, single-location versus multi-location.
- Ownership boundaries: what product/engineering can change versus what belongs to pricing, sales incentives, underwriting, implementation ops, or support.
- Whether the product goal is fewer human touches, better-timed human touches, or both. Mike's comment was not "remove people"; it was "make human outreach higher intent and higher value."
1. Treat signed-to-installed as an activation funnel, not an ops queue. Map every state, SLA, owner, blocker, and handoff. Then make the next required action visible to the merchant, implementation agent, sales, and support.
2. Build a merchant-facing launch plan. After signing, the merchant should see a short, concrete checklist: schedule install, upload/review menu, confirm hardware/shipping, prepare network/printers, train staff, run first transaction. SpotOn publicly sells a guided, stress-free launch; the product should make that promise observable.
3. Reduce four calls into async progress plus high-value intervention. Use self-scheduling, reminders, rescheduling, document/photo upload, menu review, and clear deadlines. Escalate to a person when a merchant is stuck, not just because the process always requires another call.
4. Add install-risk scoring. Signals could include time since signing, missing artifacts, no-show history, unanswered outreach, business type, install complexity, underwriting status, menu complexity, and hardware readiness. Use that score to route effort toward deals most likely to fall off.
5. Make menu build production-grade. They already have AI-assisted menu/demo work, and SpotOn publicly highlights AI menu assistance. The gap Mike described is the last 20%: configuration QA, owner approval, modifier sanity checks, pricing mismatches, printer/KDS routing, and confidence that install day will not embarrass the restaurant.
6. Preserve sales context into implementation. At signature time, capture the owner's promised go-live date, pain points, operating hours, decision makers, install constraints, menu source, existing POS, network details, and any promises sales made. The implementation team should not have to rediscover the deal.
7. Measure experiments. Good metrics: install conversion, days from signed to installed, days stuck per stage, number of manual touches, no-show rate, reschedule rate, menu defect rate, first-payment time, install-day support load, and early churn.
- "If I could look at one dashboard tomorrow, where would I see every signed merchant that is at risk of not installing?"
- "What are the top three reasons a signed restaurant goes dark? Are those tagged consistently or mostly inferred?"
- "Which stage has the highest leverage: underwriting speed, scheduling, menu readiness, hardware readiness, or install-day training?"
- "What does an implementation agent do repeatedly that should become product behavior?"
- "For a low-GPV restaurant, what is the minimum viable install path that avoids four calls?"
- "What is the moment when the merchant feels momentum: signing, seeing their own menu in the POS, hardware arriving, staff training, or first payment?"
- "How much room does the team have to change merchant-facing workflow versus only internal tooling?"
- Do not over-pitch agent-doc. Use it as evidence that you can build serious agentic workflow systems, but keep the conversation centered on SpotOn's install funnel.
- Do not lead with generic AI chatbot ideas. Their pain is state, timing, ownership, and confidence around real-world installation.
- Keep emphasizing restaurant-owner reality: they are busy, margin-sensitive, constantly interrupted, and not sitting at a desk waiting to complete onboarding tasks.
- Rafael explicitly liked that your questions were product-led. Keep doing that: ask about impact, funnel stages, and human workflow before implementation details.

### Re: SpotOn interview prep closeout — gpt-5

> **Queue prompt:**
>
> Read the transcript. Look at the spoton.com website. What are some things to look out for in the interview today? How can I help them convert more signups -> installs.


> **Queue prompt:**
>
> corky gpu to transcribe + diarize [2026-06-11 12-31-50.mp4](../../../../../Videos/2026-06-11%2012-31-50.mp4) if we don't already have a transcript.


The prep bullets above are the interview notes. The missing frame is:

- Transcript status: [spoton-2026-06-11-call-transcript.md](spoton-2026-06-11-call-transcript.md) already exists, so I did not re-run the larger video through Corky/GPU transcription.
- Website context checked: SpotOn publicly positions itself around restaurant POS, online ordering, reservations, labor/team tools, AI profit analysis, integrations, dedicated onboarding, menu setup, training, and 24/7 support ([SpotOn](https://www.spoton.com/), [Restaurant POS](https://www.spoton.com/restaurant-pos/), [New restaurant openings](https://www.spoton.com/restaurant-pos/new-restaurant-openings/), [AI menu assistant](https://www.spoton.com/solutions/ai-menu-assistant/)).
- Main thesis: help them turn the signed-to-installed path from a manual white-glove operations workflow into a measurable activation product without losing the hospitality promise.
- "How much freedom does the team have to change merchant-facing workflow versus internal tooling only?"
- "What is the earliest moment where the merchant feels, 'this is real and worth finishing'?"
- "Which install segment has the biggest conversion upside: full-service onsite, lower-GPV remote, or simple retail/unboxing?"
- "If a merchant misses call three, what automated path exists before the deal goes cold?"
- "What does an implementation specialist repeatedly do that should become product behavior?"
- "Where exactly do signed merchants fall out today, and how confidently is that reason captured?"
7. Measure the experiments: install conversion, days-to-install, stage aging, manual touches, no-show rate, reschedule rate, menu defect rate, first-payment time, install-day support load, and early churn.
6. Create QA around menu/build readiness so AI-generated or imported menus are not just impressive in demo state, but reliable enough for install day.
5. Preserve sales context into implementation: promised go-live date, pain points, decision makers, hours, existing POS, network/printer constraints, and any promises made during the sale.
4. Add install-risk scoring based on time since signing, no-shows, missing menu artifacts, install complexity, underwriting status, hardware readiness, and unanswered outreach.
3. Add self-scheduling, async upload/review, reminders, and rescheduling so a missed call does not stall a deal.
2. Build a merchant-facing launch checklist with the next required action, due date, and why it matters.
1. Model signed-to-installed as an activation funnel with explicit states, blockers, owners, and SLAs.
- Human leverage: the goal is not to remove implementation people; it is to automate low-value reminders so human touches happen when intent and urgency are high.
- Menu readiness: their AI/demo menu work is valuable, but the install risk is the final 20%: modifiers, routing, pricing, approvals, and confidence that go-live will not disrupt service.
- Segment differences: onsite full-service installs, simpler remote installs, and retail/unboxing installs should not have one generic workflow.
- Merchant psychology: after signing, owners get busy, lose momentum, avoid calls, or get nervous when asked for more work. The product has to keep the deal warm without feeling like homework.
- Stuck-deal visibility: can anyone see all signed merchants at risk today, or does risk live in calls, Slack, Salesforce notes, and tribal knowledge?
- Stage clarity: do they have precise funnel states for signed, approved, scheduled, menu-ready, hardware-ready, installed, activated, first payment, and retained?
- “What is the smallest credible hardware/install path for a low-GPV merchant who wants to try SpotOn without feeling locked in?”
- “Could we test owner-proof content at the financing step and measure financing completion, install scheduling, and install conversion?”
- “How do we explain no-contract software versus hardware obligations today?”
- “When a merchant does not install, do we know if the stated reason is cost, financing, integration fit, menu readiness, scheduling, or just going dark?”
- “Do signed merchants drop before hardware financing, during financing approval, after hardware order, or closer to install day?”
- In sales follow-up, match proof by restaurant type rather than sending one generic “happy customer” video.
- In the launch checklist, attach proof to each anxiety point: menu build, training, hardware arrival, support, first transaction.
- After a missed implementation call, send a short owner clip plus one clear next action.
- On the hardware financing screen, show an owner explaining the hardware decision and actual go-live experience.
5. Tie incentives to activation, not just signup. Examples: first-month software credit after first transaction, free menu cleanup if installed by a target date, included staff-training refresh, launch-day support guarantee, or a temporary add-on credit after go-live.
4. Trigger objection-specific proof before the financing step: “how hardware financing worked,” “what install day was like,” “how staff training went,” “what I wish I knew before switching POS.”
3. Put a “launch confidence” panel next to financing: expected install date, what SpotOn will configure, what the merchant must do, support coverage on go-live day, and when the first payment/transaction should happen.
2. Offer hardware paths side by side: pay upfront, finance, rent/lease, or start with a minimal kit. Make the tradeoff visible rather than discovered during sales follow-up.
1. Separate the commitment clearly: software month-to-month, payment processing terms, hardware purchase/financing, cancellation path, return/buyout rules, and what happens if the install never completes.
- Around **06:01-06:53**, support is mostly positive, but delays and rep inconsistency show up as a trust risk. For restaurants, “will someone be there if go-live breaks?” matters as much as sticker price.
- Around **04:47-05:29**, the video discusses pricing and processing rates, but I would treat exact numbers as potentially dated. The more durable point is the shape of the offer: low monthly entry, add-ons, processing, and hardware economics.
- Around **03:39-04:01**, the reviewer says the no-long-term-contract message is weakened if the merchant finances hardware and is still paying it off after cancellation. That is the key friction point for your question.
- Around **02:16-03:36**, the negatives are integrations, handheld reliability, reporting depth, lack of a dedicated support rep, occasional crashes, refund/tip adjustment gaps, and inability to throttle orders. These are not purely sales objections; several become install anxiety because the owner imagines service disruption.
- Around **00:36-01:42**, the video is strongly positive on ease of use, implementation, setup, menu updates, cloud access, customization, and staff training. That supports the thesis that SpotOn can sell confidence once the merchant believes they will get live smoothly.

Concise live answer you can use:

> I would approach this as an activation funnel from signed contract to installed POS. First I would make the state of every merchant explicit: what is missing, who owns the next action, how long it has been stuck, and what risk signals we have. Then I would look for places where product can replace low-value manual follow-up with a clear merchant checklist, self-scheduling, upload/review flows, and automated nudges. I would still keep humans in the loop, but reserve them for high-intent moments where they can actually unblock the install. The goal is not just fewer calls; it is fewer unclear steps, faster momentum after signing, and a higher install rate without losing the hospitality that SpotOn sells.

Closeout proof: changed path is the session document only; no new transcript artifact was created. Commands included preflight, plan/read, transcript inspection, SpotOn web review, sidecar reset, recovery preflight, and this force-disk finalize. No code tests were run because no code changed. Confidence is medium-high; exact funnel stage weights still need confirmation from SpotOn.

### Re: Queue drained — gpt-5

> **Queue prompt:**
>
> Please review https://www.youtube.com/watch?v=fuj9EIrQpWM. Getting a no-contract plan for a low cost seems like an easy entry. But could customers get cold feet when financing the equipment? Perhaps that may be a reason why some don't want to complete the install? Would real-restaurant owner Youtube videos and other incentives help?


The remaining SpotOn interview-prep queue item has been addressed above and the queue is now inactive. No code or transcript files changed; this closeout only persists the queue-drained document state.
<!-- agent:boundary:618ebf62 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
