# agent-doc

Interactive document sessions with AI agents.

Edit a markdown file, press a hotkey, and the tool diffs your changes, sends them to an AI agent, and writes the response back into the document. The document is the UI.

> **Alpha Software** — actively developed; APIs and frontmatter format may change between versions.

> **Single-user only.** agent-doc operates on the local filesystem with no access control. Use a private git repository. See the [Security](#security) section for details.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/btakita/agent-doc/main/install.sh | sh
```

**Alternatives:**

```sh
# From crates.io
cargo install agent-doc

# From PyPI
pip install agent-doc

# From a local source checkout
cargo install --path src/agent-doc --force

# Or from inside src/agent-doc
cargo install --path . --force
```

## Quick Start

```sh
# 1. Initialize project (creates .agent-doc/ and installs SKILL.md)
agent-doc init

# 2. Scaffold a session document
agent-doc init session.md "My Topic"

# 3. Claim the document to the current tmux pane
agent-doc claim session.md

# 4. Route hotkey triggers to the correct tmux pane
agent-doc route --dispatch-only session.md

# 5. Run: diff, send to agent, write response back
agent-doc run session.md
```

The typical edit cycle: write in your editor, trigger `agent-doc route --dispatch-only <file>` via a hotkey, and agent-doc dispatches the correct harness-specific trigger through the owning supervisor into the right pane. Claude Code panes receive `/agent-doc <file>`; Codex panes receive plain `agent-doc <file>`. That editor path always stays a bounded reopen send; it does not restart Codex just because the previous prompt was `/clear`.

## Key Features

- **Template mode** — named component regions (`<!-- agent:name -->`) updated independently; inline attrs (`patch=`, `max_lines=`) > `components.toml` > built-in defaults
- **CRDT merge** — yrs-based conflict-free merge for concurrent edits between agent writes and user edits
- **IPC-first writes** — socket IPC (Unix domain sockets); editor plugin receives JSON patches instead of file overwrites; preserves cursor position, undo history, and avoids "externally modified" dialogs
- **Tmux routing** — persistent per-document agent sessions; `route` dispatches to the correct pane or auto-starts one using the active harness's trigger shape (`/agent-doc` for Claude Code, plain `agent-doc` for Codex); reconciler always runs (no early exits) handling 0/1/2+ panes uniformly
- **Managed reroutes keep one tmux submit boundary** — once a Claude/Codex session is running under `agent-doc start`, route keeps the managed acceptance/queue paths on supervisor IPC, but dispatch-only editor reroutes and file-scoped `/clear` now submit through the live pane's tmux input path instead of mixing direct PTY writes and supervisor-only reopen delivery. Tmux-bound command injects are normalized once and submitted once, so editor reroutes, `/clear`, queue-dispatch, and restart handoffs all share one literal-text submit followed by a short delayed `Enter` tmux boundary without follow-up synthetic `Enter` retries
- **Fresh reroutes keep the new pane authoritative** — after route creates a fresh pane for a document, later same-session registry churn cannot hand the reopen back to an older pane and make the new pane disappear from the `agent-doc` window
- **Codex forwarded `Ctrl+C` and EOF/Ctrl-D now always hand control back to the operator** — after a stdin-forwarded `Ctrl+C` that terminates the child, or a forwarded stdin EOF/Ctrl-D, agent-doc returns to an explicit canonical `Enter`/`q` prompt mode so the operator can intentionally restart fresh or exit the supervisor cleanly even when the parent harness keeps stdin in raw-ish mode, including immediately after a successfully committed turn
- **Only genuinely promptless clean exits auto-recover** — if a fresh/fresh-restart Codex child clean-exits before it ever surfaces an idle prompt and no operator `Ctrl+D` was forwarded, agent-doc still treats that as failed startup provenance and restarts fresh automatically instead of stopping for the quit prompt
- **Dispatch-only reroutes now stay on the same live tmux submit path as file-scoped clear during the boot window** — `agent-doc route --dispatch-only <file>` still ends with a one-shot bare reopen, but when it already has a live pane-bound session it now uses that same direct tmux submit helper even if the latest run is still in the fresh-start window, instead of surfacing a second `still booting` gate that `session clear` never had
- **Dispatch-only reroutes keep post-clear editor runs on the live session** — after `agent-doc session clear <file>`, the next editor-triggered `agent-doc route --dispatch-only <file>` still sends the bare `agent-doc <FILE>` reopen through that same live pane's tmux input path; only the managed non-dispatch route keeps the tracked `/clear` fresh-restart policy
- **Dispatch-only reroutes now absorb same-file restart handoffs before surfacing a boot-window refusal** — if the first starting-pane ready probe times out but the supervisor immediately restarts the same session in-place or hands it to a new pane for the same file, route follows that newer run instead of pinning the stale pane and surfacing a false `still booting` error to the editor
- **Dispatch-only reroutes now fail closed on cross-file pane reuse before any rebind lands** — if the candidate pane is still registered to another document, route refuses the reopen instead of transiently superseding that other file's pane and then repairing the registry afterward
- **Optimistic fresh-restart retries stay traceable** — if a routed Codex fresh restart never gets back to a dispatch-ready prompt, agent-doc records a `startup_miss` on the original routed pane with the canonical absolute document path instead of silently redirecting the retry through a replacement pane
- **Required SSH drift detection now covers bare socket EPERM too** — when a document declares `required_ssh_targets`, resumed Codex recovery treats `command_execution` output like `socket: Operation not permitted` as required-SSH capability drift when the same event proves SSH command context for one of those targets; localhost/CDP EPERM remains on the separate browser-capability retry path
- **Passive mixed-root sync stays fail-safe** — when `sync --no-autostart` leaves any visible file blocked, agent-doc preserves the current visible `agent-doc` window layout and warns instead of collapsing the remaining foreign pane set into a new authoritative layout
- **Sync now refuses editor-provided non-`agent-doc` tmux windows** — if the editor passes a tmux window target that is not actually an `agent-doc` window and the target session has no discoverable `agent-doc` window by name, sync fails closed and preserves layout instead of reconciling remembered docs onto the wrong window
- **Normal route/start/sync no longer re-elect owners from legacy pane heuristics** — the normal path now trusts only the authoritative actor record and the supervisor-backed registered binding. Session-log owners, `registry_rebind` successors, and generic same-file process-tree matches remain diagnostics/repair signals, not owner-election inputs.
- **Passive sync now fails closed on legacy associated panes instead of reclaiming them implicitly** — when `sync --no-autostart` sees only session-log/process-tree/`registry_rebind` evidence for a pane, it blocks that file and requires an explicit claim/repair instead of silently re-registering the pane or cold-starting around ambiguous ownership.
- **Route readiness is binary-owned** — prompt detection / trigger acceptance lives in `route.rs` and must tolerate shell startup noise; the skill should not try to infer pane readiness from echoed command text
- **Route progress logging is UTF-8 safe** — live reroute diagnostics trim captured tmux lines on char boundaries, so Unicode prompt/status glyphs such as `…` or `·` cannot panic the binary while route is waiting for readiness
- **Harness-specific arg aliases** — `agent_args` is the generic override; `claude_args` and `codex_args` are harness-specific aliases used only by their matching backends; submodule-hosted documents also auto-add the superproject working tree plus any external git metadata directories they need (`.git/modules/...` plus the superproject `.git`) so workspace-write sessions can patch parent-repo docs and still complete the normal git lifecycle
- **Streaming** — real-time CRDT write-back loop (`agent-doc stream`) with optional chain-of-thought routing
- **Task orchestration** — `agent-doc orchestrate --mode sequential|parallel|dag` gives one surface for stepwise fresh-agent execution, worktree fan-out, and dependency-aware DAG scheduling; sequential injects each prompt into `exchange`, streams step responses into the document on CRDT docs when the backend supports it, and still closes each shared-document step with `finalize` + `session-check`; parallel preserves the existing worktree backend, and DAG mode honors `after=` dependencies in deterministic topological order
- **Editor plugins** — JetBrains and VS Code plugins for hotkey integration and IPC writes
- **Watch daemon** — auto-submit on file change with debounce and reactive mode for stream documents
- **Linked resources** — `links` frontmatter field for local files and URLs; URL content fetched, converted HTML→markdown via `htmd`, cached, and diffed on each preflight
- **Session logging** — persistent logs at `.agent-doc/logs/<session-uuid>.log` for debugging session crashes and restarts
- **Preflight gate** — `agent-doc preflight` auto-recovers open `response_captured` / `write_applied` cycles, treats `staged snapshot already matches HEAD` as an already-committed no-op closeout instead of `commit_failed`, repairs stale `preflight_started` locks when the recorded snapshot/file hashes still match exactly, otherwise still requires a real pending/capture replay before auto-closing `preflight_started`, fails closed on unrecoverable `preflight_started` drift instead of snapshot-committing over newer live content, also fails closed on hidden uncommitted closeout drift (`snapshot != HEAD` / visible bypassed `### Re:` with no recoverable cycle) before diffing, and emits `effective_tier`, `required_tier`, `suggested_tier`, `model_switch(_tier)`, and `agent_model` for the skill
- **Durable response capture** — every final response is persisted to `.agent-doc/captures/<doc-hash>/<cycle-id>.json` before write-back, so interrupted cycles can replay the exact response body instead of regenerating it; lifecycle timestamps such as `replayed_at` and `committed_at` preserve whether a response only patchbacked later during recovery
- **Template exchange guard** — template writes fail closed when prompt/response content escapes either below `<!-- /agent:exchange -->` or into the gap before later agent components such as backlog; explicit repair/compaction still normalize the safe escaped-conversation shapes, while comment-only notes without escaped conversation headings stay outside and ambiguous mixed structure is rejected
- **Respect manual cleanup** — if a user deletes that escaped conversation tail by hand in a template doc, `agent-doc repair` now discards the stale captured response and advances the snapshot to the repaired file instead of failing on a capture-baseline mismatch or reapplying the removed tail
- **`repair` closes git-backed recovery immediately** — direct `agent-doc repair` (legacy alias: `recover`) now runs the normal commit boundary after replaying or deduping a pending response in git, instead of leaving repaired assistant content uncommitted until a later `preflight`
- **Hard response commit boundary** — every appended response should cross a commit boundary unless the operator explicitly wants it left open; the normal happy path is `agent-doc finalize <file>`, while missed-patchback repair uses `agent-doc write --commit <file>`
- **Manual repo commit ordering keeps the session doc on the finalize path** — when an `agent-doc` turn includes ordinary repo `commit + push`, manual git commits should stage only the intended non-session repo files, stop immediately on any stage failure, verify the staged diff still matches that intended path set, and commit only that validated set. The session document itself still closes through `agent-doc finalize <file>` or `agent-doc write --commit <file>`, and the push happens after that closeout commit lands so the response commit is included
- **Harness-native entrypoints are binary-owned workflow starts** — `/agent-doc <file>` in Claude Code, `agent-doc <file>` in Codex/direct-exec, and equivalent harness-native entry forms must be treated as the start of the binary-managed response cycle, not as permission to patch the document manually and later say "not committed"
- **Session-document `write --commit` is strict** — when `write --commit` is used to patch back a response into a real session document (`agent_doc_session` / legacy `session`), it now requires git before mutation and only succeeds once the cycle reaches `committed`; best-effort `--commit` remains only for non-session docs and `--pending-only` maintenance
- **Mode-aware bare/run entrypoint** — `agent-doc <file>` and `agent-doc run <file>` now share the same document-mode-aware path: template docs use template patchback, append docs use inline blocks, and missing format defaults to template
- **Run closeout is durable before commit** — once `run` has written the final response (and any `resume` update), it records `write_applied` before the post-write commit so interrupted runs can be finished deterministically by `preflight`; git-backed `run` now shares the same strict post-write closeout helper as `finalize`, `write --commit`, `repair`, and the Codex Stop-hook, so success also requires the snapshot/`HEAD` proof plus `session-check`
- **Binary-owned finalize path** — `agent-doc finalize <file>` is the strict happy-path response command: it reuses the normal write pipeline but requires the document to live in git and fails unless the cycle reaches a terminal committed state
- **Codex post-write guard** — the direct-exec Codex instruction path runs `agent-doc session-check <file>` after `finalize` or manual `write --commit`, and a nonzero check blocks success reporting until recovery finishes; it flags likely direct response patchbacks (`### Re:` / `## Assistant`) that bypassed `agent-doc`, names tracked side-effect files plus the exact `agent-doc write --commit <FILE>` repair path when the closeout is still only in the working tree, fails closed when prompt-bearing user edits exist with no newer cycle start, and still self-heals already-committed historical response drift when `HEAD` proves the response is no longer out-of-band, including committed prompt+response pairs that precede newer local drift as long as that later drift did not add another assistant patchback
- **Optional closeout sidecars are advisory** — `session-check` and related closeout helpers treat late `NotFound` reads for cycle-state, capture, startup-miss, ops-log, pre-response, and CRDT sidecars as absent state instead of surfacing a transient `ENOENT` during otherwise-valid closeout verification
- **Codex Stop-hook and repair replay guard** — `agent-doc skill install` for Codex now also writes `.codex/hooks.json` and enables `features.codex_hooks = true`, so `UserPromptSubmit` tracks the active document even when the submitted prompt body includes injected AGENTS/instruction preambles ahead of the real `agent-doc <FILE>` line, and `Stop` first tries to finish the response cycle deterministically from `last_assistant_message` only when that payload validates as a single assistant closeout; tracked state survives nested `.agent-doc` root / CWD drift and later-stop turn drift within the same Codex session, while transcript-shaped or full-document payloads are kept as diagnostics and blocked instead of being replayed. The same replay-shape guard now fails closed inside `agent-doc repair`, writing blocked payload diagnostics under `.agent-doc/repair-blocked/` rather than appending a full exchange dump back into `agent:exchange`.
- **Response persistence is the close-out boundary** — the instruction surface now requires requested implementation / verification / build-install work to finish before `finalize` or `write --commit`; after that point only `session-check`, recovery, and final reporting remain
- **Imperative document edits execute work** — bundled Claude/Codex instructions now explicitly treat document-local directives like `do #id`, `run tests`, `build + install`, `commit + push`, and pending-item task text that starts with an imperative verb (for example `[#id] Fix the cross-repo ...`) as authorization to perform that repo work from the session document itself; the agent should execute the work or stop on a concrete blocker, not append false-progress status prose. The binary now rejects status-only/meta-only `run` / `finalize` responses for those directive diffs unless they include concrete execution evidence or a blocker.
- **Harness-owned full-suite verification** — agent harnesses must run the full project verification suite explicitly after code, test, build, or instruction-surface changes; `agent-doc` no longer installs a pre-commit hook that runs the whole suite implicitly
- **Model-attributed response headers** — `### Re:` headings should carry the resolved model short name (`gpt-5`, `opus-4-6`), not the harness label (`codex`, `claude`)
- **Git integration** — auto-commit each run; squash history with `agent-doc clean`
- **Commit self-heal** — `agent-doc commit` can absorb a narrowly-scoped missed agent patchback (`status`, `### Re:` response-block insertion, pending-ID superset) into the snapshot before staging, while still leaving plain user prompts uncommitted; when the snapshot lags behind an already-committed response in `HEAD`, `commit` now repairs the snapshot up to that committed `HEAD` state before later local drift can no-op closeout, as long as the later drift did not add another assistant patchback, so stale snapshots cannot rewind the document; when the snapshot already matches `HEAD` but the working tree has later local edits, `commit` now classifies that as post-commit local drift and explains that those edits remain uncommitted instead of vaguely implying a missed patchback, and the follow-up case now explicitly says no new response body was supplied so a second assistant patchback will not be synthesized; a `HEAD`-current staged snapshot still closes the cycle as an explicit no-op instead of a false `commit_failed`
- **Extreme-drift guard stays conservative** — if a tracked document's snapshot is badly stale, `commit` warns but does not re-sync the snapshot from the live file wholesale; bootstrap scaffold auto-resync is limited to files with no `HEAD` entry yet so unanswered prompts cannot be swallowed into a commit
- **Clean post-commit cleanup** — commit strips transient `(HEAD)` markers from the staged snapshot so the committed blob stays clean, and post-commit cleanup now collapses snapshot/editor state back to the same clean boundary shape instead of leaving marker-only working-tree churn behind
- **Manual repair guidance is explicit across harnesses** — bundled skill/runbook content now distinguishes missing-user-prompt insertion from missed-response repair for both Claude Code and Codex: once the prompt is already present, the default documented repair path is `agent-doc write --commit <file>`, not direct file patching or a bare write-back that stops before commit
- **Generated harness instructions stay aligned** — installed Claude/Codex hot-path instructions are rendered from one shared source surface and parity-tested so completion-boundary guidance cannot silently drift between `.claude/skills/agent-doc/SKILL.md` and `.codex/AGENTS.md`
- **Bulk resync** — validates session state and fixes stale/orphaned panes in 2 subprocess calls instead of ~20-40; `--fix --session <name>` relocates WrongSession panes via join-pane instead of killing them
- **Column memory** — `.agent-doc/last_layout.json` remembers column→agent-doc mapping; preserves 2-pane tmux layout when one editor column switches to a non-agent file
- **Stash + rescue** — replaced panes are stashed (alive in background); stash rescue brings them back when the user switches to that document again
- **Startup lock** — `.agent-doc/starting/<hash>.lock` with 5s TTL prevents double-spawn when sync fires twice in quick succession
- **Component-aware baseline guard** — detects stale baselines by comparing append-mode components only; user edits to replace-mode components (status, pending) don't trigger false positives
- **Hook system** — cross-session event coordination via `agent-doc hook fire/poll/listen/gc`; `post_write` / `post_commit` events now include `capture_id` and `response_sha256` when a durable capture exists, the system integrates with Claude Code hooks via `PostToolUse`, and Codex installs a repo-local `UserPromptSubmit` / `Stop` bridge through `.codex/hooks.json`
- **Slash command dispatch** — `preflight` extracts slash commands from user-added diff lines (`parse_slash_commands`); the SKILL executes them before responding; guards exclude code fences, blockquotes, and non-added lines
- **Dedupe stale patch cleanup** — after removing duplicate blocks, `dedupe` also deletes the stale `.agent-doc/patches/<hash>.json` to prevent the plugin's startup scan from re-applying removed content

## Architecture

The binary owns all deterministic behavior: component parsing, patch application, CRDT merge, snapshot management, git operations, tmux routing, and IPC writes. The bundled skill / AGENTS instructions are the non-deterministic orchestrator layer — they read the diff, generate responses, and decide what to write.

**Binary vs. Agent Responsibility:**

| Responsibility | Owner | Why |
|---------------|-------|-----|
| Component parsing, patch application, mode resolution | **Binary** (Rust) | Deterministic, testable, consistent across agents |
| CRDT merge, snapshot management, atomic writes | **Binary** (Rust) | Concurrency safety requires flock + atomic rename |
| Diff computation, comment stripping, truncation detection | **Binary** (Rust) | Reproducible baseline comparison |
| Git operations (commit, history, clean) | **Binary** (Rust) | Direct `std::process::Command` calls; selective commit can self-heal narrow missed agent-owned drift and normalize boundary cleanup without absorbing free-form user prompts |
| Tmux routing, session registry, pane management | **Binary** (Rust) | Process-level coordination |
| Pre-response snapshots, undo, extract, transfer | **Binary** (Rust) | File-level atomicity |
| Boundary marker lifecycle (insert, reposition, cleanup) | **Binary** (Rust) | Deterministic, all write paths need it |
| Reading diff, interpreting user intent | **Skill** (SKILL.md) | Requires LLM reasoning, including treating imperative document edits as executable directives rather than meta-only discussion topics |
| Generating response content | **Skill** (SKILL.md) | Non-deterministic |
| Deciding what to write to which component | **Skill** (SKILL.md) | Context-dependent, including the shared Claude/Codex manual-repair rule to use `agent-doc write --commit <file>` when the prompt already exists |
| Enforcing response-cycle completion for the normal happy path | **Binary** (Rust) | `agent-doc finalize` fails closed unless the write reaches a terminal committed cycle state |
| Streaming checkpoints, progress tracking | **Skill** (SKILL.md) | Response-generation timing |
| Pending item management (parse, populate, process) | **Skill** (SKILL.md) | Semantic understanding of prompts |

See [CLAUDE.md](CLAUDE.md) for the full module layout, stream mode details, and release process.

Large specs now split behind stable entrypoint files instead of growing indefinitely. For command behavior, start at [`specs/07-commands.md`](specs/07-commands.md), which indexes the focused sibling specs for core commands, tmux/session behavior, closeout, and orchestration. For the authoring rule behind that split, see [`runbooks/split-spec-files.md`](runbooks/split-spec-files.md). That runbook applies across agent-doc-managed harness instruction surfaces, while custom root instruction files stay opt-in unless they still match the generated baseline.

## Supported Editors

**JetBrains (IntelliJ, PyCharm, etc.)**

```sh
agent-doc plugin install jetbrains
```

Or install from JetBrains Marketplace. Configure an External Tool: Program=`agent-doc`, Args=`run $FilePath$`, Working dir=`$ProjectFileDir$`. Assign a keyboard shortcut.

**VS Code**

```sh
agent-doc plugin install vscode
```

Or install from the VS Code Marketplace. Add a task with `"command": "agent-doc run ${file}"` and bind it to a keybinding.

**Vim/Neovim**

```vim
nnoremap <leader>as :!agent-doc run %<CR>:e<CR>
```

## Domain Ontology

agent-doc extends the [existence kernel vocabulary](https://github.com/btakita/existence-lang) with domain-specific terms.

### Document Lifecycle

| Term | Definition |
|------|-----------|
| **Session** | A persistent conversation between a user and an agent, identified by UUID. Stored in frontmatter as `agent_doc_session`. |
| **Document** | A markdown file that serves as the UI for a session. Contains frontmatter, components, and user/agent content. |
| **Snapshot** | A baseline copy of the document at a known state. Used for diff computation and CRDT merge. |
| **Component** | A named region in a template document (`<!-- agent:name -->...<!-- /agent:name -->`). Targeted by patch blocks. |
| **Boundary** | A marker (`<!-- agent:boundary:hash -->`) that separates committed content from uncommitted user edits. |
| **Exchange** | The shared conversation surface where user and agent write inline. A component with `patch=append`. |

### Pane Lifecycle

| Term | Definition |
|------|-----------|
| **Binding** | The document→pane association stored in `sessions.json`. Created by `claim` (explicit) or `auto_start` (automatic). One document per pane. |
| **Reconciliation** | The process of matching editor layout to tmux layout. Performed by `sync`. Stashes unwanted panes, provisions missing ones. |
| **Provisioning** | Creating a new tmux pane and starting the configured agent harness for a document. Performed by `route::auto_start`. The normal path for new documents — sync triggers provisioning when it finds a session UUID with no registered pane. |
| **Initialization** | Assigning a session UUID, creating a snapshot, and committing to git. Performed by `ensure_initialized()`. Called from claim, preflight, and sync's resolve_file. |

### Integration Layer

| Term | Definition |
|------|-----------|
| **Route** | Resolve which tmux pane handles a file. Creates panes if needed (provisioning). |
| **Sync** | Reconcile editor layout with tmux layout. The primary entrypoint from the JB plugin on every tab switch. |
| **Claim** | Bind a document to a specific existing pane. Used for manual pane assignment; not needed in normal editor workflow (sync + auto_start handles it). |

### Interaction Model

| Term | Definition |
|------|-----------|
| **Directive** | A signal that authorizes and requests action. User inputs like "do", "go", "yes" are directives. Classified as `DiffType::Approval` in preflight. The directive's brevity is independent of the expected execution thoroughness — quality processes always apply in full. |
| **Cycle** | One round-trip: user edits -> preflight -> agent response -> write-back -> commit. Logged in `.agent-doc/logs/cycles.jsonl` with git state references for reproducibility, with the current per-document phase tracked in `.agent-doc/state/cycles/<doc-hash>.json` and the exact response body durably captured in `.agent-doc/captures/<doc-hash>/<cycle-id>.json` so interrupted cycles can be replayed or blocked exactly. |
| **Layout check** | Pre-agent tmux health inspection (`check_layout()`). Detects: missing window 0, non-idle stash panes, and session drift (registered panes spanning multiple tmux sessions). Reported as `layout_issues[]` in preflight JSON. |
| **Session drift** | Condition where registered document panes span more than one tmux session. Detected by preflight's `check_layout()`. Fixed by `agent-doc session set <N>` to consolidate panes into the target session. |
| **Diff** | The user's changes since the last snapshot. Classified by `classify_diff()` into a `DiffType` for skill routing. Comment-stripped before comparison. |
| **Annotation** | A user edit to agent-written content (inline modification, colon-append). Classified as `DiffType::Annotation`. |

## Security

agent-doc is designed for **single-user, local operation**. All session data (documents, snapshots, exchange history) is stored on the local filesystem and committed to a git repository.

**Current security model:**
- **Single user only.** There is no multi-user access control, authentication, or session isolation.
- **Private repo recommended.** Session documents may contain sensitive content (correspondence, research, credentials in context). Use a private git repository.
- **Prompt injection risk.** Content pasted into documents from external sources (emails, web pages, chat logs) could contain prompt injection attempts. The agent processes all document content as user input with no injection scanning.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions), the agent has full filesystem access. Injected prompts could read files or execute commands if not sandboxed.
- **Shared-doc guard is explicit, not full auth.** Documents may opt into `agent_doc_collaboration: shared`; in that mode, cross-document `extract` / `transfer` and plan-backed `do #id` work that references another `.md` file fail closed unless the document also carries `agent_doc_security_review: <review-id>`. This is an audit marker for cross-document access, not a replacement for real authentication or session isolation.

**Planned:** Collaborative security for web/networked deployments (multi-user access control, session isolation, content scanning, compartmented access patterns).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
