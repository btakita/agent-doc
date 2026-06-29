//! Pure supervisor configuration precedence.

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

pub fn source_newer_than_installed_binary(
    newest_source_secs: u64,
    installed_binary_secs: u64,
) -> bool {
    newest_source_secs > installed_binary_secs
}

pub fn auto_install_should_retry(attempt: u32, max_attempts: u32) -> bool {
    attempt < max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
