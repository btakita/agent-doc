//! Pure controller command-line recognition.

use std::path::{Path, PathBuf};

fn arg_file_name_is(arg: &str, expected: &str) -> bool {
    Path::new(arg)
        .file_name()
        .is_some_and(|name| name == expected)
}

fn token_basename(token: &str) -> &str {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token)
}

fn token_is_agent_doc_binary(token: &str) -> bool {
    token_basename(token).starts_with("agent-doc")
}

fn token_is_harness_binary(token: &str) -> bool {
    matches!(
        token_basename(token),
        "claude" | "codex" | "opencode" | "bun" | "node"
    )
}

fn token_is_non_owner_agent_doc_subcommand(token: &str) -> bool {
    matches!(token, "route" | "claim")
}

fn is_shell_c_controller_sentinel(args: &[String], agent_doc_idx: usize) -> bool {
    agent_doc_idx >= 3
        && args.get(agent_doc_idx - 2).is_some_and(|arg| arg == "-c")
        && args.first().is_some_and(|arg| {
            ["sh", "bash", "dash", "zsh"]
                .iter()
                .any(|shell| arg_file_name_is(arg, shell))
        })
}

pub fn agent_doc_controller_serve_arg_index(args: &[String]) -> Option<usize> {
    args.windows(3).enumerate().find_map(|(idx, window)| {
        (arg_file_name_is(&window[0], "agent-doc")
            && window[1] == "controller"
            && window[2] == "serve"
            && (idx == 0 || is_shell_c_controller_sentinel(args, idx)))
        .then_some(idx)
    })
}

pub fn controller_serve_project_root_from_args(args: &[String]) -> Option<PathBuf> {
    let controller_idx = agent_doc_controller_serve_arg_index(args)?;
    args[controller_idx + 3..]
        .windows(2)
        .find_map(|window| (window[0] == "--project-root").then(|| PathBuf::from(&window[1])))
}

/// True when `cmdline` is a long-lived agent-doc/harness owner invocation for
/// some document, regardless of which document.
pub fn cmdline_is_agent_doc_owner_session(cmdline: &str) -> bool {
    let tokens = cmdline.split_whitespace().collect::<Vec<_>>();
    if let Some(idx) = tokens
        .iter()
        .position(|token| token_is_agent_doc_binary(token))
    {
        let Some(next) = tokens.get(idx + 1) else {
            return false;
        };
        if *next == "start" {
            return true;
        }
        return !token_is_non_owner_agent_doc_subcommand(next);
    }

    tokens.iter().any(|token| token_is_harness_binary(token))
}

/// True when `cmdline` references at least one `.md` document path token.
pub fn cmdline_references_md_document(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|token| {
        token
            .trim_matches(|c| c == '"' || c == '\'')
            .ends_with(".md")
    })
}

/// First `.md` document path token in `cmdline`, for cross-document diagnostics.
pub fn owner_document_from_cmdline(cmdline: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .map(|token| token.trim_matches(|c| c == '"' || c == '\''))
        .find(|token| token.ends_with(".md"))
        .map(|token| token.to_string())
}

pub fn path_has_component_suffix(path: &Path, suffix: &Path) -> bool {
    let path_components: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();
    let suffix_components: Vec<_> = suffix
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();

    if suffix_components.is_empty() || suffix_components.len() > path_components.len() {
        return false;
    }

    path_components[path_components.len() - suffix_components.len()..] == suffix_components[..]
}

pub fn cmdline_has_file_match(cmdline: &str, file_path: &str) -> bool {
    if cmdline.contains(file_path) {
        return true;
    }

    let target = Path::new(file_path);
    let canonical_target = target.canonicalize().ok();
    if let Some(ref canonical) = canonical_target
        && cmdline.contains(canonical.to_string_lossy().as_ref())
    {
        return true;
    }

    for token in cmdline.split_whitespace() {
        let candidate = Path::new(token);
        if candidate.is_absolute() {
            if let Some(ref canonical) = canonical_target
                && candidate.canonicalize().ok().as_ref() == Some(canonical)
            {
                return true;
            }
            continue;
        }

        if path_has_component_suffix(target, candidate) {
            return true;
        }
        if let Some(ref canonical) = canonical_target
            && path_has_component_suffix(canonical, candidate)
        {
            return true;
        }
    }

    false
}

pub fn agent_doc_cmdline_is_owner(cmdline: &str, file_path: &str) -> bool {
    cmdline_has_file_match(cmdline, file_path) && cmdline_is_agent_doc_owner_session(cmdline)
}

/// True when `cmdline` is a live agent-doc/codex owner session for a document
/// OTHER than `claimed_file`. Cross-root safe: it is keyed on the live process
/// command line, so it recognizes a pane owned by a document rooted in another
/// project/submodule whose session registry the calling root cannot see. Used to
/// keep `claim`/`route` from commandeering such a pane.
pub fn cmdline_owns_other_document(cmdline: &str, claimed_file: &str) -> bool {
    cmdline_is_agent_doc_owner_session(cmdline)
        && cmdline_references_md_document(cmdline)
        && !agent_doc_cmdline_is_owner(cmdline, claimed_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_serve_project_root_from_args_extracts_direct_and_shell_sentinel() {
        let args = vec![
            "/some/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&args),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            "/home/me/work/sample-app/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&shell_sentinel),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );
    }

    #[test]
    fn controller_serve_project_root_from_args_rejects_non_controllers() {
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "/bin/agent-doc".to_string(),
                "status".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "sleep".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
        assert_eq!(
            controller_serve_project_root_from_args(&[
                "tmux".to_string(),
                "new-session".to_string(),
                "agent-doc".to_string(),
                "controller".to_string(),
                "serve".to_string(),
                "--project-root".to_string(),
                "/x".to_string(),
            ]),
            None
        );
    }

    #[test]
    fn cmdline_owner_session_recognizes_supervisors_and_harnesses() {
        assert!(cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc start --route-owned tasks/doc.md"
        ));
        assert!(cmdline_is_agent_doc_owner_session(
            "/usr/bin/codex /work/project/tasks/doc.md"
        ));
        assert!(!cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc route tasks/doc.md"
        ));
        assert!(!cmdline_is_agent_doc_owner_session(
            "/home/me/.cargo/bin/agent-doc claim tasks/doc.md --pane %1"
        ));
        assert!(!cmdline_is_agent_doc_owner_session("-zsh"));
    }

    #[test]
    fn owner_document_from_cmdline_extracts_bound_document() {
        assert_eq!(
            owner_document_from_cmdline(
                "/home/me/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md"
            ),
            Some("tasks/software/tsift.md".to_string())
        );
        assert_eq!(
            owner_document_from_cmdline("/usr/bin/codex \"tasks/agent-doc/agent-doc-bugs2.md\""),
            Some("tasks/agent-doc/agent-doc-bugs2.md".to_string())
        );
        assert_eq!(owner_document_from_cmdline("-zsh"), None);
    }

    #[test]
    fn agent_doc_cmdline_owner_detection_only_accepts_start_supervisor() {
        let file = "tasks/live-tmux-repro-codex.md";

        assert!(agent_doc_cmdline_is_owner(
            "/home/brian/.cargo/bin/agent-doc start tasks/live-tmux-repro-codex.md",
            file
        ));
        assert!(agent_doc_cmdline_is_owner(
            "/usr/bin/codex /home/brian/work/btakita/agent-loop/tasks/live-tmux-repro-codex.md",
            file
        ));
        assert!(!agent_doc_cmdline_is_owner(
            "/home/brian/.cargo/bin/agent-doc route tasks/live-tmux-repro-codex.md",
            file
        ));
        assert!(!agent_doc_cmdline_is_owner(
            "/home/brian/.cargo/bin/agent-doc claim tasks/live-tmux-repro-codex.md --pane %522",
            file
        ));
    }

    #[test]
    fn cmdline_owns_other_document_blocks_cross_root_commandeer() {
        let claimed = "tasks/recruit/awear.md";
        assert!(
            cmdline_owns_other_document(
                "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/sampleorders.md",
                claimed,
            ),
            "a pane owning a different document must block commandeering"
        );
        assert!(
            cmdline_owns_other_document(
                "/usr/bin/codex /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md",
                claimed,
            ),
            "a harness session for another document must block commandeering"
        );
    }

    #[test]
    fn cmdline_owns_other_document_allows_same_doc_and_non_owner_panes() {
        let claimed = "tasks/recruit/awear.md";
        assert!(
            !cmdline_owns_other_document(
                "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/recruit/awear.md",
                claimed,
            ),
            "a pane owning the claimed document is reusable"
        );
        assert!(
            !cmdline_owns_other_document("-zsh", claimed),
            "a bare shell does not own another document"
        );
        assert!(
            !cmdline_owns_other_document("/home/brian/.cargo/bin/agent-doc start", claimed),
            "an owner session with no document token is not a different-document conflict"
        );
        assert!(
            !cmdline_owns_other_document(
                "/home/brian/.cargo/bin/agent-doc route tasks/other.md",
                claimed,
            ),
            "a non-owner subcommand is not a live owner session"
        );
    }

    #[test]
    fn cmdline_owns_other_document_blocks_navigation_to_wrong_document_pane() {
        let navigated = "tasks/software/tsift.md";
        assert!(
            cmdline_owns_other_document(
                "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/agent-doc/agent-doc-bugs2.md",
                navigated,
            ),
            "a pane running a different document must not be surfaced as the navigated file's owner"
        );
        assert!(
            !cmdline_owns_other_document(
                "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md",
                navigated,
            ),
            "the navigated document's own owner pane stays reusable under the cross-document guard"
        );
    }

    #[test]
    fn cmdline_cross_document_execution_identifies_foreign_owner_document() {
        let pane_cmdline =
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md";
        let cycle_doc = "tasks/agent-doc/agent-doc-bugs2.md";
        assert!(cmdline_owns_other_document(pane_cmdline, cycle_doc));
        assert_eq!(
            owner_document_from_cmdline(pane_cmdline),
            Some("tasks/software/tsift.md".to_string())
        );
        assert!(!cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/agent-doc/agent-doc-bugs2.md",
            cycle_doc,
        ));
    }

    #[test]
    fn cmdline_file_match_accepts_submodule_relative_start_path() {
        let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
        let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/docs.md";

        assert!(
            cmdline_has_file_match(cmdline, file_path),
            "root-relative target should match pane-relative start path"
        );
    }

    #[test]
    fn cmdline_file_match_rejects_different_relative_path() {
        let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
        let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/other.md";

        assert!(
            !cmdline_has_file_match(cmdline, file_path),
            "different relative path should not match by basename alone"
        );
    }
}
