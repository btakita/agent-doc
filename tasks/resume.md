See https://www.linkedin.com/in/briantakita/

Please review my LinkedIn profile for more details. Here is my summary and most recent work experience. I will also add corky/agent-doc to highlight my work in the coding agent space. I want to emphasize that I am striving for a good process and making significant gains.

Over 20 years of software development experience developing full-stack applications. Currently focused on the full pipeline of Realtime AI Avatars using Generative models.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Presence AI logo
Lead Backend Engineer, AI Pipeline
Lead Backend Engineer, AI Pipeline
Presence AI · Full-timePresence AI · Full-time
Apr 2024 - Present · 1 yr 11 mosApr 2024 to Present · 1 yr 11 mos
- Architected and shipped the entire real-time video pipeline: facial landmark detection, LatentSync lip-sync inference (VAE decode, denoising, restoration), idle video generation, and frame sequencing on NVIDIA H100 GPUs via LiveKit
- Architected a multi-service AI monorepo with closed-loop agentic workflows and self-auditing development infrastructure (Claude Code / Codex)
- Developed an iterative agentic development methodology: lightweight living specs refined through closed-loop agent auditing and continuous human-in-the-loop feedback, accelerating design velocity while staying in flow with the emerging design
- Built reusable async primitives for high-quality agent-authored code and human comprehension: task lifecycle management, scoped cancellation, lazily resolved dependencies that decouple initialization order and cascade invalidation through the context, runtime profile logging, and stable cross-stack naming to maintain conceptual coherence for both human and agent reasoning
- Improved lip-sync quality through landmark temporal smoothing, LatentSync model upgrades, and optimized Whisper audio feature extraction via improved audio stream batching

## Assistant

### Review of Current Content

**Presence AI bullets are strong.** The arc from pipeline architecture → agentic methodology → async primitives → measurable quality improvement tells a coherent story. Key observations:

- **Bullet 3 (iterative agentic methodology)** is your most differentiated point — it positions you as someone who's designed *how to work with agents*, not just used them. This is rare and valuable.
- **Bullet 4 is dense.** "Lazily resolved dependencies that decouple initialization order and cascade invalidation through the context" packs two major ideas into one clause. Consider splitting into two bullets or simplifying to the core insight: dependency injection that works for both agent-generated and human-authored code.
- **Bullet 2** — "self-auditing development infrastructure" is compelling but vague. What does the audit loop catch? Spec drift? Regressions? Dead code? A concrete example would sharpen this.

### Suggested Additions: corky & agent-doc

These projects reinforce your "agentic development" narrative. Here's how I'd frame them:

**Open Source / Personal Projects**

**agent-doc** — Document-as-UI for Persistent AI Conversations (Rust)
- Built a novel interaction model where a markdown document becomes the interface for long-running AI agent sessions — edit offline, submit diffs, agent responds inline
- Engineered write-safe concurrent editing via 3-way merge (user edits + agent response + original), enabling uninterrupted human-in-the-loop workflows
- Designed agent-agnostic architecture with trait-based LLM backends, snapshot-based diffing (SHA256 + `similar` crate), and automatic git integration for version history

**corky** — Multi-Source Email Unification & AI-Assisted Drafting (Rust, ~9,200 LOC)
- Developed a correspondence toolkit that syncs email threads from multiple IMAP accounts (Gmail, Protonmail) into unified markdown conversations with smart deduplication
- Built crash-safe incremental IMAP sync with streaming merge — messages written immediately, sync state committed only on success
- Implemented 3-way contact sync using FNV-1a content hashing, plus bulk import normalization for Slack workspace exports and Telegram archives

### Framing Advice

The thread connecting Presence AI + agent-doc + corky is: **you're building the tooling and methodology for agent-augmented development, not just consuming it.** Your resume should make that explicit. Consider a brief "Technical Philosophy" or updated summary line like:

> "Building the tooling, primitives, and methodology for agent-augmented software development — from real-time AI pipelines to the developer workflows that produce them."

This positions you at the intersection of AI engineering and developer tooling, which is exactly where the industry is heading.

## User

Does linkedin provide an api so we can make these changes together?

## Assistant

LinkedIn's API is extremely restrictive for profile editing — the Profile API is read-only unless you're an approved LinkedIn partner. No public API exists to update experience bullets, summary, or project descriptions.

**What works:**
1. **I draft, you paste** — I write the final polished text for each section, you copy into LinkedIn's editor. The hard part is the writing, not the data entry.
2. **LinkedIn's "Import from resume"** — upload a PDF and LinkedIn suggests updates, but it's unreliable.
3. **Browser automation** (Playwright) — technically possible but fragile, against ToS, risks account restrictions.

Option 1 is the practical path. Want me to draft the final copy-paste versions for your summary, Presence AI bullets, and corky/agent-doc project entries?

## User

Option 1 works for me.

Let's start with the personal projects first. I like those. I want to emphasize that corky also does topic contexts for the agent + collaborators. It is a git based tool that handles syncing across multiple mailboxes with different scopes...and uses the git workflow to handle updates and merges.

Likewise, agent-doc is also a git based tool. It uses the git workflow to handle updates and merges.

## Assistant

Here are the copy-paste ready LinkedIn project entries:

---

**corky** — Git-Based Correspondence Toolkit for Agent-Assisted Email (Rust)

- Built a git-native correspondence system that unifies email threads from multiple IMAP accounts (Gmail, Protonmail) into scoped markdown mailboxes, with per-topic and per-contact agent context that syncs across collaborators via 3-way merge
- Designed multi-scope mailbox architecture where each collaborator receives only relevant threads and context, with bidirectional sync using content hashing (FNV-1a) and git-based conflict resolution for offline-first collaboration
- Implemented crash-safe incremental IMAP sync with streaming merge, bulk import for Slack and Telegram archives, and label-based routing rules that fan out threads to multiple mailboxes

**agent-doc** — Git-Native Document Sessions with AI Agents (Rust)

- Created a document-as-interface model for persistent AI conversations: edit a markdown file offline, submit diffs, agent responds inline — all tracked through git commits for full session history and branch-based workflows
- Engineered write-safe concurrent editing via 3-way merge (snapshot + user edits + agent response), preventing data loss when human and agent edit simultaneously
- Built agent-agnostic core with pluggable LLM backends, automatic pre-commit of user changes, and snapshot-based diffing so only changed content is sent to the agent

---

Key shifts from the previous draft:
- **corky**: Led with "git-native" and scoped collaboration. Added topic/contact context syncing and 3-way merge as a collaboration feature.
- **agent-doc**: Led with "git-native" and offline editing. Emphasized git commits for session history and branch workflows.

Ready for the Presence AI bullets and summary next.

## User

I would like to link to these projects. I also want to mention that I am dog-fooding both tools.
Does the README.md for agent-doc and corky have a link to the mdbook site as well?
Are there clear install instructions for both tools?

## Assistant

### Links & Docs Status

**corky** — Good shape:
- GitHub: https://github.com/btakita/corky
- mdbook docs: https://btakita.github.io/corky (linked prominently in README)
- Install: Clear — `pip install corky`, shell installer (`curl | sh`), or `cargo install --path .`

**agent-doc** — Gaps to fill:
- GitHub: https://github.com/btakita/agent-doc
- mdbook docs: **None** — no docs site exists or is linked in the README
- Install: Only `cargo install --path .` (from source). The README doesn't mention that it's on crates.io or PyPI, even though plan.md says it is. If it's published, the README should say `cargo install agent-doc` and/or `pip install agent-doc`.

### Recommendations Before Linking on LinkedIn

1. **agent-doc README install section** — update to show `cargo install agent-doc` (if published to crates.io) and `pip install agent-doc` (if published to PyPI). A visitor clicking through from LinkedIn should be able to install in one command.
2. **agent-doc docs site** — optional but nice. For now the README is solid enough to stand alone.
3. **Both repos**: Make sure the GitHub repos are public and the READMEs render well — that's what a recruiter or hiring manager will see.

### Updated Copy-Paste with Links + Dog-Fooding

---

**corky** — Git-Based Correspondence Toolkit for Agent-Assisted Email (Rust)
https://github.com/btakita/corky | Docs: https://btakita.github.io/corky

- Built a git-native correspondence system that unifies email threads from multiple IMAP accounts (Gmail, Protonmail) into scoped markdown mailboxes, with per-topic and per-contact agent context that syncs across collaborators via 3-way merge
- Designed multi-scope mailbox architecture where each collaborator receives only relevant threads and context, with bidirectional sync using content hashing (FNV-1a) and git-based conflict resolution for offline-first collaboration
- Implemented crash-safe incremental IMAP sync with streaming merge, bulk import for Slack and Telegram archives, and label-based routing rules that fan out threads to multiple mailboxes
- Dog-fooding daily for personal and professional correspondence across Gmail and Protonmail accounts

**agent-doc** — Git-Native Document Sessions with AI Agents (Rust)
https://github.com/btakita/agent-doc

- Created a document-as-interface model for persistent AI conversations: edit a markdown file offline, submit diffs, agent responds inline — all tracked through git commits for full session history and branch-based workflows
- Engineered write-safe concurrent editing via 3-way merge (snapshot + user edits + agent response), preventing data loss when human and agent edit simultaneously
- Built agent-agnostic core with pluggable LLM backends, automatic pre-commit of user changes, and snapshot-based diffing so only changed content is sent to the agent
- Dog-fooding as the primary interface for planning and iterating on both corky and agent-doc development — this resume session itself was conducted via agent-doc

---

The dog-fooding bullets are powerful because they demonstrate confidence in your own tools. The last line about *this resume being written via agent-doc* is a nice meta-touch.

Ready for the Presence AI bullets and summary whenever you are.

## User

Create a post with corky and agent-doc. 100% dog-fooding...spec-driven development. Ported from a Python prototype to Rust in less than a day. 100% open source.

## Assistant

### LinkedIn Post — Copy-Paste Ready

---

**Building my own agent tooling — and eating my own dog food**

I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:

**corky** — a git-native correspondence toolkit that syncs email threads from multiple IMAP accounts into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for all my personal and professional correspondence.

**agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.

Corky was started as a Python prototype. I ported Corky over to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.

With agent-doc, I use task/plan.md and other task documents to run multiple agents in parallel. While having the coherent session in the agent document itself. The nice thing about it is that it can be "compacted"...to organize the state of the session.

100% dog-fooding. 100% open source. 100% spec-driven development.

corky: https://github.com/btakita/corky
agent-doc: https://github.com/btakita/agent-doc

The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.

#OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

---

The post hits your key themes: dog-fooding as quality signal, spec-driven development as methodology, Python-to-Rust speed as proof of the approach, and open source as commitment. The closing insight about dog-fooding creating an instant feedback loop gives it a takeaway that's useful to anyone, not just people interested in your specific tools.

Want me to adjust the tone, length, or emphasis?

## User

I like it. It's casual + professional. I added the part about compacting the agent doc. Please review and make it more presentable.
