# Blog Post Outline: Building agent-doc with agent-doc

**Working title:** "Document-as-UI: How I built a tool by talking to it"

**Audience:** Developers using AI coding assistants, interested in workflow innovation

---

## 1. The Problem (200 words)

AI coding assistants are powerful, but the conversation interface is ephemeral. Context is lost between sessions. The human has to re-explain the project state every time. And there's no persistent artifact that captures the development narrative.

**Hook:** What if the document you're writing IS the conversation? What if your task tracker, architecture spec, and chat interface were the same file?

## 2. The Concept: Document-as-UI (300 words)

- Markdown file as the conversation surface
- Edits ARE prompts — no separate chat window
- Template mode: named components (`<!-- agent:name -->`) for structured documents
- The agent reads diffs, responds in-place, commits to git
- Real-time: response appears in the IDE via IPC patching

**Visual:** Side-by-side of the IDE showing a task.md with green gutter (user edits) and the Claude Code terminal streaming the response.

## 3. Case Study: agent-doc building itself (500 words)

Walk through the actual development of the boundary marker feature as it happened in the task document:

### Phase 1: Bug report
User notices response appearing above the prompt in youtube.md. Types the observation directly into the exchange component.

### Phase 2: Three proposals
Agent proposes byte offset, content hash, and boundary marker approaches. Presents a comparison table. User asks probing questions about robustness.

### Phase 3: Decision
User types "implement" — a single word that triggers the agent to build across 6 files (Rust binary + JetBrains plugin).

### Phase 4: Bug in the fix
The boundary marker search matched inside a fenced code block (lesson #13, third occurrence). User types "Please explain, add tests, and fix." Agent identifies the root cause, adds regression tests, and patches the code.

**Visual:** Git diff showing the boundary marker discussion in the exchange component, followed by the implementation commit.

## 4. Key Design Decisions (400 words)

- **CRDT merge** — user can keep typing while the agent responds (no locks, no conflicts)
- **Boundary markers** — physical anchors that move with edits (vs fragile byte offsets)
- **Selective commit** — agent response is committed, user's next prompt stays uncommitted (git gutter as visual feedback)
- **Compaction** — archive old conversation, keep the summary (lessons, architecture, release history persist)

## 5. Lessons from Dogfooding (300 words)

- The 16 lessons in the task document are a knowledge base that survives context compaction
- Lesson #13 ("skip code spans in ALL parsers") was learned three times — the document tracks the recurrence
- The tool improved itself through the same interface it provides to users
- Template mode emerged from the need to maintain structured project state alongside conversation

## 6. What's Next (200 words)

- Multi-agent backends (Codex, Gemini)
- Parallel fan-out with git worktrees
- TUI dashboard for monitoring active sessions
- The document format as a protocol, not just a tool

**Closing:** The best developer tools disappear into the workflow. agent-doc tries to make the markdown file you're already writing into a complete development interface.

---

**Estimated length:** ~2000 words
**Assets needed:**
- Screenshot: IDE with task.md open, showing git gutter colors
- Screenshot: Claude Code terminal streaming a response
- Git diff: boundary marker implementation commit
- Diagram: document flow (edit -> diff -> agent -> patch -> commit -> gutter)

**Publish targets:**
- GitHub repo README (condensed version)
- Blog (full version)
- Hacker News / Reddit (discussion post linking to blog)
