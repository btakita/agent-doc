mod agent;
mod clean;
mod config;
mod diff;
mod frontmatter;
mod git;
mod init;
mod reset;
mod snapshot;
mod submit;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agent-doc", about = "Interactive document sessions with AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diff, send to agent, append response
    Submit {
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Submit {
            file,
            branch,
            agent,
            model,
            dry_run,
            no_git,
        } => submit::run(&file, branch, agent.as_deref(), model.as_deref(), dry_run, no_git),
        Commands::Init { file, title, agent } => {
            init::run(&file, title.as_deref(), agent.as_deref())
        }
        Commands::Diff { file } => diff::run(&file),
        Commands::Reset { file } => reset::run(&file),
        Commands::Clean { file } => clean::run(&file),
    }
}
