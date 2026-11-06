//! Shared queue-prefix configuration and process-wide resolved palette.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

/// Optional global overrides for visible queue projection prefixes.
///
/// Values are strings rather than characters so operators can use ASCII words,
/// bracketed labels, or any other multi-character prefix that renders clearly in
/// their editor. Omitted fields retain agent-doc's documented defaults.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueIconConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_claimed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_running: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_reported: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciling: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prioritized: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_prioritized: Option<String>,
}

/// Effective process-wide queue prefix palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueIconSet {
    pub pending: String,
    pub blocked: String,
    pub claimed: String,
    pub running: String,
    pub reported: String,
    pub lost: String,
    pub lost_claimed: String,
    pub lost_running: String,
    pub lost_reported: String,
    pub reconciling: String,
    pub unknown: String,
    pub skipped: String,
    pub awaiting_operator: String,
    pub operator_action: String,
    pub prioritized: String,
    pub agent_prioritized: String,
}

/// Binary-owned visible lifecycle states shared by queue rows and their linked
/// tracked-work rows. These prefixes are projections only; durable queue state
/// remains authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueueLifecycleMarker {
    Pending,
    /// `#queueblocked`: a live row whose tracked item still waits on open
    /// prerequisite work (a `(stays open: blocked behind #a, #b)` clause or an
    /// `after=#id` edge naming an item still open in backlog, todo or review).
    /// It is held out of drain and dispatch until every blocker closes.
    Blocked,
    Claimed,
    Running,
    Reported,
    Lost,
    /// The attempt was lost while it was still claimed/dispatching.
    LostClaimed,
    /// The attempt was lost after a concrete worker had started.
    LostRunning,
    /// The attempt was lost, but its exact fenced completion outbox was later
    /// recovered. Both facts remain visible while coordinator closeout is due.
    LostReported,
    Reconciling,
    Unknown,
    Skipped,
    AwaitingOperator,
}

pub const ALL_LIFECYCLE_STATES: [QueueLifecycleMarker; 13] = [
    QueueLifecycleMarker::Pending,
    QueueLifecycleMarker::Blocked,
    QueueLifecycleMarker::Claimed,
    QueueLifecycleMarker::Running,
    QueueLifecycleMarker::Reported,
    QueueLifecycleMarker::Lost,
    QueueLifecycleMarker::LostClaimed,
    QueueLifecycleMarker::LostRunning,
    QueueLifecycleMarker::LostReported,
    QueueLifecycleMarker::Reconciling,
    QueueLifecycleMarker::Unknown,
    QueueLifecycleMarker::Skipped,
    QueueLifecycleMarker::AwaitingOperator,
];

impl QueueLifecycleMarker {
    pub fn prefix(self) -> &'static str {
        let icons = queue_icons();
        match self {
            Self::Pending => &icons.pending,
            Self::Blocked => &icons.blocked,
            Self::Claimed => &icons.claimed,
            Self::Running => &icons.running,
            Self::Reported => &icons.reported,
            Self::Lost => &icons.lost,
            Self::LostClaimed => &icons.lost_claimed,
            Self::LostRunning => &icons.lost_running,
            Self::LostReported => &icons.lost_reported,
            Self::Reconciling => &icons.reconciling,
            Self::Unknown => &icons.unknown,
            Self::Skipped => &icons.skipped,
            Self::AwaitingOperator => &icons.awaiting_operator,
        }
    }

    pub const fn default_prefix(self) -> &'static str {
        match self {
            Self::Pending => "📥",
            Self::Blocked => "⛔",
            Self::Claimed => "🟡",
            Self::Running => "🚧",
            Self::Reported => "☑️",
            Self::Lost => "⚠️",
            Self::LostClaimed => "⚠️🟡",
            Self::LostRunning => "⚠️🚧",
            Self::LostReported => "⚠️☑️",
            Self::Reconciling => "🔄",
            Self::Unknown => "❔",
            Self::Skipped => "⏭️",
            Self::AwaitingOperator => "⏳",
        }
    }

    /// The words agent-doc itself uses to name this state: its
    /// `[queue_icons]` key, the legend label, and their hyphenated spellings
    /// (`lost_running`, `lost + running`, `lost-running`). These are binary
    /// vocabulary for the state, never queue work text (`#lifecyclewordfusion`).
    pub const fn state_words(self) -> &'static [&'static str] {
        match self {
            Self::Pending => &["pending"],
            Self::Blocked => &["blocked"],
            Self::Claimed => &["claimed"],
            Self::Running => &["running"],
            Self::Reported => &["reported"],
            Self::Lost => &["lost"],
            Self::LostClaimed => &["lost_claimed", "lost-claimed", "lost+claimed"],
            Self::LostRunning => &["lost_running", "lost-running", "lost+running"],
            Self::LostReported => &["lost_reported", "lost-reported", "lost+reported"],
            Self::Reconciling => &["reconciling"],
            Self::Unknown => &["unknown"],
            Self::Skipped => &["skipped"],
            Self::AwaitingOperator => &["awaiting_operator", "awaiting-operator"],
        }
    }
}

/// Former reported spellings still accepted on rendered rows.
const LEGACY_REPORTED_PREFIXES: [&str; 2] = ["📫", "📤"];

/// `#lifecyclewordfusion`: a leading lifecycle state word fused to a lifecycle
/// glyph, `running🚧`, is a torn projection token, never work text.
///
/// haiven-dev `sdk-dev.md` (2026-11-03) carried `📥 running🚧 do [#q-…]` after
/// a dispatch: the state's name had been written into the row in front of its
/// glyph. Because no parser read `running🚧` as projection, it became part of
/// the row's work text and nest vector, `queue report-complete` found no claim
/// for the exact running attempt, and the closeout struck the row by its echo
/// text instead. Reading the fused token as one lifecycle token lets every
/// projection pass repaint the row canonically (one glyph, no word) and keeps
/// identity, claim matching and strikes on the real work text.
///
/// Recognition is exact: one of the binary's own state words
/// ([`QueueLifecycleMarker::state_words`], ASCII case-insensitive) immediately
/// followed, with no whitespace, by a lifecycle glyph (configured or built-in).
/// The glyph names the state; the word is discarded. Operator prose such as
/// `running tests` never matches because no glyph follows the word.
///
/// Returns the glyph's state and the byte length of the whole fused token.
pub fn fused_lifecycle_word_token(text: &str) -> Option<(QueueLifecycleMarker, usize)> {
    let word_len = ALL_LIFECYCLE_STATES
        .iter()
        .flat_map(|state| state.state_words().iter())
        .filter(|word| {
            text.len() >= word.len()
                && text.is_char_boundary(word.len())
                && text[..word.len()].eq_ignore_ascii_case(word)
        })
        .map(|word| word.len())
        .max()?;
    let after_word = &text[word_len..];
    let (state, glyph_len) = ALL_LIFECYCLE_STATES
        .iter()
        .flat_map(|state| {
            let legacy: &[&str] = if *state == QueueLifecycleMarker::Reported {
                &LEGACY_REPORTED_PREFIXES
            } else {
                &[]
            };
            [state.prefix(), state.default_prefix()]
                .into_iter()
                .chain(legacy.iter().copied())
                .map(move |glyph| (*state, glyph))
        })
        // A configured ASCII-word palette entry is not a glyph; only a prefix
        // that does not itself start with a letter can be fused to a word.
        .filter(|(_, glyph)| {
            !glyph.is_empty() && !glyph.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        })
        .filter(|(_, glyph)| after_word.starts_with(glyph))
        .map(|(state, glyph)| (state, glyph.len()))
        .max_by_key(|(_, len)| *len)?;
    Some((state, word_len + glyph_len))
}

impl Default for QueueIconSet {
    fn default() -> Self {
        Self {
            pending: "📥".into(),
            blocked: "⛔".into(),
            claimed: "🟡".into(),
            running: "🚧".into(),
            reported: "☑️".into(),
            lost: "⚠️".into(),
            lost_claimed: "⚠️🟡".into(),
            lost_running: "⚠️🚧".into(),
            lost_reported: "⚠️☑️".into(),
            reconciling: "🔄".into(),
            unknown: "❔".into(),
            skipped: "⏭️".into(),
            awaiting_operator: "⏳".into(),
            operator_action: "🙋".into(),
            prioritized: "📌".into(),
            agent_prioritized: "📍".into(),
        }
    }
}

impl QueueIconSet {
    pub fn from_config(config: &QueueIconConfig) -> Result<Self> {
        let defaults = Self::default();
        let resolved = Self {
            pending: config.pending.clone().unwrap_or(defaults.pending),
            blocked: config.blocked.clone().unwrap_or(defaults.blocked),
            claimed: config.claimed.clone().unwrap_or(defaults.claimed),
            running: config.running.clone().unwrap_or(defaults.running),
            reported: config.reported.clone().unwrap_or(defaults.reported),
            lost: config.lost.clone().unwrap_or(defaults.lost),
            lost_claimed: config.lost_claimed.clone().unwrap_or(defaults.lost_claimed),
            lost_running: config.lost_running.clone().unwrap_or(defaults.lost_running),
            lost_reported: config
                .lost_reported
                .clone()
                .unwrap_or(defaults.lost_reported),
            reconciling: config.reconciling.clone().unwrap_or(defaults.reconciling),
            unknown: config.unknown.clone().unwrap_or(defaults.unknown),
            skipped: config.skipped.clone().unwrap_or(defaults.skipped),
            awaiting_operator: config
                .awaiting_operator
                .clone()
                .unwrap_or(defaults.awaiting_operator),
            operator_action: config
                .operator_action
                .clone()
                .unwrap_or(defaults.operator_action),
            prioritized: config.prioritized.clone().unwrap_or(defaults.prioritized),
            agent_prioritized: config
                .agent_prioritized
                .clone()
                .unwrap_or(defaults.agent_prioritized),
        };
        resolved.validate()?;
        Ok(resolved)
    }

    /// Every configured prefix with its `[queue_icons]` key, in legend order.
    /// Validation and the marker legend both read this one list, so a palette
    /// field cannot be validated without also being documented
    /// (`#queuemarkerlegend`).
    pub fn entries(&self) -> [(&'static str, &str); 16] {
        [
            ("pending", self.pending.as_str()),
            ("blocked", self.blocked.as_str()),
            ("claimed", self.claimed.as_str()),
            ("running", self.running.as_str()),
            ("reported", self.reported.as_str()),
            ("lost", self.lost.as_str()),
            ("lost_claimed", self.lost_claimed.as_str()),
            ("lost_running", self.lost_running.as_str()),
            ("lost_reported", self.lost_reported.as_str()),
            ("reconciling", self.reconciling.as_str()),
            ("unknown", self.unknown.as_str()),
            ("skipped", self.skipped.as_str()),
            ("awaiting_operator", self.awaiting_operator.as_str()),
            ("operator_action", self.operator_action.as_str()),
            ("prioritized", self.prioritized.as_str()),
            ("agent_prioritized", self.agent_prioritized.as_str()),
        ]
    }

    fn validate(&self) -> Result<()> {
        let entries = self.entries();
        let mut seen = HashSet::new();
        for (name, value) in entries {
            if value.is_empty() || value.trim() != value || value.contains(['\n', '\r']) {
                bail!(
                    "queue_icons.{name} must be a non-empty single-line prefix without surrounding whitespace"
                );
            }
            if !seen.insert(value) {
                bail!("queue_icons prefixes must be distinct; duplicate value {value:?}");
            }
            for (built_in_name, built_in_value) in [
                ("pending", "📥"),
                ("blocked", "⛔"),
                ("claimed", "🟡"),
                ("running", "🚧"),
                ("reported", "☑️"),
                ("reported", "📫"),
                ("reported", "📤"),
                ("lost", "⚠️"),
                ("reconciling", "🔄"),
                ("unknown", "❔"),
                ("skipped", "⏭️"),
                ("awaiting_operator", "⏳"),
                ("operator_action", "🙋"),
                ("prioritized", "📌"),
                ("agent_prioritized", "📍"),
            ] {
                if value == built_in_value && name != built_in_name {
                    bail!(
                        "queue_icons.{name} value {value:?} conflicts with the built-in {built_in_name} prefix"
                    );
                }
            }
        }
        Ok(())
    }
}

static QUEUE_ICONS: OnceLock<QueueIconSet> = OnceLock::new();

mod legend;
pub use legend::{
    AGENT_DOC_REPLY_GLYPH, ALL_QUEUE_MARKERS, QUEUE_MARKER_ANNOTATIONS, QUEUE_MARKER_LEGEND_ANCHOR,
    QUEUE_MARKER_LEGEND_LINE_PREFIX, QUEUE_MARKER_LEGEND_URL, QUEUE_REPLY_LEGEND_ENTRY,
    QUEUE_REPLY_MEANING, QueueMarker, QueueMarkerAnnotation, is_queue_marker_legend_line,
    queue_marker_legend_help, queue_marker_legend_help_for, queue_marker_legend_line,
    queue_marker_legend_line_for, queue_marker_legend_markdown_table,
};

/// Install a validated palette before parsing or rendering queue rows.
pub fn install_queue_icon_config(config: &QueueIconConfig) -> Result<()> {
    let resolved = QueueIconSet::from_config(config)?;
    if let Some(current) = QUEUE_ICONS.get() {
        if current == &resolved {
            return Ok(());
        }
        bail!("queue icon configuration was already initialized with different values");
    }
    QUEUE_ICONS
        .set(resolved)
        .map_err(|_| anyhow::anyhow!("queue icon configuration initialization raced"))
}

/// Return the installed palette, or the documented defaults when no application
/// entrypoint has installed overrides (for library consumers and unit tests).
pub fn queue_icons() -> &'static QueueIconSet {
    QUEUE_ICONS.get_or_init(QueueIconSet::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#lifecyclewordfusion`: haiven-dev sdk-dev.md 2026-11-03 carried
    /// `📥 running🚧 do [#q-c2010d98af04]`. The fused state word and glyph is
    /// one torn lifecycle token whose glyph names the state.
    #[test]
    fn fused_state_word_and_glyph_is_one_lifecycle_token() {
        let row = "running🚧 do [#q-c2010d98af04]";
        let (state, len) = fused_lifecycle_word_token(row).expect("fused token");
        assert_eq!(state, QueueLifecycleMarker::Running);
        assert_eq!(&row[len..], " do [#q-c2010d98af04]");

        // The glyph decides the state; the word is discarded, any case.
        assert_eq!(
            fused_lifecycle_word_token("Claimed🚧 do [#a]").map(|(state, _)| state),
            Some(QueueLifecycleMarker::Running)
        );
        let (state, len) = fused_lifecycle_word_token("lost_running⚠️🚧 do [#a]").unwrap();
        assert_eq!(state, QueueLifecycleMarker::LostRunning);
        assert_eq!(len, "lost_running⚠️🚧".len());
        assert_eq!(
            fused_lifecycle_word_token("reported📤 x").map(|(state, _)| state),
            Some(QueueLifecycleMarker::Reported)
        );
    }

    #[test]
    fn operator_prose_is_never_a_fused_lifecycle_token() {
        for text in [
            "running tests on the branch",
            "running 🚧 do [#a]",
            "🚧 do [#a]",
            "runningX🚧 do [#a]",
            "do [#a] running🚧",
            "",
        ] {
            assert_eq!(fused_lifecycle_word_token(text), None, "{text:?}");
        }
    }

    #[test]
    fn reported_default_is_worker_complete_handoff() {
        assert_eq!(QueueIconSet::default().reported, "☑️");
    }

    #[test]
    fn resolves_arbitrary_multi_character_prefixes_over_defaults() {
        let icons = QueueIconSet::from_config(&QueueIconConfig {
            pending: Some("IN>".into()),
            reported: Some("OUT::".into()),
            ..QueueIconConfig::default()
        })
        .unwrap();
        assert_eq!(icons.pending, "IN>");
        assert_eq!(icons.reported, "OUT::");
        assert_eq!(icons.running, "🚧");
    }

    #[test]
    fn rejects_ambiguous_or_multiline_prefixes() {
        let duplicate = QueueIconConfig {
            pending: Some("same".into()),
            reported: Some("same".into()),
            ..QueueIconConfig::default()
        };
        assert!(QueueIconSet::from_config(&duplicate).is_err());

        let multiline = QueueIconConfig {
            pending: Some("in\nout".into()),
            ..QueueIconConfig::default()
        };
        assert!(QueueIconSet::from_config(&multiline).is_err());

        let legacy_collision = QueueIconConfig {
            pending: Some("📤".into()),
            reported: Some("OUT>".into()),
            ..QueueIconConfig::default()
        };
        assert!(QueueIconSet::from_config(&legacy_collision).is_err());
    }
}
