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
//! - `Route` additionally calls `sync::run_layout_only` when `--col` args are present,
//!   logging layout-sync failures to stderr without propagating the error.
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
//! - dispatch_route_with_cols: `--col` args present → `sync::run_layout_only` called after route
//! - dispatch_prompt_all: `--all` → `prompt::run_all`, no FILE required
//! - dispatch_history_restore: `--restore <sha>` → `history::restore` called
//! - dispatch_watch_stop: `--stop` flag → `watch::stop` called
//! - dispatch_skill_install_reload: skill updated + `--reload compact` → prints `SKILL_RELOAD=compact`
//! - dispatch_lib_path_missing: library absent → exits with code 1

mod agent;
mod audit_docs;
mod autoclaim;
mod boundary;
mod claim;
mod clean;
mod commands;
mod compact;
mod config;
mod convert;
mod parallel;
mod preflight;
mod diff;
mod extract;
mod focus;
mod git;
mod history;
mod hook_cmd;
mod hooks;
mod init;
mod install;
mod layout;
mod mode;
mod outline;
mod patch;
mod plugin;
mod prompt;
mod recover;
mod rename;
mod reset;
mod resync;
mod route;
mod sessions;
mod skill;
mod snapshot;
mod start;
mod stream;
mod run;
mod sync;
mod terminal;
pub(crate) use agent_doc::ipc_socket;
mod undo;
mod upgrade;
mod watch;
mod worktree;
mod ops_log;
mod write;

// Re-export library modules so binary-internal modules can use `crate::` paths
pub(crate) use agent_doc::component;
pub(crate) use agent_doc::crdt;
pub(crate) use agent_doc::frontmatter;
pub(crate) use agent_doc::merge;
pub(crate) use agent_doc::template;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
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

#[derive(Parser)]
#[command(name = "agent-doc", version, about = "Interactive document sessions with AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a session: diff, send to agent, append response
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
    /// Preview the diff that would be sent
    Diff {
        /// Path to the session document
        file: PathBuf,
        /// Wait for stable content (truncation detection) before computing diff
        #[arg(long)]
        wait: bool,
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
    },
    /// Audit instruction files against the codebase
    AuditDocs {
        /// Project root directory (auto-detected if omitted)
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Start Claude in a tmux pane and register the session
    Start {
        /// Path to the session document
        file: PathBuf,
    },
    /// Route /agent-doc command to the correct tmux pane
    Route {
        /// Path to the session document
        file: PathBuf,
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
        #[arg(long = "col", required = true)]
        columns: Vec<String>,
        /// Only operate on panes within this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
        /// Focus this file's pane after arranging (defaults to first file)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Replace content in a named component
    Patch {
        /// Path to the document
        file: PathBuf,
        /// Component name (e.g. "status", "log")
        component: String,
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
        /// Actually kill wrong-session panes and deregister stale entries (without this flag, dry-run only)
        #[arg(long)]
        fix: bool,
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
    /// Recover an orphaned response (from interrupted write-back after compaction)
    Recover {
        /// Path to the session document
        file: PathBuf,
    },
    /// Run all pre-agent steps (recover, commit, claims, diff, document HEAD) and output JSON
    Preflight {
        /// Path to the session document
        file: PathBuf,
    },
    /// Archive old exchanges / compact component content
    Compact {
        /// Path to the session document
        file: PathBuf,
        /// Number of recent exchanges to keep (default: 2, append-mode only)
        #[arg(long, default_value = "2")]
        keep: usize,
        /// Component to compact (template/stream mode, default: exchange)
        #[arg(long)]
        component: Option<String>,
        /// Summary message to replace content with
        #[arg(long)]
        message: Option<String>,
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
    /// Re-establish claims after context compaction (SessionStart hook)
    Autoclaim,
    /// Check for updates and upgrade to the latest version.
    Upgrade,
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
    },
    /// Migrate session state after a document file rename/move
    Rename {
        /// Original document path (may no longer exist on disk)
        old_path: PathBuf,
        /// New document path (must exist)
        new_path: PathBuf,
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
    /// Print the path to the shared library (libagent_doc.so/dylib/dll)
    LibPath,
    /// List all available commands as JSON (for editor plugin autocomplete)
    #[command(name = "commands")]
    #[allow(clippy::enum_variant_names)]
    ListCommands,
    /// Hook system for cross-session coordination
    Hook {
        #[command(subcommand)]
        action: HookAction,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Warn about newer versions on startup, but skip if running the upgrade command itself.
    if !matches!(cli.command, Commands::Upgrade) {
        upgrade::warn_if_outdated();
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
        } => run::run(&file, branch, agent.as_deref(), model.as_deref(), dry_run, no_git, &config),
        Commands::History { file, restore } => match restore {
            Some(commit) => history::restore(&file, &commit),
            None => history::list(&file),
        },
        Commands::Init { file, title, agent, mode } => {
            init::run(file.as_deref(), title.as_deref(), agent.as_deref(), mode.as_deref(), &config)
        }
        Commands::Install { editor, skip_prereqs, skip_plugins } => {
            install::run(editor.as_deref(), skip_prereqs, skip_plugins)
        }
        Commands::Diff { file, wait } => diff::run(&file, wait),
        Commands::Reset { file } => reset::run(&file),
        Commands::Clean { file } => clean::run(&file),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Start { file } => start::run(&file),
        Commands::Route { file, pane, cols, focus, debounce } => {
            let result = route::run(&file, pane.as_deref(), debounce, &cols);
            // If layout columns provided, sync tmux layout after routing (no auto-start —
            // route already handled the target file, auto-start would create duplicates)
            if !cols.is_empty()
                && let Err(e) = sync::run_layout_only(&cols, None, focus.as_deref())
            {
                eprintln!("[route] layout sync failed: {}", e);
            }
            result
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
        Commands::Commit { file } => git::commit(&file),
        Commands::Claim { file, position, pane, window, force } => claim::run(&file, position.as_deref(), pane.as_deref(), window.as_deref(), force),
        Commands::Focus { file, pane } => focus::run(&file, pane.as_deref()),
        Commands::Layout { files, split, pane, window } => {
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
        } => sync::run(&columns, window.as_deref(), focus.as_deref()),
        Commands::Patch {
            file,
            component,
            content,
        } => patch::run(&file, &component, content.as_deref()),
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
        Commands::Resync { fix } => resync::run(fix),
        Commands::Skill { command } => match command {
            SkillCommands::Install { reload, harness, all } => {
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
                    if updated
                        && let Some(ref mode) = reload
                    {
                        match mode.as_str() {
                            "restart" => {
                                println!("SKILL_RELOAD=restart");
                                println!("Skill updated. Please restart this session with --resume to reload the skill.");
                            }
                            _ => {
                                println!("SKILL_RELOAD=compact");
                                println!("Skill updated. Please run /compact to reload the updated skill instructions.");
                            }
                        }
                    }
                }
                Ok(())
            }
            SkillCommands::Check => skill::check(),
        },
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
        Commands::Write { file, baseline_file, template: is_template, stream: is_stream, ipc: is_ipc, force_disk } => {
            let baseline = baseline_file
                .as_ref()
                .map(std::fs::read_to_string)
                .transpose()
                .context("failed to read baseline file")?;
            if is_ipc {
                write::run_ipc(&file, baseline.as_deref())
            } else if is_stream {
                write::run_stream(&file, baseline.as_deref(), force_disk)
            } else if is_template {
                write::run_template(&file, baseline.as_deref())
            } else {
                // Auto-detect write strategy from frontmatter
                let content = std::fs::read_to_string(&file)
                    .context("failed to read document for mode detection")?;
                let (fm, _) = frontmatter::parse(&content)?;
                if fm.resolve_mode().is_crdt() {
                    write::run_stream(&file, baseline.as_deref(), force_disk)
                } else {
                    write::run(&file, baseline.as_deref())
                }
            }
        }
        Commands::Stream { file, interval, agent, model, no_git } => {
            stream::run(&file, interval, agent.as_deref(), model.as_deref(), no_git, &config)
        }
        Commands::TemplateInfo { file } => {
            let info = template::template_info(&file)?;
            println!("{}", serde_json::to_string_pretty(&info)?);
            Ok(())
        }
        Commands::Recover { file } => {
            let recovered = recover::run(&file)?;
            if !recovered {
                eprintln!("[recover] No pending response found for {}", file.display());
            }
            Ok(())
        }
        Commands::Preflight { file } => preflight::run(&file),
        Commands::Compact {
            file,
            keep,
            component,
            message,
        } => compact::run(&file, keep, component.as_deref(), message.as_deref()),
        Commands::Convert { file, mode, agent_doc_format, agent_doc_write } => {
            convert::run(&file, mode.as_ref(), agent_doc_format, agent_doc_write)
        }
        Commands::Mode { file, set } => mode::run(&file, set.as_deref()),
        Commands::Undo { file } => undo::run(&file),
        Commands::Extract { source, target, component } => extract::run(&source, &target, component.as_deref()),
        Commands::Transfer { source, target, component } => extract::transfer(&source, &target, &component),
        Commands::Rename { old_path, new_path } => rename::run(&old_path, &new_path),
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
        Commands::Parallel { file, tasks_explicit, model, no_git, no_worktree, timeout, dry_run } => {
            parallel::run(&file, parallel::ParallelConfig {
                tasks: tasks_explicit,
                model,
                no_git,
                no_worktree,
                timeout_secs: timeout,
                dry_run,
            })
        }
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
        Commands::ListCommands => commands::run(),
        Commands::Hook { action } => match action {
            HookAction::Fire { event, file, session_id, data } => {
                hook_cmd::fire(&event, &file, session_id.as_deref(), data.as_deref())
            }
            HookAction::Poll { event, since, root } => {
                hook_cmd::poll(&event, since, root.as_deref())
            }
            HookAction::Listen { root } => {
                hook_cmd::listen(root.as_deref())
            }
            HookAction::Gc { root } => {
                hook_cmd::gc(root.as_deref())
            }
        },
    }
}
