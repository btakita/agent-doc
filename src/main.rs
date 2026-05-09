//! # Module: main (agent-doc CLI)
//!
//! ## Spec
//! - Entry point for the `agent-doc` binary; parses the command line with `clap` derive.
//! - Top-level struct `Cli` holds a single `Commands` subcommand enum (40+ variants).
//! - `AgentDocMode` enum (`Append`, `Template`, `Stream`) is a `ValueEnum` used by `Convert`
//!   and `Mode` subcommands; `Append` maps to inline format, `Template`/`Stream` to CRDT.
//! - On startup, calls `upgrade::warn_if_outdated()` for all subcommands except `Upgrade`.
//! - Loads global config via `config::load()` before dispatching; config is threaded into
//!   subcommands that accept an agent backend (`Run`, `Stream`, `Watch`, `Init`).
//! - Each subcommand delegates immediately to its own module (`run::run`, `diff::run`, etc.);
//!   `main` contains no business logic beyond argument destructuring and dispatch.
//! - `Route` no longer runs a follow-up sync; editor/plugin sync remains the
//!   authoritative layout path.
//! - `Write` auto-detects the write strategy from frontmatter when no `--template`/`--stream`
//!   flag is given; CRDT-mode documents use `write::run_stream`, others use `write::run`.
//! - `Prompt --all` runs `prompt::run_all()`; otherwise `FILE` is required.
//! - `History --restore <commit>` calls `history::restore`; bare `History` calls `history::list`.
//! - `Watch` dispatches to `watch::stop`, `watch::status`, or `watch::start` based on flags.
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
//! - dispatch_run: `agent-doc run <file>` → `run::run` called with correct args
//! - dispatch_write_crdt_autodetect: CRDT frontmatter + no flags → `write::run_stream` selected
//! - dispatch_write_inline_autodetect: inline frontmatter + no flags → `write::run` selected
//! - dispatch_prompt_all: `--all` → `prompt::run_all`, no FILE required
//! - dispatch_history_restore: `--restore <sha>` → `history::restore` called
//! - dispatch_watch_stop: `--stop` flag → `watch::stop` called
//! - dispatch_skill_install_reload: skill updated + `--reload compact` → prints `SKILL_RELOAD=compact`
//! - dispatch_lib_path_missing: library absent → exits with code 1

mod agent;
mod annotate;
mod archive_index;
mod audit_docs;
mod autoclaim;
mod boundary;
mod callback;
mod capture;
mod claim;
mod clean;
mod cleanup_cmd;
mod codex_hook;
mod commands;
mod compact;
mod config;
mod convert;
mod cycle_state;
mod dedupe;
mod diff;
mod env;
mod extract;
mod focus;
mod fs_util;
mod gc;
mod git;
mod harness;
mod harness_prompt;
mod heuristics;
mod history;
mod hook_cmd;
mod hooks;
mod init;
mod install;
mod layout;
mod lib_gc;
mod lib_install;
mod migrate;
mod mode;
mod notify;
mod orchestrate;
mod outline;
mod parallel;
mod patch;
mod pending;
mod pending_cmd;
mod plan;
mod plugin;
mod preflight;
mod project_config;
mod project_controller;
mod prompt;
mod prompt_context;
mod prompt_contract;
mod queue;
mod queue_dispatch;
mod read;
mod rename;
mod repair;
mod replay_guard;
mod reset;
mod response_toc;
mod resync;
mod route;
mod run;
mod security;
mod session_accretion;
mod session_actor;
mod session_actor_cmd;
mod session_check;
mod session_cmd;
mod sessions;
#[cfg(test)]
mod sim_world;
mod skill;
mod snapshot;
mod start;
mod startup_miss;
mod status_cmd;
mod stream;
mod supervisor;
mod sync;
mod terminal;
#[cfg(test)]
mod test_support;
pub(crate) use agent_doc::ipc_socket;
mod ops_log;
mod undo;
mod upgrade;
mod watch;
mod worktree;
mod write;

// Re-export library modules so binary-internal modules can use `crate::` paths
pub(crate) use agent_doc::component;
pub(crate) use agent_doc::crdt;
pub(crate) use agent_doc::frontmatter;
pub(crate) use agent_doc::merge;
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
    /// Add a new gated pending item at the beginning of the list (repeatable).
    /// Prefix with canonical `id=<custom> ` to preserve a custom id instead of generating one.
    /// Leading `[#custom] ` is also accepted as compatibility input.
    #[arg(long = "pending-add-gated")]
    pending_add_gated: Vec<String>,
    /// Mark a pending item `[x]` by hash id (repeatable).
    #[arg(long = "pending-done")]
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
    /// Clear session ID and delete snapshot
    Reset {
        /// Path to the session document
        file: PathBuf,
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
    /// Start Claude in a tmux pane and register the session
    Start {
        /// Path to the session document
        file: PathBuf,
        /// Force binding the session to the current tmux pane, even if a live
        /// owner already exists in another pane
        #[arg(long)]
        force: bool,
    },
    /// Route /agent-doc command to the correct tmux pane
    Route {
        /// Path to the session document
        file: PathBuf,
        /// Resolve the owning pane and send the bare reopen without route-owned
        /// busy-session recovery, startup-miss gating, or cycle-ack waiting.
        #[arg(long)]
        dispatch_only: bool,
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
        #[arg(long, default_value_t = 0)]
        debounce: u64,
    },
    /// Detect permission prompts from a Claude Code session
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
    },
    /// Run all pre-agent steps (repair, commit, claims, diff, document HEAD) and output JSON
    Preflight {
        /// Path to the session document
        file: PathBuf,
    },
    /// Check end-of-cycle write invariant — nonzero exit if the cycle is open or a likely direct response patchback bypassed agent-doc
    SessionCheck {
        /// Path to the session document
        file: PathBuf,
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
    /// Derive a structured post-preflight planning/dispatch record for a document
    Plan {
        /// Path to the session document
        file: PathBuf,
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
    },
    /// Clear the configured tmux session when no file is provided, or clear the bound harness session when FILE is provided
    Clear {
        /// Optional path to the session document
        file: Option<PathBuf>,
    },
    /// Diagnose actor/registry/supervisor drift for a document
    Doctor {
        /// Path to the session document
        file: PathBuf,
        /// Escalate into the explicit repair path before re-checking status
        #[arg(long)]
        repair: bool,
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

    let config = config::load()?;

    match cli.command {
        Commands::Run {
            file,
            branch,
            agent,
            model,
            dry_run,
            no_git,
        } => run::run(
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
                diff::run(&file, wait)
            }
        }
        Commands::Reset { file } => reset::run(&file),
        Commands::Clean { file, archive } => clean::run(&file, archive),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Gc { root, dry_run } => {
            let result = gc::run(root.as_deref(), dry_run)?;
            if dry_run {
                eprintln!(
                    "[gc] Dry run: {} files would be deleted, {} kept",
                    result.deleted, result.skipped
                );
            }
            Ok(())
        }
        Commands::Start { file, force } => start::run(&file, force),
        Commands::Route {
            file,
            dispatch_only,
            pane,
            cols,
            focus: _focus,
            debounce,
        } => {
            // NOTE: sync::run_layout_only was previously called here after route when
            // --col args were provided. Removed because the JB plugin calls `agent-doc sync`
            // separately with the correct --window arg. Running sync from both route AND
            // the plugin created a double-sync glitch (panes bouncing between stash and
            // agent-doc window). The plugin's sync is authoritative for layout.
            let mode = if dispatch_only {
                route::RouteMode::DispatchOnly
            } else {
                route::RouteMode::Managed
            };
            route::run(&file, pane.as_deref(), debounce, &cols, mode)
        }
        Commands::Prompt { file, answer, all } => {
            if all {
                return prompt::run_all();
            }
            let file = file.context("FILE required when not using --all")?;
            match answer {
                Some(option) => prompt::answer(&file, option),
                None => prompt::run(&file),
            }
        }
        Commands::Commit { file } => git::commit(&file).map(|_| ()),
        Commands::Dedupe { file } => dedupe::run(&file),
        Commands::Claim {
            file,
            position,
            pane,
            window,
            force,
            isolate,
        } => claim::run(
            &file,
            position.as_deref(),
            pane.as_deref(),
            window.as_deref(),
            force,
            isolate,
        ),
        Commands::Focus { file, pane } => focus::run(&file, pane.as_deref()),
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
        } => {
            if rename && let Some(ref f) = focus {
                sync::write_rename_debounce(f);
            }
            if no_autostart {
                sync::run_layout_only(&columns, window.as_deref(), focus.as_deref())
            } else {
                sync::run(&columns, window.as_deref(), focus.as_deref())
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
                watch::stop()
            } else if status {
                watch::status()
            } else {
                watch::start(
                    &config,
                    watch::WatchConfig {
                        debounce_ms: debounce,
                        max_cycles,
                    },
                )
            }
        }
        Commands::Outline { file, json } => outline::run(&file, json),
        Commands::Resync { file, fix, session } => {
            if fix {
                resync::run_fix(file.as_deref(), session.as_deref())
            } else {
                resync::run(false, session.as_deref(), file.as_deref())
            }
        }
        Commands::Fix { file, session } => resync::run_fix(file.as_deref(), session.as_deref()),
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
        Commands::Write { args, commit } => write::run_command(
            write::CommandOptions {
                file: args.file,
                baseline_file: args.baseline_file,
                is_template: args.template,
                is_stream: args.stream,
                is_ipc: args.ipc,
                force_disk: args.force_disk,
                origin: args.origin,
                pending_add: args.pending_add,
                pending_add_gated: args.pending_add_gated,
                pending_done: args.pending_done,
                pending_edit: args.pending_edit,
                pending_clear: args.pending_clear,
                pending_reorder: args.pending_reorder,
                pending_gate: args.pending_gate,
                pending_ungate: args.pending_ungate,
                pending_resolve_gate: args.pending_resolve_gate,
                pending_set_gate_type: args.pending_set_gate_type,
                allow_replace_pending: args.allow_replace_pending,
                pending_only: args.pending_only,
                status: args.status,
            },
            if commit {
                write::CommitMode::BestEffort
            } else {
                write::CommitMode::None
            },
        ),
        Commands::Finalize { args } => write::run_command(
            write::CommandOptions {
                file: args.file,
                baseline_file: args.baseline_file,
                is_template: args.template,
                is_stream: args.stream,
                is_ipc: args.ipc,
                force_disk: args.force_disk,
                origin: args.origin,
                pending_add: args.pending_add,
                pending_add_gated: args.pending_add_gated,
                pending_done: args.pending_done,
                pending_edit: args.pending_edit,
                pending_clear: args.pending_clear,
                pending_reorder: args.pending_reorder,
                pending_gate: args.pending_gate,
                pending_ungate: args.pending_ungate,
                pending_resolve_gate: args.pending_resolve_gate,
                pending_set_gate_type: args.pending_set_gate_type,
                allow_replace_pending: args.allow_replace_pending,
                pending_only: args.pending_only,
                status: args.status,
            },
            write::CommitMode::Required,
        ),
        Commands::Stream {
            file,
            interval,
            agent,
            model,
            no_git,
        } => stream::run(
            &file,
            interval,
            agent.as_deref(),
            model.as_deref(),
            no_git,
            &config,
        ),
        Commands::TemplateInfo { file } => {
            let info = template::template_info(&file)?;
            println!("{}", serde_json::to_string_pretty(&info)?);
            Ok(())
        }
        Commands::Repair { file } => {
            let outcome = repair::repair(&file)?;
            if !outcome.repaired() {
                eprintln!("[repair] No pending response found for {}", file.display());
            }
            Ok(())
        }
        Commands::Preflight { file } => preflight::run(&file),
        Commands::Plan { file } => plan::run(&file),
        Commands::SessionCheck { file } => session_check::run(&file),
        Commands::Read { file, component } => read::run(&file, component.as_deref()),
        Commands::ResponseToc {
            file,
            backlog_id,
            query,
            limit,
            json,
        } => response_toc::run_toc(&file, backlog_id.as_deref(), query.as_deref(), limit, json),
        Commands::ResponseFetch {
            file,
            locator,
            before,
            after,
            json,
        } => response_toc::run_fetch(&file, &locator, before, after, json),
        Commands::ArchiveIndex { file, rebuild } => archive_index::run_index(&file, rebuild),
        Commands::ArchiveSearch {
            file,
            query,
            backlog_id,
            session,
            limit,
            json,
            rebuild,
        } => archive_index::run_search(
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
        } => compact::run(
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
            if let Some(root) = snapshot::find_project_root(&cwd) {
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
        Commands::Boundary { file, component } => boundary::run(&file, component.as_deref()),
        Commands::Terminal { file, session } => terminal::run(&file, session.as_deref()),
        Commands::Autoclaim => autoclaim::run(),
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
            Some(SessionAction::Restart { file, fresh }) => session_actor_cmd::restart(
                &file,
                if fresh {
                    session_actor_cmd::RestartMode::Fresh
                } else {
                    session_actor_cmd::RestartMode::Continue
                },
            ),
            Some(SessionAction::Clear { file: Some(file) }) => session_actor_cmd::clear(&file),
            Some(SessionAction::Clear { file: None }) => session_cmd::clear(),
            Some(SessionAction::Doctor { file, repair }) => {
                session_actor_cmd::doctor(&file, repair)
            }
            None => session_cmd::show(),
        },
        Commands::Controller { action } => match action {
            ControllerAction::Status {
                project_root,
                ensure,
            } => project_controller::run_status(project_root.as_deref(), ensure),
            ControllerAction::Serve {
                project_root,
                launch_mode,
                listen_socket,
                controller_generation,
                previous_controller_pid,
                handoff_state,
            } => project_controller::run_serve(
                project_root.as_deref(),
                &launch_mode,
                listen_socket.as_deref(),
                controller_generation,
                previous_controller_pid,
                &handoff_state,
            ),
            ControllerAction::Shutdown { project_root } => {
                project_controller::run_shutdown(project_root.as_deref())
            }
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
                let pending = callback::scan_pending_callbacks(root.as_deref())?;
                let json = serde_json::to_string_pretty(
                    &serde_json::json!({"pending_callbacks": pending}),
                )?;
                println!("{}", json);
                Ok(())
            }
            HookAction::CodexUserPromptSubmit => codex_hook::handle_user_prompt_submit(),
            HookAction::CodexStop => codex_hook::handle_stop(),
        },
        Commands::Cleanup {
            file,
            timeout,
            poll_interval,
            fallback_model,
        } => cleanup_cmd::run(&file, timeout, poll_interval, &fallback_model),
        Commands::Backlog { file, action } => match action {
            PendingAction::Add { item } => pending_cmd::add(&file, &item, false),
            PendingAction::AddGated { item } => pending_cmd::add(&file, &item, true),
            PendingAction::Remove { target, contains } => {
                pending_cmd::remove(&file, &target, contains)
            }
            PendingAction::Prune => pending_cmd::reap(&file),
            PendingAction::Reap => pending_cmd::reap(&file),
            PendingAction::Backfill => pending_cmd::backfill(&file),
            PendingAction::Done { id } => pending_cmd::done(&file, &id),
            PendingAction::Edit { id, text } => pending_cmd::edit(&file, &id, &text),
            PendingAction::Clear => pending_cmd::clear(&file),
            PendingAction::Reorder { ids } => {
                let ids: Vec<String> = ids
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                pending_cmd::reorder(&file, &ids)
            }
            PendingAction::List => pending_cmd::list(&file),
            PendingAction::ResolveGate { gate_type } => {
                pending_cmd::resolve_gate(&file, &gate_type)
            }
            PendingAction::SetGateType { id, gate_type } => {
                pending_cmd::set_gate_type(&file, &id, &gate_type)
            }
        },
        Commands::ResolveGateCmd { gate_type, scope } => {
            // Determine scan root: explicit --scope, or cwd, or project root
            let scan_root = if let Some(s) = scope {
                s
            } else {
                let cwd = std::env::current_dir()?;
                snapshot::find_project_root(&cwd).unwrap_or(cwd)
            };
            let total = pending_cmd::resolve_gate_scan(&gate_type, &scan_root)?;
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
                let request = callback::create_request(&file, &ops, context.as_deref(), ttl)?;
                println!("{}", serde_json::to_string_pretty(&request)?);
                Ok(())
            }
            CallbackAction::Read { file } => {
                match callback::read_request(&file)? {
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
                callback::write_response(&file, &request_id, &status, &summary, None)?;
                eprintln!("[callback] response written for request {}", request_id);
                Ok(())
            }
            CallbackAction::Gc { root } => {
                let cwd = std::env::current_dir()?;
                let root_path = root
                    .map(PathBuf::from)
                    .or_else(|| snapshot::find_project_root(&cwd))
                    .context("could not find project root")?;
                callback::cleanup_expired(&root_path, 300)
            }
        },
    }
}
