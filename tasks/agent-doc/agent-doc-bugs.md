---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a08ca6-2d17-7822-948a-aaff42a157ac
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Grok v0.35.359 installed and pushed; full local verification passed. Authenticated Grok MCP closeout awaits renewed quota. Next: fix extra tmux pane with two editor panes.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260910-212720.md`*

Compacted content:
- Archived 20 response topic(s): Compact commits, editor routes, and controller timeout; GH #49/#51 and the extra shell panes; #runfpecodex; 17 more
- Prior summary/context: prior compacted content: Archived 6 response topic(s): Response; #fixqueueitem; #fixhttpsgithub; 3 more

### Re: #actionable-review — review triage — opus-5

> **Queue prompt:** #actionable-review

Audited all 8 gated `agent:review` items against live state rather than their prose. Three were not actually "awaiting review" — they were concrete actions with the action itself buried in a paragraph — and one was a future feature filed in the wrong component. Those four are now out of `agent:review` (8 → 5), with the three real actions promoted to `agent:backlog`.

**Promoted to actionable backlog:**

- **`#restartideaplugin`** — IDEA PID 1035669 has 17h57m uptime and preflight still reports plugin 0.2.378, so the staged 0.2.379 package has never loaded. This single restart is the shared prerequisite of `#tmuxfocussyncverify` and `#activateinstalledjetbrai`, and satisfies the plugin floor for `#harnessswitchverify`. It was implicit in three separate review paragraphs and named as a step in none.
- **`#pypiquotafix`** (was `#pypi345quota`) — the old text was stale in both directions. PyPI's latest is **0.35.343**, not 0.35.358, and 0.35.343 itself uploaded only **1 of 4 wheels**. Every PyPI run since has failed identically (`34521723248`: `400 Project size too large`). Measured live: **10.726 GB across 165 releases, avg 65.0 MB**. Two real paths — request a limit increase (non-destructive), or delete the 34 oldest releases `0.35.0`–`0.35.112` (all pre-2026-08-02) to free 1.75 GB → 8.98 GB, ~27 releases of headroom, permanent and version numbers unreusable. Shrinking wheels is not a third path: `[profile.release]` already runs `lto = "fat"`, `codegen-units = 1`, `strip = true`.
- **`#cargotokenrevoke`** (was `#cargo355token`) — tagged `[operator-verify]` but the outstanding work is the rotation, not a verification. Checked the blast radius so the revoke can proceed without hesitation: agent-doc has no `cargo publish` step and no `CARGO_REGISTRY_TOKEN` in any of its five workflows, so revoking cannot red an agent-doc job.

**Moved to icebox:** `#ha3h` → `#ha3hpolicy`. Issue #40 is open and explicitly says not to ship before multi-user workspace support exists. That is an unstarted future feature, not code-complete work awaiting validation, so it does not belong in `agent:review`.

**Left gated, blockers confirmed real:** `#tmuxfocussyncverify`, `#activateinstalledjetbrai`, `#harnessswitchverify` (all need a live IDE plus human interaction after the restart), `#grokmcpverify` (external quota), `#coder7` (needs a reachable Coder workspace).

**No agent-drainable work came out of this.** All three promoted items are operator-executed, so they carry `[operator-verify]` and go-mode will correctly defer them rather than churn the drain. The value here is that three one-line actions are now visible in the actionable list instead of buried in eight paragraphs of gated prose.

No code, test, or build surfaces changed this cycle — the mutations are confined to this document's tracked-work components, so no suite run applies. The standing `stale_plugin` warning (live 0.2.378 vs shipped 0.2.379) is exactly what `#restartideaplugin` now tracks.
<!-- agent:boundary:d53e8ec0:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 #actionable-review
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#restartideaplugin] [operator-verify] Restart IntelliJ IDEA to activate the installed JetBrains plugin 0.2.379. Running IDEA PID 1035669 has 17h57m uptime and preflight still reports the live plugin at 0.2.378, so the staged package has never loaded. One restart unblocks #tmuxfocussyncverify and #activateinstalledjetbrai outright and satisfies the plugin prerequisite of #harnessswitchverify.
- [ ] [#pypiquotafix] [operator-verify] Clear the PyPI 10 GB project-size block that has failed every agent-doc publish since 0.35.343. Measured now: 10.726 GB across 165 releases, avg 65.0 MB/release; run 34521723248 fails with `400 Project size too large. Limit for project agent-doc total size is 10 GB`; 0.35.343 landed only 1 of 4 wheels (macosx_10_12_x86_64) so even the last "published" version is partial. Two paths: (a) request a project-size-limit increase at https://docs.pypi.org/project-management/storage-limits#requesting-a-project-size-limit-increase — non-destructive, preferred; (b) delete the 34 oldest releases 0.35.0 through 0.35.112 (all uploaded before 2026-08-02) to free 1.75 GB, leaving 8.98 GB and roughly 27 releases of headroom — permanent, and those version numbers can never be reused. Wheel-shrinking is not a third path: the release profile already sets lto=fat, codegen-units=1, strip=true. Supersedes the stale #pypi345quota review item, which named a v0.35.358 upload that never happened.
- [ ] [#cargotokenrevoke] [operator-verify] Revoke and re-issue the crates.io API token exposed in diagnostic tool output during the pane-removal investigation, then store the replacement via passage. Blast radius is local only: agent-doc ships no `cargo publish` step and holds no CARGO_REGISTRY_TOKEN secret in any of its five workflows (book, ci, mdbook, pypi, release), so revoking cannot red any agent-doc CI job. Revoke at https://crates.io/settings/tokens. Supersedes #cargo355token, which was mis-tagged as verification when the outstanding work is the rotation itself.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate installed JetBrains plugin 0.2.379, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. Automated lifecycle and transition regressions plus make check tmux-ci passed; source v0.35.362 is published. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Installed JetBrains plugin 0.2.379 is staged while running IDEA PID 1035669 remains on 0.2.378; one restart activates both the reverse tmux-focus fix and the current package generation. Remaining splice proof: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028, and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1035669 runs plugin 0.2.378; CLI/native 0.35.361 is installed. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. The unrelated Stop-hook gate-selection defect is fixed in v0.35.361; plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ha3hpolicy] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml receive.auto_create_components = ["pending"] (or = false); flip notify/write default from auto-create to consult-recipient-config. Hard-blocked until multi-user workspace support exists; issue #40 (open) explicitly says not to ship before that prerequisite. Moved out of agent:review because it is an unstarted future feature, not code-complete work awaiting validation. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
