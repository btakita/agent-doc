---
agent_doc_session: 9c8a9b79-2d92-4000-8104-23c0606086a8
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

# Dave Franklin — BCG AI Factory Builder

## Context

Dave Franklin (Inspire Solutions) recruiting for **BCG AI Factory** — 12+ month contract, 100% remote, W2 ~$140-145/hr (~$230K + benefits gap). 10-20 person AI factory buildout. Role: execution-oriented AI-native builders shipping Claude Code agents and GenAI workflows.

**Call scheduled:** Tue Mar 4 at 12:30 PM EST.

## Files Created

- `mail/contacts/dave-franklin/CLAUDE.md` — contact file, role details, compensation, call summary
- `mail/contacts/dave-franklin/AGENTS.md` — symlink to CLAUDE.md
- `mail/contacts/dave-franklin/recordings/20260303-dave-franklin.txt` — 38 min call transcription
- `mail/contacts/dave-franklin/bcg-research.md` — BCG research ($13.5B rev, BCG X, agentic AI push, Anil connection)

## Resume — Consulting Profile

Built `resume/profiles/consulting.toml` targeting BCG per Dave's suggestions. Build: `make PROFILE=consulting`

**Changes applied:**
- Added **AI Products Shipped** section (Presence AI, corky, agent-doc, tagpath)
- Consolidated honk.com + TrueCar into single entry
- Censible dates: Jan 2016 – Apr 2024 (8 years)
- Removed: Freelance, SocialChorus, Open Source sections
- Cleaned duplicate phrases between tagline and summary
- **Document-as-interface arc:** Neo Innovation/Tenfore (2015) → agent-doc (2024) — Brian owns the IP
- **corky bullet:** topics, Whisper diarized transcription, social media posting, scheduled delivery, Python→Rust spec-driven rewrite in under 1 day
- Margins reduced → 3-page PDF. Proofread clean.

**Release velocity:** 57 releases across 3 Rust tools in 11 days (~5.2/day)

**IP note:** Brian owns Tenfore document-as-interface work (claimed ownership due to unpaid debt, granted Tenfore unrestricted license). No patents found.

**Anil Podduturi** — currently at BCG, managed Tenfore project at Neo Innovation. Warm path into BCG.

**Commits pushed:**
- `b0e82d5` (resume) — consulting profile, AI Products Shipped, Honk+TrueCar merge, Censible date fix
- `8a58944` (resume) — proofread fixes, corky expansion, margin reduction (4→3 pages)

**Status:** Resume sent to Dave. Ready for call.

## Assistant

Compacted. Reduced from 355 lines of back-and-forth to a structured reference document. All key context preserved: role details, files created, resume changes, IP notes, Anil connection, commit history, and current status.

## User

Connect with Anil
- BCG: Project Helix

True Engineer builders
People are "green"

I'm building AI:
What is it?
How did you build it?
Why did you build it?


I'm in consideration for Project Helix
If I gave you a name, could you put in a word for me?
Do you know anyone?

Steve Edison
Inspyr Solutions
Tony Tomai
Eric Rose

Please fill in tony-tomai contact with context.
Please transcribe + diarize the call in main/contacts/tony-tomai/recordings

<!-- agent:exchange patch=append -->
### Draft SMS response to Dave Franklin

Dave's messages (from `dave_franklin_sms.txt`):
> "Hi Brian, this is Dave Franklin with Inspyrsolutions. I tried calling and your voicemail is not working. Anyway, still waiting on next steps from BCG. There have been several team members out on spring break returning this week."

> "Hi Brian, this is Dave Franklin with Inspyrsolutions. Still waiting on BCG to advise on next steps with interviews, offers, etc. Since several team members have been out on spring break with families, still in a holding pattern. However, wanted to keep you in the loop. If I hear anything today or tomorrow, will advise."

**Draft reply:**

> Hey Dave, appreciate the update. No rush — totally understand the spring break timing. I'm available whenever they're ready to move forward. In the meantime I've been shipping steadily, happy to share anything that would help position me.

**Tone notes:**
- Warm but not anxious — mirrors Dave's matter-of-fact style
- "No rush" signals patience (you're not desperate)
- "Shipping steadily" is a subtle signal of continued momentum without listing specifics
- "Happy to share anything" opens the door without pushing content on him
- Short — matches SMS format and Dave's communication style

### Keep it simple — skip the LinkedIn posts/videos

**No** — don't mention them. Here's why:

1. **Dave already has your resume.** He knows what you build. Sending LinkedIn content feels like campaigning.
2. **He's a recruiter, not the decision-maker.** Extra content doesn't help him get your resume in front of BCG — it's noise.
3. **"Shipping steadily" is the right signal.** Active and productive without making him click links.
4. **Save content for BCG interviews.** That's when you demo agent-doc, show livestreams, etc.

**Exception:** If Dave specifically asks "what have you been working on?" — then share 1-2 LinkedIn posts as proof points. Don't volunteer it on a status update.

> If I hear anything today or tomorrow, will advise.

Should I follow up with Dave. He mentioned that he expected an answer by yesterday or today.
Should I mention that I will be having interviews next week?
### Follow-up strategy

**Yes, follow up today.** Dave set the expectation himself — "if I hear anything today or tomorrow, will advise." That window has passed, so a brief check-in is natural and expected:

**Draft SMS:**

> Hey Dave, hope you had a good week. Just checking in — any update from BCG on next steps?

That's it. Short, warm, no pressure. He gave you the opening by setting the timeline himself.

**Mention interviews — yes, but frame it right.** Don't use it as leverage or an ultimatum. Position it as a scheduling heads-up:

> Also wanted to give you a heads-up that I have some conversations happening next week, so if BCG wants to schedule anything it'd be good to get on the calendar soon.

**Why this works:**
- "Conversations" is softer than "interviews" — doesn't sound like a threat
- "Heads-up" frames it as courtesy, not pressure
- "Get on the calendar" gives Dave something actionable to relay to BCG
- Creates real urgency without burning the relationship

**Combined draft:**

> Hey Dave, hope you had a good week. Just checking in — any update from BCG on next steps? Also wanted to give you a heads-up that I have some conversations happening next week, so if BCG wants to schedule anything it'd be good to get on the calendar soon.

**Timing:** Send it today (Thursday) so Dave has Friday to relay it before the weekend.

It is friday, 17:30 Eastern. So it's already end of day for some.
Should we wait until Monday?
### Timing: Send now, not Monday

**Send it tonight.** Friday 5:30 PM is fine for SMS — it's asynchronous, no expectation of immediate response. Advantages:

- **Top of inbox Monday morning** if Dave doesn't check tonight
- **Natural follow-up window** — his "today or tomorrow" was earlier this week. By Monday, the connection to his timeline fades
- **Recruiters work weekends** — Dave is hustling placements, he's not off the grid

**Adjusted draft (for Friday evening):**

> Hey Dave, just checking in — any update from BCG on next steps? Also wanted to give you a heads-up that I have some conversations happening next week, so if BCG wants to schedule anything it'd be good to get on the calendar soon.

Dropped "hope you had a good week" since the week isn't over yet. Otherwise same message — it reads fine for a Friday evening text.

We also have Tony Tomai (account lead) and Anil Podduturi. Should I send messages to either of them?
### Who to message

**Dave Franklin — yes, send the SMS now** (the draft from above).

**Tony Tomai — no, not now.** Tony and Dave are both at Inspyr. Messaging both about the same status check creates noise and signals anxiety. Dave is your primary contact — let him work the channel. Only escalate to Tony if Dave goes dark for 5+ business days.

**Anil Podduturi — yes, but this is a separate play.** Anil is at BCG (not Inspyr). A LinkedIn message to Anil is a warm internal reference, not a recruiter follow-up. Tony specifically asked you to reach out to Anil before his March 10 BCG call — if you haven't done that, it's overdue.

**Key rule:** The message to Anil should NOT mention Dave, Inspyr, or the recruiter pipeline. It's a genuine reconnection from a former colleague, not a job ask. Something like:

> Hey Anil — it's been a while since the Neo Innovation/Tenfore days. I've been building some interesting AI tooling (document-as-interface agents, correspondence automation) and noticed you're leading AI products at BCG. Would love to catch up if you have a few minutes.

This lets Anil connect the dots himself. If he's involved in Project Helix hiring, he'll make the connection. If you mention the recruiter pipeline, you bypass the internal referral effect.

**Summary — send tonight:**
1. Dave Franklin SMS (the draft above) — recruiter check-in + scheduling urgency
2. Anil Podduturi LinkedIn message — warm reconnection, separate from recruiter channel
3. Skip Tony for now

I already sent Anil a message. He has not read it. Tony asked me to mention that I'm going through the recruiter.

Should I edit this or send a followup?
### Anil follow-up: send a new message, don't edit

**Send a follow-up.** Don't edit the original. Reasons:

1. **Editing may not re-notify** — LinkedIn doesn't always push notifications for edited messages. A new message gets a fresh notification.
2. **Two messages > one** — it looks like you thought of something after the fact, which is natural and human.
3. **The original message stands on its own** — a warm reconnection. The follow-up adds the recruiter context Tony asked for.

**Draft follow-up:**

> By the way — I'm also going through Inspyr Solutions for a BCG opportunity (Project Helix). Tony Tomai suggested I mention that. Would be great if our paths crossed on it.

**Why this works:**
- "By the way" makes it feel like an afterthought, not a calculated move
- Names Tony directly (as Tony requested)
- "Our paths crossed" is softer than "can you put in a word for me"
- Keeps it short — one message, one ask

**Timing:** Send it right after the Dave SMS. Both messages go out tonight.

Please read my message at /home/brian/work/btakita/agent-loop/mail/contacts/anil-podduturi/20260320-linkedin-anil-podduturi.txt

And draft a followup if recommended.
### Review of Anil message + follow-up recommendation

**Your March 10 message is solid.** It already mentions Inspyr Solutions, Project Helix, the Tenfore connection, and asks directly. Tony's request is already fulfilled in the original.

**No follow-up to Anil right now.** He hasn't read it — a second message just stacks two unread messages. It doesn't increase open rate and can feel pushy.

**Better moves:**
1. **Send a connection request** if you're not already connected — connection requests get more prominent notifications than messages (especially if the message landed in his "Other" inbox)
2. **Ask Tony to mention you to Anil directly** — an internal ping from someone Anil works with at BCG is worth 10 LinkedIn messages
3. **Wait for Dave's response** — if BCG is actively moving on Helix, Anil may surface through the interview process naturally

**Action for tonight:** Just the Dave SMS. Revisit Anil Monday if Dave confirms movement.
<!-- /agent:exchange -->
