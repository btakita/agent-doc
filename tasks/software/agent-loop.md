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
**Decisions/outcomes:**
- Auto-compact: user wants more control; aggressive auto-compact unwanted. Partial auto-compact (keep active topics, archive older) preferred but not yet implemented. `agent_doc_auto_compact: 250` set as safety net.
- Eval tuning (all done): system prompt added explaining do/conciseness/### Re: header conventions; token budgets tuned per case; `simple-question-concise` fixed (0.3→0.7) by clarifying headers are multi-topic only.
- Partial compact feature was considered but dropped from pending by user.

Setup pyproject.toml
I want to have vllm as a dependency.
My goal is to run Qwen3-Coder-Next (80B) on vllm to avoid running out of claude tokens over the next couple of days
### Re: Setup pyproject.toml with vllm

`pyproject.toml` was empty — populated it with `vllm` as a dependency:

```toml
[project]
name = "agent-loop"
version = "0.1.0"
requires-python = ">=3.10,<3.13"
dependencies = [
    "vllm",
]
```

**Python version issue:** vllm requires Python 3.9–3.12. The project's `.mise.toml` pins Python 3.14 (too new). You have 3.12.12 available via mise.

To use vllm, either:
1. Override Python for this project: `mise use python@3.12` (updates `.mise.toml`)
2. Or create a separate venv with 3.12: `mise exec python@3.12 -- uv venv .venv && source .venv/bin/activate && uv pip install vllm`

For Qwen3-Coder-Next 80B you'll also need enough VRAM — 80B at fp16 is ~160GB, so either 2× H100 or use quantization (e.g. AWQ/GPTQ ~40–80GB). Run with:
```bash
vllm serve44 Qwen/Qwen3-Coder-Next-80B --tensor-parallel-size 2
```
I have a 5090 and a large amount of available RAM. What would be the best local coding model for me to run? 
### Re: Typo fix + best model for RTX 5090

Looks like `vllm serve44` is a typo — should be `vllm serve`.

**RTX 5090 has 32GB VRAM.** Qwen3-Coder-Next 80B needs ~45GB at Q4 — won't fit in vllm (GPU-only, no CPU offload).

#### Best options

**vllm (pure GPU, fast inference):**
- `Qwen2.5-Coder-32B-AWQ` — ~18GB, fits with headroom, excellent coding quality
- Any 32B model at AWQ/Q4 precision works well

**70B+ with large RAM (llama.cpp / ollama — partial GPU offload):**
- `Qwen2.5-Coder-72B-Q4` — ~40GB total; put ~32GB layers on GPU, rest in RAM
- Slower tokens/sec but higher quality; ollama handles offload automatically:
  ```bash
  ollama run qwen2.5-coder:72b
  ```

**Recommendation:** Start with `Qwen2.5-Coder-32B-AWQ` on vllm for speed. If quality ceiling isn't enough, switch to `Qwen2.5-Coder-72B-Q4` via ollama with partial offload.

> Knowledge cutoff is Aug 2025 — "Qwen3-Coder-Next" may have released after that. Check HuggingFace for latest Qwen3-Coder variants; the 32B or 14B version would be the right size target.

Show me the 2 vllm commands.
Also show me how to integrate both with claude code
### Re: vllm commands + Claude Code integration

#### The 2 vllm serve commands

**Option 1 — Qwen2.5-Coder-32B-AWQ (fits fully in 32GB VRAM, fast):**
```bash
vllm serve Qwen/Qwen2.5-Coder-32B-Instruct-AWQ \
  --host 0.0.0.0 --port 8000 \
  --dtype auto
```

**Option 2 — Qwen2.5-Coder-72B-AWQ (offloads overflow to RAM, slower):**
```bash
vllm serve Qwen/Qwen2.5-Coder-72B-Instruct-AWQ \
  --host 0.0.0.0 --port 8000 \
  --dtype auto \
  --cpu-offload-gb 20
```
`--cpu-offload-gb 20` pushes ~20GB of weights to CPU RAM so the rest fits in VRAM.

#### Claude Code integration

Claude Code speaks Anthropic protocol; vllm speaks OpenAI protocol. Use **LiteLLM** as a translation proxy:

```bash
pip install litellm[proxy]

# Start proxy (translates Anthropic → OpenAI for vllm)
litellm --model openai/Qwen2.5-Coder-32B-Instruct-AWQ \
  --api_base http://localhost:8000/v1 \
  --port 4000
```

Then run Claude Code against the proxy:
```bash
ANTHROPIC_BASE_URL=http://localhost:4000 \
ANTHROPIC_API_KEY=fake \
claude --model Qwen2.5-Coder-32B-Instruct-AWQ
```

Or add to your shell profile / `.env`:
```bash
export ANTHROPIC_BASE_URL=http://localhost:4000
export ANTHROPIC_API_KEY=fake
```

Then `claude` will use the local model by default. Swap back to Anthropic by unsetting those vars.

❯ commit + push all submodules
### Re: commit + push all submodules

Done. Committed and pushed:

| Submodule | Commit | Notes |
|-----------|--------|-------|
| `philosophy` | docs: update README and add directive term | |
| `resume` | chore: add CDW payments profile and update resume | rebased over remote changes (BrianTakita.md conflict resolved) |
| `src/chromium-bridge` | docs: update CLAUDE.md | |
| `src/corky` | feat: add max_speaker_span_ratio + improve transcribe engine; add runbooks | |
| `src/eval-runner` | feat: add parallelism, backend support, and eval make targets | |
| `src/module-harness` | docs: add ontology-references runbook | |
| `src/session-share` | agent-doc(claude-auto-ruberic): update task | commit local only — `ClaudeScore/SessionShare` repo not found on GitHub |
| `mail` | sync: update conversations, contacts, and corky config | 1109 files |

Parent repo updated with all 8 submodule pointer updates and pushed.

❯ release the submodules
### Re: release the submodules (HEAD)

Prepped and installed — **pausing for your testing confirmation before publishing upstream**.

#### corky 0.26.1
- Bumped `Cargo.toml` to 0.26.1
- `VERSIONS.md` updated
- `make check` passed (all tests green)
- Installed locally: `cargo install --path . --features transcribe-cuda,diarize`
- **Waiting on:** please test `corky transcribe` in a live session, then say "publish corky" to proceed

#### agent-kit 0.4.0
- `VERSIONS.md` updated with 0.3.1 + 0.4.0 entries
- `cargo clippy` + tests passed (43 tests)
- **Waiting on:** confirm OK to tag + publish

#### Not released (no new features since last tag)
- `src/chromium-bridge` — CLAUDE.md edit only
- `src/agent-memories/rules/runbooks/skill-harness` — CI fix only, no version bump needed
- `src/eval-runner` — internal tool, not published to crates.io
- `philosophy`, `resume`, `mail` — content repos, no release target
<!-- agent:boundary:6ad97644 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] Fix Python version for vllm: `mise use python@3.12`
- [x] Install vllm: `uv pip install vllm`
- [x] Download + serve `Qwen2.5-Coder-32B-Instruct-AWQ` via vllm
- [x] Install litellm proxy: `pip install litellm[proxy]`
- [ ] Run litellm proxy pointing at vllm
- [ ] Test `claude` against local model via `ANTHROPIC_BASE_URL`
- [ ] Fix `src/session-share` remote — `ClaudeScore/SessionShare` not found on GitHub
- [ ] **Manual test gate**: test `corky transcribe`, then approve to publish corky v0.26.1
- [ ] **Publish gate**: approve agent-kit v0.4.0 tag + publish
<!-- /agent:pending -->
