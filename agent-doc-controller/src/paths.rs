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

/// File-name prefix of the canonical generation-scoped handoff socket,
/// `controller-handoff-<pid>-<generation>.sock`.
pub const HANDOFF_SOCKET_PREFIX: &str = "controller-handoff-";

/// File-name prefix of the compact handoff socket,
/// `h<pid as 5 base-36 digits><generation in base 36>.sock`
/// (`#handoffsockcompact`). Used only when the canonical name would overflow
/// `sun_path` under a root whose public `controller.sock` still fits.
pub const COMPACT_HANDOFF_SOCKET_PREFIX: &str = "h";

/// Fixed base-36 width of the pid field in a compact handoff socket name.
/// 36^5 = 60,466,176 exceeds Linux `PID_MAX_LIMIT` (4,194,304).
const COMPACT_PID_WIDTH: usize = 5;

fn to_base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base36 digits are ASCII")
}

/// The private socket a handoff replacement binds before promotion renames it
/// onto [`socket_path`].
///
/// `#handoffsockcompact`: the canonical name is 18+ bytes longer than
/// `controller.sock`, so a root whose public socket fits `sun_path` could still
/// have a handoff socket that never binds. That is a PERMANENT failure, and the
/// serve loop used to retry it every debounce forever (observed 2026-10-10:
/// 104,411 failed handoffs over six days on one root, plus one spawned and
/// immediately-dead replacement process per attempt on another). When the
/// canonical name overflows, use a compact name in the SAME directory (promotion
/// is a `rename`, so it must stay on the public socket's filesystem). The
/// compact name for any Linux pid and any generation below 36^4 is no longer
/// than `controller.sock`, so it fits whenever the public socket fits.
pub fn handoff_socket_path(project_root: &Path, pid: u32, generation: u64) -> PathBuf {
    let dir = project_root.join(".agent-doc");
    let canonical = dir.join(format!("{HANDOFF_SOCKET_PREFIX}{pid}-{generation}.sock"));
    if canonical.as_os_str().len() <= SUN_PATH_MAX {
        return canonical;
    }
    dir.join(format!(
        "{COMPACT_HANDOFF_SOCKET_PREFIX}{:0>width$}{}.sock",
        to_base36(u64::from(pid)),
        to_base36(generation),
        width = COMPACT_PID_WIDTH
    ))
}

/// Parse `(pid, generation)` from a handoff socket file name in either the
/// canonical or the compact form. `None` for any other file (including the
/// public `controller.sock`).
pub fn parse_handoff_socket_file_name(file_name: &str) -> Option<(u32, u64)> {
    if let Some(rest) = file_name
        .strip_prefix(HANDOFF_SOCKET_PREFIX)
        .and_then(|rest| rest.strip_suffix(".sock"))
    {
        let mut fields = rest.split('-');
        let pid = fields.next()?.parse::<u32>().ok()?;
        let generation = fields.next()?.parse::<u64>().ok()?;
        return fields.next().is_none().then_some((pid, generation));
    }
    let rest = file_name
        .strip_prefix(COMPACT_HANDOFF_SOCKET_PREFIX)?
        .strip_suffix(".sock")?;
    let is_base36 = |field: &str| {
        !field.is_empty()
            && field
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
    };
    if rest.len() <= COMPACT_PID_WIDTH || !rest.is_ascii() {
        return None;
    }
    let (pid, generation) = rest.split_at(COMPACT_PID_WIDTH);
    if !is_base36(pid) || !is_base36(generation) {
        return None;
    }
    let pid = u32::try_from(u64::from_str_radix(pid, 36).ok()?).ok()?;
    let generation = u64::from_str_radix(generation, 36).ok()?;
    Some((pid, generation))
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
        assert!(
            socket_path_rejection(&root).is_some(),
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

    /// `#handoffsockcompact`: an ordinary root keeps the canonical name.
    #[test]
    fn short_roots_keep_the_canonical_handoff_socket_name() {
        let path = handoff_socket_path(Path::new("/tmp/project"), 1704535, 2);
        assert_eq!(
            path,
            Path::new("/tmp/project/.agent-doc/controller-handoff-1704535-2.sock")
        );
        assert_eq!(
            parse_handoff_socket_file_name("controller-handoff-1704535-2.sock"),
            Some((1704535, 2))
        );
    }

    /// `#handoffsockcompact`: whenever the PUBLIC socket fits `sun_path`, the
    /// handoff socket fits too — for every root length up to the limit, every
    /// Linux pid, and any generation below 36^4. This is the property
    /// whose violation produced a permanent handoff-failure loop.
    #[test]
    fn handoff_socket_fits_whenever_the_public_socket_fits() {
        let mut boundary_checked = false;
        for root_len in 1..=SUN_PATH_MAX {
            let root = PathBuf::from(format!("/{}", "r".repeat(root_len - 1)));
            if socket_path_rejection(&root).is_some() {
                continue;
            }
            boundary_checked |= socket_path(&root).as_os_str().len() == SUN_PATH_MAX;
            for pid in [1u32, 2038791, 4_194_304] {
                for generation in [0u64, 1, 2, 2233, 36 * 36 * 36 * 36 - 1] {
                    let path = handoff_socket_path(&root, pid, generation);
                    assert_eq!(
                        resolved_socket_path_rejection(&path),
                        None,
                        "root_len={root_len} pid={pid} generation={generation}: {}",
                        path.display()
                    );
                    assert_eq!(path.parent(), socket_path(&root).parent());
                    let name = path.file_name().unwrap().to_str().unwrap();
                    assert_eq!(
                        parse_handoff_socket_file_name(name),
                        Some((pid, generation))
                    );
                }
            }
        }
        assert!(
            boundary_checked,
            "the exact sun_path boundary must be exercised"
        );
    }

    /// The two live roots from the 2026-10-10 incident.
    #[test]
    fn incident_roots_get_bindable_handoff_sockets() {
        for (root, pid) in [
            (
                "/home/brian/work/btakita/agent-loop/src/agent-doc/editors/jetbrains",
                1704535u32,
            ),
            (
                "/home/brian/work/btakita/agent-loop/src/boost-client/src/monsterrodholders-dev",
                2038791,
            ),
        ] {
            let root = Path::new(root);
            assert_eq!(socket_path_rejection(root), None);
            let path = handoff_socket_path(root, pid, 2);
            assert_eq!(
                resolved_socket_path_rejection(&path),
                None,
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn non_handoff_socket_names_do_not_parse() {
        for name in [
            "controller.sock",
            "ipc.sock",
            "ipc-2041918.sock",
            "controller-handoff.sock",
            "controller-handoff-test.sock",
            "h.sock",
            "h00001.sock",
            "hooks",
            "h0000-1.sock",
            "hABCDE1.sock",
        ] {
            assert_eq!(parse_handoff_socket_file_name(name), None, "{name}");
        }
    }
}
