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
### Re: CI fixes (corky + agent-kit)

#### agent-kit v0.4.0 — done

- Clippy `collapsible_if` fix committed, re-tagged
- `cargo publish` fails "already exists" (expected — already on crates.io)
- **GitHub Release created manually**: https://github.com/btakita/agent-kit/releases/tag/v0.4.0

#### corky v0.26.3 — CI iterating

Round 3 of CI fixes:
- `aarch64` cross: added `libssl-dev` (x86_64 host) alongside `libssl-dev:arm64` in Cross.toml
- CUDA: added `libcublas-dev-12-6` step (Jimver sub-packages can't install it via `cuda-` prefix)

New CI run in progress. Waiting on result.
❯ verify:
❯ - [ ] **corky v0.26.3**: CI running (fix: host libssl-dev + libcublas-dev-12-6)
### Re: verify

Previous CI run used stale tag — it pointed to `7df8203`, not `9a0b6ea` (host libssl-dev + libcublas-dev). Retagged to correct commit and triggered new run.

**New run**: `24148041223` (in progress, "fix(ci): add host libssl-dev for cross build; add libcublas-dev-12-6")

Will update when complete.

❯ commit and push submodules
❯ release anything pending
❯ commit and push agent-loop
### Re: commit/push submodules, release, push agent-loop (HEAD)

#### Submodules committed and pushed

- **mail**: sync conversations + contacts (Robert Ross Slack drafts)
- **src/agent-doc**: `ff17475` streaming checkpoints + baseline-file support, v0.31.22
- **src/censible-core/data**: archived ESG report PDFs removed (104 files)
- **src/censible-core**: pointer updated
- **src/eval-runner**: Cargo.lock updated (agent-doc v0.31.22)
- **src/equityfundingsource-dev**: added as submodule + `325ef03` SSR routing + wrangler pages config

All other submodule pointers (agent-kit, agent-memories, agent-rules, agent-runbooks, briantakita.me-dev, ctx-core-dev, instruction-files, rappstack-dev, session-share, skill-harness) were already current.

#### corky v0.26.3 — released

CI was failing through 5 iterations. Root causes found and fixed:
1. Cross Docker image (Ubuntu 16.04) had OpenSSL 1.0.2g → fix: use `edge` image (Ubuntu 20.04, OpenSSL 1.1.1)
2. `libssl-dev:arm64` contaminated HOST build linker path → fix: x86_64-only `libssl-dev`
3. `default = ["transcribe", "diarize"]` pulled `ort-sys` into aarch64 cross; `ort-sys` tries to download prebuilt ONNX Runtime binaries → fails in CI → fix: `--no-default-features` for aarch64 cross build

**GitHub Release**: https://github.com/btakita/corky/releases/tag/v0.26.3 — 7 binaries (all platforms). crates.io 0.26.3 already published. Local install updated.

#### agent-loop pushed

All submodule pointer updates committed and pushed → `84b98b52`.
<!-- agent:boundary:d38d24c5 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] **corky v0.26.3**: released — GitHub Release + local install done
- [x] **agent-kit v0.4.0**: GitHub Release created
- [x] Commit and push submodules
- [x] Commit and push agent-loop
- [ ] Run litellm proxy pointing at vllm
- [ ] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->
