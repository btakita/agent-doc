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
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "default")
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

/// Whether the document authority a Restart Agent will re-read has caught up to
/// the `agent:` value the operator sees (`#restartagentharness`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartAgentFrontmatterAgreement {
    /// The authority declares the same harness the editor shows (or the caller
    /// supplied no explicit editor-visible value), so the supervisor's re-read
    /// resolves what the operator asked for.
    Agrees,
    /// The editor shows `editor_agent` but the authority still declares
    /// `authority_agent` (`None` = no `agent:` key). Restarting now would
    /// relaunch from frontmatter the operator already changed.
    AuthorityLagsEditor {
        editor_agent: String,
        authority_agent: Option<String>,
    },
}

/// Compare the editor-visible `agent:` against the authority's declared one.
///
/// Restart Agent re-resolves the harness from the controller's document
/// authority, never from the editor buffer. When the editor's edits have not
/// reached that authority (a debounce race, or a fenced/quarantined editor
/// replica), the re-read silently relaunches the OLD harness: on 2026-10-27 the
/// operator changed `lazily.md` to `agent: grok`, and five Restart Agent
/// invocations each relaunched Claude Code because the authority still read
/// `agent: claude`. Both sides are compared as *declared* values after
/// [`agent_doc_harness::normalize_harness_name`] (so `claude`/`claude-code` and
/// `grok`/`grok-build` agree), not as resolved binaries, because the question is
/// whether the authority has seen the operator's edit — an unknown name must
/// not agree with `claude` just because both would fall back to it.
pub fn restart_agent_frontmatter_agreement(
    editor_agent: Option<&str>,
    authority_agent: Option<&str>,
) -> RestartAgentFrontmatterAgreement {
    let normalize = |value: &str| {
        let value = agent_doc_harness::normalize_harness_name(value);
        (value != "default").then_some(value)
    };
    let Some(editor) = editor_agent.and_then(normalize) else {
        return RestartAgentFrontmatterAgreement::Agrees;
    };
    let authority = authority_agent.and_then(normalize);
    if authority.as_deref() == Some(editor.as_str()) {
        RestartAgentFrontmatterAgreement::Agrees
    } else {
        RestartAgentFrontmatterAgreement::AuthorityLagsEditor {
            editor_agent: editor,
            authority_agent: authority,
        }
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
    fn default_actor_sentinel_does_not_override_configured_harness() {
        assert_eq!(
            resolve_harness_authority(&facts(None, Some("default"), Some("codex"))),
            HarnessAuthoritySelection {
                agent: "codex".to_string(),
                source: HarnessAuthoritySource::ConfiguredDefault,
            }
        );
    }

    #[test]
    fn restart_agent_agreement_refuses_an_authority_that_lags_the_editor_agent() {
        // The 2026-10-27 lazily.md shape: editor shows grok, authority still claude.
        assert_eq!(
            restart_agent_frontmatter_agreement(Some("grok"), Some("claude-code")),
            RestartAgentFrontmatterAgreement::AuthorityLagsEditor {
                editor_agent: "grok".to_string(),
                authority_agent: Some("claude-code".to_string()),
            }
        );
        assert_eq!(
            restart_agent_frontmatter_agreement(Some("codex"), None),
            RestartAgentFrontmatterAgreement::AuthorityLagsEditor {
                editor_agent: "codex".to_string(),
                authority_agent: None,
            }
        );
        // An unknown name resolves to claude at launch, but it is still a
        // different declared value: the authority has not seen the edit.
        assert!(matches!(
            restart_agent_frontmatter_agreement(Some("groc"), Some("claude")),
            RestartAgentFrontmatterAgreement::AuthorityLagsEditor { .. }
        ));
    }

    #[test]
    fn restart_agent_agreement_accepts_aliases_and_absent_editor_evidence() {
        for (editor, authority) in [
            (Some("grok"), Some("grok")),
            (Some("grok-build"), Some("grok")),
            (Some("claude"), Some("claude-code")),
            (Some(" codex "), Some("codex")),
            (None, Some("claude-code")),
            (None, None),
            (Some(""), Some("codex")),
            (Some("default"), None),
        ] {
            assert_eq!(
                restart_agent_frontmatter_agreement(editor, authority),
                RestartAgentFrontmatterAgreement::Agrees,
                "editor={editor:?} authority={authority:?}",
            );
        }
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
