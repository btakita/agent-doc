# Resume Session

## Status

**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)

## Completed

- Markdown resume created from LinkedIn PDF export + new content
- Resume repo: public at `btakita/resume`, submodule of agent-loop
- PDF pipeline: Makefile (`make pdf`) → pandoc + weasyprint with `resume.css`
- GitHub Action: triggers on .md/.css/Makefile changes, builds PDF, commits, deploys to Pages
- GitHub Pages: landing page at btakita.github.io/resume with PDF download
- README: workflow docs, build instructions, PDF link

### Content Changes

- **Presence AI**: dates updated to April 2024 – February 2026
- **Summary**: "Most recently built the full real-time AI avatar pipeline at Presence AI. Now focused on spec-driven agent tooling and collaborative intelligence workflows."
- **Tagline**: "Building emergent software systems through agent loops — spec-driven development, automated verification, and collaborative intelligence tooling where agents and humans work in parallel with full context. 20+ years full stack experience."
- **Top Skills**: Spec-Driven Development, Agents, Rust, Python, TypeScript, Zig
- **Open Source section**: 2002 – Present
  - **corky** — Multi-Source Correspondence Toolkit for Agent-Augmented Collaboration (Rust)
  - **agent-doc** — Document-as-Interface for Interactive Sessions with AI Agents (Rust)
  - Other contributions (italic, visually lighter): RSpec, SvelteJS, RMemo/RelementJS, rappstack, Access Query Analyzer, lazily-py, lazily-zig
- **Proofread**: 13 fixes (grammar, tense, parallelism, style)
- **SLOC metric removed** from corky heading
- **`| Docs` link removed** from corky GitHub line

### LinkedIn Drafts

**About section** (copy-paste ready):
Over 20 years of software development experience developing full-stack applications. Most recently built the full real-time AI avatar pipeline at Presence AI — facial landmark detection, LatentSync lip-sync inference, and frame sequencing on H100 GPUs. Now building spec-driven agent tooling and collaborative intelligence workflows in Rust.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Currently dog-fooding two open-source tools: corky (git-native correspondence toolkit) and agent-doc (persistent AI document sessions) — both built through the same spec-driven agent methodology they enable.

**LinkedIn post** (copy-paste ready):
> **Building my own agent tooling — and eating my own dog food**
>
> I've been building two open-source Rust tools that address gaps I kept hitting in my own agentic development workflow:
>
> **corky** — a git-native correspondence toolkit that syncs topics, contacts, and threads from email (IMAP), Telegram, and Slack into scoped markdown mailboxes. Each collaborator gets only the threads and AI context relevant to them, with bidirectional sync via 3-way merge. I use it daily for my personal and professional correspondence.
>
> **agent-doc** — a document-as-interface for persistent AI conversations. Edit a markdown file offline, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back inline. Git tracks every exchange. I use it as my primary planning and iteration tool — including for writing this post.
>
> Corky started as a Python prototype. I ported it to Rust in under a day using spec-driven development: write a detailed spec first, let the agent implement against it, audit the output against the spec. The specs become living documents that evolve with the code.
>
> With agent-doc, I run multiple agent sessions in parallel across different task documents — each one a coherent, persistent conversation. When a session gets long, I compact it: reorganize, delete noise, keep the signal. The agent sees the cleaned-up document as ground truth on the next run.
>
> 100% dog-fooding. 100% open source. 100% spec-driven development.
>
> corky: https://github.com/btakita/corky
> agent-doc: https://github.com/btakita/agent-doc
>
> The biggest lesson: when you use your own tools every day, the feedback loop is instant. Bugs become personal. Missing features become obvious. The quality bar rises because you're the first user who's annoyed by every rough edge.
>
> #OpenSource #Rust #AITooling #AgenticDevelopment #DeveloperTools

### Social Media Posting (corky)

Implemented `corky social` module — commit `772c71e`, pushed to main.

**What was built:**
- 8 source files in `corky/src/social/` — platform enum, profiles.toml registry, YAML frontmatter drafts, URN-keyed token store, OAuth2 auth flow, LinkedIn API, publish orchestration, CLI dispatch
- 54 edge-case tests across 5 test files (P1-P9, D1-D9, T1-T9, A1-A5, PB1-PB10)
- SPECS.md §12 with full edge case table
- `make check` clean (clippy + 279 tests)

**CLI commands:**
```
corky social auth <platform> [--profile NAME]
corky social draft <platform> [BODY] [--author X] [--visibility public] [--tags X,Y]
corky social publish <file>
corky social check
corky social list [--status X]
corky social rename-author <old> <new>
```

**Key design decisions (from iterative discussion):**
- No API posting penalty on LinkedIn — safe to use
- Token-based permissions: whoever has the OAuth token can publish
- `profiles.toml` (git-tracked) is the unified identity registry — maps profile names to platform URNs
- Token store (`~/.config/corky/tokens.json`) keyed by URN, not by handle — works across mailboxes
- Profile names are mailbox-local aliases; URN is the stable identity
- Conflict detection: validation at auth time + `corky social check` + git merge on `profiles.toml`
- Late-joining collaborators get clear error messages guiding them to pick a unique profile name

**Next steps to use:**
1. Create LinkedIn app at [linkedin.com/developers](https://www.linkedin.com/developers/)
2. Enable "Share on LinkedIn" + "Sign In with LinkedIn using OpenID Connect"
3. Add `http://127.0.0.1:8484/callback` as redirect URI
4. Set credentials in `.corky.toml` under `[social.linkedin]` or via env vars

## Pending

*(none)*

## User

Let's use the new draft post system. In mail, under me "brian", add the draft.

It is now strange that the directory is called mail. I wonder if corky is doing too much now.

Or is there a different abstraction to hold this communication with other email address & social media.

Is mailbox still a good metaphor for this scope?

## Assistant

### Draft Created

Created two files in `mail/`:

- **`profiles.toml`** — registered `btakita` with LinkedIn handle `briantakita` (URN will be set after `corky social auth linkedin --profile btakita`)
- **`social/20260224-agent-tooling-linkedin.md`** — the LinkedIn post with YAML frontmatter, status: `draft`

To publish later: change `status: draft` → `status: ready`, then `corky social publish mail/social/20260224-agent-tooling-linkedin.md`.

### Is "mail" still the right metaphor?

Good instinct to question this. The tension is real: `mail/` originally meant "email data directory." Now it holds social posts, profiles, contacts that aren't email-specific. But **corky isn't doing too much — the abstraction just needs a name upgrade.**

What corky actually manages is **correspondence** — structured communication across channels. Email, Slack imports, Telegram imports, and now social posts are all outbound/inbound correspondence. The tool name "corky" (correspondence kit) already captures this.

**The directory name `mail/` is the real problem, not the scope.** Options:

| Name | Pros | Cons |
|---|---|---|
| `mail/` (keep) | Zero migration, familiar | Misleading now that social posts live here |
| `corky/` | Matches tool name | Collides with source repo name in dev workflow |
| `correspondence/` | Accurate, matches "corky" etymology | Long |
| `comms/` | Short, covers email + social + messaging | Informal |
| `data/` | Generic, never wrong | Too vague |

**"Mailbox" as metaphor** still works at the sub-level — a mailbox is a scoped view of correspondence for a collaborator. Alice's mailbox contains the threads, drafts, and social posts relevant to her. That's fine.

The issue is really just the top-level `mail/` directory name. A rename to `comms/` or keeping `mail/` with the understanding that it's shorthand would both work. This is mostly cosmetic — the resolution chain in `resolve.rs` already abstracts the path, so renaming later is a one-line change + migration script.

**Recommendation:** Keep `mail/` for now. It's entrenched in docs, gitignores, and muscle memory. Rename if/when Bluesky or Mastodon are added and it feels genuinely confusing. The abstraction (correspondence across channels) is sound — only the directory name lags behind.

## User

I like comms as a term...but havn't seen it used other than by military.

Would mail be more confusing than comms for multiple social network support?
I can see correspondence hence comms as being more correctly scoped as a broader category than mail.

So it would be logical to use comms...but I'm not aware of precedence with other apps.
