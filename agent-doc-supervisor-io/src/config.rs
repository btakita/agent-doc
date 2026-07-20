//! File/env-backed supervisor configuration readers.
//!
//! This module gathers effectful sources for supervisor feature knobs
//! (environment, document frontmatter, project config) and delegates precedence
//! to `agent_doc_supervisor::config`.

use std::path::Path;

pub const SUPERVISOR_AUTO_RECYCLE_ENV: &str = "AGENT_DOC_SUPERVISOR_AUTO_RECYCLE";
pub const AGENT_CHANGE_RESTART_ENV: &str = "AGENT_DOC_AGENT_CHANGE_RESTART";
pub const SUPERVISOR_AUTO_INSTALL_ENV: &str = "AGENT_DOC_SUPERVISOR_AUTO_INSTALL";

fn frontmatter_bool(
    doc: &Path,
    read: impl FnOnce(agent_doc_frontmatter::frontmatter::Frontmatter) -> Option<bool>,
) -> Option<bool> {
    std::fs::read_to_string(doc).ok().and_then(|content| {
        agent_doc_frontmatter::frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| read(fm))
    })
}

/// Resolve supervisor auto-recycle from env, document frontmatter, and project config.
pub fn supervisor_auto_recycle_enabled(doc: &Path) -> bool {
    let env = std::env::var(SUPERVISOR_AUTO_RECYCLE_ENV).ok();
    let frontmatter = frontmatter_bool(doc, |fm| fm.supervisor_auto_recycle);
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_supervisor_auto_recycle;
    agent_doc_supervisor::config::resolve_supervisor_auto_recycle(
        env.as_deref(),
        frontmatter,
        project,
    )
}

/// Resolve agent-change restart from env, document frontmatter, and project config.
pub fn agent_change_restart_enabled(doc: &Path) -> bool {
    let env = std::env::var(AGENT_CHANGE_RESTART_ENV).ok();
    let frontmatter = frontmatter_bool(doc, |fm| fm.agent_change_restart);
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_agent_change_restart;
    agent_doc_supervisor::config::resolve_agent_change_restart(env.as_deref(), frontmatter, project)
}

/// Resolve supervisor auto-install from env, document frontmatter, and project config.
pub fn supervisor_auto_install_enabled(doc: &Path) -> bool {
    let env = std::env::var(SUPERVISOR_AUTO_INSTALL_ENV).ok();
    let frontmatter = frontmatter_bool(doc, |fm| fm.supervisor_auto_install);
    let project =
        agent_doc_project_config_io::load_project_for_doc(doc).agent_doc_supervisor_auto_install;
    agent_doc_supervisor::config::resolve_supervisor_auto_install(
        env.as_deref(),
        frontmatter,
        project,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, prior }
        }

        fn unset(key: &'static str) -> Self {
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, prior }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn write_doc(root: &Path, frontmatter: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("doc.md");
        std::fs::write(&doc, frontmatter).unwrap();
        doc
    }

    #[test]
    fn supervisor_feature_knobs_resolve_from_frontmatter_over_project() {
        let _lock = ENV_LOCK.lock();
        let _recycle = EnvGuard::unset(SUPERVISOR_AUTO_RECYCLE_ENV);
        let _restart = EnvGuard::unset(AGENT_CHANGE_RESTART_ENV);
        let _install = EnvGuard::unset(SUPERVISOR_AUTO_INSTALL_ENV);
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = write_doc(
            tmp.path(),
            "---\nagent_doc_supervisor_auto_recycle: true\nagent_doc_agent_change_restart: true\nagent_doc_supervisor_auto_install: true\n---\n",
        );
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "agent_doc_supervisor_auto_recycle = false\nagent_doc_agent_change_restart = false\nagent_doc_supervisor_auto_install = false\n",
        )
        .unwrap();

        assert!(supervisor_auto_recycle_enabled(&doc));
        assert!(agent_change_restart_enabled(&doc));
        assert!(supervisor_auto_install_enabled(&doc));
    }

    #[test]
    fn supervisor_feature_knobs_resolve_env_over_frontmatter() {
        let _lock = ENV_LOCK.lock();
        let _recycle = EnvGuard::set(SUPERVISOR_AUTO_RECYCLE_ENV, "off");
        let _restart = EnvGuard::set(AGENT_CHANGE_RESTART_ENV, "off");
        let _install = EnvGuard::set(SUPERVISOR_AUTO_INSTALL_ENV, "off");
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = write_doc(
            tmp.path(),
            "---\nagent_doc_supervisor_auto_recycle: true\nagent_doc_agent_change_restart: true\nagent_doc_supervisor_auto_install: true\n---\n",
        );

        assert!(!supervisor_auto_recycle_enabled(&doc));
        assert!(!agent_change_restart_enabled(&doc));
        assert!(!supervisor_auto_install_enabled(&doc));
    }
}
