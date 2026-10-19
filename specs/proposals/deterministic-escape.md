# Proposal: Deterministic Escapes In Agent-Doc Documents

> **Status:** proposal, non-normative. Nothing here is implemented. Normative
> behavior stays in [`SPEC.md`](../../SPEC.md) and the numbered `specs/` files;
> a phase below becomes normative only when its queue item lands spec + tests.

## Question

> Would it make sense to mix deterministic logic into agent-doc documents? Like
> a programming language that is agentic first but can escape to deterministic
> logic?
>
> Is there any precedence of agentic programming languages? How about an
> embedded programming language for markdown? How about a template-driven
> approach? Perhaps we can support functions + a built in `prompt("...")`
> function, support conditional logic? Could we use fenced python or js? If we
> support functions, could we setup events or hooks? Would Jupyter be a good
> inspiration or since agent-doc is more of a graph, some sort of event-driven
> graph programming language?

## Answer In One Paragraph

Yes, and agent-doc already is one. A session document is a program that the
agent interprets by default. The binary interprets a growing, closed vocabulary
deterministically: preset expansion, queue grammar and attributes, tracked ids,
slash commands, orchestration DAGs, closeout guards. What is missing is an
**operator-extensible** deterministic escape. Today a project cannot declare
"run `make check-fast`, record what actually happened, and refuse to commit
without it" in a form the binary executes and enforces. That intent stays in
prose (`update spec + tests`), the agent interprets it, and the only evidence
is the agent's own claim. The recommendation is **not** a general-purpose
language. Add *named, project-registered steps*: commands defined in a
committed registry, run by the binary, recorded as content-addressed receipts,
and referenced from the document by name only. Phase 1 adds only the runner and
receipts. Each later phase is one queue item that lets the document bind those
receipts as gates, as pipeline stages between queue items, as hooks on
lifecycle events, and as escapes back into an agent turn.

## Direct Answers To The Sub-Questions

**Is there precedent for agentic programming languages?** Yes, plenty. See
the Precedent section below. The languages either embed LLM calls in deterministic
code (LMQL, Guidance, SGLang, APPL, PDL, BAML, Jac's `by llm`, Semantic Kernel)
or are pseudo-languages the model interprets itself (SudoLang, OpenProse). None
of them has agent-doc's shape: a concurrently edited, committed document whose
deterministic directives are enforced by a separate binary.

**An embedded programming language for markdown?** No. The document has two
concurrent authors (operator and agent) and receives pasted untrusted text
through a CRDT. A language there would let any byte edit change program
semantics, including half-typed edits mid-turn, which is why steering already
debounces incomplete shapes. Every grammar addition also costs four editor
plugins and four harness skills. agent-doc already has a small *declarative*
embedded vocabulary: component markers, attributes, queue grammar, presets.
Keep growing that one named directive at a time, each with spec + tests.
Markdoc (recall) made the same choice: tags with declared attributes and no
arbitrary code in content.

**A template-driven approach?** Only for producing prompts and renderings,
and only as pure substitution. agent-doc is already template-driven: template
format, components, and presets as parameterized prompts (a preset invocation
can already carry an argument, as in `#gh-fix <url>`). A sound extension is
substitution of preset arguments and step-receipt fields, such as a failed
step's bounded tail, into a preset body (Phase 4, and optional Phase 8).
Template logic (Handlebars/Jinja `if`/`each`) is not: it is a language again,
just hidden in a template.

**Functions plus a built-in `prompt("...")`?** Functions, yes, as *named*
things, invoked symmetrically as Semantic Kernel does. Presets already are the
agentic functions: a preset is a named `prompt("...")`. Steps (this proposal)
would be the deterministic functions. A `prompt()` callable from inside a
script is the wrong ownership direction. The script would have to own the turn
lifecycle (preflight, capture, closeout, commit), and that lifecycle belongs to
the binary. The deterministic-code-calls-agent escape already exists outside
the document: a script may call `agent-doc orchestrate --task ...`. Steps must
not call it by default, because that would be a recursive turn with a `write`
effect.

**Conditional logic?** Only over typed facts, from a fixed predicate set the
binary evaluates: step passed/failed, receipt exists for this tree, queue
drained (`after=`), time reached (`--- start at`). That covers Phase 3's halt
and Phase 4's `else`. No general `if <expression>`. A branch that needs
judgement is the agent's job, and the agent can already enqueue or gate work.
The test is the code-enforced-directives rule: a conditional belongs to the
binary only if its predicate could be a `#[test]`.

**Fenced Python or JS?** No to executing fenced code from the document.
- *Sandboxing:* Python has no reliable in-process sandbox. JS can run in an
  isolated engine with permissions, but useful steps need the filesystem, git,
  and network, which is exactly what a sandbox removes. The result is either
  useless or unsafe.
- *Determinism:* the fence is CRDT-edited text. Something would have to pin
  which revision ran, plus the interpreter and dependency versions, and the
  output is still nondeterministic.
- *Replay:* re-running on recovery repeats side effects. Receipts avoid that
  only if the code is content-addressed, which a repo file already is.
- *Injection:* the spec treats fenced code as inert literal evidence (pasted
  logs and diffs). Making fences executable breaks that invariant and turns
  every paste into a hazard.

Put the code in a repository file (`scripts/x.py`) and register it as a step.
It is then reviewable, testable, lintable, and content-addressed, at the cost of
one extra file.

**If we support functions, events or hooks?** Yes. This is the strongest idea
in the list, as long as hooks subscribe to the **typed ledger facts** that
already exist ([13-state-backbone](../13-state-backbone.md): `PreflightStarted`,
`ResponseCaptured`, `CommitObserved`, `QueueHeadCompleted`,
`CycleAbandoned`) and not to raw file events. A hook is a registry entry
`on = "CommitObserved"` that runs a `read` or `check` step after the fact is
durable. It runs outside every actor's critical section, exactly once per
`(event id, step)`. `write`-effect hooks never fire automatically. See
Phase 5.

**Jupyter, or an event-driven graph language?** The graph, but agent-doc
already has it, and it should not be exposed as a language. Jupyter is the
wrong model for execution: linear cells, hidden kernel state, out-of-order
runs, and outputs stored as document text that merge-conflicts. It is right
about one thing: results should appear next to what produced them, which
Phases 3 and 6 provide. The event-driven graph exists today. The state
backbone is an append-only typed ledger with idempotent reducers and
single-owner actors, and its own model table already rates "event-driven" as
the strong backbone fit. Live state runs on lazily `Source` / `Computed` /
`Effect`. Steps map onto it directly:

| Graph role | agent-doc primitive | Step mapping |
| --- | --- | --- |
| Source (input) | worktree tree id, `HEAD`, document component nodes, ledger facts | `inputs = worktree \| head \| none`, `on = <fact>` |
| Computed (memoized, content-keyed) | reducers and projections; lazily `Computed` | a receipt keyed by `(step, entry_sha256, input)`, recomputed only when an input changes (marimo-style reactivity without a kernel) |
| Effect (owned side effect) | single-owner actors (controller, supervisor, queue orchestrator) | step execution runs on the owning actor and emits a typed `StepReceiptRecorded` fact |
| Edge | queue order, `after=`, orchestrate DAG, derivation edges | `requires:`, `/step` position, `else`, `on=` |
| Projection | document / queue / proof projections | result cells, `agent:signals` readings, closeout gate verdicts |

The operator declares nodes (steps) and edges (four small attributes). The
controller evaluates the graph. Nothing new needs a scheduler.

**Recommendation (one direction):** an **event-driven graph of named,
committed, deterministic steps**, built on the existing state backbone and
evaluated by the binary. The document references steps by name through
`requires:`, `/step`, `else`, and `on=`. Rejected: an embedded language,
executable fences, Jupyter-style cells, and general conditionals. Templates are
accepted only as pure parameter substitution.

## Precedent

There is substantial precedent. It splits into two families plus older
document-execution systems. In the "Basis" column, *verified* means the
description was confirmed against the project's own paper or documentation via
web search on 2026-10-15; nothing was run hands-on. *Recall* means it is stated
from background knowledge and not re-checked for this proposal.

### Family A: deterministic host language, LLM calls as first-class steps

The program is ordinary code, and the model is one kind of call within it. This
is the reverse direction of the question: deterministic-first, escaping to the
agent.

| System | What it does | Basis | Relation to this proposal |
| --- | --- | --- | --- |
| LMQL (ETH Zurich, 2022) | A Python superset that interleaves algorithmic logic with LLM calls. It enforces declarative constraints during decoding via token masking. ([paper](https://arxiv.org/pdf/2212.06094)) | verified | Interleaves at the token level inside a single generation. agent-doc's unit is a whole turn, so token-level constraints are out of scope. |
| Guidance (Microsoft) | A template language that interleaves `gen`, `select`, conditionals, and loops with constrained generation (regex/CFG). ([project](https://www.microsoft.com/en-us/research/project/guidance-control-lm-output/tools/)) | verified | Same granularity gap as LMQL. Its lesson is that control flow belongs to the host and the model fills holes. |
| SGLang | A Python-embedded DSL (`gen`, `select`, `fork`, `join`) plus a runtime that reuses the KV cache across calls. ([paper](https://arxiv.org/html/2312.07104v2)) | verified | `fork`/`join` maps onto agent-doc's existing `subagents` fan-out and `orchestrate --mode parallel`. Nothing new is needed there. |
| APPL (2024, ACL 2025) | Embeds prompts in Python functions and vice versa. Its runtime traces calls so a run can be **replayed without re-querying**. ([paper](https://arxiv.org/abs/2406.13161v1)) | verified | Replay-from-trace is the property receipts provide here: a replayed cycle reuses recorded step results and never re-executes them. |
| PDL, Prompt Declaration Language (IBM) | A YAML language whose blocks are model calls, file/stdin reads, data, **code blocks the interpreter executes**, and `if`/`for`/`repeat`. Outputs accumulate into the context. ([paper](https://arxiv.org/html/2410.19135v1)) | verified | The closest mechanism: declarative blocks mixing model and code. The deliberate difference here: PDL runs code written *in the program*, while this proposal never runs code written in the document (see Safety). |
| BAML (BoundaryML) | A DSL of typed LLM functions. The compiler generates clients that coerce responses to the declared type or fail loudly. ([docs](https://docs.boundaryml.com/ref/baml/function)) | verified | "Fail loudly on type mismatch" is the right failure posture for step outputs bound into a document. |
| Jac / Meaning-Typed Programming (OOPSLA 2025) | A Python superset where `def f(...) -> T by llm()` makes the model the executor of an ordinary typed function. The prompt is derived from names and types. ([paper](https://arxiv.org/pdf/2405.08965), [docs](https://docs.jaseci.org/quick-guide/ideas-behind-jac/)) | verified | The cleanest language-level "escape to agent" construct. Phase 4 below is the document-side analogue: a failed deterministic step escapes into an agent prompt. |
| DSPy (Stanford) | "Programming, not prompting": typed signatures and modules, with optimizers that compile the prompts. ([site](https://dspy.ai)) | verified | Optimizes the agentic side and doesn't bear on deterministic escapes. Mentioned so it is not mistaken for one. |
| Semantic Kernel (Microsoft) | Plugins expose *semantic functions* (prompt templates) and *native functions* (host code) that callers invoke symmetrically. ([docs](https://learn.microsoft.com/en-us/semantic-kernel/concepts/plugins/)) | verified | The symmetric call model is the target shape: a document names a step the same way it names a preset, and the binary decides who executes it. |
| LangGraph, Temporal | A graph of LLM nodes and code nodes (LangGraph). Durable workflows whose orchestration code must be deterministic for replay while side-effecting activities are recorded (Temporal). | recall | Temporal's split is the model for replay: the queue/orchestration layer stays deterministic and replayable, and steps are recorded activities. |

### Family B: agentic-first languages, interpreted by the model itself

These match the question's wording, "agentic first", most closely. Each one
lacks an engine that *guarantees* the deterministic parts.

| System | What it does | Basis | Relation to this proposal |
| --- | --- | --- | --- |
| SudoLang (Eric Elliott) | A natural-language pseudocode with interfaces, constraints, `/commands`, and `\|>` composition, all interpreted by the LLM. ([repo](https://github.com/paralleldrive/sudolang)) | verified | All semantics are model-interpreted, so a "constraint" is a request, not a guarantee. agent-doc's [code-enforced directives](../../runbooks/code-enforced-directives.md) rule exists precisely to avoid this. |
| OpenProse | A markdown-first "programming language for AI sessions". `.prose` programs spawn subagents with explicit control flow. Loading the language spec into context makes the LLM *simulate* the VM. ([site](https://www.prose.md), [docs](https://docs.openclaw.ai/prose)) | verified | The nearest neighbour in surface form: markdown programs, subagents, contracts. The opposite enforcement model: agent-doc's binary is a real interpreter for its directives, and this proposal extends that rather than asking the model to simulate one. |
| Claude Code hooks / skills with bundled scripts | Harness-level deterministic escapes: hooks run commands at lifecycle events, and skills ship scripts the agent calls. | recall | agent-doc already uses hooks (preflight-in-binary, coined-id guard, steering). Steps are the *document-level, operator-declared* counterpart. |

### Older document-execution systems

| System | Basis | Lesson taken |
| --- | --- | --- |
| Org-mode Babel | verified (`:cache` header) / recall (`org-confirm-babel-evaluate`) | Named source blocks with results written back into the document. `:cache yes` skips re-evaluation when the SHA1 of block plus arguments matches the hash stored on the `#+RESULTS:` line ([header args](https://org-babel.readthedocs.io/en/latest/header-args/)). This is the receipt idea. Babel also prompts before evaluating, because a document that runs its own text is a known attack surface. |
| Jupyter / marimo | recall | Jupyter's out-of-order execution and hidden kernel state show what to avoid. marimo's reactive DAG of cells is the fix, and agent-doc's equivalent is content-addressed inputs. |
| Literate programming (Knuth's WEB) | recall | The document is the source of the code (tangle/weave). This proposal does **not** do that: code lives in the repository, and the document only names it. |
| Markdoc (Stripe), MDX | recall | Both embed structure in markdown. MDX allows arbitrary JSX/JS in content; Markdoc deliberately allows only declared tags and attributes with no arbitrary code. agent-doc's component markers are already the Markdoc choice. |
| Make, Bazel, Dagger | recall | Content-addressed inputs → cached results. Already used in this repo by `make tla` and `scripts/dev-check.py` receipts. |

**Where agent-doc sits.** Family A puts the model inside deterministic code.
Family B puts the deterministic code inside the model's imagination. agent-doc
holds an under-occupied third position. The program is a concurrently edited,
git-committed **document**. An LLM is its default interpreter, and a binary is
the deterministic interpreter for a fixed directive vocabulary, with
fail-closed guards. The proposal extends the binary's vocabulary by one
operator-extensible construct. It does not turn the document into a language.

## What Already Exists

Each existing mechanism below is a deterministic escape. All of them come from
a **closed** vocabulary defined by the binary.

| Mechanism | Deterministic part (binary) | Agentic part | Where |
| --- | --- | --- | --- |
| Prompt presets (`prompt_presets` / `presets`, project inheritance, structured `prompt` + `runbook`) | resolution, alias merging, identity-collision registry, expansion into `prompt_preset_expansions`, runbook path confinement | interpreting the expanded prose | [02-document-format](../02-document-format.md), [project-preset-inheritance](../project-preset-inheritance.md), [07-orchestration-commands](../07-orchestration-commands.md), [preset-runbooks](../../runbooks/preset-runbooks.md) |
| `agent:queue` grammar and attributes (`id=`, `after=`, `subagents[=N]`, `preset=`, `go`/`start`/`stop`/`pause`, `disable-auto-clear`, start/stop fences, `--- start at <time>`) | parsing, head selection, dependency gating, activation, consumption proof, auto-strike, atomic refill | answering each head | [02-document-format §2.5](../02-document-format.md), [07-orchestration-commands](../07-orchestration-commands.md), `agent-doc-queue/`, `agent-doc-element-queue/` |
| Tracked ids and granular write flags (`--done`, `--backlog-add`, `--backlog-gate`, `--review-add`, `--todo-add`) | component mutation, de-duplication, gates, `agent:todo` checklist-count guard | deciding *which* flags a turn earned | [pending-ops](../../runbooks/pending-ops.md), [pending-system](../pending-system.md) |
| Slash / builtin queue heads (`/model`, `/compact`, `/clear`) | delivered at idle **without an agent turn**, then struck and committed | none | [07-orchestration-commands § Command classification](../07-orchestration-commands.md) |
| `agent-doc plan` | `repo_actions` parsed from imperative prose (`extract_imperative_directives` in `agent-doc-diff/src/lib.rs`), `required_commands`, `blockers`, `execution_scope` | executing the plan | `src/plan.rs`, [07-orchestration-commands § plan](../07-orchestration-commands.md) |
| `orchestration_request` / `agent-doc orchestrate` (`sequential`, `parallel`, `dag`, auto-DAG schedules with persisted node state and `--resume-schedule`) | task freezing, topological order, fail-fast on cycles, mid-run source-change stop, per-node proof | each node's work | [07-orchestration-commands](../07-orchestration-commands.md), `src/orchestrate.rs` |
| Subagent authorization and handback (`queue brief`, `#waypostauthorization`, `[#subagent-operator-request]`) | resolving the operator's verbatim authorization per item, the packet grammar | the worker | `agent-doc-queue/src/subagent_brief.rs`, [07-orchestration-commands](../07-orchestration-commands.md) |
| Hooks (preflight on `UserPromptSubmit`, `PreToolUse` coined-id guard, steering, Codex Stop) | run at lifecycle points, and can refuse a tool call | none | `agent-doc-hooks-io/src/` |
| Closeout guards and `session-check` | fail-closed queue proof, retained-write and drift gates, operator-todo advisory | none | [07-closeout-commands](../07-closeout-commands.md) |
| Element registry (turn roles, write policies, informational `notes`/`todo`) | which components trigger turns and who may write them. "Effectful plugin execution" is explicitly deferred | none | [14-realtime-workflow § Element Models](../14-realtime-workflow.md), `agent-doc-element-registry/` |
| `agent:signals` | descriptor exists; "built-in tests, queue, supervisor, and IPC signals are expected first; custom signal plugins are deferred" | none | [document-format runbook](../../runbooks/document-format.md), `agent-doc-element-signals/` |
| Dashboard-as-document (`agent-doc patch`, `watch`) | external scripts write components | the agent reacts | [dashboard guide](../../docs/guide/dashboard.md) |
| Code-enforced directives | the rule: "if it can be a `#[test]`, it belongs in the binary" | — | [code-enforced-directives](../../runbooks/code-enforced-directives.md) |
| Receipts and invalidation | `make tla` and `dev-check.py record-full-check` content receipts; dynamic-context packs invalidated by input hashes | — | `scripts/dev-check.py`, `scripts/run_tla.sh`, [dynamic-context](../../runbooks/dynamic-context.md) |
| Formal models | TLA+/Lean models of the binary's own concurrency | — | `formal/` |

**The gap.** Every row is a binary-defined construct. A project cannot add its
own. The dashboard pattern lets scripts write *into* a document, but no
document construct tells the binary to run a script, when to run it, what the
result gates, or what happens on failure. `plan` already parses `run tests` out
of prose deterministically, but it can only advise. The agent runs the command,
reads the output, and reports. A skipped or misread step is caught only by
human review.

## Where The Line Goes

Apply the code-enforced-directives rule at project scope:

- **Deterministic**: anything whose outcome is a checkable predicate, such as
  an exit status, a file hash, a parsed field, a time, or a dependency
  relation. The binary executes or verifies it, records it, and enforces it.
- **Agentic**: anything that needs judgement, such as interpreting intent,
  writing code or prose, choosing among options, or diagnosing a failure.

The document carries both kinds. It must make the boundary *visible*: a reader
can tell which lines the binary will act on by themselves and which the agent
will interpret. The binary owns sequencing, gating, and evidence. The agent
owns content and judgement. Control flow (order, dependency, fan-out, stop)
stays in the existing queue/orchestrate constructs and is not reinvented as
`if`/`for` syntax.

## Design: Named Steps

### Registry (committed, outside the document)

A step is a named argv in a committed project file, `agent-doc-steps.toml` at
the project root. It is not under `.agent-doc/`, which is commonly gitignored
local state.

```toml
[steps.check-fast]
argv = ["make", "check-fast"]
effect = "check"          # read | check | write
timeout_secs = 1800
inputs = "worktree"       # worktree | head | none
summary_lines = 20        # bounded tail rendered into the document

[steps.ci-status]
argv = ["gh", "run", "list", "--workflow", "CI", "--limit", "1", "--json", "status,conclusion"]
effect = "read"
inputs = "none"
max_age_secs = 300        # freshness bound when inputs = none

[steps.install]
argv = ["make", "install"]
effect = "write"
inputs = "worktree"
```

- `argv` is executed directly, never through a shell, with `cwd` at the
  project root. There is no interpolation of document text into argv.
- `effect` classifies the blast radius. `read` and `check` steps must not be
  expected to mutate the repository. `write` steps (install, push, deploy) are
  never triggered implicitly (see Safety).
- `inputs` selects the fingerprint: `worktree` is the content-addressed tree of
  tracked plus untracked non-ignored files (a `git write-tree` over a private
  index, in the same private-index style as the commit path in
  [09-git-integration](../09-git-integration.md)); `head` is the `HEAD` tree
  id; `none` means time-bounded only.

### Receipts (the deterministic record)

`agent-doc step run <FILE> <name>` executes one step and stores a receipt in
the project's `state.db`:

```text
step=check-fast entry_sha256=<hash of the registry entry> input=<tree id>
exit=0 duration_ms=… started_at=… output_sha256=… artifact=<log handle>
```

- The receipt key is `(step, entry_sha256, input)`. A matching passing receipt
  means the step has already passed for these bytes, so re-running it is a
  no-op that returns the receipt. This is org-babel `:cache`, keyed on content
  rather than mtime.
- Output passes through `agent-doc-secret-redact` before anything is rendered
  or logged. The full log is an artifact handle that `tsift digest-runner` can
  read, not document text.
- A receipt is honest evidence and never a retry loop. A flaky failure is
  recorded as a failure, and the step is never auto-rerun until it goes green.

## Recommendation And Phases

**Verdict:** build named steps, in this order. Each phase is sized as one queue
item (spec + tests, no release coupling). Stop after any phase if the next one
is not pulling its weight.

### Phase 1 (smallest first step): registry + `agent-doc step run` + receipts

- Syntax: the `agent-doc-steps.toml` schema above, plus
  `agent-doc step run <FILE> <name> [--json]` and
  `agent-doc step receipts <FILE> [<name>]`.
- Semantics: resolve the project root from `<FILE>` exactly as preset
  resolution does. Load and validate the registry: unknown keys, an empty
  argv, or a missing `effect` fail closed. Compute the input fingerprint, then
  return an existing matching receipt or run the argv with the timeout. Persist
  the receipt and print it. **No document mutation, no gating.** `agent-doc
  plan` may emit `agent-doc step run <FILE> <name>` in `required_commands` when
  a preset or prompt names a registered step, replacing free-form "run the
  tests".
- Tests: registry parse and refusal cases. Receipt reuse on an identical tree
  and a re-run after a one-byte edit. A registry-entry change invalidates
  receipts. Timeout and non-zero exit are recorded as failures, never retried.
  Secret redaction of the rendered tail. The session document is byte-identical
  before and after.

### Phase 2 (first document-visible use): `requires:` on structured presets

```yaml
presets:
  '#release':
    prompt: update spec + tests. build + install for local testing. commit + push
    runbook: runbooks/release.md
    requires: [check-fast]
```

- Preflight lists `required_steps` with exact `step run` commands in that
  preset's `prompt_preset_expansions` entry.
- Closeout for a turn that invoked the preset refuses to commit unless each
  required step has a **passing receipt whose input equals the tree being
  committed**. It names the missing or stale step and the exact command. It
  *reads* receipts and never runs a step, so closeout latency and its wedge
  surface stay unchanged.
- This converts the most common prose authorization ("+ tests") into an
  enforced postcondition, and it reuses the existing preset/authorization path,
  including subagent briefs.

### Phase 3: `/step <name>` as a queue item (deterministic pipeline stage)

```html
<!-- agent:queue id=ship -->
- fix the failing parser test
- /step check-fast
- /step install
- write release notes for the change
<!-- /agent:queue -->
```

- `/step` joins the existing slash-command class. It is delivered at idle with
  **no agent turn**, like `/model` and `/clear`. On pass, the binary strikes
  the item, publishes one bounded result cell to `agent:exchange` (the same
  binary-authored cell pattern as subagent completion feedback), and
  continues. On failure, the queue halts as a stop fence does, with a
  `queue_halted: "step_failed"` reason and the receipt handle.
- Exactly-once on replay: dispatch looks up the receipt before running, so a
  crash between run and strike reuses the receipt. A `write` step records a
  `started` row first. A `started` row without a matching `finished` row
  blocks automatic re-run and needs an operator decision (`write` steps are
  at-least-once by nature).

### Phase 4: escape back to the agent on failure

```html
- /step check-fast else '#review'
```

- On failure, instead of halting, the binary inserts the named preset as the
  next queue head, with the receipt's bounded tail and artifact handle bound
  into the prompt. This is Jac's `by llm` in reverse: deterministic code
  escapes into an agent call with typed inputs, namely the receipt.
- The `else` target must be a registered preset. Free text is not accepted.

### Phase 5: hooks on ledger facts (`on=`)

```toml
[steps.check-fast]
argv = ["make", "check-fast"]
effect = "check"
inputs = "worktree"
on = ["CommitObserved"]   # typed state-backbone facts only
```

- After the named fact is durable, the owning controller runs the step as an
  owned effect, off every actor's critical section. It runs exactly once per
  `(fact event id, step)`. A replayed or re-emitted fact does not bump the
  document epoch, so it does not re-run the step.
- The receipt is published as a typed `StepReceiptRecorded` fact. That fact is
  itself subscribable, which is how a pipeline chains without a scheduler.
- `on=` is allowed only for `read` / `check` steps. A `write` step declaring
  `on=` is a registry validation error.
- Prerequisite: Phase 1 receipts are recorded as ledger facts, not a side
  table only.

### Phase 6: render readings into `agent:signals`

- The `tests` signal that `agent:signals` already anticipates is sourced from
  step receipts: last result per registered step, input short hash, and age.
- The component is binary-written, informational, and a non-trigger. Its
  writes never start a turn (avoiding the dashboard `watch` feedback loop) and
  fall outside every `TurnScope`, so op-affectedness classifies them as
  `independent`.

### Phase 7 (optional): read-only preflight bindings

- A document may list `effect = "read"` steps whose bounded output is attached
  to the cycle contract, for example CI status or `git status` digests. The
  receipt freshness rules (`inputs` / `max_age_secs`) decide reuse, matching
  the dynamic-context invalidation rule.
- Only `read` steps, and only with a hard output budget.

### Phase 8 (optional): preset parameters (the template direction)

- A preset body may contain `{{arg}}` (the invocation argument) and
  `{{step.<name>.status}}` / `{{step.<name>.tail}}` (fields of the latest
  receipt for the current input). Rendering is pure substitution at preflight
  expansion time, with no conditionals, loops, filters, or I/O. An unknown
  placeholder fails closed.

## Semantics Summary

| When | Who executes | What can run | Effect on the document |
| --- | --- | --- | --- |
| On demand (`step run`) | binary, invoked by operator or agent | any registered step | none (Phase 1) |
| Preflight (Phase 7) | binary | `read` steps with fresh-or-run receipts | contract fields only |
| After a ledger fact (Phase 5) | owning controller, as an owned effect | `read` / `check` steps with `on=` | none directly; emits `StepReceiptRecorded` |
| Closeout (Phase 2) | binary | **nothing runs**; receipts are checked | refuse or allow the commit |
| Queue idle dispatch (Phases 3–4) | supervisor/binary, no agent turn | registered `check` / `read` steps; `write` only per Safety | strike + one result cell, halt, or insert a preset head |
| Signals render (Phase 6) | controller | nothing runs; receipts are projected | binary-owned informational component |

## Safety

The threat model in [10-security](../10-security.md) is a trusted operator and
untrusted content. The invariant this proposal must preserve is that **no
document text can make the binary execute code the operator has not
committed.**

1. **Names, never code.** The document can only reference a registered step
   name. It contains no executable blocks, no argv, and no interpolation.
   Pasted text that says `/step x` can at most run an already-registered,
   reviewed command.
2. **The committed registry is the trust root.** Implicit triggers (closeout
   checks, queue dispatch, preflight bindings) use the registry entry exactly
   as it exists at `HEAD`. A dirty or untracked registry entry runs only via
   explicit `agent-doc step run` with a warning, the same posture as
   org-babel's confirm-before-evaluate.
3. **Provenance gates effects.** A queue `/step` item fires only when the
   op-log actor that wrote the line is `user` (existing provenance from the
   durable operation log in [02-document-format](../02-document-format.md)).
   An agent-inserted or recovery-projected `/step` line is inert until the
   operator confirms it.
4. **`write` steps need explicit operator authorization.** They never run from
   preflight, closeout, Phase 4 escapes, or Phase 5 hooks. In a queue they require the
   operator-authored line itself. Under subagent dispatch they never run in a
   worker. That makes the existing coordinator-only `make install` rule
   expressible as `effect = "write"`.
5. **Authorization reuse.** `requires:` lives on the same preset whose text is
   the operator's authorization (`#waypostauthorization`), so the brief a
   subagent receives carries the required steps verbatim.
6. **Bounded output.** Redacted, tail-bounded rendering, with the full log
   behind an artifact handle.

## Determinism, Replay, And Concurrency

- **Idempotence:** receipts make every implicit trigger replay-safe. A
  repeated preflight, a resumed queue, or a recovered closeout re-reads the
  receipt and never re-executes the step.
- **What is committed:** the registry (by the project), the document text
  (`requires:`, `/step` lines, result cells, signals rendering) through the
  normal commit path. Receipts and logs stay in `state.db` and artifacts as
  evidence, never as policy.
- **CRDT / concurrent editing:** step-originated document writes go through the
  same node-keyed binary write path as other binary-owned components. They
  target either a single appended exchange cell or a binary-owned informational
  component, never operator text. A step whose run changes the session
  document's bytes (a misbehaving argv) is recorded as a failure.
- **Subagents:** a worker in its own worktree produces receipts keyed by *its*
  tree. After integration the coordinator's tree differs, so a Phase 2 gate
  correctly demands a fresh receipt for the integrated tree. This is honest by
  construction and needs no cross-worktree trust.

## Failure Modes Designed Out

| Failure | Handling |
| --- | --- |
| Agent claims tests passed and they did not run | Phase 2 gate: no receipt for this tree → no commit |
| Long step wedges a closeout | closeout only reads receipts |
| Stale pass after more edits | the input fingerprint differs, so the receipt is not reused |
| Registry edited after a run | `entry_sha256` differs, so the receipt is not reused |
| Flaky test re-run until green | receipts record each run, and nothing auto-retries |
| Crash between run and queue strike | receipt lookup before run, `write` steps guarded by `started` rows |
| Step output feeds back into a turn loop | signals are a non-trigger component, and result cells are binary-authored |
| Secret in command output | redaction before render, full log behind a handle |

## Non-Goals

- **No general-purpose language.** No expressions, variables, loops, or
  conditionals in the document. Sequencing stays in `agent:queue` (`after=`,
  `id=`, fences) and `orchestrate --mode dag`.
- **No executable code in documents.** No fenced blocks that run, and no
  tangle. Code lives in the repository and the document names it.
- **No new runtime for steps.** A step is an argv. Prefer calling existing
  tools (`make`, `cargo`, `gh`, `tsift digest-runner`) over building features
  into the step runner.
- **No token-level control** (LMQL/Guidance style). The unit of agentic work
  stays one turn.
- **No model-simulated determinism** (SudoLang/OpenProse style). If a rule
  matters, the binary enforces it.

## Open Questions

- Registry location and name (`agent-doc-steps.toml` at the root versus a
  table in an existing committed config).
- Whether `inputs = "worktree"` should support path narrowing
  (`inputs = ["src/**"]`), as in the dev-check affected-scope receipts.
- Whether Phase 4's `else` should allow a free-text prompt bound to the receipt
  or stay registered-preset-only, which is safer.
