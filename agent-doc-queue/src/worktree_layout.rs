//! Paired agent-doc worktree layout (`#worktreetmuxrouterskew`).
//!
//! The workspace manifest depends on tmux-router by `path = "../tmux-router"`.
//! A crate one directory deeper uses `../../tmux-router`, which is the same
//! sibling of the agent-doc checkout. A checkout placed directly under a
//! directory named `worktrees` (the shared `~/worktrees` root) therefore
//! resolves that path to `<worktrees>/tmux-router`. That shared checkout is
//! not this worktree's private sibling and is stale against main. Setup must
//! not choose it, and must not refresh or move it.
//!
//! The paired layout nests the checkout one level down:
//!
//! ```text
//! <worktrees-root>/<label>/
//!   agent-doc/      git worktree of agent-doc
//!   tmux-router/    detached worktree at the revision this tree builds against
//! ```
//!
//! `../tmux-router` is then `<worktrees-root>/<label>/tmux-router`.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Directory name of the tmux-router path dependency.
pub const TMUX_ROUTER_DIR_NAME: &str = "tmux-router";

/// Directory name of the agent-doc checkout inside a paired layout.
pub const AGENT_DOC_DIR_NAME: &str = "agent-doc";

/// Directory name of the shared worktree root whose direct child named
/// `tmux-router` is the stale checkout flat layouts resolve.
pub const SHARED_WORKTREES_DIR_NAME: &str = "worktrees";

/// Coordinator and worker rule (`subagent_worktree_layout_rule!`).
///
/// A string literal so dispatch guidance can `concat!` it. Exchange subagents
/// have no preamble, so those texts append this rule themselves.
#[macro_export]
macro_rules! subagent_worktree_layout_rule {
    () => {
        "Worktree setup (`#worktreetmuxrouterskew`): agent-doc depends on tmux-router by `path = \"../tmux-router\"`, and a nested crate's `../../tmux-router` is that same sibling. Never place the agent-doc worktree directly under `~/worktrees`: that resolves `../tmux-router` to the shared `~/worktrees/tmux-router` checkout, which is stale against main and must not be refreshed or moved. Use the paired layout `<parent>/<label>/agent-doc` beside a private detached `<parent>/<label>/tmux-router` at the tmux-router revision this agent-doc tree builds against, so `../tmux-router` is that private sibling and never the shared checkout."
    };
}

/// [`subagent_worktree_layout_rule!`] as a constant.
pub const SUBAGENT_WORKTREE_LAYOUT_RULE: &str = crate::subagent_worktree_layout_rule!();

/// Where the agent-doc worktree and its private tmux-router sibling live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedWorktreeLayout {
    /// `<worktrees-root>/<label>`.
    pub parent: PathBuf,
    /// `<parent>/agent-doc`.
    pub agent_doc: PathBuf,
    /// `<parent>/tmux-router`, the path `../tmux-router` resolves to.
    pub tmux_router: PathBuf,
}

/// Workspace path dependency `../<dep>` from an agent-doc worktree root.
pub fn workspace_path_dep(agent_doc_worktree: &Path, dep: &str) -> PathBuf {
    agent_doc_worktree.join("..").join(dep)
}

/// `../../<dep>` from a crate directory inside the agent-doc worktree.
///
/// That is the manifest path nested crates use. It names the same sibling as
/// [`workspace_path_dep`] once `.` and `..` are collapsed.
pub fn nested_crate_path_dep(agent_doc_worktree: &Path, dep: &str) -> PathBuf {
    agent_doc_worktree
        .join("nested-crate")
        .join("..")
        .join("..")
        .join(dep)
}

/// Collapse `.` and `..` without reading the filesystem.
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::ParentDir) | None => out.push(".."),
                Some(Component::RootDir) | Some(Component::Prefix(_)) | Some(Component::CurDir) => {
                }
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when `path` is `<something>/worktrees/tmux-router`.
pub fn is_shared_worktrees_tmux_router(path: &Path) -> bool {
    path.file_name() == Some(OsStr::new(TMUX_ROUTER_DIR_NAME))
        && path.parent().and_then(Path::file_name) == Some(OsStr::new(SHARED_WORKTREES_DIR_NAME))
}

/// True when `agent_doc_worktree`'s `../tmux-router` is the shared checkout.
///
/// Lexical only: the shared directory does not have to exist. Choosing the
/// path is the bug, whether or not that checkout is currently on disk.
pub fn resolves_shared_worktrees_tmux_router(agent_doc_worktree: &Path) -> bool {
    let resolved = normalize_lexical(&workspace_path_dep(
        agent_doc_worktree,
        TMUX_ROUTER_DIR_NAME,
    ));
    is_shared_worktrees_tmux_router(&resolved)
}

/// Paired layout under `worktrees_root` for `label`.
///
/// The returned agent-doc path does not resolve `../tmux-router` to the
/// shared checkout. `label` is one path segment, so it cannot escape the root.
pub fn paired_layout(
    worktrees_root: &Path,
    label: &str,
) -> Result<PairedWorktreeLayout, &'static str> {
    if worktrees_root.as_os_str().is_empty() {
        return Err("worktrees root is empty");
    }
    validate_label(label)?;
    let parent = worktrees_root.join(label);
    let agent_doc = parent.join(AGENT_DOC_DIR_NAME);
    let tmux_router = parent.join(TMUX_ROUTER_DIR_NAME);
    if resolves_shared_worktrees_tmux_router(&agent_doc) {
        return Err("paired layout resolved the shared tmux-router checkout");
    }
    let resolved = normalize_lexical(&workspace_path_dep(&agent_doc, TMUX_ROUTER_DIR_NAME));
    if resolved != tmux_router {
        return Err("paired layout tmux-router path does not match ../tmux-router");
    }
    Ok(PairedWorktreeLayout {
        parent,
        agent_doc,
        tmux_router,
    })
}

fn validate_label(label: &str) -> Result<(), &'static str> {
    if label.is_empty()
        || label == "."
        || label == ".."
        || label.contains('/')
        || label.contains('\\')
        || label.contains('\0')
    {
        return Err("worktree label must be a single path segment");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_worktrees() -> PathBuf {
        PathBuf::from("/home/brian/worktrees")
    }

    #[test]
    fn flat_worktree_under_worktrees_resolves_the_shared_checkout() {
        let flat = home_worktrees().join("agent-doc-foo");
        assert!(resolves_shared_worktrees_tmux_router(&flat));
        assert_eq!(
            normalize_lexical(&workspace_path_dep(&flat, TMUX_ROUTER_DIR_NAME)),
            home_worktrees().join(TMUX_ROUTER_DIR_NAME)
        );
        assert_eq!(
            normalize_lexical(&nested_crate_path_dep(&flat, TMUX_ROUTER_DIR_NAME)),
            home_worktrees().join(TMUX_ROUTER_DIR_NAME)
        );
    }

    #[test]
    fn every_direct_child_of_worktrees_resolves_the_shared_checkout() {
        for name in [
            "agent-doc-foo",
            ".agent-doc-build-agent-doc-1-2",
            "tmux-router",
        ] {
            let flat = home_worktrees().join(name);
            assert!(
                resolves_shared_worktrees_tmux_router(&flat),
                "{name} should resolve the shared checkout"
            );
        }
    }

    #[test]
    fn paired_layout_resolves_a_private_sibling_not_the_shared_checkout() {
        let layout = paired_layout(&home_worktrees(), "agent-doc-foo").unwrap();
        assert_eq!(
            layout.agent_doc,
            home_worktrees()
                .join("agent-doc-foo")
                .join(AGENT_DOC_DIR_NAME)
        );
        assert_eq!(
            layout.tmux_router,
            home_worktrees()
                .join("agent-doc-foo")
                .join(TMUX_ROUTER_DIR_NAME)
        );
        assert!(!resolves_shared_worktrees_tmux_router(&layout.agent_doc));
        assert_ne!(
            layout.tmux_router,
            home_worktrees().join(TMUX_ROUTER_DIR_NAME)
        );
        assert_eq!(
            normalize_lexical(&workspace_path_dep(&layout.agent_doc, TMUX_ROUTER_DIR_NAME)),
            layout.tmux_router
        );
        assert_eq!(
            normalize_lexical(&nested_crate_path_dep(
                &layout.agent_doc,
                TMUX_ROUTER_DIR_NAME
            )),
            layout.tmux_router
        );
        assert!(!is_shared_worktrees_tmux_router(&layout.tmux_router));
    }

    #[test]
    fn source_sibling_under_src_is_not_the_shared_worktrees_checkout() {
        let repo = PathBuf::from("/tmp/workspace/src/agent-doc");
        let build = PathBuf::from("/tmp/workspace/src/.agent-doc-build-agent-doc-1-2");
        assert!(!resolves_shared_worktrees_tmux_router(&repo));
        assert!(!resolves_shared_worktrees_tmux_router(&build));
        assert_eq!(
            normalize_lexical(&workspace_path_dep(&build, TMUX_ROUTER_DIR_NAME)),
            PathBuf::from("/tmp/workspace/src/tmux-router")
        );
    }

    #[test]
    fn label_cannot_escape_the_worktrees_root() {
        assert_eq!(
            paired_layout(&home_worktrees(), ".."),
            Err("worktree label must be a single path segment")
        );
        assert_eq!(
            paired_layout(&home_worktrees(), "a/b"),
            Err("worktree label must be a single path segment")
        );
        assert_eq!(
            paired_layout(&home_worktrees(), ""),
            Err("worktree label must be a single path segment")
        );
        assert_eq!(
            paired_layout(Path::new(""), "label"),
            Err("worktrees root is empty")
        );
    }

    #[test]
    fn rule_names_the_paired_layout_and_forbids_the_shared_checkout() {
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("#worktreetmuxrouterskew"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("`path = \"../tmux-router\"`"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("`../../tmux-router`"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("~/worktrees/tmux-router"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("must not be refreshed or moved"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("`<parent>/<label>/agent-doc`"));
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("`<parent>/<label>/tmux-router`"));
        assert_eq!(
            SUBAGENT_WORKTREE_LAYOUT_RULE,
            crate::subagent_worktree_layout_rule!()
        );
    }
}
