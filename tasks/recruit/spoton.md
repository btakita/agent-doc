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

Concise live answer you can use:

> I would approach this as an activation funnel from signed contract to installed POS. First I would make the state of every merchant explicit: what is missing, who owns the next action, how long it has been stuck, and what risk signals we have. Then I would look for places where product can replace low-value manual follow-up with a clear merchant checklist, self-scheduling, upload/review flows, and automated nudges. I would still keep humans in the loop, but reserve them for high-intent moments where they can actually unblock the install. The goal is not just fewer calls; it is fewer unclear steps, faster momentum after signing, and a higher install rate without losing the hospitality that SpotOn sells.

Closeout proof: changed path is the session document only; no new transcript artifact was created. Commands included preflight, plan/read, transcript inspection, SpotOn web review, sidecar reset, recovery preflight, and this force-disk finalize. No code tests were run because no code changed. Confidence is medium-high; exact funnel stage weights still need confirmation from SpotOn.
<!-- agent:boundary:11315da6 -->
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
