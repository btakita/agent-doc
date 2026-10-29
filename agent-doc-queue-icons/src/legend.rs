//! Queue marker legend: the one definition of what every visible queue prefix
//! means (`#queuemarkerlegend`).
//!
//! `agent-doc queue --help`, the per-document legend line in `agent:status`,
//! and the book's marker table are all rendered from [`ALL_QUEUE_MARKERS`] and
//! [`QUEUE_MARKER_ANNOTATIONS`]. `QueueMarker::label` and
//! `QueueMarker::meaning` match exhaustively on [`QueueLifecycleMarker`], so a
//! new lifecycle state does not compile until it is documented. The tests pin
//! that every `[queue_icons]` palette field has an entry here.

use crate::{QueueIconSet, QueueLifecycleMarker};

/// Stable book anchor for the legend. Other pages and the onboarding guide
/// link to `docs/guide/queue-markers.md#queue-markers`.
pub const QUEUE_MARKER_LEGEND_ANCHOR: &str = "queue-markers";

/// Public URL of the legend page, linked from the per-document legend line.
pub const QUEUE_MARKER_LEGEND_URL: &str =
    "https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers";

/// Leading text of the per-document legend line. A status line is the
/// binary-owned legend only when it starts with this prefix and links
/// [`QUEUE_MARKER_LEGEND_URL`], so operator prose that merely mentions queue
/// markers is never mistaken for it.
pub const QUEUE_MARKER_LEGEND_LINE_PREFIX: &str = "Queue markers:";

/// Every visible prefix the binary paints on (or accepts from) a queue row.
///
/// Lifecycle markers are mutually exclusive: a row shows exactly one.
/// `OperatorAction` composes with a lifecycle marker (`🙋🚧`). The two pins
/// are operator- or agent-authored priority, not lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueMarker {
    Lifecycle(QueueLifecycleMarker),
    OperatorAction,
    Prioritized,
    AgentPrioritized,
}

/// Legend order. Matches [`QueueIconSet::entries`] one-to-one.
pub const ALL_QUEUE_MARKERS: [QueueMarker; 13] = [
    QueueMarker::Lifecycle(QueueLifecycleMarker::Pending),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Blocked),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Claimed),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Running),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Reported),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Lost),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Reconciling),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Unknown),
    QueueMarker::Lifecycle(QueueLifecycleMarker::Skipped),
    QueueMarker::Lifecycle(QueueLifecycleMarker::AwaitingOperator),
    QueueMarker::OperatorAction,
    QueueMarker::Prioritized,
    QueueMarker::AgentPrioritized,
];

impl QueueMarker {
    /// The `[queue_icons]` key that overrides this prefix.
    pub const fn config_key(self) -> &'static str {
        match self {
            Self::Lifecycle(state) => match state {
                QueueLifecycleMarker::Pending => "pending",
                QueueLifecycleMarker::Blocked => "blocked",
                QueueLifecycleMarker::Claimed => "claimed",
                QueueLifecycleMarker::Running => "running",
                QueueLifecycleMarker::Reported => "reported",
                QueueLifecycleMarker::Lost => "lost",
                QueueLifecycleMarker::Reconciling => "reconciling",
                QueueLifecycleMarker::Unknown => "unknown",
                QueueLifecycleMarker::Skipped => "skipped",
                QueueLifecycleMarker::AwaitingOperator => "awaiting_operator",
            },
            Self::OperatorAction => "operator_action",
            Self::Prioritized => "prioritized",
            Self::AgentPrioritized => "agent_prioritized",
        }
    }

    /// This marker's prefix in `icons`.
    pub fn prefix_in(self, icons: &QueueIconSet) -> &str {
        icons
            .entries()
            .into_iter()
            .find(|(key, _)| *key == self.config_key())
            .map(|(_, value)| value)
            .expect("every queue marker has a palette entry")
    }

    /// Short label used in the one-line document legend.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lifecycle(state) => match state {
                QueueLifecycleMarker::Pending => "pending",
                QueueLifecycleMarker::Blocked => "blocked",
                QueueLifecycleMarker::Claimed => "claimed",
                QueueLifecycleMarker::Running => "running",
                QueueLifecycleMarker::Reported => "reported",
                QueueLifecycleMarker::Lost => "lost",
                QueueLifecycleMarker::Reconciling => "needs reconcile",
                QueueLifecycleMarker::Unknown => "unknown",
                QueueLifecycleMarker::Skipped => "skipped",
                QueueLifecycleMarker::AwaitingOperator => "awaiting operator",
            },
            Self::OperatorAction => "operator action",
            Self::Prioritized => "pinned",
            Self::AgentPrioritized => "agent pin",
        }
    }

    /// One-sentence meaning used by `--help` and the book table.
    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Lifecycle(state) => match state {
                QueueLifecycleMarker::Pending => {
                    "Live row with no claim, worker report, or completion evidence; ready to run."
                }
                QueueLifecycleMarker::Blocked => {
                    "Its tracked item still waits on open work: a `(stays open: blocked behind #a, #b)` clause or an `after=#id` edge names an item still open in backlog, todo or review. `⟦blocked by a, b⟧` lists those ids. The row is held out of drain and subagent dispatch and returns to pending once every blocker closes."
                }
                QueueLifecycleMarker::Claimed => {
                    "Reserved for a worker by `queue claim`; no spawned worker is bound yet."
                }
                QueueLifecycleMarker::Running => {
                    "A spawned worker is bound to this exact claim by `queue start`."
                }
                QueueLifecycleMarker::Reported => {
                    "The worker reported back (`queue report-complete`); the coordinator still has to integrate, verify, and close the row."
                }
                QueueLifecycleMarker::Lost => {
                    "The claim expired or its worker was lost; reclaim it (a new generation) or `queue release --requeue` it."
                }
                QueueLifecycleMarker::Reconciling => {
                    "A binary completion cell whose recorded item is this row's own work is in the document, but no current claim or report receipt owns it. A longer line that only shares the tracked id is not this row. The row is held out of dispatch until the coordinator closes it (striking retires the cell) or reconciles it."
                }
                QueueLifecycleMarker::Unknown => {
                    "Durable queue state is unreadable or contradicts the row (for example a struck row with a live claim); diagnose it, do not redispatch."
                }
                QueueLifecycleMarker::Skipped => {
                    "It was dispatched and came back unconsumed; the queue advanced past it to the next runnable head."
                }
                QueueLifecycleMarker::AwaitingOperator => {
                    "Agent work is finished and no worker holds it; it waits on an operator verdict (`[operator-verify]` or a gated review)."
                }
            },
            Self::OperatorAction => {
                "A blocking operator request is open for this row. It combines with a lifecycle marker (`🙋🚧`) and links the request packet by its `[#request-id]`."
            }
            Self::Prioritized => {
                "Operator priority pin (`**pin**` and `:pushpin:` are also accepted); the row keeps its position at the top tier."
            }
            Self::AgentPrioritized => {
                "Agent priority pin (`*pin*` and `:round_pushpin:` are also accepted); it sorts above unpinned rows, never above operator pins."
            }
        }
    }
}

/// A non-prefix token the binary writes into queue rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueMarkerAnnotation {
    pub token: &'static str,
    pub meaning: &'static str,
}

/// Annotations and legacy spellings that appear on queue rows. Like the
/// prefixes, every one is stripped before queue work identity is computed.
pub const QUEUE_MARKER_ANNOTATIONS: [QueueMarkerAnnotation; 6] = [
    QueueMarkerAnnotation {
        token: "[#<owner>#<N>]",
        meaning: "Owner and claim generation of the current attempt or report receipt, for example `[#subagent:fix#2]`. Metadata, not work identity.",
    },
    QueueMarkerAnnotation {
        token: "⟦blocked by <id>, <id>⟧",
        meaning: "Follows the blocked marker and names the open items the row waits on (ids without `#`). Metadata, not work identity.",
    },
    QueueMarkerAnnotation {
        token: "⟦effect:N⟧",
        meaning: "Numbers live rows that have identical text, so each one is its own claimable effect.",
    },
    QueueMarkerAnnotation {
        token: "⟦local⟧ / ⟦published⟧ / ⟦published+local⟧",
        meaning: "Worker-reported publication state: local changes (✎), published (⇧), or published and then changed locally again (✎⇧). Metadata, not work identity.",
    },
    QueueMarkerAnnotation {
        token: "📫 / 📤",
        meaning: "Older spellings of the reported marker; normalized on the next projection write.",
    },
    QueueMarkerAnnotation {
        token: "⟦<owner>#<N>⟧",
        meaning: "Older spelling of the owner annotation; normalized to `[#<owner>#<N>]`.",
    },
];

/// The compact single-line legend for `icons`, as the binary writes it into a
/// document's `agent:status` component.
pub fn queue_marker_legend_line_for(icons: &QueueIconSet) -> String {
    let entries = ALL_QUEUE_MARKERS
        .iter()
        .map(|marker| format!("{} {}", marker.prefix_in(icons), marker.label()))
        .collect::<Vec<_>>()
        .join(" · ");
    format!("{QUEUE_MARKER_LEGEND_LINE_PREFIX} {entries} ([legend]({QUEUE_MARKER_LEGEND_URL}))")
}

/// The legend line for the process-wide installed palette.
pub fn queue_marker_legend_line() -> String {
    queue_marker_legend_line_for(crate::queue_icons())
}

/// Whether `line` is a binary-owned legend line (any palette, any version of
/// the label text): it starts with [`QUEUE_MARKER_LEGEND_LINE_PREFIX`] and
/// links the legend anchor.
pub fn is_queue_marker_legend_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with(QUEUE_MARKER_LEGEND_LINE_PREFIX)
        && line.contains(&format!("queue-markers.md#{QUEUE_MARKER_LEGEND_ANCHOR}"))
}

/// Long-help legend for `agent-doc queue` and its lifecycle subcommands.
///
/// Rendered from the built-in defaults, never from [`crate::queue_icons`]:
/// clap builds help before the configured palette is installed, and reading
/// the process palette here would lock the defaults in.
pub fn queue_marker_legend_help() -> String {
    queue_marker_legend_help_for(&QueueIconSet::default())
}

/// [`queue_marker_legend_help`] for an explicit palette (`agent-doc queue
/// legend` prints the installed one).
pub fn queue_marker_legend_help_for(icons: &QueueIconSet) -> String {
    let mut out = String::from(
        "Queue markers (binary-owned prefixes on agent:run / agent:queue rows; never part of the work text):\n",
    );
    for marker in ALL_QUEUE_MARKERS {
        out.push_str(&format!(
            "  {}  {:<18} {}\n",
            marker.prefix_in(icons),
            marker.label(),
            marker.meaning()
        ));
    }
    out.push_str("Annotations:\n");
    for annotation in QUEUE_MARKER_ANNOTATIONS {
        out.push_str(&format!(
            "  {:<16} {}\n",
            annotation.token, annotation.meaning
        ));
    }
    out.push_str(if *icons == QueueIconSet::default() {
        "Defaults shown; override them in [queue_icons] (~/.config/agent-doc/config.toml).\n"
    } else {
        "Configured [queue_icons] palette shown (~/.config/agent-doc/config.toml).\n"
    });
    out.push_str(&format!("Full legend: {QUEUE_MARKER_LEGEND_URL}"));
    out
}

/// The book's marker table, rendered from the defaults. The legend page
/// embeds this text verbatim and a test fails when they drift.
pub fn queue_marker_legend_markdown_table() -> String {
    let icons = QueueIconSet::default();
    let mut out = String::from(
        "| Marker | `[queue_icons]` key | State | Meaning |\n| --- | --- | --- | --- |\n",
    );
    for marker in ALL_QUEUE_MARKERS {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            marker.prefix_in(&icons),
            marker.config_key(),
            marker.label(),
            marker.meaning()
        ));
    }
    out.push_str("\n| Annotation | Meaning |\n| --- | --- |\n");
    for annotation in QUEUE_MARKER_ANNOTATIONS {
        out.push_str(&format!(
            "| `{}` | {} |\n",
            annotation.token, annotation.meaning
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALL_LIFECYCLE_STATES;

    /// Destructures every palette field without `..`: adding a
    /// `[queue_icons]` field fails to compile here until it is given a
    /// legend entry, and the assertion proves the entry exists.
    #[test]
    fn every_palette_field_has_a_legend_entry() {
        let QueueIconSet {
            pending,
            blocked,
            claimed,
            running,
            reported,
            lost,
            reconciling,
            unknown,
            skipped,
            awaiting_operator,
            operator_action,
            prioritized,
            agent_prioritized,
        } = QueueIconSet::default();
        let fields = [
            pending,
            blocked,
            claimed,
            running,
            reported,
            lost,
            reconciling,
            unknown,
            skipped,
            awaiting_operator,
            operator_action,
            prioritized,
            agent_prioritized,
        ];
        let icons = QueueIconSet::default();
        let legend: Vec<&str> = ALL_QUEUE_MARKERS
            .iter()
            .map(|marker| marker.prefix_in(&icons))
            .collect();
        assert_eq!(fields.len(), ALL_QUEUE_MARKERS.len());
        for field in &fields {
            assert!(
                legend.contains(&field.as_str()),
                "{field} has no legend entry"
            );
        }
        let keys: Vec<&str> = icons.entries().iter().map(|(key, _)| *key).collect();
        let legend_keys: Vec<&str> = ALL_QUEUE_MARKERS
            .iter()
            .map(|marker| marker.config_key())
            .collect();
        assert_eq!(keys, legend_keys, "legend order must match the palette");
    }

    /// The exhaustive match fails to compile for a new lifecycle variant; the
    /// assertions prove it is in both the lifecycle and legend lists.
    #[test]
    fn every_lifecycle_state_is_listed_and_documented() {
        for state in [
            QueueLifecycleMarker::Pending,
            QueueLifecycleMarker::Blocked,
            QueueLifecycleMarker::Claimed,
            QueueLifecycleMarker::Running,
            QueueLifecycleMarker::Reported,
            QueueLifecycleMarker::Lost,
            QueueLifecycleMarker::Reconciling,
            QueueLifecycleMarker::Unknown,
            QueueLifecycleMarker::Skipped,
            QueueLifecycleMarker::AwaitingOperator,
        ] {
            match state {
                QueueLifecycleMarker::Pending
                | QueueLifecycleMarker::Blocked
                | QueueLifecycleMarker::Claimed
                | QueueLifecycleMarker::Running
                | QueueLifecycleMarker::Reported
                | QueueLifecycleMarker::Lost
                | QueueLifecycleMarker::Reconciling
                | QueueLifecycleMarker::Unknown
                | QueueLifecycleMarker::Skipped
                | QueueLifecycleMarker::AwaitingOperator => {}
            }
            assert!(ALL_LIFECYCLE_STATES.contains(&state), "{state:?}");
            assert!(
                ALL_QUEUE_MARKERS.contains(&QueueMarker::Lifecycle(state)),
                "{state:?} has no legend entry"
            );
            let marker = QueueMarker::Lifecycle(state);
            assert_eq!(
                marker.prefix_in(&QueueIconSet::default()),
                state.default_prefix()
            );
        }
        assert_eq!(ALL_LIFECYCLE_STATES.len(), 10);
    }

    #[test]
    fn help_line_and_table_name_every_marker() {
        let icons = QueueIconSet::default();
        let help = queue_marker_legend_help();
        let line = queue_marker_legend_line_for(&icons);
        let table = queue_marker_legend_markdown_table();
        for marker in ALL_QUEUE_MARKERS {
            let prefix = marker.prefix_in(&icons);
            for (surface, text) in [("help", &help), ("line", &line), ("table", &table)] {
                assert!(
                    text.contains(prefix),
                    "{surface} is missing {prefix} ({marker:?})"
                );
            }
            assert!(help.contains(marker.meaning()));
            assert!(table.contains(marker.meaning()));
            assert!(line.contains(marker.label()));
        }
        for annotation in QUEUE_MARKER_ANNOTATIONS {
            assert!(help.contains(annotation.token));
            assert!(table.contains(annotation.token));
        }
        assert!(help.contains("🔄  needs reconcile"));
        assert!(help.contains("⛔  blocked"));
        assert!(line.contains("⛔ blocked"));
    }

    #[test]
    fn legend_line_is_single_line_and_self_identifying() {
        let line = queue_marker_legend_line_for(&QueueIconSet::default());
        assert!(!line.contains('\n'));
        assert!(is_queue_marker_legend_line(&line));
        assert!(is_queue_marker_legend_line(&format!("  {line}  ")));
        // Never an annotation or list shape a queue/tracked-work parser reads.
        assert!(!line.contains("[#"));
        assert!(!line.trim_start().starts_with('-'));
        assert!(!is_queue_marker_legend_line(
            "Queue markers: the 🔄 row is stale"
        ));
        assert!(!is_queue_marker_legend_line("Status: all green"));
    }

    #[test]
    fn legend_line_follows_a_custom_palette() {
        let icons = QueueIconSet::from_config(&crate::QueueIconConfig {
            reconciling: Some("RECON>".into()),
            ..crate::QueueIconConfig::default()
        })
        .unwrap();
        let line = queue_marker_legend_line_for(&icons);
        assert!(line.contains("RECON> needs reconcile"));
        assert!(!line.contains("🔄"));
        assert!(is_queue_marker_legend_line(&line));
    }

    /// The book page embeds the generated table verbatim.
    #[test]
    fn book_legend_page_matches_the_generated_table() {
        let page = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/guide/queue-markers.md"
        ))
        .expect("docs/guide/queue-markers.md exists");
        assert!(
            page.contains(&queue_marker_legend_markdown_table()),
            "docs/guide/queue-markers.md drifted from the generated legend; \
             paste the output of `agent-doc queue legend --markdown` into it"
        );
        assert!(page.starts_with("# Queue Markers"));
    }

    /// The README's short table is prose, so it is checked for coverage
    /// rather than embedded: every default prefix and the stable anchor link.
    #[test]
    fn readme_queue_marker_section_covers_every_marker() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md"))
            .expect("README.md exists");
        let section = readme
            .split("\n## Queue markers\n")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("README.md has a `## Queue markers` section");
        let icons = QueueIconSet::default();
        for marker in ALL_QUEUE_MARKERS {
            let prefix = marker.prefix_in(&icons);
            assert!(
                section.contains(&format!("`{prefix}`")),
                "README queue markers section is missing {prefix}"
            );
        }
        assert!(section.contains("docs/guide/queue-markers.md#queue-markers"));
    }
}
