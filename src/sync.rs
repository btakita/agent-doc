//! `agent-doc sync` — 2D layout sync: mirror a columnar editor layout in tmux.
//!
//! Usage: agent-doc sync --col plan.md,corky.md --col agent-doc.md [--window @1] [--focus plan.md]
//!
//! Each `--col` is a comma-separated list of files. Columns arrange left-to-right.
//! Within each column, files stack top-to-bottom.
//!
//! Delegates the actual layout algorithm to the `tmux-router` crate.
//! This module provides the agent-doc-specific frontmatter resolution layer.

use anyhow::Result;
use std::cell::RefCell;
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

use tmux_router::FileResolution;

pub fn run(col_args: &[String], window: Option<&str>, focus: Option<&str>) -> Result<()> {
    run_with_tmux(col_args, window, focus, &Tmux::default_server())
}

pub fn run_with_tmux(
    col_args: &[String],
    window: Option<&str>,
    focus: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    let registry_path = sessions::registry_path();
    let files_needing_session = RefCell::new(Vec::new());

    let resolve_file = |path: &Path| -> Option<FileResolution> {
        let content = std::fs::read_to_string(path).ok()?;
        let (fm, _) = frontmatter::parse(&content).ok()?;
        match fm.session {
            Some(key) => {
                if fm.tmux_session.is_none() {
                    files_needing_session.borrow_mut().push(path.to_path_buf());
                }
                Some(FileResolution::Registered {
                    key,
                    tmux_session: fm.tmux_session,
                })
            }
            None => Some(FileResolution::Unmanaged),
        }
    };

    let result =
        tmux_router::sync(col_args, window, focus, tmux, &registry_path, &resolve_file)?;

    // Write tmux_session back to files that need it
    if let Some(ref session_name) = result.target_session {
        for file in files_needing_session.borrow().iter() {
            if let Ok(content) = std::fs::read_to_string(file)
                && let Ok(updated) = frontmatter::set_tmux_session(&content, session_name)
                && updated != content
            {
                let _ = std::fs::write(file, &updated);
            }
        }
    }
    Ok(())
}
