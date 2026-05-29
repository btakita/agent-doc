---
agent_doc_session: e48b3024-e796-496f-85c5-7c37c346b046
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please add a contact for brandon-cinquegrana
❯ - https://www.linkedin.com/in/brandoncinquegrana/

❯ use markitdown to convert /home/brian/work/btakita/agent-loop/tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.pdf to a md file
### Re: Add a contact for brandon-cinquegrana — done — opus-4-8

Created the corky contact at `mail/contacts/brandon-cinquegrana/` and enriched it from the LinkedIn profile you linked.

- **Created:** `corky contact add` scaffolded it, then I **normalized to the repo's kebab-case convention** — corky's `add` made a space-named dir (`Brandon Cinquegrana/`), the lone outlier among 56 kebab dirs, so I renamed the directory and the `[contacts.brandon-cinquegrana]` key in `mail/.corky.toml` to match. `CLAUDE.md → AGENTS.md` symlink intact; `corky contact info brandon-cinquegrana` resolves cleanly.
- **Enriched `AGENTS.md`** from the profile (1st-degree connection): headline *"Helping Startups Scale with On-Demand HR & Recruiting Support,"* Dallas–Fort Worth Metroplex, Rutgers University, 500+ connections, mutuals Alex & Saylor, and your recent connection note.
- **No email/phone yet** — only the LinkedIn URL was available (the Contact-info overlay wouldn't load reliably). Flagged in the contact's Notes and added `#brandon-email` to the backlog so it's not forgotten before any outreach.

### Re: Convert the settlement PDF with markitdown — done — opus-4-8

Converted via `markitdown[pdf]` (base markitdown lacks PDF support, so I ran the PDF extra):

- **Output:** `tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.md` — 137 lines, clean text (verified it's real legal prose, not OCR noise).
- Sensitive legal content, so I'm not quoting terms here — the file is local and uncommitted.

Both the contact files and the converted `.md` are left **uncommitted** in the working tree (you didn't ask to commit, and `#commit-push` is only defined as a preset, not invoked). Say the word and I'll commit + push.
❯ Should I add the description of my recent projects to my freelancing work or as separate companies?
❯ - dreamable
❯ - equityfundingsource
❯ - monsterrodholders

### Re: Recent projects — freelancing umbrella vs. separate companies — opus-4-8

Default to **one freelancing/consulting umbrella** with dreamable, equityfundingsource, and monsterrodholders listed as dated engagements beneath it. Break one out into its own company entry only when the relationship was actually a *role*, not a contract.

The decision rule:

| Present as… | When |
|---|---|
| **Sub-bullet under your consulting banner** | Engaged as a contractor/consultant; deliverable-based; weeks-to-months; no title or equity. |
| **Standalone company entry** | You held a defined ongoing role (founder, co-founder, principal, officer) or equity — or the engagement is long/notable enough that the name carries more weight than the umbrella. |

Why the umbrella is the default for a recruiter audience (Brandon screens for startups):

- Three separate short company entries read as job-hopping. The same three as clients under *"Independent Software Consultant, 2023–present"* read as a stable, in-demand consultant.
- It's truthful to a contractor relationship and parses cleanly in ATS — continuous employment, one employer block.
- You frame the narrative once instead of defending three exits.

Applying it to your three:

- **monsterrodholders** — WooCommerce/WordPress client work. Umbrella, client bullet. Lead with outcome (migration, checkout/WAF fixes, uptime/revenue), not the stack.
- **equityfundingsource** — the one to decide deliberately. Founder/principal/equity stake → standalone entry. Client build → umbrella. Tell me which and I'll phrase it.
- **dreamable** — present as a **completed engagement**, neutrally, under the umbrella. It closed in a settlement / mutual release, so keep it low-profile: dates plus a neutral scope line, no outcome drama, and never reference the settlement. Don't drop it if doing so opens a visible gap, but don't feature it either.

Structure I'd use:

```
Independent Software Consultant — Self-employed | 2023–Present
  • EquityFundingSource — <one-line impact>
  • MonsterRodHolders   — <one-line impact>
  • Dreamable           — <neutral scope line>
```

If equityfundingsource was a founder role, lift it to its own header *above* the consulting block and keep the other two beneath the umbrella.

One question back: were any of these equity/founder roles, or were all three client contracts? That single fact is what changes the answer.

❯ Please transcribe + diarize the call with brandon-cinquegrana.
❯ [phone_20260528-140602_2144332110.m4a](../../mail/contacts/brandon-cinquegrana/recordings/phone_20260528-140602_2144332110.m4a)

### Re: Transcribe + diarize the Brandon Cinquegrana call — opus-4-8

Done → `mail/contacts/brandon-cinquegrana/recordings/phone_20260528-140602_2144332110.transcript.md` (18:36, 312 segments, GPU Whisper large-v3-turbo + pyannote chunked diarization). Left uncommitted in the working tree, same as the other contact artifacts.

**Diarization caveat:** full-audio diarization detected 0 speakers; the chunked (600s) pass recovered 2, but the labels are unreliable. The first ~10 minutes collapsed into a single **Brian** block that actually contains both voices, and several short turns in the back half are swapped (the team-size and rate answers tagged *Brian* are Brandon's). Treat speaker tags as approximate — I can do a manual attribution pass if you need clean labels.

**What the call was:** Brandon is recruiting for **Equal Experts** (Whisper mangled it to "EagleXworth" / "Equal Iceberg") — a UK software consultancy building out its North-American network (~200–300 consultants across US/Canada/LatAm vs. a few thousand in the UK). He's sourcing senior / principal back-end & full-stack engineers. He's an ex-Google recruiter now contracting for them through his own LLC.

**Key facts:**

- **Rate:** $120–150/hr to the engineer; a margin (not 2×) added for the client.
- **Process:** send resume via LinkedIn → take-home assignment (~2 hrs of work, 14-day window) → technical-team review → pairing interview → join the network + Slack.
- **Pipeline:** two projects starting ~June staffing senior back-end engineers; engagements run 12 weeks–2 years; teams start at 3–4 and can grow past 10. No obligation to take every project offered.
- **Your action item:** send Brandon your resume via LinkedIn (you said this evening). Added to the backlog as `#brandon-resume`.

**This answers my open question from the last cycle.** On the call you classified all three for Brandon yourself:

- **Dreamable** — "kind of a full-time role… for a couple of months," now ended.
- **EquityFundingSource** — a "smaller client" (the commercial-real-estate portal build), not your own venture.
- **monsterrodholders** — the other smaller client (retail, ~$200K/yr).

So none are founder/equity roles — all three belong **under the consulting umbrella**, with Dreamable as the most prominent entry (longest, near-full-time) and EFS / MRH as concurrent client bullets. That resolves the equityfundingsource question: client build → umbrella bullet, not a standalone company.
❯ This call happened before I signed the Separation agreement with robert-ross. Am I liable for any information in the call?

### Re: Liability for what you said in the call — opus-4-8

**Not legal advice — you have a signed agreement, so a quick attorney read is the only authoritative answer.** What I can do is map what the call actually contains against the clauses in the signed release. (I'm treating "Separation agreement with robert-ross" as the *Dreamable Settlement and Mutual Release* — Rob is the Dreamable principal; the robert-ross session links straight into `../dreamable/`. Tell me if it's a separate document.)

**First, the timing framing is weaker than it sounds.** "The call was before I signed" doesn't give a clean pass, because the obligations that matter here aren't *created* by the settlement — they **survive termination** and almost always originate in the **original engagement contract**:

- **§5 Confidentiality** — hold in strictest confidence, don't use or disclose Dreamable's non-public info (technical data, trade secrets, product plans, customer/finance/personnel info). Explicitly survives termination.
- **§3 Intellectual Property** — code/docs/work product developed during the engagement are the Company's; survives termination.
- **§8 Non-Disparagement** — no negative statements about the other party.

If the original engagement had a confidentiality/IP clause (very common), you were already bound during the call — the settlement just reaffirms it. I don't have that original contract; **that's the first thing to confirm.**

**Now, what the call actually contains** (I re-scanned the transcript):

- **Non-disparagement (§8): clean.** You said nothing negative about Dreamable or Rob — only that the engagement "ended." No exposure.
- **Settlement terms: not disclosed.** The call predates signing, and you didn't discuss any terms. No exposure.
- **Names: none.** You never named Rob or any person.
- **The one real exposure — your description of Dreamable's product.** You described it as "agent-to-community creation… like Rome… LiveKit integration… real-time agent walkthroughs of technical material… generated on the fly or pre-generated with interactive components." That is exactly the "technical data / product plans" §5 is about — **if** it's non-public. The agentic-harness eval tools you mentioned are lower risk (you described that work *existed*, not the IP itself), but if they were built *for* Dreamable, §3 makes them Company IP.

**What cuts in your favor:**

- The description was high-level and generic, to a **recruiter** (not a competitor), in the normal course of describing your own experience.
- If Dreamable has publicly described its product (website, marketing, demos), that content isn't "non-public" and §5 doesn't reach it.
- The release is **mutual** — it bars *claims that existed as of the effective date*. Conduct before signing that the company knew or could have known about is likely within the released claims. But standard releases carve out the surviving §3/§5/§8 duties and don't waive *future* misuse, so the release helps for past claims, not for going-forward disclosure.

**Practical recommendation:**

1. Don't circulate the recording or transcript further.
2. For the resume and the Equal Experts take-home, describe Dreamable by **your transferable skills** — LiveKit, real-time multimodal, full-stack — not Dreamable's product architecture or unreleased plans. (Same as the umbrella framing from the prior cycle, just with a confidentiality lens.)
3. Confirm the **original engagement contract's** confidentiality/IP scope, and check whether Dreamable's product is publicly described.
4. If you want certainty, have an attorney compare §3/§5 against the transcript — it's a 15-minute read for them.

Bottom line: the call is **low risk** as recruiter small-talk, the disparagement/terms angles are clean, and the realistic-but-modest exposure is the product description under §5 — mitigated by being generic, to a recruiter, and possibly already public. Added `#dreamable-confidentiality-check` to the backlog to close the open factual questions before you send anything.
❯ Please create a resume for the job.

### Re: Create a resume for the Equal Experts job — opus-4-8

Built → `resume/BrianTakita-equal-experts.pdf` (4 pages). Open: `file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-equal-experts.pdf`

**What I made:**

- A new profile `resume/profiles/equal-experts.toml` targeting what Brandon described — a **senior/principal full-stack & back-end consultant** for a staffing consultancy placing engineers on varied client projects. It emphasizes consulting breadth + seniority + back-end depth, with the real-time-AI / agent-tooling work as the differentiator, and bolds the matching phrases/skills. Experience is filtered to the strongest senior arc (Independent Consulting → Presence AI → Open Source → Censible → SocialChorus → CrystalCommerce → Rundavoo → Milyoni → TrueCar → Pivotal Labs), dropping the intern / IT-analyst / minor entries.

**One thing I had to fix first.** The build source isn't `BrianTakita.md` — `build.py` reads `resume/data.toml` through the `agent-resume` tool. That `data.toml` was **stale**: it had no Independent Consulting section at all, so the first build silently dropped your most recent and most relevant work (the section filter matched nothing, no warning). I added the **Independent Consulting** block to `data.toml` — Equity Funding Source, MonsterRodHolders, and **Confidential AI Startup** — so it now leads the Experience section.

**Confidentiality kept intact.** Dreamable appears only as **"Confidential AI Startup"** with skills-only framing (real-time AI inference/streaming pipelines, agent-harness tooling, investor demos — Rust/Python/TS/LiveKit/WebRTC/CopilotKit), no product architecture and no name. That matches both the prior cycle's umbrella framing and the §5 confidentiality review. Verified: "Dreamable" appears 0 times in the PDF.

**Notes / your call:**

- **Length:** 4 pages. Fine for a 20-year senior/principal consultant a recruiter is vetting, but if you'd rather a tight 2-pager I can add a `[sections]` trim (drop Career Highlights or the pre-2014 roles) and rebuild.
- **Uncommitted:** the new profile, the `data.toml` edit, and the PDF are uncommitted in the `resume/` submodule. Say the word and I'll commit them there + bump the submodule pointer.
- This covers `#brandon-resume`'s artifact; I left that item open since sending it to Brandon is still pending. Added `#resume-data-toml-sync` to track the source-drift cleanup.
<!-- agent:boundary:08ee3851 -->
<!-- /agent:exchange -->
###
<!--
Please read tasks/dreamable/052826_dreamable_-_brian_takita_settlement_and_mutual_release_agreement_signed.md
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#dreamable-confidentiality-check] [recommended] Before sending the resume / doing the Equal Experts take-home, pin down the scope of Dreamable's confidentiality + IP clauses (signed mutual release §3 IP-assignment and §5 Confidentiality both survive termination) and check the ORIGINAL engagement contract for a pre-existing NDA. Keep all Dreamable framing skills-only (LiveKit / real-time / full-stack), never proprietary product architecture or unreleased plans. Consider an attorney review of the 2026-05-28 Brandon call transcript.
- [ ] [#brandon-resume] [Contact] Send Brian's resume to Brandon Cinquegrana via LinkedIn for the Equal Experts network vetting (a ~2hr / 14-day take-home assignment follows). Brian said he'd send it the evening of 2026-05-28. Resume should present Dreamable + EquityFundingSource + monsterrodholders under one consulting umbrella — all client/consulting engagements per the call, no founder roles.
- [ ] [#brandon-email] [Contact] Get Brandon Cinquegrana's email/phone before drafting any email - only LinkedIn is on file. Pull from the LinkedIn 'Contact info' panel or ask Brian directly. [recommended]
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
