use anyhow::Result;
use std::path::Path;

use crate::git;

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    git::squash_session(file)
}
