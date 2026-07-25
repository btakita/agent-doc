//! Project-local controller filesystem paths.

use std::path::{Path, PathBuf};

pub const SOCKET_FILE: &str = "controller.sock";

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn controller_paths_are_project_local() {
        let root = Path::new("/tmp/project");

        assert_eq!(
            socket_path(root),
            Path::new("/tmp/project/.agent-doc/controller.sock")
        );
    }
}
