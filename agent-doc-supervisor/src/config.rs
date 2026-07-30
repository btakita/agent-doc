//! Pure supervisor configuration precedence.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentLaunchArgsSources {
    pub frontmatter_agent_args: Option<String>,
    pub frontmatter_claude_args: Option<String>,
    pub frontmatter_codex_args: Option<String>,
    pub frontmatter_opencode_args: Option<String>,
    pub config_agent_args: Option<String>,
    pub config_claude_args: Option<String>,
    pub config_codex_args: Option<String>,
    pub config_opencode_args: Option<String>,
    pub env_claude_args: Option<String>,
}

pub fn resolve_agent_launch_args(
    harness_binary: &str,
    sources: AgentLaunchArgsSources,
) -> Option<String> {
    match harness_binary {
        "claude" => sources
            .frontmatter_agent_args
            .or(sources.frontmatter_claude_args)
            .or(sources.config_agent_args)
            .or(sources.config_claude_args)
            .or(sources.env_claude_args),
        "codex" => sources
            .frontmatter_agent_args
            .or(sources.frontmatter_codex_args)
            .or(sources.config_agent_args)
            .or(sources.config_codex_args),
        "opencode" => sources
            .frontmatter_agent_args
            .or(sources.frontmatter_opencode_args)
            .or(sources.config_agent_args)
            .or(sources.config_opencode_args),
        _ => sources.frontmatter_agent_args.or(sources.config_agent_args),
    }
}

fn truthy_or_falsey(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn resolve_default_on_bool(
    env: Option<&str>,
    frontmatter: Option<bool>,
    project: Option<bool>,
) -> bool {
    if let Some(raw) = env
        && let Some(value) = truthy_or_falsey(raw)
    {
        return value;
    }
    frontmatter.or(project).unwrap_or(true)
}

pub fn resolve_supervisor_auto_recycle(
    env: Option<&str>,
    frontmatter: Option<bool>,
    project: Option<bool>,
) -> bool {
    resolve_default_on_bool(env, frontmatter, project)
}

pub fn resolve_agent_change_restart(
    env: Option<&str>,
    frontmatter: Option<bool>,
    project: Option<bool>,
) -> bool {
    resolve_default_on_bool(env, frontmatter, project)
}

pub fn resolve_supervisor_auto_install(
    env: Option<&str>,
    frontmatter: Option<bool>,
    project: Option<bool>,
) -> bool {
    resolve_default_on_bool(env, frontmatter, project)
}

pub fn is_agent_doc_dogfood_session(file: &Path, project_root: &Path, crate_root: &Path) -> bool {
    if crate_root == project_root {
        return file.starts_with(project_root);
    }
    if file.starts_with(crate_root) {
        return true;
    }
    let Ok(relative) = file.strip_prefix(project_root) else {
        return false;
    };
    let mut components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str());
    let Some(first) = components.next() else {
        return false;
    };
    if first != "tasks" {
        return false;
    }
    let Some(second) = components.next() else {
        return false;
    };
    if second == "agent-doc" {
        return true;
    }
    if second.starts_with("agent-doc") && second.ends_with(".md") {
        return true;
    }
    second == "software" && components.next() == Some("agent-doc.md")
}

pub fn source_newer_than_installed_binary(
    newest_source_secs: u64,
    installed_binary_secs: u64,
) -> bool {
    newest_source_secs > installed_binary_secs
}

pub fn auto_install_should_retry(attempt: u32, max_attempts: u32) -> bool {
    attempt < max_attempts
}

/// Grace window (seconds) so an artifact built effectively at the same time as
/// the latest source edit is not flagged. This tolerates coarse mtime
/// resolution and source files touched moments after a build starts.
pub const STALE_INSTALL_GRACE_SECS: u64 = 300;

/// Classify installed artifacts whose mtimes predate the latest source edit by
/// more than `grace_secs`. `None` mtime means the artifact is absent and is not
/// itself stale.
pub fn classify_stale_install_artifacts<'a>(
    source_ts: u64,
    artifacts: &[(&'a str, Option<u64>)],
    grace_secs: u64,
) -> Vec<&'a str> {
    artifacts
        .iter()
        .filter_map(|(label, mtime)| match mtime {
            Some(m) if m.saturating_add(grace_secs) < source_ts => Some(*label),
            _ => None,
        })
        .collect()
}

/// Route-owned host supervisor staleness is binary identity, not process start time.
///
/// A supervisor that re-exec'd in place keeps its old start time but maps the installed
/// binary inode, so it is fresh. Unknown running inode fails open as not stale.
pub fn host_supervisor_is_stale(
    running_exe_inode: Option<u64>,
    installed_binary_inode: u64,
) -> bool {
    match running_exe_inode {
        Some(running) => running != installed_binary_inode,
        None => false,
    }
}

/// Positive proof that a live supervisor maps the currently installed binary.
///
/// An unreadable `/proc/<pid>/exe` is unknown, not fresh. Callers that report a
/// completed replacement must require this stronger predicate instead of
/// treating the continued existence of the old PID as success.
pub fn host_supervisor_maps_installed_binary(
    running_exe_inode: Option<u64>,
    installed_binary_inode: u64,
) -> bool {
    running_exe_inode.is_some_and(|running| running == installed_binary_inode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch_sources() -> AgentLaunchArgsSources {
        AgentLaunchArgsSources::default()
    }

    #[test]
    fn agent_launch_args_claude_prefers_claude_alias_chain() {
        let sources = AgentLaunchArgsSources {
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            config_claude_args: Some("--old-flag".into()),
            env_claude_args: Some("--env-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("claude", sources).as_deref(),
            Some("--dangerously-skip-permissions")
        );
    }

    #[test]
    fn agent_launch_args_claude_prefers_agent_args_over_claude_args() {
        let sources = AgentLaunchArgsSources {
            frontmatter_agent_args: Some("--model sonnet".into()),
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("claude", sources).as_deref(),
            Some("--model sonnet")
        );
    }

    #[test]
    fn agent_launch_args_claude_uses_env_fallback_last() {
        let sources = AgentLaunchArgsSources {
            env_claude_args: Some("--env-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("claude", sources).as_deref(),
            Some("--env-flag")
        );
    }

    #[test]
    fn agent_launch_args_codex_prefers_codex_alias_chain() {
        let sources = AgentLaunchArgsSources {
            frontmatter_codex_args: Some("-s danger-full-access".into()),
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            config_codex_args: Some("-s workspace-write".into()),
            config_claude_args: Some("--old-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("codex", sources).as_deref(),
            Some("-s danger-full-access")
        );
    }

    #[test]
    fn agent_launch_args_codex_ignores_claude_args_aliases() {
        let sources = AgentLaunchArgsSources {
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            config_claude_args: Some("--old-flag".into()),
            env_claude_args: Some("--env-flag".into()),
            ..launch_sources()
        };

        assert_eq!(resolve_agent_launch_args("codex", sources), None);
    }

    #[test]
    fn agent_launch_args_codex_uses_agent_args_only() {
        let sources = AgentLaunchArgsSources {
            frontmatter_agent_args: Some("-s danger-full-access".into()),
            frontmatter_codex_args: Some("-s workspace-write".into()),
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            config_agent_args: Some("-s workspace-write".into()),
            config_codex_args: Some("-s read-only".into()),
            config_claude_args: Some("--old-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("codex", sources).as_deref(),
            Some("-s danger-full-access")
        );
    }

    #[test]
    fn agent_launch_args_codex_uses_config_codex_args_fallback() {
        let sources = AgentLaunchArgsSources {
            config_codex_args: Some("-s danger-full-access".into()),
            config_claude_args: Some("--old-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("codex", sources).as_deref(),
            Some("-s danger-full-access")
        );
    }

    #[test]
    fn agent_launch_args_opencode_prefers_opencode_alias_chain() {
        let sources = AgentLaunchArgsSources {
            frontmatter_opencode_args: Some("--dangerously-skip-permissions".into()),
            frontmatter_codex_args: Some("-s danger-full-access".into()),
            frontmatter_claude_args: Some("--old-claude".into()),
            config_opencode_args: Some("--from-config".into()),
            config_codex_args: Some("-s workspace-write".into()),
            config_claude_args: Some("--old-flag".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("opencode", sources).as_deref(),
            Some("--dangerously-skip-permissions")
        );
    }

    #[test]
    fn agent_launch_args_opencode_ignores_claude_and_codex_aliases() {
        let sources = AgentLaunchArgsSources {
            frontmatter_claude_args: Some("--dangerously-skip-permissions".into()),
            frontmatter_codex_args: Some("-s danger-full-access".into()),
            config_claude_args: Some("--old-flag".into()),
            config_codex_args: Some("-s workspace-write".into()),
            env_claude_args: Some("--env-flag".into()),
            ..launch_sources()
        };

        assert_eq!(resolve_agent_launch_args("opencode", sources), None);
    }

    #[test]
    fn agent_launch_args_opencode_uses_config_opencode_args_fallback() {
        let sources = AgentLaunchArgsSources {
            config_opencode_args: Some("--dangerously-skip-permissions".into()),
            config_claude_args: Some("--old-flag".into()),
            config_codex_args: Some("-s workspace-write".into()),
            ..launch_sources()
        };

        assert_eq!(
            resolve_agent_launch_args("opencode", sources).as_deref(),
            Some("--dangerously-skip-permissions")
        );
    }

    #[test]
    fn default_on_bool_precedence_is_env_frontmatter_project_default() {
        let r = resolve_default_on_bool;

        assert!(r(Some("1"), Some(false), Some(false)));
        assert!(r(Some("true"), None, None));
        assert!(r(Some(" ON "), Some(false), Some(false)));
        assert!(!r(Some("0"), Some(true), Some(true)));
        assert!(!r(Some("off"), Some(true), Some(true)));

        assert!(r(None, Some(true), Some(false)));
        assert!(!r(None, Some(false), Some(true)));
        assert!(r(Some("garbage"), Some(true), Some(false)));

        assert!(r(None, None, Some(true)));
        assert!(!r(None, None, Some(false)));
        assert!(r(None, None, None));
        assert!(r(Some(""), None, None));
    }

    #[test]
    fn named_supervisor_knobs_share_default_on_precedence() {
        assert!(resolve_supervisor_auto_recycle(None, None, None));
        assert!(resolve_agent_change_restart(None, None, None));
        assert!(resolve_supervisor_auto_install(None, None, None));
        assert!(!resolve_supervisor_auto_recycle(
            Some("off"),
            Some(true),
            Some(true)
        ));
        assert!(!resolve_agent_change_restart(
            Some("off"),
            Some(true),
            Some(true)
        ));
        assert!(!resolve_supervisor_auto_install(
            Some("off"),
            Some(true),
            Some(true)
        ));
    }

    #[test]
    fn dogfood_session_path_policy_accepts_agent_doc_scope_only() {
        let project = std::path::Path::new("/workspace");
        let crate_root = project.join("src/agent-doc");

        assert!(is_agent_doc_dogfood_session(
            &project.join("src/agent-doc/specs/supervisor.md"),
            project,
            &crate_root
        ));
        assert!(is_agent_doc_dogfood_session(
            &project.join("tasks/agent-doc/agent-doc-bugs2.md"),
            project,
            &crate_root
        ));
        assert!(is_agent_doc_dogfood_session(
            &project.join("tasks/agent-doc-bugs.md"),
            project,
            &crate_root
        ));
        assert!(is_agent_doc_dogfood_session(
            &project.join("tasks/software/agent-doc.md"),
            project,
            &crate_root
        ));
        assert!(!is_agent_doc_dogfood_session(
            &project.join("tasks/professional/sampleportal.md"),
            project,
            &crate_root
        ));
        assert!(!is_agent_doc_dogfood_session(
            &project.join("tasks/software/lazily-rs.md"),
            project,
            &crate_root
        ));
        assert!(!is_agent_doc_dogfood_session(
            std::path::Path::new("/other/tasks/agent-doc/agent-doc-bugs2.md"),
            project,
            &crate_root
        ));
    }

    #[test]
    fn dogfood_session_path_policy_accepts_whole_project_when_crate_is_project_root() {
        let project = std::path::Path::new("/workspace/agent-doc");

        assert!(is_agent_doc_dogfood_session(
            &project.join("src/main.rs"),
            project,
            project
        ));
        assert!(!is_agent_doc_dogfood_session(
            std::path::Path::new("/workspace/other/tasks/agent-doc.md"),
            project,
            project
        ));
    }

    #[test]
    fn source_newer_than_installed_binary_is_strict() {
        assert!(source_newer_than_installed_binary(101, 100));
        assert!(!source_newer_than_installed_binary(100, 100));
        assert!(!source_newer_than_installed_binary(99, 100));
    }

    #[test]
    fn auto_install_retries_until_final_attempt() {
        assert!(auto_install_should_retry(1, 3));
        assert!(auto_install_should_retry(2, 3));
        assert!(!auto_install_should_retry(3, 3));
        assert!(!auto_install_should_retry(4, 3));
        assert!(!auto_install_should_retry(1, 1));
    }

    #[test]
    fn stale_install_classifier_flags_only_artifacts_older_than_source_edit() {
        let source = 10_000u64;
        let grace = 60u64;

        assert!(
            classify_stale_install_artifacts(
                source,
                &[("bin", Some(source + 5)), ("cdylib", Some(source + 1))],
                grace,
            )
            .is_empty()
        );

        let stale = classify_stale_install_artifacts(
            source,
            &[("bin", Some(source - 600)), ("cdylib", Some(source + 1))],
            grace,
        );
        assert_eq!(stale, vec!["bin"]);

        assert!(
            classify_stale_install_artifacts(source, &[("bin", Some(source - 30))], grace)
                .is_empty()
        );
        assert_eq!(
            classify_stale_install_artifacts(source, &[("bin", Some(source - 61))], grace),
            vec!["bin"]
        );

        assert!(
            classify_stale_install_artifacts(source, &[("bin", None), ("cdylib", None)], grace)
                .is_empty()
        );
    }

    #[test]
    fn stale_install_uses_source_file_mtime_not_commit_time_so_build_before_commit_is_not_stale() {
        let source_ts = 10_000u64;
        let grace = STALE_INSTALL_GRACE_SECS;

        assert!(
            classify_stale_install_artifacts(source_ts, &[("bin", Some(source_ts + 10))], grace)
                .is_empty(),
            "a binary built just after the source edit must not be stale"
        );

        assert!(
            classify_stale_install_artifacts(source_ts, &[("bin", Some(source_ts + 660))], grace)
                .is_empty(),
            "a binary newer than the source edit is fresh regardless of any later commit time"
        );

        assert_eq!(
            classify_stale_install_artifacts(
                source_ts,
                &[("bin", Some(source_ts - grace - 1))],
                grace,
            ),
            vec!["bin"],
            "a binary older than the source edit beyond the grace must still flag"
        );
    }

    #[test]
    fn host_supervisor_staleness_is_inode_identity() {
        let installed_inode = 4242u64;

        assert!(host_supervisor_is_stale(
            Some(installed_inode + 1),
            installed_inode
        ));
        assert!(!host_supervisor_is_stale(
            Some(installed_inode),
            installed_inode
        ));
        assert!(!host_supervisor_is_stale(None, installed_inode));
    }

    #[test]
    fn host_supervisor_freshness_requires_positive_inode_identity() {
        let installed_inode = 4242u64;

        assert!(host_supervisor_maps_installed_binary(
            Some(installed_inode),
            installed_inode
        ));
        assert!(!host_supervisor_maps_installed_binary(
            Some(installed_inode + 1),
            installed_inode
        ));
        assert!(!host_supervisor_maps_installed_binary(
            None,
            installed_inode
        ));
    }
}
