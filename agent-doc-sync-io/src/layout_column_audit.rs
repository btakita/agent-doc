//! GH #109: validate and attribute the pane that realises each layout column.
//!
//! The layout path (sync → `tmux_router`) used to realise a column with
//! whichever pane the document resolved to, and record nothing about the
//! choice. In the reported case `%66` was a legitimate column pane — it is
//! bound to `pmt2/mr/1102.md`, which the retained desired layout names as a
//! column — but its route-owned supervisor mapped a superseded binary (agent-doc
//! had already titled it `⚠ STALE SUPERVISOR`), and it was swapped out of
//! `1:stash` into the visible window with no log line naming why it was chosen
//! or that it was stale.
//!
//! This module owns three things:
//!
//! 1. [`pane_supervisor_freshness`] — the reusable "does this pane's supervisor
//!    run the installed build?" predicate. It reuses the same directional
//!    `#supdirstale` rule as preflight (`host_supervisor_pid_binary_is_stale`)
//!    and consults agent-doc's own `⚠ STALE SUPERVISOR` title only as a fallback
//!    witness when the binary identity cannot be observed.
//! 2. [`audit_layout_column_panes`] — run once after `tmux_router` realises the
//!    layout. Every column whose pane moved windows (a stash → layout promotion
//!    in particular), whose pane runs another document, or whose supervisor is
//!    stale gets one `layout_column_pane_selected` line naming the candidates
//!    and why the winner was chosen.
//! 3. The stale-supervisor consequence: the pane is the document's own live
//!    harness, so it is neither excluded (which would leave the document's
//!    column unrealised or provision a second owner) nor reaped. Instead the
//!    existing safe-boundary recycle is requested — the supervisor re-execs onto
//!    the installed build at its next idle boundary, preserving the harness
//!    child and the pane id — and a distinct `layout_column_pane_supervisor_stale`
//!    diagnostic is written.

use crate::sync::{PaneOccupant, pane_occupant_for_document};
use agent_doc_controller::dispatch::is_stash_window_name;
use agent_doc_turn::turn_status::STALE_SUPERVISOR_PANE_MARKER;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

/// How long one supervisor's safe-boundary recycle request is considered
/// in flight before the layout path asks again. The request is an idempotent
/// marker the supervisor consumes at its idle boundary; this window only keeps
/// every tab switch from re-requesting (and re-checkpointing) the same one.
pub const STALE_COLUMN_RECYCLE_REREQUEST_AFTER: Duration = Duration::from_secs(600);

/// Freshness of the route-owned supervisor running in one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneSupervisorFreshness {
    /// The supervisor maps the installed build. `title_marker` records a
    /// leftover `⚠ STALE SUPERVISOR` title (it lags an in-place re-exec until
    /// the next title refresh); the binary identity outranks it.
    Current {
        supervisor_pid: u32,
        title_marker: bool,
    },
    /// The supervisor runs a superseded build. `evidence` names the witness.
    Stale {
        supervisor_pid: Option<u32>,
        evidence: &'static str,
    },
    /// Not enough evidence either way. Never read as fresh or as stale.
    Unknown { reason: &'static str },
}

impl PaneSupervisorFreshness {
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }

    pub fn supervisor_pid(&self) -> Option<u32> {
        match self {
            Self::Current { supervisor_pid, .. } => Some(*supervisor_pid),
            Self::Stale { supervisor_pid, .. } => *supervisor_pid,
            Self::Unknown { .. } => None,
        }
    }

    /// Single `supervisor=` log token.
    pub fn log_token(&self) -> String {
        match self {
            Self::Current {
                supervisor_pid,
                title_marker,
            } => format!(
                "current:pid={supervisor_pid}{}",
                if *title_marker {
                    ":title_marker_lag"
                } else {
                    ""
                }
            ),
            Self::Stale {
                supervisor_pid,
                evidence,
            } => format!(
                "stale:pid={}:evidence={evidence}",
                supervisor_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            Self::Unknown { reason } => format!("unknown:{reason}"),
        }
    }
}

/// True when a pane title carries agent-doc's own stale-supervisor marker.
pub fn title_has_stale_supervisor_marker(title: &str) -> bool {
    title.trim_start().starts_with(STALE_SUPERVISOR_PANE_MARKER)
}

/// Pure freshness decision over the observed facts.
///
/// `binary_stale` is the directional binary-identity verdict for
/// `supervisor_pid` (`None` = unobservable). The binary identity is the
/// authority; the title marker — agent-doc's own earlier diagnosis — only
/// decides when the binary cannot be observed.
pub fn classify_pane_supervisor_freshness(
    supervisor_pid: Option<u32>,
    binary_stale: Option<bool>,
    title_marker: bool,
) -> PaneSupervisorFreshness {
    match (supervisor_pid, binary_stale) {
        (Some(pid), Some(true)) => PaneSupervisorFreshness::Stale {
            supervisor_pid: Some(pid),
            evidence: if title_marker {
                "binary_identity+title_marker"
            } else {
                "binary_identity"
            },
        },
        (Some(pid), Some(false)) => PaneSupervisorFreshness::Current {
            supervisor_pid: pid,
            title_marker,
        },
        (pid, _) if title_marker => PaneSupervisorFreshness::Stale {
            supervisor_pid: pid,
            evidence: "title_marker",
        },
        (None, _) => PaneSupervisorFreshness::Unknown {
            reason: "no_supervisor_process",
        },
        (Some(_), None) => PaneSupervisorFreshness::Unknown {
            reason: "binary_identity_unobservable",
        },
    }
}

/// Reusable IO predicate: the freshness of the agent-doc supervisor serving
/// `file` inside `pane_id`.
///
/// `known_title` lets a caller that already snapshotted pane titles avoid a
/// second tmux round-trip; `None` reads the title live. Read-only: it never
/// requests a recycle and never touches the pane.
pub fn pane_supervisor_freshness(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    known_title: Option<&str>,
) -> PaneSupervisorFreshness {
    let title_marker = match known_title {
        Some(title) => title_has_stale_supervisor_marker(title),
        None => agent_doc_tmux_io::display_message_value(tmux, Some(pane_id), "#{pane_title}")
            .is_some_and(|title| title_has_stale_supervisor_marker(&title)),
    };
    let supervisor_pid = agent_doc_tmux_io::pane_pid(tmux, pane_id).and_then(|pane_pid| {
        agent_doc_process_owner_io::process_tree_agent_doc_owner_pid_for_file(
            &pane_pid.to_string(),
            &file.to_string_lossy(),
        )
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    });
    let binary_stale = supervisor_pid
        .and_then(agent_doc_controller_io::project_controller::host_supervisor_pid_binary_is_stale);
    classify_pane_supervisor_freshness(supervisor_pid, binary_stale, title_marker)
}

/// Where the pane that realised a column came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnPaneSource {
    /// A controller/registry projection sync proved and handed to tmux-router.
    PreResolved,
    /// tmux-router's own durable-registry lookup by session key.
    Registry,
    /// tmux-router's in-memory donor/spare assignment for an unresolved file.
    RouterEphemeral,
}

impl ColumnPaneSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreResolved => "pre_resolved",
            Self::Registry => "registry",
            Self::RouterEphemeral => "router_ephemeral",
        }
    }
}

/// Pure source attribution.
pub fn classify_column_pane_source(
    pane: &str,
    pre_resolved: Option<&str>,
    registry: Option<&str>,
) -> ColumnPaneSource {
    if pre_resolved == Some(pane) {
        ColumnPaneSource::PreResolved
    } else if registry == Some(pane) {
        ColumnPaneSource::Registry
    } else {
        ColumnPaneSource::RouterEphemeral
    }
}

/// `binding=` log token for a column pane relative to the column's document.
pub fn column_binding_token(occupant: &PaneOccupant) -> String {
    match occupant {
        PaneOccupant::Free => "own".to_string(),
        PaneOccupant::OtherDocument(other) => format!("other_document:{other}"),
        PaneOccupant::ForeignHarness => "foreign_harness".to_string(),
    }
}

/// True when the pane left a stash window for a non-stash window in this pass.
pub fn promoted_from_stash(origin_window: Option<&str>, final_window: Option<&str>) -> bool {
    origin_window.is_some_and(is_stash_window_name)
        && final_window.is_some_and(|window| !is_stash_window_name(window))
}

/// Whether one realised column deserves a selection line. Steady-state syncs,
/// where the column's own fresh pane stayed where it was, stay quiet.
pub fn column_selection_is_notable(
    origin_window: Option<&str>,
    final_window: Option<&str>,
    occupant: &PaneOccupant,
    freshness: &PaneSupervisorFreshness,
) -> bool {
    origin_window != final_window || *occupant != PaneOccupant::Free || freshness.is_stale()
}

/// Pure recycle de-duplication: request when never requested, or when the
/// previous request is older than `window`.
pub fn stale_column_recycle_due(
    last_requested: Option<Instant>,
    now: Instant,
    window: Duration,
) -> bool {
    last_requested.is_none_or(|last| now.saturating_duration_since(last) >= window)
}

fn stale_column_recycle_requests() -> &'static Mutex<HashMap<String, Instant>> {
    static REQUESTS: std::sync::OnceLock<Mutex<HashMap<String, Instant>>> =
        std::sync::OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Claim the right to request a recycle for `ledger_key` (a supervisor pid,
/// or a pane when no pid was observable) now.
fn claim_stale_column_recycle(ledger_key: &str, now: Instant) -> bool {
    let mut requests = match stale_column_recycle_requests().lock() {
        Ok(requests) => requests,
        Err(poisoned) => {
            eprintln!(
                "[sync] stale-column recycle ledger was poisoned by an earlier panic; recovering it"
            );
            poisoned.into_inner()
        }
    };
    if !stale_column_recycle_due(
        requests.get(ledger_key).copied(),
        now,
        STALE_COLUMN_RECYCLE_REREQUEST_AFTER,
    ) {
        return false;
    }
    requests.insert(ledger_key.to_string(), now);
    true
}

/// One tmux pane's window and title, captured in a single `list-panes -a`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneWindowSnapshot {
    pub window_name: String,
    pub title: String,
}

const PANE_SNAPSHOT_FORMAT: &str = "#{pane_id}\t#{window_name}\t#{pane_title}";

/// Parse `list-panes -a -F '#{pane_id}\t#{window_name}\t#{pane_title}'`.
pub fn parse_pane_window_snapshot(output: &str) -> HashMap<String, PaneWindowSnapshot> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let pane = fields.next()?.trim();
            if pane.is_empty() {
                return None;
            }
            let window_name = fields.next().unwrap_or_default().to_string();
            let title = fields.next().unwrap_or_default().to_string();
            Some((pane.to_string(), PaneWindowSnapshot { window_name, title }))
        })
        .collect()
}

/// Snapshot every pane's window and title. An unreachable tmux yields an empty
/// map after logging, so the audit degrades to "origin unknown" rather than
/// guessing.
pub fn snapshot_pane_windows(tmux: &Tmux) -> HashMap<String, PaneWindowSnapshot> {
    match agent_doc_tmux_io::list_panes_all(tmux, PANE_SNAPSHOT_FORMAT) {
        Ok(output) => parse_pane_window_snapshot(&output),
        Err(error) => {
            eprintln!("[sync] layout column audit: could not snapshot tmux panes: {error}");
            HashMap::new()
        }
    }
}

/// Inputs the audit needs from one sync pass.
pub struct LayoutColumnAuditInput<'a> {
    /// `(file, pane)` pairs tmux-router realised.
    pub file_panes: &'a [(PathBuf, String)],
    /// Panes sync proved and handed to tmux-router.
    pub pre_resolved: &'a HashMap<PathBuf, String>,
    /// Durable-registry pane per file, as tmux-router would look it up.
    pub registry_pane: &'a dyn Fn(&Path) -> Option<String>,
    /// Pane → window/title before tmux-router ran.
    pub before: &'a HashMap<String, PaneWindowSnapshot>,
    /// Pane → window/title after tmux-router ran.
    pub after: &'a HashMap<String, PaneWindowSnapshot>,
}

/// GH #109: validate and attribute every realised layout column.
///
/// Diagnostic plus the one non-destructive repair the evidence licenses: a
/// stale supervisor in the column's OWN pane gets a safe-boundary recycle
/// request. A pane bound to another document or to a foreign harness is only
/// reported — this audit never touches, moves, or reaps a pane.
pub fn audit_layout_column_panes(tmux: &Tmux, input: &LayoutColumnAuditInput<'_>) {
    // One `/proc` walk serves every column's ownership and supervisor lookup.
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    for (file, pane) in input.file_panes {
        let before = input.before.get(pane);
        let after = input.after.get(pane);
        let origin_window = before.map(|snapshot| snapshot.window_name.as_str());
        let final_window = after.map(|snapshot| snapshot.window_name.as_str());
        let occupant = pane_occupant_for_document(tmux, pane, file);
        let title = after.or(before).map(|snapshot| snapshot.title.as_str());
        let freshness = if occupant == PaneOccupant::Free {
            pane_supervisor_freshness(tmux, pane, file, title)
        } else {
            // Not this document's supervisor; its freshness says nothing
            // about whether this column is served correctly.
            PaneSupervisorFreshness::Unknown {
                reason: "not_document_owner",
            }
        };
        // A stale supervisor is notable when its recycle is (re)requested this
        // pass; between requests a column that stayed put stays quiet, so a
        // tab-switch storm cannot flood ops.log with the same diagnosis.
        let stale_recycle_due = match &freshness {
            PaneSupervisorFreshness::Stale { supervisor_pid, .. } => {
                let ledger_key = supervisor_pid
                    .map(|pid| format!("pid:{pid}"))
                    .unwrap_or_else(|| format!("pane:{pane}"));
                claim_stale_column_recycle(&ledger_key, Instant::now())
            }
            _ => false,
        };
        let notable_freshness = if freshness.is_stale() && !stale_recycle_due {
            &PaneSupervisorFreshness::Unknown {
                reason: "stale_recycle_already_requested",
            }
        } else {
            &freshness
        };
        if !column_selection_is_notable(origin_window, final_window, &occupant, notable_freshness) {
            continue;
        }
        let pre_resolved = input.pre_resolved.get(file).map(String::as_str);
        let registry = (input.registry_pane)(file);
        let source = classify_column_pane_source(pane, pre_resolved, registry.as_deref());
        let promoted = promoted_from_stash(origin_window, final_window);
        let selection = format!(
            "layout_column_pane_selected file={} pane={} source={} origin_window={} window={} promoted_from_stash={} binding={} supervisor={} candidates=pre_resolved:{},registry:{} (GH #109)",
            file.display(),
            pane,
            source.as_str(),
            origin_window.unwrap_or("unknown"),
            final_window.unwrap_or("unknown"),
            promoted,
            column_binding_token(&occupant),
            freshness.log_token(),
            pre_resolved.unwrap_or("none"),
            registry.as_deref().unwrap_or("none"),
        );
        crate::append_sync_log(&selection);
        agent_doc_ops_log_io::log_op(file, &selection);

        if occupant != PaneOccupant::Free {
            let violation = format!(
                "layout_column_pane_foreign_document file={} pane={} source={} binding={} promoted_from_stash={} pane_effect=none (GH #109)",
                file.display(),
                pane,
                source.as_str(),
                column_binding_token(&occupant),
                promoted,
            );
            eprintln!("[sync] warning: {violation}");
            crate::append_sync_log(&violation);
            agent_doc_ops_log_io::log_op(file, &violation);
            continue;
        }

        if let PaneSupervisorFreshness::Stale {
            supervisor_pid,
            evidence,
        } = &freshness
        {
            // De-duplicated above on the supervisor process when it is known,
            // and on the pane when only the title witnessed the staleness.
            let recycle = if stale_recycle_due {
                let status =
                    agent_doc_controller_io::project_controller::schedule_stale_supervisor_cp_recycle(
                        file,
                        "layout_column_selection",
                    );
                format!("safe_boundary_recycle_requested recycle_status={status}")
            } else {
                "safe_boundary_recycle_already_requested".to_string()
            };
            let stale = format!(
                "layout_column_pane_supervisor_stale file={} pane={} supervisor_pid={} evidence={} promoted_from_stash={} source={} action={} pane_effect=none harness_effect=none (GH #109)",
                file.display(),
                pane,
                supervisor_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                evidence,
                promoted,
                source.as_str(),
                recycle,
            );
            eprintln!("[sync] warning: {stale}");
            crate::append_sync_log(&stale);
            agent_doc_ops_log_io::log_op(file, &stale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_identity_outranks_a_lagging_title_marker() {
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(false), true),
            PaneSupervisorFreshness::Current {
                supervisor_pid: 42,
                title_marker: true
            },
            "an in-place re-exec maps the installed inode before the title refresh clears the marker"
        );
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(true), false),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: Some(42),
                evidence: "binary_identity"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(true), true),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: Some(42),
                evidence: "binary_identity+title_marker"
            }
        );
    }

    #[test]
    fn title_marker_decides_only_when_binary_identity_is_unobservable() {
        // GH #105 ask 2 / GH #109 ask 4: agent-doc's own diagnosis is consulted.
        assert_eq!(
            classify_pane_supervisor_freshness(Some(7), None, true),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: Some(7),
                evidence: "title_marker"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(None, None, true),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: None,
                evidence: "title_marker"
            }
        );
        // Missing evidence is never "fresh".
        assert_eq!(
            classify_pane_supervisor_freshness(Some(7), None, false),
            PaneSupervisorFreshness::Unknown {
                reason: "binary_identity_unobservable"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(None, None, false),
            PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            }
        );
    }

    #[test]
    fn stale_title_marker_is_recognised_in_every_title_shape() {
        for (active, stale) in [(true, true), (false, true)] {
            let title = agent_doc_turn::turn_status::pane_title_for_status(active, stale);
            assert!(title_has_stale_supervisor_marker(&title), "{title}");
        }
        for active in [true, false] {
            let title = agent_doc_turn::turn_status::pane_title_for_status(active, false);
            assert!(!title_has_stale_supervisor_marker(&title), "{title}");
        }
        assert!(!title_has_stale_supervisor_marker("custom title"));
    }

    #[test]
    fn issue_109_snapshot_classifies_as_stale_stash_promotion_of_the_columns_own_pane() {
        // The reported evidence: `%66` bound to 1102.md (a retained-layout
        // column), titled `⚠ STALE SUPERVISOR`, supervisor exe inode differs
        // from the installed one, moved from `stash` into `agent-doc`.
        let before = parse_pane_window_snapshot(
            "%66\tstash\t⚠ STALE SUPERVISOR ⟳ agent-doc: turn in progress\n%416\tagent-doc\t⟳ agent-doc: turn in progress\n",
        );
        let after = parse_pane_window_snapshot(
            "%66\tagent-doc\t⚠ STALE SUPERVISOR\n%416\tagent-doc\t⟳ agent-doc: turn in progress\n",
        );
        let origin = before.get("%66").map(|s| s.window_name.as_str());
        let final_window = after.get("%66").map(|s| s.window_name.as_str());
        assert!(promoted_from_stash(origin, final_window));
        let freshness = classify_pane_supervisor_freshness(
            Some(1245655),
            Some(true),
            title_has_stale_supervisor_marker(&after["%66"].title),
        );
        assert!(freshness.is_stale());
        assert_eq!(
            freshness.log_token(),
            "stale:pid=1245655:evidence=binary_identity+title_marker"
        );
        assert!(column_selection_is_notable(
            origin,
            final_window,
            &PaneOccupant::Free,
            &freshness
        ));
        // The good pane that stayed put with a fresh supervisor stays quiet.
        let fresh = classify_pane_supervisor_freshness(Some(2926921), Some(false), false);
        assert!(!column_selection_is_notable(
            Some("agent-doc"),
            Some("agent-doc"),
            &PaneOccupant::Free,
            &fresh
        ));
    }

    #[test]
    fn a_pane_bound_to_another_document_is_always_reported_for_the_column() {
        // GH #109 ask 1/2: even a pane that stayed put must be named when the
        // column it fills belongs to a different document.
        let occupant = PaneOccupant::OtherDocument("tasks/pmt2/mr/1102.md".to_string());
        let unknown = PaneSupervisorFreshness::Unknown {
            reason: "not_document_owner",
        };
        assert!(column_selection_is_notable(
            Some("agent-doc"),
            Some("agent-doc"),
            &occupant,
            &unknown
        ));
        assert_eq!(
            column_binding_token(&occupant),
            "other_document:tasks/pmt2/mr/1102.md"
        );
        assert_eq!(
            column_binding_token(&PaneOccupant::ForeignHarness),
            "foreign_harness"
        );
        assert_eq!(column_binding_token(&PaneOccupant::Free), "own");
    }

    #[test]
    fn promotion_requires_leaving_a_stash_window_for_a_visible_one() {
        assert!(promoted_from_stash(Some("stash"), Some("agent-doc")));
        assert!(promoted_from_stash(Some("stash-2"), Some("agent-doc")));
        assert!(!promoted_from_stash(Some("agent-doc"), Some("stash")));
        assert!(!promoted_from_stash(Some("stash"), Some("stash")));
        assert!(!promoted_from_stash(None, Some("agent-doc")));
        assert!(!promoted_from_stash(Some("stash"), None));
    }

    #[test]
    fn column_source_names_why_the_pane_was_chosen() {
        assert_eq!(
            classify_column_pane_source("%66", Some("%66"), Some("%66")),
            ColumnPaneSource::PreResolved
        );
        assert_eq!(
            classify_column_pane_source("%66", None, Some("%66")),
            ColumnPaneSource::Registry
        );
        assert_eq!(
            classify_column_pane_source("%66", Some("%1"), Some("%2")),
            ColumnPaneSource::RouterEphemeral
        );
    }

    #[test]
    fn stale_recycle_is_requested_once_per_window_per_supervisor() {
        let now = Instant::now();
        let window = Duration::from_secs(600);
        assert!(stale_column_recycle_due(None, now, window));
        assert!(!stale_column_recycle_due(Some(now), now, window));
        assert!(stale_column_recycle_due(
            Some(now),
            now + Duration::from_secs(600),
            window
        ));
        // The process-wide ledger applies the same rule.
        let key = "pid:test-gh109-ledger";
        assert!(claim_stale_column_recycle(key, now));
        assert!(!claim_stale_column_recycle(
            key,
            now + Duration::from_secs(1)
        ));
        assert!(claim_stale_column_recycle(
            key,
            now + STALE_COLUMN_RECYCLE_REREQUEST_AFTER
        ));
        assert!(
            claim_stale_column_recycle("pane:%test-gh109", now),
            "a different supervisor/pane has its own window"
        );
    }

    #[test]
    fn pane_snapshot_parser_keeps_titles_with_tabs_and_skips_blank_lines() {
        let parsed = parse_pane_window_snapshot("%1\tagent-doc\ta\tb\n\n%2\tstash\t\n");
        assert_eq!(parsed["%1"].window_name, "agent-doc");
        assert_eq!(parsed["%1"].title, "a\tb");
        assert_eq!(parsed["%2"].window_name, "stash");
        assert_eq!(parsed["%2"].title, "");
        assert_eq!(parsed.len(), 2);
    }
}
