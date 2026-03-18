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

### Re: Verification additions + SKILL.md draft

Good additions on the verification checklist — incorporated all four:
- Audit docs (via `make precommit`)
- README.md currency check
- VERSIONS.md currency check
- Test coverage for the change

Also verify that SPEC.md is up to date for the library or app.
Please ensure there is no drift between the architecture and the documentation.

**Created `/dev` skill** at `.claude/skills/dev/SKILL.md`. Here's what it covers:

**Commands:**
- `/dev <submodule> check` — run checks (clippy, test, audit-docs)
- `/dev <submodule> release` — full release orchestration with pre-release checklist
- `/dev <submodule> status` — version, dirty state, unpushed commits, tag drift
- `/dev status` — all submodules at once
- `/dev <submodule> changelog` — draft VERSIONS.md entry from git log

**Release workflow** is the core — it's type-aware:
| Type | Projects | Targets |
|------|----------|---------|
| Rust+Maturin | agent-doc, corky | crates.io + PyPI + GitHub Release + binary |
| Rust lib | instruction-files, tmux-router, agent-kit, tagpath | crates.io |
| Python | lazily-py | PyPI |
| JS/TS apps | briantakita.me-dev, brookebrodack-dev, presence | Docker + VPS |

**Pre-release checklist** (blocks release if any fail):
1. Tests pass (`make check`)
2. Audit docs pass (`make precommit`)
3. README.md is current
4. VERSIONS.md has new version entry
5. Tests cover the change (regression test for bugfix, basic coverage for feature)
6. Version bump done (Cargo.toml + pyproject.toml in sync)
7. Clean working tree

**Also handles:**
- Dependency ordering (instruction-files → agent-doc → corky)
- `pass` integration for all secrets
- Unpublished tag recovery (move tag forward)
- Parent submodule pointer updates after release
- Post-publish verification (cargo search, pip index, gh release view)

The skill is a thin orchestrator — no CLI binary, just a SKILL.md that Claude follows. Want to test-drive it on a submodule, or iterate on the design first?

The dev skill should also have meta cognition. It should update whenever relevant changes are made.
 <!-- /agent:exchange -->
