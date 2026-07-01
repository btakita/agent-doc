//! Project-local controller filesystem paths.

use std::path::{Path, PathBuf};

pub const SOCKET_FILE: &str = "controller.sock";
pub const STATE_FILE: &str = "controller-state.json";
pub const ACTOR_PROJECTION_FILE: &str = "session-actors.json";
pub const LAYOUT_PROJECTION_FILE: &str = "last_layout.json";
pub const LOCK_FILE: &str = "controller-launch.lock";

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

pub fn state_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(STATE_FILE)
}

pub fn actor_projection_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(ACTOR_PROJECTION_FILE)
}

pub fn layout_projection_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(LAYOUT_PROJECTION_FILE)
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
            state_path(root),
            Path::new("/tmp/project/.agent-doc/controller-state.json")
        );
        assert_eq!(
            actor_projection_path(root),
            Path::new("/tmp/project/.agent-doc/session-actors.json")
        );
        assert_eq!(
            layout_projection_path(root),
            Path::new("/tmp/project/.agent-doc/last_layout.json")
        );
        assert_eq!(
            launch_lock_path(root),
            Path::new("/tmp/project/.agent-doc/locks/controller-launch.lock")
        );
    }
}
