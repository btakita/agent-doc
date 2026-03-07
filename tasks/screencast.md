---
agent_doc_session: null
agent_doc_mode: template
---

# agent-doc Screencast

## Recording Setup

<!-- agent:setup -->
**Script:** `~/bin/screencast-toggle`
**Hotkey:** `$mod+F9` (Super + fn-layer F9) — toggle start/stop
**Output:** `~/Videos/screencast-<timestamp>.mp4`

**How it works:**
- Start: captures full screen at 30fps + system audio via ffmpeg x11grab + PulseAudio
- Stop: sends SIGINT to ffmpeg for clean file finalization
- Desktop notification on start/stop via `notify-send`
- Uses `libx264 ultrafast` preset — low CPU overhead, good quality (CRF 18)
- PID tracked in `/tmp/screencast-toggle.pid`

**Post-production:** `ffmpeg -ss 00:00:05 -to 00:01:05 -i input.mp4 -c copy output.mp4`

**Environment:** X11 + i3 + ffmpeg + pipewire-pulse | SliceMK split keyboard (F9 on fn layer key 41, fnpad key 25)
<!-- /agent:setup -->

## Blog Post Outline

<!-- agent:blog -->
1. **Hook**: "Your markdown file IS the chat interface"
2. **The problem**: Context windows overflow, conversations are ephemeral, agent responses disappear into terminal scrollback
3. **The solution**: agent-doc — document-as-UI with three interaction modes
4. **Modes explained**:
   - **Append**: Traditional chat. `## User` / `## Assistant` blocks. Linear history.
   - **Template**: Named components. Agent patches specific regions. Living documents.
   - **Exchange**: Shared conversation surface. User and agent write in the same space. Design discussions.
5. **The design story**: How `response_mode` became `agent_doc_mode`, how `input`/`output` evolved into `exchange`
6. **Demo link**: Screencast embedded
7. **What's next**: Multi-agent backends, TUI dashboard, plugin ecosystem
<!-- /agent:blog -->

## Screencast Script (~60 seconds)

<!-- agent:script -->
No fluff — compelling workflow demo. Let the workflow speak for itself. Real content, real project, real interaction.

| Time | Action | Voiceover |
|------|--------|-----------|
| 0:00 | Open a real project document in IDEA | "This is a real project. The document tracks architecture decisions." |
| 0:05 | Show existing components (status, architecture) | "Named components — the agent updates them in place" |
| 0:10 | Type a design question in the exchange | "I write directly in the document" |
| 0:15 | Hit submit | "One click sends to Claude" |
| 0:18 | Split view: terminal streaming + document updating | "Response streams in real time — into the document, not a chat window" |
| 0:30 | Add an inline annotation to the response | "I disagree with this — annotate inline, submit again" |
| 0:35 | Show revised response preserving the annotation | "3-way merge preserves my edits" |
| 0:45 | `git diff` in terminal | "Every change is tracked. The document IS the conversation history" |
| 0:55 | Quick cut: second document, different project | "Any markdown file. Any project." |
| 0:60 | End: document with rich component structure | "agent-doc — documents that work with you" |

**Tone:** Matter-of-fact. Not marketing fluff — a compelling short demo of the workflow.
**Tips:** Fast cuts, no pauses. Use a real project for authenticity. Focus on the "magic moment" — type, submit, response appears in-place. Side-by-side: terminal + editor.
**Format:** Pre-recorded session, edited before publishing. Use a demo project with no real secrets.
<!-- /agent:script -->

## Exchange

<!-- agent:exchange -->
<!-- /agent:exchange -->
