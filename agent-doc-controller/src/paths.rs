//! Project-local controller filesystem paths.

use std::path::{Path, PathBuf};

pub const SOCKET_FILE: &str = "controller.sock";

/// Maximum byte length for a Unix domain socket path (`sun_path` is 108 bytes
/// including the NUL terminator on Linux).
///
/// Mirrors `agent_doc_supervisor_io::ipc::SUN_PATH_MAX`. The supervisor socket
/// answers an overflow by relocating to a short runtime path; the controller
/// socket cannot — see [`socket_path_rejection`].
pub const SUN_PATH_MAX: usize = 107;

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

/// Why the controller socket for `project_root` can never be bound or connected,
/// if it cannot.
///
/// The controller socket path is fixed at `<root>/.agent-doc/controller.sock`
/// because the JetBrains and VS Code plugins resolve that exact path themselves.
/// The supervisor socket relocates to a short runtime path when it overflows
/// `sun_path`, but doing that here would bind the controller somewhere no editor
/// ever looks — trading a loud failure for a permanently missing editor
/// authority. So an over-long project root is a **permanent** condition, and the
/// only useful response is to name it immediately (`#ctrlsockpathtoolong`).
///
/// Reported 2026-08-09: under a 108-byte project root, every client instead
/// retried a bind that could never succeed until its budget ran out — a fresh
/// EMPTY document burned a full 90s preflight admission budget, which reads as
/// "agent-doc is slow" rather than "this path is too long".
pub fn socket_path_rejection(project_root: &Path) -> Option<String> {
    resolved_socket_path_rejection(&socket_path(project_root))
}

/// [`socket_path_rejection`] for an already-resolved socket path.
///
/// Connect and wait sites are handed the socket path itself — including
/// generation-scoped handoff sockets whose file name is not [`SOCKET_FILE`] — so
/// they must measure the path they will actually bind rather than re-deriving a
/// canonical one that may be shorter.
pub fn resolved_socket_path_rejection(path: &Path) -> Option<String> {
    let len = path.as_os_str().len();
    if len <= SUN_PATH_MAX {
        return None;
    }
    Some(format!(
        "project controller socket path is {len} bytes, over the {SUN_PATH_MAX}-byte \
         AF_UNIX sun_path limit: {}. No controller can bind or be reached here, so \
         retrying cannot help. Move the project under a shorter root, or symlink it \
         to one and open the document through the shorter path.",
        path.display()
    ))
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

    #[test]
    fn ordinary_project_roots_are_not_rejected() {
        assert_eq!(socket_path_rejection(Path::new("/tmp/project")), None);
    }

    /// `#ctrlsockpathtoolong`: the length that matters is the resolved socket
    /// path, not the root, so the check must account for the fixed
    /// `/.agent-doc/controller.sock` suffix.
    #[test]
    fn a_root_whose_socket_path_overflows_sun_path_is_rejected() {
        let root = PathBuf::from(format!("/{}", "r".repeat(SUN_PATH_MAX)));
        assert_eq!(
            socket_path_rejection(&root).is_none(),
            false,
            "a root at the limit still overflows once the socket suffix is appended"
        );

        let reason = socket_path_rejection(&root).expect("rejected");
        assert!(reason.contains("sun_path"), "{reason}");
        assert!(reason.contains("controller.sock"), "{reason}");
        assert!(
            reason.contains("retrying cannot help"),
            "the reason must say the condition is permanent: {reason}"
        );
    }

    /// The boundary is exact on both sides: one byte under passes, one byte
    /// over is rejected.
    #[test]
    fn the_sun_path_boundary_is_exact() {
        let suffix_len = socket_path(Path::new("")).as_os_str().len();
        let longest_ok = PathBuf::from("/".repeat(SUN_PATH_MAX - suffix_len));
        assert_eq!(socket_path(&longest_ok).as_os_str().len(), SUN_PATH_MAX);
        assert_eq!(socket_path_rejection(&longest_ok), None);

        let one_over = PathBuf::from("/".repeat(SUN_PATH_MAX - suffix_len + 1));
        assert_eq!(socket_path(&one_over).as_os_str().len(), SUN_PATH_MAX + 1);
        assert!(socket_path_rejection(&one_over).is_some());
    }
}
