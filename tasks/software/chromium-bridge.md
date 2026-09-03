---
agent_doc_session: a94c7d23-e171-482f-93f4-530b7f2c436f
resume:
  codex: 01a05da3-e4c6-7713-bc93-024c04bd6d7a
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->
chromium-bridge v0.2.0 released. Type command shipped. Sift integration via corky search backend. Pipeline: chromium-bridge → corky → sift/ragie.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/bb8c740a0ebacf764f935ba2b918e38c9401a2d86031faaf8bfbf291cb18ba6a-20260505-173309.md`*

❯ Active backlog:
❯ - [ ] [#cbnet] [recommended] Add network request listing/inspection to chromium-bridge
❯ - [ ] [#cbsnap] [recommended] Add structured accessibility snapshot output with stable element refs to chromium-bridge
❯ - [ ] [#brewtap] Create btakita/homebrew-tap for brew formula distribution (deferred — low priority)
❯ - 2 more active item(s)
❯ do [#cbnet]. spec-test-build-install-commit-push
### Re: #cbnet — gpt-5
Implemented `network` capture for chromium-bridge.

What shipped:
- `chromium-bridge network list` reloads the target tab by default, or navigates first with `--url`, then prints captured request ids, status, type, size, and mime.
- `chromium-bridge network inspect <matcher>` captures the same load, matches by exact request id or URL substring, and prints request/response headers plus request/response bodies when CDP retains them.
- `README.md`, `SPEC.md`, and bundled `SKILL.md` now document the new command family and its capture window flags.

Verification completed:
- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build --release`
- `cargo install --path . --force`
- Installed binary smoke check: `chromium-bridge network --help` and `chromium-bridge network inspect --help`

❯ Git:
- Commit: `078f943` (`feat: add network request inspection`)
- Pushed to `origin/main`
❯ do [#cbsnap]. spec-test-build-install-commit-push
### Re: #cbsnap — gpt-5
Implemented structured accessibility snapshots for `chromium-bridge`.

What shipped:
- `chromium-bridge snapshot` now renders the visible accessibility tree with stable `ref=dom:<id>` / `ref=ax:<nodeId>` handles and preserved indentation.
- `chromium-bridge snapshot --json` now returns a normalized snapshot object with `rootRefs`, `nodes[*].ref`, `parentRef`, `childRefs`, and serialized AX values/properties for follow-up tooling.
- `chromium-bridge snapshot --json --raw` preserves the protocol-level AXNode dump for low-level debugging.
- `README.md`, `SPEC.md`, and bundled `SKILL.md` now document the normalized snapshot output and raw escape hatch.

Verification completed:
- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build --release`
- `cargo install --path . --force`
- Installed binary smoke checks: `chromium-bridge snapshot --help`, `chromium-bridge --help`

Git:
- Commit: `f47cb1b` (`feat: add structured accessibility snapshots`)
- Pushed to `origin/main`
❯ do [#brewtap]. spec-test-build-install-commit-push
### Re: #brewtap — gpt-5
Created the Homebrew tap and wired `chromium-bridge`'s install docs/spec to it.

What shipped:
- Created `btakita/homebrew-tap` (`https://github.com/btakita/homebrew-tap`) with `Formula/chromium-bridge.rb` targeting the published `v0.2.0` GitHub release assets for macOS/Linux on `x86_64` and `aarch64`, plus a tap README with install/upgrade commands.
- Updated `src/chromium-bridge/README.md` and `SPEC.md` to document `brew tap btakita/tap && brew install chromium-bridge`.
- Formula validation now uses `chromium-bridge help` instead of `--version`, because the CLI does not expose a version flag.

Verification completed:
- `cargo fmt --check`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build --release`
- `cargo install --path . --force`
- Smoke checks: `chromium-bridge help`, `chromium-bridge network --help`
- Release asset digests verified from GitHub for `v0.2.0`
- `brew` and `ruby` are not installed in this environment, so I could not run `brew audit` or a Ruby syntax check locally.

Git:
- `btakita/homebrew-tap`: `2a5f674` (`feat: add chromium-bridge formula`) — pushed to `origin/main`
- `btakita/chromium-bridge`: `7a59853` (`docs: add homebrew install path`) — pushed to `origin/main`

Note:
- The tap currently targets the latest published release assets, which are `v0.2.0`. The source repo is already at `0.2.1`, so the formula needs a version/digest bump when `v0.2.1` is released.
❯ do [#3jmq]. spec-test-build-install-commit-push
### Re: #3jmq — gpt-5
Implemented `chromium-bridge ingest` for direct corky ingestion.

What shipped:
- Added `chromium-bridge ingest <url>` to navigate, extract markdown, and write a corky-style conversation document directly into the resolved `mail/conversations/` corpus.
- Added corky-aware resolution for the target mailbox root (`.corky.toml` / `corky.toml` in cwd, `mail/` in cwd, `CORKY_DATA`, then `~/Documents/mail`).
- Added optional post-write integration flags: `--route` runs `corky sync routes`, and `--ragie-push` / `--ragie-full` run Ragie indexing through corky.
- Updated `README.md`, `SPEC.md`, `SKILL.md`, and `CLAUDE.md`, and added unit coverage for slug generation, corky path resolution, and rendered document shape.

Verification completed:
- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build --release`
- `cargo install --path . --force`
- Smoke checks: `chromium-bridge ingest --help`, `chromium-bridge --help`

Git:
- Commit: `a293d3f` (`feat: add corky ingest command`)
- Pushed to `origin/main`
❯ do [#fp48]. spec-test-build-install-commit-push
### Re: #fp48 — gpt-5
Implemented project mailbox mirroring for `chromium-bridge ingest` so one capture can stay in the shared corky corpus and also land in a project-specific corpus for sift.

What shipped:
- `chromium-bridge ingest --mailbox <name>` now writes the normal root file plus a mirrored copy under `mailboxes/<name>/conversations/`, with path-separator validation so the extra write stays inside the corky tree.
- Human and JSON output now report the mailbox copy path when present.
- `README.md`, `SPEC.md`, `SKILL.md`, and `CLAUDE.md` now document the mailbox corpus pattern and show how to point multiple corky sift backends at shared plus project-specific corpora.

Verification completed:
- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build --release`
- `cargo install --path . --force`
- Smoke checks: `chromium-bridge ingest --help`, `chromium-bridge --help`

Git:
- Commit: `85aa751` (`feat: mirror ingests into project mailboxes`)
- Pushed to `origin/main`
### Re: Windows install package — gpt-5

> **Queue prompt:** Please add a windows package that can be installed.

Released `chromium-bridge v0.3.1` with installable Windows packages.

What shipped:
- Tagged releases now build and publish `.zip` packages for Windows x64 (`x86_64-pc-windows-msvc`) and ARM64 (`aarch64-pc-windows-msvc`).
- Added a PowerShell installer that detects the host architecture, downloads the matching GitHub release asset, requires and verifies its SHA-256 release digest, installs `chromium-bridge.exe` under `%LOCALAPPDATA%\Programs\chromium-bridge`, and adds that directory to the user `PATH`.
- Added packaging contract tests and updated README/spec distribution documentation.

Install from PowerShell:

```powershell
irm https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.ps1 | iex
```

Verification completed:
- `cargo fmt --check`
- `make check` — Clippy clean; 14 tests passed
- `cargo build --release`
- `cargo install --path . --force`
- Installed binary smoke check: `chromium-bridge help`
- `actionlint .github/workflows/release.yml`
- GitHub Release workflow: all six build targets and the release job passed; the Windows x64 job also installed and ran the packaged executable on a Windows runner
- Published ZIPs were downloaded, inspected, and matched GitHub's SHA-256 digests:
  - x64: `4f58d08ee6cbf9c80c7938a84d83fcb1cc98e9620e959c2330b228cdc9848a2f`
  - ARM64: `f67c1492a29413d91c8f699878511f1e6709c2303f22028b949fe084092b173d`

Git:
- Commit: `7713bc2` (`feat: add installable Windows packages`) — pushed to `origin/main`
- Release: [v0.3.1](https://github.com/btakita/chromium-bridge/releases/tag/v0.3.1)
`backend.md` is unwedgeed and active again.

- Fixed the retained queue-strike settlement invariant in agent-doc 0.35.320.
- Installed it, recycled the controller, and verified `session-check` succeeds.
- A pure preflight now selects the next queue item with `queue_continuation_required=true`.
- The backend Claude pane is actively processing work.
- Published commit `7fde87d8a` and tag `v0.35.320`; release workflows are running.
- `make check` and targeted settlement/controller tests passed.

The fix remains fail-closed: authority-only settlement applies solely to committed `free_text_strike` intents with complete response and queue-delta proof. Other divergence still requires normal editor/disk convergence.

The Chromium Windows packages are published in [v0.3.1](https://github.com/btakita/chromium-bridge/releases/tag/v0.3.1). Its agent-doc response is safely retained in editor/controller authority, but IntelliJ 0.2.371 has not saved it to disk. Plugin 0.2.372 is installed; please restart/reload IntelliJ, then tell me and I’ll complete the pending commit check.

### Re: Fix contracts.md Run Agent Doc stall — gpt-5

Fixed and released agent-doc 0.35.321.

Runtime recovery completed first:

- `contracts.md`: repaired the retained response, removed only the already-answered free-text queue residue (`What is the PR review + merge order?`) through the binary-owned component path, committed it, and proved the closeout with `session-check`. The next runnable queue item then started and completed. After the release install, the stale supervisor was replaced in continue mode; the session is now generation 13, `alive-idle`, `prompt_ready=true`, with controller and supervisor both mapped to the installed binary.
- `backend.md`: cleared the stale runtime state and interrupted-clear residue. `session-check` repaired its historical committed-capture snapshot drift and now reports the latest cycle committed. Its one remaining queue head is intentionally deferred for operator proof rather than a runnable item being stranded. Controller and supervisor freshness both match the installed binary.

The root cause was an explicit `PassThroughPlainTrigger` exception in route admission. An editor `Run Agent Doc` could submit a new bare trigger while the prior cycle still owned an unresolved response/component closeout. The new preflight then correctly refused admission, leaving the user-facing session looking wedged; repeated observation could not repair the ordering violation.

The fix removes that exception. Every routed trigger now crosses the same closeout boundary before tmux submission:

- no open cycle → dispatch normally;
- open cycle → perform the existing single recovery attempt, then await the controller's closeout projection edge;
- terminal/superseded cycle → dispatch;
- unresolved cycle after the bounded wait → fail closed without injecting another turn.

Plain triggers remain plain after admission: one tmux text-plus-submit operation, no prompt-derived queue mutation, and no pane-capture or dispatch-start polling.

Architecture contract:

- Invariant: route cannot report a new Run as dispatched while a prior response or retained component write remains open.
- Policy owner: `agent-doc-controller` classifies the closeout gate; `agent-doc-route-io` performs only the classified effects.
- Reactive topology: document/cycle authority → controller closeout projection → terminal state edge → route injection → dispatch receipt.
- Imperative audit: the existing one-shot recovery attempt remains; long-lived progress is controller-state-driven. No new polling loop, SQLite arbitration path, side channel, or unsafe disk-authority fallback was added.

Verification passed:

- controller closeout-policy regression;
- routed-I/O plain-Run/open-cycle regression;
- full `make check`;
- prior CI and tmux-CI inspection were green;
- exact committed checkout installed successfully, including editor packages, followed by library reload and controller recycle.

Published `agent-doc` commit `2ae58cba8` and tag `v0.35.321`; CI, Release, and PyPI workflows were observed after dispatch without waiting on them. Concurrent uncommitted closeout/repair work that appeared later in the shared submodule was preserved and excluded from this release.

### Re: Response

Fixed, released, installed, and pushed.

- `contracts.md` is unwedged: stale queue residue removed, closeout committed, `session-check` passes, and the fresh supervisor reports `alive-idle` / `prompt_ready=true`.
- `backend.md` is unwedged: historical snapshot drift repaired and the cycle is committed. Its remaining queue item is intentionally operator-gated.
- Root cause fixed: plain Run triggers can no longer bypass an unresolved closeout.
- Released `agent-doc` v0.35.321 at `2ae58cba8`.
- Full `make check` and targeted regressions passed.
- Installed from the exact committed checkout and recycled the runtimes.
- Pushed agent-doc, tag `v0.35.321`, and agent-loop main through `5b1d1fbd34`.
- Concurrent unrelated worktree changes were preserved and excluded.
<!-- agent:boundary:28f7ea75 -->
<!-- /agent:exchange -->
###

## Queue

<!-- agent:queue priority -->
- 🚧 do [#fixcontractsrun]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] 🚧 [#fixcontractsrun] Fix contracts.md Run Agent Doc stall
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:notes -->
<!-- /agent:notes -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

<!-- agent:done archive="tasks/chromium-bridge.done.md" -->

<!-- completed work archived in tasks/backend.done.md -->
<!-- /agent:done -->
