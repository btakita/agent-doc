//! Online-learned typing-completion gate (`#steergateperceptron`).
//!
//! An online logistic regression over the decision features the gate already
//! computes ([`GateFeatures`], logged by `#steergatelog`). It implements the
//! existing [`CompletionClassifier`] trait, so it plugs into the one settle
//! decision ([`super::edit_settle::settle_decision`]) rather than forming a
//! parallel gate:
//!
//! - probability `>= 0.8` ([`SEND_THRESHOLD`]) answers `Complete` (send once
//!   half the quiet window has passed);
//! - probability `<= 0.3` ([`HOLD_THRESHOLD`]) answers `StillTyping` (hold);
//! - in between it has no verdict and the deterministic tier decides.
//!
//! The deterministic rules stay the hard floor inside `settle_decision`: an
//! unbalanced delimiter always holds, and the max-hold always delivers
//! (`possibly_partial`), whatever the model says.
//!
//! # Seed
//!
//! [`GateWeights::seeded`] reproduces the deterministic gate exactly. Two
//! conjunction features carry it: "complete-looking and quiet for half the
//! window" and "inconclusive and quiet for the full window" are precisely the
//! deterministic settle conditions. With bias `-2` and weight `+4` on each,
//! the model answers send (`p = 0.88`) exactly when the deterministic gate
//! settles and hold (`p <= 0.12`) exactly when it holds; every other weight
//! starts at zero. `seeded_weights_match_the_deterministic_gate_on_the_fixture_corpus`
//! proves the decisions are identical, so day one is a no-op.
//!
//! # Learning
//!
//! Labels from the decision log become targets: a premature delivery should
//! have been held (`0`), an on-time or late delivery sent (`1`), a hold that
//! turned out late should have sent (`1`), a hold the operator kept typing
//! through was right (`0`). Each label applies one stochastic gradient step on
//! the log loss. The harness is deliberately not a feature: the same document
//! state gets the same decision in every harness.

use super::edit_settle::{
    CompletionClassifier, CompletionSignal, CompletionVerdict, GateComponent, GateFeatures,
    TrailingToken,
};

/// Send when the model is at least this sure the operator finished.
pub const SEND_THRESHOLD: f64 = 0.8;
/// Hold when the model is at most this sure.
pub const HOLD_THRESHOLD: f64 = 0.3;
/// Step size of one online update.
pub const DEFAULT_LEARNING_RATE: f64 = 0.5;
/// Weights stay inside `[-WEIGHT_BOUND, WEIGHT_BOUND]`, so a burst of labels
/// cannot push the model somewhere a few corrections cannot bring it back.
pub const WEIGHT_BOUND: f64 = 8.0;

/// The model's input names, in weight order.
pub const FEATURE_NAMES: [&str; 21] = [
    "bias",
    "complete_quiet_half_window",
    "inconclusive_quiet_full_window",
    "signal_incomplete",
    "unbalanced_delimiters",
    "token_article",
    "token_conjunction",
    "token_preposition",
    "token_function_word",
    "token_question",
    "token_terminal",
    "token_id_ref",
    "token_url",
    "token_code_span",
    "token_dangling_punct",
    "closed_list_item",
    "pause_vs_median",
    "typing_speed",
    "item_age",
    "component_exchange",
    "quiet_vs_window",
];
pub const FEATURE_COUNT: usize = FEATURE_NAMES.len();

fn ratio(value: f64, cap: f64) -> f64 {
    (value / cap).clamp(0.0, 1.0)
}

/// The model's input vector for one decision.
pub fn feature_vector(f: &GateFeatures) -> [f64; FEATURE_COUNT] {
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    let token = |class: TrailingToken| flag(f.trailing_token == class);
    let quiet = f.quiet_ms;
    [
        1.0,
        flag(f.signal == CompletionSignal::Complete && quiet >= f.debounce_ms / 2),
        flag(f.signal == CompletionSignal::Inconclusive && quiet >= f.debounce_ms),
        flag(f.signal == CompletionSignal::Incomplete),
        flag(f.unbalanced_delimiters),
        token(TrailingToken::Article),
        token(TrailingToken::Conjunction),
        token(TrailingToken::Preposition),
        token(TrailingToken::FunctionWord),
        token(TrailingToken::Question),
        token(TrailingToken::Terminal),
        token(TrailingToken::IdRef),
        token(TrailingToken::Url),
        token(TrailingToken::CodeSpan),
        token(TrailingToken::DanglingPunct),
        flag(f.closed_list_item),
        f.pause_ratio().map_or(0.0, |r| ratio(r, 4.0)),
        f.typing_chars_per_min
            .map_or(0.0, |cpm| ratio(f64::from(cpm), 600.0)),
        ratio(f.item_age_ms as f64, 60_000.0),
        flag(f.component == GateComponent::Exchange),
        if f.debounce_ms == 0 {
            1.0
        } else {
            ratio(quiet as f64 / f.debounce_ms as f64, 4.0)
        },
    ]
}

/// The model's three-zone answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateZone {
    /// `p >= 0.8`: finished; send.
    Send,
    /// `p <= 0.3`: still typing; hold.
    Hold,
    /// In between: the deterministic tier decides.
    Defer,
}

impl GateZone {
    pub fn of(probability: f64) -> Self {
        if probability >= SEND_THRESHOLD {
            Self::Send
        } else if probability <= HOLD_THRESHOLD {
            Self::Hold
        } else {
            Self::Defer
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Hold => "hold",
            Self::Defer => "defer",
        }
    }

    /// The verdict this zone gives the settle decision.
    pub fn verdict(self) -> Option<CompletionVerdict> {
        match self {
            Self::Send => Some(CompletionVerdict::Complete),
            Self::Hold => Some(CompletionVerdict::StillTyping),
            Self::Defer => None,
        }
    }
}

/// One feature's share of a decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    pub feature: &'static str,
    pub value: f64,
    pub weight: f64,
    pub contribution: f64,
}

/// The learned weights for one operator in one project.
#[derive(Debug, Clone, PartialEq)]
pub struct GateWeights {
    /// One weight per [`FEATURE_NAMES`] entry.
    pub weights: Vec<f64>,
    /// Labelled updates applied since the seed.
    pub updates: u64,
}

impl Default for GateWeights {
    fn default() -> Self {
        Self::seeded()
    }
}

impl GateWeights {
    /// Weights that reproduce the deterministic gate exactly.
    pub fn seeded() -> Self {
        let mut weights = vec![0.0; FEATURE_COUNT];
        weights[0] = -2.0;
        weights[1] = 4.0;
        weights[2] = 4.0;
        weights[3] = -2.0;
        weights[4] = -4.0;
        Self {
            weights,
            updates: 0,
        }
    }

    /// Stored weights of the wrong shape (an older feature set) restart from
    /// the seed rather than misreading positions.
    pub fn normalized(self) -> Self {
        if self.weights.len() == FEATURE_COUNT && self.weights.iter().all(|w| w.is_finite()) {
            self
        } else {
            Self::seeded()
        }
    }

    pub fn score(&self, features: &GateFeatures) -> f64 {
        feature_vector(features)
            .iter()
            .zip(&self.weights)
            .map(|(x, w)| x * w)
            .sum()
    }

    pub fn probability(&self, features: &GateFeatures) -> f64 {
        1.0 / (1.0 + (-self.score(features)).exp())
    }

    pub fn zone(&self, features: &GateFeatures) -> GateZone {
        GateZone::of(self.probability(features))
    }

    /// Each feature's contribution `weight * value` to the score.
    pub fn contributions(&self, features: &GateFeatures) -> Vec<Contribution> {
        feature_vector(features)
            .iter()
            .zip(&self.weights)
            .zip(FEATURE_NAMES)
            .map(|((value, weight), feature)| Contribution {
                feature,
                value: *value,
                weight: *weight,
                contribution: value * weight,
            })
            .collect()
    }

    /// One online logistic-regression step toward `target` (1 = the operator
    /// had finished, 0 = still typing).
    pub fn update(&mut self, features: &GateFeatures, target: f64, learning_rate: f64) {
        let error = target - self.probability(features);
        for (weight, value) in self.weights.iter_mut().zip(feature_vector(features)) {
            *weight = (*weight + learning_rate * error * value).clamp(-WEIGHT_BOUND, WEIGHT_BOUND);
        }
        self.updates += 1;
    }
}

/// The learned gate as a [`CompletionClassifier`].
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedGate {
    pub weights: GateWeights,
}

impl LearnedGate {
    pub fn new(weights: GateWeights) -> Self {
        Self { weights }
    }
}

impl CompletionClassifier for LearnedGate {
    fn cached_verdict(&self, _content_hash: &str) -> Option<CompletionVerdict> {
        None
    }

    fn assess(&self, features: &GateFeatures, _content_hash: &str) -> Option<CompletionVerdict> {
        self.weights.zone(features).verdict()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_settle::{
        SettleDecision, SettleInputs, completion_signal, has_unbalanced_delimiters,
        settle_decision, trailing_token,
    };

    const CORPUS: &[&str] = &[
        "Should we release + publish the",
        "Should we release + publish the C++ bindings?",
        "do [#abc]",
        "see [#abc",
        "#subagent: https://github.com/btakita/agent-doc/issues/118",
        "fix the `foo",
        "use `foo()`",
        "rename (the helper",
        "say \"hello",
        "Fix the bug.",
        "Why does it hang?",
        "release + publish",
        "add retries to the uploader",
        "items:",
        "this and",
        "-",
        "",
        "```rust\nfn main() {",
        "```\ncode\n```",
    ];

    fn features_for(
        text: &str,
        inputs: &SettleInputs,
        closed: bool,
        median: Option<u64>,
        typing: Option<u32>,
        component: GateComponent,
    ) -> GateFeatures {
        GateFeatures {
            trailing_token: trailing_token(text),
            signal: inputs.signal,
            unbalanced_delimiters: inputs.unbalanced_delimiters,
            closed_list_item: closed,
            quiet_ms: inputs.quiet_for_ms.max(inputs.stable_for_ms).unwrap_or(0),
            median_pause_ms: median,
            typing_chars_per_min: typing,
            item_age_ms: inputs.held_for_ms,
            component,
            debounce_ms: inputs.debounce_ms,
            max_hold_ms: inputs.max_hold_ms,
        }
    }

    /// The day-one guarantee: over the fixture corpus and every timing and
    /// context combination, the seeded model's decisions (including when to
    /// re-check) are exactly the deterministic gate's.
    #[test]
    fn seeded_weights_match_the_deterministic_gate_on_the_fixture_corpus() {
        let gate = LearnedGate::new(GateWeights::seeded());
        let quiets = [
            0, 250, 999, 1000, 1001, 1249, 1250, 1999, 2000, 2001, 2500, 5000, 30_000, 44_999,
            45_000, 60_000,
        ];
        let mut compared = 0usize;
        let mut sends = 0usize;
        let mut holds = 0usize;
        for text in CORPUS {
            let signal = completion_signal(text);
            let unbalanced = has_unbalanced_delimiters(text);
            for &debounce_ms in &[0u64, 500, 2_000, 2_500] {
                for &max_hold_ms in &[45_000u64, 10_000, 1_000] {
                    for &quiet in &quiets {
                        for stable in [None, Some(0), Some(quiet)] {
                            for held in [0, quiet, 45_000] {
                                for (closed, median, typing, component) in [
                                    (false, None, None, GateComponent::Queue),
                                    (true, Some(800), Some(300), GateComponent::Exchange),
                                ] {
                                    let inputs = SettleInputs {
                                        quiet_for_ms: Some(quiet),
                                        stable_for_ms: stable,
                                        held_for_ms: held,
                                        debounce_ms,
                                        max_hold_ms,
                                        signal,
                                        unbalanced_delimiters: unbalanced,
                                        verdict: None,
                                    };
                                    let features = features_for(
                                        text, &inputs, closed, median, typing, component,
                                    );
                                    let verdict = gate.assess(&features, "hash");
                                    match verdict {
                                        Some(CompletionVerdict::Complete) => sends += 1,
                                        Some(_) => holds += 1,
                                        None => {}
                                    }
                                    let learned =
                                        settle_decision(SettleInputs { verdict, ..inputs });
                                    assert_eq!(
                                        learned,
                                        settle_decision(inputs),
                                        "{text:?} {inputs:?} {features:?}"
                                    );
                                    compared += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(compared > 50_000, "{compared}");
        // The seed is confident everywhere: it never defers, so the
        // equivalence is not the trivial "always defer" one.
        assert_eq!(sends + holds, compared);
        assert!(sends > 0 && holds > 0);
    }

    /// Hard floors: an unbalanced delimiter holds whatever the model says,
    /// and the max-hold delivers whatever the model says.
    #[test]
    fn deterministic_floors_override_the_model() {
        let inputs = SettleInputs {
            quiet_for_ms: Some(30_000),
            stable_for_ms: None,
            held_for_ms: 30_000,
            debounce_ms: 2_000,
            max_hold_ms: 45_000,
            signal: CompletionSignal::Incomplete,
            unbalanced_delimiters: true,
            verdict: Some(CompletionVerdict::Complete),
        };
        assert!(!settle_decision(inputs).deliver());
        let expired = SettleInputs {
            quiet_for_ms: Some(45_000),
            held_for_ms: 45_000,
            signal: CompletionSignal::Complete,
            unbalanced_delimiters: false,
            verdict: Some(CompletionVerdict::StillTyping),
            ..inputs
        };
        assert_eq!(settle_decision(expired), SettleDecision::MaxHoldExpired);
    }

    type UpdateRule = fn(&mut GateWeights, &GateFeatures, f64);

    fn real_rule(weights: &mut GateWeights, f: &GateFeatures, target: f64) {
        weights.update(f, target, DEFAULT_LEARNING_RATE);
    }

    fn premature_prone(i: u64) -> GateFeatures {
        // Inconclusive text delivered at the full window while the operator
        // was mid-thought (short pause relative to their median).
        GateFeatures {
            trailing_token: TrailingToken::Word,
            signal: CompletionSignal::Inconclusive,
            unbalanced_delimiters: false,
            closed_list_item: false,
            quiet_ms: 2_000 + i * 10,
            median_pause_ms: Some(6_000),
            typing_chars_per_min: Some(240),
            item_age_ms: 9_000,
            component: GateComponent::Queue,
            debounce_ms: 2_000,
            max_hold_ms: 45_000,
        }
    }

    fn on_time(i: u64) -> GateFeatures {
        GateFeatures {
            trailing_token: TrailingToken::Question,
            signal: CompletionSignal::Complete,
            quiet_ms: 1_000 + i * 10,
            ..premature_prone(i)
        }
    }

    /// Replays the labelled corrections through `rule`; returns how many
    /// premature-prone items the model still sends, and whether on-time
    /// items still send.
    fn learn(rule: UpdateRule) -> (usize, bool, GateWeights) {
        let mut weights = GateWeights::seeded();
        for round in 0..20 {
            for i in 0..5 {
                // Delivered and re-edited inside the window: premature (0).
                rule(&mut weights, &premature_prone(i + round), 0.0);
                // Delivered and left alone: on time (1).
                rule(&mut weights, &on_time(i + round), 1.0);
            }
        }
        let still_sent = (0..10)
            .filter(|i| weights.zone(&premature_prone(*i)) == GateZone::Send)
            .count();
        let on_time_sent = (0..10).all(|i| weights.zone(&on_time(i)) == GateZone::Send);
        (still_sent, on_time_sent, weights)
    }

    fn learns_fewer_premature_sends(rule: UpdateRule) -> bool {
        let seeded = GateWeights::seeded();
        let before = (0..10)
            .filter(|i| seeded.zone(&premature_prone(*i)) == GateZone::Send)
            .count();
        let (after, on_time_sent, weights) = learn(rule);
        before == 10
            && after == 0
            && on_time_sent
            && (0..10).all(|i| weights.zone(&premature_prone(i)) == GateZone::Hold)
    }

    /// Labelled premature deliveries move the weights toward holding that
    /// shape (into the hold zone, not merely to "defer"), while on-time
    /// deliveries keep sending.
    #[test]
    fn labelled_corrections_move_weights_toward_fewer_premature_sends() {
        let seeded = GateWeights::seeded();
        let (_, _, learned) = learn(real_rule);
        assert!(learns_fewer_premature_sends(real_rule));
        assert!(learned.updates == 200, "{}", learned.updates);
        let names = |name: &str| FEATURE_NAMES.iter().position(|n| *n == name).unwrap();
        let idx = names("inconclusive_quiet_full_window");
        assert!(
            learned.weights[idx] < seeded.weights[idx],
            "{:?}",
            learned.weights
        );
        assert!(learned.weights[names("token_question")] > 0.0);
    }

    /// Mutation check: the learning test must fail for a broken update rule.
    #[test]
    fn the_learning_test_catches_a_broken_update_rule() {
        fn sign_flipped(w: &mut GateWeights, f: &GateFeatures, target: f64) {
            let error = target - w.probability(f);
            for (weight, value) in w.weights.iter_mut().zip(feature_vector(f)) {
                *weight = (*weight - DEFAULT_LEARNING_RATE * error * value)
                    .clamp(-WEIGHT_BOUND, WEIGHT_BOUND);
            }
        }
        fn ignores_target(w: &mut GateWeights, f: &GateFeatures, _target: f64) {
            w.update(f, 1.0, DEFAULT_LEARNING_RATE);
        }
        fn no_step(w: &mut GateWeights, f: &GateFeatures, target: f64) {
            w.update(f, target, 0.0);
        }
        fn inverted_target(w: &mut GateWeights, f: &GateFeatures, target: f64) {
            w.update(f, 1.0 - target, DEFAULT_LEARNING_RATE);
        }
        for (name, rule) in [
            ("sign_flipped", sign_flipped as UpdateRule),
            ("ignores_target", ignores_target),
            ("no_step", no_step),
            ("inverted_target", inverted_target),
        ] {
            assert!(
                !learns_fewer_premature_sends(rule),
                "mutant {name} survived"
            );
        }
        assert!(learns_fewer_premature_sends(real_rule));
    }

    #[test]
    fn contributions_explain_the_score() {
        let weights = GateWeights::seeded();
        let f = on_time(0);
        let contributions = weights.contributions(&f);
        assert_eq!(contributions.len(), FEATURE_COUNT);
        let total: f64 = contributions.iter().map(|c| c.contribution).sum();
        assert!((total - weights.score(&f)).abs() < 1e-9);
        assert_eq!(weights.zone(&f), GateZone::Send);
        let wrong_shape = GateWeights {
            weights: vec![1.0; 3],
            updates: 7,
        };
        assert_eq!(wrong_shape.normalized(), GateWeights::seeded());
    }
}
