---
agent_doc_session: e75d497d-f147-4953-a2af-5966a4e34a20
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
See https://x.com/tannerlinsley/status/2035062107339997458

This is who I can target with agent-doc.
### Re: Tanner Linsley tweet

Can't fetch the tweet — X blocks automated scraping. Could you paste the tweet text here so I can analyze the audience angle?

In the meantime, **Tanner Linsley context:** creator of TanStack (React Query, TanStack Router, TanStack Table, TanStack Start). His audience is React/TypeScript developers building production apps — heavily focused on developer experience, state management, and framework tooling.

**If agent-doc targets this audience, the pitch would be:**
- Developers who already use AI coding assistants but want structured, persistent sessions (not ephemeral chat)
- Document-as-interface pattern appeals to devs who think in files, not chat windows
- TanStack community values DX-first tools with clean APIs — agent-doc's CLI-native approach fits

Please paste the tweet text so I can give a more specific take.

```
Ghostty was fun, but time for something else. 

I still love opencode, too but with CC plans dead on it… I’m feeling lost. 

Full GUI?
T3 Code?
Opencode GUI?
Warp?
Back to cursor?
Try CC again?
Raw Codex?

My 🧠 hurts  and I just need to keep shipping.
```
### Tanner's pain = agent-doc's pitch

Tanner's tweet distills the exact problem agent-doc solves. He's not complaining about AI quality — he's complaining about **the interface layer**. Every tool he lists (Ghostty, opencode, CC, Cursor, Warp, T3 Code, Codex) is a different *shell* around the same LLM. The UX is the bottleneck, not the model.

**agent-doc's angle:** You don't need another app. You need a file.

The document-as-interface pattern eliminates the tool-switching problem because:
- **Your editor IS the interface** — no new app to learn, no new keybindings, no new mental model
- **Sessions persist as markdown files** — git-tracked, diffable, searchable, portable
- **Works with any LLM backend** — the document is the protocol, not the client
- **"Just keep shipping"** — edit a file, hit submit, get response inline. No context window panic, no UI chrome

**Draft reply to Tanner:**

> The tool-switching fatigue is real. I hit the same wall and built something different: a markdown file IS the interface. Edit in your editor, submit via CLI, response appears inline. No app, no GUI, no new keybindings. Sessions persist as git-tracked files.
>
> It's called agent-doc. Open source, works with Claude Code. The document is the protocol.
>
> https://github.com/btakita/agent-doc [replace with real link]

Why do you keep on using `https://github.com/nicholasgasior/agent-doc`?
The url is https://github.com/btakita/agent-doc

**Tone calibration:**
- Lead with empathy (you felt the same pain)
- Don't trash any specific tool (Tanner has relationships with these teams)
- "Document is the protocol" is the sticky phrase — it reframes the problem
- Link to the repo, not a landing page — devs want to see code
- Keep it under 280 chars if possible, or use a thread

**Should you reply directly?** Yes. Tanner has massive reach (100K+ followers). A genuine reply from someone who built a solution to the exact pain he's describing is valuable signal, not spam. But it needs to be authentic — you actually built this, and the approach is genuinely different from everything he listed.

**Alternative: quote tweet** with a short demo video of agent-doc in action (editor + CLI, no GUI). Visual proof > text claims.

One issue I have with agent-doc is I have done no user testing. I'm not sure what the install + onboarding experience will be like.
### URL corrected + onboarding gap

**URL fix:** Sorry — that was a hallucinated URL. Correct link: https://github.com/btakita/agent-doc

**Updated draft reply:**

> The tool-switching fatigue is real. I hit the same wall and built something different: a markdown file IS the interface. Edit in your editor, submit via CLI, response appears inline. No app, no GUI, no new keybindings. Sessions persist as git-tracked files.
>
> It's called agent-doc. Open source, works with Claude Code.
>
> https://github.com/btakita/agent-doc

### Onboarding gap — real concern, manageable risk

You're right that no user testing is a gap. But here's the calculus:

**Why it's okay to reply anyway:**
- Tanner is a *builder*. He'll read the README, clone, and try it. He won't expect a polished onboarding wizard.
- His tweet says "I just need to keep shipping" — he's looking for pragmatic tools, not consumer products
- Open source = expectation of rough edges. The question is whether the *concept* lands, not the install experience.

**What could go wrong:**
- `cargo install` fails on his system (missing deps, Rust version)
- README doesn't explain the mental model clearly enough
- No quick-start example to copy-paste

**Minimum viable onboarding before replying:**
1. **Test `cargo install agent-doc` on a clean-ish system** — does it build?
2. **README has a 30-second quick-start** — `cargo install agent-doc && agent-doc init plan.md && agent-doc submit plan.md`
3. **One working example file** in the repo someone can clone and try immediately

**Alternative approach:** Reply to Tanner without the link first. Just describe the concept. If he's interested ("that sounds cool, link?"), then you share the repo. This gives you time to polish onboarding while still planting the seed.

> Same pain here. I stopped chasing tools and made the document the interface — markdown file in your editor, CLI submit, response inline. No app, no GUI. Sessions persist as git-tracked files. Been shipping with it for weeks.

This version has no link, no name-drop. It's a concept pitch. If Tanner engages, you follow up with the repo.

Can we use lightpanda to scrape the twitter post?
Do you need my credentials to login using lightpanda?
### Lightpanda + X scraping

Lightpanda is installed but **X blocks it the same way** — returns "Something went wrong, try again" without a logged-in session. X requires authentication to view tweets.

**Don't pass credentials through lightpanda** — it's a headless browser that would need to handle X's OAuth flow, 2FA, etc. Not worth the security risk for one tweet.

**Good news: we already have the tweet text** (you pasted it earlier). No scraping needed for this use case. If you want to scrape X threads regularly, the better path would be:
1. **Nitter instances** (if any are still alive) — unauthenticated X frontend
2. **X API** with a developer account ($100/mo basic tier)
3. **Browser extension** that exports tweet threads to clipboard/file
4. **Manual paste** (what you did — works fine for occasional use)

For now, the tweet content is captured and we have the draft replies ready. Want to finalize which version to send?
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
