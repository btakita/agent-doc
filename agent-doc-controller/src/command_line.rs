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

/// Canonicalize a command-line path for identity comparisons, falling back to
/// the raw path when it does not currently resolve.
pub fn canonical_path_for_command_line_compare(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn same_project_controller_args_match_project_root(
    args: &[String],
    project_root: &Path,
) -> bool {
    let Some(raw_root) = controller_serve_project_root_from_args(args) else {
        return false;
    };
    canonical_path_for_command_line_compare(&raw_root)
        == canonical_path_for_command_line_compare(project_root)
}

/// Index of the `agent-doc start` invocation in `args` (direct or under a
/// `sh -c` sentinel), mirroring [`agent_doc_controller_serve_arg_index`].
pub fn agent_doc_start_arg_index(args: &[String]) -> Option<usize> {
    args.windows(2).enumerate().find_map(|(idx, window)| {
        (arg_file_name_is(&window[0], "agent-doc")
            && window[1] == "start"
            && (idx == 0 || is_shell_c_controller_sentinel(args, idx)))
        .then_some(idx)
    })
}

/// Extract the document path a long-lived `agent-doc start --route-owned <doc>`
/// supervisor process is serving. Returns `None` unless the args are an
/// `agent-doc start` invocation carrying the `--route-owned` flag with a `.md`
/// document token after the subcommand. Pure sibling of
/// [`controller_serve_project_root_from_args`]: `/proc` walkers resolve the doc
/// to a project root via the caller's filesystem adapter.
pub fn start_route_owned_document_from_args(args: &[String]) -> Option<PathBuf> {
    let start_idx = agent_doc_start_arg_index(args)?;
    let tail = &args[start_idx + 2..];
    if !tail.iter().any(|arg| arg == "--route-owned") {
        return None;
    }
    tail.iter()
        .find(|arg| {
            let trimmed = arg.trim_matches(|c| c == '"' || c == '\'');
            trimmed.ends_with(".md")
        })
        .map(PathBuf::from)
}

pub fn args_have_preparing_handoff(args: &[String]) -> bool {
    args.windows(2)
        .any(|window| window[0] == "--handoff-state" && window[1] == "preparing")
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

/// First markdown document bound by a command line that is itself recognized
/// as an agent-doc/harness owner session. This is stricter than
/// [`owner_document_from_cmdline`]: transient tools such as `rg SPEC.md` may
/// mention markdown while running below a harness, but they do not own it.
pub fn agent_doc_owner_document_from_cmdline(cmdline: &str) -> Option<String> {
    cmdline_is_agent_doc_owner_session(cmdline).then(|| owner_document_from_cmdline(cmdline))?
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

/// True when `cmdline` is a bare agent-harness session (`claude`, `codex`,
/// `opencode`, …) that carries no `.md` document and was not launched through
/// the `agent-doc` binary — a human's own session, not agent-doc-managed state.
///
/// `#bare-foreign-session-guard`: [`cmdline_owns_other_document`] requires a
/// `.md` token, so a plain `claude` pane answers "owns no document" — and every
/// consumer read that absence of proof as permission, making the operator's own
/// live session electable as a document owner and reapable. The cross-repo
/// guard cannot cover this: a Claude Code session started *inside the project*
/// has the same git toplevel as the document, so it passes every same-repo
/// check. Operator-reported 2026-07-19 — a pure Claude Code session in the
/// project directory (tmux session 1, window 0) was hijacked and its panes
/// killed.
///
/// Ownership must be proven, never assumed: a harness pane agent-doc cannot
/// show it started is foreign, and foreign panes are left alone.
pub fn cmdline_is_unmanaged_harness_session(cmdline: &str) -> bool {
    let tokens = cmdline.split_whitespace().collect::<Vec<_>>();
    // Anything the agent-doc binary launched is managed, whatever its shape.
    if tokens.iter().any(|token| token_is_agent_doc_binary(token)) {
        return false;
    }
    tokens.iter().any(|token| token_is_harness_binary(token))
        && !cmdline_references_md_document(cmdline)
}

/// True when `cmdline` is the `agent-doc` binary itself.
pub fn cmdline_runs_agent_doc_binary(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(token_is_agent_doc_binary)
}

/// Tree-level form of [`cmdline_is_unmanaged_harness_session`]: true when a whole
/// process tree is a harness session agent-doc did not start.
///
/// **This must be decided over the tree, never per process** (`#panehijackself`).
/// agent-doc *starts* the harness as a child, so a managed pane's tree is
///
/// ```text
/// zsh → agent-doc start --route-owned tasks/plan.md → claude --resume <id>
/// ```
///
/// and the `claude` process **on its own** carries neither an `agent-doc` token
/// nor a `.md`, so the per-process predicate answers "unmanaged" for it. Lifting
/// that per-process answer with `any()` therefore called *every* agent-doc-managed
/// pane foreign, and the cross-document owner guard then refused to surface each
/// pane as the owner of its **own** document — which is what stopped the editor's
/// automatic tmux pane swap on document switch.
///
/// The tree is managed iff the `agent-doc` binary appears anywhere in it. A `.md`
/// token elsewhere in the tree is deliberately **not** enough: a transient
/// `rg SPEC.md` running under the operator's own `claude` would otherwise make
/// that session claimable, which is the exact `#bare-foreign-session-guard`
/// regression. Ownership must be proven, never assumed.
pub fn cmdlines_are_unmanaged_harness_session<'a, I>(cmdlines: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let mut saw_unmanaged_harness = false;
    for cmdline in cmdlines {
        if cmdline_runs_agent_doc_binary(cmdline) {
            return false;
        }
        saw_unmanaged_harness |= cmdline_is_unmanaged_harness_session(cmdline);
    }
    saw_unmanaged_harness
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three panes as they really run, captured from the live tmux server
    /// while the automatic pane swap was broken. `agent-doc start` is the pane's
    /// child and the harness is *its* child, so the harness cmdline alone carries
    /// neither an `agent-doc` token nor the document.
    fn managed_pane_tree() -> [&'static str; 3] {
        [
            "-zsh",
            "/home/brian/.cargo/bin/agent-doc start --route-owned \
             --route-owned-reap-policy auto tasks/agent-doc/agent-doc-bugs2.md",
            "/opt/claude-code/bin/claude --dangerously-skip-permissions --model opus \
             --resume 68af54ca-b852-448f-95ed-24f29b695261",
        ]
    }

    /// `#panehijackself` — the regression that broke the editor's automatic tmux
    /// pane swap: every agent-doc-managed pane classified as a foreign session, so
    /// the cross-document owner guard refused to surface any pane as the owner of
    /// its *own* document.
    ///
    /// The per-process predicate answering "unmanaged" for the harness child is
    /// not the bug — that is what it is asked. The bug was `any()`-ing it across
    /// the tree. This asserts both halves, so a future refactor cannot "fix" the
    /// tree predicate by weakening the per-process one.
    #[test]
    fn agent_doc_managed_pane_tree_is_not_an_unmanaged_harness_session() {
        let tree = managed_pane_tree();

        assert!(
            cmdline_is_unmanaged_harness_session(tree[2]),
            "precondition: the harness child alone carries no agent-doc token and no \
             document, so per-process it does look unmanaged — which is why the \
             decision must be made over the tree"
        );

        assert!(
            !cmdlines_are_unmanaged_harness_session(tree),
            "a pane whose tree contains the agent-doc binary is managed; calling it \
             foreign makes the pane unelectable as its own document's owner"
        );
    }

    /// The guard the fix must not weaken: an operator's own harness session stays
    /// foreign even when a transient child mentions markdown. `rg SPEC.md` under a
    /// bare `claude` must not launder that pane into a claimable one.
    #[test]
    fn bare_operator_harness_tree_stays_unmanaged_even_with_a_markdown_child() {
        assert!(
            cmdlines_are_unmanaged_harness_session([
                "-zsh",
                "/opt/claude-code/bin/claude --dangerously-skip-permissions",
                "rg --line-number needle SPEC.md",
            ]),
            "a markdown token below the operator's own harness is not proof agent-doc \
             started it (#bare-foreign-session-guard)"
        );
        assert!(
            !cmdlines_are_unmanaged_harness_session(["-zsh", "vim notes.md"]),
            "a tree with no harness session at all is not an unmanaged harness session"
        );
    }

    /// `#bare-foreign-session-guard` — the operator's own Claude Code session,
    /// started by hand in the project directory with no document argument. It
    /// binds no `.md`, so `cmdline_owns_other_document` answers false; the
    /// unmanaged-harness predicate is what keeps it from being claimed.
    #[test]
    fn bare_operator_harness_session_is_recognized_as_unmanaged() {
        for cmdline in [
            "claude",
            "/usr/bin/claude",
            "claude --continue",
            // The exact cmdline observed in the hijacked pane (tmux 1:0).
            "/opt/claude-code/bin/claude --resume",
            "codex",
            "opencode",
        ] {
            assert!(
                cmdline_is_unmanaged_harness_session(cmdline),
                "operator-started harness session must be unmanaged: {cmdline}"
            );
            assert!(
                !cmdline_owns_other_document(cmdline, "/w/tasks/session.md"),
                "precondition: a bare harness session binds no document, which is \
                 exactly why the unmanaged guard is required: {cmdline}"
            );
        }
    }

    /// agent-doc's OWN panes must stay managed, or the guard would protect them
    /// from the reaping and re-election they legitimately need.
    #[test]
    fn agent_doc_managed_sessions_are_not_treated_as_unmanaged() {
        for cmdline in [
            "agent-doc start tasks/session.md",
            "agent-doc start --route-owned /w/tasks/session.md",
            "/home/me/.cargo/bin/agent-doc start --force tasks/other.md",
            "claude /w/tasks/session.md",
        ] {
            assert!(
                !cmdline_is_unmanaged_harness_session(cmdline),
                "agent-doc-managed session must not be treated as unmanaged: {cmdline}"
            );
        }
    }

    /// A non-harness pane is not protected by this guard — it has nothing to do
    /// with agent sessions and must keep its existing classification.
    #[test]
    fn plain_shell_panes_are_not_unmanaged_harness_sessions() {
        for cmdline in ["zsh", "bash", "vim notes.md", "uv run dev", "npm run dev"] {
            assert!(
                !cmdline_is_unmanaged_harness_session(cmdline),
                "non-harness pane must not match the harness guard: {cmdline}"
            );
        }
    }

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
    fn start_route_owned_document_from_args_extracts_direct_and_shell_sentinel() {
        let args = vec![
            "/home/me/.cargo/bin/agent-doc".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
            "tasks/software/tsift.md".to_string(),
        ];
        assert_eq!(
            start_route_owned_document_from_args(&args),
            Some(PathBuf::from("tasks/software/tsift.md"))
        );

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            "/home/me/work/sample-app/agent-doc".to_string(),
            "start".to_string(),
            "--route-owned".to_string(),
            "tasks/doc.md".to_string(),
        ];
        assert_eq!(
            start_route_owned_document_from_args(&shell_sentinel),
            Some(PathBuf::from("tasks/doc.md"))
        );
    }

    #[test]
    fn start_route_owned_document_from_args_rejects_non_route_owned_starts() {
        // `start` without `--route-owned` is not a route-owned supervisor.
        assert_eq!(
            start_route_owned_document_from_args(&[
                "/bin/agent-doc".to_string(),
                "start".to_string(),
                "tasks/doc.md".to_string(),
            ]),
            None
        );
        // A non-start subcommand never matches.
        assert_eq!(
            start_route_owned_document_from_args(&[
                "/bin/agent-doc".to_string(),
                "route".to_string(),
                "--route-owned".to_string(),
                "tasks/doc.md".to_string(),
            ]),
            None
        );
        // `--route-owned` present but no `.md` document token.
        assert_eq!(
            start_route_owned_document_from_args(&[
                "/bin/agent-doc".to_string(),
                "start".to_string(),
                "--route-owned".to_string(),
            ]),
            None
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
    fn same_project_controller_args_match_project_root_matches_only_same_controller_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let args = vec![
            "/home/user/.cargo/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(same_project_controller_args_match_project_root(
            &args,
            dir.path()
        ));

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            dir.path().join("agent-doc").display().to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(same_project_controller_args_match_project_root(
            &shell_sentinel,
            dir.path()
        ));

        let other_dir = tempfile::TempDir::new().unwrap();
        assert!(!same_project_controller_args_match_project_root(
            &args,
            other_dir.path()
        ));

        let non_controller = vec![
            "agent-doc".to_string(),
            "preflight".to_string(),
            dir.path().join("task.md").display().to_string(),
        ];
        assert!(!same_project_controller_args_match_project_root(
            &non_controller,
            dir.path()
        ));

        let tmux_launcher = vec![
            "tmux".to_string(),
            "new-session".to_string(),
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(!same_project_controller_args_match_project_root(
            &tmux_launcher,
            dir.path()
        ));
    }

    #[test]
    fn args_have_preparing_handoff_detects_exact_flag_pair() {
        assert!(args_have_preparing_handoff(&[
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ]));
        assert!(!args_have_preparing_handoff(&[
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--handoff-state".to_string(),
            "stable".to_string(),
        ]));
        assert!(!args_have_preparing_handoff(&[
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "preparing".to_string(),
            "--handoff-state".to_string(),
        ]));
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
    fn agent_doc_owner_document_ignores_transient_markdown_tools() {
        assert_eq!(
            agent_doc_owner_document_from_cmdline(
                "agent-doc start --route-owned /repo/tasks/selected.md"
            )
            .as_deref(),
            Some("/repo/tasks/selected.md"),
        );
        assert_eq!(agent_doc_owner_document_from_cmdline("rg SPEC.md"), None);
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
