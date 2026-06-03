//! Fleet-wide operational control plane (`#ipc-admin-api`).
//!
//! Phase 1 implements the two read-only verbs that replace the manual
//! `ps` / `pstree` / `pgrep` forensics required to resolve a cross-document
//! actor contamination incident (`#xdoc-route-sweep-commits-sibling-docs`):
//!
//! - `agent-doc admin list` — enumerate every actor in the project fleet (one
//!   row per document: session, pane, window, supervisor pid, harness,
//!   generation, state, and pane liveness).
//! - `agent-doc admin detect` — derived diagnostics over the actor store: a live
//!   pane that is the authoritative binding of more than one non-closed document
//!   (the cross-document execution contamination), and non-closed actors whose
//!   pane is dead (orphaned bindings that route/sync should reap).
//!
//! The mutating verbs (`rebind`, `stop`) and the editor surface are tracked as
//! follow-up phases; see `tasks/agent-doc/plan-ipc-admin-api-actor-control.md`.
//! Keeping the detect/enumerate logic here (and pure over its inputs) follows
//! the Shared-Foundation rule: the control plane lives in the binary, and the
//! plugins shell the CLI/FFI rather than re-deriving fleet state.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::session_actor::ActorState;
use crate::sessions::{self, SessionRegistry, Tmux};

type ActorStore = BTreeMap<String, crate::session_actor::ActorRecord>;

/// One enumerated actor row (`admin list`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdminActor {
    pub document_id: String,
    pub session_id: String,
    pub pane: String,
    pub window: String,
    pub harness: String,
    pub generation: u64,
    pub state: String,
    pub pane_alive: bool,
    pub supervisor_pid: Option<u32>,
    pub cwd: Option<String>,
}

/// One derived diagnostic (`admin detect`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdminFinding {
    pub kind: String,
    pub detail: String,
    pub documents: Vec<String>,
    pub pane: Option<String>,
}

/// Resolve the project root for fleet enumeration: explicit `--project-root`,
/// else the nearest `.agent-doc` ancestor of the current directory.
fn resolve_root(project_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    crate::fs_util::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Build the enumerated actor list from the actor store + registry, using
/// `pane_alive` to mark live panes.
///
/// Pure over its inputs (liveness is injected) so it can be unit-tested without
/// a live tmux server. Registry entries are matched to actor records by session
/// id to enrich the supervisor pid and cwd that the actor store does not hold.
pub fn build_actor_list(
    actors: &ActorStore,
    registry: &SessionRegistry,
    pane_alive: impl Fn(&str) -> bool,
) -> Vec<AdminActor> {
    let by_session: BTreeMap<&str, &sessions::SessionEntry> = registry
        .values()
        .map(|entry| (entry.session_id.as_str(), entry))
        .collect();

    actors
        .values()
        .map(|record| {
            let reg = by_session.get(record.session_id.as_str());
            AdminActor {
                document_id: record.document_id.clone(),
                session_id: record.session_id.clone(),
                pane: record.pane_id.clone(),
                window: record.window_id.clone(),
                harness: record.harness.clone(),
                generation: record.generation,
                state: record.state.as_str().to_string(),
                pane_alive: !record.pane_id.is_empty() && pane_alive(&record.pane_id),
                supervisor_pid: reg.map(|e| e.pid),
                cwd: reg.map(|e| e.cwd.clone()),
            }
        })
        .collect()
}

/// Derive cross-document / staleness findings from the actor store.
///
/// Pure over its inputs (liveness is injected):
///
/// - `cross_document_pane`: a single pane id is the authoritative binding of
///   more than one non-`Closed` document — the contamination where two
///   documents execute in the same pane (`#xdoc-route-sweep-commits-sibling-docs`).
/// - `stale_dead_pane`: a non-`Closed` actor whose pane is not alive — an
///   orphaned binding that route/sync should reap.
pub fn detect_findings(actors: &ActorStore, pane_alive: impl Fn(&str) -> bool) -> Vec<AdminFinding> {
    let mut findings = Vec::new();

    // A *live* pane claimed by more than one non-closed document is the
    // cross-document contamination the manual ps/pstree hunt was chasing. A dead
    // shared pane is just stale bindings (reported as `stale_dead_pane` below),
    // not live contention, so liveness gates this finding to keep it actionable.
    let mut by_pane: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for record in actors.values() {
        if record.state == ActorState::Closed || record.pane_id.is_empty() {
            continue;
        }
        if !pane_alive(&record.pane_id) {
            continue;
        }
        by_pane
            .entry(record.pane_id.as_str())
            .or_default()
            .push(record.document_id.as_str());
    }
    for (pane, docs) in &by_pane {
        if docs.len() > 1 {
            let mut documents: Vec<String> = docs.iter().map(|d| d.to_string()).collect();
            documents.sort();
            findings.push(AdminFinding {
                kind: "cross_document_pane".to_string(),
                detail: format!(
                    "pane {} is the live binding of {} documents; only one actor may own a pane",
                    pane,
                    documents.len()
                ),
                documents,
                pane: Some((*pane).to_string()),
            });
        }
    }

    // A non-closed actor whose pane is dead is an orphaned binding.
    for record in actors.values() {
        if record.state == ActorState::Closed || record.pane_id.is_empty() {
            continue;
        }
        if !pane_alive(&record.pane_id) {
            findings.push(AdminFinding {
                kind: "stale_dead_pane".to_string(),
                detail: format!(
                    "actor for {} is {} but its pane {} is not alive (orphaned binding)",
                    record.document_id,
                    record.state.as_str(),
                    record.pane_id
                ),
                documents: vec![record.document_id.clone()],
                pane: Some(record.pane_id.clone()),
            });
        }
    }

    findings
}

/// `agent-doc admin list` — enumerate the project fleet.
pub fn list(project_root: Option<&Path>, json: bool) -> Result<()> {
    let root = resolve_root(project_root)?;
    let actors = crate::project_controller::load_actor_store(&root)?;
    let registry = sessions::load_in(&root)?;
    let tmux = Tmux::default_server();
    let rows = build_actor_list(&actors, &registry, |pane| tmux.pane_alive(pane));

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No registered actors in {}", root.display());
        return Ok(());
    }
    println!("{} actor(s) in {}:", rows.len(), root.display());
    for row in &rows {
        println!(
            "  {} [{}] pane={} ({}) gen={} {} pid={} session={}",
            row.document_id,
            row.harness,
            row.pane,
            if row.pane_alive { "alive" } else { "dead" },
            row.generation,
            row.state,
            row.supervisor_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".to_string()),
            row.session_id,
        );
    }
    Ok(())
}

/// `agent-doc admin detect` — derived fleet diagnostics.
pub fn detect(project_root: Option<&Path>, json: bool) -> Result<()> {
    let root = resolve_root(project_root)?;
    let actors = crate::project_controller::load_actor_store(&root)?;
    let tmux = Tmux::default_server();
    let findings = detect_findings(&actors, |pane| tmux.pane_alive(pane));

    if json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }
    if findings.is_empty() {
        println!("No fleet anomalies detected in {}", root.display());
        return Ok(());
    }
    println!("{} finding(s) in {}:", findings.len(), root.display());
    for f in &findings {
        println!("  [{}] {}", f.kind, f.detail);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_actor::{ActorLastTransition, ActorRecord};

    fn record(document_id: &str, session_id: &str, pane: &str, state: ActorState) -> ActorRecord {
        ActorRecord {
            document_id: document_id.to_string(),
            session_id: session_id.to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 10,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    fn store(records: Vec<ActorRecord>) -> ActorStore {
        records
            .into_iter()
            .map(|r| (r.document_id.clone(), r))
            .collect()
    }

    fn entry(session_id: &str, pane: &str, pid: u32, cwd: &str) -> sessions::SessionEntry {
        sessions::SessionEntry {
            pane: pane.to_string(),
            pid,
            cwd: cwd.to_string(),
            started: "2026-06-02".to_string(),
            session_id: session_id.to_string(),
            file: String::new(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        }
    }

    #[test]
    fn build_actor_list_enriches_with_registry_and_liveness() {
        let actors = store(vec![
            record("tasks/a.md", "sid-a", "%1", ActorState::Busy),
            record("tasks/b.md", "sid-b", "%2", ActorState::Ready),
        ]);
        let mut registry = SessionRegistry::new();
        registry.insert("a".to_string(), entry("sid-a", "%1", 1001, "/proj"));
        registry.insert("b".to_string(), entry("sid-b", "%2", 1002, "/proj"));

        // %1 alive, %2 dead.
        let rows = build_actor_list(&actors, &registry, |p| p == "%1");
        assert_eq!(rows.len(), 2);

        let a = rows.iter().find(|r| r.document_id == "tasks/a.md").unwrap();
        assert_eq!(a.pane, "%1");
        assert!(a.pane_alive);
        assert_eq!(a.state, "busy");
        assert_eq!(a.supervisor_pid, Some(1001));
        assert_eq!(a.cwd.as_deref(), Some("/proj"));

        let b = rows.iter().find(|r| r.document_id == "tasks/b.md").unwrap();
        assert!(!b.pane_alive, "dead pane must be marked not alive");
        assert_eq!(b.supervisor_pid, Some(1002));
    }

    #[test]
    fn detect_flags_two_documents_sharing_one_live_pane() {
        // The contamination class: agent-doc-bugs2 and lazily-rs both bound to %70.
        let actors = store(vec![
            record("tasks/agent-doc-bugs2.md", "sid-a", "%70", ActorState::Busy),
            record("tasks/lazily-rs.md", "sid-b", "%70", ActorState::Ready),
        ]);
        let findings = detect_findings(&actors, |_| true);
        assert_eq!(findings.len(), 1, "exactly one cross-document finding");
        let f = &findings[0];
        assert_eq!(f.kind, "cross_document_pane");
        assert_eq!(f.pane.as_deref(), Some("%70"));
        assert_eq!(
            f.documents,
            vec![
                "tasks/agent-doc-bugs2.md".to_string(),
                "tasks/lazily-rs.md".to_string()
            ]
        );
    }

    #[test]
    fn detect_flags_non_closed_actor_with_dead_pane() {
        let actors = store(vec![record(
            "tasks/a.md",
            "sid-a",
            "%9",
            ActorState::Busy,
        )]);
        let findings = detect_findings(&actors, |_| false); // pane dead
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "stale_dead_pane");
        assert_eq!(findings[0].pane.as_deref(), Some("%9"));
    }

    #[test]
    fn detect_clean_fleet_reports_nothing() {
        let actors = store(vec![
            record("tasks/a.md", "sid-a", "%1", ActorState::Busy),
            record("tasks/b.md", "sid-b", "%2", ActorState::Ready),
        ]);
        let findings = detect_findings(&actors, |_| true);
        assert!(
            findings.is_empty(),
            "distinct live panes, one owner each → no findings, got {findings:?}"
        );
    }

    #[test]
    fn detect_does_not_flag_cross_document_on_dead_shared_pane() {
        // Two non-closed actors sharing a DEAD pane are stale bindings, not live
        // contention: only `stale_dead_pane` should fire (once per actor), never
        // `cross_document_pane`.
        let actors = store(vec![
            record("tasks/a.md", "sid-a", "%9", ActorState::Ready),
            record("tasks/b.md", "sid-b", "%9", ActorState::Ready),
        ]);
        let findings = detect_findings(&actors, |_| false); // pane dead
        assert!(
            findings.iter().all(|f| f.kind == "stale_dead_pane"),
            "dead shared pane must not produce cross_document_pane, got {findings:?}"
        );
        assert_eq!(
            findings.iter().filter(|f| f.kind == "stale_dead_pane").count(),
            2,
            "each non-closed actor on the dead pane is an orphaned binding"
        );
    }

    #[test]
    fn detect_ignores_closed_actors_sharing_a_pane() {
        // A closed actor that previously used a pane must not count toward the
        // cross-document contention with the live owner that reused the pane.
        let actors = store(vec![
            record("tasks/old.md", "sid-old", "%5", ActorState::Closed),
            record("tasks/new.md", "sid-new", "%5", ActorState::Busy),
        ]);
        let findings = detect_findings(&actors, |_| true);
        assert!(
            findings.is_empty(),
            "closed actor must not contend for the pane, got {findings:?}"
        );
    }
}
