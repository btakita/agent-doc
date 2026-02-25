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

**LinkedIn post draft**: `mail/social/20260224-agent-tooling-linkedin.md` (status: draft)
To publish: change `status: draft` → `status: ready`, then `corky social publish mail/social/20260224-agent-tooling-linkedin.md`.

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

**Next steps to use:**
1. Create LinkedIn app at [linkedin.com/developers](https://www.linkedin.com/developers/)
2. Enable "Share on LinkedIn" + "Sign In with LinkedIn using OpenID Connect"
3. Add `http://127.0.0.1:8484/callback` as redirect URI
4. Set credentials in `.corky.toml` under `[social.linkedin]` or via env vars

### Directory Naming Convention

Explored renaming `mail/` → `mailbox/`, `comms/`, `correspondence/`. Conclusion: **keep `mail/`**.

- `mailbox/` creates `mailbox/mailboxes/` path stutter (literal, not conceptual)
- `comms/` — militaristic tone clashes with "corky"
- `correspondence/` — too long (15 chars)
- Flatten approaches rejected — namespace collisions between mailbox names and system dirs
- "Mailbox" remains the correct internal abstraction (actor model precedent, Hewitt 1973)
- `mailboxes/` subdir is load-bearing architecture — namespace boundary preventing collisions

Documented in **SPECS.md §2.5 Directory Naming Convention**.

## Pending

*(none)*

## User

Is it possible to schedule the linked in post using their api?

## Assistant

**No** — the LinkedIn Posts API does not support scheduled publishing.

The `POST /rest/posts` endpoint only accepts two `lifecycleState` values:
- `PUBLISHED` — publishes immediately
- `DRAFT` — saves as draft (not visible to anyone)

There is no `scheduledPublishAt` or equivalent field. LinkedIn's native web UI has a scheduling feature (clock icon when composing, up to 3 months ahead), but that capability is not exposed through the API.

**Options for corky:**

1. **Client-side scheduling** — corky already has `scheduled_at` in the draft frontmatter schema (SPECS.md §12.3). A cron job or systemd timer could check for ready drafts where `scheduled_at` is in the past and call `corky social publish` on them. Simple, reliable.

2. **Use DRAFT state + timed publish** — create the post as a LinkedIn draft via API, then publish it at the scheduled time. But the API doesn't support publishing an existing draft either — you'd have to create a new `PUBLISHED` post at that time anyway.

3. **Manual** — set `status: ready` in the draft, then run `corky social publish` when you want it to go live.

Recommendation: option 1 is the cleanest. A one-liner cron entry like:
```
*/5 * * * * corky social publish-scheduled
```
...that checks `scheduled_at` on ready drafts and publishes any that are due. This would be a small addition to the existing `corky social` module.

Want me to implement `corky social publish-scheduled`?

## User

I would also want to support email scheduling as well...What would the refactored plan look like?
What would be the common module used by the email and social media modules for example?

## Assistant

### Current State

The two draft systems have **completely different formats**:

| | Email drafts (`draft/`) | Social drafts (`social/draft.rs`) |
|---|---|---|
| Format | Markdown metadata (`**To**: ...`) | YAML frontmatter (`---`) |
| Status | `**Status**: draft` | `status: draft` |
| `scheduled_at` | **Does not exist yet** | Field exists, unused |
| Publish action | SMTP send via `lettre` | HTTP POST to platform API |
| Location | `drafts/` or `mailboxes/*/drafts/` | `social/` |

They share no types and no traits. Good — that means we can add a thin scheduling layer without refactoring either system.

### Proposed Architecture

**New module: `corky/src/schedule.rs`** (single file, not a directory)

```rust
/// A pending scheduled item discovered by scanning the filesystem.
pub struct ScheduledItem {
    pub path: PathBuf,
    pub kind: ScheduledKind,
    pub scheduled_at: DateTime<Utc>,
}

pub enum ScheduledKind {
    Social,
    Email,
}
```

The scheduler does **not** define a trait. Instead, it:

1. **Scans** `social/` for drafts where `status: ready` and `scheduled_at <= now`
2. **Scans** `drafts/` (and `mailboxes/*/drafts/`) for email drafts where `Status: scheduled` and `Scheduled-At <= now`
3. **Dispatches** to existing publish functions:
   - `Social` → `social::publish::run_publish(path)`
   - `Email` → `draft::send::run_send(path)`
4. **Reports** results (published, skipped, errored)

### Why No Trait

A `Schedulable` trait would force coupling between `social::SocialDraft` and `draft::EmailDraft` — they'd both need to implement a shared interface. But their publish paths are fundamentally different (OAuth + REST vs SMTP). The scheduler doesn't need polymorphism; it just needs to know:
- Where to look (directories)
- What to check (`scheduled_at` field + status)
- What to call (existing publish functions)

A match on `ScheduledKind` is simpler and more explicit than dynamic dispatch.

### Changes Required

| File | Change |
|---|---|
| `corky/src/schedule.rs` | **New.** Scan logic, dispatch, CLI handler |
| `corky/src/draft/mod.rs` | Add `scheduled_at` field to email draft parsing. Add `Status: scheduled` state |
| `corky/src/social/publish.rs` | Extract path-based publish (currently takes `&str` file arg) — already suitable |
| `corky/src/cli.rs` | Add `Schedule(ScheduleCommands)` variant |
| `corky/src/main.rs` | Add match arm |
| `corky/src/lib.rs` | Add `pub mod schedule;` |
| `corky/SPECS.md` | Add §13 Scheduling |

### Email Draft Format Change

Current:
```markdown
**Status**: draft
```

Add:
```markdown
**Status**: scheduled
**Scheduled-At**: 2026-02-25T09:00:00Z
```

New status flow: `draft` → `scheduled` → `sent` (parallel to social's `draft` → `ready` → `published`)

### CLI

```
corky schedule run                    # Check all scheduled items, publish any due
corky schedule list                   # Show pending scheduled items with times
corky schedule run --dry-run          # Show what would be published without doing it
```

One cron entry covers everything:
```
*/5 * * * * corky schedule run
```

### Edge Cases

| # | Edge Case | Expected Behavior |
|---|---|---|
| S1 | No scheduled items | Exit 0, no output |
| S2 | Item due, publish succeeds | Publish, update status, print summary |
| S3 | Item due, publish fails (network) | Error logged, item stays scheduled, exit 1 |
| S4 | Item in future | Skipped |
| S5 | `scheduled_at` missing on ready/scheduled item | Skipped (not a scheduled item) |
| S6 | Multiple items due | Process all, report per-item results |
| S7 | Email draft missing account config | Error with guidance |
| S8 | Social draft missing token | Error with `corky social auth` guidance |
| S9 | `--dry-run` flag | Print what would happen, don't publish |
| S10 | Clock skew (scheduled_at slightly in future) | 30-second grace window |

### Summary

The common module is **`schedule.rs`** — a thin orchestrator that scans both draft systems and dispatches to their existing publish paths. No shared trait, no refactoring of existing types. The only change to existing modules is adding `scheduled_at` to email drafts.

Want me to implement this?

## User

implement scheduling
