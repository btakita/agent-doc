---
session: db08a891-c339-4a49-9f49-2d8a1f30ed10
resume: null
agent: null
model: null
branch: null
---
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

## Assistant

### "Comms" in tech — precedent check

**You're right that it's uncommon as a directory/app name.** Here's the landscape:

**Apps using "comms" or similar:**
- Google, Slack, Discord, Teams — all use "messaging" or "channels," never "comms"
- **Intercom** describes itself as a "customer communications platform" — uses "communications" in marketing, not directory structure
- **Twilio** brands as "Communications API"
- In gamedev/enterprise: "comms" appears in internal tooling but not consumer products

**The military association is real** — "comms" feels operational, terse, tactical. But for a CLI tool used by developers, that register actually works. Developers are comfortable with terse jargon (`src/`, `bin/`, `lib/`, `pkg/`).

**Closer precedents for multi-channel data directories:**
- `~/.local/share/` — XDG generic
- `~/Messages/` — macOS iMessage store
- Git uses `.git/` — tool name as directory

**The real question: who sees this directory name?**

If it's primarily you (developer workflow, symlink), the name matters less. If it's new users running `corky init`, the name is their first impression of scope. The default fallback `~/Documents/mail` becoming `~/Documents/comms` would immediately signal "this handles more than email." That's a meaningful UX signal for new users.

**My updated take:** `comms/` is fine for a CLI tool. The military connotation is a feature, not a bug — it signals crisp, operational communication management. No consumer app uses it, which means no confused expectations. And it's only 5 characters.

But this is a **non-blocking decision**. `resolve.rs` abstracts the path, so renaming later is a one-line change + migration script. Don't let it slow down the LinkedIn app setup and first post.

**If you want to rename now:** it's a small change — update `resolve.rs` fallback, `.gitignore`, symlink, docs. Maybe 30 minutes of work including tests. If you want to defer, `mail/` keeps working.

## User

I'm still not sure what name to pick yet.

corky and correspondence makes sense...but correspondence is long.

corky and comms seems like the name doesn't match as much...this is an intuitive gut feeling of how the names feel together...like corky is kindof a silly friendly name but comms is more militaristic. Though I'm sure there are soldiers named Corky...but it's not exactly what I think of when I say Corky.

corky and mail is less of a match than corky and correspondence...but not much less of a match.
Also languages that use the Actor Pattern use the term mailbox. Not sure if this is an Actor pattern...but it does seem like it could be. Could you assess the Actor Pattern analogy with the Corky workflow?

## Assistant

### Actor Pattern — Corky Analogy

Good intuition. Let's map it formally.

**Actor Pattern primitives (Hewitt 1973, Erlang/OTP, Akka):**

| Concept | Actor Pattern | Corky |
|---|---|---|
| **Actor** | Independent entity with private state | Collaborator (Brian, Alice) with scoped view |
| **Mailbox** | Per-actor message queue | Per-collaborator collection of threads, drafts, posts |
| **Message** | Immutable data sent between actors | Email, Slack thread, Telegram message, social post |
| **Isolation** | No shared memory; actors only see their own state | Each collaborator sees only relevant threads (scoped) |
| **Async** | Fire-and-forget message passing | Email/social/messaging is inherently async |

**Where the analogy holds well:**

- **Mailbox as interface.** In Actor systems, the mailbox IS how you interact with an actor — you send messages to it, the actor processes them on its own schedule. In Corky, the mailbox IS the collaborator's interface — all their correspondence arrives there, they process it on their own schedule. This is a real structural parallel.
- **Scoped isolation.** Actors have private state that can't be peeked at from outside. Corky mailboxes are scoped — Alice doesn't see Brian's unrelated threads. The scoping mechanism differs (language-level vs topic filtering), but the principle is the same.
- **Message-oriented.** Everything in Corky is a discrete message: an email, a thread reply, a social post. Actors think the same way.

**Where it diverges:**

- **No computation in the mailbox.** Actor mailboxes process messages and produce side effects. Corky mailboxes are passive stores — the human (or AI agent) does the processing externally. Corky is closer to a message broker than an actor runtime.
- **Shared state via git.** Pure actors never share mutable state. Corky uses git as shared state with 3-way merge for conflict resolution. This is more like a CRDT or distributed database than actor isolation.
- **No supervision tree.** Actor systems have hierarchical supervision (restart strategies, failure cascading). Corky collaborators are flat peers.

**Verdict: the analogy is partial but the naming precedent is strong.**

Corky's "mailbox" maps cleanly to the Actor Pattern's mailbox in the most important way: **it's the per-entity collection of inbound correspondence.** The Actor Pattern gives "mailbox" a formal CS meaning that's broader than "place where postal mail arrives." In actor systems, a mailbox receives any kind of message — HTTP requests, database events, timer ticks, signals. Social posts fit that abstraction naturally.

**What this means for the naming decision:**

- "Mailbox" has formal CS precedent from 1973 — it's not just postal mail
- If you frame `mail/` as "actor mailbox" rather than "postal mail," the scope already covers social posts, messaging imports, and any future channel
- The tone matches too: "corky mailbox" feels right — friendly, approachable, technically grounded

**Revised options with the actor lens:**

| Name | Vibe with "corky" | Scope | Precedent |
|---|---|---|---|
| `mail/` | Good — friendly, familiar | Narrow connotation (postal) | Universal |
| `mailbox/` | Good — same register | Broad (actor model) | CS formal (1973) |
| `comms/` | Mismatch — militaristic | Broad | Rare in tech |
| `correspondence/` | Good — matches etymology | Exact | Formal English |

`mailbox/` might be the sweet spot: friendly enough for corky, broad enough for social posts, and backed by 50 years of CS precedent. Only 7 characters. And it's already the term you use internally for the scoped-per-collaborator concept — promoting it to the directory name just makes the abstraction explicit.

## User

I like mailbox...A hesitation I have is the mailbox/mailboxes/ directory...
This demonstrates that mailbox is recursive...which is actually good. So maybe that's ok.

It is strange...but it does demonstrate the recursive directory structure.

## Assistant

### The `mailbox/mailboxes/` recursion

You're right that it looks recursive — and you're right that it IS recursive, by design. The top-level `mailbox/` is the corky data root. Each collaborator's subdirectory within it is also conceptually a "mailbox." So `mailbox/brian/` means "Brian's mailbox inside the mailbox store." That's not a bug — it's the abstraction being honest about itself.

**Precedent for self-referential nesting:**

- `git/` repo containing `.git/` — "git inside git"
- `node_modules/*/node_modules/` — modules containing modules
- Unix `/home/` — the home of all homes
- Erlang/OTP: a process (actor) can spawn child processes, each with their own mailbox. The system IS a mailbox containing mailboxes

**The path in practice:**

```
mailbox/                    <- corky data root (the "system mailbox")
  brian/                    <- Brian's scoped mailbox
    conversations/
    drafts/
    contacts/
  alice/                    <- Alice's scoped mailbox
    conversations/
    drafts/
  profiles.toml
  social/
    20260224-agent-tooling-linkedin.md
```

Users would never type `mailbox/mailboxes/` — there's no literal `mailboxes/` subdirectory. The collaborator directories (brian/, alice/) ARE the mailboxes directly. So the recursion is conceptual, not a literal path stutter.

**The strangeness is actually clarity.** When someone sees `mailbox/brian/conversations/`, they immediately understand: "this is Brian's mailbox, and it contains his conversations." If the top-level were `data/` or `store/`, that self-documenting quality disappears.

If the conceptual recursion still feels wrong, the only alternative that avoids it would be calling the top-level something different from what it contains — like `post_office/` containing mailboxes. But that's worse: more characters, more metaphor to explain, and "post office" has even stronger postal connotations.

**Bottom line:** `mailbox/` works. The recursion is the point — it's mailboxes all the way down.

## User

There is a mailboxes/ directory though...I think this is how corky works. Please verify

See /home/brian/work/btakita/agent-loop/mail/mailboxes/lucas

## Assistant

### Correction — `mailboxes/` IS a literal directory

You're right, I was wrong. Verified the actual layout:

```
mail/                           <- data root (owner's mailbox + shared data)
  conversations/                <- owner's conversations
  drafts/                       <- owner's drafts
  contacts/                     <- shared contacts
  mailboxes/                    <- collaborator mailboxes
    lucas/                      <- Lucas's scoped mailbox
      conversations/
      drafts/
      contacts/
  social/                       <- social drafts
  profiles.toml
```

The data root doubles as the owner's mailbox. `mailboxes/` is a real subdirectory containing collaborator scoped views. So renaming to `mailbox/` would produce:

```
mailbox/mailboxes/lucas/conversations/
```

That's a literal path stutter — not just conceptual. Valid concern.

**Options to resolve:**

| # | Approach | Path example | Migration |
|---|---|---|---|
| 1 | **Keep `mail/`** | `mail/mailboxes/lucas/` | None |
| 2 | **`mailbox/` + flatten** | `mailbox/brian/`, `mailbox/lucas/` (no `mailboxes/` subdir) | Restructure owner data into named subdir |
| 3 | **`mailbox/` + rename inner** | `mailbox/peers/lucas/` | Rename `mailboxes/` to `peers/` |
| 4 | **`correspondence/` or `comms/`** | `correspondence/mailboxes/lucas/` | Rename top-level only |

**Option 2 (flatten) is architecturally cleanest** — every collaborator is a peer, including the owner. Brian's data moves from `mailbox/conversations/` to `mailbox/brian/conversations/`. The root only contains shared data (profiles.toml, social/). But it's a bigger migration + resolve.rs refactor.

**Option 1 (keep `mail/`) is pragmatic** — zero migration, no stutter. `mail/mailboxes/lucas/` reads naturally: "in the mail store, mailboxes, Lucas's." The actor-model "mailbox" concept still works as the internal abstraction even if the directory isn't literally called that.

**Updated recommendation:** Given the stutter problem, `mail/` is actually the strongest option after all. It avoids `mailbox/mailboxes/`, it's short, it's familiar. The postal connotation is slightly narrow, but "mail" in Unix already means "messages" broadly (think `maildir`, `sendmail`, `/var/mail`). And corky's full name is "correspondence kit" — the directory name doesn't need to carry the full semantic weight.

If the stutter bothers you enough to do option 2 (flatten), that's the right long-term architecture. But it's a real refactor, not a rename.

## User

My hesitation with flattening is that a mailbox name can conflict with a system file name...such as social.
Or whatever is added in the future. Is there a convention for these predefined names that we can use? Like a leading .social or something else?

## Assistant

### Namespace collision in the flatten approach

Good catch. With flatten, the root mixes mailbox names and system directories:

```
mailbox/
  brian/          <- mailbox
  lucas/          <- mailbox
  social/         <- system (collision if someone's named "social")
  contacts/       <- system
  profiles.toml   <- system
```

**Conventions to separate them:**

| Convention | Example | Precedent | Tradeoff |
|---|---|---|---|
| **Dotfile prefix** | `.social/`, `.profiles.toml` | Unix hidden files, `.git/` | Social drafts are user content — hiding them feels wrong |
| **Underscore prefix** | `_social/`, `_contacts/` | Jekyll `_layouts/`, Hugo | Visible but clearly "system"; less standard |
| **System subdir** | `.corky/social/`, `.corky/contacts/` | `.git/`, `.config/` | Clean separation; extra nesting |
| **Reserved name list** | Validate at `corky init` time | K8s reserved namespaces | Simple but brittle — new system dirs break existing mailboxes |

**The dotfile approach has a problem:** `social/` contains user-facing drafts (your LinkedIn post lives there). Making it `.social/` hides user content, which is counterintuitive.

**The system subdir approach is cleanest if you flatten:**

```
mailbox/
  brian/                          <- mailbox
  lucas/                          <- mailbox
  .corky/                         <- system data (no human is named ".corky")
    social/
    contacts/
    contacts.toml
    profiles.toml
    manifest.toml
```

The `.` prefix guarantees no collision with human names. All system data scoped under one dir. Mailboxes are the only visible entries at root.

**But this reveals the real tradeoff.** The current `mailboxes/` subdir approach already solves the collision problem elegantly — mailboxes are namespaced under `mailboxes/`, system dirs live at root. No `.` prefixes, no reserved lists, no extra nesting. The cost is just the `mailboxes/` directory name.

| Approach | Collision handling | Complexity |
|---|---|---|
| Current (`mail/mailboxes/lucas/`) | Built-in via namespace | Zero — already works |
| Flatten + `.corky/` | Dotfile system dir | Migration + resolve.rs refactor + docs |
| Flatten + reserved names | Validation logic | Fragile — new system dirs are breaking changes |
| Flatten + underscore | Convention | Non-standard, still needs validation |

**Updated recommendation:** Keep the current structure. The `mailboxes/` subdir is doing real architectural work — it's a namespace boundary that prevents collisions. The flatten approach requires re-solving a problem that `mailboxes/` already handles.

For the top-level name: **`mail/` is fine.** Every alternative creates a new problem (stutter, tone mismatch, collision). `mail/` has none of these issues. The naming discussion has been productive — it confirmed that the current architecture is sound, and "mailbox" remains the right internal abstraction even if the directory isn't literally called that.

## User

Can we have succinct documentation on the name convention somewhere? It's worthwhile to discuss.

## Assistant

### Naming Convention — Documentation

Here's a compact reference. Where should it live permanently?

**Options:**
- **corky/SPECS.md** — add a section on directory naming rationale (alongside existing specs)
- **corky/CLAUDE.md** — already has conventions; add naming rationale there
- **mail/README.md** — document the data directory's own naming

I'd suggest **SPECS.md** since it's the canonical reference for design decisions. Here's the block:

---

**Directory Naming Convention**

| Directory | Purpose | Name rationale |
|---|---|---|
| `mail/` | Corky data root | Short, familiar, Unix precedent (`maildir`, `/var/mail`). Avoids `mailbox/mailboxes/` stutter. |
| `mailboxes/` | Collaborator namespace | Namespace boundary — prevents collisions between mailbox names and system dirs (`social/`, `contacts/`). |
| `social/` | Social media drafts | Channel-specific content at data root level. |
| `contacts/` | Contact registry | Shared across all mailboxes. |

**Why "mailbox" internally, "mail" as directory:**
- "Mailbox" is the correct abstraction — per-entity message collection (actor model, Hewitt 1973)
- `mail/` is shorthand for the data root; it doesn't need to carry the full semantic weight
- `mailbox/` was considered but creates `mailbox/mailboxes/` path stutter
- `comms/` rejected — militaristic tone clashes with "corky"
- `correspondence/` rejected — too long (15 chars)
- Flatten approaches rejected — namespace collisions between mailbox names and system dirs

**The `mailboxes/` subdir is load-bearing architecture**, not cosmetic. It prevents a collaborator named "social" or "contacts" from colliding with system directories. Do not flatten without solving the namespace problem first.

---

Want me to add this to SPECS.md now?

## User

yes

## Assistant

### Added to SPECS.md §2.5

Added the naming convention block as **§2.5 Directory Naming Convention** in `corky/SPECS.md`, right after the Data Directory sections (§2.1–§2.4). It includes:

- Directory naming table (`mail/`, `mailboxes/`, `social/`, `contacts/`)
- "Why mailbox internally, mail as directory" rationale bullets
- The load-bearing architecture warning about `mailboxes/`

This keeps the naming rationale co-located with the layout spec it documents. The discussion that produced it (Actor Pattern analysis, stutter problem, namespace collisions) is preserved here in the session doc for context, while SPECS.md gets just the actionable reference.

## User

cleanup this doc
