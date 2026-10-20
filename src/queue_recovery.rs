//! Lost queue recovery audit for foreign-owned session documents.
//!
//! The command is intentionally read-only with respect to the session document:
//! it reconstructs historical queue heads from durable local evidence and
//! reports which prompts are unaccounted in the current document. The optional
//! `--restore-patch` emits a separate operator-reviewed restoration patch file
//! (it never mutates the session document itself).

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_element::element;
use agent_doc_queue::document_queue as queue;

const QUEUE_COMPONENT: &str = "queue";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LostQueueReport {
    pub file: String,
    pub current_heads: Vec<String>,
    pub historical_head_count: usize,
    pub source_count: usize,
    pub covered_ids: Vec<String>,
    pub restore_candidates: Vec<RestoreCandidate>,
    pub git_history_only_candidates: Vec<GitHistoryCandidate>,
    pub proof: RecoveryProof,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RestoreCandidate {
    pub text: String,
    pub id: Option<String>,
    pub sources: Vec<String>,
}

/// A historical queue head found only in git history, classified by whether it
/// is safe to restore into this document.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct GitHistoryCandidate {
    pub candidate: RestoreCandidate,
    /// True when the prompt does not reference a foreign document and is a
    /// candidate for operator-reviewed restoration here.
    pub restorable: bool,
    /// Explicit per-candidate guidance (restore hint or non-restorable reason).
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RecoveryProof {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone)]
struct QueueSource {
    name: String,
    content: String,
}

pub(crate) fn run(
    file: &Path,
    json: bool,
    max_git_versions: usize,
    restore_patch: Option<&Path>,
) -> Result<()> {
    let report = build_report(file, max_git_versions)?;
    if let Some(patch_path) = restore_patch {
        let value = restore_patch_value(file, &report);
        fs::write(patch_path, serde_json::to_string_pretty(&value)?).with_context(|| {
            format!("failed to write restore patch to {}", patch_path.display())
        })?;
        if !json {
            println!(
                "restore_patch written={} restorable={} non_restorable={}",
                patch_path.display(),
                value["restorable_count"].as_u64().unwrap_or(0),
                value["non_restorable_count"].as_u64().unwrap_or(0),
            );
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report);
    }
    Ok(())
}

pub(crate) fn build_report(file: &Path, max_git_versions: usize) -> Result<LostQueueReport> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("file not found: {}", file.display()))?;
    let current_doc = fs::read_to_string(&canonical)
        .with_context(|| format!("failed to read {}", canonical.display()))?;
    let current_heads = queue_heads_from_doc(&current_doc, true);
    let current_head_set: BTreeSet<String> = current_heads.iter().cloned().collect();
    let coverage_text = current_coverage_text(&canonical, &current_doc)?;

    let mut sources = Vec::new();
    collect_snapshot_sources(&canonical, &mut sources);
    collect_git_sources(&canonical, max_git_versions, &mut sources);

    let mut historical: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in &sources {
        for head in queue_heads_from_doc(&source.content, false) {
            historical
                .entry(head)
                .or_default()
                .insert(source.name.clone());
        }
    }

    let mut covered_ids = BTreeSet::new();
    let mut restore_candidates = Vec::new();
    let mut git_history_only_candidates = Vec::new();
    for (text, source_names) in &historical {
        if current_head_set.contains(text) {
            continue;
        }
        let id = queue_prompt_id(text);
        if let Some(id) = &id
            && id_is_accounted_for(id, &coverage_text)
        {
            covered_ids.insert(id.clone());
            continue;
        }
        let candidate = RestoreCandidate {
            text: text.clone(),
            id,
            sources: source_names.iter().cloned().collect(),
        };
        if candidate
            .sources
            .iter()
            .any(|source| !source.starts_with("git:"))
        {
            restore_candidates.push(candidate);
        } else {
            let restorable = !is_foreign_owned(&candidate.text, &canonical);
            let recommendation = restore_recommendation(&candidate.text, &canonical, restorable);
            git_history_only_candidates.push(GitHistoryCandidate {
                candidate,
                restorable,
                recommendation,
            });
        }
    }

    let historical_head_count = historical.len();
    let proof = if !restore_candidates.is_empty() {
        RecoveryProof {
            status: "restore_candidates_found".to_string(),
            message: format!(
                "{} historical queue head(s) are not present, completed, or id-accounted in the current document",
                restore_candidates.len()
            ),
        }
    } else if historical_head_count == 0 {
        RecoveryProof {
            status: "no_historical_queue_heads".to_string(),
            message: "No queue heads were found in snapshots, sidecars, or git history; no restore is needed.".to_string(),
        }
    } else if !git_history_only_candidates.is_empty() {
        let restorable = git_history_only_candidates
            .iter()
            .filter(|candidate| candidate.restorable)
            .count();
        let foreign = git_history_only_candidates.len() - restorable;
        RecoveryProof {
            status: "git_history_only_review".to_string(),
            message: format!(
                "{} git-history-only queue head(s) are unaccounted ({} restorable, {} non-restorable/foreign); no current snapshot/baseline/sidecar restore candidate was found.",
                git_history_only_candidates.len(),
                restorable,
                foreign
            ),
        }
    } else {
        RecoveryProof {
            status: "user_removal_or_completion_proof".to_string(),
            message: "Every historical queue head is still queued/completed or its id appears in the current document or done archive; no lost queue content needs restore.".to_string(),
        }
    };

    Ok(LostQueueReport {
        file: canonical.display().to_string(),
        current_heads,
        historical_head_count,
        source_count: sources.len(),
        covered_ids: covered_ids.into_iter().collect(),
        restore_candidates,
        git_history_only_candidates,
        proof,
    })
}

fn print_human_report(report: &LostQueueReport) {
    println!(
        "queue_lost_recovery file={} sources={} historical_heads={} restore_candidates={} proof={}",
        report.file,
        report.source_count,
        report.historical_head_count,
        report.restore_candidates.len(),
        report.proof.status
    );
    println!("{}", report.proof.message);
    if report.restore_candidates.is_empty() {
        if !report.covered_ids.is_empty() {
            println!("covered_ids: {}", report.covered_ids.join(", "));
        }
        if !report.git_history_only_candidates.is_empty() {
            println!("git_history_only_candidates:");
            for candidate in &report.git_history_only_candidates {
                match &candidate.candidate.id {
                    Some(id) => println!("- {} (id: #{})", candidate.candidate.text, id),
                    None => println!("- {}", candidate.candidate.text),
                }
                println!("  sources: {}", candidate.candidate.sources.join(", "));
                let label = if candidate.restorable {
                    "restorable"
                } else {
                    "non-restorable"
                };
                println!("  {label}: {}", candidate.recommendation);
            }
        }
        return;
    }

    println!("restore_candidates:");
    for candidate in &report.restore_candidates {
        match &candidate.id {
            Some(id) => println!("- {} (id: #{})", candidate.text, id),
            None => println!("- {}", candidate.text),
        }
        println!("  sources: {}", candidate.sources.join(", "));
    }
}

fn collect_snapshot_sources(file: &Path, sources: &mut Vec<QueueSource>) {
    if let Ok(Some(content)) = agent_doc_snapshot_io::load_document_baseline(file) {
        sources.push(QueueSource {
            name: "document_baseline".to_string(),
            content,
        });
    }
}

fn collect_git_sources(file: &Path, max_versions: usize, sources: &mut Vec<QueueSource>) {
    if max_versions == 0 {
        return;
    }
    let Ok((git_root, rel_path)) = resolve_git_paths(file) else {
        return;
    };
    let Ok(output) = Command::new("git")
        .current_dir(&git_root)
        .args(["log", "--format=%H", "--", &rel_path])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let commits = String::from_utf8_lossy(&output.stdout);
    for commit in commits.lines().take(max_versions) {
        let Ok(show) = Command::new("git")
            .current_dir(&git_root)
            .args(["show", &format!("{commit}:{rel_path}")])
            .output()
        else {
            continue;
        };
        if !show.status.success() {
            continue;
        }
        let short = &commit[..12.min(commit.len())];
        sources.push(QueueSource {
            name: format!("git:{short}"),
            content: String::from_utf8_lossy(&show.stdout).to_string(),
        });
    }
}

fn resolve_git_paths(file: &Path) -> Result<(PathBuf, String)> {
    let canonical = file.canonicalize()?;
    let parent = canonical.parent().unwrap_or(Path::new("/"));
    let output = Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;
    if !output.status.success() {
        bail!("file is not in a git repository: {}", file.display());
    }
    let git_root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let rel_path = canonical
        .strip_prefix(&git_root)
        .with_context(|| {
            format!(
                "file {} is not under git root {}",
                canonical.display(),
                git_root.display()
            )
        })?
        .to_string_lossy()
        .to_string();
    Ok((git_root, rel_path))
}

fn queue_heads_from_doc(doc: &str, include_completed: bool) -> Vec<String> {
    let Some(body) = queue_component_text(doc) else {
        return Vec::new();
    };
    let Ok(entries) = queue::parse(&body) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut heads = Vec::new();
    for entry in entries {
        let text = match entry {
            queue::QueueEntry::Prompt(prompt) => normalize_prompt_text(&prompt.text),
            queue::QueueEntry::Completed(prompt) if include_completed => {
                normalize_prompt_text(&prompt.text)
            }
            _ => None,
        };
        if let Some(text) = text
            && seen.insert(text.clone())
        {
            heads.push(text);
        }
    }
    heads
}

fn queue_component_text(doc: &str) -> Option<String> {
    let components = element::parse(doc).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == QUEUE_COMPONENT)?;
    Some(queue.content(doc).to_string())
}

fn normalize_prompt_text(text: &str) -> Option<String> {
    let normalized = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!normalized.is_empty()).then_some(normalized)
}

/// Text an id must appear in to count as accounted for: the current document
/// plus every done archive the writer's resolver locates.
///
/// GH #325 (same class): this used to join the raw `agent:done archive=` value
/// onto the document directory, so a project-relative value read
/// `<doc dir>/<project path>`, a bare `archive` read the document directory
/// itself, and a `document:` / `project:` value read a literal
/// `document:x.done.md` file. Every one failed silently and an archived id was
/// reported as lost. Reading through
/// `done_archive::read_done_archive_sources` keeps this audit on the same
/// archive set preflight uses, and an invalid declared target fails the audit
/// instead of under-reporting coverage.
fn current_coverage_text(file: &Path, current_doc: &str) -> Result<String> {
    let mut text = current_doc.to_string();
    let sources =
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, current_doc)
            .with_context(|| format!("failed to read done archives for {}", file.display()))?;
    for source in sources {
        text.push('\n');
        text.push_str(&source.content);
    }
    Ok(text)
}

fn id_is_accounted_for(id: &str, coverage_text: &str) -> bool {
    let bracketed = format!("[#{id}]");
    let bare = format!("#{id}");
    coverage_text.contains(&bracketed) || coverage_text.contains(&bare)
}

fn queue_prompt_id(text: &str) -> Option<String> {
    let hash = text.find('#')?;
    let id: String = text[hash + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    (!id.is_empty()).then(|| id.to_ascii_lowercase())
}

/// Returns true when `text` references a session document other than `file`
/// (e.g. a `sampleorders.md` prompt leaked into this document's queue via
/// cross-document contamination). Such prompts are non-restorable here.
fn is_foreign_owned(text: &str, file: &Path) -> bool {
    foreign_doc_reference(text, file).is_some()
}

/// Extracts the first foreign `<name>.md` reference in `text`, where `<name>`
/// differs from the current document's file stem.
fn foreign_doc_reference(text: &str, file: &Path) -> Option<String> {
    let self_stem = file.file_stem().and_then(|stem| stem.to_str())?;
    for raw in text.split_whitespace() {
        let token =
            raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.');
        if let Some(stem) = token.strip_suffix(".md")
            && !stem.is_empty()
            && stem != self_stem
        {
            return Some(format!("{stem}.md"));
        }
    }
    None
}

fn restore_recommendation(text: &str, file: &Path, restorable: bool) -> String {
    if restorable {
        return "Git-history-only with no foreign-document reference; re-queue \
            into this document after operator review."
            .to_string();
    }
    let foreign =
        foreign_doc_reference(text, file).unwrap_or_else(|| "a foreign document".to_string());
    format!(
        "Non-restorable here: prompt references {foreign} and is likely \
            cross-document contamination; restore it under the owning document."
    )
}

/// Builds the operator-reviewed restoration patch value: restorable candidates
/// (safe to re-queue here) separated from non-restorable foreign ones.
fn restore_patch_value(file: &Path, report: &LostQueueReport) -> Value {
    let mut restorable = Vec::new();
    let mut non_restorable = Vec::new();
    for candidate in &report.git_history_only_candidates {
        if candidate.restorable {
            restorable.push(serde_json::json!({
                "text": candidate.candidate.text,
                "id": candidate.candidate.id,
                "sources": candidate.candidate.sources,
            }));
        } else {
            non_restorable.push(serde_json::json!({
                "text": candidate.candidate.text,
                "id": candidate.candidate.id,
                "recommendation": candidate.recommendation,
            }));
        }
    }
    serde_json::json!({
        "file": file.display().to_string(),
        "restorable_count": restorable.len(),
        "non_restorable_count": non_restorable.len(),
        "restorable": restorable,
        "non_restorable": non_restorable,
        "apply_hint": "Re-queue each restorable prompt into this document's agent:queue \
            block after review, or add it to agent:backlog with the `queue` attribute and run \
            `agent-doc queue sync`. Do not restore non_restorable (foreign) prompts here.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A valid external archive: `archive=done.md` used to stand here, but a
    /// declared target must end in `.done.md`, and the shared resolver fails
    /// closed on anything else.
    fn doc_with_queue(queue_body: &str, extra: &str) -> String {
        doc_with_queue_and_done(queue_body, extra, "archive=session.done.md")
    }

    fn doc_with_queue_and_done(queue_body: &str, extra: &str, done_attrs: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{extra}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Completed\n\n<!-- agent:done {done_attrs} -->\n<!-- /agent:done -->\n"
        )
    }

    const ARCHIVED_ID: &str = "gh325archived";

    /// GH #325 fixture: a nested session document `tasks/a/session.md` whose
    /// git history queued `[#gh325archived]`, now dequeued and recorded only in
    /// the external done archive (not yet written). Returns the project dir
    /// and the document path.
    fn gh325_archived_project(done_attrs: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        fs::create_dir_all(dir.path().join("tasks/a")).unwrap();
        let file = dir.path().join("tasks/a/session.md");
        fs::write(
            &file,
            doc_with_queue_and_done(&format!("- ship it [#{ARCHIVED_ID}]\n"), "", done_attrs),
        )
        .unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["config", "commit.gpgsign", "false"]);
        run_git(dir.path(), &["add", "tasks/a/session.md"]);
        run_git(dir.path(), &["commit", "-m", "queued archived work"]);
        fs::write(&file, doc_with_queue_and_done("", "", done_attrs)).unwrap();
        (dir, file)
    }

    fn write_archive_entry(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!("# Agent Doc Completed Work\n\n- 2026-10-20 [#{ARCHIVED_ID}] ship it\n"),
        )
        .unwrap();
    }

    fn assert_reported_lost(report: &LostQueueReport) {
        assert!(report.covered_ids.is_empty(), "{report:?}");
        assert_eq!(report.git_history_only_candidates.len(), 1, "{report:?}");
        assert_eq!(
            report.git_history_only_candidates[0]
                .candidate
                .id
                .as_deref(),
            Some(ARCHIVED_ID)
        );
        assert_eq!(report.proof.status, "git_history_only_review");
    }

    fn assert_accounted_by_archive(report: &LostQueueReport) {
        assert_eq!(
            report.covered_ids,
            vec![ARCHIVED_ID.to_string()],
            "{report:?}"
        );
        assert!(report.git_history_only_candidates.is_empty(), "{report:?}");
        assert!(report.restore_candidates.is_empty(), "{report:?}");
        assert_eq!(report.proof.status, "user_removal_or_completion_proof");
    }

    /// Before the archive exists the id is lost; once the archive the writer's
    /// resolver names holds it, `recover-lost` accounts for it. The first half
    /// proves the second is not vacuous.
    fn assert_archive_accounts_for_id(done_attrs: &str, archive_rel: &str) {
        let (dir, file) = gh325_archived_project(done_attrs);
        assert_reported_lost(&build_report(&file, 5).unwrap());
        write_archive_entry(&dir.path().join(archive_rel));
        assert_accounted_by_archive(&build_report(&file, 5).unwrap());
    }

    #[test]
    fn recover_lost_reads_project_relative_done_archive() {
        // `history.done.md` is not a `<stem>.done.md` sibling candidate, so
        // only the declared-target resolution can find it.
        assert_archive_accounts_for_id(
            "archive=tasks/a/history.done.md",
            "tasks/a/history.done.md",
        );
    }

    #[test]
    fn recover_lost_ignores_legacy_document_relative_join_of_project_relative_archive() {
        // The old raw join read `<doc dir>/<project-relative value>`. That file
        // is the 0.35.521–0.35.523 legacy location, never completed history.
        let (dir, file) = gh325_archived_project("archive=tasks/a/history.done.md");
        write_archive_entry(&dir.path().join("tasks/a/tasks/a/history.done.md"));
        assert_reported_lost(&build_report(&file, 5).unwrap());
    }

    #[test]
    fn recover_lost_reads_bare_default_done_archive() {
        // A bare `archive` is `<stem>.done.md` beside the document; the raw
        // join read the document directory itself and found nothing.
        assert_archive_accounts_for_id("archive", "tasks/a/session.done.md");
    }

    #[test]
    fn recover_lost_reads_document_anchored_done_archive() {
        assert_archive_accounts_for_id(
            "archive=document:../history.done.md",
            "tasks/history.done.md",
        );
    }

    #[test]
    fn recover_lost_reads_project_anchored_done_archive() {
        assert_archive_accounts_for_id(
            "archive=project:archives/session-history.done.md",
            "archives/session-history.done.md",
        );
    }

    #[test]
    fn recover_lost_fails_closed_on_invalid_done_archive() {
        // `archive=done.md` is not a `.done.md` target. Ignoring it would
        // under-report coverage and offer completed work for restore.
        let (_dir, file) = gh325_archived_project("archive=done.md");
        let err = build_report(&file, 5).unwrap_err();
        assert!(
            format!("{err:#}").contains("must point to a .done.md file"),
            "{err:#}"
        );
    }

    #[test]
    fn recover_lost_queue_reads_git_history() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        fs::write(&file, doc_with_queue("- do [#fromgit]\n", "")).unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["add", "session.md"]);
        run_git(dir.path(), &["commit", "-m", "initial queue"]);
        fs::write(&file, doc_with_queue("", "")).unwrap();

        let report = build_report(&file, 5).unwrap();
        assert!(report.restore_candidates.is_empty());
        assert_eq!(report.git_history_only_candidates.len(), 1);
        let candidate = &report.git_history_only_candidates[0];
        assert_eq!(candidate.candidate.text, "do [#fromgit]");
        assert!(
            candidate
                .candidate
                .sources
                .iter()
                .any(|source| source.starts_with("git:"))
        );
        // No foreign-document reference → restorable, with a restore hint.
        assert!(candidate.restorable);
        assert!(candidate.recommendation.contains("re-queue"));
    }

    #[test]
    fn recover_lost_classifies_foreign_git_prompt_as_non_restorable() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        fs::write(
            &file,
            doc_with_queue("- review the sampleorders.md test-email CSV row\n", ""),
        )
        .unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["add", "session.md"]);
        run_git(dir.path(), &["commit", "-m", "foreign prompt"]);
        fs::write(&file, doc_with_queue("", "")).unwrap();

        let report = build_report(&file, 5).unwrap();
        assert_eq!(report.git_history_only_candidates.len(), 1);
        let candidate = &report.git_history_only_candidates[0];
        assert!(!candidate.restorable);
        assert!(candidate.recommendation.contains("Non-restorable"));
        assert!(candidate.recommendation.contains("sampleorders.md"));
    }

    #[test]
    fn recover_lost_restore_patch_separates_restorable_from_foreign() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        fs::write(
            &file,
            doc_with_queue("- do [#fromgit]\n- review sampleorders.md CSV row\n", ""),
        )
        .unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["add", "session.md"]);
        run_git(dir.path(), &["commit", "-m", "mixed queue"]);
        fs::write(&file, doc_with_queue("", "")).unwrap();

        let report = build_report(&file, 5).unwrap();
        let value = restore_patch_value(&file, &report);
        assert_eq!(value["restorable_count"].as_u64(), Some(1));
        assert_eq!(value["non_restorable_count"].as_u64(), Some(1));
        // The restorable entry is the non-foreign prompt.
        assert_eq!(value["restorable"][0]["text"], "do [#fromgit]");
        // The foreign entry carries a non-restorable recommendation.
        assert!(
            value["non_restorable"][0]["recommendation"]
                .as_str()
                .unwrap()
                .contains("Non-restorable")
        );
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }
}
