//! Reactive preflight reads and signal-gated effects (`#preflightreactive`).
//!
//! Preflight used to encode its transaction in the lexical order of one large
//! function: repair happened to precede commit, queue convergence happened to
//! precede the baseline checkpoint, and cycle-open happened to be below both.
//! A caller or refactor could skip or move any one of those calls without the
//! type system or runtime noticing.
//!
//! This module separates the two kinds of state:
//!
//! - [`PreflightReadState`] is the document-scoped [`lazily::Computed`] the
//!   preflight CLI refreshes through the project controller. The CLI reads one
//!   projection instead of independently assembling diff, queue, tier,
//!   claims, related-document, and accretion values.
//! - [`PreflightEffectState`] derives the only effect that is currently ready
//!   from explicit signals. Calling an effect out of order is an error. Adding
//!   a new effect requires handling the exhaustive [`PreflightEffect`] match,
//!   so insertion cannot silently change the ordering contract.
//!
//! Cycle-open deliberately remains an effect. Its durable `cycle-<id>` identity
//! is not derivable state and is therefore represented only as the final gated
//! transition.

use std::error::Error;
use std::fmt;

use lazily::{Computed, Source, ThreadSafeContext};
use serde::{Deserialize, Serialize};

use crate::DocumentScope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightQueueProjection {
    pub prompts: Vec<String>,
    pub selected_prompts: Vec<String>,
    pub active: Option<bool>,
    pub deferred: bool,
    pub start_at: Option<String>,
    pub trigger: Option<serde_json::Value>,
    pub halted: Option<String>,
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub drainable_head_count: usize,
    pub continuation_required: bool,
    pub continuation_guidance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightTierProjection {
    pub effective: Option<String>,
    pub required: Option<String>,
    pub suggested: Option<String>,
    pub agent_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightRelatedDocumentProjection {
    pub path: String,
    pub summary: String,
    pub exists: bool,
}

/// The preflight CLI publishes observations here through the controller; the
/// graph derives the public read projection from this one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightReadFacts {
    /// Hashes are provenance, not authority. They make the exact document,
    /// baseline, and config cut carried by this projection observable.
    pub document_hash: String,
    pub baseline_hash: String,
    pub config_hash: String,
    pub diff: Option<String>,
    pub queue: PreflightQueueProjection,
    pub tiers: PreflightTierProjection,
    pub claims: Vec<String>,
    pub related_documents: Vec<PreflightRelatedDocumentProjection>,
    pub session_accretion: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightReadProjection {
    pub document_hash: String,
    pub baseline_hash: String,
    pub config_hash: String,
    pub current: bool,
    pub diff: Option<String>,
    pub queue: PreflightQueueProjection,
    pub tiers: PreflightTierProjection,
    pub claims: Vec<String>,
    pub related_documents: Vec<PreflightRelatedDocumentProjection>,
    pub session_accretion: Option<serde_json::Value>,
}

pub fn derive_read_projection(facts: &PreflightReadFacts) -> PreflightReadProjection {
    PreflightReadProjection {
        document_hash: facts.document_hash.clone(),
        baseline_hash: facts.baseline_hash.clone(),
        config_hash: facts.config_hash.clone(),
        current: !facts.document_hash.is_empty()
            && !facts.baseline_hash.is_empty()
            && !facts.config_hash.is_empty(),
        diff: facts.diff.clone(),
        queue: facts.queue.clone(),
        tiers: facts.tiers.clone(),
        claims: facts.claims.clone(),
        related_documents: facts.related_documents.clone(),
        session_accretion: facts.session_accretion.clone(),
    }
}

/// Document-scoped, continuously invalidated preflight read projection.
pub struct PreflightReadState {
    ctx: ThreadSafeContext,
    facts: Source<PreflightReadFacts>,
    projection: Computed<PreflightReadProjection>,
}

impl PreflightReadState {
    pub fn new_in(scope: &DocumentScope) -> Self {
        let ctx = scope.ctx().clone();
        let facts = ctx.source(PreflightReadFacts::default());
        let projection = ctx.computed(move |c| derive_read_projection(&c.get(&facts)));
        Self {
            ctx,
            facts,
            projection,
        }
    }

    pub fn observe(&self, facts: PreflightReadFacts) {
        self.ctx.set(&self.facts, facts);
    }

    pub fn projection(&self) -> PreflightReadProjection {
        self.ctx.get(&self.projection)
    }

    pub fn projection_cell(&self) -> &Computed<PreflightReadProjection> {
        &self.projection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightEffect {
    Repair,
    PriorCycleCommit,
    BaselineCheckpoint,
    CycleOpen,
}

impl fmt::Display for PreflightEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token = match self {
            Self::Repair => "repair",
            Self::PriorCycleCommit => "prior_cycle_commit",
            Self::BaselineCheckpoint => "baseline_checkpoint",
            Self::CycleOpen => "cycle_open",
        };
        f.write_str(token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightEffectSignals {
    pub authority_current: bool,
    pub repair_settled: bool,
    pub prior_cycle_commit_settled: bool,
    pub derived_reads_current: bool,
    pub baseline_checkpoint_settled: bool,
    pub cycle_open_required: bool,
    pub cycle_opened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PreflightEffectProjection {
    pub ready_effect: Option<PreflightEffect>,
    pub waiting_for: Option<String>,
    pub complete: bool,
}

pub fn derive_effect_projection(signals: &PreflightEffectSignals) -> PreflightEffectProjection {
    if !signals.authority_current {
        return PreflightEffectProjection {
            waiting_for: Some("authority_current".to_string()),
            ..PreflightEffectProjection::default()
        };
    }
    if !signals.repair_settled {
        return PreflightEffectProjection {
            ready_effect: Some(PreflightEffect::Repair),
            ..PreflightEffectProjection::default()
        };
    }
    if !signals.prior_cycle_commit_settled {
        return PreflightEffectProjection {
            ready_effect: Some(PreflightEffect::PriorCycleCommit),
            ..PreflightEffectProjection::default()
        };
    }
    if !signals.derived_reads_current {
        return PreflightEffectProjection {
            waiting_for: Some("derived_reads_current".to_string()),
            ..PreflightEffectProjection::default()
        };
    }
    if !signals.baseline_checkpoint_settled {
        return PreflightEffectProjection {
            ready_effect: Some(PreflightEffect::BaselineCheckpoint),
            ..PreflightEffectProjection::default()
        };
    }
    if signals.cycle_open_required && !signals.cycle_opened {
        return PreflightEffectProjection {
            ready_effect: Some(PreflightEffect::CycleOpen),
            ..PreflightEffectProjection::default()
        };
    }
    PreflightEffectProjection {
        complete: true,
        ..PreflightEffectProjection::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightEffectGateError {
    attempted: Option<PreflightEffect>,
    projection: PreflightEffectProjection,
}

impl fmt::Display for PreflightEffectGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.attempted, self.projection.ready_effect) {
            (Some(attempted), Some(ready)) => {
                write!(
                    f,
                    "preflight effect `{attempted}` is not ready; `{ready}` is ready"
                )
            }
            (Some(attempted), None) => write!(
                f,
                "preflight effect `{attempted}` is not ready; waiting_for={}",
                self.projection.waiting_for.as_deref().unwrap_or("terminal")
            ),
            (None, Some(ready)) => {
                write!(f, "preflight effects are incomplete; `{ready}` is ready")
            }
            (None, None) => write!(
                f,
                "preflight effects are incomplete; waiting_for={}",
                self.projection.waiting_for.as_deref().unwrap_or("terminal")
            ),
        }
    }
}

impl Error for PreflightEffectGateError {}

/// The effect coordinator is a Computed over signals, not an imperative phase
/// counter. Every mutation re-derives the ready effect.
pub struct PreflightEffectState {
    ctx: ThreadSafeContext,
    signals: Source<PreflightEffectSignals>,
    projection: Computed<PreflightEffectProjection>,
}

impl PreflightEffectState {
    pub fn new_in(scope: &DocumentScope) -> Self {
        let ctx = scope.ctx().clone();
        let signals = ctx.source(PreflightEffectSignals::default());
        let projection = ctx.computed(move |c| derive_effect_projection(&c.get(&signals)));
        Self {
            ctx,
            signals,
            projection,
        }
    }

    pub fn observe_authority_current(&self, current: bool) {
        self.update(|signals| signals.authority_current = current);
    }

    pub fn observe_derived_reads_current(&self, current: bool) {
        self.update(|signals| signals.derived_reads_current = current);
    }

    pub fn observe_cycle_open_required(&self, required: bool) {
        self.update(|signals| signals.cycle_open_required = required);
    }

    pub fn projection(&self) -> PreflightEffectProjection {
        self.ctx.get(&self.projection)
    }

    pub fn require(&self, effect: PreflightEffect) -> Result<(), PreflightEffectGateError> {
        let projection = self.projection();
        if projection.ready_effect == Some(effect) {
            Ok(())
        } else {
            Err(PreflightEffectGateError {
                attempted: Some(effect),
                projection,
            })
        }
    }

    pub fn settle(&self, effect: PreflightEffect) -> Result<(), PreflightEffectGateError> {
        self.require(effect)?;
        self.update(|signals| match effect {
            PreflightEffect::Repair => signals.repair_settled = true,
            PreflightEffect::PriorCycleCommit => {
                signals.prior_cycle_commit_settled = true;
            }
            PreflightEffect::BaselineCheckpoint => {
                signals.baseline_checkpoint_settled = true;
            }
            PreflightEffect::CycleOpen => signals.cycle_opened = true,
        });
        Ok(())
    }

    pub fn require_complete(&self) -> Result<(), PreflightEffectGateError> {
        let projection = self.projection();
        if projection.complete {
            Ok(())
        } else {
            Err(PreflightEffectGateError {
                attempted: None,
                projection,
            })
        }
    }

    fn update(&self, mutate: impl FnOnce(&mut PreflightEffectSignals)) {
        let mut signals = self.ctx.get(&self.signals);
        mutate(&mut signals);
        self.ctx.set(&self.signals, signals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_projection_recomputes_when_observations_change() {
        let scope = DocumentScope::new();
        let state = PreflightReadState::new_in(&scope);
        assert!(!state.projection().current);

        state.observe(PreflightReadFacts {
            document_hash: "doc-a".to_string(),
            baseline_hash: "base-a".to_string(),
            config_hash: "config-a".to_string(),
            diff: Some("+prompt a".to_string()),
            ..PreflightReadFacts::default()
        });
        assert_eq!(state.projection().diff.as_deref(), Some("+prompt a"));

        state.observe(PreflightReadFacts {
            document_hash: "doc-b".to_string(),
            baseline_hash: "base-a".to_string(),
            config_hash: "config-a".to_string(),
            diff: Some("+prompt b".to_string()),
            ..PreflightReadFacts::default()
        });
        let projection = state.projection();
        assert!(projection.current);
        assert_eq!(projection.document_hash, "doc-b");
        assert_eq!(projection.diff.as_deref(), Some("+prompt b"));
    }

    #[test]
    fn no_effect_can_skip_its_derived_dependency() {
        let scope = DocumentScope::new();
        let effects = PreflightEffectState::new_in(&scope);
        effects.observe_authority_current(true);

        let error = effects
            .settle(PreflightEffect::PriorCycleCommit)
            .expect_err("commit must not skip repair");
        assert!(error.to_string().contains("`repair` is ready"));

        effects.settle(PreflightEffect::Repair).unwrap();
        effects.settle(PreflightEffect::PriorCycleCommit).unwrap();
        assert_eq!(
            effects.projection().waiting_for.as_deref(),
            Some("derived_reads_current")
        );

        let error = effects
            .settle(PreflightEffect::CycleOpen)
            .expect_err("cycle-open must wait for derived reads and baseline");
        assert!(
            error
                .to_string()
                .contains("waiting_for=derived_reads_current")
        );
    }

    #[test]
    fn cycle_open_is_exactly_once_and_only_after_checkpoint() {
        let scope = DocumentScope::new();
        let effects = PreflightEffectState::new_in(&scope);
        effects.observe_authority_current(true);
        effects.settle(PreflightEffect::Repair).unwrap();
        effects.settle(PreflightEffect::PriorCycleCommit).unwrap();
        effects.observe_derived_reads_current(true);
        effects.observe_cycle_open_required(true);

        let error = effects
            .settle(PreflightEffect::CycleOpen)
            .expect_err("cycle-open must not skip baseline checkpoint");
        assert!(error.to_string().contains("`baseline_checkpoint` is ready"));

        effects.settle(PreflightEffect::BaselineCheckpoint).unwrap();
        effects.settle(PreflightEffect::CycleOpen).unwrap();
        effects.require_complete().unwrap();

        assert!(
            effects.settle(PreflightEffect::CycleOpen).is_err(),
            "a durable cycle identity must not be opened twice"
        );
    }

    #[test]
    fn no_cycle_needed_finishes_at_the_checkpoint_signal() {
        let scope = DocumentScope::new();
        let effects = PreflightEffectState::new_in(&scope);
        effects.observe_authority_current(true);
        effects.settle(PreflightEffect::Repair).unwrap();
        effects.settle(PreflightEffect::PriorCycleCommit).unwrap();
        effects.observe_derived_reads_current(true);
        effects.observe_cycle_open_required(false);
        effects.settle(PreflightEffect::BaselineCheckpoint).unwrap();

        effects.require_complete().unwrap();
        assert_eq!(effects.projection().ready_effect, None);
    }
}
