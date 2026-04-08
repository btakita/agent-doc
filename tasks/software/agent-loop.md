---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
agent_doc_auto_compact: 250
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

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260408-160355.md`*
### Re: CI fixes (corky + agent-kit) (HEAD)

#### agent-kit v0.4.0 — done

- Clippy `collapsible_if` fix committed, re-tagged
- `cargo publish` fails "already exists" (expected — already on crates.io)
- **GitHub Release created manually**: https://github.com/btakita/agent-kit/releases/tag/v0.4.0

#### corky v0.26.3 — CI iterating

Round 3 of CI fixes:
- `aarch64` cross: added `libssl-dev` (x86_64 host) alongside `libssl-dev:arm64` in Cross.toml
- CUDA: added `libcublas-dev-12-6` step (Jimver sub-packages can't install it via `cuda-` prefix)

New CI run in progress. Waiting on result.
<!-- agent:boundary:cca9f168 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [ ] **corky v0.26.3**: CI running (fix: host libssl-dev + libcublas-dev-12-6)
- [x] **agent-kit v0.4.0**: GitHub Release created
- [ ] Run litellm proxy pointing at vllm
- [ ] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->
