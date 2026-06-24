---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog + queue items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

<!-- agent:status patch=replace -->

done
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted content:
- Archived prior response topics and recovery notes from this long-running agent-doc session.
- Current cycle repaired an interleaved exchange produced after stale prompt routing, then answered and implemented the state-backbone follow-up.

### Re: response overwrite sidecar recovery - gpt-5

> **Queue prompt:**
>
> The response is still overwriting parts of the previous response.

The earlier recovery cycle repaired the live stale-sidecar state without changing `src/agent-doc` source. The focused overwrite/rebase regressions passed, and the latest remote CI run at that point was green.

Proof:
- Changed paths: session document/sidecar recovery only; `src/agent-doc` stayed clean.
- Verification: all three focused regressions passed. Latest `src/agent-doc` CI was green: run `28070395022`.
- Escalation: none.

### Re: #advr02 - gpt-5

> **Queue prompt:**
>
> do [#advr02]

The managed Codex proof restart fix was implemented in the prior cycle.

- `agent-doc-orchestration/src/start.rs`: added proof epochs so stale proof threads cannot satisfy a newer launch gate.
- `agent-doc-orchestration/src/start/run.rs`: centralized proof setup from `HarnessLaunchSpec` and restarts managed proof collection after harness-change restarts.
- `agent-doc-orchestration/src/startup_miss.rs`: proof scans reset on `agent_restart_performed`.
- `agent-doc-orchestration/src/route.rs`: added the post-restart proof regression.

Verification covered the focused proof regressions, `make check`, `make install`, and latest CI status. Commits were `d71bd627` in `src/agent-doc` and parent gitlink commit `2f517d881`.

### Re: state backbone - gpt-5

> **Queue prompt:**
>
> I want to have a backbone for all state. We should keep the Cycle State Machine. But for the rest of the state would any be a good match?

Yes. Keep the Cycle State Machine as the global lifecycle authority, but use an append-only typed event ledger plus deterministic projections as the durable backbone for the rest of the state. That gives agent-doc one replayable truth without forcing document state, queue state, transport state, supervisor state, proof state, and editor IPC state into one giant FSM.

The model I documented is:

- Cycle FSM: owns only turn lifecycle: preflight, plan, execute, finalize, commit, session-check, interruption, and recovery.
- Typed event ledger: records facts such as `PreflightStarted`, `BaselineSaved`, `QueueHeadSelected`, `ResponseCaptured`, `EditorAckObserved`, `IpcProofInsufficient`, `CommitObserved`, `AgentRestartPerformed`, and `CapabilityProofObserved`.
- Deterministic projections: derive document, queue, closeout, transport, supervisor, route, and proof state from events.
- Actor ownership: one owner mutates each live surface at a time: document writer, supervisor, route/dispatch actor, editor IPC bridge, and harness pane actor.
- Local FSMs: remain appropriate for closed subdomains such as transport connection, proof gate readiness, pane lifecycle, recycle lifecycle, and writeback lifecycle.

Model fit:

| Alternative | Fit | Where it belongs |
| :--- | :--- | :--- |
| FSM | Good locally, too rigid globally | Keep the Cycle State Machine and small local lifecycle machines. |
| Behavior Trees | Weak as durable state, useful as policy | Recovery or dispatch decision policy that reads projections and emits commands. |
| GOAP | Poor fit for the backbone | Too much planning machinery for correctness-critical writeback. |
| Coroutines | Useful for protocols, not durable state | Linear handshakes, waiting loops, and restart sequences, with progress persisted as events/checkpoints. |
| Event-driven | Strong fit when disciplined | Use typed events, append-only storage, idempotent reducers, causation IDs, owner epochs, and replay tests. |
| MPC | Not a fit | Agent-doc is discrete workflow convergence, not continuous control optimization. |

Implemented `#statebb1` as docs/spec work:

- Added `src/agent-doc/specs/13-state-backbone.md`.
- Linked it from `src/agent-doc/SPEC.md`, `docs/README.md`, `docs/SUMMARY.md`, and `docs/reference/flow-map.md`.
- Recorded it in `src/agent-doc/VERSIONS.md` for `0.34.44`.
- Committed `src/agent-doc` as `b3239b70 docs: define state backbone`.
- Committed the parent submodule pointer as `35659b5a0 chore: update agent-doc submodule`.

Proof:
- Changed paths: `src/agent-doc/specs/13-state-backbone.md`, `src/agent-doc/SPEC.md`, `src/agent-doc/docs/README.md`, `src/agent-doc/docs/SUMMARY.md`, `src/agent-doc/docs/reference/flow-map.md`, `src/agent-doc/VERSIONS.md`, parent `src/agent-doc` gitlink, and this session document.
- Commands: `tsift status`; `agent-doc preflight`; `agent-doc plan`; interrupted stale `agent-doc orchestrate`; `git -C src/agent-doc diff --cached --check`; `make check`; `gh run list --workflow CI --limit 1`; scoped git commits.
- Verification: `make check` passed in `src/agent-doc` with Clippy, 5053 nextest tests, doctests, and the wider closeout sim. Latest GitHub Actions `CI` run was `completed success` for run `28070395022` on `2026-06-24T02:11:52Z`.
- Push: not pushed; the parent repo already had unrelated ahead-of-origin commits.
- Escalation: none.
- Confidence: high for the documented state model and docs/spec integration.
<!-- agent:boundary:8c55a650 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
- ~~The response is still overwriting parts of the previous response.~~
- ~~:round_pushpin: do [#advr02]~~
---
I want to have a backbone for all state. We should keep the Cycle State Machine. But for the rest of the state would any be a good match?

| Alternative | Best Used For | Key Benefit | Key Limitation |
| :--- | :--- | :--- | :--- |
| **Finite State Machine (FSM)** | **Simple AI, Animations, UI Flow** | **High performance (O(1)); deterministic; easy to visualize for small N.** | **State Explosion:** Complexity grows exponentially ($N^2$ transitions) as states increase; rigid and hard to interrupt. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//medium.com/@prathameshburade07/finite-state-machine-in-game-ai-sharing-what-i-learn-001-1262dbd112e5&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/4R4hFITz_F_be0roUiWbTZKhsywr3fnLTMTkFL5HFow/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvOTZhYmQ1N2Q4/NDg4ZDcyODIyMDZi/MzFmOWNhNjE3Y2E4/Y2YzMThjNjljNDIx/ZjllZmNhYTcwODhl/YTcwNDEzYy9tZWRp/dW0uY29tLw&quot;,&quot;title&quot;&#58;&quot;Finite&#32;State&#32;Machine&#32;in&#32;Game&#32;AI&#32;—&#32;Sharing&#32;What&#32;I&#32;Learn&#32;001&quot;,&quot;snippet&quot;&#58;&quot;Friendly&#32;—&#32;Small&#32;footprint,&#32;predicatable&#32;memory&#32;usage.&#32;3.&#32;Deterministic&#32;—&#32;Same&#32;inputs&#32;always&#32;produce&#32;same&#32;outputs.&#32;4.&#32;Easy&#32;to&#32;serialize&#32;—&#32;Game&#32;saves,&#32;networking,&#32;replay&#32;systems.&#32;Disadvantages&#32;&#58;&#32;1.&#32;State&#32;Explosion&#32;—&#32;Complex&#32;behaviors&#32;need&#32;exponentially&#32;more&#32;states.&#32;2.&#32;Transition&#32;Complex…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//redefinegamedev.com/are-state-machines-still-relevant-for-ai/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/DcSTWzu6iWo3Jk9SwDuNJzqqJerVCMcFg0BF1UMbUz8/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvOWM5OTkxZGU4/ZWY2ZDQwODY2MTQx/MTY3NTkxMmEwNmQ4/NzBjNmE4NjI3NjFh/ZGE2YzNlM2YxM2M1/NDE0YzRlYi9yZWRl/ZmluZWdhbWVkZXYu/Y29tLw&quot;,&quot;title&quot;&#58;&quot;Are&#32;State&#32;Machines&#32;still&#32;Relevant&#32;for&#32;AI?&quot;,&quot;snippet&quot;&#58;&quot;…other&#32;game&#32;flow&#32;control&#32;implementations.&#92;nThis&#32;is&#32;why&#32;most&#32;game&#32;engines&#32;do&#32;have&#32;FSM&#32;for&#32;animations&#32;(&#32;Unity&#32;,&#32;Unreal&#32;,&#32;Godot&#32;).&#92;nPro&#32;tip&#58;&#32;you&#32;can&#32;repurpose&#32;most&#32;of&#32;these&#32;systems&#32;to&#32;create&#32;game&#32;AIs,&#32;instead&#32;of&#32;building&#32;your&#32;own&#32;Finite&#32;State&#32;Machin…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.mingukwon.com/posts/finite-state-machine-&amp;-behaviour-tree/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/Z-3iXYMWf6U0ZBzIg6AppjyYKHMTRGtJkRnVb7tr_14/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvMWYzMmFjNzFi/OTJhZjI5MjJmNzZl/MDNkZGJiYjc2YTRh/NWNhM2I2NzYzZTQ5/ZWMwNThlODNiMjMw/NWFiOTM3My93d3cu/bWluZ3Vrd29uLmNv/bS8&quot;,&quot;title&quot;&#58;&quot;Finite&#32;State&#32;Machine&#32;&amp;&#32;Behavior&#32;Tree&#32;in&#32;Robotics&quot;,&quot;snippet&quot;&#58;&quot;…swarm&#32;behavior.&#92;nFinite&#32;State&#32;Machines&#32;and&#32;Behavior&#32;Trees&#32;are&#32;both&#32;powerful&#32;control&#32;architectures&#32;used&#32;in&#32;robotics&#32;applications,&#32;with&#32;unique&#32;advantages&#32;and&#32;challenges.&#32;FSMs&#32;provide&#32;an&#32;intuitive&#32;structure&#32;and&#32;are&#32;easy&#32;to&#32;implement,&#32;but&#32;their&#32;complexity&#32;can&#32;grow&#32;exponentially&#32;as&#32;states&#32;and&#32;tran…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//peerdh.com/blogs/programming-insights/comparing-behavior-trees-with-finite-state-machines-for-ai-decision-making-23&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/BBGD3XQ3pk7qX2MjEHAnwUanLpgYWV5IKHmXjydChZc/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvN2RiMDQ5MjIw/OTcyN2MxOTg2NDkx/YzJjYTkwMjM4ODcz/ODkwYmM3MGVhOTdi/MWU0ZjIzNDhlMjVk/NDI0Mjc3NC9wZWVy/ZGguY29tLw&quot;,&quot;title&quot;&#58;&quot;Comparing&#32;Behavior&#32;Trees&#32;With&#32;Finite&#32;State&#32;Machines&#32;For&#32;Ai&#32;Decision&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…actions&#32;over&#32;others&#32;or&#32;executing&#32;multiple&#32;actions&#32;simultaneously.&#32;FSMs&#32;are&#32;more&#32;rigid,&#32;as&#32;they&#32;follow&#32;a&#32;strict&#32;state-transition&#32;model.&#32;In&#32;terms&#32;of&#32;performance,&#32;FSMs&#32;can&#32;be&#32;more&#32;efficient&#32;for&#32;simple&#32;behaviors&#32;due&#32;to&#32;their&#32;straightforward&#32;nature.&#32;However,&#32;as&#32;the&#32;complexi…&quot;&#125;&#93;"} |
| **Behavior Trees** | Game AI, Complex Behaviors | Priority-based execution; easy to add interruptions; modular. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//peerdh.com/blogs/programming-insights/comparing-behavior-trees-with-finite-state-machines-for-ai-decision-making-23&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/BBGD3XQ3pk7qX2MjEHAnwUanLpgYWV5IKHmXjydChZc/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvN2RiMDQ5MjIw/OTcyN2MxOTg2NDkx/YzJjYTkwMjM4ODcz/ODkwYmM3MGVhOTdi/MWU0ZjIzNDhlMjVk/NDI0Mjc3NC9wZWVy/ZGguY29tLw&quot;,&quot;title&quot;&#58;&quot;Comparing&#32;Behavior&#32;Trees&#32;With&#32;Finite&#32;State&#32;Machines&#32;For&#32;Ai&#32;Decision&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…date(False,&#32;False)&#32;&#32;#&#32;Changes&#32;state&#32;to&#32;Chasing&#92;n&#92;n&#92;nFSMs&#32;can&#32;become&#32;complex&#32;and&#32;hard&#32;to&#32;manage&#32;as&#32;the&#32;number&#32;of&#32;states&#32;and&#32;transitions&#32;increases.&#32;Each&#32;new&#32;state&#32;may&#32;require&#32;additional&#32;transitions,&#32;leading&#32;to&#32;a&#32;tangled&#32;web&#32;of&#32;logic.&#32;In&#32;contrast,&#32;Behavior&#32;Trees&#32;are&#32;in…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.reddit.com/r/unrealengine/comments/1eskk42/should_i_use_behavior_trees_or_finite_state/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/U-eHNCapRHVNWWCVPPMTIvOofZULh0_A_FQKe8xTE4I/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvN2ZiNTU0M2Nj/MTFhZjRiYWViZDlk/MjJiMjBjMzFjMDRk/Y2IzYWI0MGI0MjVk/OGY5NzQzOGQ5NzQ5/NWJhMWI0NC93d3cu/cmVkZGl0LmNvbS8&quot;,&quot;title&quot;&#58;&quot;r/unrealengine&#32;on&#32;Reddit&#58;&#32;Should&#32;I&#32;use&#32;behavior&#32;trees&#32;or&#32;Finite&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…tasks&#32;you&#32;want&#32;the&#32;AI&#32;to&#32;do.&#32;You&#32;can&#32;make&#32;a&#32;hierarchical&#32;state&#32;machine&#32;where&#32;states&#32;are&#32;running&#32;within&#32;states.&#32;What&#32;behavior&#32;trees&#32;do&#32;is&#32;awfully&#32;similar&#32;to&#32;a&#32;finite&#32;state&#32;machine&#32;except&#32;its&#32;done&#32;in&#32;the&#32;form&#32;of&#32;decoupled&#32;behavior…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.davideaversa.it/blog/choosing-behavior-tree-goap-planning/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/IsRcWSyhapb5w6AnXzz4_1ChpQZHoNJqxklzISwe0f0/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvYzQ3NTk5NDIx/OWNiNjk4ZmZmYTBj/Yzg5ZTQ3M2EyZDky/ODRhYjk5NjZlNjNl/MWI5NWE1MGU4ZDM2/MDc0MjMzMi93d3cu/ZGF2aWRlYXZlcnNh/Lml0Lw&quot;,&quot;title&quot;&#58;&quot;Choosing&#32;between&#32;Behavior&#32;Tree&#32;and&#32;GOAP&#32;(Planning)&#32;·&#32;Davide&#32;Aversa&quot;,&quot;snippet&quot;&#58;&quot;…on&#32;/r/gamedesign&#32;some&#32;days&#32;ago.&#32;The&#32;main&#32;point&#32;of&#32;the&#32;question&#32;was&#58;&#32;how&#32;can&#32;I&#32;decide&#32;if&#32;it&#32;“better”&#32;to&#32;implement&#32;the&#32;decision-making&#32;layer&#32;of&#32;our&#32;game&#32;AI&#32;with&#32;Behavior&#32;Trees&#32;(BTs)&#32;or&#32;with&#32;more&#32;advanced&#32;plan-based…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//opsive.com/support/documentation/behavior-designer-pro/concepts/behavior-trees-vs-finite-state-machines/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/XIWUng7bz-jCUr7c5AXgyBfxtLf91Zk93ZTroTzWhQ0/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvNjQxYjE3YWNh/MjM3NGExNTYzZWUx/NTVmZjkxOGVjZWNm/MTBlMDYyMjViZjBk/YzYzMDlhZGUzYTVh/MjJmNmI0MS9vcHNp/dmUuY29tLw&quot;,&quot;title&quot;&#58;&quot;Behavior&#32;Trees&#32;vs&#32;Finite&#32;State&#32;Machines&#32;-&#32;Opsive&quot;,&quot;snippet&quot;&#58;&quot;…and&#32;Finite&#32;State&#32;Machines&#32;(FSMs)&#32;are&#32;widely&#32;used&#32;techniques&#32;for&#32;organizing&#32;complex&#32;gameplay&#32;and&#32;AI&#32;logic.&#32;Although&#32;both&#32;can&#32;be&#32;used&#32;to&#32;model&#32;behavior,&#32;they&#32;are&#32;built&#32;around&#32;different&#32;core&#32;responsibilities&#32;and&#32;excel&#32;at&#32;different&#32;layers&#32;of&#32;system&#32;design.&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//opsive.com/support/documentation/state-designer/concepts/finite-state-machines-vs-behavior-trees/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/XIWUng7bz-jCUr7c5AXgyBfxtLf91Zk93ZTroTzWhQ0/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvNjQxYjE3YWNh/MjM3NGExNTYzZWUx/NTVmZjkxOGVjZWNm/MTBlMDYyMjViZjBk/YzYzMDlhZGUzYTVh/MjJmNmI0MS9vcHNp/dmUuY29tLw&quot;,&quot;title&quot;&#58;&quot;Finite&#32;State&#32;Machines&#32;vs&#32;Behavior&#32;Trees&#32;-&#32;Opsive&quot;,&quot;snippet&quot;&#58;&quot;Control&#58;&#32;Structured&#32;and&#32;predictable&#32;flow&#32;&#124;&#32;Decision-Making&#58;&#32;Flexible&#32;and&#32;reactive&#32;behavior&#32;&#124;&#32;State&#32;Control&#58;&#32;Stable&#32;execution&#32;within&#32;states&#32;&#124;&#32;Decision-Making&#58;&#32;Dynamic&#32;branching&#32;logic&#32;&#124;&#32;State&#32;Control&#58;&#32;Clear&#32;entry&#32;and&#32;exit&#32;behavior&#32;&#124;&#32;Decision-Making&#58;&#32;Priority&#32;and&#32;fallback&#32;handling&#32;&#124;&#32;Finite&#32;State&#32;Machines&#32;vs&#32;Behavior&#32;Trees…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.researchgate.net/publication/383241439_Sravnenie_modelej_Igrovogo_Iskusstvennogo_Intellekta&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/WJ25-tL-91vp8kSoF0YS7d8CZVivP1cG-EjlGvrmxOc/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvZjE0MDk0Yzli/MWQ3NGQ2ZTNjODVh/YWVmMDNkM2NkZWI5/NGEzNjdhMmEyY2E2/N2EzN2Y0OThlZjdi/YWU3MjkwMi93d3cu/cmVzZWFyY2hnYXRl/Lm5ldC8&quot;,&quot;title&quot;&#58;&quot;(PDF)&#32;Сравнение&#32;моделей&#32;Игрового&#32;...&quot;,&quot;snippet&quot;&#58;&quot;Behavior&#32;Tree&#32;–&#32;Дерево&#32;поведения;&#32;3.&#32;Utility&#32;AI&#32;–&#32;Утилитарный&#32;искусственный&#32;интеллект;&#32;4.&#32;GOAP&#32;–&#32;Ориентированный&#32;на&#32;задаче&#32;планировщик&#32;действий;&#32;5.&#32;HTN&#32;–&#32;Сеть&#32;иерархических&#32;задач.&#32;Каждая&#32;модель,&#32;описанная&#32;ниже,&#32;представлена&#32;в&#32;своей&#32;&#92;&quot;минимально&#32;комплектации&#92;&quot;,&#32;поскольку&#32;для&#32;большинства&#32;реализаций…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//redefinegamedev.com/are-state-machines-still-relevant-for-ai/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/DcSTWzu6iWo3Jk9SwDuNJzqqJerVCMcFg0BF1UMbUz8/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvOWM5OTkxZGU4/ZWY2ZDQwODY2MTQx/MTY3NTkxMmEwNmQ4/NzBjNmE4NjI3NjFh/ZGE2YzNlM2YxM2M1/NDE0YzRlYi9yZWRl/ZmluZWdhbWVkZXYu/Y29tLw&quot;,&quot;title&quot;&#58;&quot;Are&#32;State&#32;Machines&#32;still&#32;Relevant&#32;for&#32;AI?&quot;,&quot;snippet&quot;&#58;&quot;Finite&#32;State&#32;Machine&#32;from&#32;scratch.&#32;And,&#32;as&#32;a&#32;bonus,&#32;you&#32;will&#32;also&#32;get&#32;a&#32;visual&#32;editor&#32;in&#32;most&#32;cases.&#92;nWhat&#32;if&#32;you&#32;want&#32;more&#32;complex&#32;behaviors,&#32;more&#32;re-usability?&#32;Then&#32;you&#32;might&#32;have&#32;to&#32;look&#32;into&#32;other&#32;systems&#32;like&#32;Behavior&#32;Trees…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.socratopia.app/library/game-code-anatomy-en/chapter-12&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/P-AteXDqPCvos_z6zRd2NrNU_lxw_Lc5GuisL0z9Vdw/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvZWE4MWZkYTQx/MGEyNWFkZDBiMWM2/ZTliMjQ5YjE4YjNj/YTU3OTZkNDM1ZGZl/YjM0N2M1ZjZiODFh/ZDE0NThmMy93d3cu/c29jcmF0b3BpYS5h/cHAv&quot;,&quot;title&quot;&#58;&quot;Beyond&#32;State&#32;Machines&#58;&#32;Utility&#32;AI,&#32;Behavior&#32;Trees,&#32;and&#32;Goal-Oriented&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…action&#32;is&#32;a&#32;behavior&#32;tree&#32;subtree&#32;that&#32;handles&#32;the&#32;messy&#32;details&#32;(moving&#32;to&#32;cover&#32;involves&#32;navigation,&#32;animation,&#32;possibly&#32;calling&#32;for&#32;cover&#32;fire,&#32;all&#32;naturally&#32;expressed&#32;as&#32;a&#32;tree).&#32;The&#32;planner&#32;does&#32;the&#32;strategic&#32;thinking;&#32;the&#32;trees&#32;do&#32;the&#32;tactical&#32;execution.&quot;&#125;&#93;"} | Can become opaque ("deep-tree opacity") at scale; continuous evaluation overhead. |
| **GOAP** | Adaptive AI, Strategy Games | Dynamic goal achievement; emergent multi-step planning. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//www.socratopia.app/library/game-code-anatomy-en/chapter-12&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/P-AteXDqPCvos_z6zRd2NrNU_lxw_Lc5GuisL0z9Vdw/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvZWE4MWZkYTQx/MGEyNWFkZDBiMWM2/ZTliMjQ5YjE4YjNj/YTU3OTZkNDM1ZGZl/YjM0N2M1ZjZiODFh/ZDE0NThmMy93d3cu/c29jcmF0b3BpYS5h/cHAv&quot;,&quot;title&quot;&#58;&quot;Beyond&#32;State&#32;Machines&#58;&#32;Utility&#32;AI,&#32;Behavior&#32;Trees,&#32;and&#32;Goal-Oriented&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…still&#32;too&#32;expensive&#32;to&#32;redo&#32;every&#32;frame,&#32;so&#32;production&#32;systems&#32;plan&#32;only&#32;on&#32;goal&#32;change&#32;or&#32;plan&#32;invalidation;&#32;the&#32;scheduling&#32;around&#32;that&#32;is&#32;its&#32;own&#32;engineering&#32;work).&#32;Teams&#32;pay&#32;the&#32;cost&#32;when&#32;the&#32;AI's&#32;value&#32;comes&#32;from&#32;emergent&#32;multi-step&#32;p…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.davideaversa.it/blog/choosing-behavior-tree-goap-planning/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/IsRcWSyhapb5w6AnXzz4_1ChpQZHoNJqxklzISwe0f0/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvYzQ3NTk5NDIx/OWNiNjk4ZmZmYTBj/Yzg5ZTQ3M2EyZDky/ODRhYjk5NjZlNjNl/MWI5NWE1MGU4ZDM2/MDc0MjMzMi93d3cu/ZGF2aWRlYXZlcnNh/Lml0Lw&quot;,&quot;title&quot;&#58;&quot;Choosing&#32;between&#32;Behavior&#32;Tree&#32;and&#32;GOAP&#32;(Planning)&#32;·&#32;Davide&#32;Aversa&quot;,&quot;snippet&quot;&#58;&quot;…point&#32;of&#32;the&#32;question&#32;was&#58;&#32;how&#32;can&#32;I&#32;decide&#32;if&#32;it&#32;“better”&#32;to&#32;implement&#32;the&#32;decision-making&#32;layer&#32;of&#32;our&#32;game&#32;AI&#32;with&#32;Behavior&#32;Trees&#32;(BTs)&#32;or&#32;with&#32;more&#32;advanced&#32;plan-based&#32;techniques&#32;such&#32;as&#32;Goal&#32;Oriented&#32;Action&#32;Planning&#32;(GOAP)…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//www.researchgate.net/publication/383241439_Sravnenie_modelej_Igrovogo_Iskusstvennogo_Intellekta&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/WJ25-tL-91vp8kSoF0YS7d8CZVivP1cG-EjlGvrmxOc/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvZjE0MDk0Yzli/MWQ3NGQ2ZTNjODVh/YWVmMDNkM2NkZWI5/NGEzNjdhMmEyY2E2/N2EzN2Y0OThlZjdi/YWU3MjkwMi93d3cu/cmVzZWFyY2hnYXRl/Lm5ldC8&quot;,&quot;title&quot;&#58;&quot;(PDF)&#32;Сравнение&#32;моделей&#32;Игрового&#32;...&quot;,&quot;snippet&quot;&#58;&quot;Behavior&#32;Tree&#32;–&#32;Дерево&#32;поведения;&#32;3.&#32;Utility&#32;AI&#32;–&#32;Утилитарный&#32;искусственный&#32;интеллект;&#32;4.&#32;GOAP&#32;–&#32;Ориентированный&#32;на&#32;задаче&#32;планировщик&#32;действий;&#32;5.&#32;HTN&#32;–&#32;Сеть&#32;иерархических&#32;задач.&#32;Каждая&#32;модель,&#32;описанная&#32;ниже,&#32;представлена&#32;в&#32;своей&#32;&#92;&quot;минимально&#32;комплектации&#92;&quot;,&#32;поскольку&#32;для&#32;большинства&#32;реализаций…&quot;&#125;,&#123;&quot;url&quot;&#58;&quot;https&#58;//medium.com/@umut_cirak/state-machines-choosing-the-right-ai-model-for-your-game-298fdfe8b7c6&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/4R4hFITz_F_be0roUiWbTZKhsywr3fnLTMTkFL5HFow/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvOTZhYmQ1N2Q4/NDg4ZDcyODIyMDZi/MzFmOWNhNjE3Y2E4/Y2YzMThjNjljNDIx/ZjllZmNhYTcwODhl/YTcwNDEzYy9tZWRp/dW0uY29tLw&quot;,&quot;title&quot;&#58;&quot;State&#32;Machines&#58;&#32;Choosing&#32;the&#32;Right&#32;AI&#32;Model&#32;for&#32;Your&#32;Game&quot;,&quot;snippet&quot;&#58;&quot;…as&#32;seen&#32;in&#32;the&#32;image&#32;below&#58;&#32;https&#58;//www.gamedeveloper.com/programming.&#32;This&#32;complexity&#32;can&#32;make&#32;transitions&#32;and&#32;conditions&#32;hard&#32;to&#32;handle,&#32;potentially&#32;leading&#32;to&#32;bugs.&#32;In&#32;such&#32;cases,&#32;alternative&#32;AI&#32;techniques&#32;like&#32;Behavior&#32;Trees&#32;or&#32;Goal-Oriented&#32;Action&#32;Planning&#32;(GOAP…&quot;&#125;&#93;"} | Computationally expensive (requires search algorithms); harder to debug. |
| **Co-routines** | Protocol Parsing, Sequential Logic | Cleaner code; no explicit state variables needed. | Harder to manage complex branching or parallel tasks compared to trees. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//www.davideaversa.it/blog/choosing-behavior-tree-goap-planning/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/IsRcWSyhapb5w6AnXzz4_1ChpQZHoNJqxklzISwe0f0/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvYzQ3NTk5NDIx/OWNiNjk4ZmZmYTBj/Yzg5ZTQ3M2EyZDky/ODRhYjk5NjZlNjNl/MWI5NWE1MGU4ZDM2/MDc0MjMzMi93d3cu/ZGF2aWRlYXZlcnNh/Lml0Lw&quot;,&quot;title&quot;&#58;&quot;Choosing&#32;between&#32;Behavior&#32;Tree&#32;and&#32;GOAP&#32;(Planning)&#32;·&#32;Davide&#32;Aversa&quot;,&quot;snippet&quot;&#58;&quot;…solved&#32;using&#32;simpler,&#32;reactive&#32;methods&#32;such&#32;as&#32;BTs&#32;or&#32;Finite-State&#32;Machines?&#32;To&#32;answer&#32;this&#32;question,&#32;you&#32;need&#32;to&#32;know&#32;the&#32;fundamental&#32;differences&#32;between&#32;the&#32;two.&#92;nBehavior&#32;Trees&#32;vs.&#32;Planning&#92;nBTs,&#32;roughly&#32;speaking,&#32;are&#32;a&#32;fancy&#32;way&#32;to&#32;encode&#32;complex&#32;sequen…&quot;&#125;&#93;"} |
| **Event-Driven** | Decoupled Systems, UI | Reacts to events; reduces tight coupling between components. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//www.reddit.com/r/unrealengine/comments/1eskk42/should_i_use_behavior_trees_or_finite_state/&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/U-eHNCapRHVNWWCVPPMTIvOofZULh0_A_FQKe8xTE4I/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvN2ZiNTU0M2Nj/MTFhZjRiYWViZDlk/MjJiMjBjMzFjMDRk/Y2IzYWI0MGI0MjVk/OGY5NzQzOGQ5NzQ5/NWJhMWI0NC93d3cu/cmVkZGl0LmNvbS8&quot;,&quot;title&quot;&#58;&quot;r/unrealengine&#32;on&#32;Reddit&#58;&#32;Should&#32;I&#32;use&#32;behavior&#32;trees&#32;or&#32;Finite&#32;...&quot;,&quot;snippet&quot;&#58;&quot;…level&#32;tasks&#32;you&#32;want&#32;the&#32;AI&#32;to&#32;do.&#32;You&#32;can&#32;make&#32;a&#32;hierarchical&#32;state&#32;machine&#32;where&#32;states&#32;are&#32;running&#32;within&#32;states.&#32;What&#32;behavior&#32;trees&#32;do&#32;is&#32;awfully&#32;similar&#32;to&#32;a&#32;finite&#32;state&#32;machine&#32;except&#32;its&#32;done&#32;in&#32;the&#32;form&#32;of&#32;decoupl…&quot;&#125;&#93;"} | Can lead to "spaghetti code" if event flows are not strictly documented. |
| **MPC** | Robotics, Control Systems | Optimizes outcomes; handles continuous variables and constraints. :inlineCitations{data="&#91;&#123;&quot;url&quot;&#58;&quot;https&#58;//medium.com/@prathameshburade07/finite-state-machine-in-game-ai-sharing-what-i-learn-001-1262dbd112e5&quot;,&quot;favicon&quot;&#58;&quot;https&#58;//imgs.search.brave.com/4R4hFITz_F_be0roUiWbTZKhsywr3fnLTMTkFL5HFow/rs&#58;fit&#58;32&#58;32&#58;1&#58;0/g&#58;ce/aHR0cDovL2Zhdmlj/b25zLnNlYXJjaC5i/cmF2ZS5jb20vaWNv/bnMvOTZhYmQ1N2Q4/NDg4ZDcyODIyMDZi/MzFmOWNhNjE3Y2E4/Y2YzMThjNjljNDIx/ZjllZmNhYTcwODhl/YTcwNDEzYy9tZWRp/dW0uY29tLw&quot;,&quot;title&quot;&#58;&quot;Finite&#32;State&#32;Machine&#32;in&#32;Game&#32;AI&#32;—&#32;Sharing&#32;What&#32;I&#32;Learn&#32;001&quot;,&quot;snippet&quot;&#58;&quot;Advantages&#32;&#58;&#32;1.&#32;Performance&#32;Efficient&#32;—&#32;Minimal&#32;overhead,&#32;O(1)&#32;state&#32;execution.&#32;2.&#32;Memory&#32;Friendly&#32;—&#32;Small&#32;footprint,&#32;predicatable&#32;memory&#32;usage.&#32;3.&#32;Deterministic&#32;—&#32;Same&#32;inputs&#32;always&#32;produce&#32;same&#32;outputs.&#32;4.&#32;Easy&#32;to&#32;serialize&#32;—&#32;Game&#32;saves,&#32;networking,&#32;replay&#32;system…&quot;&#125;&#93;"} | High computational cost; requires an accurate model of the system dynamics. |
---
- ~~:round_pushpin: do [#statebb1]~~
- Fix Insufficient IPC Proof issues.
- do [#advr03]
- do [#advr04]
- do [#advr05]
- do [#advr06]
- do [#advr07]
- do [#advr08]
- do [#advr09]
- do [#advr10]
- do [#advr11]
- do [#advr12]
- do [#advr13]
- do [#advr14]
- do [#advr15]
- do [#advr16]
- do [#advr17]
- do [#advr18]
- JB `Run Agent Doc` still does not quickly submit the prompt.
- do [#advr19]
<!-- /agent:queue -->
<!--
- 🧪 test edit for [#qnodemerge4wire]
---
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#advr03] Advance original #j9ja: finish #optverify marker coverage by adding `agent_restart_performed` in #agentreloadrestart execution and a restart-agent menu-invoke marker in #restartagentmenu; then auto-resolve the stale review gate from ops-log proof/disproof markers.
- [ ] [#advr04] Advance original #restartagentmenu: implement JB `RestartAgentAction.kt`, plugin.xml registration, TerminalUtil restart wiring, pluginVersion bump, signed+unsigned builds, action invocation log, and resolved harness log. Plan: tasks/agent-doc/plan-restart-agent.md.
- [ ] [#advr05] [recommended] Advance original #s81q: add Stop Agent and mirror Stop+Restart into VS Code extension menus; implement JB `StopAgentAction.kt`, plugin.xml registration, VS Code command contributions, and signed/unsigned plugin artifacts.
- [ ] [#advr06] Advance original #recyclerename: rename JB + VS Code "Restart Supervisor Process" UI text to "Recycle Supervisor" while keeping stable action IDs, and ensure Restart Agent and Recycle Supervisor commands sit alongside each other. Plan: tasks/agent-doc/plan-restart-agent.md.
- [ ] [#advr07] [recommended] Advance original #supinstallvis: make supervisor auto-install/rebuild status visible in pane body or plugin notification surface, choose a non-input-consuming mechanism, add ops/log proof, and avoid disturbing keepalive Enter-to-restart reader.
- [ ] [#advr08] Advance original #runexitrestart: implement/verify recovery for cleanly exited harness panes by detecting `Press Enter to restart`, restarting the harness, waiting for dispatch-ready proof, and submitting prompt-bearing work so Run Agent Doc cannot drop on clean exit.
- [ ] [#advr09] Advance original #jbrundispatch: replace operator-capture gate with deterministic Run Agent Doc dispatch diagnostics using admin detect, route trace, pane state, dispatch/start proof, and auto-filed route bugs; add regression coverage where possible.
- [ ] [#advr10] Advance original #editorbufwin: finish editor-buffer-wins P2 by auto-recovering and committing preserved queue additions when replay normalization neutralizes the queue component. Plan: tasks/agent-doc/plan-editor-buffer-wins-queue-merge.md.
- [ ] [#advr11] Advance original #docdriftgrace: implement component-scoped graceful drift resolution that auto-rebases unrelated changes and surfaces only structural conflicts on in-flight prompt/response. Plan: tasks/agent-doc/plan-graceful-doc-drift-resolution.md.
- [ ] [#advr12] Advance original #ftqpartialdup: confirm whether #hap7/#gf5n fixes same-node partial free-text queue duplication; if not, add same-node/free-text conflict guard and regression for truncated duplicate queue prompts.
- [ ] [#advr13] Advance original #8bfz: implement single live JB plugin consumer per document using a per-doc owner lease; non-owner windows defer patch apply + saveDocument, with plugin PatchWatcher + binary lease primitive and multi-window proof.
- [ ] [#advr14] Advance original #qpauseux: finish paused-queue plugin UX with info/action notifications, Resume or restart-supervisor/resume actions, no raw error toast, plugin artifacts, and render/ops proof.
- [ ] [#advr15] Advance original #jbdisprecycle: convert recycle-boundary Run Agent Doc verification into deterministic proof for exactly one `dispatch_start_proof` with submit and no stranded start_session failure; ops marker `route_dispatch_submit_recycle_settle`.
- [ ] [#advr16] Advance original #tb4q: complete #qfocsup focused-cycle supervisor clear-and-continue proof with install/recycle if needed, `session-check ui_outcome=deferred_for_supervisor_drain`, `context_reset`, `go_drain_dispatch`, and fresh-agent drain evidence.
- [ ] [#advr17] Advance original #fbwireverify: complete #fbwire/#fullboundary acceptance with slow-ACK no inflight pileup and wedged no-ACK bounded timeout + ERROR marker + playback artifact; add deterministic tmux-ci/sim coverage where possible.
- [ ] [#advr18] Advance original #qnodemerge4verify: after JB/VS Code plugin install, live-test non-ASCII edit-during-merge around `café 日本 😀`; prove accepted byte-offset editor ops and no mojibake/interleaving, plus automated log checker for `editor_op_recorded` + `editor_ops_for_base`.
- [ ] [#advr19] Advance original #cyh0: identify and instrument remaining File Cache Conflict trigger by capturing exact action, agent-doc command, and ops marker such as `write_authority action=... transport=disk`; distinguish ack-content, VCS-refresh/save, and editor-IPC convergence surfaces.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#qloop1] #agent-doc-feature Define durable queue loop annotation syntax and parser/rendering behavior: mark loop-only queue/backlog entries, keep annotation/state out of prompt completion proof, and expose a stable current-item marker for editor/supervisor surfaces.
- [ ] [#qloop2] #agent-doc-feature Implement idle-loop scheduling for annotated loop items: when normal drainable queue work is empty, dispatch loop items in deterministic sequence, persist the loop cursor/last-run state, and prove regular queue items always preempt loop work.
- [ ] [#qloop3] #agent-doc-feature Add loop poll controls: global default poll period plus per-loop override, next-run time gates, bounded backoff/jitter, and tests proving no tight spin when there are no relevant events.
- [ ] [#qloop4] #agent-doc-feature Add event-aware loop scheduling: record deterministic event counters/timestamps and let loop items schedule earlier/later based on event frequency before any model-assisted scheduling hook.
- [ ] [#qobs1] #agent-doc-feature Add a bounded observer telemetry snapshot surface: supervisor/session actors expose compact per-document state (cycle phase, queue head, actor readiness, IPC inflight, recent typed ops-log markers, and artifact handles) without dumping raw transcripts by default.
- [ ] [#qobs2] #agent-doc-feature Implement deterministic observer rules that scan session-check/preflight/ops-log snapshots for annotated issues and recurring failure patterns, then file deduped session-doc backlog bugs with evidence paths and stable issue fingerprints.
- [ ] [#qobs3] #agent-doc-feature Add a sidecar observer agent-doc loop that can monitor all running project sessions from snapshot artifacts, rate-limit bug filing, avoid pane interference, and route created bug items to the correct session document or a configured observer inbox.
- [ ] [#qobs4] #agent-doc-feature Add model-assisted observer triage as advisory-only: feed compact snapshot/context-pack handles to a subscription or local model, require deterministic evidence before auto-filing bugs, and record model provenance/confidence on suggestions.
- [ ] [#qlm1] #agent-doc-feature Define queue item backend-selection syntax for local-model execution, separate from queued `/model <tier>` switches: support explicit per-item backend/model annotations on normal and loop queue items without changing prompt text or completion proof.
- [ ] [#qlm2] #agent-doc-feature Implement a local-model queue dispatcher backend that can run eligible queue/loop items through a configured local model, capture stdout/structured results, and write responses through the normal agent-doc closeout boundary.
- [ ] [#qlm3] #agent-doc-feature Add local-model dispatch safety gates and telemetry: capability checks, GPU/resource lease integration, fallback policy, provenance logs, and tests proving local-model queue items cannot be silently marked complete without a committed response/proof.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
