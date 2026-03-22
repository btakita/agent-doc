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

**Re: LinkedIn post for YouTube video + blog post**

Links in the post body get deprioritized by LinkedIn's algorithm. First comment is the right move.

**Draft LinkedIn Post:**

Built a 60-minute screen recording of my agent-driven recruiter workflow — from resume tailoring to outreach, all orchestrated through tmux with multiple Claude Code sessions running in parallel.

What you'll see:
- Real-time spec-driven development with agent-doc interactive documents
- corky handling correspondence drafting and email management
- Multi-pane tmux routing where each agent handles one focused task
- The full loop: research → spec → build → review → ship

This is what 20+ years of software development looks like when you add agent tooling to the stack. Not vibe coding — structured collaboration with AI that follows the same engineering practices I've always used, just at a pace that wasn't possible before.

The tools shown (corky, agent-doc) are open source and published on crates.io.

**First comment (post immediately after):**

Full video: https://www.youtube.com/watch?v=IenfqO2vmAw
Blog post with context: https://briantakita.me/posts/agent-resume-recruiter-workflow-live
<!-- agent:boundary:3a5d3157-8d4c-4bd7-8023-922a24af7626 -->
<!-- /agent:exchange -->
