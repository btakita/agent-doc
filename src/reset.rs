use anyhow::Result;
use std::path::Path;

use crate::{frontmatter, snapshot};

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Clear session ID in frontmatter
    let content = std::fs::read_to_string(file)?;
    let (mut fm, body) = frontmatter::parse(&content)?;
    fm.session = None;
    let updated = frontmatter::write(&fm, body)?;
    std::fs::write(file, updated)?;

    // Delete snapshot
    snapshot::delete(file)?;

    eprintln!("Reset session for {}", file.display());
    Ok(())
}
