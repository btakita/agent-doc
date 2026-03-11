---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_mode: template
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange -->
Converted component from `output` to `exchange`. Write prompts here — both user and agent share this surface.

commit submodule. release any pending features + bug fixes. commit & push agent-loop

**Completed** — submodules committed, pointers updated, agent-loop pushed.

| Submodule | Version | Action |
|-----------|---------|--------|
| mail | main@a153669 | Committed 39 files (conversations, contacts, drafts) + pushed |
| agent-doc | v0.14.5+2 | Pointer updated (JetBrains plugin fixes, no crate release needed) |
| corky | v0.14.0 | Pointer updated |
| tmux-router | v0.2.4 | Pointer updated |

Parent commit: `cca8d88` — includes all submodule pointers + 22 task doc updates. Pushed to `main`.

Only untracked item remaining: `src/agent-kit` (new, not yet added to parent).

audit docs

**Audit docs completed.**

| Project | Issues | Action |
|---------|--------|--------|
| agent-doc | 5 → 1 (warning only) | Updated README.md + SPEC.md for route cascade & sync_after_claim. Touched SKILL.md + CLAUDE.md (reviewed, not stale). Domain Ontology table stays in CLAUDE.md per convention. |
| corky | 3 → 0 | Updated README.md + SPEC.md — transcribe now enabled by default (CPU). Touched AGENTS.md (reviewed, not stale). |

Submodule commits pushed, parent pointers updated → `234d137`. Pushed to `main`.

commit and push corky changes. Would it be possible to update the README on the crates.io docs for the current corky release?

Corky is already committed and pushed (done in previous cycle — `7c3f4ea`).

**crates.io README update:** crates.io serves the README embedded in the published crate tarball. There's no API to update docs for an existing version. Options:

1. **Publish v0.14.1** — bump version, `cargo publish`. The only change is docs (README + SPEC reflecting transcribe-by-default). Clean and correct.
2. **Do nothing** — the crates.io README is slightly stale but functionally harmless (just says "install with `--features transcribe`" instead of "enabled by default").

Want me to publish v0.14.1 for the docs fix?

Is there any pending features or bug fixes for corky?

**Corky pending work:**

- **No open issues or PRs.** No TODO/FIXME/HACK in source.
- **One pending task** from `tasks/email.md`: btak-dev Gmail account setup (`btak.dev@gmail.com` in `mail/.corky.toml`) — discussed but never applied.
- **23 unmerged branches** — most are stale feature experiments. Notable ones:
  - `telegram-html-import` / `telegram-slack-import` — Telegram import: status?
  - `contact-sync-v2` — contact sync v2: status?
  - `upgrade-self-update` — self-update mechanism: status?
  - `draft-attachments` — draft attachments: status?
  - `contact-enrichment` — contact enrichment: status?
  - `rust-2024-edition` — edition upgrade: already done. delete

No code-level bugs or pending features blocking a release. If you want to publish v0.14.1 (docs-only), it's clean to do so. The stale branches could be pruned if those features are abandoned.

**Branch status — all merged via PRs, all deleted:**

| Branch | PR | Status |
|--------|-----|--------|
| `telegram-html-import` | #5 | Merged, deleted |
| `telegram-slack-import` | #4 | Merged, deleted |
| `contact-sync-v2` | #6 | Merged, deleted |
| `upgrade-self-update` | #8 | Merged, deleted |
| `upgrade-check` | #7 | Merged, deleted |
| `draft-attachments` | #17 | Merged, deleted |
| `contact-enrichment` | #3 | Merged, deleted |
| `rust-2024-edition` | #19 | Merged, deleted (edition=2024 on main) |
| `linkedin-images` | — | Merged, deleted |
| `social-credential-cmd` | #11 | Merged, deleted |
| `topic-sync` | #9 | Merged, deleted |
| `spec-rename` | #14 | Merged, deleted |
| `v0.11.3` | #13 | Merged, deleted |
| `transcribe-filter` | #16 | Merged, deleted |
| `label-clear` | #15 | Merged, deleted |
| `oauth-fixes` | #12 | Merged, deleted |
| `social-to-linkedin` | #18 | Merged, deleted |
| `rename-unanswered` | #2 | Merged, deleted |
| `rust-port` | #1 | Merged, deleted |

**Result:** Corky now has only `main`. 19 stale branches removed (local + remote).
<!-- /agent:exchange -->
