---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
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

**Completed** — all submodules committed, pushed, and parent pointers updated.

| Submodule | Action |
|-----------|--------|
| mail | Committed 94 files (conversations, contacts, drafts, social) + pushed. Note: `contacts/kevin-volk/recordings/20260309-kevin-volk.mp4` (1.5GB) excluded — added to .gitignore. |
| philosophy | Committed .iml update + pushed |
| resume | Committed .iml update + pushed (pointer advanced to `0638e8e`) |
| censible-core | Committed .iml update + pushed |
| cloudflare | Committed .iml update + pushed |
| cogov-dev | Committed .iml update + pushed |
| herbaliciousbliss-dev | Committed .iml update + pushed |
| agent-doc | Pointer updated (v0.17.9) |
| corky | Pointer updated (v0.15.2) |
| ctx-core-dev | Pointer updated |
| lazily-py | Pointer updated |
| lazily-zig | Pointer updated |
| prescience-notes-dev | Pointer updated |
| rappstack-dev | Pointer updated |

**Releases:** All publishable projects (agent-doc, corky, lazily-py, lazily-zig) are at their latest tags — no pending releases needed.

Parent commit: `862dc11` — includes all 14 submodule pointers, task docs, skills, settings. Pushed to `main`.

Also added `*.mp4` to `tasks/youtube/.gitignore` (1.6GB + 3.7GB video files were too large for git).

Was the .iml file added in the commit? If yes...let's reset and ignore iml files.
<!-- /agent:exchange -->
