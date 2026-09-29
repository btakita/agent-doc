//! Name what else holds a document's Claude Code conversation (`#nameelseclaude`).
//!
//! Claude Code refuses a second attach with a bare "This conversation is open in
//! another app / press R to continue". That message names nothing, so an
//! operator staring at four live panes cannot tell which one — or whether the
//! holder is a terminal at all.
//!
//! The information is already on disk. Claude Code writes one record per live
//! session to `~/.claude/sessions/<pid>.json`, carrying `sessionId`, `pid`,
//! `cwd`, `tmux` (`session:@window.%pane`), `entrypoint` (`cli` / `sdk-ts`),
//! `kind`, `name`, and `status`. Liveness is provable from `/proc/<pid>`.
//!
//! Operator-reported 2026-09-28 on `src/haiven-dev/tasks/api.md`. Verified at
//! the time: the four live haiven-dev CLI records held four DISTINCT session
//! ids, so agent-doc had **not** routed a second client onto one conversation —
//! the other holder was not a registry-visible CLI at all, leaving a desktop /
//! web / Chrome-native-host client as the remaining shape.
//!
//! That is the load-bearing case, and it is why this module reports the empty
//! result **positively**. "No second CLI holder exists" is the finding that
//! tells the operator to stop hunting through panes and look outside the
//! terminal; rendering it as silence would reproduce the original bug one layer
//! down.
//!
//! This module only *reports*. It never kills, claims, or reaps a session —
//! `#bare-foreign-session-guard` applies with full force here, because a record
//! this reader can see is frequently a session agent-doc did not start.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// One `~/.claude/sessions/<pid>.json` record, as far as this reader cares.
///
/// Unknown fields are ignored on purpose: the file is written by Claude Code and
/// gains fields independently of agent-doc, so a strict shape would turn a
/// harmless upstream addition into a reporting outage.
#[derive(Debug, Clone, Deserialize)]
pub struct ConversationRecord {
    pub pid: u32,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tmux: Option<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Whether a record still describes a running process.
///
/// Stale records outlive their process — a crash or a `kill -9` leaves the JSON
/// behind — and reporting one as a live holder would send the operator to hunt a
/// pane that no longer exists. That is the same "named an unblocker you cannot
/// perform" failure this whole change is about, so staleness is reported as its
/// own state rather than filtered into silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderLiveness {
    Live,
    /// The record exists but its pid is gone.
    StaleDeadPid,
}

/// A holder of some Claude Code conversation, with its liveness resolved.
#[derive(Debug, Clone)]
pub struct ConversationHolder {
    pub record: ConversationRecord,
    pub liveness: HolderLiveness,
    /// This record's `sessionId` is the document's own Claude conversation
    /// (`resume.claude`) — i.e. it is the exact holder the lock refers to.
    pub holds_conversation: bool,
}

impl ConversationHolder {
    /// A one-line operator-facing description.
    pub fn describe(&self) -> String {
        let name = self.record.name.as_deref().unwrap_or("<unnamed>");
        let entrypoint = self.record.entrypoint.as_deref().unwrap_or("unknown");
        let status = self.record.status.as_deref().unwrap_or("unknown");
        let tmux = self
            .record
            .tmux
            .as_deref()
            .map(|t| format!(" tmux={t}"))
            .unwrap_or_default();
        let liveness = match self.liveness {
            HolderLiveness::Live => "live",
            HolderLiveness::StaleDeadPid => "STALE(dead pid)",
        };
        let holds = if self.holds_conversation {
            " holds_this_conversation"
        } else {
            ""
        };
        format!(
            "{name} pid={} session={} entrypoint={entrypoint} kind={} status={status}{tmux} {liveness}{holds}",
            self.record.pid,
            self.record.session_id,
            self.record.kind.as_deref().unwrap_or("unknown"),
        )
    }
}

/// What the operator should be told about a document's conversation holders.
///
/// The three cases are deliberately distinct. Collapsing "no records at all"
/// into "no holders" would hide a reader that is looking in the wrong place,
/// which is indistinguishable from a genuine answer — the exact confusion this
/// module exists to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationHolderReport {
    /// The session directory could not be read at all.
    Unreadable(String),
    /// Records were readable and none is a live CLI holder for this scope.
    ///
    /// This is a POSITIVE finding: the lock the operator is looking at is not
    /// held by another terminal, so the holder is a desktop / web / native-host
    /// client and no amount of pane-hunting will find it.
    NoLiveCliHolder {
        stale: Vec<String>,
        conversation: ConversationFinding,
    },
    /// One or more live holders, most recently useful first.
    Holders {
        live: Vec<String>,
        stale: Vec<String>,
        conversation: ConversationFinding,
    },
}

/// What is known about the document's OWN Claude conversation (`resume.claude`).
///
/// The project-wide holder list answers "what else is running here"; this answers
/// the question the lock actually asks — "who holds THIS conversation". After an
/// `agent: claude` → `agent: codex` switch the document keeps its old
/// `resume.claude` id, and that id is what a Claude Code client refuses to reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationFinding {
    /// The document records no Claude conversation id.
    NoRecordedConversation,
    /// A live CLI record carries the document's conversation id.
    HeldByLiveCli { conversation_id: String },
    /// No live CLI record carries it: the holder, if any, is outside the terminal.
    NotHeldByLiveCli { conversation_id: String },
}

impl ConversationFinding {
    fn status_line(&self) -> Option<String> {
        match self {
            Self::NoRecordedConversation => None,
            Self::HeldByLiveCli { conversation_id } => Some(format!(
                "conversation_holders.this_conversation: claude={conversation_id} is held by the \
                 live CLI marked holds_this_conversation"
            )),
            Self::NotHeldByLiveCli { conversation_id } => Some(format!(
                "conversation_holders.this_conversation: claude={conversation_id} is not held by any \
                 live Claude Code CLI session; if a client reports it open, the holder is outside \
                 the terminal (desktop, web, or browser native host)"
            )),
        }
    }
}

impl ConversationHolderReport {
    /// Render as `session status` lines. Always emits at least one line.
    pub fn status_lines(&self) -> Vec<String> {
        match self {
            Self::Unreadable(why) => {
                vec![format!("conversation_holders: unreadable ({why})")]
            }
            Self::NoLiveCliHolder {
                stale,
                conversation,
            } => {
                let mut lines = vec![
                    "conversation_holders: none — no other live Claude Code CLI session holds \
                     this conversation. If a client still reports it open, the holder is outside \
                     the terminal (desktop, web, or browser native host); pane-hunting will not \
                     find it."
                        .to_string(),
                ];
                lines.extend(conversation.status_line());
                lines.extend(
                    stale
                        .iter()
                        .map(|s| format!("conversation_holders.stale: {s}")),
                );
                lines
            }
            Self::Holders {
                live,
                stale,
                conversation,
            } => {
                let mut lines = vec![format!("conversation_holders: {} live", live.len())];
                lines.extend(live.iter().map(|s| format!("conversation_holders.live: {s}")));
                lines.extend(conversation.status_line());
                lines.extend(
                    stale
                        .iter()
                        .map(|s| format!("conversation_holders.stale: {s}")),
                );
                lines
            }
        }
    }
}

/// Build the report from already-resolved holders.
///
/// Pure, so the classification is testable without a home directory or a
/// process table.
#[cfg(test)]
pub fn report_from_holders(holders: Vec<ConversationHolder>) -> ConversationHolderReport {
    report_for_conversation(holders, None)
}

/// [`report_from_holders`] plus the finding for the document's own conversation.
pub fn report_for_conversation(
    holders: Vec<ConversationHolder>,
    conversation_id: Option<&str>,
) -> ConversationHolderReport {
    let conversation = match conversation_id {
        None => ConversationFinding::NoRecordedConversation,
        Some(id) => {
            let held = holders
                .iter()
                .any(|h| h.holds_conversation && h.liveness == HolderLiveness::Live);
            if held {
                ConversationFinding::HeldByLiveCli {
                    conversation_id: id.to_string(),
                }
            } else {
                ConversationFinding::NotHeldByLiveCli {
                    conversation_id: id.to_string(),
                }
            }
        }
    };
    let mut live = Vec::new();
    let mut stale = Vec::new();
    for holder in holders {
        let described = holder.describe();
        match holder.liveness {
            HolderLiveness::Live => live.push(described),
            HolderLiveness::StaleDeadPid => stale.push(described),
        }
    }
    if live.is_empty() {
        ConversationHolderReport::NoLiveCliHolder {
            stale,
            conversation,
        }
    } else {
        ConversationHolderReport::Holders {
            live,
            stale,
            conversation,
        }
    }
}

/// Whether a record's `cwd` places it in scope for `document`.
///
/// Scope is the document's project root rather than the document path: Claude
/// Code records the session's working directory, never the file it is editing,
/// so a path-equality test would match nothing and report every document as
/// unheld.
pub fn record_is_in_scope(record: &ConversationRecord, project_root: &Path) -> bool {
    record
        .cwd
        .as_deref()
        .map(|cwd| Path::new(cwd) == project_root)
        .unwrap_or(false)
}

/// The directory Claude Code writes its live-session records to.
pub fn sessions_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude").join("sessions"))
}

/// Is this pid still running?
fn pid_is_live(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Read and classify every conversation holder whose cwd is `project_root` —
/// plus, wherever its cwd is, any record holding `conversation_id` (the
/// document's `resume.claude`) — excluding `exclude_pid` (normally the caller,
/// which is not "something else").
pub fn observe_holders(
    project_root: &Path,
    exclude_pid: Option<u32>,
    conversation_id: Option<&str>,
) -> ConversationHolderReport {
    let Some(dir) = sessions_dir() else {
        return ConversationHolderReport::Unreadable("HOME is not set".to_string());
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            return ConversationHolderReport::Unreadable(format!("{}: {err}", dir.display()));
        }
    };

    let mut holders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                // Never swallow: a record we cannot read is a hole in the
                // answer, and the operator is entitled to know the answer has
                // holes rather than read it as "nothing else is open".
                eprintln!(
                    "[agent-doc] warning: conversation holder record {} is unreadable: {err}",
                    path.display()
                );
                continue;
            }
        };
        let record: ConversationRecord = match serde_json::from_str(&raw) {
            Ok(record) => record,
            Err(err) => {
                eprintln!(
                    "[agent-doc] warning: conversation holder record {} did not parse: {err}",
                    path.display()
                );
                continue;
            }
        };
        if Some(record.pid) == exclude_pid {
            continue;
        }
        let holds_conversation = conversation_id == Some(record.session_id.as_str());
        if !holds_conversation && !record_is_in_scope(&record, project_root) {
            continue;
        }
        let liveness = if pid_is_live(record.pid) {
            HolderLiveness::Live
        } else {
            HolderLiveness::StaleDeadPid
        };
        holders.push(ConversationHolder {
            record,
            liveness,
            holds_conversation,
        });
    }
    report_for_conversation(holders, conversation_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32, session: &str, cwd: &str) -> ConversationRecord {
        ConversationRecord {
            pid,
            session_id: session.to_string(),
            cwd: Some(cwd.to_string()),
            tmux: Some("1:@115.%238".to_string()),
            entrypoint: Some("cli".to_string()),
            kind: Some("interactive".to_string()),
            name: Some("agent-loop-42".to_string()),
            status: Some("idle".to_string()),
        }
    }

    /// The reported case: four live CLI sessions, four DISTINCT session ids, so
    /// agent-doc had not double-routed anything and the real holder was not a
    /// terminal. The empty answer must be stated, not implied by silence.
    #[test]
    fn no_live_cli_holder_is_reported_positively() {
        let report = report_from_holders(vec![]);
        assert_eq!(
            report,
            ConversationHolderReport::NoLiveCliHolder {
                stale: vec![],
                conversation: ConversationFinding::NoRecordedConversation,
            }
        );
        let lines = report.status_lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("outside the terminal"),
            "the operator must be told where to look next: {}",
            lines[0]
        );
    }

    /// A dead pid must never be reported as a live holder — that sends the
    /// operator to hunt a pane that no longer exists.
    #[test]
    fn a_stale_record_is_not_a_live_holder() {
        let report = report_from_holders(vec![ConversationHolder {
            record: record(4242, "s-dead", "/repo"),
            liveness: HolderLiveness::StaleDeadPid,
            holds_conversation: false,
        }]);
        match &report {
            ConversationHolderReport::NoLiveCliHolder { stale, .. } => {
                assert_eq!(stale.len(), 1);
                assert!(stale[0].contains("STALE(dead pid)"));
            }
            other => panic!("a dead pid is not a live holder: {other:?}"),
        }
        // The positive "look outside the terminal" line still appears, because
        // a stale record is not another terminal holding the conversation.
        assert!(report.status_lines()[0].contains("outside the terminal"));
    }

    #[test]
    fn a_live_holder_is_named_with_its_pane_and_session() {
        let report = report_from_holders(vec![ConversationHolder {
            record: record(1279131, "e9ae0a31", "/repo"),
            liveness: HolderLiveness::Live,
            holds_conversation: false,
        }]);
        let lines = report.status_lines();
        assert_eq!(lines[0], "conversation_holders: 1 live");
        let described = &lines[1];
        for expected in [
            "agent-loop-42",
            "pid=1279131",
            "session=e9ae0a31",
            "entrypoint=cli",
            "tmux=1:@115.%238",
            "live",
        ] {
            assert!(
                described.contains(expected),
                "holder line must name {expected}: {described}"
            );
        }
    }

    /// Scope is the project root, not the document path. Keying on the document
    /// would match nothing, and "matched nothing" renders as "nothing else is
    /// open" — a confident wrong answer.
    #[test]
    fn scope_matches_the_project_root_not_the_document() {
        let rec = record(1, "s", "/repo");
        assert!(record_is_in_scope(&rec, Path::new("/repo")));
        assert!(!record_is_in_scope(
            &rec,
            Path::new("/repo/tasks/haiven.md")
        ));
        assert!(!record_is_in_scope(&rec, Path::new("/other")));
    }

    #[test]
    fn a_record_without_a_cwd_is_never_claimed_to_be_in_scope() {
        let mut rec = record(1, "s", "/repo");
        rec.cwd = None;
        assert!(!record_is_in_scope(&rec, Path::new("/repo")));
    }

    /// Upstream may add fields at any time; a strict shape would turn that into
    /// a reporting outage.
    #[test]
    fn unknown_fields_do_not_break_parsing() {
        let raw = r#"{
            "pid": 7,
            "sessionId": "abc",
            "cwd": "/repo",
            "brandNewUpstreamField": {"nested": true}
        }"#;
        let parsed: ConversationRecord = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.pid, 7);
        assert_eq!(parsed.session_id, "abc");
        assert_eq!(parsed.status, None);
        assert!(record_is_in_scope(&parsed, Path::new("/repo")));
    }

    #[test]
    fn live_and_stale_holders_are_reported_separately() {
        let report = report_from_holders(vec![
            ConversationHolder {
                record: record(1, "live-one", "/repo"),
                liveness: HolderLiveness::Live,
                holds_conversation: false,
            },
            ConversationHolder {
                record: record(2, "dead-one", "/repo"),
                liveness: HolderLiveness::StaleDeadPid,
                holds_conversation: false,
            },
        ]);
        match &report {
            ConversationHolderReport::Holders { live, stale, .. } => {
                assert_eq!(live.len(), 1);
                assert_eq!(stale.len(), 1);
            }
            other => panic!("expected both buckets: {other:?}"),
        }
        let lines = report.status_lines();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().any(|l| l.starts_with("conversation_holders.live:")));
        assert!(lines.iter().any(|l| l.starts_with("conversation_holders.stale:")));
    }

    /// After `agent: claude` -> `agent: codex` the document keeps its old
    /// `resume.claude` id. The report must say whether THAT conversation has a
    /// live CLI holder, and flag the exact record when one does.
    #[test]
    fn the_documents_own_conversation_is_named_and_flagged() {
        let report = report_for_conversation(
            vec![
                ConversationHolder {
                    record: record(10, "other-session", "/repo"),
                    liveness: HolderLiveness::Live,
                    holds_conversation: false,
                },
                ConversationHolder {
                    record: record(11, "baaf5655", "/elsewhere"),
                    liveness: HolderLiveness::Live,
                    holds_conversation: true,
                },
            ],
            Some("baaf5655"),
        );
        let lines = report.status_lines();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("session=baaf5655") && l.ends_with("holds_this_conversation")),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("this_conversation: claude=baaf5655 is held by the live CLI")),
            "{lines:#?}"
        );
    }

    #[test]
    fn an_unheld_conversation_points_outside_the_terminal_even_with_other_live_clis() {
        let report = report_for_conversation(
            vec![ConversationHolder {
                record: record(10, "other-session", "/repo"),
                liveness: HolderLiveness::Live,
                holds_conversation: false,
            }],
            Some("baaf5655"),
        );
        let lines = report.status_lines();
        assert_eq!(lines[0], "conversation_holders: 1 live");
        assert!(
            lines.iter().any(|l| l.contains("claude=baaf5655 is not held")
                && l.contains("outside the terminal")),
            "{lines:#?}"
        );
    }

    /// A dead record that once held the conversation must not count as held.
    #[test]
    fn a_stale_record_of_the_conversation_does_not_count_as_held() {
        let report = report_for_conversation(
            vec![ConversationHolder {
                record: record(12, "baaf5655", "/repo"),
                liveness: HolderLiveness::StaleDeadPid,
                holds_conversation: true,
            }],
            Some("baaf5655"),
        );
        assert!(matches!(
            &report,
            ConversationHolderReport::NoLiveCliHolder {
                conversation: ConversationFinding::NotHeldByLiveCli { .. },
                ..
            }
        ));
    }

    /// `#nameelseclaude`: the report must NOT be gated on the document-declared
    /// harness.
    ///
    /// Operator-reported 2026-09-28 on `src/haiven-dev/tasks/api.md` and
    /// `tasks/frontend.md`. Both declare `agent: codex`, so a
    /// `ctx.harness == "claude-code"` gate skipped this entire report — while the
    /// operator, running Claude Code, faced the bare "This conversation is open in
    /// another app" lock with nothing named and three idle panes to guess between.
    /// The queue head carried the cause verbatim: "This happened after
    /// transitioning from `agent: claude` to `agent: codex`".
    ///
    /// A Claude Code conversation lock belongs to the CLI the operator is running.
    /// The document's declared agent cannot speak to it, so it must never decide
    /// whether the holders are named.
    #[test]
    fn session_status_does_not_gate_holder_reporting_on_the_declared_harness() {
        let source = include_str!("session_actor_cmd.rs");
        let call = ["conversation_holders::", "observe_holders("].concat();
        let call_at = source
            .find(call.as_str())
            .expect("session status must still report conversation holders");
        // Built from fragments so this guard never matches its own source text.
        let forbidden = ["ctx.harness == ", "\"claude-code\""].concat();
        let preceding = &source[..call_at];
        let window_start = preceding.len().saturating_sub(400);
        assert!(
            !preceding[window_start..].contains(forbidden.as_str()),
            "the holder report must not sit behind a declared-harness gate; a \
             document carrying `agent: codex` still hits Claude Code conversation \
             locks (#nameelseclaude)"
        );
    }
}
