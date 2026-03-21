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
### Tanner's pain = agent-doc's pitch (HEAD)

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
> https://github.com/nicholasgasior/agent-doc [replace with real link]

**Tone calibration:**
- Lead with empathy (you felt the same pain)
- Don't trash any specific tool (Tanner has relationships with these teams)
- "Document is the protocol" is the sticky phrase — it reframes the problem
- Link to the repo, not a landing page — devs want to see code
- Keep it under 280 chars if possible, or use a thread

**Should you reply directly?** Yes. Tanner has massive reach (100K+ followers). A genuine reply from someone who built a solution to the exact pain he's describing is valuable signal, not spam. But it needs to be authentic — you actually built this, and the approach is genuinely different from everything he listed.

**Alternative: quote tweet** with a short demo video of agent-doc in action (editor + CLI, no GUI). Visual proof > text claims.
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
