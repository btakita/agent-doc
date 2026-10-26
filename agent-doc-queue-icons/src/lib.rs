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
    pub claimed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost: Option<String>,
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
    pub claimed: String,
    pub running: String,
    pub reported: String,
    pub lost: String,
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
    Claimed,
    Running,
    Reported,
    Lost,
    Reconciling,
    Unknown,
    Skipped,
    AwaitingOperator,
}

pub const ALL_LIFECYCLE_STATES: [QueueLifecycleMarker; 9] = [
    QueueLifecycleMarker::Pending,
    QueueLifecycleMarker::Claimed,
    QueueLifecycleMarker::Running,
    QueueLifecycleMarker::Reported,
    QueueLifecycleMarker::Lost,
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
            Self::Claimed => &icons.claimed,
            Self::Running => &icons.running,
            Self::Reported => &icons.reported,
            Self::Lost => &icons.lost,
            Self::Reconciling => &icons.reconciling,
            Self::Unknown => &icons.unknown,
            Self::Skipped => &icons.skipped,
            Self::AwaitingOperator => &icons.awaiting_operator,
        }
    }

    pub const fn default_prefix(self) -> &'static str {
        match self {
            Self::Pending => "📥",
            Self::Claimed => "🟡",
            Self::Running => "🚧",
            Self::Reported => "☑️",
            Self::Lost => "⚠️",
            Self::Reconciling => "🔄",
            Self::Unknown => "❔",
            Self::Skipped => "⏭️",
            Self::AwaitingOperator => "⏳",
        }
    }
}

impl Default for QueueIconSet {
    fn default() -> Self {
        Self {
            pending: "📥".into(),
            claimed: "🟡".into(),
            running: "🚧".into(),
            reported: "☑️".into(),
            lost: "⚠️".into(),
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
            claimed: config.claimed.clone().unwrap_or(defaults.claimed),
            running: config.running.clone().unwrap_or(defaults.running),
            reported: config.reported.clone().unwrap_or(defaults.reported),
            lost: config.lost.clone().unwrap_or(defaults.lost),
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

    fn validate(&self) -> Result<()> {
        let entries = [
            ("pending", self.pending.as_str()),
            ("claimed", self.claimed.as_str()),
            ("running", self.running.as_str()),
            ("reported", self.reported.as_str()),
            ("lost", self.lost.as_str()),
            ("reconciling", self.reconciling.as_str()),
            ("unknown", self.unknown.as_str()),
            ("skipped", self.skipped.as_str()),
            ("awaiting_operator", self.awaiting_operator.as_str()),
            ("operator_action", self.operator_action.as_str()),
            ("prioritized", self.prioritized.as_str()),
            ("agent_prioritized", self.agent_prioritized.as_str()),
        ];
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
