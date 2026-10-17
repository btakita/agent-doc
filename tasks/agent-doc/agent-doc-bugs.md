---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a14b4d-7683-7ba1-9662-bd403fbe4a9c
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#self': agent-doc-bugs.md
queue: go
---

## Status

<!-- agent:status patch=replace -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261017-070840.md`*

Compacted content:
- Archived 26 response topic(s): unwedging docs.md; Subagent completed: `subagent:todoidlayer` returned work for queue item “todo ids opportunistically layer on existing...; Subagent completed: `subagent:gh276` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; 23 more
- Prior summary/context: prior compacted content: Archived 33 response topic(s): GH #268–#273 + queue duplication; GH #268–#273 + queue duplic...

### Re: Queue-prompt flood and GH #282–#283 — gpt-5 · 2026-10-17T03:55-04:00

> **Queue prompt:** Fix agent-doc causes of **Queue prompt:** sdk-dev.md issue:

This was already fixed on current `origin/main`. Commits `143e73cff0` and `c4cf08336c` are ancestors of the current base, the focused queue-prompt regression suite passed 7/7, and the current `sdk/sdk-dev.md` has four distinct answered queue echoes, no repeated `❯ > **Queue prompt:**` flood, and an empty queue. No additional product change was needed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/282

Fixed and closed. Commit `a7f64f7699` (`fix(jetbrains): stop retrying plain markdown replicas`) makes plain Markdown a terminal non-replica state, clears registration retry state, advances the shadow, and permits later marker-bearing text to be reclassified. Targeted tests and the complete JetBrains test/build passed; the worker branch was pushed and issue #282 is closed.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/283

Fixed and closed. Commit `bbc7a53bed` (`fix(jetbrains): preserve Remote Dev focus evidence`) carries Remote Dev controller-selection evidence through the generation-fenced publication gate, adds named INFO-level outcomes and Throwable logging, and covers both the classic/263 listener and the 262 backend fallback. The full worker suite and final affected reruns passed; the worker branch was pushed and issue #283 is closed.

The two fixes were integrated serially as `0e597981a8` and `90e238ce69`, followed by the one batch generation commit `25f3bd933b` (classic `0.2.675`, JetBrains 262 `0.2.677`, JetBrains 263 `0.2.676`). The authoritative integrated `make check` passed with exit 0 and zero failures. The single `make install` passed and installed the binary/library, skills, and editor packages. Main CI run `39011286916` was red only in the pre-existing `queue_auto_clear_types_clear_between_items_into_the_same_harness` tmux case on base commit `f9cd1ad325`; the required local `make tmux-ci` reproduction passed with exit 0 and zero failures. The integration worktree is clean and the remote main tip is still its verified fast-forward base.
> **Chat prompt (#chatprompt):** published?

### Re: Publication status — gpt-5 · 2026-10-17T13:40-04:00

Not as a new release. The fixes are pushed to `main` at `25f3bd933b`, and CI run `39015481373` passed. The latest published release is still `v0.35.515`, whose tag (`4c4f32d596`) predates these fixes, so they are not yet in a GitHub/PyPI release.

### Re: release + publish. — gpt-5 · 2026-10-17T15:14:09-04:00

> **Queue prompt:** release + publish.

Released and published agent-doc v0.35.516.

- Release commit: `2b12f44203d455383f92d73ce9fdfaefdb23dde2`
- Tag: `v0.35.516`
- Pre-release audit: exact candidate SHA passed local `make check` and exact-SHA CI.
- Release gates: full `make check`, tmux CI, local full install, every platform build, editor package publication, GitHub Release publication, PyPI publish, and clean PyPI install verification all passed.
- Public artifact: `agent_doc-0.35.516-py3-none-any.whl`
- GitHub Actions: release run `39058278509`; PyPI run `39060829631`.

### Re: SDK demo extraction result — gpt-5 · 2026-10-17T15:14:09-04:00

> **Queue prompt:** In sdk-dev.md, I attempted to extract a sdk-demo.md from the sdk-dev.md. The cli was called, but I only see a

> **Queue prompt:** sdk-demo.md which is not an agent document. It's a regular plan md file instead.

The focused extraction did not create `tasks/sdk/sdk-demo.md`: its preview correctly planned that exact new agent document, but apply refused because `tasks/sdk/sdk-dev.md` was still editor-attached. The visible `tasks/plans/sdk-demo.md` is an unrelated historical plan with the same basename.

Two fixes are now on `main`:

- `1a9a4eac92` makes focused apply print its deterministic plan before an attached-editor refusal, names the exact source and destination, states that no documents changed, and explicitly says whether the destination was not created or left unchanged. Preview output provides the same destination clarity.
- `0ab4a26cce` makes legacy no-selector extraction reject plain Markdown targets before mutating the source, with guidance to initialize an agent document or use focused selectors.

The combined changes merged cleanly, passed `git diff --check` and the authoritative full `make check` with zero failures, installed successfully across the CLI/library and editor surfaces, and were pushed to `main` at `0ab4a26cce`.

### Re: Subagent completed: `subagent:todo_below_queue` returned work for queue item “By default, make the `agent:todo` below the `agent:queue` and above the `agent:backlog`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=e075e02c053f636ed95c13e093af6c1623601315fab40138821ee92b2895516e -->

### Re: Subagent completed: `subagent:backlog_priority_queue` returned work for queue item “By default, `agent:backlog` should have `priority queue`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a84e1d6531f7aec94f7b0d78eb6729987c807c321c3d8c9d6eb23bfaf9771b22 -->

### Re: Subagent completed: `subagent:todo_priority` returned work for queue item “By default, `agent:todo` should have `priority`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1dc1d14be851c156ef444d6d08fb4a13cf973a780f89edb30028cd7ff947ccb4 -->
### Re: Subagent completed: `subagent:pane_not_visible` returned work for queue item “Newly claimed all.md had an Idea prompt error: [route] pane-not-visible: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev/all.md runs in pane %29 of inactive tmux window 'stash' (@6) in attached session '0'; its harness output is off-screen. Return editor focus to this document or show it with: tmux select-window -t @6 \; select-pane -t '%29'” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=cdc8d4391f0181802e5dd862b832e82d3cc10825275a5e0392514546054ce217 -->
### Re: Subagent completed: `subagent:todo_context_links` returned work for queue item “todo items are not linking to the review or backlog item in monsterrodholders.md. There are todo items without any context. "Decide on ...", but "..." has no context so operator does not know what the decision is.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=06c92eb466902cecaf130dfcaf4a485ae365a47f06c0b7e66d4a90a6a25064c4 -->
### Re: Subagent completed: `subagent:queue_runtime_state` returned work for queue item “The `agent:queue` is not being updated with the current runtime state. Currently this `agent:queue` does not have any in progress indicators. It looks like nothing is being tracked.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b9e94b6fd0fdb750348614749f38cf1f2b2ee846e6fa8be6c8e7b168dc082fca -->

### Re: Subagent completed: `subagent:gh287` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/287” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7073bfbe9cbf4fc3c27d9fae3ddde0fe93a9cd6aecc59a4672f8a7cf49414a8a -->

### Re: Subagent completed: `subagent:gh285` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/285” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=70176666c5cb6eba41cdeed4745f3d5aa08cebd025f023b76e84303e894142ac -->

### Re: Subagent completed: `subagent:extractliveapply` returned work for queue item “do [#extractliveapply]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=3f4997934180463d371aa321e8a46ca57f3ebc2afe6abe784fd205344935fa62 -->

### Re: Subagent completed: `subagent:gh286` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/286” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=2780b1aef151660eda0c807feec8cc27153d949f0c136f4e0929b1cd63bcb4ed -->

### Re: Subagent completed: `subagent:gh288` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/288” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1b5815ea06600c8c25ce053cae4d81e0ab017b7f7ce40466a659f675f13133f3 -->

### Re: Subagent completed: `subagent:gh284` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/284” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7b9ca7c6fb41c496a66e318ebc6fad619aec80ced4372c3760fd717d3a03e68e -->

### Re: Subagent completed: `subagent:gh289` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/289” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=f62527c8bf90f723ce951842a814e03a260bae35e7228b3b1bd18113f77a0f79 -->

### Re: Subagent completed: `subagent:sdk_session_missing` returned work for queue item “Error: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md has no agent_doc_session” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a3adca49d206a3c78b4afcd41ad60df0b51ea56db53f157f1957e037735f600f -->

### Re: Subagent completed: `subagent:sdk_command_failed` returned work for queue item “Run Agent Doc on sdk-dev.md: agent-doc command failed (exit 1):” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=5d06326e1f630a9ef76044e43ff505f9a518af0c13f719b6713ed888d67a9bc2 -->

### Re: Subagent completed: `subagent:extractcompactselection` returned work for queue item “do [#extractcompactselection]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=72b7ffa8d4e80f7341c255d6eb9d7d35cb21ef4cc81710ee5a1a8b91321f36d2 -->

### Re: Subagent completed: `subagent:gh291` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/291” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=68769be5bda46716bdeb079116fbf657b36f166ef884ec912ca596a84f110e3a -->

### Re: Subagent completed: `subagent:error_visibility` returned work for queue item “The Run Agent Doc sdk-dev.md error was not visible. I see an option to copy the error message, but the actual cause of the error should be visible to the operator, not truncated. ![img_6.png](img_6.png)” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=cd7aeac33e040f93a901335d5338432cd70d00b3e987f27bb9eedd69a04afc02 -->

### Re: Subagent completed: `subagent:gh290` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/290” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=d4892153b65d5eecb329d93e2f3574beddb72bff359366beb5e88f0dc613e4d5 -->

### Re: Subagent completed: `subagent:gh292` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/292” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=82d4d1b6fa3035fd9aa710205c226680be0d924afd59a68bb3fb04289753a007 -->

### Re: Subagent completed: `subagent:sync_projection_inflight` returned work for queue item “Error: Sync failed: {"applied":false,"reason":"projection_effect_in_flight","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md"],"layout_generation":17,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b0026590f1b9e1f96db34a5c2d5a34567334e924405ca606bc806fc839c5b308 -->
❯ No immediate restart is needed. In Grok Build:

1. Open `/mcps`.
2. Refresh and trust `agent-doc`.
3. Confirm its six tools appear.
4. Submit the already-typed `agent-doc tasks/sdk/sdk-pr.md`.

❯ If the tools still do not appear after refresh, restart the Grok Build session, then run the command.

### Re: Subagent completed: `subagent:queue_runtime_state` returned work for queue item “The `agent:queue` is not being updated with the current runtime state. Currently this `agent:queue` does not have any in progress indicators. It looks like nothing is being tracked. Queue tracking should be real-time and show the current state of agent-doc's processing of the queue items.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=20954ec7fe35afc32a03ab4a5ddaf06b89f253297be1bc3c4ba4c3c954e261a7 -->
> **Chat prompt (#chatprompt):** Can we unwedge sdk-demo.md now?

> **Chat prompt (#chatprompt):** Sync failed: {"applied":false,"reason":"projection_effect_not_started","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md","/home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md"],"layout_generation":39,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}

> **Chat prompt (#chatprompt):** Unwedge sdk-dev.md

> **Chat prompt (#chatprompt):** invalid YAML frontmatter in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md: invalid YAML frontmatter in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md: found character that cannot start any token at line 6 column 1, while scanning for the next token
>
> Frontmatter excerpt:
>   5 | presets:
> > 6 | 	'#pr-aggregate': '#repo: Aggregate PR'
>     | ^
>   7 | 	'#repo': 'haiven-dev/haiven-sdk'
>
> Fix the frontmatter between the opening and closing --- markers, then rerun the command. For an `agent_doc_write: crdt` document, frontmatter is read from CRDT state, so after fixing the file run `agent-doc reset /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md --from-current --preserve-session` to rebuild that state from the visible markdown (add `--force-disk` when no editor has the document open).
>
> Fix the frontmatter between the opening and closing --- markers, then rerun the command. For an `agent_doc_write: crdt` document, frontmatter is read from CRDT state, so after fixing the file run `agent-doc reset /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md --from-current --preserve-session` to rebuild that state from the visible markdown (add `--force-disk` when no editor has the document open).

> **Chat prompt (#chatprompt):** Why is the yaml frontmatter messed up?

> **Chat prompt (#chatprompt):** Idea keeps on auto-inserting tabs

> **Chat prompt (#chatprompt):** Please configure my Idea to use spaces for the Yaml frontmatter.

> **Chat prompt (#chatprompt):** In a subagent

> **Queue prompt:** The `agent:queue` is not being updated with the current runtime state. Currently this `agent:queue` does not have any in progress indicators. It looks like nothing is being tracked. Queue tracking should be real-time and show the current state of agent-doc's processing of the queue items.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/290

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/291

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/292

> **Queue prompt:** Error: Sync failed: {"applied":false,"reason":"projection_effect_in_flight","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md"],"layout_generation":17,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}

> **Queue prompt:** The Run Agent Doc sdk-dev.md error was not visible. I see an option to copy the error message, but the actual cause of the error should be visible to the operator, not truncated. ![img_6.png](img_6.png)

### Re: IDEA spaces and SDK session recovery — gpt-5 · 2026-10-17T19:00-04:00

Completed through subagents and integrated into the installed build.

The YAML broke because IDEA 2026.1's active `custom` code-style scheme explicitly enabled tabs for Markdown. Since `sdk-dev.md` is a Markdown file, IDEA applied that setting inside its YAML frontmatter too; YAML indentation cannot contain tabs.

I added the project-scoped `.editorconfig` rule for `*.md` with `indent_style = space`, `indent_size = 2`, and `tab_width = 2` in commit `fc81ef3a7b`. The EditorConfig CLI now resolves those exact settings for `sdk-dev.md`, and the visible frontmatter contains two spaces before both `presets` entries.

Session state:

- `sdk-dev.md` is unwedgeed and currently executing an active Codex turn in pane `%22`; I did not reset, force disk, or disturb that live turn.
- `sdk-demo.md` is healthy and ready in pane `%34`.
- Grok Build's `sdk-pr.md` is healthy and ready in pane `%32`. The light-grey autosuggestion is now classified as ghost text rather than unsent operator input.
- No restart is required. IntelliJ dynamically upgraded the plugin; idle supervisor/controller recycling is automatic.

The integrated agent-doc fixes are installed and pushed to `main` at `94a0ef8952`. They include stale-focus rejection for `projection_effect_not_started`, separated in-flight timeout handling, native-save convergence for retained captures, the Grok ghost-suggestion classifier, route snapshot/native-save ordering, and a visible bounded error cause in IDEA notifications. The real-time queue projection implementation was already present in the integrated tree: claims and refreshes project `🚧` state synchronously through editor/CRDT authority, report-complete retains it through integration, and release/strike clears it.

Verification: the authoritative integrated `make check` passed all 10,980 tests; the route regression passed 100/100 stress iterations; focused JetBrains/plugin suites passed; `make install` completed for the CLI/library, IntelliJ plugin `0.2.687` (plus split packages `0.2.688`/`0.2.689`), and VS Code extension `0.2.83`. The new `main` CI run is currently in progress, so local proof—not the earlier unrelated red run—is the completion evidence.

Changed paths include the agent-doc Rust controller/session/queue paths, JetBrains integration and tests, specifications/runbooks, generated harness mirrors, and the root `.editorconfig`. Confidence is high. No operator escalation remains.

### Re: Subagent completed: `subagent:queue_priority_subagents` returned work for queue item “By default, `agent:queue` should have `priority subagents`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=3cd646dad54c4f5b03d10e065b9a55bdc011ad723f87bb01c107069dd8e46b13 -->

### Re: Subagent completed: `subagent:todo_priority` returned work for queue item “By default, `agent:todo` should have `priority`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=9b189fadc17d173c6ec5d2aff4bc4d0901914bc2c8302063c7a4bfe68409a8a0 -->

### Re: Subagent completed: `subagent:backlog_priority_queue` returned work for queue item “By default, `agent:backlog` should have `priority queue`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7515dd89bea5d8b455071766e20b8d547d7a03eba3d85dc40be0caa3b4a05469 -->

### Re: Subagent completed: `subagent:todo_component_order` returned work for queue item “By default, make the `agent:todo` below the `agent:queue` and above the `agent:backlog`.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=4a036f3569ee0b435f763aa350eb61e5abd82c1db12a33c74a79a878d2211fec -->

### Re: Subagent completed: `subagent:todo_context_links` returned work for queue item “todo items are not linking to the review or backlog item in monsterrodholders.md. There are todo items without any context. "Decide on ...", but "..." has no context so operator does not know what the decision is.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=dc4afd4558efb52bd9fa80591f9a2eaddcaf52a508fa69c37c5297d032ed8ec2 -->
> **Queue prompt:** By default, make the `agent:todo` below the `agent:queue` and above the `agent:backlog`.

> **Queue prompt:** By default, `agent:queue` should have `priority subagents`.

> **Queue prompt:** By default, `agent:todo` should have `priority`.

> **Queue prompt:** By default, `agent:backlog` should have `priority queue`.

> **Queue prompt:** todo items are not linking to the review or backlog item in monsterrodholders.md. There are todo items without any context. "Decide on ...", but "..." has no context so operator does not know what the decision is.

> **Queue prompt:** Run Agent Doc on sdk-dev.md: agent-doc command failed (exit 1):

> **Queue prompt:** Error: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md has no agent_doc_session

### Re: contextual TODO links, defaults, and SDK recovery — gpt-5 · 2026-10-17T19:05-04:00

Completed and landed on `main`.

New documents now place `agent:todo` directly below `agent:queue` and above `agent:backlog`. Their default attributes are `agent:queue priority subagents`, `agent:todo priority`, and `agent:backlog priority queue`. The landed commits are `32095508c1`, `1efc4e9849`, `cf014114d6`, and `1e26878654`.

TODO rendering now resolves a cited review/backlog id and includes its source context, so prompts such as “Decide on …” are no longer detached from the decision they reference. That fix landed as `35b0a8023f`.

The two SDK routing heads are also resolved: `sdk-dev.md` has a valid session, space-indented frontmatter, and an active Codex turn in pane `%22`. The route/session recovery, visible error-cause, native-save ordering, and projection fixes are installed from `main` at `94a0ef8952`.

Changed paths cover document scaffolding, TODO parsing/rendering, sync and extraction scaffolds, CLI integration, specifications, runbooks, generated harness mirrors, routing/session handling, and regressions. Verification passed: focused component/TODO tests, workspace build, authoritative `make check` (10,952 passed, 172 skipped for the TODO-link branch), `make tmux-ci`, and `git diff --check`. The combined installed `main` batch passed all 10,980 tests. Confidence is high; no escalation remains.

### Re: Subagent completed: `subagent:all_md_inactive_pane` returned work for queue item “Newly claimed all.md had an Idea prompt error: [route] pane-not-visible: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev/all.md runs in pane %29 of inactive tmux window 'stash' (@6) in attached session '0'; its harness output is off-screen. Return editor focus to this document or show it with: tmux select-window -t @6 \; select-pane -t '%29'” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=5aff9edde2cc8100c2df0f6339c765da30d6e795074e3bde7c13cf548de4fcb0 -->
> **Queue prompt:** Newly claimed all.md had an Idea prompt error: [route] pane-not-visible: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev/all.md runs in pane %29 of inactive tmux window 'stash' (@6) in attached session '0'; its harness output is off-screen. Return editor focus to this document or show it with: tmux select-window -t @6 \; select-pane -t '%29'

### Re: retain cross-root route visibility through dispatch — gpt-5 · 2026-10-17T19:11-04:00

Resolved on `main` by `dea6e24257`. A cross-root editor route previously released the main-window route-layout lease before the nested controller dispatched, allowing passive IDEA focus churn to restash `all.md`. The owner now retains that lease through dispatch and releases it by bounded RPC, with lazy expiry if the client disappears.

Changed paths: `agent-doc-controller-io/src/project_controller.rs`, its RPC module, routing specification, and feature reference. Verification on current `origin/main`: the targeted cross-root regression passed, and `cargo test -p agent-doc-controller-io` passed 775 tests with 0 failures and 10 ignored. Confidence is high; no escalation remains for this item.

A separate verification audit found that the TODO source-context fix is incomplete for legacy rows and implicit singly linked `[#id]` actions. I recorded that remaining migration/canonicalization work in the backlog rather than claiming it is finished.
<!-- agent:boundary:77bcbba7 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~The `agent:queue` is not being updated with the current runtime state. Currently this `agent:queue` does not have any in progress indicators. It looks like nothing is being tracked. Queue tracking should be real-time and show the current state of agent-doc's processing of the queue items.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~By default, make the `agent:todo` below the `agent:queue` and above the `agent:backlog`.~~
- ~~By default, `agent:queue` should have `priority subagents`.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~By default, `agent:todo` should have `priority`.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~By default, `agent:backlog` should have `priority queue`.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~todo items are not linking to the review or backlog item in monsterrodholders.md. There are todo items without any context. "Decide on ...", but "..." has no context so operator does not know what the decision is.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~Newly claimed all.md had an Idea prompt error: [route] pane-not-visible: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev/all.md runs in pane %29 of inactive tmux window 'stash' (@6) in attached session '0'; its harness output is off-screen. Return editor focus to this document or show it with: tmux select-window -t @6 \; select-pane -t '%29'~~
- There is currently 1 tmux pane due to an auto-sync issue. Let me know when you compete diagnostics so I can manually sync tmux.
- #gh-fix https://github.com/btakita/agent-doc/issues/284
- #gh-fix https://github.com/btakita/agent-doc/issues/285
- #gh-fix https://github.com/btakita/agent-doc/issues/286
- #gh-fix https://github.com/btakita/agent-doc/issues/287
- #gh-fix https://github.com/btakita/agent-doc/issues/288
- #gh-fix https://github.com/btakita/agent-doc/issues/289
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/290~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/291~~ — auto-struck: answered this cycle (#ftstrike)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/292~~ — auto-struck: answered this cycle (#ftstrike)
- do [#extractliveapply]
- do [#extractcompactselection]
- ~~Error: /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-dev.md has no agent_doc_session~~ — auto-struck: answered this cycle (#ftstrike)
- ~~Error: Sync failed: {"applied":false,"reason":"projection_effect_in_flight","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md"],"layout_generation":17,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}~~ — auto-struck: answered this cycle (#ftstrike)
- ~~The Run Agent Doc sdk-dev.md error was not visible. I see an option to copy the error message, but the actual cause of the error should be visible to the operator, not truncated. ![img_6.png](img_6.png)~~ — auto-struck: answered this cycle (#ftstrike)
- do [#todocontextmigration]
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [x] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [ ] [#8ph6] In the docs.md session: check off the Google Drive re-auth todo (--todo-done httpsdocsgoogle once backfilled, or --todo-done with its exact text)
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#extractliveapply] agent-doc extract: provide a first-class coordinated detach/freeze/apply/resume path for focused extraction when both source and target are attached to live CRDT editors. Current --apply correctly refuses cross-document CAS and tells the operator to close tabs, but JetBrains can immediately re-register those tabs, leaving no reliable CLI workflow; do not require GUI automation.
- [ ] [#extractcompactselection] agent-doc extract: keep prepared extraction selections executable after exchange compaction. Persist semantic selectors/selection manifests against archive locators, and support selectively transferring completed/done items; today a saved operation can lose its live exchange-node selectors after compaction and --ids does not select done items.
- [ ] [#todocontextmigration] Canonicalize visible source qualification for every singly linked agent:todo item (explicit and implicit), emit migration/audit findings for legacy rows, and migrate monsterrodholders.md through the supported agent-doc authority path.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
