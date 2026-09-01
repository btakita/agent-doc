//! Reactive harness authority for an existing supervisor session.
//!
//! A missing or temporarily unreadable `agent:` projection is absence of an
//! override, not an instruction to replace the running actor with the global or
//! built-in default.  Existing-session resolution therefore keeps the active
//! actor between the explicit document override and the configured defaults.

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, Source, ThreadSafeContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessAuthoritySource {
    Document,
    ActiveActor,
    ConfiguredDefault,
    BuiltinDefault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessAuthorityFacts {
    pub declared_document_agent: Option<String>,
    pub active_actor_harness: Option<String>,
    pub configured_default_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessAuthoritySelection {
    pub agent: String,
    pub source: HarnessAuthoritySource,
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn resolve_harness_authority(facts: &HarnessAuthorityFacts) -> HarnessAuthoritySelection {
    let (agent, source) = if let Some(agent) = nonempty(facts.declared_document_agent.as_deref()) {
        (agent, HarnessAuthoritySource::Document)
    } else if let Some(agent) = nonempty(facts.active_actor_harness.as_deref()) {
        (agent, HarnessAuthoritySource::ActiveActor)
    } else if let Some(agent) = nonempty(facts.configured_default_agent.as_deref()) {
        (agent, HarnessAuthoritySource::ConfiguredDefault)
    } else {
        ("claude", HarnessAuthoritySource::BuiltinDefault)
    };
    HarnessAuthoritySelection {
        agent: agent.to_string(),
        source,
    }
}

/// Process-lifetime reactive authority used by restart and idle-watch paths.
/// Updating an unchanged observation is inert in Lazily; an explicit document
/// edge overrides the active actor, while an absent edge keeps that actor.
pub struct HarnessAuthorityState {
    ctx: ThreadSafeContext,
    facts: Source<HarnessAuthorityFacts>,
    selection: Computed<HarnessAuthoritySelection>,
}

impl HarnessAuthorityState {
    pub fn new_in(
        scope: &ProcessScope,
        active_actor_harness: Option<String>,
        configured_default_agent: Option<String>,
    ) -> Self {
        let ctx = scope.ctx().clone();
        let facts = ctx.source(HarnessAuthorityFacts {
            declared_document_agent: None,
            active_actor_harness,
            configured_default_agent,
        });
        let facts_for_selection = facts;
        let selection =
            ctx.computed(move |ctx| resolve_harness_authority(&ctx.get(&facts_for_selection)));
        Self {
            ctx,
            facts,
            selection,
        }
    }

    pub fn resolve_document_agent(
        &self,
        declared_document_agent: Option<&str>,
        configured_default_agent: Option<&str>,
    ) -> HarnessAuthoritySelection {
        let mut facts = self.ctx.get(&self.facts);
        facts.declared_document_agent = declared_document_agent.map(str::to_string);
        facts.configured_default_agent = configured_default_agent.map(str::to_string);
        self.ctx.set(&self.facts, facts);
        self.ctx.get(&self.selection)
    }

    pub fn set_active_actor_harness(&self, active_actor_harness: &str) {
        let mut facts = self.ctx.get(&self.facts);
        facts.active_actor_harness = Some(active_actor_harness.to_string());
        facts.declared_document_agent = None;
        self.ctx.set(&self.facts, facts);
    }

    pub fn current(&self) -> HarnessAuthoritySelection {
        self.resolve_document_agent(None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        declared: Option<&str>,
        active: Option<&str>,
        configured: Option<&str>,
    ) -> HarnessAuthorityFacts {
        HarnessAuthorityFacts {
            declared_document_agent: declared.map(str::to_string),
            active_actor_harness: active.map(str::to_string),
            configured_default_agent: configured.map(str::to_string),
        }
    }

    #[test]
    fn missing_document_agent_keeps_active_codex_actor() {
        assert_eq!(
            resolve_harness_authority(&facts(None, Some("codex"), Some("claude"))),
            HarnessAuthoritySelection {
                agent: "codex".to_string(),
                source: HarnessAuthoritySource::ActiveActor,
            }
        );
    }

    #[test]
    fn explicit_document_agent_overrides_active_actor() {
        assert_eq!(
            resolve_harness_authority(&facts(Some("claude"), Some("codex"), None)),
            HarnessAuthoritySelection {
                agent: "claude".to_string(),
                source: HarnessAuthoritySource::Document,
            }
        );
    }

    #[test]
    fn no_actor_uses_configured_then_builtin_default() {
        assert_eq!(
            resolve_harness_authority(&facts(None, None, Some("opencode"))).agent,
            "opencode"
        );
        assert_eq!(
            resolve_harness_authority(&facts(None, None, None)).agent,
            "claude"
        );
    }

    #[test]
    fn repeated_absent_observation_is_stable_and_explicit_edge_rearms() {
        let scope = ProcessScope::new();
        let state = HarnessAuthorityState::new_in(&scope, Some("codex".into()), None);
        let first = state.resolve_document_agent(None, None);
        let second = state.resolve_document_agent(None, None);
        assert_eq!(first, second);
        assert_eq!(second.source, HarnessAuthoritySource::ActiveActor);

        assert_eq!(
            state.resolve_document_agent(Some("claude"), None).source,
            HarnessAuthoritySource::Document
        );
        state.set_active_actor_harness("claude");
        assert_eq!(state.current().agent, "claude");
        assert_eq!(state.current().source, HarnessAuthoritySource::ActiveActor);
    }
}
