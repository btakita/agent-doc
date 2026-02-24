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

## Assistant

Your inline edits are solid — the corky-only Python origin is an important correction, and the compaction concept is a strong addition. I tightened the agent-doc paragraph and polished the flow. Here's the final version:

---

**Building my own agent tooling — and eating my own dog food**

I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:

**corky** — a git-native correspondence toolkit that syncs email threads from multiple IMAP accounts into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for all my personal and professional correspondence.

**agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.

Corky started as a Python prototype. I ported it to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.

With agent-doc, I run multiple agent sessions in parallel across different task documents — each one a coherent, persistent conversation. When a session gets long, I compact it: reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run.

100% dog-fooding. 100% open source. 100% spec-driven development.

corky: https://github.com/btakita/corky
agent-doc: https://github.com/btakita/agent-doc

The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.

#OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

---

**Changes from your edit:**
- Tightened "I use task/plan.md and other task documents to run multiple agents in parallel" → "I run multiple agent sessions in parallel across different task documents"
- Made the compaction description concrete: "reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run."
- Removed the ellipsis ("compacted"...) for cleaner flow
- Kept your "100% spec-driven development" addition

The post in the document body (lines 165-184) still has your raw edits. You can either copy from this polished version or I can update the post inline — let me know which you prefer.

## User

show me the current version

## Assistant

Here's the current polished version — clean copy-paste block:

---

**Building my own agent tooling — and eating my own dog food**

I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:

**corky** — a git-native correspondence toolkit that syncs topics, contacts, email threads from multiple IMAP accounts, Telegram, Slack...into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for my personal and professional correspondence.

**agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.

Corky started as a Python prototype. I ported it to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.

With agent-doc, I run multiple agent sessions in parallel across different task documents — each one a coherent, persistent conversation. When a session gets long, I compact it: reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run.

100% dog-fooding. 100% open source. 100% spec-driven development.

corky: https://github.com/btakita/corky
agent-doc: https://github.com/btakita/agent-doc

The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.

#OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

---

## User

Iterate on draft

## Assistant

Your inline edit to corky's description is the right move — adding topics, contacts, Telegram, and Slack shows the breadth. The sentence got long and the ellipsis breaks the professional tone. Here's the tightened version:

---

**Building my own agent tooling — and eating my own dog food**

I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:

**corky** — a git-native correspondence toolkit that syncs topics, contacts, and threads from email (IMAP), Telegram, and Slack into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for my personal and professional correspondence.

**agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.

Corky started as a Python prototype. I ported it to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.

With agent-doc, I run multiple agent sessions in parallel across different task documents — each one a coherent, persistent conversation. When a session gets long, I compact it: reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run.

100% dog-fooding. 100% open source. 100% spec-driven development.

corky: https://github.com/btakita/corky
agent-doc: https://github.com/btakita/agent-doc

The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.

#OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

---

**What changed:**
- Restructured corky line: "syncs topics, contacts, and threads from email (IMAP), Telegram, and Slack" — cleaner than the ellipsis version, lists three data types before three sources
- Removed "all" from "all my personal" (your edit) — reads tighter

## User

Does the linkedin api support creating posts?

## Assistant

Yes — unlike profile editing, LinkedIn's API **does** support creating posts.

**Posts API**: `POST https://api.linkedin.com/v2/posts` ([docs](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/posts-api))
- Scope needed: `w_member_social`
- Supports: text posts, articles, link shares, image/video, mentions

**Setup**:
1. Create a LinkedIn Developer App at developer.linkedin.com
2. Request `w_member_social` + `r_liteprofile` scopes
3. OAuth 2.0 flow (browser redirect → auth code → access token)
4. POST with your content

The OAuth flow is the annoying part — browser redirect to authorize, then exchange for a token. Once you have the token, publishing is a single API call.

Want me to draft a script to publish the post? Could be a small Python or Rust script, or even a `curl` one-liner once you have the token.

## User

How do I export my resume with my agent-doc and corky projects in the experience?

## Assistant

Two parts: getting projects into the Experience section, and exporting.

### Getting Projects into Experience

LinkedIn has a "Projects" section, but it doesn't export cleanly to PDF and recruiters skim past it. **Add them as an Experience entry instead:**

- **Title**: Creator / Solo Developer (or "Independent Software Engineer")
- **Company**: Open Source (or self-employed)
- **Date range**: when you started → Present
- **Description**: the corky + agent-doc bullets we've been drafting

This puts them in the Experience section of any export, right alongside Presence AI.

### Exporting

1. **LinkedIn "Save to PDF"**: Profile > More > Save to PDF. Quick but limited formatting — it exports Experience, Education, Skills. Good enough for most purposes.
2. **Build your own resume**: Take the content we've drafted here and put it in a markdown or LaTeX template you control. Better formatting, not dependent on LinkedIn's renderer, and you can tailor per-application.

Option 2 is the power move — you already have all the copy-paste ready content from this session. A clean markdown resume rendered to PDF (via Pandoc, or a tool like Reactive Resume) gives you full control.

Want me to draft a complete markdown resume file combining your Presence AI experience + the open source projects?

## User

Yes, please. Markdown resume. My LinkedIn profile resume is at /home/brian/Documents/resume/BrianTakita-20260224.pdf

My profile is at https://www.linkedin.com/in/btakita/

## Assistant

Done. Created `/home/brian/Documents/resume/BrianTakita.md` — a complete markdown resume with all 6 pages from your LinkedIn PDF plus the open source projects.

**What's new vs the PDF:**
- **Open Source — Creator / Solo Developer** entry added between Presence AI and your freelance work, with the polished corky + agent-doc bullets and dog-fooding lines
- All experience entries preserved from the PDF, cleaned up for markdown formatting
- Links made clickable (GitHub repos, project URLs)
- Consistent date formatting and heading structure

**To render as PDF:**
```sh
pandoc /home/brian/Documents/resume/BrianTakita.md -o /home/brian/Documents/resume/BrianTakita.pdf
```
Or use a styled template for better formatting:
```sh
pandoc BrianTakita.md -o BrianTakita.pdf --pdf-engine=xelatex -V geometry:margin=1in
```

The file is at `/home/brian/Documents/resume/BrianTakita.md` — edit it directly or continue iterating here.

## User

Let's move BrianTakita.md into a resume repo. This repo will be a submodule of agent-loop.

## Assistant

Good idea — keeps the resume versioned and independent. Before I set this up, a few questions:

1. **Repo visibility**: Public (recruiters can browse directly from GitHub) or private?
2. **Repo name**: `btakita/resume`? Or something else?
3. **Submodule path**: `resume/` in agent-loop?
4. **Session doc**: Keep `tasks/resume.md` here in agent-loop (it's the agent-doc session, not the resume itself), or move it into the new repo?

Once you confirm, I'll create the GitHub repo, initialize it, move `BrianTakita.md` in, and add it as a submodule.

## User

1. Public
2. btakita/resume
3. resume/
4. keep tasks/resume.md here

## Assistant

Done. Set up the resume repo:

1. Created **public** repo: https://github.com/btakita/resume
2. Initialized with `BrianTakita.md` and pushed to main
3. Added as submodule at `resume/` in agent-loop
4. Session doc stays here at `tasks/resume.md`

**Current state:**
- `resume/BrianTakita.md` — your markdown resume (submodule, own git history)
- `tasks/resume.md` — this agent-doc session (agent-loop repo)
- `/home/brian/Documents/resume/BrianTakita.md` — the original copy (can be deleted or symlinked)

The submodule is staged but not committed to agent-loop yet. To edit the resume, work directly in `resume/BrianTakita.md` and commit/push from within the submodule.

## User

commit and push. Ignore instruction-files for now since another agent is working on it.

## Assistant

Done. Committed snapshots and pushed all 23 commits to remote:

- **Resume submodule** was already committed (auto-staged by `git submodule add`)
- **Snapshots** committed for both resume and plan sessions
- **instruction-files/** left uncommitted as requested
- **Pushed** `cdb92db..310ef38 main -> main`

Everything is live:
- https://github.com/btakita/resume — public resume repo
- https://github.com/btakita/agent-loop — updated with resume submodule

## User

Generate the pdf

## Assistant

Done. Generated `resume/BrianTakita.pdf` using pandoc + weasyprint with custom CSS.

**Pipeline**: `BrianTakita.md` → pandoc (HTML) → weasyprint (PDF) with `resume.css`

**Files in resume repo:**
- `BrianTakita.md` — source
- `BrianTakita.pdf` — generated PDF
- `resume.css` — print stylesheet

**Fixes applied:**
- Hidden pandoc's auto-generated title block via CSS (`header#title-block-header { display: none }`)
- Contact info formatted with pipe separators on one line

**To regenerate:**
```sh
pandoc resume/BrianTakita.md -o /tmp/resume.html --standalone --metadata title="Brian Takita" && weasyprint /tmp/resume.html resume/BrianTakita.pdf --stylesheet resume/resume.css
```

Note: TeX Live format files are broken on your system (`xelatex.fmt` / `pdflatex.fmt` missing). The weasyprint path works well as an alternative. If you want the LaTeX path, you'll need `texlive-core` reinstalled or format files regenerated.

## User

Let's remove Productivity without the Bloat...because almost nobody cares. Let's focus on spec-driven development & software for humans and agents. Print what you propose here first.

Remove the SLOC metric from my rust project.

## Assistant

### Proposed Changes

**Tagline (line 3):**

Current:
> First principles emphasize powerful primitives, bottom-up development discipline, & systems with an observable domain ontology. Full stack developer with over 20 years experience. Productivity without the bloat.

Proposed:
> Spec-driven development with AI agents. Building software that works for both humans and agents — from real-time AI pipelines to the developer tooling that produces them. Full stack developer with over 20 years experience.

The greatest relevance is tooling for agent loops to quickly create emergent software systems. I'm struggling to capture the entire concept with a single sentence. But I'll list out what I want to capture and we can distill the essence from there:
- Spec-driven development for AI agents
- Emergent software systems from agent loops
- Automated testing
- agent-doc to run multiple agent sessions with fully readable context in parallel
- agent-loop tools for collaborative intelligence

**Corky project heading:**

Current:
> **corky** — Git-Based Correspondence Toolkit (Rust, ~9,200 LOC)

Proposed:
> **corky** — Git-Based Correspondence Toolkit (Rust)

Approve and I'll apply both changes to `resume/BrianTakita.md` and regenerate the PDF.

## User

The other changes look look. Need to iterate on the tagline.

## Assistant

Applied the SLOC removal to `resume/BrianTakita.md`. Now iterating on the tagline.

Your five themes distilled: **you design the specs, tools, and workflows that let agents produce emergent software systems — and you verify the results.**

Three options:

**A** (methodology-focused):
> Spec-driven agent development. Designing the methodology, tooling, and verification loops that turn AI agents into reliable software collaborators. Full stack developer with over 20 years experience.

**B** (systems-focused):
> Building emergent software systems through agent loops — spec-driven development, automated verification, and collaborative intelligence tooling where agents and humans work in parallel with full context. 20+ years full stack experience.

**C** (concise):
> Spec-driven development with AI agents. Building the tooling for collaborative intelligence — agent loops that produce emergent, verified software systems. Full stack developer with over 20 years experience.

Pick one, remix, or edit inline.

## User

B

## Assistant

Applied Option B tagline and regenerated PDF.

**Updated `resume/BrianTakita.md`:**
- Tagline: "Building emergent software systems through agent loops — spec-driven development, automated verification, and collaborative intelligence tooling where agents and humans work in parallel with full context. 20+ years full stack experience."
- Corky: SLOC metric removed (applied earlier)

PDF regenerated at `resume/BrianTakita.pdf`. Both files ready to commit and push to the resume repo.

## User

My Presence job has ended as of February 2026. I will update Linked In as well.

Help me with about as well:

Over 20 years of software development experience developing full-stack applications. Currently focused on the full pipeline of Realtime AI Avatars using Generative models.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

## Assistant

Updated the resume:

**Changes to `resume/BrianTakita.md`:**
- Presence AI dates: "April 2024 – Present" → "April 2024 – February 2026"
- Summary: "Currently focused on the full pipeline of Realtime AI Avatars" → "Most recently built the full real-time AI avatar pipeline at Presence AI. Now focused on spec-driven agent tooling and collaborative intelligence workflows."

PDF regenerated at `resume/BrianTakita.pdf`.

### LinkedIn About Section — Updated Draft

Here's a copy-paste replacement for your LinkedIn About:

---

Over 20 years of software development experience developing full-stack applications. Most recently built the full real-time AI avatar pipeline at Presence AI — facial landmark detection, LatentSync lip-sync inference, and frame sequencing on H100 GPUs. Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Currently dog-fooding two open-source tools: corky (git-native correspondence toolkit) and agent-doc (persistent AI document sessions) — both built through the same spec-driven agent methodology they enable.

---

This keeps the original philosophical paragraph intact while updating the focus and adding a forward-looking paragraph about your current work. The corky/agent-doc mention ties your About to your Experience entries.

Want to adjust the tone or emphasis?

### LinkedIn Presence AI Update

For the Presence AI entry on LinkedIn, change the date from "Present" to "February 2026". The bullets can stay as-is — they're strong and the past tense reads naturally since the bullets describe completed work.

## User

Thank you. Find the lucal PAT on my github. Give the lucas PAT push permission on the btakita/resume repo.

## Assistant

**Cannot manage PATs via API.** GitHub's REST API doesn't expose personal access token management — only the web UI does.

### Steps to add `btakita/resume` to your PAT

1. Go to **https://github.com/settings/tokens?type=beta** (fine-grained tokens page)
2. Find the token named "lucas" (or "lucal")
3. Click **Edit**
4. Under **Repository access**, add `btakita/resume` to the selected repositories
5. Under **Permissions → Repository permissions**, ensure **Contents** is set to **Read and write** (grants push)
6. Click **Regenerate token**

If it's a **classic PAT** (at https://github.com/settings/tokens), it already has access to all repos matching its scopes — no per-repo config needed. Only fine-grained PATs need per-repo setup.

## User

add a gh action to generate the pdf using pandoc, commit, and push to the branch

Make a gh page with the pdf

## User

Add Makefile to build the resume pdf

## Assistant

Done. All three pieces implemented, committed, and pushed to `btakita/resume`:

### 1. Makefile

```makefile
make pdf    # builds BrianTakita.pdf from .md + .css
make clean  # removes generated PDF
```

Uses pandoc (md → html) + weasyprint (html → pdf) with `resume.css`. Tested locally — works.

### 2. GitHub Action (`.github/workflows/build-pdf.yml`)

- **Triggers**: push to `BrianTakita.md`, `resume.css`, or `Makefile` (plus manual `workflow_dispatch`)
- **Build job**: installs pandoc + weasyprint, runs `make pdf`, commits and pushes the PDF
- **Deploy job**: copies PDF + MD to GitHub Pages with a landing page

### 3. GitHub Pages

- **Enabled** at https://btakita.github.io/resume/
- Landing page with **Download PDF** and **View Markdown** buttons
- Auto-deploys on every push via the Action

**Pushed**: `0e92c7f` — Makefile + workflow + resume.css + updated resume (Presence AI dates)

The Action should be running now. Check status at: https://github.com/btakita/resume/actions

## User

Add a README explaining how this the workflow works, and how to use it.

Also link to the pdf.

Create a gh page with the pdf

## Assistant

Done. Created `README.md` and pushed to `btakita/resume`.

**README** includes:
- **Download PDF** link pointing to https://btakita.github.io/resume/BrianTakita.pdf
- Local build instructions (`make pdf` / `make clean`)
- GitHub Action workflow explanation (triggers, steps)
- GitHub Pages section linking to https://btakita.github.io/resume/

**GitHub Pages** was already enabled in the previous step — it's live at https://btakita.github.io/resume/ with a landing page that has PDF download and Markdown view buttons.

The README push will also trigger the Action again (since it's a push to main), but it won't rebuild the PDF since `README.md` isn't in the trigger paths.

## User

For Open Source — Creator / Solo Developer, I have been creating open source software since 2002. So let's reflect that in the resume. Should I have other projects in there such as:

- RSpec Ruby BDD framework: core contributor
- SvelteJS: Contributor
- RMemo/RelementJS: The smallest & most composable reactive Isomorphic Component Library in Javascript
- rappstack: Full Stack Framework for building web applications & extenable app modules using BunJS & Elysia
- anime-face-detector: Modernize fork to use uv/ruff/ty and convert from cv2 to pytorch

## Assistant

**Date range**: Agreed — changing from "2024 – Present" to "2002 – Present". Over two decades of open source work.

**Which projects to include**: Be selective. The resume already lists Erector/JS Test Server/Unison under Pivotal Labs. The Open Source section should highlight projects where you were creator or core contributor.

### Recommended additions (strong signal)

| Project | Why | Narrative fit |
|---|---|---|
| **RSpec** — Core contributor | High-profile, widely known BDD framework. Ties to "automated verification" theme. | Strong |
| **SvelteJS** — Contributor | Framework-level engagement with modern frontend ecosystem. | Good |
| **RMemo/RelementJS** — Creator | "Smallest & most composable" shows minimalism/first-principles. Isomorphic = full stack. | Good |

### Optional (depends on space)

| Project | Consideration |
|---|---|
| **rappstack** | Full stack framework — shows breadth, but overlaps with freelance experience. Include if you want to emphasize BunJS/Elysia. |
| **anime-face-detector** | Modernization fork — niche. Less relevant unless targeting CV/ML roles. The uv/ruff/ty modernization shows you stay current with Python tooling. |

### Proposed structure

Keep corky and agent-doc as featured projects (full bullet descriptions), add others as a concise list below:

```markdown
### Open Source — Creator / Solo Developer
**2002 – Present**

[corky and agent-doc sections stay as-is]

**Other contributions:**
- **RSpec** — Core contributor to the Ruby BDD testing framework
- **SvelteJS** — Contributor
- **RMemo/RelementJS** — Creator. Smallest & most composable reactive isomorphic component library in JavaScript
- **rappstack** — Creator. Full-stack framework using BunJS & Elysia for extensible web app modules
```

This keeps the section focused — featured projects get detailed bullets, others get one-liners showing breadth.

Want me to apply this, or adjust which projects make the cut?

## User

Expand BDD to be Behavior Driven Development (BDD) — it's a common term.

Add:
- Access Query Analyzer: .NET Winforms query analyzer for MS Access

## Assistant

Applied all changes to `resume/BrianTakita.md` and regenerated PDF:

1. **Date range**: "2024 – Present" → "2002 – Present"
2. **BDD expanded**: "Ruby Behavior Driven Development (BDD) testing framework"
3. **Access Query Analyzer added**: ".NET WinForms query analyzer for MS Access"
4. **All other projects added**: RSpec, SvelteJS, RMemo/RelementJS, rappstack

The Open Source section now has corky + agent-doc as featured projects with full bullets, followed by "Other contributions" with five one-liners showing 20+ years of breadth.

PDF regenerated locally. Commit and push to the resume repo when ready.

## User

Add:
lazily-py: Lazy evaluation with context caching in Python
lazily-zig: Cross Platform & Thread-safe Lazy evaluation with context caching in Zig

## Assistant

Added both to `resume/BrianTakita.md` under "Other contributions":

- **lazily-py** — Creator. Lazy evaluation with context caching in Python
- **lazily-zig** — Creator. Cross-platform & thread-safe lazy evaluation with context caching in Zig

PDF regenerated. The "Other contributions" list now has 7 projects spanning Ruby, JavaScript, Python, Zig, .NET, and Rust — nice polyglot breadth.

## User

Other contributions should have each item on a separate line.

## Assistant

Done. Changed "Other contributions" from a tight bulleted list to individual paragraphs — each project is now its own block with a blank line between items:

```
**RSpec** — Core contributor to the Ruby Behavior Driven Development (BDD) testing framework

**SvelteJS** — Contributor

**RMemo/RelementJS** — Creator. Smallest & most composable reactive isomorphic component library in JavaScript
...
```

PDF regenerated. Each project now has clear visual separation in the rendered output.

## User

Give the Other contributions section some distinction...perhaps with itallics or no bold copy to distinguish from other jobs?

## Assistant

Switched the entire "Other contributions" section from **bold** to *italic* — both the heading and project names:

- Featured projects (corky, agent-doc): **bold** headings with full bullet descriptions — same visual weight as job entries
- Other contributions: *italic* names with one-liner descriptions — visually lighter, clearly secondary

This creates a natural hierarchy: your eyes are drawn to the featured projects first, then the contributions list fills in the breadth.

PDF regenerated.

## User

Add Python, Typescript, & Zig to my Top Skills. I have mostly vibe-coded Rust but I think it's a great fit with Claude. 
