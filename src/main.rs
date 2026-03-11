mod agent;
mod audit_docs;
mod autoclaim;
mod claim;
mod clean;
mod commands;
mod compact;
mod config;
mod crdt;
mod convert;
mod diff;
mod focus;
mod frontmatter;
mod git;
mod init;
mod layout;
mod merge;
mod mode;
mod outline;
mod patch;
mod plugin;
mod prompt;
mod component;
mod recover;
mod reset;
mod resync;
mod route;
mod sessions;
mod skill;
mod snapshot;
mod start;
mod stream;
mod submit;
mod sync;
mod template;
mod upgrade;
mod watch;
mod write;

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
    /// Scaffold a new session document
    Init {
        /// Path for the new session document
        file: PathBuf,
        /// Session title
        title: Option<String>,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Document mode: append (default) or template
        #[arg(long)]
        mode: Option<String>,
    },
    /// Preview the diff that would be sent
    Diff {
        /// Path to the session document
        file: PathBuf,
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
    Resync,
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
    /// Archive old exchanges to reduce document size (append-mode only)
    Compact {
        /// Path to the session document
        file: PathBuf,
        /// Number of recent exchanges to keep (default: 2)
        #[arg(long, default_value = "2")]
        keep: usize,
    },
    /// Convert a document between append and template modes
    Convert {
        /// Path to the session document
        file: PathBuf,
        /// Target mode (default: template)
        #[arg(value_enum, default_value = "template")]
        mode: AgentDocMode,
    },
    /// Get or set the document mode (append/template)
    Mode {
        /// Path to the session document
        file: PathBuf,
        /// Set mode: append or template
        #[arg(long)]
        set: Option<String>,
    },
    /// Re-establish claims after context compaction (SessionStart hook)
    Autoclaim,
    /// Check for updates and upgrade to the latest version.
    Upgrade,
    /// List all available commands as JSON (for editor plugin autocomplete)
    #[command(name = "commands")]
    #[allow(clippy::enum_variant_names)]
    ListCommands,
}

#[derive(Subcommand)]
enum PluginAction {
    /// Download and install an editor plugin
    Install {
        /// Editor: jetbrains, vscode
        editor: String,
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
    /// Install the skill definition to .claude/skills/agent-doc/SKILL.md
    Install {
        /// After install, output reload instructions: compact (default) or restart
        #[arg(long)]
        reload: Option<String>,
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
        } => submit::run(&file, branch, agent.as_deref(), model.as_deref(), dry_run, no_git, &config),
        Commands::Init { file, title, agent, mode } => {
            init::run(&file, title.as_deref(), agent.as_deref(), mode.as_deref(), &config)
        }
        Commands::Diff { file } => diff::run(&file),
        Commands::Reset { file } => reset::run(&file),
        Commands::Clean { file } => clean::run(&file),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Start { file } => start::run(&file),
        Commands::Route { file, pane } => route::run(&file, pane.as_deref()),
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
        Commands::Claim { file, position, pane, window } => claim::run(&file, position.as_deref(), pane.as_deref(), window.as_deref()),
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
        Commands::Resync => resync::run(),
        Commands::Skill { command } => match command {
            SkillCommands::Install { reload } => {
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
                Ok(())
            }
            SkillCommands::Check => skill::check(),
        },
        Commands::Plugin { action } => match action {
            PluginAction::Install { editor } => plugin::install(&editor),
            PluginAction::Update { editor } => plugin::update(&editor),
            PluginAction::List => plugin::list(),
        },
        Commands::Write { file, baseline_file, template: is_template, stream: is_stream } => {
            let baseline = baseline_file
                .as_ref()
                .map(std::fs::read_to_string)
                .transpose()
                .context("failed to read baseline file")?;
            if is_stream {
                write::run_stream(&file, baseline.as_deref())
            } else if is_template {
                write::run_template(&file, baseline.as_deref())
            } else {
                // Auto-detect stream mode from frontmatter
                let content = std::fs::read_to_string(&file)
                    .context("failed to read document for mode detection")?;
                let (fm, _) = frontmatter::parse(&content)?;
                if fm.mode.as_deref() == Some("stream") {
                    write::run_stream(&file, baseline.as_deref())
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
        Commands::Compact { file, keep } => compact::run(&file, keep),
        Commands::Convert { file, mode } => convert::run(&file, &mode),
        Commands::Mode { file, set } => mode::run(&file, set.as_deref()),
        Commands::Autoclaim => autoclaim::run(),
        Commands::Upgrade => upgrade::run(),
        Commands::ListCommands => commands::run(),
    }
}
