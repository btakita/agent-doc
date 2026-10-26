//! `agent-doc queue add` — supported inbound queue append for external tools
//! (`#queueinbound`, GH #237).
//!
//! An editor plugin, a game-engine play-mode bridge, or a CI watcher appends an
//! attributed free-text head (`[from:<source>] <text>`) to a session's
//! `agent:queue` through the SAME binary-owned write path backlog→queue mirrors
//! use (`QueueCommandEffects::converge_document_or_disk`): while an editor holds
//! the document the change lands in the live CRDT replica, never as a disk write
//! behind the editor buffer. The pure head shape and dedupe rule live in
//! `agent_doc_queue::queue_inbound`; this module adds:
//!
//! - secret redaction (`agent_doc_secret_redact::redact`) of the text and of each
//!   attachment before anything is written;
//! - attachment storage: each `--attach <PATH>` is copied, redacted, to
//!   `<project>/.agent-doc/inbound/<content-hash>-<name>` and referenced from the
//!   head as `(attachment: .agent-doc/inbound/...)`, so a multi-line log never
//!   enters the queue line itself;
//! - the queue-mutation lease (`QueueEditGuard`) around the read-plan-write, so
//!   preflight maintenance and the supervisor idle watch defer to this writer.
//!
//! The snapshot is deliberately NOT checkpointed: an inbound head is new queue
//! input exactly like an operator-typed line, so preflight lists it in
//! `queue_prompts` and mid-turn steering reports it as a queue addition
//! (`dispatch=drain_after_current`, unless subagent intent upgrades it).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::queue_cmd::QueueCommandEffects;
use agent_doc_queue::queue_inbound::{self, InboundAppendPlan};

/// Largest attachment `queue add` accepts.
pub const MAX_INBOUND_ATTACHMENT_BYTES: u64 = 1024 * 1024;

/// Project-relative directory holding redacted attachment copies.
pub const INBOUND_ATTACHMENT_DIR: &str = ".agent-doc/inbound";

#[derive(Debug, Clone)]
pub struct QueueAddRequest<'a> {
    pub source: &'a str,
    pub key: Option<&'a str>,
    pub text: &'a str,
    pub attachments: &'a [PathBuf],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum QueueAddOutcome {
    /// The head was appended to `agent:queue`.
    Added {
        head: String,
        attachments: Vec<String>,
    },
    /// A live head already carries this source+key (or identical text).
    Duplicate { existing: String },
}

fn project_root_for(file: &Path) -> PathBuf {
    agent_doc_fs::find_project_root_canonical(file)
        .or_else(|| agent_doc_fs::find_project_root(file))
        .unwrap_or_else(|| {
            file.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn sanitized_attachment_name(path: &Path) -> String {
    let raw = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".to_string());
    let cleaned: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('.').is_empty() {
        "attachment".to_string()
    } else {
        cleaned
    }
}

/// Copy one attachment, redacted, into the project inbound directory and return
/// its project-relative path. The name is content-addressed, so a retried add
/// reuses the same file and renders the same head.
pub fn store_redacted_attachment(project_root: &Path, path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("queue add: cannot read attachment {}", path.display()))?;
    if !metadata.is_file() {
        bail!("queue add: attachment {} is not a file", path.display());
    }
    if metadata.len() > MAX_INBOUND_ATTACHMENT_BYTES {
        bail!(
            "queue add: attachment {} is {} bytes; the limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_INBOUND_ATTACHMENT_BYTES
        );
    }
    let bytes = std::fs::read(path)
        .with_context(|| format!("queue add: cannot read attachment {}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!(
            "queue add: attachment {} is not UTF-8 text; only text attachments are redactable",
            path.display()
        )
    })?;
    let redacted = agent_doc_secret_redact::redact(&text);
    let short = agent_doc_hash::short_content_hash(&redacted);
    let name = format!("{short}-{}", sanitized_attachment_name(path));
    let dir = project_root.join(INBOUND_ATTACHMENT_DIR);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("queue add: cannot create {}", dir.display()))?;
    let target = dir.join(&name);
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, redacted.as_bytes())
        .with_context(|| format!("queue add: cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &target).with_context(|| {
        format!(
            "queue add: cannot move attachment into {}",
            target.display()
        )
    })?;
    Ok(format!("{INBOUND_ATTACHMENT_DIR}/{name}"))
}

/// Append one attributed inbound head to `file`'s `agent:queue`.
pub fn add_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    request: &QueueAddRequest<'_>,
) -> Result<QueueAddOutcome> {
    queue_inbound::validate_inbound_token("source", request.source)?;
    if let Some(key) = request.key {
        queue_inbound::validate_inbound_token("key", key)?;
    }
    let text =
        queue_inbound::normalize_inbound_text(&agent_doc_secret_redact::redact(request.text))?;
    let project_root = project_root_for(file);
    let attachments = request
        .attachments
        .iter()
        .map(|path| store_redacted_attachment(&project_root, path))
        .collect::<Result<Vec<_>>>()?;
    let head = queue_inbound::render_inbound_head(request.source, request.key, &text, &attachments);

    // #sqedit-race: one queue writer at a time; preflight maintenance and the
    // supervisor idle watch defer while this lease is held.
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "queue_add")?;
    let plan = queue_inbound::plan_inbound_append(&current, &head, request.source, request.key)
        .with_context(|| format!("queue add: {}", file.display()))?;
    match plan {
        InboundAppendPlan::Duplicate { existing } => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_add outcome=duplicate source={} key={}",
                    request.source,
                    request.key.unwrap_or("-")
                ),
            );
            Ok(QueueAddOutcome::Duplicate { existing })
        }
        InboundAppendPlan::Append { content, head } => {
            effects.converge_document_or_disk(file, &content, &current, "queue_add")?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_add outcome=added source={} key={} attachments={} head_hash={}",
                    request.source,
                    request.key.unwrap_or("-"),
                    attachments.len(),
                    agent_doc_hash::content_hash(&head)
                ),
            );
            Ok(QueueAddOutcome::Added { head, attachments })
        }
    }
}

/// Insert worker-produced follow-ups after their exact originating head in one
/// identified queue, through the same editor/CRDT authority as other queue
/// mutations.
pub fn add_followups_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: &str,
    origin: &str,
    followups: &[String],
) -> Result<usize> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "queue_followup")?;
    let plan = queue_inbound::plan_queue_followups_targeted(&current, queue_id, origin, followups)
        .with_context(|| format!("queue follow-up: {}", file.display()))?;
    effects.converge_document_or_disk(file, &plan.content, &current, "queue_followup")?;
    let target_queue_id = plan.queue_id.as_str();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_followup_write queue_id={target_queue_id} origin_hash={} count={} redirected_from_loop={} (#loopnoinsert)",
            agent_doc_hash::content_hash(origin),
            followups.len(),
            plan.redirected_from_loop.as_deref().unwrap_or("-"),
        ),
    );
    if let Some(loop_id) = plan.redirected_from_loop.as_deref() {
        eprintln!(
            "[queue] follow-up origin is in agent:loop id `{loop_id}`; added {} follow-up(s) to agent:run id `{target_queue_id}` instead (loops never receive follow-ups)",
            followups.len()
        );
    }
    // `#capturegatedispatched`: follow-up heads are tracked work, so they meet
    // this cycle's generic backlog-capture requirement. The heads already
    // landed; a failed mark only leaves the gate armed, so it must not fail
    // (and invite a duplicating retry of) the follow-up write.
    if let Err(err) = agent_doc_cycle_state_io::mark_queue_followups_added(file) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("queue_followup_capture_mark_failed queue_id={queue_id} error={err:#}"),
        );
    }
    Ok(followups.len())
}

/// CLI entry: run [`add_with_effects`] and print the outcome.
pub fn add(
    effects: &impl QueueCommandEffects,
    file: &Path,
    request: &QueueAddRequest<'_>,
    json: bool,
) -> Result<()> {
    let outcome = add_with_effects(effects, file, request)?;
    if json {
        println!("{}", serde_json::to_string(&outcome)?);
        return Ok(());
    }
    match outcome {
        QueueAddOutcome::Added { head, .. } => {
            println!("{}: queued `{head}`", file.display())
        }
        QueueAddOutcome::Duplicate { existing } => println!(
            "{}: not queued; a live head already covers it: `{existing}`",
            file.display()
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_cmd::QueueCommandConsumeOutcome;

    /// Detached-document effects: the plain disk read/write the runtime uses when
    /// no editor holds the document. The live-editor path is covered by the
    /// SimWorld test in the root crate.
    struct DiskEffects;

    impl QueueCommandEffects for DiskEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }
        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            Ok(std::fs::write(file, target_content)?)
        }
        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!("queue add never consumes")
        }
        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!("queue add never consumes")
        }
        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!("queue add never strikes")
        }
        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!("queue add never acknowledges")
        }
        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!("queue add never acknowledges")
        }
        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            unreachable!("queue add never prunes")
        }
    }

    fn project(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    const DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:queue go -->\n",
        "- current task\n",
        "<!-- /agent:queue -->\n",
    );

    // Assembled at runtime so the source never carries a token-shaped literal.
    fn fake_github_token() -> String {
        format!("ghp_{}", "A".repeat(36))
    }

    #[test]
    fn same_key_twice_produces_one_head() {
        let (_dir, doc) = project(DOC);
        let request = QueueAddRequest {
            source: "ci",
            key: Some("build-42"),
            text: "build 42 failed",
            attachments: &[],
        };
        let first = add_with_effects(&DiskEffects, &doc, &request).unwrap();
        assert!(matches!(first, QueueAddOutcome::Added { .. }), "{first:?}");
        let retry = QueueAddRequest {
            text: "build 42 failed (retry wording)",
            ..request.clone()
        };
        let second = add_with_effects(&DiskEffects, &doc, &retry).unwrap();
        assert_eq!(
            second,
            QueueAddOutcome::Duplicate {
                existing: "[from:ci key=build-42] build 42 failed".into()
            }
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches("key=build-42").count(), 1, "{content}");
    }

    #[test]
    fn secret_in_text_and_attachment_is_redacted_before_writing() {
        let (dir, doc) = project(DOC);
        let token = fake_github_token();
        let log = dir.path().join("test output.log");
        std::fs::write(&log, format!("line 1\nGITHUB token {token}\nline 3\n")).unwrap();
        let text = format!("PlayerSpec failed, token {token}");
        let outcome = add_with_effects(
            &DiskEffects,
            &doc,
            &QueueAddRequest {
                source: "unity",
                key: None,
                text: &text,
                attachments: std::slice::from_ref(&log),
            },
        )
        .unwrap();
        let QueueAddOutcome::Added { head, attachments } = outcome else {
            panic!("expected add");
        };
        assert_eq!(attachments.len(), 1);
        assert!(attachments[0].starts_with(".agent-doc/inbound/"));
        assert!(
            attachments[0].ends_with("-test_output.log"),
            "{attachments:?}"
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(!content.contains(&token), "{content}");
        assert!(content.contains("[REDACTED_GITHUB]"), "{content}");
        assert!(content.contains(&format!("- {head}\n")), "{content}");
        let stored = std::fs::read_to_string(dir.path().join(&attachments[0])).unwrap();
        assert!(!stored.contains(&token), "{stored}");
        assert!(stored.contains("[REDACTED_GITHUB]"), "{stored}");
        assert!(stored.contains("line 3"), "{stored}");
    }

    /// `#capturegatedispatched`: inserted follow-up heads are recorded on the
    /// open cycle as tracked capture, so the closeout backlog-capture gate
    /// does not refuse a cycle whose follow-ups went to the queue.
    #[test]
    fn followups_record_queue_capture_on_the_open_cycle() {
        let content = DOC.replace("<!-- agent:queue go -->", "<!-- agent:queue id=main go -->");
        let (_dir, doc) = project(&content);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        let before = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(!before.queue_followups_added_this_cycle);

        let count = add_followups_with_effects(
            &DiskEffects,
            &doc,
            "main",
            "current task",
            &["follow-up from the worker".to_string()],
        )
        .unwrap();
        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- follow-up from the worker"), "{content}");
        let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(after.queue_followups_added_this_cycle, "{after:?}");
        assert!(
            !after.had_pending_mutations,
            "not a backlog mutation: {after:?}"
        );
    }

    #[test]
    fn invalid_source_is_refused_without_writing() {
        let (_dir, doc) = project(DOC);
        let err = add_with_effects(
            &DiskEffects,
            &doc,
            &QueueAddRequest {
                source: "bad source",
                key: None,
                text: "x",
                attachments: &[],
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("--source"), "{err}");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), DOC);
    }
}
