//! # Module: main (agent-doc CLI)
//!
//! ## Spec
//! - Entry point for the `agent-doc` binary; parses the command line with `clap` derive.
//! - Top-level struct `Cli` holds a single `Commands` subcommand enum (40+ variants).
//! - `AgentDocMode` enum (`Append`, `Template`, `Stream`) is a `ValueEnum` used by `Convert`
//!   and `Mode` subcommands; `Append` maps to inline format, `Template`/`Stream` to CRDT.
//! - On startup, calls `upgrade::warn_if_outdated()` for all subcommands except `Upgrade`.
//! - Loads global config via `agent_doc_orchestration::config::load()` before dispatching; config is threaded into
//!   subcommands that accept an agent backend (`Run`, `Stream`, `Watch`, `Init`).
//! - Each subcommand delegates immediately to its own module (`agent_doc_orchestration::run::run`, `agent_doc_orchestration::diff::run`, etc.);
//!   `main` contains no business logic beyond argument destructuring and dispatch.
//! - `Route` no longer runs a follow-up sync; editor/plugin sync remains the
//!   authoritative layout path.
//! - `Write` auto-detects the write strategy from frontmatter when no `--template`/`--stream`
//!   flag is given; CRDT-mode documents use `agent_doc_orchestration::write::run_stream`, others use `agent_doc_orchestration::write::run`.
//! - `Prompt --all` runs `agent_doc_orchestration::prompt::run_all()`; otherwise `FILE` is required.
//! - `History --restore <commit>` calls `history::restore`; bare `History` calls `history::list`.
//! - `Watch` dispatches to `agent_doc_orchestration::watch::stop`, `agent_doc_orchestration::watch::status`, or `agent_doc_orchestration::watch::start` based on flags.
//! - `Skill install --reload` prints `SKILL_RELOAD=compact` or `SKILL_RELOAD=restart` when the
//!   skill was updated, enabling the caller to take the appropriate reload action.
//! - `LibPath` prints the platform-appropriate shared library path (`libagent_doc.so/dylib/dll`)
//!   next to the binary, exiting with code 1 if not found.
//! - `ListCommands` emits a JSON array of all available subcommand names for plugin autocomplete.
//!
//! ## Agentic Contracts
//! - `main` returns `anyhow::Result<()>`; any subcommand error propagates and prints to stderr.
//! - Subcommand modules are the single source of truth for their behavior; `main` only routes.
//! - Config is loaded once and passed by reference; subcommands must not reload config.
//! - `Upgrade` bypasses the version check that all other subcommands run on startup.
//!
//! ## Evals
//! - dispatch_run: `agent-doc run <file>` → `agent_doc_orchestration::run::run` called with correct args
//! - dispatch_write_crdt_autodetect: CRDT frontmatter + no flags → `agent_doc_orchestration::write::run_stream` selected
//! - dispatch_write_inline_autodetect: inline frontmatter + no flags → `agent_doc_orchestration::write::run` selected
//! - dispatch_prompt_all: `--all` → `agent_doc_orchestration::prompt::run_all`, no FILE required
//! - dispatch_history_restore: `--restore <sha>` → `history::restore` called
//! - dispatch_watch_stop: `--stop` flag → `agent_doc_orchestration::watch::stop` called
//! - dispatch_skill_install_reload: skill updated + `--reload compact` → prints `SKILL_RELOAD=compact`
//! - dispatch_lib_path_missing: library absent → exits with code 1

mod annotate;
mod audit_docs;
mod auto_dag;
mod autoclaim;
mod clean;
mod cleanup_cmd;
mod commands;
mod convert;
mod describe_image;
mod extract;
mod history;
mod hook_cmd;
mod init;
mod install;
mod jobs;
mod layout;
mod lib_gc;
mod lib_install;
mod migrate;
mod mode;
mod notify;
mod ops_report;
mod orchestrate;
mod outline;
mod parallel;
mod patch;
mod plan;
mod plugin;
mod project_config;
mod queue_dispatch;
mod read;
mod rename;
mod reset;
mod serve;
mod session_actor_cmd;
mod session_cmd;
#[cfg(test)]
mod sim_world;
mod skill;
mod terminal;
#[cfg(test)]
mod test_support;
mod tsift_graph;
mod undo;
mod upgrade;
mod worktree;

// Re-export library modules so binary-internal modules can use `crate::` paths
pub(crate) use agent_doc::component;
pub(crate) use agent_doc::crdt;
pub(crate) use agent_doc::frontmatter;
pub(crate) use agent_doc::template;

use anyhow::Context;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Document mode for agent-doc sessions.
#[derive(Clone, Debug, ValueEnum)]
pub enum AgentDocMode {
    /// Append-mode: alternating ## User / ## Assistant blocks
    Append,
    /// Template-mode: in-place component patching
    Template,
    /// Stream-mode: real-time CRDT write-back (superset of template)
    Stream,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum PatchMode {
    Replace,
    Append,
    Prepend,
}

/// Turn lifecycle actions for `agent-doc turn-status`
/// (#claude-busy-status-during-active-turn).
#[derive(Subcommand)]
pub enum TurnStatusAction {
    /// A turn just started (UserPromptSubmit hook): show "turn in progress".
    Active,
    /// The turn just ended (Stop hook): clear the status.
    Idle,
    /// Install the monitor's hooks for Claude, Codex, and OpenCode.
    Install {
        /// Install root for all harnesses (default: cwd). Mainly for testing.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Install into each harness's user config dir instead of the project.
        #[arg(long)]
        user: bool,
        /// Also enable tmux `pane-border-status` on the running server so the
        /// "turn in progress" border title is visible immediately.
        #[arg(long)]
        tmux: bool,
    },
}

#[derive(Parser)]
#[command(
    name = "agent-doc",
    version,
    about = "Interactive document sessions with AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn looks_like_document_path(arg: &str) -> bool {
    let path = Path::new(arg);
    path.exists() || path.components().count() > 1 || path.extension().is_some()
}

fn rewrite_bare_file_invocation(mut args: Vec<OsString>) -> Vec<OsString> {
    let Some(first) = args.get(1).and_then(|arg| arg.to_str()) else {
        return args;
    };
    if first.starts_with('-') {
        return args;
    }

    let is_known_subcommand = Cli::command()
        .get_subcommands()
        .any(|sub| sub.get_name() == first || sub.get_all_aliases().any(|alias| alias == first));
    if !is_known_subcommand && looks_like_document_path(first) {
        args.insert(1, OsString::from("run"));
    }
    args
}

fn deprecated_pending_alias_used(args: &[OsString]) -> bool {
    matches!(args.get(1).and_then(|arg| arg.to_str()), Some("pending"))
}

fn deprecated_done_flag_used(args: &[OsString]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.to_str(), Some("--pending-done" | "--backlog-done")))
}

#[derive(Args, Clone)]
struct WriteArgs {
    /// Path to the session document
    file: PathBuf,
    /// Baseline content for 3-way merge (reads from file if omitted)
    #[arg(long)]
    baseline_file: Option<PathBuf>,
    /// Template mode: parse <!-- patch:name --> blocks and apply to components
    #[arg(long)]
    template: bool,
    /// Stream mode: template patches with CRDT merge (conflict-free)
    #[arg(long)]
    stream: bool,
    /// IPC mode: write patch JSON to .agent-doc/patches/ for IDE plugin consumption
    #[arg(long)]
    ipc: bool,
    /// Force direct disk write, skip IPC even when plugin is installed
    #[arg(long)]
    force_disk: bool,
    /// Write origin identifier for tracing (e.g., "skill", "watch", "stream")
    #[arg(long)]
    origin: Option<String>,
    /// Add a new pending item at the beginning of the list (repeatable).
    /// Prefix with canonical `id=<custom> ` to preserve a custom id instead of generating one.
    /// Leading `[#custom] ` is also accepted as compatibility input.
    #[arg(long = "pending-add")]
    pending_add: Vec<String>,
    /// Add a new pending item to another document's backlog (repeatable pairs: FILE TEXT).
    /// Prefix TEXT with canonical `id=<custom> ` to preserve a custom id.
    #[arg(long = "pending-add-to", num_args = 2, value_names = ["FILE", "TEXT"])]
    pending_add_to: Vec<String>,
    /// Add a new gated pending item at the beginning of the list (repeatable).
    /// Prefix with canonical `id=<custom> ` to preserve a custom id instead of generating one.
    /// Leading `[#custom] ` is also accepted as compatibility input.
    #[arg(long = "pending-add-gated")]
    pending_add_gated: Vec<String>,
    /// Add a new pending item immediately AFTER an existing item (repeatable pairs: ID TEXT).
    /// Chains build a deterministic order: `--pending-add-after A "B" --pending-add-after B "C"` → A→B→C.
    #[arg(long = "pending-add-after", num_args = 2, value_names = ["ID", "TEXT"])]
    pending_add_after: Vec<String>,
    /// Add a new pending item immediately BEFORE an existing item (repeatable pairs: ID TEXT).
    #[arg(long = "pending-add-before", num_args = 2, value_names = ["ID", "TEXT"])]
    pending_add_before: Vec<String>,
    /// Add a new pending item at the END of the active list (repeatable). Alias `--pending-append`.
    #[arg(long = "pending-add-back", alias = "pending-append")]
    pending_add_back: Vec<String>,
    /// Mark a backlog or icebox item `[x]` by hash id (repeatable).
    /// `--pending-done` and `--backlog-done` are deprecated aliases.
    #[arg(long = "done", alias = "pending-done", alias = "backlog-done")]
    pending_done: Vec<String>,
    /// Edit a pending item: `id=new text` (repeatable).
    #[arg(long = "pending-edit")]
    pending_edit: Vec<String>,
    /// Clear all pending items.
    #[arg(long = "pending-clear")]
    pending_clear: bool,
    /// Reorder pending items by comma-separated hash ids.
    #[arg(long = "pending-reorder")]
    pending_reorder: Option<String>,
    /// Transition a pending item to `[/]` (gated) by hash id (repeatable).
    /// Idempotent on already-gated items; errors on `[x]` items.
    #[arg(long = "pending-gate")]
    pending_gate: Vec<String>,
    /// Transition a pending item from `[/]` back to `[ ]` by hash id (repeatable).
    /// Errors on `[ ]` or `[x]` items — the source must be gated.
    #[arg(long = "pending-ungate")]
    pending_ungate: Vec<String>,
    /// Resolve all items matching a typed gate (e.g., [/release] → [x]).
    #[arg(long = "pending-resolve-gate")]
    pending_resolve_gate: Vec<String>,
    /// Set a typed gate on a gated item: `id=gate_type` (e.g., `gqep=release`).
    #[arg(long = "pending-set-gate-type")]
    pending_set_gate_type: Vec<String>,
    /// Add a new gated item directly to the review list (repeatable).
    #[arg(long = "review-add")]
    review_add: Vec<String>,
    /// Edit a review item: `id=new text` (repeatable).
    #[arg(long = "review-edit")]
    review_edit: Vec<String>,
    /// Allow `replace:pending` blocks in stdin (escape hatch, hidden).
    /// `--allow-patch-pending` is accepted as a deprecated alias (#25ag).
    #[arg(
        long = "allow-replace-pending",
        alias = "allow-patch-pending",
        hide = true
    )]
    allow_replace_pending: bool,
    /// Only mutate pending component — skip stdin reading and exchange synthesis.
    /// Requires at least one --pending-* flag; incompatible with --template/--stream/--ipc.
    #[arg(long = "pending-only")]
    pending_only: bool,
    /// Replace the status component content (repeatable for multi-line).
    #[arg(long = "status")]
    status: Option<String>,
    /// Override the tagpath agent-doc lint dialect mode for this run.
    /// Values: `off | warn | strict`. Precedence: CLI > frontmatter
    /// `agent_doc_lint_dialect` > workspace `.agent-doc/config.toml`
    /// `[lint] dialect` > default (`warn`).
    #[arg(long = "lint", value_name = "MODE")]
    lint: Option<String>,
    /// Cross-repo sibling commit driven by this cycle (repeatable).
    /// After the session-document commit lands, stage and commit the named
    /// sibling working tree with a `Session-Doc:` git trailer auto-injected
    /// (URL points at the just-committed session-document blob). Stage the
    /// sibling's files before invoking `finalize` / `write --commit`. Pair
    /// each `--commit-sibling` with one `--commit-sibling-message` in the
    /// same order.
    #[arg(long = "commit-sibling", value_name = "REPO")]
    commit_sibling: Vec<PathBuf>,
    /// Commit message for each `--commit-sibling` (repeatable, positional pairing).
    #[arg(long = "commit-sibling-message", value_name = "MSG")]
    commit_sibling_message: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a session: diff, send to agent, write response by document mode
    Run {
        /// Path to the session document
        file: PathBuf,
        /// Auto-create a branch for session commits
        #[arg(short = 'b')]
        branch: bool,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Preview what would be sent without submitting
        #[arg(long)]
        dry_run: bool,
        /// Skip git commit after submit
        #[arg(long)]
        no_git: bool,
    },
    /// List or restore exchange component versions from git history
    History {
        /// Path to the session document
        file: PathBuf,
        /// Restore exchange content from a specific commit (prepend to current)
        #[arg(long)]
        restore: Option<String>,
    },
    /// Annotated git log for a session document (shows pre-compact tags)
    Log {
        /// Path to the session document
        file: PathBuf,
    },
    /// Inspect operational logs
    Ops {
        #[command(subcommand)]
        action: OpsAction,
    },
    /// Show document content at a specific point in git history
    Show {
        /// Path to the session document
        file: PathBuf,
        /// Show the file N commits back from HEAD (e.g. --back 1 → HEAD~1)
        #[arg(long)]
        back: Option<usize>,
        /// Show the Nth commit in git log order (0 = newest, 1 = next oldest, …)
        #[arg(long)]
        at: Option<usize>,
        /// Show the commit pointed to by this tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Scaffold a new session document (omit file to initialize project)
    Init {
        /// Path for the new session document (omit to initialize project)
        file: Option<PathBuf>,
        /// Session title
        title: Option<String>,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Document mode: append (default) or template
        #[arg(long)]
        mode: Option<String>,
    },
    /// System-level setup: check prerequisites, install editor plugins
    Install {
        /// Editor to install plugin for (jetbrains or vscode; auto-detected if omitted)
        #[arg(long)]
        editor: Option<String>,
        /// Skip prerequisite checks
        #[arg(long)]
        skip_prereqs: bool,
        /// Skip plugin installation
        #[arg(long)]
        skip_plugins: bool,
    },
    /// Preview the diff that would be sent, or diff between two git refs
    Diff {
        /// Path to the session document
        file: PathBuf,
        /// Wait for stable content (truncation detection) before computing diff
        #[arg(long)]
        wait: bool,
        /// Starting git ref for historical diff (e.g. commit hash, tag, HEAD~2)
        #[arg(long)]
        from: Option<String>,
        /// Ending git ref for historical diff (default: HEAD)
        #[arg(long)]
        to: Option<String>,
    },
    /// Clear session ID and delete or rebuild snapshot state
    Reset {
        /// Path to the session document
        file: PathBuf,
        /// Rebuild snapshot and CRDT state from the current visible markdown
        #[arg(long)]
        from_current: bool,
        /// With --from-current, preserve resume/session metadata and capture history
        #[arg(long)]
        preserve_session: bool,
    },
    /// Squash session git history into one commit
    Clean {
        /// Path to the session document
        file: PathBuf,
        /// Create an archive tag before squashing (preserves full history)
        #[arg(long)]
        archive: bool,
    },
    /// Audit instruction files against the codebase
    AuditDocs {
        /// Project root directory (auto-detected if omitted)
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Garbage-collect orphaned files in .agent-doc/
    Gc {
        /// Project root directory (auto-detected if omitted)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,
    },
    /// List or restore a document's pre-mutation recovery checkpoints
    /// (pre-auto-run / pre-compact tags)
    Checkpoint {
        /// Path to the session document
        file: PathBuf,
        /// Restore only this document from the named checkpoint tag (other files
        /// untouched); review and commit afterward
        #[arg(long, value_name = "TAG")]
        restore: Option<String>,
        /// Show `git diff <TAG> -- <FILE>` for the named checkpoint tag
        #[arg(long, value_name = "TAG", conflicts_with = "restore")]
        diff: Option<String>,
    },
    /// Start Claude in a tmux pane and register the session
    Start {
        /// Path to the session document
        file: PathBuf,
        /// Force binding the session to the current tmux pane, even if a live
        /// owner already exists in another pane
        #[arg(long)]
        force: bool,
        /// Internal route-owned pane mode: exit and reap after the first
        /// binary-owned document cycle commits when the document has no
        /// continued-interaction signals.
        #[arg(long = "route-owned", hide = true)]
        route_owned: bool,
        /// Internal route-owned pane reap policy.
        #[arg(
            long = "route-owned-reap-policy",
            hide = true,
            default_value_t = agent_doc_orchestration::start::RouteOwnedReapPolicy::Auto
        )]
        route_owned_reap_policy: agent_doc_orchestration::start::RouteOwnedReapPolicy,
    },
    /// Route agent-doc command to the correct tmux pane
    Route {
        /// Path to the session document
        file: PathBuf,
        /// Resolve the owning pane and send the bare reopen without route-owned
        /// busy-session recovery, startup-miss gating, or cycle-ack waiting.
        #[arg(long)]
        dispatch_only: bool,
        /// Send the plain `agent-doc <FILE>` reopen even for harnesses whose
        /// normal startup trigger is slash-command based.
        #[arg(long)]
        plain_trigger: bool,
        /// Tmux pane ID for lazy claiming (auto-claims if existing claim is stale)
        #[arg(long)]
        pane: Option<String>,
        /// Editor layout columns (comma-separated files per column, repeatable)
        #[arg(long = "col")]
        cols: Vec<String>,
        /// Focused file in the editor (for tmux pane focus)
        #[arg(long)]
        focus: Option<String>,
        /// Wait for typing to settle before routing (milliseconds, 0 = no debounce)
        #[arg(long, default_value_t = 500)]
        debounce: u64,
        /// Override the bounded wait for the authoritative actor to become
        /// dispatch-ready (seconds). When the actor is still in `starting`
        /// state, route normally fails closed after a harness-specific
        /// timeout (e.g. 10s for claude). User-initiated dispatches —
        /// especially the JB plugin's `Run Agent Doc` — can pass a longer
        /// wait (e.g. 60) so the user does not have to manually rerun while
        /// the supervisor is still booting. Capped at 600s.
        #[arg(long)]
        wait_for_ready: Option<u64>,
    },
    /// Detect permission prompts from a Claude Code or OpenCode session
    Prompt {
        /// Path to the session document (omit with --all)
        file: Option<PathBuf>,
        /// Answer a prompt by selecting option N (1-based)
        #[arg(long)]
        answer: Option<usize>,
        /// Poll all active sessions instead of a single file
        #[arg(long)]
        all: bool,
    },
    /// Commit a session document (git add + commit with timestamp)
    Commit {
        /// Path to the session document
        file: PathBuf,
    },
    /// Remove consecutive duplicate response blocks
    Dedupe {
        /// Path to the session document
        file: PathBuf,
    },
    /// Reclaim an orphaned cycle after an explicit run cancel: abandon an empty
    /// `preflight_started` cycle (no response capture) so the next dispatch can
    /// start fresh immediately instead of waiting for the staleness window.
    Cancel {
        /// Path to the session document
        file: PathBuf,
    },
    /// Claim a document for the current tmux pane
    Claim {
        /// Path to the session document
        file: PathBuf,
        /// Positional hint to select pane by position (left, right, top, bottom)
        #[arg(long)]
        position: Option<String>,
        /// Explicit tmux pane ID (e.g. %42) — overrides position detection
        #[arg(long)]
        pane: Option<String>,
        /// Scope pane resolution to this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
        /// Force overwrite tmux_session even if already set to a different session
        #[arg(long)]
        force: bool,
        /// Spawn a fresh Claude Code session in a new tmux window scoped to the
        /// document's nearest git repo root (loads CLAUDE.md, memory, skills for
        /// that repo rather than the superproject)
        #[arg(long)]
        isolate: bool,
    },
    /// Focus the tmux pane for a session document
    Focus {
        /// Path to the session document
        file: PathBuf,
        /// Explicit tmux pane ID — overrides session lookup
        #[arg(long)]
        pane: Option<String>,
        /// Defer stash-pane reparenting to the sync reconciler instead of
        /// additively promoting on focus. Editor-navigation focus sets this so
        /// the promote does not race the follow-up sync and grow the window to
        /// an extra pane (#jb-nav-3pane-promote-swap).
        #[arg(long)]
        no_stash_promote: bool,
    },
    /// Arrange tmux panes to mirror editor split layout
    Layout {
        /// Session documents to arrange
        files: Vec<PathBuf>,
        /// Split direction: h (horizontal/side-by-side) or v (vertical/stacked)
        #[arg(long, short, default_value = "h")]
        split: String,
        /// Explicit tmux pane ID — scopes pane selection to this pane's session
        #[arg(long)]
        pane: Option<String>,
        /// Only operate on panes within this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
    },
    /// Sync tmux panes to a 2D columnar layout matching the editor
    Sync {
        /// Columns of comma-separated file paths (left-to-right). Repeat for each column.
        /// When omitted, sync falls back to the recorded `.agent-doc/last_layout.json`
        /// for the current sync scope.
        #[arg(long = "col")]
        columns: Vec<String>,
        /// Only operate on panes within this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
        /// Focus this file's pane after arranging (defaults to first file)
        #[arg(long)]
        focus: Option<String>,
        /// Signal that this sync was triggered by a file rename. Creates a debounce marker
        /// that suppresses auto-start for the focused file across subsequent syncs (5s TTL).
        #[arg(long)]
        rename: bool,
        /// Arrange/reconcile existing panes without auto-starting replacement sessions.
        #[arg(long)]
        no_autostart: bool,
        /// Treat provided --col values as the exact editor-visible projection.
        /// This disables focus-only expansion from remembered column memory.
        #[arg(long)]
        exact_visible: bool,
    },
    /// Replace content in a named component
    Patch {
        /// Path to the document
        file: PathBuf,
        /// Component name (e.g. "status", "log")
        component: String,
        /// Patch mode override. Defaults to replace.
        #[arg(long, value_enum, default_value = "replace")]
        mode: PatchMode,
        /// Replacement content (reads from stdin if omitted)
        content: Option<String>,
    },
    /// Watch session files for changes and auto-submit
    Watch {
        /// Stop the running watch daemon
        #[arg(long)]
        stop: bool,
        /// Show watch daemon status
        #[arg(long)]
        status: bool,
        /// Debounce delay in milliseconds
        #[arg(long, default_value = "500")]
        debounce: u64,
        /// Maximum agent-triggered cycles per file
        #[arg(long, default_value = "3")]
        max_cycles: u32,
    },
    /// Display markdown outline with section structure and token counts
    Outline {
        /// Path to the markdown document
        file: PathBuf,
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
    /// Plan the completion work-graph (auto-dag) for a document's backlog +
    /// review items: classify each (implementable / live-verify / IPC-capture /
    /// blocked / done) and emit a Mermaid diagram + nested list
    /// (`#auto-dag-first-class`, the agent-doc analogue of `/goal`)
    AutoDag {
        /// Path to the markdown document
        file: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate sessions.json against live tmux panes, remove stale entries
    Resync {
        /// Limit checks/fixes to a single session document
        file: Option<PathBuf>,
        /// Actually kill wrong-session panes and deregister stale entries (without this flag, dry-run only)
        #[arg(long)]
        fix: bool,
        /// Relocate WrongSession panes to this tmux session via join-pane instead of killing them.
        /// Requires --fix. Example: --session 10
        #[arg(long)]
        session: Option<String>,
    },
    /// Fix stale routing/session issues globally or for one session document (`resync --fix` alias)
    Fix {
        /// Limit fixes to a single session document
        file: Option<PathBuf>,
        /// Relocate WrongSession panes to this tmux session via join-pane instead of killing them.
        /// Example: --session 10
        #[arg(long)]
        session: Option<String>,
    },
    /// Manage the Claude Code skill definition
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage editor plugins
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Append an assistant response to a session document (reads from stdin)
    Write {
        #[command(flatten)]
        args: WriteArgs,
        /// Commit the document to git after a successful write (skipped silently if not in a git repo)
        #[arg(long)]
        commit: bool,
    },
    /// Append an assistant response and require the cycle to reach a committed state
    Finalize {
        #[command(flatten)]
        args: WriteArgs,
    },
    /// Stream agent output to document in real-time (CRDT merge)
    Stream {
        /// Path to the session document
        file: PathBuf,
        /// Write-back interval in milliseconds
        #[arg(long, default_value = "200")]
        interval: u64,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Skip git commit after stream completes
        #[arg(long)]
        no_git: bool,
        /// Override the tagpath agent-doc lint dialect mode for this stream run.
        /// Values: `off | warn | strict`. Precedence: CLI > frontmatter
        /// `agent_doc_lint_dialect` > workspace `.agent-doc/config.toml`
        /// `[lint] dialect` > default (`warn`).
        #[arg(long = "lint", value_name = "MODE")]
        lint: Option<String>,
    },
    /// Show template structure of a document (components, modes, content)
    TemplateInfo {
        /// Path to the document
        file: PathBuf,
    },
    /// Repair an orphaned response or stale document cycle (`recover` alias kept)
    #[command(name = "repair", visible_alias = "recover")]
    Repair {
        /// Path to the session document
        file: PathBuf,
        /// Apply the unambiguously-safe closeout recovery for the classified
        /// drift state in one step (abandon empty preflight cycle / commit
        /// boundary-artifact drift), instead of orphaned-response repair.
        #[arg(long)]
        apply_recovery: bool,
    },
    /// Run all pre-agent steps (repair, commit, claims, diff, document HEAD) and output JSON
    Preflight {
        /// Path to the session document
        file: PathBuf,
        /// Pure inspection probe: emit the same JSON but do NOT open a
        /// `preflight_started` cycle, so a diagnostic preflight never leaves an
        /// open cycle that later wedges `session-check`
        /// (`#preflight-probe-side-effect-free`).
        #[arg(long)]
        probe: bool,
    },
    /// Check end-of-cycle write invariant — nonzero exit if the cycle is open or a likely direct response patchback bypassed agent-doc
    SessionCheck {
        /// Path to the session document
        file: PathBuf,
        /// Strict Codex final gate: exit nonzero when a clean document still owes an active `agent:queue auto` continuation
        #[arg(long)]
        codex_final_gate: bool,
    },
    /// Serve a localhost HTTP markdown editor for one document or a project session list
    Serve {
        /// Path to a session document or project directory. Defaults to the nearest project root from CWD.
        file: Option<PathBuf>,
        /// Bind host (default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
        /// Bind port (default: 7333)
        #[arg(long)]
        port: Option<u16>,
        /// Edit bearer token. Defaults to a generated token when auth is required.
        #[arg(long, value_name = "TOKEN")]
        auth_token: Option<String>,
        /// Read-only bearer token for viewer access.
        #[arg(long, value_name = "TOKEN")]
        read_only_token: Option<String>,
        /// TLS certificate PEM path. Requires --tls-key.
        #[arg(long, value_name = "PEM")]
        tls_cert: Option<PathBuf>,
        /// TLS private key PEM path. Requires --tls-cert.
        #[arg(long, value_name = "PEM")]
        tls_key: Option<PathBuf>,
    },
    /// Describe an image file using a vision-capable AI model
    DescribeImage {
        /// Path to the image file
        image: PathBuf,
        /// Vision provider (openai, anthropic)
        #[arg(long)]
        provider: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// API key (defaults to AGENT_DOC_VISION_API_KEY or provider-specific env var)
        #[arg(long)]
        api_key: Option<String>,
        /// Custom prompt for the vision model
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Print document content to stdout (full file or a single named component).
    Read {
        /// Path to the session document
        file: PathBuf,
        /// Name of a specific component to extract (e.g. "exchange", "backlog").
        /// If omitted, the full file is printed.
        #[arg(long)]
        component: Option<String>,
    },
    /// List live and archived response sections for targeted retrieval
    ResponseToc {
        /// Path to the session document
        file: PathBuf,
        /// Exact backlog / prompt id to match (with or without leading #)
        #[arg(long = "id")]
        backlog_id: Option<String>,
        /// Free-text query over response headings and bodies
        #[arg(long)]
        query: Option<String>,
        /// Max archive entries to include
        #[arg(long, default_value_t = 6)]
        limit: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Load an exact live or archived response section, optionally with neighbors
    ResponseFetch {
        /// Path to the session document
        file: PathBuf,
        /// Locator from `agent-doc response-toc`
        #[arg(long)]
        locator: String,
        /// Include this many earlier adjacent sections
        #[arg(long, default_value_t = 0)]
        before: usize,
        /// Include this many later adjacent sections
        #[arg(long, default_value_t = 0)]
        after: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Build or refresh the sqlite archive index for compacted turns
    ArchiveIndex {
        /// Path to a session document in the target project
        file: PathBuf,
        /// Drop and rebuild the derived index from archive markdown
        #[arg(long)]
        rebuild: bool,
    },
    /// Search the sqlite archive index for compacted turns
    ArchiveSearch {
        /// Path to a session document in the target project
        file: PathBuf,
        /// Free-text query over indexed archive chunks
        #[arg(long)]
        query: Option<String>,
        /// Exact backlog / prompt id to match (with or without leading #)
        #[arg(long = "id")]
        backlog_id: Option<String>,
        /// Restrict to a specific archived session id
        #[arg(long)]
        session: Option<String>,
        /// Max results to print
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Rebuild the derived index before searching
        #[arg(long)]
        rebuild: bool,
    },
    /// Archive old exchanges / compact component content
    Compact {
        /// Path to the session document
        file: PathBuf,
        /// Number of recent exchanges/topics to keep.
        /// Append mode default: 2. Template mode: omit to archive all (full compact),
        /// or pass N to keep last N `### Re:` topic sections (partial compact).
        #[arg(long)]
        keep: Option<usize>,
        /// Component to compact (template/stream mode, default: exchange)
        #[arg(long)]
        component: Option<String>,
        /// Summary message to replace content with
        #[arg(long)]
        message: Option<String>,
        /// Git tag name for pre-compact checkpoint (default: auto-generated
        /// agent-doc/<doc-name>/pre-compact-N). Use "skip" to disable tagging.
        #[arg(long)]
        tag: Option<String>,
        /// Close out compaction via the agent-doc commit path and verify VCS refresh when available
        #[arg(long)]
        commit: bool,
    },
    /// Convert a document between append and template modes
    Convert {
        /// Path to the session document
        file: PathBuf,
        /// Target mode (deprecated positional — use --agent-doc-format / --agent-doc-write instead)
        #[arg(value_enum)]
        mode: Option<AgentDocMode>,
        /// Set document format (append | template)
        #[arg(long, value_enum)]
        agent_doc_format: Option<frontmatter::AgentDocFormat>,
        /// Set write strategy (merge | crdt)
        #[arg(long, value_enum)]
        agent_doc_write: Option<frontmatter::AgentDocWrite>,
    },
    /// Get or set the document mode (format + write strategy)
    Mode {
        /// Path to the session document
        file: PathBuf,
        /// Set mode: append or template (deprecated — use --format / --write)
        #[arg(long)]
        set: Option<String>,
    },
    /// Print and clear the claims log (.agent-doc/claims.log)
    Claims,
    /// Fan-out: decompose task into parallel worktree-isolated subagents
    Parallel {
        /// Path to the session document
        file: PathBuf,
        /// Explicit subtask descriptions (repeatable)
        #[arg(long = "task")]
        tasks_explicit: Vec<String>,
        /// Model override for subtask agents
        #[arg(long)]
        model: Option<String>,
        /// Skip git commits in worktrees
        #[arg(long)]
        no_git: bool,
        /// Run without worktrees (read-only tasks, shared CWD)
        #[arg(long)]
        no_worktree: bool,
        /// Per-task timeout in seconds
        #[arg(long, default_value = "600")]
        timeout: u64,
        /// Show plan without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Orchestrate sequential, parallel, or dependency-aware task batches against one document
    Orchestrate {
        /// Path to the session document
        file: PathBuf,
        /// Orchestration mode
        #[arg(long, value_enum, default_value_t = orchestrate::OrchestrateMode::Sequential)]
        mode: orchestrate::OrchestrateMode,
        /// Explicit task descriptions (repeatable)
        #[arg(long = "task")]
        tasks_explicit: Vec<String>,
        /// Read task descriptions from a markdown/text file
        #[arg(long = "from-file")]
        from_file: Option<PathBuf>,
        /// Extract the latest task list/code block from the document exchange
        #[arg(long = "from-exchange")]
        from_exchange: bool,
        /// Extract all active prompts and preset directives from agent:queue
        #[arg(long = "from-queue")]
        from_queue: bool,
        /// Resume a persisted auto-DAG schedule id from .agent-doc/schedules
        #[arg(long = "resume-schedule")]
        resume_schedule: Option<String>,
        /// Agent backend override for sequential or DAG execution
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Skip git commits in worktrees (parallel mode only; sequential/DAG require finalize)
        #[arg(long)]
        no_git: bool,
        /// Run without worktrees (parallel mode only)
        #[arg(long)]
        no_worktree: bool,
        /// Per-task timeout in seconds (parallel mode only)
        #[arg(long, default_value = "600")]
        timeout: u64,
        /// Show the resolved plan without executing
        #[arg(long)]
        dry_run: bool,
        /// Show each task's fully expanded prompt (with presets applied) without executing
        #[arg(long)]
        plan: bool,
    },
    /// Re-establish claims after context compaction (SessionStart hook)
    Autoclaim,
    /// Surface turn-in-progress status on the agent's own tmux pane border.
    /// Hook-driven: `UserPromptSubmit` runs `active`, `Stop` runs `idle`
    /// (#claude-busy-status-during-active-turn). Best-effort; no-op outside tmux.
    #[command(name = "turn-status")]
    TurnStatus {
        #[command(subcommand)]
        action: TurnStatusAction,
    },
    /// Derive a structured post-preflight planning/dispatch record for a document
    Plan {
        /// Path to the session document
        file: PathBuf,
    },
    /// Manage lower-agent job packets for a session document
    Jobs {
        #[command(subcommand)]
        action: JobsAction,
    },
    /// Check for updates and upgrade to the latest version.
    Upgrade,
    /// Generate content-source annotation sidecar for a document
    Annotate {
        /// Path to the session document
        file: PathBuf,
        /// Force regeneration even if cache is valid
        #[arg(long)]
        force: bool,
        /// Use git blame for full history attribution
        #[arg(long)]
        history: bool,
    },
    /// Undo the last agent response (restore pre-response state)
    Undo {
        /// Path to the session document
        file: PathBuf,
    },
    /// Extract the last exchange entry from source to target document
    Extract {
        /// Source document
        source: PathBuf,
        /// Target document
        target: PathBuf,
        /// Component name to extract from (default: exchange)
        #[arg(long)]
        component: Option<String>,
    },
    /// Transfer entire component content from source to target document
    Transfer {
        /// Source document
        source: PathBuf,
        /// Target document
        target: PathBuf,
        /// Component name to transfer
        component: String,
        /// Bypass pane ownership check on target (for cross-session transfers)
        #[arg(long)]
        bypass_claim: bool,
        /// Transfer only specific backlog/pending or icebox items by ID (comma-separated, e.g., "#id1,#id2")
        #[arg(long)]
        items: Option<String>,
        /// Insert a referral pointer instead of moving content (target reads source on demand)
        #[arg(long)]
        referral: bool,
    },
    /// Migrate session state after a document file rename/move
    Rename {
        /// Original document path (may no longer exist on disk)
        old_path: PathBuf,
        /// New document path (must exist)
        new_path: PathBuf,
    },
    /// Migrate documents: rename deprecated components and strip deprecated attributes
    Migrate {
        /// Session documents to migrate
        files: Vec<PathBuf>,
        /// Scan project root for all documents with deprecated markers
        #[arg(long)]
        all: bool,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Open an external terminal with tmux attached to the session
    Terminal {
        /// Path to the session document
        file: PathBuf,
        /// Tmux session name (overrides frontmatter tmux_session)
        #[arg(long)]
        session: Option<String>,
    },
    /// Insert a boundary marker at the end of a component for response ordering
    Boundary {
        /// Path to the session document
        file: PathBuf,
        /// Component name (default: exchange)
        #[arg(long)]
        component: Option<String>,
    },
    /// Append a blockquote notification to a document's exchange component
    Notify {
        /// Path to the document
        file: PathBuf,
        /// Notification message (optional when --pending-add is used)
        message: Option<String>,
        /// Source document or session
        #[arg(long)]
        source: Option<String>,
        /// Sections affected (for re-evaluation directive)
        #[arg(long)]
        affects: Option<String>,
        /// Skip git commit after notification
        #[arg(long)]
        no_commit: bool,
        /// Add a pending item to the target document (repeatable). Auto-creates agent:backlog if absent.
        #[arg(long = "pending-add")]
        pending_add: Vec<String>,
        /// Add a gated pending item (repeatable). Like --pending-add but assigns [/] instead of [ ].
        #[arg(long = "pending-add-gated")]
        pending_add_gated: Vec<String>,
        /// Do not auto-create agent:backlog component if absent
        #[arg(long = "no-create-pending")]
        no_create_pending: bool,
    },
    /// Print the path to the shared library (libagent_doc.so/dylib/dll)
    LibPath,
    /// Remove stale versioned shared libraries not in use
    GcLibs {
        /// Target directory (default: directory containing agent-doc binary)
        #[arg(long)]
        target_dir: Option<String>,
    },
    /// Install versioned shared library with atomic symlink swap
    LibInstall {
        /// Source .so path (default: target/release/libagent_doc.so)
        #[arg(long)]
        source: Option<String>,
        /// Target directory (default: directory containing agent-doc binary)
        #[arg(long)]
        target_dir: Option<String>,
    },
    /// List all available commands as JSON (for editor plugin autocomplete)
    #[command(name = "commands")]
    #[allow(clippy::enum_variant_names)]
    ListCommands,
    /// Hook system for cross-session coordination
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Clean up document: compact, prune pending, apply callback results
    Cleanup {
        /// Path to the session document
        file: PathBuf,
        /// Timeout waiting for Claude session response (seconds)
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Polling interval for callback response (milliseconds)
        #[arg(long, default_value_t = 1000)]
        poll_interval: u64,
        /// Model for fallback agent (default: sonnet)
        #[arg(long, default_value = "sonnet")]
        fallback_model: String,
    },
    /// Manage the agent:backlog component (`pending` is a deprecated alias)
    #[command(name = "backlog", alias = "pending")]
    Backlog {
        /// Path to the session document
        file: PathBuf,
        #[command(subcommand)]
        action: PendingAction,
    },
    /// Operate on the review component of a session document
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Manage the agent:queue component
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Resolve typed gates across tracked documents.
    /// Scans documents under the project root for [/<type>] items and flips to [x].
    /// Designed for hook integration: `agent-doc resolve-gate release`
    #[command(name = "resolve-gate")]
    ResolveGateCmd {
        /// Gate type to resolve (e.g., "release", "deploy")
        gate_type: String,
        /// Restrict scan to documents under this directory (defaults to project root)
        #[arg(long)]
        scope: Option<PathBuf>,
    },
    /// Manage bidirectional IPC callbacks
    Callback {
        #[command(subcommand)]
        action: CallbackAction,
    },
    /// Show or change the configured tmux session
    Session {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// Manage the project-local controller shell
    Controller {
        #[command(subcommand)]
        action: ControllerAction,
    },
    /// Fleet-wide actor admin control plane (`#ipc-admin-api`)
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },
}

#[derive(Subcommand)]
enum AdminAction {
    /// Enumerate every actor in the project fleet (one row per document)
    List {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
    },
    /// Derived diagnostics: cross-document pane contention + orphaned bindings
    Detect {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Live read-only fleet dashboard over `admin list` / `admin detect`
    Dashboard {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit a single model snapshot as JSON and exit
        #[arg(long)]
        json: bool,
        /// Render one frame and exit instead of polling
        #[arg(long)]
        once: bool,
        /// Poll interval in milliseconds (default ~1s)
        #[arg(long, default_value_t = agent_doc_orchestration::dashboard::DEFAULT_INTERVAL_MS)]
        interval: u64,
    },
}

#[derive(Subcommand)]
enum OpsAction {
    /// Summarize high-signal ops.log events by document/session
    Summary {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Number of trailing ops.log lines to scan; 0 scans the full file
        #[arg(long, default_value_t = ops_report::default_summary_limit())]
        limit: usize,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Gather cycle/patch diagnostics from agent-doc logs and sidecars
    Diagnose {
        /// Project root to inspect (defaults to --file root or nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Session document path used for project-root and file correlation
        #[arg(long)]
        file: Option<PathBuf>,
        /// Cycle id to correlate, e.g. cycle-1779845677327
        #[arg(long)]
        cycle_id: Option<String>,
        /// Editor IPC patch id to correlate
        #[arg(long)]
        patch_id: Option<String>,
        /// agent_doc_session / harness session id to correlate
        #[arg(long)]
        session_id: Option<String>,
        /// Number of trailing lines to scan in text logs; 0 scans full files
        #[arg(long, default_value_t = ops_report::default_summary_limit())]
        limit: usize,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum JobsAction {
    /// Generate job packets from the current planning record
    Create {
        /// Path to the session document
        file: PathBuf,
        /// Also create an operation document for retained audit/review
        #[arg(long)]
        operation_doc: bool,
        /// Preserve generated packets after success; records intent in index metadata
        #[arg(long)]
        audit: bool,
        /// Token/byte budget used for generated tsift context sidecars
        #[arg(long, default_value_t = 6000)]
        budget: usize,
    },
    /// List generated job packets for a document
    List {
        /// Path to the session document
        file: PathBuf,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
    /// Show job packet completion status
    Status {
        /// Path to the session document
        file: PathBuf,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
    /// Collect worker result sidecars or embedded Worker Result JSON blocks
    Collect {
        /// Path to the session document
        file: PathBuf,
        /// Specific cycle id to collect; defaults to latest
        #[arg(long)]
        cycle: Option<String>,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ControllerAction {
    /// Show project controller status as JSON
    Status {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Lazily launch the controller before reading status
        #[arg(long)]
        ensure: bool,
    },
    /// Run the controller server loop
    #[command(hide = true)]
    Serve {
        /// Project root to serve (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Bootstrap launch mode to persist in controller state
        #[arg(long, default_value = "managed")]
        launch_mode: String,
        /// Private socket used while promoting a replacement controller
        #[arg(long, hide = true)]
        listen_socket: Option<PathBuf>,
        /// Controller generation to persist for a replacement controller
        #[arg(long, hide = true)]
        controller_generation: Option<u64>,
        /// Previous authoritative controller PID during handoff
        #[arg(long, hide = true)]
        previous_controller_pid: Option<u32>,
        /// Handoff state to persist at startup
        #[arg(long, hide = true, default_value = "stable")]
        handoff_state: String,
    },
    /// Stop the project controller if it is running
    Shutdown {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Fire a hook event
    Fire {
        /// Event name (e.g., post_write, post_commit, claim)
        event: String,
        /// Document file path
        file: String,
        /// Session ID (auto-read from frontmatter if omitted)
        #[arg(long)]
        session_id: Option<String>,
        /// JSON data to attach to the event
        #[arg(long)]
        data: Option<String>,
    },
    /// Poll for hook events
    Poll {
        /// Event name to poll
        event: String,
        /// Only return events newer than this timestamp (unix seconds)
        #[arg(long, default_value_t = 0)]
        since: u64,
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Start hook socket listener
    Listen {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Clean up expired events
    Gc {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Check for pending callback requests (called by PostToolUse hooks)
    CheckCallbacks {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Track the active `agent-doc` document for a Codex session (stdin JSON hook payload)
    CodexUserPromptSubmit,
    /// Enforce the Codex end-of-turn `session-check` guard (stdin JSON hook payload)
    CodexStop,
}

#[derive(Subcommand)]
enum PendingAction {
    /// Add an item to the pending component (front of list; assigns stable hash id + `[ ]`)
    Add {
        /// The pending item description. Prefix with canonical `id=<custom> ` to preserve a custom id.
        /// Leading `[#custom] ` is also accepted as compatibility input.
        item: String,
    },
    /// Add a gated item to the pending component (front of list; assigns stable hash id + `[/]`)
    AddGated {
        /// The pending item description. Prefix with canonical `id=<custom> ` to preserve a custom id.
        /// Leading `[#custom] ` is also accepted as compatibility input.
        item: String,
    },
    /// Remove an item from the pending component
    Remove {
        /// Content to match
        target: String,
        /// Treat target as a substring match
        #[arg(long, short)]
        contains: bool,
    },
    /// Remove completed items (legacy — alias for `reap`)
    Prune,
    /// Reap `[x]` items and print removed ids
    Reap,
    /// Run lazy backfill — assign missing hash ids and checkboxes
    Backfill,
    /// Mark an item done by id
    Done {
        /// Hash id (without the `#` prefix)
        id: String,
    },
    /// Rewrite an item's text, preserving its hash id
    Edit {
        /// Hash id (without the `#` prefix)
        id: String,
        /// New item text
        text: String,
    },
    /// Clear all pending items
    Clear,
    /// Reorder items by hash id (comma-separated)
    Reorder {
        /// Comma-separated list of hash ids
        ids: String,
    },
    /// List current pending items
    List,
    /// Resolve all items matching a typed gate (e.g., [/release] → [x])
    ResolveGate {
        /// Gate type to resolve (e.g., "release", "deploy")
        gate_type: String,
    },
    /// Set a typed gate on a gated item ([/] → [/release])
    SetGateType {
        /// Hash id (without the `#` prefix)
        id: String,
        /// Gate type (e.g., "release", "deploy")
        gate_type: String,
    },
}

#[derive(Subcommand)]
enum QueueAction {
    /// One-shot sync from backlog items with `queue` attribute into agent:queue
    Sync {
        /// Path to the session document
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    /// Add a backlog follow-up task for each gated review item so it is driven
    /// back out of review (ungate → done). Idempotent: review ids already
    /// covered by an existing ungate task are skipped.
    #[command(name = "ungate-tasks")]
    UngateTasks {
        /// Path to the session document
        file: PathBuf,
    },
    /// List gated review items in a token-efficient form (id, gate-type, tags,
    /// NEXT-step annotation) so a long review list can be triaged at a glance.
    List {
        /// Path to the session document
        file: PathBuf,
        /// Only items with this typed gate (e.g. `release`)
        #[arg(long)]
        gate_type: Option<String>,
        /// Only items carrying this hashtag (with or without leading `#`)
        #[arg(long)]
        tag: Option<String>,
        /// Only items that have a `NEXT:` annotation (actionable)
        #[arg(long, conflicts_with = "no_next")]
        has_next: bool,
        /// Only items WITHOUT a `NEXT:` annotation (the stale set to triage)
        #[arg(long, conflicts_with = "has_next")]
        no_next: bool,
        /// Emit JSON instead of the compact text form
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CallbackAction {
    /// Create a callback request for a document
    Request {
        /// Path to the session document
        file: PathBuf,
        /// Operations requested (comma-separated: compact,prune-pending,summary)
        operations: String,
        /// Optional additional context
        #[arg(long)]
        context: Option<String>,
        /// TTL in seconds before the request expires
        #[arg(long, default_value_t = 300)]
        ttl: u64,
    },
    /// Read the pending callback request for a document
    Read {
        /// Path to the session document
        file: PathBuf,
    },
    /// Write a callback response for a document
    Respond {
        /// Path to the session document
        file: PathBuf,
        /// The request_id to respond to (must match the pending request)
        #[arg(long)]
        request_id: String,
        /// Response status: "success" or "error"
        #[arg(long, default_value = "success")]
        status: String,
        /// Summary text
        #[arg(long)]
        summary: String,
    },
    /// Clean up expired callback requests
    Gc {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Set the configured tmux session and migrate panes
    Set {
        /// Target tmux session name (e.g., "5")
        name: String,
    },
    /// Show the authoritative actor/session status for a document
    Status {
        /// Path to the session document
        file: PathBuf,
    },
    /// Show the actor/session transition history for a document
    History {
        /// Path to the session document
        file: PathBuf,
    },
    /// Explicitly attach a document session to a tmux pane, creating a new generation
    Attach {
        /// Path to the session document
        file: PathBuf,
        /// Explicit tmux pane ID (defaults to the current pane when inside tmux)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Restart the live session supervisor for a document
    #[command(name = "restart-supervisor", visible_alias = "restart")]
    Restart {
        /// Path to the session document
        file: PathBuf,
        /// Request a fresh restart instead of the default continue-mode restart
        #[arg(long)]
        fresh: bool,
        /// Interrupt a busy live pane and restart the supervisor anyway
        #[arg(long)]
        force: bool,
    },
    /// Clear the configured tmux session when no file is provided, or clear the bound harness session when FILE is provided
    Clear {
        /// Optional path to the session document
        file: Option<PathBuf>,
    },
    /// Intentionally interrupt a live bound harness session, then clear it
    #[command(name = "interrupt-clear")]
    InterruptClear {
        /// Path to the session document
        file: PathBuf,
    },
    /// Diagnose actor/registry/supervisor drift for a document
    Doctor {
        /// Path to the session document
        file: PathBuf,
        /// Escalate into the explicit repair path before re-checking status
        #[arg(long)]
        repair: bool,
    },
    /// Dump the state of ALL actors in a project (actor record + cycle phase +
    /// closeout recovery classification) for investigating state drift
    Debug {
        /// Optional path to a document whose project to inspect (defaults to the
        /// current working directory's project root)
        file: Option<PathBuf>,
        /// Emit a human-readable summary instead of JSON
        #[arg(long)]
        human: bool,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Download and install an editor plugin
    Install {
        /// Editor: jetbrains, vscode
        editor: String,
        /// Install from local build instead of GitHub Releases
        #[clap(long)]
        local: bool,
    },
    /// Update an installed plugin to the latest version
    Update {
        /// Editor: jetbrains, vscode
        editor: String,
    },
    /// List installed editor plugins
    List,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Install the skill definition for the detected (or specified) agent harness
    Install {
        /// After install, output reload instructions: compact (default) or restart
        #[arg(long)]
        reload: Option<String>,
        /// Target harness: claude, opencode, codex, cursor, generic (auto-detected if omitted)
        #[arg(long)]
        harness: Option<String>,
        /// Install for all supported harnesses
        #[arg(long)]
        all: bool,
    },
    /// Check if the installed skill matches the binary version
    Check,
}

/// Initialize structured logging. When `AGENT_DOC_LOG` is set (e.g., "debug"),
/// logs are written to `.agent-doc/logs/debug.log`. When unset, this is a no-op.
fn init_tracing() {
    let filter = match std::env::var("AGENT_DOC_LOG") {
        Ok(val) => val,
        Err(_) => return, // No logging configured — zero overhead
    };

    // Find .agent-doc/logs/ directory (walk up from CWD)
    let log_dir = {
        let mut dir = std::env::current_dir().unwrap_or_default();
        loop {
            let candidate = dir.join(".agent-doc/logs");
            if candidate.is_dir() {
                break Some(candidate);
            }
            if !dir.pop() {
                break None;
            }
        }
    };

    let Some(log_dir) = log_dir else {
        eprintln!("[tracing] AGENT_DOC_LOG set but no .agent-doc/logs/ found — logging disabled");
        return;
    };

    let file_appender = tracing_appender::rolling::daily(&log_dir, "debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard so it lives for the program lifetime
    std::mem::forget(_guard);

    use tracing_subscriber::EnvFilter;
    let env_filter = EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("debug"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .init();

    tracing::debug!("agent-doc tracing initialized (filter: {})", filter);
}

fn main() -> anyhow::Result<()> {
    // Initialize structured logging via AGENT_DOC_LOG env var.
    // Examples: AGENT_DOC_LOG=debug, AGENT_DOC_LOG=agent_doc::preflight=debug
    // When set, logs to .agent-doc/logs/debug.log (auto-rotated).
    // When unset, no file logging (zero overhead).
    init_tracing();

    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let pending_alias_used = deprecated_pending_alias_used(&raw_args);
    let done_flag_alias_used = deprecated_done_flag_used(&raw_args);
    let cli = Cli::parse_from(rewrite_bare_file_invocation(raw_args));

    // Warn about newer versions on startup, but skip if running the upgrade command itself.
    if !matches!(cli.command, Commands::Upgrade) {
        upgrade::warn_if_outdated();
    }

    if pending_alias_used && matches!(cli.command, Commands::Backlog { .. }) {
        eprintln!(
            "[deprecation] `agent-doc pending` is deprecated — use `agent-doc backlog` instead"
        );
    }
    if done_flag_alias_used
        && matches!(
            cli.command,
            Commands::Write { .. } | Commands::Finalize { .. }
        )
    {
        eprintln!(
            "[deprecation] `--pending-done` and `--backlog-done` are deprecated — use `--done` instead"
        );
    }

    let config = agent_doc_orchestration::config::load()?;

    match cli.command {
        Commands::Run {
            file,
            branch,
            agent,
            model,
            dry_run,
            no_git,
        } => agent_doc_orchestration::run::run(
            &file,
            branch,
            agent.as_deref(),
            model.as_deref(),
            dry_run,
            no_git,
            &config,
        ),
        Commands::History { file, restore } => match restore {
            Some(commit) => history::restore(&file, &commit),
            None => history::list(&file),
        },
        Commands::Log { file } => history::log(&file),
        Commands::Ops { action } => match action {
            OpsAction::Summary {
                project_root,
                limit,
                json,
            } => ops_report::run_summary(project_root.as_deref(), limit, json),
            OpsAction::Diagnose {
                project_root,
                file,
                cycle_id,
                patch_id,
                session_id,
                limit,
                json,
            } => ops_report::run_diagnose(
                project_root.as_deref(),
                file.as_deref(),
                cycle_id.as_deref(),
                patch_id.as_deref(),
                session_id.as_deref(),
                limit,
                json,
            ),
        },
        Commands::Show {
            file,
            back,
            at,
            tag,
        } => history::show(&file, back, at, tag.as_deref()),
        Commands::Init {
            file,
            title,
            agent,
            mode,
        } => init::run(
            file.as_deref(),
            title.as_deref(),
            agent.as_deref(),
            mode.as_deref(),
            &config,
        ),
        Commands::Install {
            editor,
            skip_prereqs,
            skip_plugins,
        } => install::run(editor.as_deref(), skip_prereqs, skip_plugins),
        Commands::Diff {
            file,
            wait,
            from,
            to,
        } => {
            if let Some(from_ref) = from {
                let to_ref = to.as_deref().unwrap_or("HEAD");
                history::git_diff(&file, &from_ref, to_ref)
            } else {
                agent_doc_orchestration::diff::run(&file, wait)
            }
        }
        Commands::Reset {
            file,
            from_current,
            preserve_session,
        } => reset::run(&file, from_current, preserve_session),
        Commands::Clean { file, archive } => clean::run(&file, archive),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Checkpoint {
            file,
            restore,
            diff,
        } => agent_doc_orchestration::checkpoint::run(&file, restore.as_deref(), diff.as_deref()),
        Commands::Gc { root, dry_run } => {
            let result = agent_doc_orchestration::gc::run(root.as_deref(), dry_run)?;
            if dry_run {
                eprintln!(
                    "[gc] Dry run: {} files would be deleted, {} kept",
                    result.deleted, result.skipped
                );
            }
            Ok(())
        }
        Commands::Start {
            file,
            force,
            route_owned,
            route_owned_reap_policy,
        } => match route_owned_reap_policy {
            agent_doc_orchestration::start::RouteOwnedReapPolicy::Auto => {
                agent_doc_orchestration::start::run(&file, force, route_owned)
            }
            policy => agent_doc_orchestration::start::run_with_reap_policy(
                &file,
                force,
                route_owned,
                policy,
            ),
        },
        Commands::Route {
            file,
            dispatch_only,
            plain_trigger,
            pane,
            cols,
            focus: _focus,
            debounce,
            wait_for_ready,
        } => {
            // NOTE: agent_doc_orchestration::sync::run_layout_only was previously called here after route when
            // --col args were provided. Removed because the JB plugin calls `agent-doc sync`
            // separately with the correct --window arg. Running sync from both route AND
            // the plugin created a double-sync glitch (panes bouncing between stash and
            // agent-doc window). The plugin's sync is authoritative for layout.
            let mode = if dispatch_only {
                agent_doc_orchestration::route::RouteMode::DispatchOnly
            } else {
                agent_doc_orchestration::route::RouteMode::Managed
            };
            let wait_for_ready =
                wait_for_ready.map(|secs| std::time::Duration::from_secs(secs.min(600)));
            agent_doc_orchestration::route::run(
                &file,
                pane.as_deref(),
                debounce,
                &cols,
                mode,
                plain_trigger,
                wait_for_ready,
            )
        }
        Commands::Prompt { file, answer, all } => {
            if all {
                return agent_doc_orchestration::prompt::run_all();
            }
            let file = file.context("FILE required when not using --all")?;
            match answer {
                Some(option) => agent_doc_orchestration::prompt::answer(&file, option),
                None => agent_doc_orchestration::prompt::run(&file),
            }
        }
        Commands::Commit { file } => agent_doc_orchestration::git::commit(&file).map(|_| ()),
        Commands::Dedupe { file } => agent_doc_orchestration::dedupe::run(&file),
        Commands::Cancel { file } => {
            match agent_doc_orchestration::repair::cancel_preflight_cycle(&file)? {
                agent_doc_orchestration::repair::CancelOutcome::Abandoned => {
                    println!(
                        "[cancel] abandoned orphaned preflight_started cycle; next dispatch starts fresh"
                    );
                }
                agent_doc_orchestration::repair::CancelOutcome::NoOpenCycle => {
                    println!("[cancel] no open cycle to reclaim");
                }
                agent_doc_orchestration::repair::CancelOutcome::Protected => {
                    println!(
                        "[cancel] open cycle owns real work (advanced past preflight or has a response capture); left intact"
                    );
                }
            }
            Ok(())
        }
        Commands::Claim {
            file,
            position,
            pane,
            window,
            force,
            isolate,
        } => agent_doc_orchestration::claim::run(
            &file,
            position.as_deref(),
            pane.as_deref(),
            window.as_deref(),
            force,
            isolate,
        ),
        Commands::Focus {
            file,
            pane,
            no_stash_promote,
        } => {
            if no_stash_promote {
                agent_doc_orchestration::focus::run_no_promote(&file, pane.as_deref())
            } else {
                agent_doc_orchestration::focus::run(&file, pane.as_deref())
            }
        }
        Commands::Layout {
            files,
            split,
            pane,
            window,
        } => {
            let split = match split.as_str() {
                "v" | "vertical" => layout::Split::Vertical,
                _ => layout::Split::Horizontal,
            };
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            layout::run(&paths, split, pane.as_deref(), window.as_deref())
        }
        Commands::Sync {
            columns,
            window,
            focus,
            rename,
            no_autostart,
            exact_visible,
        } => {
            if rename && let Some(ref f) = focus {
                agent_doc_orchestration::sync::write_rename_debounce(f);
            }
            if no_autostart {
                if exact_visible {
                    agent_doc_orchestration::sync::run_layout_only_exact_visible(
                        &columns,
                        window.as_deref(),
                        focus.as_deref(),
                    )
                } else {
                    agent_doc_orchestration::sync::run_layout_only(
                        &columns,
                        window.as_deref(),
                        focus.as_deref(),
                    )
                }
            } else {
                agent_doc_orchestration::sync::run(&columns, window.as_deref(), focus.as_deref())
            }
        }
        Commands::Patch {
            file,
            component,
            mode,
            content,
        } => patch::run(&file, &component, mode, content.as_deref()),
        Commands::Watch {
            stop,
            status,
            debounce,
            max_cycles,
        } => {
            if stop {
                agent_doc_orchestration::watch::stop()
            } else if status {
                agent_doc_orchestration::watch::status()
            } else {
                agent_doc_orchestration::watch::start(
                    &config,
                    agent_doc_orchestration::watch::WatchConfig {
                        debounce_ms: debounce,
                        max_cycles,
                    },
                )
            }
        }
        Commands::Outline { file, json } => outline::run(&file, json),
        Commands::AutoDag { file, json } => {
            agent_doc_orchestration::auto_dag::run(&file, json)
        }
        Commands::Resync { file, fix, session } => {
            if fix {
                agent_doc_orchestration::resync::run_fix(file.as_deref(), session.as_deref())
            } else {
                agent_doc_orchestration::resync::run(false, session.as_deref(), file.as_deref())
            }
        }
        Commands::Fix { file, session } => {
            agent_doc_orchestration::resync::run_fix(file.as_deref(), session.as_deref())
        }
        Commands::Skill { command } => {
            match command {
                SkillCommands::Install {
                    reload,
                    harness,
                    all,
                } => {
                    if all {
                        skill::install_all()?;
                    } else if let Some(ref h) = harness {
                        let env = agent_kit::detect::Environment::from_name(h)
                        .ok_or_else(|| anyhow::anyhow!(
                            "unknown harness '{}'. Valid: claude, opencode, codex, cursor, generic", h
                        ))?;
                        skill::install_for(env)?;
                    } else {
                        let updated = skill::install_and_check_updated()?;
                        if updated && let Some(ref mode) = reload {
                            match mode.as_str() {
                                "restart" => {
                                    println!("SKILL_RELOAD=restart");
                                    println!(
                                        "Skill updated. Please restart this session with --resume to reload the skill."
                                    );
                                }
                                _ => {
                                    println!("SKILL_RELOAD=compact");
                                    println!(
                                        "Skill updated. Please run /compact to reload the updated skill instructions."
                                    );
                                }
                            }
                        }
                    }
                    Ok(())
                }
                SkillCommands::Check => skill::check(),
            }
        }
        Commands::Plugin { action } => match action {
            PluginAction::Install { editor, local } => {
                if local {
                    plugin::install_local(&editor)
                } else {
                    plugin::install(&editor)
                }
            }
            PluginAction::Update { editor } => plugin::update(&editor),
            PluginAction::List => plugin::list(),
        },
        Commands::Write { args, commit } => {
            let lint_override = match args.lint.as_deref() {
                None => None,
                Some(s) => Some(
                    agent_doc_orchestration::lint_gate::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
            };
            agent_doc_orchestration::write::run_command(
                agent_doc_orchestration::write::CommandOptions {
                    file: args.file,
                    baseline_file: args.baseline_file,
                    is_template: args.template,
                    is_stream: args.stream,
                    is_ipc: args.ipc,
                    force_disk: args.force_disk,
                    origin: args.origin,
                    pending_add: args.pending_add,
                    pending_add_to: args.pending_add_to,
                    pending_add_gated: args.pending_add_gated,
                    pending_add_after: args.pending_add_after,
                    pending_add_before: args.pending_add_before,
                    pending_add_back: args.pending_add_back,
                    pending_done: args.pending_done,
                    pending_edit: args.pending_edit,
                    pending_clear: args.pending_clear,
                    pending_reorder: args.pending_reorder,
                    pending_gate: args.pending_gate,
                    pending_ungate: args.pending_ungate,
                    pending_resolve_gate: args.pending_resolve_gate,
                    pending_set_gate_type: args.pending_set_gate_type,
                    review_add: args.review_add,
                    review_edit: args.review_edit,
                    allow_replace_pending: args.allow_replace_pending,
                    pending_only: args.pending_only,
                    status: args.status,
                    lint_override,
                    commit_sibling: args.commit_sibling,
                    commit_sibling_message: args.commit_sibling_message,
                },
                if commit {
                    agent_doc_orchestration::write::CommitMode::BestEffort
                } else {
                    agent_doc_orchestration::write::CommitMode::None
                },
            )
        }
        Commands::Finalize { args } => {
            let lint_override = match args.lint.as_deref() {
                None => None,
                Some(s) => Some(
                    agent_doc_orchestration::lint_gate::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
            };
            agent_doc_orchestration::write::run_command(
                agent_doc_orchestration::write::CommandOptions {
                    file: args.file,
                    baseline_file: args.baseline_file,
                    is_template: args.template,
                    is_stream: args.stream,
                    is_ipc: args.ipc,
                    force_disk: args.force_disk,
                    origin: args.origin,
                    pending_add: args.pending_add,
                    pending_add_to: args.pending_add_to,
                    pending_add_gated: args.pending_add_gated,
                    pending_add_after: args.pending_add_after,
                    pending_add_before: args.pending_add_before,
                    pending_add_back: args.pending_add_back,
                    pending_done: args.pending_done,
                    pending_edit: args.pending_edit,
                    pending_clear: args.pending_clear,
                    pending_reorder: args.pending_reorder,
                    pending_gate: args.pending_gate,
                    pending_ungate: args.pending_ungate,
                    pending_resolve_gate: args.pending_resolve_gate,
                    pending_set_gate_type: args.pending_set_gate_type,
                    review_add: args.review_add,
                    review_edit: args.review_edit,
                    allow_replace_pending: args.allow_replace_pending,
                    pending_only: args.pending_only,
                    status: args.status,
                    lint_override,
                    commit_sibling: args.commit_sibling,
                    commit_sibling_message: args.commit_sibling_message,
                },
                agent_doc_orchestration::write::CommitMode::Required,
            )
        }
        Commands::Stream {
            file,
            interval,
            agent,
            model,
            no_git,
            lint,
        } => {
            let lint_override = match lint.as_deref() {
                Some(s) => Some(
                    agent_doc_orchestration::lint_gate::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
                None => None,
            };
            agent_doc_orchestration::stream::run(
                &file,
                interval,
                agent.as_deref(),
                model.as_deref(),
                no_git,
                &config,
                lint_override,
            )
        }
        Commands::TemplateInfo { file } => {
            let info = template::template_info(&file)?;
            println!("{}", serde_json::to_string_pretty(&info)?);
            Ok(())
        }
        Commands::Repair {
            file,
            apply_recovery,
        } => {
            if apply_recovery {
                use agent_doc_orchestration::flow::closeout::RecoveryApplication;
                match agent_doc_orchestration::flow::closeout::apply_closeout_recovery(&file)? {
                    RecoveryApplication::NothingToDo => {
                        eprintln!("[repair] {} is clean — no recovery needed", file.display());
                    }
                    RecoveryApplication::Applied { state, action } => {
                        eprintln!(
                            "[repair] applied recovery [{}]: {} for {}",
                            state.as_str(),
                            action,
                            file.display()
                        );
                    }
                    RecoveryApplication::NotApplied {
                        state,
                        reason,
                        recommended,
                    } => {
                        eprintln!(
                            "[repair] recovery [{}] not auto-applied: {}\n[repair] run: {}",
                            state.as_str(),
                            reason,
                            recommended
                        );
                    }
                }
                return Ok(());
            }
            let outcome = agent_doc_orchestration::repair::repair(&file)?;
            if !outcome.repaired() {
                eprintln!("[repair] No pending response found for {}", file.display());
            }
            Ok(())
        }
        Commands::Preflight { file, probe } => {
            agent_doc_orchestration::preflight::run_with_options(
                &file,
                agent_doc_orchestration::preflight::PreflightOptions { probe },
            )
        }
        Commands::Plan { file } => plan::run(&file),
        Commands::Jobs { action } => match action {
            JobsAction::Create {
                file,
                operation_doc,
                audit,
                budget,
            } => jobs::create(
                &file,
                jobs::CreateOptions {
                    operation_doc,
                    audit,
                    budget,
                },
            ),
            JobsAction::List { file, json } => jobs::list(&file, json),
            JobsAction::Status { file, json } => jobs::status(&file, json),
            JobsAction::Collect { file, cycle, json } => {
                jobs::collect(&file, cycle.as_deref(), json)
            }
        },
        Commands::SessionCheck {
            file,
            codex_final_gate,
        } => agent_doc_orchestration::session_check::run_with_options(&file, codex_final_gate),
        Commands::Serve {
            file,
            host,
            port,
            auth_token,
            read_only_token,
            tls_cert,
            tls_key,
        } => serve::run(serve::ServeOptions::new(
            file,
            host,
            port,
            auth_token,
            read_only_token,
            tls_cert,
            tls_key,
        )),
        Commands::Read { file, component } => read::run(&file, component.as_deref()),
        Commands::DescribeImage {
            image,
            provider,
            model,
            api_key,
            prompt,
        } => describe_image::run(
            &image,
            provider.as_deref(),
            model.as_deref(),
            api_key.as_deref(),
            prompt.as_deref(),
        ),
        Commands::ResponseToc {
            file,
            backlog_id,
            query,
            limit,
            json,
        } => agent_doc_orchestration::response_toc::run_toc(
            &file,
            backlog_id.as_deref(),
            query.as_deref(),
            limit,
            json,
        ),
        Commands::ResponseFetch {
            file,
            locator,
            before,
            after,
            json,
        } => agent_doc_orchestration::response_toc::run_fetch(&file, &locator, before, after, json),
        Commands::ArchiveIndex { file, rebuild } => {
            agent_doc_orchestration::archive_index::run_index(&file, rebuild)
        }
        Commands::ArchiveSearch {
            file,
            query,
            backlog_id,
            session,
            limit,
            json,
            rebuild,
        } => agent_doc_orchestration::archive_index::run_search(
            &file,
            query.as_deref(),
            backlog_id.as_deref(),
            session.as_deref(),
            limit,
            json,
            rebuild,
        ),
        Commands::Compact {
            file,
            keep,
            component,
            message,
            tag,
            commit,
        } => agent_doc_orchestration::compact::run(
            &file,
            keep,
            component.as_deref(),
            message.as_deref(),
            tag.as_deref(),
            commit,
        ),
        Commands::Convert {
            file,
            mode,
            agent_doc_format,
            agent_doc_write,
        } => convert::run(&file, mode.as_ref(), agent_doc_format, agent_doc_write),
        Commands::Mode { file, set } => mode::run(&file, set.as_deref()),
        Commands::Annotate {
            file,
            force,
            history,
        } => annotate::run(&file, force, history),
        Commands::Undo { file } => undo::run(&file),
        Commands::Extract {
            source,
            target,
            component,
        } => extract::run(&source, &target, component.as_deref()),
        Commands::Transfer {
            source,
            target,
            component,
            bypass_claim,
            items,
            referral,
        } => {
            let item_ids: Option<Vec<String>> = items.map(|s| {
                s.split(',')
                    .map(|id| id.trim().trim_start_matches('#').to_string())
                    .collect()
            });
            extract::transfer(
                &source,
                &target,
                &component,
                bypass_claim,
                item_ids.as_deref(),
                referral,
            )
        }
        Commands::Rename { old_path, new_path } => rename::run(&old_path, &new_path),
        Commands::Migrate {
            files,
            all,
            dry_run,
        } => migrate::run(&files, all, dry_run),
        Commands::Claims => {
            let cwd = std::env::current_dir()?;
            if let Some(root) = agent_doc_orchestration::snapshot::find_project_root(&cwd) {
                let log_path = root.join(".agent-doc/claims.log");
                if let Ok(contents) = std::fs::read_to_string(&log_path)
                    && !contents.is_empty()
                {
                    print!("{}", contents);
                    std::fs::write(&log_path, "")?;
                }
            }
            Ok(())
        }
        Commands::Parallel {
            file,
            tasks_explicit,
            model,
            no_git,
            no_worktree,
            timeout,
            dry_run,
        } => orchestrate::run_parallel_compat(
            &file,
            parallel::ParallelConfig {
                tasks: tasks_explicit
                    .into_iter()
                    .map(|task| parallel::ParallelTask {
                        description: task.clone(),
                        prompt: task,
                    })
                    .collect(),
                model,
                no_git,
                no_worktree,
                timeout_secs: timeout,
                dry_run,
            },
            &config,
        ),
        Commands::Orchestrate {
            file,
            mode,
            tasks_explicit,
            from_file,
            from_exchange,
            from_queue,
            resume_schedule,
            agent,
            model,
            no_git,
            no_worktree,
            timeout,
            dry_run,
            plan,
        } => orchestrate::run(
            &file,
            orchestrate::OrchestrateConfig {
                mode,
                tasks_explicit,
                from_file,
                from_exchange,
                from_queue,
                resume_schedule,
                agent,
                model,
                no_git,
                no_worktree,
                timeout_secs: timeout,
                dry_run,
                plan,
            },
            &config,
        ),
        Commands::Notify {
            file,
            message,
            source,
            affects,
            no_commit,
            pending_add,
            pending_add_gated,
            no_create_pending,
        } => notify::run(
            &file,
            message.as_deref(),
            source.as_deref(),
            affects.as_deref(),
            !no_commit,
            &pending_add,
            &pending_add_gated,
            no_create_pending,
        ),
        Commands::Boundary { file, component } => {
            agent_doc_orchestration::boundary::run(&file, component.as_deref())
        }
        Commands::Terminal { file, session } => terminal::run(&file, session.as_deref()),
        Commands::Autoclaim => autoclaim::run(),
        Commands::TurnStatus { action } => match action {
            TurnStatusAction::Active => agent_doc_orchestration::turn_status::run(true),
            TurnStatusAction::Idle => agent_doc_orchestration::turn_status::run(false),
            TurnStatusAction::Install { dir, user, tmux } => {
                skill::install_turn_status_hooks(dir.as_deref(), user, tmux)
            }
        },
        Commands::Upgrade => upgrade::run(),
        Commands::LibPath => {
            // Print the path to the shared library built alongside this binary.
            // The cdylib is in the same target directory as the binary.
            let exe = std::env::current_exe()?;
            let dir = exe.parent().unwrap();
            #[cfg(target_os = "linux")]
            let lib_name = "libagent_doc.so";
            #[cfg(target_os = "macos")]
            let lib_name = "libagent_doc.dylib";
            #[cfg(target_os = "windows")]
            let lib_name = "agent_doc.dll";
            let lib_path = dir.join(lib_name);
            if lib_path.exists() {
                println!("{}", lib_path.display());
            } else {
                eprintln!("[lib-path] library not found at {}", lib_path.display());
                eprintln!("[lib-path] build with: cargo build --release");
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::GcLibs { target_dir } => lib_gc::run(target_dir.as_deref()),
        Commands::LibInstall { source, target_dir } => {
            lib_install::run(source.as_deref(), target_dir.as_deref())
        }
        Commands::ListCommands => commands::run(),
        Commands::Session { action } => match action {
            Some(SessionAction::Set { name }) => session_cmd::set(&name),
            Some(SessionAction::Status { file }) => session_actor_cmd::status(&file),
            Some(SessionAction::History { file }) => session_actor_cmd::history(&file),
            Some(SessionAction::Attach { file, pane }) => {
                session_actor_cmd::attach(&file, pane.as_deref())
            }
            Some(SessionAction::Restart { file, fresh, force }) => session_actor_cmd::restart(
                &file,
                if fresh {
                    session_actor_cmd::RestartMode::Fresh
                } else {
                    session_actor_cmd::RestartMode::Continue
                },
                force,
            ),
            Some(SessionAction::Clear { file: Some(file) }) => session_actor_cmd::clear(&file),
            Some(SessionAction::Clear { file: None }) => session_cmd::clear(),
            Some(SessionAction::InterruptClear { file }) => {
                session_actor_cmd::interrupt_clear(&file)
            }
            Some(SessionAction::Doctor { file, repair }) => {
                session_actor_cmd::doctor(&file, repair)
            }
            Some(SessionAction::Debug { file, human }) => {
                session_actor_cmd::debug(file.as_deref(), !human)
            }
            None => session_cmd::show(),
        },
        Commands::Controller { action } => match action {
            ControllerAction::Status {
                project_root,
                ensure,
            } => agent_doc_orchestration::project_controller::run_status(
                project_root.as_deref(),
                ensure,
            ),
            ControllerAction::Serve {
                project_root,
                launch_mode,
                listen_socket,
                controller_generation,
                previous_controller_pid,
                handoff_state,
            } => agent_doc_orchestration::project_controller::run_serve(
                project_root.as_deref(),
                &launch_mode,
                listen_socket.as_deref(),
                controller_generation,
                previous_controller_pid,
                &handoff_state,
            ),
            ControllerAction::Shutdown { project_root } => {
                agent_doc_orchestration::project_controller::run_shutdown(project_root.as_deref())
            }
        },
        Commands::Admin { action } => match action {
            AdminAction::List { project_root, json } => {
                agent_doc_orchestration::admin::list(project_root.as_deref(), json)
            }
            AdminAction::Detect { project_root, json } => {
                agent_doc_orchestration::admin::detect(project_root.as_deref(), json)
            }
            AdminAction::Dashboard {
                project_root,
                json,
                once,
                interval,
            } => agent_doc_orchestration::dashboard::dashboard(
                project_root.as_deref(),
                json,
                once,
                interval,
            ),
        },
        Commands::Hook { action } => match action {
            HookAction::Fire {
                event,
                file,
                session_id,
                data,
            } => hook_cmd::fire(&event, &file, session_id.as_deref(), data.as_deref()),
            HookAction::Poll { event, since, root } => {
                hook_cmd::poll(&event, since, root.as_deref())
            }
            HookAction::Listen { root } => hook_cmd::listen(root.as_deref()),
            HookAction::Gc { root } => hook_cmd::gc(root.as_deref()),
            HookAction::CheckCallbacks { root } => {
                let pending =
                    agent_doc_orchestration::callback::scan_pending_callbacks(root.as_deref())?;
                let json = serde_json::to_string_pretty(
                    &serde_json::json!({"pending_callbacks": pending}),
                )?;
                println!("{}", json);
                Ok(())
            }
            HookAction::CodexUserPromptSubmit => {
                agent_doc_orchestration::codex_hook::handle_user_prompt_submit()
            }
            HookAction::CodexStop => agent_doc_orchestration::codex_hook::handle_stop(),
        },
        Commands::Cleanup {
            file,
            timeout,
            poll_interval,
            fallback_model,
        } => cleanup_cmd::run(&file, timeout, poll_interval, &fallback_model),
        Commands::Backlog { file, action } => match action {
            PendingAction::Add { item } => {
                agent_doc_orchestration::pending_cmd::add(&file, &item, false)
            }
            PendingAction::AddGated { item } => {
                agent_doc_orchestration::pending_cmd::add(&file, &item, true)
            }
            PendingAction::Remove { target, contains } => {
                agent_doc_orchestration::pending_cmd::remove(&file, &target, contains)
            }
            PendingAction::Prune => agent_doc_orchestration::pending_cmd::reap(&file),
            PendingAction::Reap => agent_doc_orchestration::pending_cmd::reap(&file),
            PendingAction::Backfill => agent_doc_orchestration::pending_cmd::backfill(&file),
            PendingAction::Done { id } => agent_doc_orchestration::pending_cmd::done(&file, &id),
            PendingAction::Edit { id, text } => {
                agent_doc_orchestration::pending_cmd::edit(&file, &id, &text)
            }
            PendingAction::Clear => agent_doc_orchestration::pending_cmd::clear(&file),
            PendingAction::Reorder { ids } => {
                let ids: Vec<String> = ids
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                agent_doc_orchestration::pending_cmd::reorder(&file, &ids)
            }
            PendingAction::List => agent_doc_orchestration::pending_cmd::list(&file),
            PendingAction::ResolveGate { gate_type } => {
                agent_doc_orchestration::pending_cmd::resolve_gate(&file, &gate_type)
            }
            PendingAction::SetGateType { id, gate_type } => {
                agent_doc_orchestration::pending_cmd::set_gate_type(&file, &id, &gate_type)
            }
        },
        Commands::Review { action } => match action {
            ReviewAction::UngateTasks { file } => {
                let report =
                    agent_doc_orchestration::pending_cmd::add_ungate_tasks_for_review(&file)?;
                println!("  scanned review: {} gated item(s)", report.scanned);
                println!("  added {} backlog ungate task(s)", report.added.len());
                println!("  (skipped {} already-tracked)", report.skipped.len());
                Ok(())
            }
            ReviewAction::List {
                file,
                gate_type,
                tag,
                has_next,
                no_next,
                json,
            } => {
                let filter = agent_doc_orchestration::pending_cmd::ReviewListFilter {
                    gate_type,
                    tag,
                    has_next: if has_next {
                        Some(true)
                    } else if no_next {
                        Some(false)
                    } else {
                        None
                    },
                };
                let items =
                    agent_doc_orchestration::pending_cmd::list_review_items(&file, &filter)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    println!("(no gated review items match)");
                } else {
                    for v in &items {
                        let gate = v
                            .gate_type
                            .as_deref()
                            .map(|g| format!(" [{g}]"))
                            .unwrap_or_default();
                        let tags = if v.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", v.tags.join(" "))
                        };
                        println!("#{}{}  {}{}", v.id, gate, v.summary, tags);
                        if let Some(next) = &v.next {
                            println!("    → NEXT: {next}");
                        }
                    }
                    println!("\n{} gated review item(s)", items.len());
                }
                Ok(())
            }
        },
        Commands::Queue { action } => match action {
            QueueAction::Sync { file } => agent_doc_orchestration::queue_cmd::sync(&file),
        },
        Commands::ResolveGateCmd { gate_type, scope } => {
            // Determine scan root: explicit --scope, or cwd, or project root
            let scan_root = if let Some(s) = scope {
                s
            } else {
                let cwd = std::env::current_dir()?;
                agent_doc_orchestration::snapshot::find_project_root(&cwd).unwrap_or(cwd)
            };
            let total =
                agent_doc_orchestration::pending_cmd::resolve_gate_scan(&gate_type, &scan_root)?;
            if total == 0 {
                eprintln!(
                    "[resolve-gate] no [/{}] items found under {}",
                    gate_type,
                    scan_root.display()
                );
            } else {
                eprintln!(
                    "[resolve-gate] resolved {} total [/{}] item(s)",
                    total, gate_type
                );
            }
            Ok(())
        }
        Commands::Callback { action } => match action {
            CallbackAction::Request {
                file,
                operations,
                context,
                ttl,
            } => {
                let ops: Vec<&str> = operations.split(',').map(|s| s.trim()).collect();
                let request = agent_doc_orchestration::callback::create_request(
                    &file,
                    &ops,
                    context.as_deref(),
                    ttl,
                )?;
                println!("{}", serde_json::to_string_pretty(&request)?);
                Ok(())
            }
            CallbackAction::Read { file } => {
                match agent_doc_orchestration::callback::read_request(&file)? {
                    Some(request) => {
                        println!("{}", serde_json::to_string_pretty(&request)?);
                    }
                    None => {
                        println!("{{}}");
                        eprintln!("[callback] no pending request for {}", file.display());
                    }
                }
                Ok(())
            }
            CallbackAction::Respond {
                file,
                request_id,
                status,
                summary,
            } => {
                agent_doc_orchestration::callback::write_response(
                    &file,
                    &request_id,
                    &status,
                    &summary,
                    None,
                )?;
                eprintln!("[callback] response written for request {}", request_id);
                Ok(())
            }
            CallbackAction::Gc { root } => {
                let cwd = std::env::current_dir()?;
                let root_path = root
                    .map(PathBuf::from)
                    .or_else(|| agent_doc_orchestration::snapshot::find_project_root(&cwd))
                    .context("could not find project root")?;
                agent_doc_orchestration::callback::cleanup_expired(&root_path, 300)
            }
        },
    }
}
