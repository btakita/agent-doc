---
agent_doc_session: 89137ef6-5f05-44dc-a60d-ef56aba98eda
tmux_session: '0'
---


# tmux-router

Extracted from `tasks/agent-doc.md` — tmux pane routing crate.

## Overview

`tmux-router` is a Rust crate (library + CLI) for declaratively routing files to tmux panes. It syncs an editor's split layout to tmux pane arrangement.

## CLI API

```
tmux-router sync --col a.md,b.md --col c.md [--window @1] [--focus a.md] [--session claude]
tmux-router register <file> <pane-id> [--session claude]
tmux-router unregister <file>
tmux-router status                    # show registry + pane health
tmux-router resync                    # prune dead panes from registry
tmux-router focus <file>              # select pane for file
```

## Design Decisions

| Decision | Choice |
|----------|--------|
| Registry | Own file: `.tmux-router/registry.json` |
| File identity | File-path-as-key (no UUID dependency) |
| Library API | `tmux_router::{Tmux, Registry, reconcile()}` |
| Config | `~/.config/tmux-router/config.toml` + CLI flags override |

## Architecture

### What moves to tmux-router

From `agent-doc/src/sessions.rs`:
- `Tmux` struct (tmux command abstraction)
- `SessionEntry` / `SessionRegistry` types
- `IsolatedTmux` test infrastructure
- Pane lifecycle: `pane_alive`, `new_session`, `new_window`, `send_keys`
- Stash window: `find_stash_window`, `ensure_stash_window`, `stash_pane`
- Query methods: `pane_window`, `pane_session`, `list_window_panes`, `list_panes_ordered`

From `agent-doc/src/sync.rs`:
- `Layout` / `Column` types
- `reconcile()` algorithm (attach-first: ATTACH → SELECT → DETACH → REORDER → VERIFY)
- `find_column_pane()`, `find_best_window()`, `join_target()`, `equalize_sizes()`
- `SyncLog` structured logging
- All 25+ integration tests and 3 proptest property-based tests

### What stays in agent-doc

- Frontmatter parsing (`agent_doc_session` → file path mapping)
- Snapshot system
- Agent backends (Claude, Junie)
- Skill/upgrade system
- `claim`/`focus`/`start` commands (thin wrappers calling tmux-router)

### Boundary

agent-doc maps `agent_doc_session` UUID → file path, then calls `tmux_router::sync()` with file paths. tmux-router handles everything from file-path-as-key onward.

## Key Lessons (from 7 iterations of sync reconciliation)

1. **Never break wanted panes** from the target window — scatter-gather destroys working state
2. **No registry dependency for eviction** — break ANY unwanted pane, not just tracked ones
3. **Re-query tmux state before each operation** — cached state goes stale instantly
4. **Attach before detach** — prevents pane selection flicker
5. **All join-pane calls use `-d`** — no intermediate selection changes during reconcile
6. **Same-column-only auto-register** — cross-column donors cause wrong focus
7. **Plugin must validate windows** — stale registry entries cause cascading failures

## Testing Strategy

**Dual-layer coverage — no test deletion during extraction:**

- **tmux-router**: All 25+ reconciliation integration tests + 3 proptests + unit tests move here. These test algorithm correctness independently.
- **agent-doc**: Keep integration tests for the full flow (frontmatter → path mapping → tmux-router). Add contract tests verifying correct API usage. Edge cases discovered in agent-doc get corresponding tests in tmux-router too.

Tests are copied/adapted, not just moved. Both crates must pass independently.

## Extraction Plan

1. ~~Create `tmux-router/` crate in agent-loop workspace (or standalone repo)~~ **Done** — `btakita/tmux-router` on GitHub
2. ~~Move `Tmux`, `Registry`, `reconcile()` + types~~ **Done** — 46 tests pass
3. Update agent-doc to `use tmux_router::*` — **Pending**
4. Ensure all tests pass in both crates — **Pending**
5. Release tmux-router v0.1.0, agent-doc v0.8.0 — **Pending**

## Status

**v0.1.0 extracted** (`4cba1c8`):
- `src/tmux.rs` — `Tmux` + `IsolatedTmux` with all methods
- `src/registry.rs` — `RegistryEntry`, `Registry`, load/save/lookup/prune (parameterized path)
- `src/sync.rs` — `Layout`, `reconcile()`, `sync()` with callback-based `FileResolution`
- 46 tests pass (unit + integration + proptest)
- Added as submodule to agent-loop

**Next:** Wire agent-doc to depend on tmux-router and delegate sync/sessions to the library.

## User

Is the cli interface implemented?

## Assistant

No. tmux-router is currently **library-only** — there's no `src/main.rs` or `[[bin]]` in `Cargo.toml`. The CLI API listed at the top of this document is a design spec that hasn't been built.

All tmux routing is currently done through **agent-doc**, which calls the `tmux_router` library API directly (Rust function calls, not CLI subprocess). The commands like `agent-doc sync`, `agent-doc claim`, `agent-doc focus` are thin wrappers that call `tmux_router::reconcile()`, `tmux_router::Registry`, etc.

**Current status (v0.1.3):**
- Library API: fully implemented and published to crates.io
- 48 tests (integration + proptest + unit)
- Used by agent-doc v0.9.3 as a dependency
- No standalone CLI binary

The CLI binary was listed as "Pending / Not Built" in `tasks/agent-doc.md`. It would be useful for non-Rust consumers or shell scripting, but agent-doc doesn't need it since it links directly.

Want me to implement the CLI?

## User

Yes...how would it work? Could I send shell commands?

It also looks like I can send data to the panes. What would the interaction with a program running in the pane be like in this case?

## Assistant

### CLI Design

The CLI would be a thin wrapper around the existing library API. Two categories:

**Layout management** (what agent-doc already uses via library calls):
```bash
tmux-router sync --col a.md --col b.md --session 0 --window @0
tmux-router register tasks/plan.md %5
# Are we not using claim? (HEAD)
tmux-router status          # show registry + pane health
tmux-router resync           # prune dead panes
tmux-router focus tasks/plan.md
```

**Pane interaction** (new, directly useful from shell):
```bash
tmux-router send %5 "ls -la"              # send shell command to pane
tmux-router send %5 --no-enter "partial"  # send text without pressing Enter
tmux-router capture %5                     # read pane content (stdout)
tmux-router capture %5 --lines 50         # last 50 lines of scrollback
```

### Sending Commands to Panes

`send_keys()` already exists in the library — it types text into a pane literally and presses Enter. Whatever is running in that pane receives the keystrokes as if you typed them.

Tmux already have send? What value would tmux-router have over the stock tmux send?

**Interaction with a shell:**
```bash
tmux-router send %5 "cd /tmp && ls"
# → the shell in pane %5 runs the command (HEAD)
```

**Interaction with Claude Code:**
```bash
tmux-router send %5 "fix the failing test"
# → Claude Code in pane %5 receives the prompt (HEAD)
```

**Interaction with any TUI (vim, htop, etc.):**
```bash
tmux-router send %5 --raw ":wq"    # send to vim
tmux-router send %5 --raw "q"      # send to htop
```

### Reading Pane Output

Not yet implemented, but tmux has `capture-pane`:
```bash
tmux-router capture %5
# → returns the visible content of pane %5 (what you'd see on screen) (HEAD)
# Uses: tmux capture-pane -t %5 -p (HEAD)
```

This enables **scripted workflows**:
```bash
# Send a command, wait, read the result (HEAD)
tmux-router send %5 "cargo test"
sleep 5
tmux-router capture %5 | grep "test result"
```

### The Full Picture

tmux panes are **bidirectional channels**. The program running inside doesn't know or care whether keystrokes come from a human or `send-keys`. So:

| Running in pane | `send` does | `capture` returns |
|----------------|-------------|-------------------|
| bash/zsh | Executes command | Terminal output |
| Claude Code | Submits prompt | Agent response stream |
| vim/neovim | Sends keystrokes | Editor screen |
| python REPL | Evaluates expression | REPL output |
| Any TUI | Sends input | Screen buffer |

This makes tmux-router a **universal agent harness** — you can drive any CLI tool programmatically, including other AI agents, without them needing an API. agent-doc's `send_keys` already uses this for Claude Code prompt injection.

### Implementation Plan

1. Add `src/main.rs` with clap CLI (~150 lines)
2. Add `capture_pane()` to `Tmux` struct (~20 lines)
3. Wire `send`, `capture`, `sync`, `register`, `status`, `resync`, `focus` subcommands
4. Publish as dual crate (library + binary)

~200 lines total. Want me to build it?

## User



## Assistant

Done. agent-doc v0.9.4 released:

- **Cargo.toml**: `tmux-router = "0.2"`, version bumped to 0.9.4
- **pyproject.toml**: synced to 0.9.4
- **crates.io**: published
- **PyPI**: published
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.9.4 (with binary)
- **Installed locally**: `agent-doc v0.9.4`
- **agent-loop**: submodule pointer updated, pushed to main

All 150 agent-doc tests pass (129 unit + 21 integration).

## User


## Assistant

Built the CLI — 48 tests pass, clippy clean. But first, your two questions:

### "Are we not using claim?"

`claim` is an agent-doc concept — it binds a UUID-based `agent_doc_session` to a pane. In tmux-router standalone, there's no UUID system, so `register` is the equivalent (maps file-path → pane). I could add `claim` as an alias for `register` if that's more intuitive, or agent-doc could keep its own `claim` command that calls `tmux-router register` under the hood (which is what it does now via library calls).

### "What value over stock tmux send?"

Three things:

1. **Registry-aware addressing** — send by file name instead of raw pane ID:
   ```bash
   tmux-router send plan.md "cargo test"   # looks up pane from registry
   # vs (HEAD)
   tmux send-keys -t %5 "cargo test" Enter  # must know pane ID
   ```

2. **Literal-by-default** — tmux `send-keys` interprets special key names by default (`C-c`, `Enter`, `Space`), which mangles code/text. tmux-router defaults to literal mode (`-l`) with `--raw` as opt-in for when you actually want key interpretation.

3. **Enter handling** — `send` automatically appends Enter with a 50ms delay (TUI-safe). Stock tmux requires manually adding `Enter` as a separate argument.

So `send`/`capture` wrap tmux's primitives with saner defaults and file-name addressing. For one-off commands, `tmux send-keys` works fine. The value shows up in scripted workflows where you're driving multiple panes by name.

### What was built

- `src/main.rs` (~160 lines) — clap CLI with 8 subcommands: `sync`, `register`, `unregister`, `status`, `resync`, `focus`, `send`, `capture`
- `capture_pane()` on `Tmux` — reads screen buffer via `tmux capture-pane -p`
- `send_keys_raw()` on `Tmux` — sends raw tmux key names (C-c, Escape, etc.)

I should also update `send` to accept file names (registry lookup) in addition to raw pane IDs. Want me to add that + release?

## User

Yes. I'm trying to figure out the use cases of tmux-router standalone.
So let's say someone is using nvim. They bind a tmux pane with a file. The tmux pane can run a program that watches the file.

In the case of agent-doc, the file is a md file that acts as a agent loop interface.

What other programs would tmux-router be instrumental in supporting?
Perhaps a log watcher?

## Assistant

### The Pattern: File → Pane → Program

The generic pattern is: a file changes, a program in a bound pane reacts to it. tmux-router manages the binding and layout. The program in the pane is whatever you want.

**Concrete use cases:**

| Use Case | File | Program in Pane | Interaction |
|----------|------|----------------|-------------|
| **Agent loop** (agent-doc) | `plan.md` | Claude Code | File edits → agent prompt → response appended |
| **Log watcher** | `app.log` | `tail -f` or `lnav` | Real-time log stream bound to a pane |
| **Test runner** | `src/main.rs` | `cargo watch -x test` | File save → tests rerun in bound pane |
| **REPL-driven dev** | `scratch.py` | `ipython` | Edit file, send snippets to REPL via `send` |
| **Build monitor** | `Cargo.toml` | `cargo build --watch` | Dependency changes → rebuild visible in pane |
| **Database console** | `queries.sql` | `psql` / `sqlite3` | Send SQL from file to db console |
| **Jupyter alternative** | `notebook.md` | `python3` REPL | Markdown cells → send code blocks to REPL, capture output |
| **Multi-agent orchestration** | `task-a.md`, `task-b.md` | Multiple Claude Code instances | Route tasks to different agents, each in their own pane |
| **Documentation preview** | `README.md` | `mdcat` or browser refresh | File watch → re-render in pane |
| **Server + client** | `server.rs`, `client.rs` | Two processes | Side-by-side panes, send commands to either |

### The nvim Workflow

A nvim user could:

```bash
# Open project — layout mirrors nvim splits (HEAD)
tmux-router sync --col src/main.rs --col tests/main_test.rs

# Pane 1: cargo watch (auto-test on save) (HEAD)
# Pane 2: tail -f logs/app.log (HEAD)

# From nvim, send the current file's test to run: (HEAD)
tmux-router send tests/main_test.rs "cargo test test_specific_case"

# Read the test output: (HEAD)
tmux-router capture tests/main_test.rs --lines 20
```

The key insight: **nvim already has terminals, but they're inside nvim's process**. tmux panes are independent — they survive nvim restarts, can be shared across editors, and can be driven by scripts without nvim running.

### What Makes tmux-router Different From Just Using tmux?

Raw tmux gives you panes but no **semantic addressing**. You have to track pane IDs (`%5`, `%12`) yourself. tmux-router adds:

1. **File-to-pane binding** — address panes by the file they serve
2. **Layout reconciliation** — declare desired layout, tmux-router figures out the joins/breaks/reorders
3. **Registry persistence** — bindings survive across sessions
4. **Health management** — dead panes get pruned, stale bindings cleaned up

It's the difference between "I have 6 tmux panes" and "I have a pane for my test runner, a pane for my log watcher, and a pane for my REPL."

### Also: File-name addressing is now live

Updated `send` and `capture` to accept file names from the registry:

```bash
tmux-router send plan.md "cargo test"      # resolves plan.md → %5 from registry
tmux-router capture plan.md --lines 20     # same
tmux-router send %5 "cargo test"           # raw pane IDs still work
```

## User

Implement. Also document use cases.

## Assistant

Done. Two things shipped:

### 1. File-name addressing in `send` and `capture`

`resolve_target()` checks if the target starts with `%` (raw pane ID) or looks it up in the registry. Works for both `send` and `capture`:

```bash
tmux-router send plan.md "cargo test"     # registry lookup
tmux-router send %5 "cargo test"          # raw pane ID
tmux-router capture plan.md --lines 20    # registry lookup
```

### 2. README with documented use cases

Updated `tmux-router/README.md` with full CLI reference and 6 use cases:

- **Test runner** — `cargo watch` in a bound pane, capture results
- **REPL-driven dev** — send code to IPython from your editor
- **Log watcher** — `tail -f` bound to a log file
- **Database console** — send SQL to `psql` by file name
- **Multi-agent orchestration** — multiple Claude Code instances, each bound to a task file
- **Scripted workflows** — bash scripts using `send` + `capture` for automation

All 48 tests pass, clippy clean. Ready to bump version and release when you want.

## User

Release tmux-router and agent-loop

## Assistant

Released both.

### tmux-router v0.2.0

- **crates.io**: published (`cargo install tmux-router` now gets the CLI)
- **GitHub Release**: https://github.com/btakita/tmux-router/releases/tag/v0.2.0 (with prebuilt binary)
- **Tag**: `v0.2.0` pushed

Version bumped from 0.1.3 → 0.2.0 (new feature: CLI binary).

### agent-loop

- Submodule pointer updated to tmux-router `v0.2.0` (`f2d3490`)
- Pushed to `main`

Note: you said "agent-doc" but I released agent-loop (the parent repo with submodule pointer). agent-doc's `Cargo.toml` still points to `tmux-router = { path = "../tmux-router", version = "0.1" }` — it will need a version constraint bump to `"0.2"` and a new release if you want it to use the CLI features. Want me to update agent-doc too?

Yes

## User

I did not see the follow up response in this document.
Is this a bug with tmux-router or agent-doc?

Looks like this file became unclaimed as well.

## Assistant

This is a **Claude Code skill execution bug**, not a tmux-router or agent-doc crate bug.

**What happened:** You wrote "Yes" inline (line 398) confirming the agent-doc update. I processed it and released agent-doc v0.9.4 — but while I was writing the response back to this file, a second `/agent-doc tasks/agent-doc.md` message arrived. I context-switched to handle agent-doc.md's "compact this file" request, and the write-back to tmux-router.md was lost.

**The release did happen:**
- agent-doc v0.9.4 — published to crates.io, PyPI, GitHub Release
- `tmux-router = "0.2"` in agent-doc's Cargo.toml
- agent-loop submodule pointer updated
- Installed locally

The bug: when two `/agent-doc` invocations arrive in quick succession, the first document's response write-back can get dropped if the agent context-switches to the second document before completing step 4 (Edit + snapshot update).

**Regarding the unclaimed file:** When I `cd`'d into the agent-doc submodule to run `cargo build`/`cargo publish`, the CWD shifted. tmux routing is window-scoped — if the skill execution crossed tmux pane boundaries or the claim was session-scoped to the old CWD, the claim could have been lost.

**Fixes needed:**
1. The skill should complete all 5 steps (respond, write-back, snapshot) for each document before handling the next `/agent-doc` invocation
2. Claims should be resilient to CWD changes during skill execution

## User

Can the agent-doc executable help with tracking the context of n tasks? Or is this entirely something that the skill needs to track?

Also, please look at the newly released Anthropic Skill Creator...with the skill metrics. Is this something that can help?

## Assistant

### Can agent-doc track N task contexts?

Both the executable and skill need to cooperate. The bug was: skill started document A's write-back, got interrupted by document B, never finished A. Here's how they'd split responsibility:

**What the executable can provide:**

1. **Execution lock** (`agent-doc lock <file>`) — filesystem lock preventing concurrent skill execution. Second invocation returns "busy, document X in progress."
We may want to have subagents instead. What if the skill always launches subagents? Something to watch out for would be concurrency. Perhaps the skill can also decide whether or not to use a subtree?
2. **Task queue** (`agent-doc queue add <file>`) — ordered queue in `.agent-doc/queue.json`. Skill processes one document per cycle, checks queue after completion.
3. **Pending write tracking** (`agent-doc pending`) — record that a document's response hasn't been written back yet. Skill checks on startup and completes pending writes first.

**What the skill must enforce:**

- Atomic document cycles: steps 1-5 (read, diff, respond, write-back, snapshot) must complete before accepting the next `/agent-doc` invocation
- The skill can't truly block incoming messages (Claude Code delivers them), but it can defer processing by checking the lock/queue

**Simplest fix:** Add a "pending response" file (`.agent-doc/pending/<hash>.md`) written before step 3 (respond) and deleted after step 4 (write-back). On next invocation, the skill checks for pending writes and completes them first.

I effectively want the left or right side to take care of running concurrent agents that all respond back to the document they were called with.

No tasks should be lost. Use subagents if necessary. Orchestration needs to be properly handled. Some tasks will depend on others to complete first. These should be properly orchestrated.

### Anthropic Skill Creator — Yes, Directly Applicable

The [Skill Creator](https://claude.com/blog/improving-skill-creator-test-measure-and-refine-agent-skills) (March 2026) adds eval/benchmark/improve modes with metrics:

**How it helps us:**

1. **Eval mode** — define test prompts for the agent-doc skill:
   - "Submit document A, verify response written back"
   - "Submit document A then immediately B, verify both get responses"
   - "Submit with inline annotation, verify annotation addressed"
   - Each gets pass/fail + token usage + elapsed time

2. **Benchmark mode** — A/B test skill versions:
   - Current skill vs. skill with pending-write protection
   - Measure: write-back success rate, response latency, token cost

3. **Improve mode** — analyze failures and suggest skill edits:
   - The context-switch bug would show up as a consistent failure pattern
   - Skill Creator suggests fixes (e.g., "add completion check before processing next document")

4. **Trigger optimization** — tune skill description for reliable activation:
   - Our skill triggers on `/agent-doc` which is already explicit, but if we add auto-trigger on file save, description precision becomes critical

**Next steps:**
- Install Skill Creator: add to `.claude/skills/`
- Write eval suite for the agent-doc skill (~10 test cases)
- Run benchmark: current skill vs. pending-write-protected version
- Use metrics to validate the fix before releasing

Want me to set up the Skill Creator eval suite for agent-doc?

Yes

## User



## Assistant

### Subagent Architecture — The Right Fix

You're right that subagents are the correct solution, not locks/queues. The core insight: **every `/agent-doc` invocation should spawn a subagent that owns its document cycle end-to-end**.

**Proposed architecture:**

```
User: /agent-doc plan.md
Skill: spawn subagent → "process plan.md document cycle"
       → subagent runs steps 1-5 autonomously
       → main context immediately free

User: /agent-doc corky.md (while plan.md is still running)
Skill: spawn subagent → "process corky.md document cycle"
       → runs in parallel, no interference
```

**Concurrency model:**
- **Independent documents** → parallel subagents (no coordination needed)
The parallel subagents to make documents independent would be good. 
- **Same document** → serial (file lock via `.agent-doc/locks/<hash>.lock`)
- **Dependent tasks** (e.g., "release A then B") → skill detects dependency, chains subagents: spawn A, on completion spawn B

Sometime I revise a request on the same document. I want the task to pick it up before completing it's currently running operation. The llm can decide. I think Claude already does a good job with "interruptions" on a single document. I dont see a need to lock. Do you?

**What the skill decides:**
1. Is this a simple document submit? → spawn subagent immediately
2. Does this require shared state (release, cross-document work)? → orchestrate: spawn A, wait, spawn B
3. Does this reference another in-progress task? → check `.agent-doc/active/` for running subagents, coordinate

**What agent-doc executable provides:**
- `agent-doc active` — list in-progress document cycles (PID + file + start time)
- `agent-doc lock <file>` / `agent-doc unlock <file>` — file-level coordination
- `.agent-doc/active/<hash>.json` — written on cycle start, deleted on completion

**Benefit:** The main context becomes a pure orchestrator — it reads user intent, decides what to parallelize vs. serialize, spawns subagents, and reports results. No risk of losing write-backs because each subagent's context is isolated.

### Skill Creator Eval Suite

I've reviewed the [Skill Creator](https://github.com/anthropics/skills/tree/main/skills/skill-creator) format. It expects:
- `evals/evals.json` — test prompts + assertions
- Subagent-based execution (with-skill vs. without-skill)
- Results in `<skill>-workspace/iteration-N/eval-N/`

**Test cases for agent-doc skill:**

| ID | Name | Prompt | Asserts |
|----|------|--------|---------|
| 0 | `single-submit` | Submit doc with new User block | Response written to file, snapshot updated |
| 1 | `empty-submit` | Submit unchanged doc | "Nothing changed" message, no file edit |
| 2 | `inline-annotation` | Submit doc with inline annotation in Assistant block | Annotation addressed in response |
| 3 | `comment-ignored` | Add HTML comment only | No response triggered |
| 4 | `concurrent-submit` | Submit doc A, immediately submit doc B | Both docs get responses written back |
| 5 | `git-commit` | Submit doc in git repo | `agent-doc commit` called before response |
| 6 | `snapshot-persistence` | Submit, verify snapshot matches doc state | SHA256 path correct, content matches |
| 7 | `cwd-resilience` | Submit while CWD is a submodule | Write-back uses absolute paths, succeeds |
| 8 | `claim-workflow` | `/agent-doc claim file.md` | `agent-doc claim` executed, no doc processing |
| 9 | `diff-accuracy` | Edit middle of doc, submit | Only the changed section triggers response |

This is a plan — the actual eval infrastructure needs the Skill Creator skill installed to run. Want me to:
- **A)** Install Skill Creator and run the full eval suite now
- **B)** Focus on implementing the subagent architecture first (the fix), then eval it
- **C)** Both — implement subagent architecture, then eval it with Skill Creator

Recommendation: **B then eval** — fix the architecture first, then use Skill Creator to prove it works.

## User

Yes...Option B with no lock...unless you can explain a compelling reason for a lock.

## Assistant

No compelling reason for a lock. You're right on all counts:

1. **Same-document revision** — the subagent re-reads before write-back, picking up your edits naturally
2. **Parallel documents** — independent subagents, no shared state to corrupt
3. **Interruption pattern** — Claude already handles "user edited during response" via the re-read-before-write step

Lock removed from the design. The only coordination primitive is `.agent-doc/active/<hash>.json` — not for locking, just for the skill to know "a subagent is already processing this file, skip spawning another."

Will the file be deleted when all tasks are completed for the claimed file?
Or will the file be kept?

### Implemented: Subagent Delegation in SKILL.md

Updated `.claude/skills/agent-doc/SKILL.md` with the subagent execution model:

**Key changes:**
- Added "Execution Model — Subagent Per Document" section
- Every `/agent-doc <FILE>` invocation MUST delegate to a subagent via the Agent tool
- Pre-flight checks (claim, version, claims log) stay in main context
- Steps 1-5 (read, diff, respond, write-back, snapshot) run in the subagent
- Same-document re-submit: subagent picks up revisions on re-read, no lock
- No `.agent-doc/locks/` directory — removed entirely

**Why this fixes the bug:** The Agent tool runs to completion. When a second `/agent-doc` message arrives, it queues as a system reminder. The first subagent finishes its full cycle (including write-back and snapshot), THEN the main agent processes the second message.

The next `/agent-doc` invocation will use the new subagent pattern. Want to test it now?

yes

## User

## Assistant

### Active file lifecycle

The `.agent-doc/active/<hash>.json` file is **ephemeral** -- it exists only while a subagent is processing a document cycle. Created when the subagent starts, deleted when it completes (after write-back and snapshot
(update). It is NOT the same as the claim.

The two primitives serve different questions:

| Primitive | Question it answers | Lifecycle |
|-----------|-------------------|-----------|
| **Claim** (registry) | "Which pane owns this file?" | Persists across sessions until explicitly released |
| **Active marker** (`.agent-doc/active/<hash>.json`) | "Is someone working on this right now?" | Created on cycle start, deleted on cycle end |

So to answer directly: **yes, the active file is deleted** when the subagent finishes processing the document. It does not persist. If you check `.agent-doc/active/` and it's empty, no subagents are currently running. If a file exists there, a cycle is in progress.

The claim (registry entry binding file to pane) survives independently -- it only gets removed by `agent-doc unclaim` or `tmux-router unregister`.

Could we have the active marker be in the claim primitive?
One edge case is what happens when a task is running, the agent document is claimed by another tmux pane.

I have observed ambigious behavior when running a document that was previously claimed by another session that happened before the machine restarted.
It seems that no action is taken, but I'm not 100% sure.

If the document is claimed and then rerun, it seems to work normally.

### Testing the subagent pattern

This response IS the test. It was generated by a subagent processing this document's cycle. If you're reading this in the document with the snapshot updated, the subagent pattern works -- the full cycle (read, diff, respond, write-back, snapshot) completed atomically within the subagent context.

This is the first invocation using the new subagent delegation model from SKILL.md.

## User



## Assistant

No new content detected in this submission, but you have three unanswered inline questions from the previous cycle (embedded in the Assistant block around the active marker discussion). Addressing them now:

### 1. Could the active marker live in the claim primitive?

Yes, and it would simplify things. Instead of two separate systems (registry claim + `.agent-doc/active/<hash>.json`), the registry entry itself could carry an `active: bool` or `active_since: timestamp` field:

```json
{
  "file": "tasks/tmux-router.md",
  "pane": "%5",
  "active": true,
  "active_since": "2026-03-06T10:30:00Z"
}
```

**Advantages:**
- Single source of truth -- no orphaned active markers if a subagent crashes
- `agent-doc status` shows both claim and processing state in one view
- Simpler cleanup: `resync` already prunes dead panes, it can also clear stale `active` flags

**One concern:** the registry is file-based (JSON). Two subagents writing to it concurrently could race. But since same-document submissions are serial (no lock, just "subagent re-reads"), only cross-document parallelism exists, and each touches a different registry entry. Safe enough.

Could there be a race condition. Such is with rapidly running commands in sequence?
Would last write wins be safe? Could there be 2 concurrent agent runs due to a race condition?

Recommendation: merge active marker into the claim. Drop `.agent-doc/active/` entirely.

### 2. Edge case: document claimed by another tmux pane while a task is running

Current behavior: the second `claim` would overwrite the registry entry, pointing the file to a new pane. The first subagent (still running) would write its response back to the file (it uses the file path, not the pane), but the snapshot update and any tmux focus operations would target the old pane.

**Desired behavior options:**
- **A) Last-write-wins** (current) -- second claim takes over, first subagent's response still lands in the file but pane focus goes to the new pane
- **B) Reject** -- if a subagent is active on this file, refuse the re-claim until it completes
- **C) Transfer** -- re-claim succeeds, notify the running subagent to redirect its tmux operations to the new pane

Option A is probably fine. The file is the important artifact, not the pane. The response gets written regardless of which pane "owns" the file.

### 3. Stale claims after machine restart

This is likely the root cause of the "ambiguous behavior." After a reboot:
- tmux server restarts -- all pane IDs (`%5`, `%12`) are reassigned
- The registry still has old pane IDs from the previous session
- `agent-doc` tries to use the stale pane ID, which either doesn't exist or belongs to a different pane

**What should happen:**
- On first invocation after restart, `agent-doc` (or tmux-router) should detect that the claimed pane is dead (`pane_alive()` returns false) and auto-prune the stale entry
- Then re-claim with the current pane
Yes

**What likely happens now:** the stale claim exists, `pane_alive()` fails silently somewhere, and the system falls through to a no-op or error path without clear feedback. "No action taken" matches this -- the code sees a claim exists, doesn't verify the pane is alive, and skips re-claiming.

**Fix:** Add a `validate_claims()` step at session start that prunes dead pane entries. This could be automatic in `agent-doc claim` -- if the existing claim points to a dead pane, silently replace it.

Implement.

Want me to implement any of these (merge active into claim, startup validation, stale claim pruning)?

## User



## Assistant

### Race condition analysis

The registry is a JSON file with read-modify-write semantics. There IS a theoretical TOCTOU race: agent A reads, agent B reads, agent A writes, agent B writes (clobbering A's entry). In practice this is unlikely because:

- Each subagent touches a **different registry key** (keyed by session UUID), so concurrent writes only risk clobbering if they overlap at the file level
- `std::fs::write` does an atomic write on Linux (write to page cache, single rename-like fsync) — not as safe as write-to-temp-then-rename, but adequate for low-contention use
- Subagent lifecycles are staggered (claim happens once at session start, not continuously)

**Would last-write-wins be safe?** Yes, for the current design. The worst case is one agent's registry entry gets dropped and silently re-added on next claim. No data corruption, just a transient missing entry.

**Could there be 2 concurrent agent runs due to a race condition?** No — each subagent owns its own session UUID and pane. The registry maps session→pane, so two agents would write different keys. Even if one clobbers the other's JSON write, the "loser" re-claims on next invocation. For true correctness, `flock`-based advisory locking would eliminate the window entirely, but the collision probability is low enough that it's not worth the complexity now.

Let's have a `flock` based advisory. Also mitigate for windows.
Would `flock` fix the TOCTOU race?

### Stale claim auto-prune: implemented

The `resync::prune()` call at the top of `agent-doc claim` already removed dead-pane entries from the registry. What was missing was **file-specific feedback** — when a stale claim existed for the exact file being claimed, it was silently pruned with no indication to the user.

**What I added:** A `validate_file_claim()` function in `claim.rs` that runs after `resync::prune()`. It:

1. Loads the registry and checks for entries matching the file being claimed
2. Tests each matching entry's pane via `pane_alive()`
3. If dead, logs a clear message: `stale claim: <file> was bound to dead pane %X, replacing`
4. Removes the stale entry and saves

This handles the machine-restart scenario where tmux pane IDs are reassigned: the user now sees exactly why a re-claim was needed instead of the previous silent behavior.

**File changed:** `/home/brian/work/btakita/agent-loop/agent-doc/src/claim.rs`

Build and all 150 tests pass (`cargo build` + `cargo test`).

## User


## Assistant

### Does `flock` fix the TOCTOU race?

Yes, completely. The TOCTOU (time-of-check-to-time-of-use) race is: Agent A reads registry, Agent B reads registry, Agent A writes, Agent B writes (clobbering A). With `flock`, Agent A acquires an exclusive lock, then reads, modifies, writes, and releases. Agent B blocks on the lock until A is done. The read-modify-write becomes atomic with respect to other cooperating processes.

### Cross-platform strategy

- **Linux/macOS:** `flock()` syscall
- **Windows:** `LockFileEx` syscall
- **Abstraction:** The `fs2` crate (0.4) provides `FileExt::lock_exclusive()` and `FileExt::unlock()` that dispatch to the correct OS primitive on all three platforms. Single dependency, no conditional compilation needed.

### Implementation

Added `fs2 = "0.4"` to `tmux-router/Cargo.toml` and implemented a `RegistryLock` RAII guard in `tmux-router/src/registry.rs`:

- **`RegistryLock::acquire(registry_path)`** -- opens/creates `<path>.lock` sibling file, calls `lock_exclusive()` (blocks until available), returns a guard
- **`Drop` for `RegistryLock`** -- explicitly calls `unlock()` (also released on file close)
- **`with_registry(path, closure)`** -- convenience: acquire lock, load, run closure on `&mut Registry`, save, release. Eliminates TOCTOU by holding the lock across the entire read-modify-write
- **`with_registry_val(path, closure)`** -- same but returns a value from the closure

All read-modify-write call sites now hold the lock:

| Function | File |
|----------|------|
| `register_full()` | `agent-doc/src/sessions.rs` |
| `validate_file_claim()` | `agent-doc/src/claim.rs` |
| `update_window_for_entry()` | `tmux-router/src/registry.rs` |
| `prune()` | `tmux-router/src/registry.rs` |

Read-only callers (`lookup`, `load` for display, `prompt`, `watch discover`) don't need the lock -- they tolerate slightly stale data and don't write back.

**Files changed:**
- `/home/brian/work/btakita/agent-loop/tmux-router/Cargo.toml` -- added `fs2 = "0.4"`
- `/home/brian/work/btakita/agent-loop/tmux-router/src/lib.rs` -- exported `RegistryLock`, `with_registry`, `with_registry_val`
- `/home/brian/work/btakita/agent-loop/tmux-router/src/registry.rs` -- `RegistryLock` guard, `with_registry`/`with_registry_val` helpers, locking in `update_window_for_entry` and `prune`
- `/home/brian/work/btakita/agent-loop/agent-doc/src/sessions.rs` -- locking in `register_full`, re-export `RegistryLock`
- `/home/brian/work/btakita/agent-loop/agent-doc/src/claim.rs` -- locking in `validate_file_claim`

Build and all 150 tests pass (`cargo build` + `cargo test` in both tmux-router and agent-doc). The lock is re-entrant safe on the same thread (advisory locks are per-process on Unix), so nested calls from `register_full` calling `load`/`save` work correctly.

## User

release tmux-router, agent-doc

## Assistant

Released both packages with the flock advisory locking changes.

**tmux-router v0.2.1:**
- Committed: `RegistryLock` RAII guard, `with_registry`/`with_registry_val` helpers, locking in `update_window_for_entry` and `prune`, `fs2` dependency
- Published: crates.io, GitHub Release (https://github.com/btakita/tmux-router/releases/tag/v0.2.1)

**agent-doc v0.9.5:**
- Committed: registry locking in `register_full` and `validate_file_claim`, stale claim auto-pruning, `RegistryLock` re-export, tmux-router bumped to 0.2.1
- Published: crates.io, PyPI, GitHub Release with prebuilt binary (https://github.com/btakita/agent-doc/releases/tag/v0.9.5)
- Installed locally: `~/.cargo/bin/agent-doc` replaced v0.9.4 with v0.9.5
- `pyproject.toml` version synced to 0.9.5

**agent-loop:** Submodule pointers updated and pushed.

All 48 tmux-router tests and 150 agent-doc tests pass. Clippy clean.

## User

