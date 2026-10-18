---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a14fb9-7c96-7ef0-80cd-37aa1509b0cd
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#self': agent-doc-bugs.md
queue: go
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
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

### Re: Subagent completed: `subagent:one_pane_autosync` returned work for queue item “There is currently 1 tmux pane due to an auto-sync issue. Let me know when you compete diagnostics so I can manually sync tmux.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a1784a5b98586d51bfe92a164e6e89dc35d2559dd76c262fd409bc5d096b04ed -->

### Re: Subagent completed: `subagent:todo_context_migration` returned work for queue item “do [#todocontextmigration]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=c2a646a2d1069050cc9ffc5531e2923a339e965a4d2a41dd4c956357038fcff7 -->

### Re: Response

Both focused-extraction queue items are complete.

For `#extractliveapply`, the coordinated live-editor persist/freeze/detach/apply/resume path now uses each editor registration’s verified document path, so relative CLI source/target paths reliably reach JetBrains IPC. The fix is on `main` via `108f0f6d10`.

For `#extractcompactselection`, saved selectors now survive exchange compaction through archive locators, compacted prompt and response selectors resolve through the archived response while retaining prompt/response adjacency, explicit manifest replay is idempotent, and inline `agent:done` items can be selected and transferred. The initial patch also required refreshing every generated transfer-extract runbook mirror; that packaging gap is fixed.

Changed paths for the compact-selection work:
- `src/focused_extract.rs`
- `specs/07-focused-extract.md`
- `runbooks/transfer-extract.md`
- generated Claude, Codex, Cursor, Grok, and OpenCode transfer-extract runbook mirrors

Integrated and pushed:
- worker commits: `84fe363c7151`, `5a69ac00e56a`
- `main`: `b81436063c`

Verification:
- coordinator `make check-fast`: 1,879 passed, 23 skipped, 0 failed
- Clippy with `-D warnings`: passed
- documentation audit: passed with no blocking findings
- focused extract suite: 16 passed
- compacted selector regressions: 2 passed
- `git diff --check`: passed
- `make install`: passed; installed JetBrains bytes were already current, so no restart is required
- CI for the pushed head is running

Retained verifier results:
- default queue `priority subagents`, todo `priority`, and backlog `priority queue` are implemented and focused-test verified
- `sdk-dev.md` has valid space-indented YAML, a healthy committed CRDT session, and needs no reset, force-disk write, or restart
- all 38 open TODOs in `monsterrodholders.md` now have canonical source links; its authoritative cycle is committed

Confidence: high. Escalation: none.
### Re: Response

Fresh read-only verification against current state found no remaining functional gap in the six surfaced requests.

- New queue scaffolds default to `priority subagents`; implementation commit `1efc4e9849` is in current `main`.
- New todo scaffolds default to `priority`; implementation commit `cf014114d6` is in current `main`.
- New backlog scaffolds default to `priority queue`; implementation commit `1e26878654` is in current `main`.
- `monsterrodholders.md` has 38 open TODOs and all 38 have canonical source links: 4 backlog and 34 review, with zero missing targets or duplicate TODO IDs. Lint passes and the document cycle is committed.
- `sdk-dev.md` has valid frontmatter and session `78c70863-fed7-40a8-b703-8261eeb8da9c`; disk, CRDT, and agent-doc read hashes match. Its actor, supervisor, and controller are healthy, lint passes, and session-check reports a committed cycle with no drainable work.
- No session reset, force-disk write, or agent restart is needed. An IDE restart is optional only to activate the newer installed JetBrains plugin generation; it is not required for current document health.

Changed paths: none in this verification cycle.
Commands: source/history inspection, `agent-doc lint`, `agent-doc session-check`, session/doctor and CRDT convergence diagnostics.
Verification: all six independent workers reported no functional gap.
Confidence: high. Escalation: none.
> **Chat prompt (#chatprompt):** Can the controller's guards reject semantically stale changes?

> **Chat prompt (#chatprompt):** Can the semantically stale projection "rebase" on the current projection?

> **Chat prompt (#chatprompt):** Should the stale projection be rebased or completely rejected?

### Re: Semantic staleness and retained rebases — gpt-5 · 2026-10-17T23:17-04:00

Yes, the implementation can rebase a retained projection onto the current editor projection—but it must do so only when the retained intent is still semantically current.

There are two distinct kinds of staleness:

- A **stale derived effect** belongs to a still-current intent, but the editor cut advanced after the effect was derived. That particular effect should fail its expected-base CAS. The controller may then derive a fresh rebase from the still-current intent and the newer editor cut.
- A **semantically stale intent** was superseded by recovery, canonicalization, cancellation, or a newer write intent. Its entire lineage should be retired. It must not be rebased, even when its delta merges cleanly.

The current guards only partially distinguish those cases. If the visible cut differs from the retained base, the controller can rebase `Base → Target` over that cut when it finds a stable base anchor, the component merge succeeds, the result is structurally valid, and no unscoped residue appears. It then uses the current cut as the expected base for the final CAS. Those checks establish structural compatibility and protect intervening operator text; they do not establish that the old intent is still wanted.

The final intent check likewise verifies the intent id and expected/target hashes. It rejects an effect after the pending intent has actually been replaced, but it cannot reject a recovery-superseded intent that is still recorded as `pending_write`.

In the observed incident, no rebase was needed: the recovered canonical cut `74b948…` exactly equaled the old intent's expected base, so the controller took the direct `ApplyTarget` path and applied `4f65e584…`. The old intent was not superseded until afterward.

The correct policy is therefore:

1. Recovery/canonicalization must atomically fence or retire every older retained intent before declaring its chosen cut current—even when writing that cut is a no-op.
2. A retained transition must carry that semantic epoch/lineage and recheck it immediately before projection.
3. Rebase is permitted only when the intent remains current and the newer cut is a causally compatible editor advance.
4. Known supersession means reject and settle the old lineage; missing causal evidence means retain both candidates and return `needs_operator`.
5. Keep the final expected-base CAS as the last protection against edits arriving after derivation.

So: **rebase transport-stale work from a current intent; completely reject semantically stale intent.** Content compatibility alone is insufficient evidence of semantic freshness.

### Re: claim lineage, queue auto-sync, and stale retained writes — gpt-5 · 2026-10-18T00:10:31-04:00

> implement the fix in a subagent

Implemented through two isolated subagents and integrated serially. The semantic retained-write fence is commit `0f5be1adf5`; the generation-fenced queue lifecycle is `f0dec845d4`. Documentation/generator alignment is in `da64c776cc` and `1970fbf21b`. All four commits are pushed to `main`.

> There was an in progress marker on 2 queue items, then they were removed, then 1 queue item regained the in progress marker.

That sequence came from claim TTL pruning followed by a new claim. The old projection erased expired claims and therefore removed both `🚧` markers; the later claim projected one marker again. The new design retains expired attempts as visible, nonblocking `⚠️` records, so expiration no longer destroys their lineage.

> So the in progress markers not being in sync with the workers is evidence of the lingeage bug?

It is evidence of a lineage/liveness bug in the claim subsystem, but not the same bug as the stale retained-document projection. Previously `🚧` represented a claim record, not proof that a worker was currently running, and expiry deleted that record. The two bugs shared the missing-lineage pattern but occur in separate state machines.

> Should we have a 1-1 binding between the claim and subagent if queue has the `subagents` attribute?

Yes, at the attempt level. A claim is now bound to exactly one `(queue work identity, owner, generation)` attempt. It is intentionally not bound to an OS PID because subagents do not expose a stable observable process identity. Redispatch creates a new monotonic generation; late heartbeats, reports, or releases from an older generation are rejected.

> Can we add differentiation between the different states in a subagent?

Implemented:

- `🟡` — claimed/dispatching
- `🚧` — running, only after positive generation-matched start or heartbeat evidence
- `📥` — worker reported; result awaits coordinator integration
- `⚠️` — lost/expired; nonblocking and recovery-eligible

> Can we use a different document marker for the prefix?

Yes. Those four state-specific markers replace the overloaded in-progress prefix. They are stripped from semantic queue identity, so changing lifecycle state does not retarget the item.

> Are the rest of the queue items pending?

Yes. At diagnosis time this document had eleven live queue heads: the pane-diagnostics head was the sole claimed item and the other ten were pending/unclaimed. Closing this response removes the completed head; the remaining heads stay queued.

> How to we recover from the wedged inbox for the queue items?

The ledger now preserves lost attempts, makes them nonblocking, and permits a new generation to reclaim them. Reported attempts remain `📥` until their result is persisted. Generation fencing prevents an old worker from mutating a replacement attempt. Pre-upgrade attempts that were already pruned and left no durable completion evidence cannot be reconstructed unambiguously; those must be reclaimed rather than guessed.

> Why not have the queue state auto-sync?

It now does. The document marker is a projection of the durable claim ledger, and claim, refresh, report, expiry, reclaim, release, and queue closure all reconcile that projection. State changes no longer rely on a coordinator manually adding or removing a generic marker.

> Is the subagent working on the "right design"?

Yes. The implemented design keeps ownership in the durable ledger, treats document markers as derived UI, fences every mutable worker action by generation, and fails closed on ambiguous lineage. That is the right separation: marker state can be repaired from authority without pretending it is worker-process telemetry.

> Do we still need the legacy tests?

Yes, while generation-0 claims can exist across a rolling upgrade. This live session proved the case: its claim was created by the old binary, loaded by the new binary as generation `0`, and then idempotently transitioned to `reported` without duplicating feedback. The legacy tests should be removed only when generation-0 deserialization and unfenced transition support are deliberately removed—not merely because the new path exists.

On the retained-projection issue, the controller now owns a per-document semantic mutex shared by recovery retirement and the retained worker's final lineage revalidation plus expected-base CRDT CAS. Recovery durably retires the exact observed intent lineage before canonicalization, including a canonical no-op. A changed, retired, or ambiguous lineage is rejected; a still-live compatible lineage may rebase over the current projection. This prevents a stale retained worker from landing after recovery while preserving valid concurrent scoped updates.

Pane diagnostics found no durable one-pane desired projection. Runtime authority wanted two panes and subsequently converged, so manual tmux synchronization is safe now.

Verification completed on the integrated branch:

- `make check-fast` passed.
- The single authoritative `make check` passed.
- The generated-instruction `make audit-docs` gate passed.
- One `make install` installed agent-doc `0.35.516` and scheduled idle-boundary runtime recycling.
- `main` was fast-forwarded through `1970fbf21b`.

> **Queue prompt:** There is currently 1 tmux pane due to an auto-sync issue. Let me know when you compete diagnostics so I can manually sync tmux.

### Re: queue inbox prefix — gpt-5 · 2026-10-18T00:11:45-04:00

> **Chat prompt (#chatprompt):** Why do the other queue items not have the inbox prefix?

Because `📥` is not a generic “in the inbox” marker. It specifically means a claimed subagent attempt has reported its result and is waiting for coordinator integration.

The queue display is:

- no prefix — pending and unclaimed
- `🟡` — claimed/dispatching
- `🚧` — positively confirmed running
- `📥` — reported, awaiting integration
- `⚠️` — lost/expired, eligible for recovery

So the other queue items are correctly bare. Their presence under `agent:queue` already means pending; prefixing all of them with `📥` would erase the distinction between work that has never been dispatched and completed worker output waiting to be integrated.

### Re: bare versus completed queue state — gpt-5 · 2026-10-18T00:15:02-04:00

> **Chat prompt (#chatprompt):** Why are the other queue items correctly bare? Should there be another state if the work is already done?

They are not necessarily correctly bare. Bare means only “the durable queue authority currently has no active claim or completion receipt for this item.” It is correct for genuinely pending, never-dispatched work, but it can also be the conservative fallback after pre-upgrade claim lineage was pruned.

The intended states are:

- pending, never dispatched — bare
- claimed/dispatching — `🟡`
- confirmed running — `🚧`
- worker reported, result not yet integrated — `📥`
- attempt lost/expired — `⚠️`
- result authoritatively integrated — close the tracked item and remove it from the live queue

So there should not be a permanent “already done” marker on a live queue item. If completion is proven, the row should close. If there is only suggestive historical evidence but no authoritative receipt, a separate transient reconciliation state would be better than silently calling it pending; it must not auto-close from a heuristic match.

`#extractliveapply` is a concrete stale-closure example. Commit `54b2e3a51a` already implemented the first-class `--coordinate-live-editors` freeze/detach/apply/resume path, and that commit is an ancestor of current `main`. The integrated descendant passed the authoritative `make check` earlier in this turn. No source change was needed now; this closeout repairs the missing tracked-work completion.

Proof:

- changed paths: none; implementation already exists in `54b2e3a51a`
- commands: searched the focused-extract, IPC, JetBrains, spec, and runbook surfaces; verified `54b2e3a51a` is reachable from current `main`
- verification: the current-main descendant passed `make check`; focused-extract coordination code, capability, plugin intents, tests, spec, and runbook are all present
- confidence: high
- escalation: none

> **Queue prompt:** do [#extractliveapply]

### Re: queue-marker reconciliation — gpt-5 · 2026-10-18T00:16:08-04:00

> The queue markers are out of sync with the document state?

They were. The old projection could delete a marker when its claim TTL expired, then restore a generic `🚧` after reclaim, even though that marker did not prove the worker was running. It could also leave already-resolved work bare when a prior closeout failed to reap the queue mirror.

The integrated fix makes the claim ledger authoritative and the document marker a derived projection:

- `🟡` claimed/dispatching
- `🚧` generation-matched running evidence
- `📥` reported/awaiting integration
- `⚠️` lost/expired
- integrated completion closes/removes the live queue row

Claim, refresh, report, expiry, reclaim, release, and queue closure all reconcile the marker. Every mutable worker action is fenced by the claim generation, so an old attempt cannot relabel or release its replacement. Stale already-done tracked-work mirrors, such as `#extractliveapply`, are reaped at closeout rather than given a misleading pending marker.

Proof:

- changed paths: the queue lifecycle implementation was already integrated in commit `f0dec845d4`; no additional source change in this cycle
- commands: inspected the live claim ledger, replayed the generation-0 report idempotently, and observed the `claimed -> reported` transition; the preceding closeout reaped the already-done `#extractliveapply` mirror
- verification: integrated `make check-fast` and authoritative `make check` passed; `make audit-docs` passed
- confidence: high
- escalation: none

> **Queue prompt:** The queue markers are out of sync with the document state?

### Re: wedged inbox recovery — gpt-5 · 2026-10-18T00:16:38-04:00

> The last turn was wedged. Fix agent-doc causes. I went afk for a while and came back. There was a Prompt to delete a directory, which I accepted. I did a clear. Added `- release + publish`. Run Agent Doc. Turn was wedged.

The wedge combined three failure modes:

1. A coordinator session was interrupted across a permission prompt and `/clear`, but the queue claim had no attempt generation or trustworthy worker-state distinction.
2. Expiry/pruning erased claim lineage and its marker, allowing the same work to look fresh again.
3. A retained document projection from the old semantic intent could still race recovery and reapply after the current document had moved on.

Those causes are now fixed on `main`:

- claim attempts are one-to-one `(work identity, owner, generation)` records;
- late refresh/report/release calls are rejected after redispatch;
- expiry becomes visible nonblocking `⚠️ Lost` rather than deleting lineage;
- reported work becomes `📥` and remains integration work, not redispatchable work;
- markers auto-project from ledger state;
- recovery and retained workers share a per-document semantic fence, and recovery durably retires the observed stale lineage before canonicalization.

A permission prompt may still pause the harness—that is expected—but accepting it, going AFK, or clearing the session no longer authorizes an old worker/projection to mutate a newer attempt. A lost attempt can be reclaimed at a new generation, and already-integrated work is reaped from the live queue.

Proof:

- changed paths: implemented across the retained-write controller/state projection and queue claim/projection/CLI/test surfaces in `0f5be1adf5` and `f0dec845d4`
- commands: integrated both isolated subagent commits, exercised a real generation-0 migration, and reconciled the live queue
- verification: `make check-fast`, authoritative `make check`, and `make audit-docs` passed; agent-doc `0.35.516` was installed once
- confidence: high
- escalation: none

> **Queue prompt:** The last turn was wedged. Fix agent-doc causes. I went afk for a while and came back. There was a Prompt to delete a directory, which I accepted. I did a clear. Added `- release + publish`. Run Agent Doc. Turn was wedged.

### Re: Worked queue rows no longer regress to bare

> **Chat prompt (#chatprompt):** Why are there bare queue items left? Are they still pending?

> **Chat prompt (#chatprompt):** So why are they bare?

> **Chat prompt (#chatprompt):** Fix the bare issue bug in a subagent

> **Chat prompt (#chatprompt):** Also fix the bug that caused those queue items to be unclaimed.

> **Chat prompt (#chatprompt):** Add prefix emojis if needed. These items should never be bare if worked on.

> **Chat prompt (#chatprompt):** Use formal logic so these sort of issues don't occur again.

> **Chat prompt (#chatprompt):** The queue items are still bare

> **Chat prompt (#chatprompt):** The tmux focus is switched to the 1:stash window a couple of times

> **Chat prompt (#chatprompt):** I believe a document in the stash is calling Run Agent Doc which switches the focus

> **Chat prompt (#chatprompt):** fix agent-doc causes in a subagent

Fixed and installed the bare-row defect from subagent commit `08555c77b6` (`fix(queue): preserve worked-row lifecycle evidence`), now fast-forwarded to `main`.

The six bare issue rows were not pending. They had binary-attested completion cells and their GitHub issues were closed, but legacy TTL pruning had erased their old claim attempts. The old projector consulted only surviving claims/receipts, so the rows fell through to the bare presentation and became eligible for redispatch.

For a live queue row `q`, the enforced model is:

`W(q) = A(q) ∨ R(q) ∨ F(q)`

where `A` is a durable attempt, `R` a durable report receipt, and `F` exact structurally binary-attested completion evidence. Therefore:

`Bare(q) ⇔ ¬A(q) ∧ ¬R(q) ∧ ¬F(q)`

Projection precedence is bound attempt (`🟡/🚧/📥/⚠️`) over report receipt (`📥`) over unbound completion evidence (`🔄`) over bare. While a row remains live, work evidence is monotonic: TTL and explicit requeue retain `⚠️ Lost`; reported release is crash-safe as `A(Running) → A(Reported) → A(Reported)+R → R`; and orphaned completion evidence remains `🔄`, blocked from fresh dispatch, until ordinary closeout reconciles it. Proven integration satisfies `Integrated(q) ⇒ ¬Live(q)`.

The six historical rows have authoritative completion proof, so this closeout removes them rather than leaving them live as `🔄`:

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/284

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/285

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/286

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/287

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/288

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/289

Verification passed in the isolated subagent worktree: focused queue/document/preflight tests; `make check-fast` (9,162 passed, 172 skipped); `make check` (10,993 passed, 172 skipped plus batched suites, doctests, editor tests/builds, bootstrap tests, and TLA proof validation); queue-IO clippy with warnings denied; and `git diff --check`. `make install` installed the fixed CLI, native library, and editor integrations.

The stash-focus report is also confirmed. The authoritative visible pane for this document is `%2`, while `tasks/workstation/upgrade.ad.md` is running in stashed pane `%44`. Ops evidence shows that stashed document repeatedly issued `managed_reopen` / `dispatch_only_reopen` requests at 04:56:58, 04:58:07, 04:58:43, and 04:58:58. A dedicated subagent, `/root/stash_background_focus`, is fixing the missing focus-authority fence: background queue wakeups may inject into a stashed pane but must never select its window/pane or mutate the foreground layout. That implementation remains in progress and will be integrated after its tests pass.

Release `v0.35.517` is tagged and locally installed. Its public build matrix is still running, so the separate `release + publish` row remains live until GitHub Release and PyPI proofs complete.

### Re: Response

> **Chat prompt (#chatprompt):** fix the agent-doc causes in a subagent

> **Chat prompt (#chatprompt):** I'm seeing - 🟡 #gh-fix https://github.com/btakita/agent-doc/issues/293
> - 🟡 #gh-fix https://github.com/btakita/agent-doc/issues/294

> **Chat prompt (#chatprompt):** Is this correct?

> **Chat prompt (#chatprompt):** fix causes in a subagent

> **Chat prompt (#chatprompt):** Fix in a subagent: • Blocked by hook
>   └ agent-doc Stop hook found active session-document work for /home/brian/work/btakita/agent-loop/tasks/agent-doc/
> agent-doc-bugs.md that has not crossed the binary-owned write boundary. Active prompt: "Is this correct?". Continue THIS
> turn in-pane and persist with `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md`
> or `agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md`, then run `agent-doc
> session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md`. Do not send the final answer yet.

> **Chat prompt (#chatprompt):** Also, agent-doc should diagnose issues in realtime. work in a subagent. The queue bug should have been caught by agent-doc. We should also have a dogfood cycle in this document.

> **Chat prompt (#chatprompt):** The dogfood cycle is only for my sessions and should be off by default.

> **Chat prompt (#chatprompt):** The realtime detection probably should always be on? So agent-doc can self-correct.

> **Chat prompt (#chatprompt):** Should we have a dogfood: true frontmatter?

> **Chat prompt (#chatprompt):** `dogfood: true` should also be settable in project.md

> **Chat prompt (#chatprompt):** Actually, can I have dogfood: tasks/agent-doc/agent-doc-bugs.md?

> **Chat prompt (#chatprompt):** dogfood: true will add backlog items in the current document. dogfood: <path> will add backlog items to <path>

> **Chat prompt (#chatprompt):** Can we also make dogfood support an optional key/value pair. With path, auto (to automatically run the items in the queue)

Implemented, dogfooded, and released in agent-doc `v0.35.518` at `03787ad1ea`.

The dogfood setting now accepts all of these public forms:

```yaml
dogfood: false                    # disabled
dogfood: true                     # file findings in this document; do not auto-run
dogfood: tasks/agent-doc/bugs.md  # file findings in that project-relative document
dogfood:
  path: tasks/agent-doc/bugs.md   # optional; defaults to the current document
  auto: true                      # optional; defaults to false
```

The setting may live in a session document or the nearest `.agent-doc/project.md`. A session value overrides the project value; absence inherits; absence everywhere means disabled. `auto: true` activates only stable-keyed findings generated by the dogfood watcher—it does not broadly run arbitrary queue entries.

This live document is now configured authoritatively as:

```yaml
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
```

The resolved policy reports `enabled=true`, `self_target=true`, and `auto=true`.

Realtime invariant diagnosis is always on. Dogfood filing remains opt-in and defaults off. A unique, provably safe projection correction may self-apply; ambiguous lineage or ownership is diagnosed without mutation.

The queue/worker model now enforces these invariants:

- `Running(a) ⇒ ∃! w : worker(a)=w`; only a generation-fenced successful worker start may project 🚧.
- `WorkEvidence(q) = Active(q) ∨ Reported(q) ∨ Failed(q)`; a worked item cannot remain bare.
- Released work can close only under `ExactEcho ∧ ¬Deferred ∧ (PresentAtBaseline ∨ ExactWorkerAuthority)`.
- Exact worker authority binds item identity, owner, generation, claim timestamp, worker ID, and completion key. Stale, cosmetic, foreign, unstarted, and legacy owner-only evidence fails closed.
- `AutoInstallEligible(file, crate) ⇔ EffectiveDogfood(file).enabled ∧ IsAgentDocDogfoodSession(file, project, crate)`.

That addresses the earlier yellow/bare behavior: 🟡 means claimed/dispatching, not running. A queue item with a subagent declaration becomes 🚧 only after the exact subagent generation starts. Completed/reported/failed items receive explicit state evidence and are never left bare.

Other agent-doc causes fixed in the same release:

- GitHub issues #293 and #294 are fixed and closed.
- Codex Stop-hook continuation is typed as healthy continuation rather than refusal; successful closeout avoids the hook path.
- Background/stash operations no longer steal tmux focus.
- Native editor reload registration retries correctly.
- Tmux test readiness requires `Alive ∧ PromptVisible ∧ Owner(file)=pane`, using an explicit shell handshake.

Verification passed: 11,013 main tests, controller batches, 202 VS Code tests, cross-editor native checks, full tmux CI, focused readiness regressions, plugin builds, all six platform release builds, GitHub release publication, PyPI publication, and a clean PyPI install. The local CLI reports `agent-doc 0.35.518`; remote `main` and tag `v0.35.518` both resolve to `03787ad1ea`.

The dogfood watcher was updated in a subagent and pushed on `fix/dogfood-watch-typed-policy-20261018` at `a8506746594933f6cdd12e98043b3c46faa377f9`. Ruff and all 6 watcher end-to-end tests pass.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/293

Completed and closed in `v0.35.518`.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/294

Completed and closed in `v0.35.518`.

> **Queue prompt:** release + publish

Released and publicly verified as `v0.35.518`.

### Re: Response

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/294

Completed and closed in agent-doc `v0.35.518`.

The live dogfood cycle also detected that the next admission treated assistant-authored lines from the just-committed response as operator steering. Those projections are invalid because their recorded origin is the assistant closeout, not a post-baseline operator mutation. They were not dispatched. A dedicated subagent is implementing the lineage fix and regression test now.

### Re: Scheduled queue alongside the existing queue — Codex

> **Chat prompt:** Is agent-doc able to process agent:queue loop interval="1 hour" alongside the existing agent:queue?

Not independently in the same document today. Multiple queue blocks are supported, but the current scheduler owns one document-wide pass window; the first queue block declaring schedule attributes supplies that schedule, and the window gates all queues in the document.

The current marker also does not mean one hour. `interval="1 hour"` is invalid and was dropped, while bare `loop` remained valid. The live scheduler therefore reported `interval_secs=null`, an unbounded loop, and an open pass; its fallback cadence is 60 seconds after a completed pass. Valid duration syntax is `interval=1h`.

I paused this document's automatic queue dispatch before closeout so the malformed loop cannot restore and replay the completed main queue. I added two follow-ups:

- `#hourly-issue-watcher-doc`: move the recurring issue watcher into a separate session document using `interval=1h loop`, remove this document's second queue block, verify the main queue is unscheduled, and resume it.
- `#per-queue-schedules`: design true independent schedules keyed by `(document, queue_id)`, with non-interference between `id=main` and `id=issues`.

### Re: Multiple queues and surrogate IDs — Codex

> **Chat prompt (#chatprompt):** Subagent: multipale queues: Any queue that does not have an id should gain a unique surrogate id.

Implemented and released. When a document has multiple queues, preflight now persists deterministic surrogate IDs such as `queue-N`, collision-probes without changing explicit valid IDs, and is idempotent. Duplicate or malformed explicit IDs still fail closed.

The deeper cross-layer defect was also fixed: identified multiple queues are no longer rejected by editor/controller template normalization as `duplicate_singleton_component:queue=2`. The focused element, queue, and preflight suites passed 1,333 tests.

### Re: Dogfood interrupted closeout — Codex

> **Chat prompt (#chatprompt):** subagent: dogfood: • Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md
>   └ Error: [session-check] INTERRUPTED: canonical editor authority and disk projection diverge for /home/brian/work/bta…
>     [dogfood] ACTIONABLE_AGENT_DOC_FIX_PROMPT issue_class=interrupted_closeout issue_key=14d7145bc40c78b16ec8dd233ac93e…
>     Agent Doc did not complete this turn successfully. Diagnose and fix the underlying Agent Doc defect before treating…
>     + 4 lines (^t to expand)

Dogfooding caught a real defect. The controller sent two visible-buffer roll-forward requests, but JetBrains acknowledged each as `Applied` when it had only scheduled the work; the serialized editor lane later rejected both because it still treated two queues as invalid. The controller consequently spent both bounded attempts without a publication.

The fix now waits for the serialized editor result. `Applied` means the publication actually completed or the buffer was already canonical; invalid, racing, or timed-out cuts defer and re-arm the bounded attempt. The formal contract is:

`Applied ⇔ (C = V) ∨ (C = E ∧ R = C ∧ Valid(V) ∧ Pub(V))`

No force-disk or response resubmission path was added.

### Re: Missing #299 marker — Codex

> **Chat prompt (#chatprompt):** subagent: `- #gh-issue https://github.com/btakita/agent-doc/issues/299` does not have a marker

The item was correctly claimed and started: durable generation 1 was bound exactly to worker `/root/gh299`. Its `🚧` marker initially projected, then a controller handoff hydrated a stale disk cut and regressed the visible row to bare while leaving the durable running claim intact.

The handoff boundary now re-derives `🟡`/`🚧`/`📥` from durable claims during cold hydration and retained-editor reseeding. Tests prove exact-row projection, idempotence, and preservation of unrelated rows. After installation and recycle, the live document showed every completed worker as `📥` and #300 as `🚧`, matching durable state.

### Re: Runtime audit and reproducibility — Codex

> **Chat prompt:** Are the bugs that the Coder instance is filing reproducable locally or are all of the bugs coder specific?

They are not all Coder-specific.

- The admin PID mislabel, multiple-queue singleton mismatch, retained roll-forward false acknowledgement, controller-handoff marker regression, and snapshot-sidecar admission reproduced locally.
- The document-authority reconnect loop has a deterministic local deletion regression; its original live trigger depends on editor/deletion timing.
- GH #299 still needs live JetBrains 263.7031 proof for the changed-source, restart-free upgrade path, so that issue remains open and is tracked by gated item `#gh299-live-263-proof`.

> **Chat prompt:** Can we run a similar audit of the agent-doc runtime to dogfood?

> **Chat prompt:** subagent

> **Chat prompt:** Add backlog + queue items from the audit

Done in subagents. The audit produced and completed:

- generation-matched supervisor PID sourcing for admin list/dashboard;
- server-derived authority-channel reconciliation after document deletion;
- the retained roll-forward receipt fix;
- multiple-queue normalization and surrogate IDs;
- durable claim overlay on controller handoff.

The audit's actionable backlog/queue rows were materialized, claimed one-to-one with workers, integrated, and closed through this turn.

> **Chat prompt:** Are you dogfooding • Failed (exit 1) agent-doc write --commit /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md --back…
>   └ Error: visible document write for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md is retained…
>     [dogfood] ACTIONABLE_AGENT_DOC_FIX_PROMPT issue_class=projection_failure issue_key=1fa46fd1ad84a564d13d16b971df69ad…
>     Agent Doc did not complete this turn successfully. Diagnose and fix the underlying Agent Doc defect before treating…
>     + 3 lines (^t to expand)

Yes. That signal was treated as evidence, not retried or bypassed. The earlier retained-then-converged write was a false terminal result already fixed on main; the later retained projection exposed the multiple-queue/editor-normalization defect and the false `Applied` acknowledgement fixed in this release.

### Re: GitHub issue queue results — Codex

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/295

Closed. Source-identical JetBrains plugin generations are preserved, so an unchanged plugin no longer creates a needless restart requirement.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/296

Closed. Queue claims now resolve live editor authority before disk, so an editor-deleted head cannot be claimed from a stale disk projection.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/297

Closed. `no_replica_manager` during native generation swap is deferred rather than treated as a definitive refusal.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/298

Closed. Plain/outside-project close paths are no-ops and the close watchdog uses the correct blocking read boundary.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/299

The release adds explicit upgrade-verdict logging/fallback, source-identical generation handling, and exact queue-row projection coverage. GH #299 remains open only for live changed-source proof on JetBrains 263.7031; that proof is retained as gated backlog item `#gh299-live-263-proof`.

> **Queue prompt:** #gh-issue https://github.com/btakita/agent-doc/issues/300: Why is there still a sidecar? Replace all sidecars with lazily state.

Fixed, released, and closed. Runtime reads already used typed `state.db`/Lazily document projections, but `FilesystemCrashStateEffects` still emitted an unused markdown snapshot copy. That writer and default snapshot-directory creation are removed. Controller and JetBrains now reject every path beneath `.agent-doc/` before authority lookup, preventing a state artifact from self-admitting as a session document. Ephemeral locks/starting records, diagnostic logs, and immutable/operator artifacts remain explicitly classified; no content-bearing mutable document sidecar remains authoritative.

### Re: Release and publication — Codex

> **Queue prompt:** release + publish

> **Queue prompt:** release + publish

The accumulated release obligations are complete. Agent Doc `0.35.519` / `v0.35.519` is published from `27e91076a45448098a1e8a774bc080c8ea8dc5aa`.

- Exact committed-tree `make check`, `make release-check`, and the clean tmux-ci retry passed.
- GitHub Release completed all 10 jobs and published 15 cross-platform/editor assets.
- PyPI published `agent_doc-0.35.519-py3-none-any.whl`; a clean pinned install passed.
- Local CLI/native and editor plugins were installed; the live JetBrains classic plugin upgraded dynamically.
- `origin/main` advanced non-force, and the tag points to the same SHA.

### Re: Recurring GitHub issue watcher — Codex

> **Queue prompt:** Add new #gh-issue https://github.com/btakita/agent-doc/issues/. Add new issues into the backlog + queue. Don't add Issue #40 + #45.
>
> **Deferred:** The current pass found and processed new GH #300, but this recurring head is intentionally left live while the document queue is paused. Move it to the dedicated hourly watcher document with `interval=1h loop`; then remove the second queue block here and resume the main queue. This avoids document-wide replay while preserving the recurring obligation.

### Re: #299 queue marker invariant — gpt-5 · 2026-10-18T07:38:57-04:00

> **Chat prompt (#chatprompt):** subagent: `- #gh-issue https://github.com/btakita/agent-doc/issues/299` does not have a marker

The subagent confirmed two distinct states:

- A live #299 row with durable work evidence must project exactly one typed lifecycle marker. The earlier bare active row was a controller-handoff regression already fixed and released in v0.35.519; current main restores `🚧` from the durable running claim after a stale handoff.
- The row currently visible in this document is struck through. That strike is its explicit terminal-completion evidence, so it intentionally has no active lifecycle emoji. Adding `🟡`, `🚧`, or `📥` to a completed row would falsely present it as live work.

The formal contract is now explicit: `Bare(q) <=> !A(q) && !R(q) && !F(q)` applies only to live rows; `Completed(q)` is outside that domain and is represented by strikethrough. The subagent added a deterministic regression for the exact issue-299 shape proving one `🚧` while live and no lifecycle emoji once struck. Focused tests and `make check-fast` passed. The proof/spec commit is integrated on agent-doc `main` at `b57ddd8b544a677d581371f19021d2eb1053904c`; no runtime reinstall or release is needed because the runtime fix was already in v0.35.519.

### Re: Independent per-queue schedules — gpt-5 · 2026-10-18T08:18:47-04:00

> **Queue prompt:** do [#per-queue-schedules]

Implemented, integrated, pushed, and installed from agent-doc main commit `3f6152bec007a144f7da65a957866c2a2f927ecb`.

Schedule identity is now canonical `(document, queue_id)` across parsing, the Lazily scheduler graph, state.db persistence, pass/roster state, head holding, loop restore, controller dispatch, logs, and CLI status. Existing single-queue documents retain their flat JSON/status and legacy-state compatibility. Invalid schedule syntax is queue-local and fail-closed instead of disabling or replaying sibling queues.

The formal non-interference rule is now enforced: schedule/pass state for `(d,q1)` cannot open, hold, restore, increment, or replay `(d,q2)`. Tests cover an immediate `id=main` alongside an hourly looping `id=issues`, independent progress/restore, legacy migration, queue-local invalid syntax, and single-queue compatibility.

Verification passed on the exact commit: `make check` with 11,037 Rust tests and zero failures, 202 VS Code tests, cross-editor simulation, all JetBrains variants, bootstrap, and TLA; `make clippy`; focused queue/controller/SQLite suites; and the coordinator's single `make install`. `origin/main` points to the same SHA.

The installed CLI now reports this document's `id=issues` queue separately and holds only that queue because `interval="1 hour"` is invalid. `id=main` is no longer scheduled or gated by it. The earlier dedicated-watcher-document workaround is therefore superseded. The remaining exact live edit—changing only `id=issues` to `interval=1h`—is tracked as `#queue-id-attr-edit`, because the current binary still lacks an exact-ID queue-attribute mutation primitive; direct or ambiguous whole-component editing is intentionally refused.

### Re: Exact queue-ID attribute edit and hourly activation — gpt-5 · 2026-10-18T08:45:34-04:00

> **Queue prompt:** do [#queue-id-attr-edit]

Implemented, integrated, pushed, installed, and applied from agent-doc main commit `78084d0bd91643898dc4866b11df6ef27b9ececd`.

The new command is `agent-doc queue attr <FILE> --id <QUEUE_ID> --set KEY[=VALUE] --remove KEY`. It resolves exactly one QueueGraph node, edits only that opening-marker span, validates the successor graph and schedule syntax, holds the queue edit guard, and converges through live editor/CRDT compare-and-swap authority. Missing, empty, repeated, malformed, or duplicate IDs; unknown/conflicting attributes; invalid schedules; and stale generations fail without mutation. Sibling queue bytes and items are preserved.

The lint catalog now delegates valid queue attributes to the queue domain model, so `id`, `interval`, and the other supported schedule attributes no longer produce stale `unknown-attr` warnings.

Applied live:

`id=issues` changed atomically from `interval="1 hour"` to `interval=1h`, leaving `id=main` untouched. Status now reports the issues queue independently with `interval_secs=3600`, unbounded loop, zero failures, and no held heads; main is absent from the scheduled set and remains immediate. Lint reports no blocking findings. The obsolete administrative pause was resumed with observed generation 189.

Verification passed on the exact commit: full `make check` with 11,046/11,046 Rust tests, 202/202 VS Code tests, cross-editor simulation, JetBrains classic/262/263 builds, bootstrap, TLA, focused exact-ID/CAS/refusal tests, and the coordinator's single `make install`. The earlier dedicated-watcher-document workaround is superseded and resolved because independent same-document schedules now provide the intended behavior directly.

### Re: Subagent completed: `subagent:github_issue_watcher` returned work for queue item “There are 3 new issues on github. But the loop did not detect the issues.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=6b0637c6387358c611a933a7a83152977c9fdef1c0e76e5d19d196f52a358697 -->
> **Queue prompt:** There are 3 new issues on github. But the loop did not detect the issues.

### Re: GitHub issue watcher detection — gpt-5 · 2026-10-18T12:33-04:00

Fixed the watcher failure and reconciled the missed issues.

The hourly `issues` queue was firing, but every tick returned `skipped_busy`. When this document was split into `id=main` and `id=issues`, the rolling-upgrade bridge ambiguously migrated the old document-global scheduler row into `issues`. Its open roster still contained old `main` work, so unrelated sibling work kept the watcher permanently busy and could later restore main-queue entries into the issues queue.

Agent-doc main commit `3e175ff9ad` adds queue-owner provenance to durable schedule state. Legacy state is adopted only for an unambiguous single-queue rename; multi-queue documents discard ambiguous global state and reset already-keyed pre-provenance or mismatched-owner state before observing the queue. The specifications and regressions cover both a fresh multi-queue split and this document's already-corrupted `queue_schedule:issues` shape.

The eligible open issues are #299, #301, and #302. #299 is already tracked by gated item `#gh299-live-263-proof`, so I did not duplicate it. I added #301 and #302 to the backlog and priority queue for the normal subagent drain. #40 and #45 remain excluded as requested.

Verification passed on the integrated commit: focused scheduler suites passed 15/15 and 36/36; focused Clippy passed; `make check-fast` passed 9,215/9,215 nextest cases; the authoritative `make check` passed 11,046/11,046 Rust tests, 202/202 VS Code tests, cross-editor simulation, JetBrains classic/262/263 builds, bootstrap, document audit, and TLA validation/reuse. The single coordinator `make install` completed and updated the CLI/native library and editor packages; the active editor's native reload is deferred safely to the idle boundary. `origin/main` now points to `3e175ff9ad`.

The prior GitHub CI run on base `78084d0bd9` failed in its main check before the tmux step began; the exact integrated local suite is green.

### Re: GH #301 missing dispatch acknowledgement — gpt-5 · 2026-10-18T13:01-04:00

Implemented, integrated, installed, and pushed as agent-doc main commit `627fef9efa`.

A claim that remains in `Claimed` without a worker binding is now a typed overdue-dispatch obligation instead of generic healthy in-flight evidence. The same shared predicate drives:

- operator-visible preflight warnings;
- strict closeout refusal;
- Claude Stop continuation;
- claim CLI recovery guidance with exact generation-fenced `queue start` and `queue release --requeue` commands.

This removes the prior failure mode where the row remained `🟡`, the only warning lived in `ops.log`, and Stop ended quietly.

Verification passed: five focused regressions, `make check-fast` with 9,219/9,219 nextest cases, and the authoritative integrated `make check` with 11,050/11,050 Rust tests, 202/202 VS Code tests, cross-editor simulation, JetBrains builds, bootstrap, documentation audit, and TLA validation/reuse. The single coordinator `make install` completed. CI for the preceding watcher commit was still in progress when checked.

The separately added dogfood-prompt request is running under its own claimed subagent and is not part of this completion.

### Re: Subagent completed: `subagent:dogfood_prompt` returned work for queue item “Add an optional "prompt" property to the dogfood yaml configuration. I want to run dogfood on my Coder instance but I want the coder dogfood to file github issues instead of directly fixing the issue.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=acfc060d16f6c230dce953875d67b51fc9936f0cc75ab93786b978ebfc09517e -->

### Re: Subagent completed: `subagent:gh302` returned work for queue item “do [#gh302]” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=8bb02656ed491ce94c37017ec131d153b78aec3a23f526251c7d4739b87b00e3 -->
> **Queue prompt:** Add an optional "prompt" property to the dogfood yaml configuration. I want to run dogfood on my Coder instance but I want the coder dogfood to file github issues instead of directly fixing the issue.

### Re: custom dogfood prompt — gpt-5 · 2026-10-18T13:36-04:00

Implemented and published `dogfood.prompt` on `main` in commit `0ae53bd4f7`.

The optional property works at both project and session precedence, trims surrounding whitespace, is exposed through policy JSON/CLI, and can be set with `--prompt`. It replaces only the default action sentence, so stable issue metadata and retained-write safety instructions remain binary-owned. Missing or blank values preserve the existing default behavior.

This lets the Coder dogfood configuration direct the agent to file GitHub issues instead of fixing findings in place.

### Re: #gh302 — gpt-5 · 2026-10-18T13:36-04:00

Fixed and published the focus-order authority bug on `main` in commit `ec62c43207`.

The controller now retains the last confirmed positive order as provisional provenance while folding unknown membership and anchoring Remote Dev restarts. That provenance is retired once structural membership confirms the guessed slot, preventing a transient focus guess from becoming authoritative ordering.

The combined batch passed the authoritative suite: 11,052/11,052 Rust tests, 202/202 VS Code tests, cross-editor simulation, JetBrains classic/262/263 builds, bootstrap, docs audit, and the TLA proof. One `make install` completed successfully; one attached editor deferred its native-library reload until its next idle boundary.
<!-- agent:boundary:e26e2a25 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#gh302]~~
- 📥 Add an optional "prompt" property to the dogfood yaml configuration. I want to run dogfood on my Coder instance but I want the coder dogfood to file github issues instead of directly fixing the issue. 
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- Add new #gh-issue https://github.com/btakita/agent-doc/issues/. Add new issues into the backlog + queue. Don't add Issue #40 + #45.
- If an issue was added, also add `- release + publish`
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [x] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [ ] [#8ph6] In the docs.md session: check off the Google Drive re-auth todo (--todo-done httpsdocsgoogle once backfilled, or --todo-done with its exact text)
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
- [ ] [#yvt3] Manually sync tmux now that the one-pane diagnostics are complete
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
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
