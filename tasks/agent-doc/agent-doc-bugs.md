---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---


## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — env frontmatter wired into all Claude-spawning paths (start / run / stream / parallel). Build clean, 42 targeted tests passing. Ready for live test.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
v0.33.0 session. Landed: tsift project extracted (btakita/tsift, src/tsift submodule, tasks/tsift.md) absorbing skills-janitor; #9v7h (--pending-add-gated + hash echo + state marker rejection); #txex (notify --pending-add/--pending-add-gated, auto-create pending, pending-only mode); #hrmt (synced repo-local SKILL.md to v0.32.0); #e46w/#f8j3/#y4da/#zvmd/#np8p/#0xka (already fixed: boundary lag, narrow_to_submodule, annotate_re_headings); #77mf (typed gate markers [/release]/[/deploy], resolve-gate per-file+project-wide, set-gate-type, skill-layer inference on do); v0.33.0 released crates.io+GitHub (typed gates, resolve-gate, --pending-only, --pending-add-gated, --status, notify pending, session clear, supervisor PTY); #p2xm already fixed (orphaned patch:exchange leak template.rs:257-265); #wcf5 (copy-on-read guard: mtime capture, skip recovery on mtime change, 4 integration tests). State: 1116 tests, 2 gated pending (#a002 awaiting production telemetry, #ha3h blocked on multi-user support).
<!-- agent:boundary:76ad7a6e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#a002] Safety rail + forensic log in normalize_user_prompts_in_exchange — ✅ landed, awaiting large-drift trip for `normalize_threshold_exceeded` telemetry.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Ship with multi-user support, not before. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->
## Assistant
