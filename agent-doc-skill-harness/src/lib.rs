//! Agent Doc's downstream integration for `skill-harness`.
//!
//! The generic registry stays in `skill-harness`; Agent Doc owns its detection
//! marker, priority, and install path here. Consumers opt in by calling
//! [`register_agent_doc_plugin`] on their registry.

use std::path::PathBuf;

use skill_harness::{PluginContext, PluginRegistry, SkillHarnessPlugin};

/// Environment marker exported to harnesses supervised by Agent Doc.
pub const AGENT_DOC_SESSION_ENV: &str = "AGENT_DOC_SESSION";

/// Agent Doc wins over ordinary harness plugins when its supervised session is active.
pub const AGENT_DOC_PLUGIN_PRIORITY: i32 = 100;

/// Agent Doc-owned `skill-harness` integration.
#[derive(Debug, Default, Clone, Copy)]
pub struct AgentDocSkillHarnessPlugin;

impl SkillHarnessPlugin for AgentDocSkillHarnessPlugin {
    fn id(&self) -> &'static str {
        "agent-doc"
    }

    fn priority(&self) -> i32 {
        AGENT_DOC_PLUGIN_PRIORITY
    }

    fn detect(&self, context: &PluginContext<'_>) -> bool {
        context.has_env_var(AGENT_DOC_SESSION_ENV)
    }

    fn skill_rel_path(&self, skill_name: &str) -> PathBuf {
        PathBuf::from(format!(".agent-doc/plugins/{skill_name}/SKILL.md"))
    }
}

/// Register Agent Doc after the generic registry's built-in harness plugins.
///
/// The higher priority makes Agent Doc authoritative when both
/// `AGENT_DOC_SESSION` and a nested harness marker such as `CODEX_CLI` are set.
pub fn register_agent_doc_plugin(registry: &mut PluginRegistry) {
    registry.register(AgentDocSkillHarnessPlugin);
}

/// Build the standard `skill-harness` registry with Agent Doc enabled.
pub fn registry_with_agent_doc() -> PluginRegistry {
    let mut registry = PluginRegistry::with_default_plugins();
    register_agent_doc_plugin(&mut registry);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use skill_harness::EnvironmentProvider;
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    #[derive(Default)]
    struct TestEnvironment {
        vars: BTreeMap<String, OsString>,
    }

    impl TestEnvironment {
        fn with(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), OsString::from(value));
            self
        }
    }

    impl EnvironmentProvider for TestEnvironment {
        fn var_os(&self, key: &str) -> Option<OsString> {
            self.vars.get(key).cloned()
        }
    }

    #[test]
    fn inactive_without_agent_doc_session_marker() {
        let env = TestEnvironment::default().with("CODEX_CLI", "1");
        let context = PluginContext::new(&env);
        let registry = registry_with_agent_doc();

        assert_eq!(registry.detect(&context).unwrap().id(), "codex");
    }

    #[test]
    fn agent_doc_overrides_nested_harness_and_resolves_owned_path() {
        let env = TestEnvironment::default()
            .with("CODEX_CLI", "1")
            .with(AGENT_DOC_SESSION_ENV, "session-id");
        let context = PluginContext::new(&env);
        let registry = registry_with_agent_doc();

        let plugin = registry.detect(&context).unwrap();
        assert_eq!(plugin.id(), "agent-doc");
        assert_eq!(
            plugin.skill_rel_path("compose-skills"),
            PathBuf::from(".agent-doc/plugins/compose-skills/SKILL.md")
        );

        let config = registry
            .skill_config("compose-skills", "content", "1.0.0", &context)
            .unwrap();
        assert_eq!(
            config.skill_path(None),
            PathBuf::from(".agent-doc/plugins/compose-skills/SKILL.md")
        );
    }
}
