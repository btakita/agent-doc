//! # Module: focus
//!
//! Focus the tmux pane associated with a session document.
//!
//! Usage: `agent-doc focus <file.md>`
//!
//! ## Spec
//! - `run(file, pane)`: entry point; delegates to `run_with_tmux` using the default
//!   tmux server.
//! - `run_with_tmux(file, pane_override, tmux)`: if `pane_override` is `Some`, skips
//!   frontmatter lookup and calls `tmux select-pane` on the supplied pane directly;
//!   errors if the override pane is not alive.
//! - When `pane_override` is `None`, reads the file from disk, parses YAML frontmatter,
//!   and extracts the `agent_doc_session` UUID; errors if the field is absent.
//! - Looks up the UUID in `sessions.json` via `sessions::lookup`; errors if no entry
//!   is found or if the registered pane is dead.
//! - On success, calls `tmux select-pane` and logs the focused pane + file path to stderr.
//!
//! ## Agentic Contracts
//! - `run_with_tmux` never modifies `sessions.json` or the document on disk.
//! - A file without `agent_doc_session` in its frontmatter always returns an error with
//!   a message directing the caller to run `claim` first.
//! - A registered pane that is no longer alive returns an error; the caller is responsible
//!   for pruning or re-claiming.
//! - `pane_override` is an escape hatch for callers that already know the pane ID (e.g.
//!   `layout.rs` focusing a resolved pane); it bypasses all registry and frontmatter I/O.
//!
//! ## Evals
//! - `focus_live_pane` (aspirational): file has a valid session UUID and a live pane →
//!   `select-pane` is called and `Ok(())` is returned.
//! - `focus_dead_pane` (aspirational): session UUID exists in registry but pane is dead →
//!   error containing "pane … is dead" is returned.
//! - `focus_no_session` (aspirational): file frontmatter has no `agent_doc_session` →
//!   error directing caller to run `claim` is returned.
//! - `focus_file_not_found` (aspirational): file path does not exist on disk →
//!   error containing "file not found" is returned.
//! - `focus_pane_override_live` (aspirational): `pane_override` supplied and pane is live →
//!   registry is never read and `select-pane` is called on the override pane.
//! - `focus_pane_override_dead` (aspirational): `pane_override` supplied but pane is dead →
//!   error containing "pane … is dead" is returned without reading frontmatter.

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

pub fn run(file: &Path, pane: Option<&str>) -> Result<()> {
    run_with_tmux(file, pane, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, pane_override: Option<&str>, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // If an explicit pane was provided, use it directly
    if let Some(p) = pane_override {
        if tmux.pane_alive(p) {
            tmux.select_pane(p)?;
            eprintln!("Focused pane {} ({})", p, file.display());
            return Ok(());
        } else {
            anyhow::bail!("pane {} is dead for {}", p, file.display());
        }
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;
    let session_id = match fm.session {
        Some(id) => id,
        None => anyhow::bail!(
            "no session UUID in {} (use Claim to register)",
            file.display()
        ),
    };

    let pane = sessions::lookup(&session_id)?;
    match pane {
        Some(pane_id) if tmux.pane_alive(&pane_id) => {
            tmux.select_pane(&pane_id)?;
            eprintln!("Focused pane {} ({})", pane_id, file.display());
            Ok(())
        }
        Some(pane_id) => {
            anyhow::bail!("pane {} is dead for {}", pane_id, file.display());
        }
        None => {
            anyhow::bail!(
                "no pane registered for {} (session {})",
                file.display(),
                &session_id[..std::cmp::min(8, session_id.len())]
            );
        }
    }
}
