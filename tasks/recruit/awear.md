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
<!-- agent:boundary:a86559c9 -->
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
