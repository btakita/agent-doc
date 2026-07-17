//! Project-local controller filesystem paths.

use std::path::{Path, PathBuf};

pub const SOCKET_FILE: &str = "controller.sock";
pub const LOCK_FILE: &str = "controller-launch.lock";

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

pub fn launch_lock_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc/locks").join(LOCK_FILE)
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
        assert_eq!(
            launch_lock_path(root),
            Path::new("/tmp/project/.agent-doc/locks/controller-launch.lock")
        );
    }
}
