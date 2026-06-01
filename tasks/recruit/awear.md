---
agent_doc_session: ee25d7aa-b238-46a1-83cf-db5b91a8bf9e
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
❯ Use corky to review https://mail.google.com/mail/u/1/#search/awear/FMfcgzQgMCWGQRhwKgBVPZGRjXXFjhXs using the btak.dev@gmail.com account.

❯ Please evaluate my response and provide feedback.

### Re: Reviewing the Awear thread via corky — opus-4-8

Retrieved the thread. One wrinkle worth recording: `corky sync refetch <url>` could **not** resolve `FMfcgzQgMCWGQRhwKgBVPZGRjXXFjhXs` — that's a Gmail *web permalink* pointing at an **unsent draft**, and corky's `gmail.readonly` sync only indexes real messages (drafts are skipped), so it reported "not found in any Gmail API account." I re-authed btak.dev, ran a full account sync (pulled in `awear-follow-up`, `awear-pre-interview-questions-scheduling-confirmation`, and the NDA reminders), then read the live draft directly from the open Gmail tab via chromium-bridge. So the feedback below is on your actual composed draft, not a stale copy.

Thread state: Ashley sent the pre-interview questions Mon 5/26, you forwarded it to yourself, and she **followed up Thu 5/29** because no reply had landed yet. Your draft is the reply that hasn't been sent.

### Re: Evaluation & feedback on your draft — opus-4-8

**Overall:** The substance is right for this audience. Awear is screening for an AI-native engineer, and your answers (>95% AI-written, dogfooding your own harness, 390k Rust / 190k TS solo in ~4 months, the agent-doc walkthrough) land exactly where they want a candidate to be. This draft gets you the interview. The problems are not in the technical content — they're in three places where you give the reader a reason to hesitate, plus one open ask you don't close.

**Fix before sending (highest leverage first):**

1. **The "engagement end" line is working against you.** "I had an engagement end this past week on a project that took up much time" does two unhelpful things: it frames the message as an apology for being two days late (drawing attention to the delay instead of burying it), and "an engagement end" invites the exact question you don't want in a screen — *why did it end?* You don't owe a recruiter that narrative. Cut it, or neutralize to forward-looking: "I just wrapped a project and have good availability now." Lead with the availability, not the excuse.

2. **The PresenceAI weaknesses read as airing grievances.** "CEO was not involved day to day. Role alignment + tech alignment was an issue. Lack of constructive communication." — Ashley's parenthetical ("people, lines of code, temporal duration") was listing *dimensions of scale*, not inviting a post-mortem on team dynamics. To a prospective employer, criticizing a former CEO/team is a classic red flag: it signals you might narrate *them* the same way later. Keep the weaknesses technical or neutral ("coordinating a multi-modal realtime stack across a distributed team was the hard part; alignment across roles was a recurring cost"). Save the candid version for a live conversation where tone carries it.

3. **Your AI "weakness" answer is thin.** "Bugs from dogfooding can disrupt my workflow" is nearly a humblebrag and dodges what they're actually probing: *do you understand where AI breaks down?* That's a core competency question for an AI-tooling company. Give a real one — verification/review overhead scaling with generation speed, hallucinated APIs, architectural drift over long sessions, context-window limits forcing decomposition. Showing you've mapped the failure modes is more impressive than the LoC count.

4. **You never address the NDA.** Ashley asked twice and there are two outstanding "action-needed-awear-nda" reminder threads. The draft is silent on it. State it plainly — "NDA is signed and returned" or "I'll return the NDA today" — so you close the loop she explicitly flagged. Leaving it unanswered after a reminder reads as inattentive.

**Strengths — keep these as-is:**
- The direct ">95%" with no hedging. Exactly the confidence they're looking for.
- The concrete manual-coding examples (renames, hard-to-express algorithms, tight feedback loops). Specific and credible.
- The agent-doc description is your real differentiator — bi-directional CRDT markdown, tmux dispatch, the element model. This is what separates you from candidates who just *use* Claude Code.
- The dependency-minimalism / "evaluate tools and implement the essential logic" note signals engineering judgment, not just tool adoption.

**Polish (minor):**
- The agent-doc element list (status/exchange/backlog/icebox/review/queue/...) is the strongest content but the longest — for a *screen* it's borderline. Consider trimming to exchange + backlog + queue as exemplars and offering "happy to walk through the rest live." Optional; if you keep it, it still reads as depth.
- "my largest project in re: LoC" → "by LoC" (the Latin "in re:" reads lawyerly).
- A few run-together spaces from the compose box: "all (>99%) my development.The feedback loop", "such as agent-doc.I use". Fix on send.
- "finishing at ~190k LoC" / "finishing at ~130k" for a *past* company reads oddly present-tense — "~190k LoC" alone is cleaner.
- You lean on "&" throughout. Fine in your voice, but for a first written impression with a recruiter, spelling out "and" reads a touch more finished.
- Confirm "next week" explicitly on the availability so there's no ambiguity (you gave ET times, which is good — Ashley asked for EDT).

**Net:** ship it after edits 1–4. The content already clears the bar; those four changes remove the only handholds for a "no." Want me to produce a revised draft incorporating all of this so you can review side-by-side before sending?
❯ I made the changes. Will sign the NDA before sending in the morning.
❯ Please review and provide feedback.

### Re: Re-review of the revised draft — opus-4-8

You resolved all four. This is ready to send.

- **Engagement line** → "I just wrapped a project and have good availability now." Clean, forward-looking, no opening for "why did it end." Fixed.
- **PresenceAI weakness** → "Coordinating a multi-modal realtime stack across a distributed team from the West Coast to Europe to SouthEast Asia was the hard part; alignment across roles was a recurring cost." This is the biggest improvement — it's neutral, it's technical, and the West-Coast-to-Europe-to-SEA detail actually *upgrades* the answer by showing the scope you operated across. No grievance smell. Fixed.
- **AI weakness** → "parallel work with a codebase growing in complexity can cause the agents to not solve some issues despite spending many turns on it. Such problems require undivided attention, review, and analysis." This is a real, credible failure-mode answer and exactly the judgment Awear is screening for. It also quietly signals you run parallel agents at scale, which is a plus. Fixed.
- **NDA** → "I have signed the NDA" in the opening. Closes the loop she flagged twice.

**One caveat on the NDA:** the sentence is past-tense ("I have signed"). Your note says you'll sign in the morning before sending — just make sure the signature is actually returned *before* you hit send, so the statement is true at send time. If there's any chance it slips, soften to "I'm returning the signed NDA today."

**Trivial leftovers (optional, ~30 seconds):**
- Still one run-together space: "all (>99%) my development.The feedback loop" → add the space after the period.
- "PresenceAI was written in Python (finishing at ~190k LoC)" — "finishing at" still reads present-tense for a past company; "~190k LoC" alone is cleaner. Minor.
- A few "+" remain where prose would read smoother as "and": "Jetbrains + VSCode", "Software engineering principles + architecture", "strengths/weaknesses". Leave them if you like the terse feel — not worth blocking on.

None of these change the outcome. Sign the NDA, fix the one missing space if you're being thorough, and send it.
❯ Please review this NDA https://dochub.com/jan-qell5f/gYzM7mBwDB83ZbpKqJ6EoO/awear-nda-brian-takita-pdf
❯ Anything out of the ordinary? Anything to be aware of?

### Re: NDA review — anything out of the ordinary? — opus-4-8

I read the full document in the open DocHub tab (the PDF body renders to canvas, so I pulled it page-by-page via chromium-bridge — 3 pages, captured and read each). Here's the read.

##### What it is
A standard **one-way (unilateral) NDA**. Awear Inc. (Delaware corp; the "1209 Orange St, Wilmington" is just the Corporation Trust registered-agent address, not a real office) is the **Discloser**; you are the **Recipient**. It only protects *their* confidential information — nothing you tell them is covered. That's normal for a candidate/pre-engagement NDA. Effective date 5/23/2026; already signed by Ash Saulsbury (CEO) on their side. You still owe 4 fields: page initials + your signature + date (Title can be left blank or "N/A" — you're signing as an individual).

##### Nothing alarming — the protective carve-outs you'd want are present
- **§4(d) independent development** — info you develop independently, without use of or reference to their Confidential Information, is carved out. This is the important one for you: it protects your own tooling (agent-doc, etc.) as long as you don't build it *from* their materials.
- **§4** also has the usual prior-possession, third-party-source, public-knowledge, and compelled-by-law (with notice) carve-outs.
- **No non-compete, no non-solicit, no general invention/IP assignment** of your pre-existing or independent work. Good.

##### Two clauses to be aware of
1. **§7 Existence of Discussions** — neither party may, without the other's prior written consent, disclose, acknowledge, or confirm *that the discussions exist* or describe their subject matter. **This is the one that touches your workflow directly: do not name Awear or reference these talks on a livestream or any public channel without their written OK.** Private session docs are fine; public streams are not. Worth a conscious guard since you stream coding sessions.
2. **§11 Feedback assignment** — any "Feedback" (enhancements, corrections, suggestions) you give them *in connection with their Confidential Information* is voluntary, royalty-free, and you **assign all rights in it to them**. It's scoped to feedback about their confidential materials, not your general IP, so it's not a grab — but it's an *assignment*, not just a license. Practical takeaway: during the exploratory/interview phase, don't hand over substantive product or architecture contributions expecting to retain rights in them. Keep your own work clearly your own.

##### Minor nuances (not blockers)
- **Survival is long.** The 3-year "term" (§6) is only the window during which disclosures are covered. Obligations survive **5 years after termination** for ordinary confidential info and **indefinitely for anything that qualifies as a trade secret**.
- **"Executed as a sealed instrument"** (signature block) — in some states a sealed instrument carries a longer statute of limitations than an ordinary contract. Combined with the trade-secret survival, breach claims have a long tail. Standard boilerplate, just be aware the obligations don't quietly expire.
- **Broad CI definition** (§2): includes the catch-all "(c) would be apparent to a reasonable person … is of a proprietary or confidential nature" — so not everything has to be marked "Confidential" for the obligation to attach. Standard, but it means treat anything non-public they show you as covered.
- Governing law: **Delaware**. Remedies clause (§8) gives them injunctive relief / specific performance — standard.

##### Bottom line
Nothing predatory, nothing unusual for a candidate NDA — it's a clean, fairly standard unilateral agreement with the carve-outs that matter (especially independent development) intact. **Safe to sign.** The only two behavioral commitments to actually internalize: (1) **don't publicly disclose you're talking to Awear** (§7) — relevant to your streaming, and (2) **don't gift them substantive IP as "feedback"** during discussions (§11). Sign, initial the three pages, date it, and return.
<!-- agent:boundary:c549baac -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
