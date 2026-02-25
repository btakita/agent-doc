mod agent;
mod audit_docs;
mod claim;
mod clean;
mod config;
mod diff;
mod frontmatter;
mod git;
mod init;
mod prompt;
mod reset;
mod route;
mod sessions;
mod skill;
mod snapshot;
mod start;
mod submit;
mod upgrade;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    },
    /// Manage the Claude Code skill definition
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Check for updates and upgrade to the latest version.
    Upgrade,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Install the skill definition to .claude/skills/agent-doc/SKILL.md
    Install,
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
        Commands::Init { file, title, agent } => {
            init::run(&file, title.as_deref(), agent.as_deref(), &config)
        }
        Commands::Diff { file } => diff::run(&file),
        Commands::Reset { file } => reset::run(&file),
        Commands::Clean { file } => clean::run(&file),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Start { file } => start::run(&file),
        Commands::Route { file } => route::run(&file),
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
        Commands::Claim { file } => claim::run(&file),
        Commands::Skill { command } => match command {
            SkillCommands::Install => skill::install(),
            SkillCommands::Check => skill::check(),
        },
        Commands::Upgrade => upgrade::run(),
    }
}
