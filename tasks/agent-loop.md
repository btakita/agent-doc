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

### Submodules committed & pushed

| Submodule | Commit | Summary |
|-----------|--------|---------|
| **agent-doc** | `7b8c95c`, `b043099` | vscode v0.2.1 + README, bump to v0.17.16 |
| **briantakita.me-dev** | `c557bc2` | add @rappstack/domain--server dep |
| **brookebrodack-dev** | `7c09f1e` | deploy workflow + deps + site build |
| **rappstack-dev** | `19ee4da` | update domain--server |
| **tmux-router** | `7391d14`, `2c65cfd` | send_keys timing + focus_pane logging, bump to v0.2.5 |

### Released

- **agent-doc v0.17.16** — crates.io, PyPI, GitHub Release (binary + vsix + JB plugin)
- **tmux-router v0.2.5** — crates.io, GitHub Release (binary)

Add VERSIONS.md to tmux-router

### agent-loop

Parent repo committed (`2b39ffb`) and pushed — all submodule pointers + task documents updated.
<!-- /agent:exchange -->
