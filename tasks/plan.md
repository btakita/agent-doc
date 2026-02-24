---
session: cf853a21-3aaf-4775-9cc8-0e789109f9dd
agent: null
model: null
branch: null
---

# Plan: agent-doc & agent-loop

## Status

- **agent-doc**: v0.1.4 — crates.io + PyPI. 90 tests passing.
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `agent-doc upgrade` with GitHub Releases download
- **corky**: v0.9.4 — crates.io + PyPI. 216 tests passing.
  - Contact sync v2: conversation-aware eligibility, explicit sharing, alias matching, 3-way merge via `.sync-state.json`
  - Upgrade check + self-update: crates.io query with 24h cache, startup warning, `corky upgrade` with GitHub Releases download

## Outstanding

### agent-doc

- [ ] Codex backend (`src/agent/codex.rs`) — plan at `tasks/integrations/codex.md`

### corky — Telegram

- [ ] Phase 2: Live sync via `grammers-client` — plan at `corky/tasks/integrations/telegram.md`
- [ ] Phase 3: Watch mode (real-time push)
- [ ] Phase 4: Send via `draft push`

### corky — Slack

- [ ] Phase 2: API sync via `slack-morphism` — plan at `corky/tasks/integrations/slack.md`
- [ ] Phase 3: Socket Mode (real-time push)

### Future

- [ ] Streaming responses (agent-doc)
- [ ] Document-only permission flow — no terminal (agent-doc)
- [ ] Desktop notifications on completion (agent-doc)
- [ ] Multi-agent documents — sections routed to different agents (agent-doc)
- [ ] Email transport for async human-agent loops — bridge with corky (agent-doc)

## Reference

- **Decisions & architecture:** `agent-doc/SPECS.md`
- **Module layout & conventions:** `agent-doc/CLAUDE.md`
- **Workspace structure:** `SPECS.md`
- **Workflow orchestration:** `CLAUDE.md`

## User

### Personal Data
Should I comingle by person communication with development in this repo?
I'm mainly concerned about the artifacts of my development. Like I don't want my personal communication to be in the commit history. But I'm using commits to iterate on plan.md.

Can we explore other options for separating personal communication from development artifacts?
Would a sidecar git repo be a good solution?

### Topics
I want to have topics that can be shared with collaborators. Communication can be assigned to topics as well via rules. A topic is a way of grouping. Other ways of groupings are labels and contacts.

A topic will have it's own AGENTS.md->CLAUDE.md/CLAUDE.local.md filespace. Other context files may be in there as well.

There needs to be a way to group topics with threads. Perhaps the frontmatter of the thread can contain the topic?

If a topic is missing, then the topic can be auto-created with suggested after a pass through the emails.

Each thread implicitly has it's own topic as well. So there should be an easy way to add context to the thread. Perhaps a context file can be created for thread as needed? Two options I see:

1. The filename can have a suffix like `.context.md`
2. A subdirectory with the <thread basename>/AGENTS.md file can be created.

I suppose the latter is more flexible, but not as simple & convenient. How about we start off with `.context.md` for now 

This also means collaborator threads, including topics, need to be synced across all collaborators.

### Context Routing with collaborators
I need a way for context to be routed bi-directionally with collaborators. The existing solution on the contact of the 3 way merge of AGENTS.md is sufficient for topics. I mainly need this process to be a central part of the collaboration workflow. So topics, contacts, and threads would all share this.

## Assistant

### Personal Data — Separation Options

Your concern is valid: `plan.md` commits are useful development artifacts, but personal communication shouldn't leak into git history. Currently `mail/` is gitignored (good), but session docs in `tasks/` do get committed.

**Four options:**

1. **Nested git repo inside `tasks/`** — Initialize `tasks/.git`, gitignore `tasks/` from parent. agent-doc commits stay local to the nested repo.
   - Pro: Clean separation. Main repo stays code/specs only. Full session history preserved.
   - Con: Two repos to manage locally.

2. **Sidecar git repo** — e.g., `~/.agent-doc-sessions/agent-loop/` with symlinks or config pointing agent-doc there.
   - Pro: Complete separation. Reusable across projects.
   - Con: Cross-referencing between session docs and code becomes harder.

3. **Branch-based isolation** — Session docs on a `sessions/plan` branch that never merges to main. agent-doc already has `branch:` frontmatter.
   - Pro: One repo, clean main. History exists but is separate.
   - Con: Branch switching friction. Stale branches.

4. **Ephemeral commits with squash** — Commit for diff gutters, squash before pushing.
   - Pro: Simplest change. Iteration history stays local.
   - Con: Doesn't help if you push to a shared remote.

**Recommendation:** Option 1 — nested git repo inside `tasks/`. Initialize once, main repo never sees it. agent-doc already does the commits; you get full session history without polluting the main commit log. If you later want to share session docs, you can push the nested repo independently.

### Topics — Design Sketch

Starting with `.context.md` is the right call. Here's how topics layer into corky's existing architecture:

**Thread → Topic association:**
Add `**Topic**: {topic-name}` to conversation frontmatter (alongside Labels, Accounts, Thread ID). Follows corky's existing metadata pattern — regex-parseable, accumulative.

**Filesystem layout:**
```
mail/
  topics/
    {topic-slug}/
      CLAUDE.md          # shared context (3-way merged with collaborators)
      CLAUDE.local.md    # private context (never synced)
  conversations/
    {thread-slug}.md           # has **Topic**: foo in metadata
    {thread-slug}.context.md   # per-thread context (simple start)
```

**Auto-creation:** During sync, if a thread's labels/sender match a topic rule that doesn't exist yet, create `topics/{slug}/CLAUDE.md` with a stub and flag as "suggested." A `corky topics suggest` command could do a batch pass and propose assignments.

**Topic assignment rules** — extend `.corky.toml`:
```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]

[topics.hiring]
labels = ["recruiting"]
```

**Graduation path:** `.context.md` → `{thread-slug}/CLAUDE.md` subdirectory when thread context grows. No code changes needed — just convention.

### Context Routing with Collaborators

The existing 3-way merge (FNV-1a content hash in `.sync-state.json`) is the right primitive. Topics extend it naturally:

**What syncs per collaborator:**
- `topics/{slug}/CLAUDE.md` — same merge strategy as contact CLAUDE.md files
- `{thread-slug}.context.md` — syncs alongside its conversation (same eligibility: if thread syncs, context syncs)
- `**Topic**` metadata on threads — accumulates like labels

**State extension:**
```json
{
  "contacts": { ... },
  "topics": {
    "project-alpha": {
      "mailboxes": {
        "alice": "fnv1a_hash_hex"
      }
    }
  }
}
```

Mirrors the `contacts` structure exactly. The unifying insight: topics, contacts, and threads all share the same sync primitive — content-hash-based 3-way merge with eligibility rules. The only difference is what triggers eligibility:
- **Contacts**: conversation sender matching
- **Topics**: label/contact rule matching
- **Threads**: implicit (if the conversation syncs, its context syncs)

This means the core merge machinery in `contact/sync.rs` can be generalized into a shared `context_sync` module that all three consume.

## User

The topics should be configurable in .corky.toml. Including sharing topics with collaborators.

## Assistant

### Topics in `.corky.toml` — Full Config Design

Agreed — topics should follow the exact same `shared_with` pattern that contacts already use. Here's the complete config spec:

**`.corky.toml` — topic config:**

```toml
[topics.project-alpha]
labels = ["project-alpha", "alpha-dev"]
contacts = ["alice", "bob"]
shared_with = ["alice", "bob"]   # sync topic context to these mailboxes

[topics.hiring]
labels = ["recruiting"]
contacts = ["alice"]
shared_with = ["alice"]          # only alice gets hiring context
# bob does NOT get this topic's context, even if he has matching threads

[topics.personal-dev]
labels = ["dev-journal"]
# no shared_with — topic stays root-only, never synced to collaborators
```

**The `shared_with` field** works identically to `[contacts.*.shared_with]`:

- `shared_with = ["alice"]` → `topics/hiring/CLAUDE.md` gets 3-way merged into `mailboxes/alice/topics/hiring/CLAUDE.md`
- Without `shared_with`, the topic context stays root-only — threads may still be tagged with the topic in collaborator mailboxes, but they won't receive the shared `CLAUDE.md`
- Bidirectional: collaborator edits to topic context merge back to root on sync

**Eligibility rules — parallel to contacts:**

| | Contact | Topic |
|---|---|---|
| **Auto-match** | sender slugifies to contact name | thread label or sender in `contacts` list |
| **Explicit sharing** | `shared_with` includes mailbox | `shared_with` includes mailbox |
| **Root → Mailbox** | only if eligible | only if eligible |
| **Mailbox → Root** | always | always |

**Rust struct** (mirrors `Contact`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopicConfig {
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub contacts: Vec<String>,
    #[serde(default)]
    pub shared_with: Vec<String>,
}
```

Added to `CorkyConfig`:

```rust
pub struct CorkyConfig {
    // ... existing fields ...
    pub topics: HashMap<String, TopicConfig>,
}
```

**`.sync-state.json`** — topics slot mirrors contacts:

```json
{
  "contacts": { "alice": { "mailboxes": { "work": "fnv1a_hex" } } },
  "topics": { "project-alpha": { "mailboxes": { "alice": "fnv1a_hex", "bob": "fnv1a_hex" } } }
}
```

**Implementation order:**

1. Add `TopicConfig` struct + wire into `CorkyConfig` deserialization
2. Extract core 3-way merge from `contact/sync.rs` → shared `context_sync` module
3. Topic sync: eligibility check (label/contact match OR `shared_with`), then delegate to shared merge
4. Thread tagging: during sync, auto-add `**Topic**: {name}` to matching conversations
5. `corky topics suggest` — scan untagged threads, propose topic assignments
6. `corky topics list` — show topics with their sync status per mailbox

The beauty is that steps 2-3 are mostly refactoring existing contact sync code. The new logic is just the eligibility check in step 3.

## User

Create an Makefile automation to run make release on both agent-doc and corky. I need a way to update my local copy of the repo with the latest changes.

Publish any pending releases.

