---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: codex
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---



## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange. Key decisions: move orchestration and post-preflight dispatch toward binary-owned routing; shared orchestrate prompt is still not a first-class feature; frontmatter prompt presets were planned and #fppr Option A was implemented in src/agent-doc; the cancelled Codex Stop-hook replay plan is implemented in src/agent-doc as acc7478, which blocks transcript-shaped last_assistant_message payloads from being replayed and stores blocked payloads only for diagnostics. Open work remains #fppr and #ha3h.

❯ #fppr: If I change the frontmatter while a claude/codex session is active, will agent-doc pick up the changes?
### Re: #fppr frontmatter changes in an active session — gpt-5

Yes for the next `agent-doc` cycle, but not always immediately and not for every setting.

What picks up frontmatter changes on the next cycle:
- `agent-doc preflight` rereads the document each run, so frontmatter-driven routing like `prompt_presets`, `model`, `env`, and similar document-derived fields are picked up on the next preflight/respond cycle
- `agent-doc plan` and `agent-doc orchestrate` also reread the current file, so `#fppr`-style preset changes are picked up the next time those commands run

What does **not** hot-reload into an already-running harness process:
- startup-time harness settings such as `agent_args`, `claude_args`, and `codex_args` are resolved once in `start` and then held for the lifetime of that running Claude/Codex process
- anything already prepared/sent for an in-flight orchestrate step will keep using the prompt that was built at launch time

So the practical answer for `#fppr` is:
- if you edit `prompt_presets` in frontmatter and then trigger the next `preflight` / `orchestrate` cycle, `agent-doc` should use the updated presets
- if you want changed harness startup args to apply, restart that active Claude/Codex session
- if a task was already launched before the edit, that already-running task will not retroactively re-read the updated preset text

❯ do #fppr. update spec + tests. commit + push
### Re: #fppr prompt presets for orchestrate — gpt-5

Implemented the remaining `#fppr` contract in `src/agent-doc`:
- frontmatter now round-trips `prompt_presets`
- preflight emits and validates `prompt_presets_requested`
- orchestrate resolves `preset ...` / `presets ...` directives and expands labeled preset blocks into sequential, dag, and parallel task prompts
- spec and skill docs now describe the binary-owned preset flow

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path .`

Committed and pushed in `src/agent-doc` as `ed38f01` on `20260422/codex-agent-backend`.

❯ How can I define a preset?
### Re: defining a prompt preset — gpt-5

Define it in the document frontmatter under `prompt_presets`, then reference it from the exchange with `preset <name>` or `presets <a>, <b>`.

Example:

```yaml
---
prompt_presets:
  "#1": |
    Today is 2026-04-25.
    Keep the work tree clean.
  release-check: |
    Run the full verification suite before finalize.
---
```

Then in `agent:exchange`:

```md
synchronous orchestra
preset #1
- do #prep
- do #report
```

Or with multiple presets:

```md
parallel orchestra
presets #1, release-check
- do #a
- do #b
```

`agent-doc orchestrate` expands those into each task prompt as labeled blocks like `(preset #1)` before the task text.
<!-- agent:boundary:497d6f8d -->
<!-- /agent:exchange -->

<!--
Why did the prompt "When I call an orchestrate, can I define a prompt that will be applied to every orchestrate task in one place?" not have a prepended `❯ ` in the patchback?
 
Plan next steps with the "binary-owned prose grammar". Such as how subagents should orchestrate their subtasks. However, src/claudescore tasks/docs.md did finish #ntoc in <10m, so Claude Code figured out how to execute the tasks quickly. The user may instruct suboptimal instructions. Perhaps we could have a plan mode as well to discuss how to orchestrate? Or maybe we should just rely on the agent harness to orchestrate?
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
