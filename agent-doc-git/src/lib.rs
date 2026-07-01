//! Pure git command and path policy helpers.

use agent_doc_document::commit_normalization::{
    normalize_committed_exchange_artifacts, normalize_component_content_for_absorb,
    redact_component_contents_for_absorb,
};
use agent_doc_document::transient_markers::{
    normalize_post_commit_re_heading_drift, normalize_transient_agent_doc_markers,
};
use agent_doc_document_realtime::write_policy::is_safe_user_follow_up_exchange_growth;
use agent_doc_element::element::is_backlog_component;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

/// A pre-mutation recovery checkpoint tag for a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryTag {
    pub name: String,
    pub slug: String,
    pub ordinal: u64,
    pub short_sha: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmodulePointerDrift {
    pub relative_path: String,
    pub parent_head: Option<String>,
    pub submodule_head: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostCommitLocalDriftKind {
    UserFollowUp,
    WorkingTreeEdits,
}

impl PostCommitLocalDriftKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserFollowUp => "user_follow_up",
            Self::WorkingTreeEdits => "working_tree_edits",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::UserFollowUp => "later local user follow-up edits",
            Self::WorkingTreeEdits => "later local working-tree edits",
        }
    }
}

/// Compute `path` relative to `root`, canonicalizing both sides so symlinks do
/// not cause `strip_prefix` mismatches. Falls back through non-canonical strip
/// and finally to the original path.
pub fn relative_to_root(path: &Path, root: &Path) -> PathBuf {
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(rel) = canon_path.strip_prefix(&canon_root) {
        return rel.to_path_buf();
    }
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.to_path_buf();
    }
    path.to_path_buf()
}

pub fn is_index_lock_contention_text(text: &str) -> bool {
    text.contains("index.lock") || text.contains("Unable to create")
}

pub fn render_git_process_output(output: &Output) -> String {
    render_git_streams(&output.stderr, &output.stdout)
}

pub fn output_has_index_lock_contention(output: &Output) -> bool {
    is_index_lock_contention_text(&String::from_utf8_lossy(&output.stderr))
        || is_index_lock_contention_text(&String::from_utf8_lossy(&output.stdout))
}

pub fn commit_retry_backoff(attempt: u32) -> Duration {
    Duration::from_millis(50 * (1u64 << attempt))
}

/// Parse the path field from non-`-z` `git status --porcelain=v1` output.
pub fn parse_porcelain_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let status = line.get(..2)?;
    if status == "??" {
        return None;
    }
    let raw = line.get(3..)?.trim();
    if raw.is_empty() {
        return None;
    }
    let path = raw.rsplit(" -> ").next().unwrap_or(raw).trim();
    Some(path.trim_matches('"').to_string())
}

pub fn tracked_modified_paths_from_porcelain(
    porcelain: &str,
    submodule_paths: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut paths = Vec::new();
    for line in porcelain.lines() {
        let Some(path) = parse_porcelain_path(line) else {
            continue;
        };
        if path.is_empty() || submodule_paths.contains(&path) {
            continue;
        }
        paths.push(path);
    }
    paths.sort();
    paths.dedup();
    paths
}

pub fn parse_submodule_paths(status_output: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for line in status_output.lines() {
        // ` <sha> <path> (<describe>)` / `+<sha> <path>` / `-<sha> <path>` /
        // `U<sha> <path>` — the path is the second whitespace-separated field.
        if let Some(path) = line
            .trim_start_matches(['+', '-', 'U', ' '])
            .split_whitespace()
            .nth(1)
        {
            set.insert(path.to_string());
        }
    }
    set
}

pub fn line_looks_like_explicit_post_commit_prompt_directive(line: &str) -> bool {
    let mut trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed
        .strip_prefix("[ ]")
        .or_else(|| trimmed.strip_prefix("[x]"))
        .or_else(|| trimmed.strip_prefix("[X]"))
        .or_else(|| trimmed.strip_prefix("[/]"))
    {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("[#")
        && let Some(close) = rest.find(']')
    {
        trimmed = rest[close + 1..].trim_start();
    }

    let lower = trimmed
        .trim_start_matches('\u{276f}')
        .trim_start()
        .to_ascii_lowercase();
    trimmed.starts_with('\u{276f}')
        || trimmed.ends_with('?')
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("fix this")
        || lower.starts_with("run tests")
        || lower.starts_with("build + install")
        || lower.starts_with("build and install")
        || lower.starts_with("commit + push")
        || lower.starts_with("commit and push")
        || lower.contains(" spec-test")
        || lower.contains(" spec test")
}

pub fn classify_prompt_bearing_post_commit_drift(
    has_changes: bool,
    has_explicit_prompt_target: bool,
    has_content_edit: bool,
    has_recovery_artifact: bool,
) -> Option<PostCommitLocalDriftKind> {
    if !has_changes {
        return None;
    }
    if has_explicit_prompt_target && !has_content_edit && !has_recovery_artifact {
        Some(PostCommitLocalDriftKind::UserFollowUp)
    } else {
        Some(PostCommitLocalDriftKind::WorkingTreeEdits)
    }
}

pub fn classify_post_commit_local_drift_from_checks(
    contents_equal: bool,
    transient_only: bool,
    re_heading_only: bool,
    safe_user_follow_up: bool,
    prompt_classifier_kind: Option<PostCommitLocalDriftKind>,
) -> Option<PostCommitLocalDriftKind> {
    if contents_equal || transient_only || re_heading_only {
        return None;
    }
    if safe_user_follow_up {
        return Some(PostCommitLocalDriftKind::UserFollowUp);
    }
    prompt_classifier_kind.or(Some(PostCommitLocalDriftKind::WorkingTreeEdits))
}

pub fn is_safe_user_only_follow_up_after_committed_head(head_doc: &str, current_doc: &str) -> bool {
    if head_doc == current_doc {
        return false;
    }

    let head_body = agent_doc_frontmatter::frontmatter::parse(head_doc)
        .map(|(_, body)| body)
        .unwrap_or(head_doc);
    let current_body = agent_doc_frontmatter::frontmatter::parse(current_doc)
        .map(|(_, body)| body)
        .unwrap_or(current_doc);

    if redact_component_contents_for_absorb(head_body)
        != redact_component_contents_for_absorb(current_body)
    {
        return false;
    }

    let Ok(head_components) = agent_doc_element::element::parse(head_body) else {
        return false;
    };
    let Ok(current_components) = agent_doc_element::element::parse(current_body) else {
        return false;
    };
    if head_components.len() != current_components.len() {
        return false;
    }

    let mut saw_exchange = false;

    for (head_comp, current_comp) in head_components.iter().zip(current_components.iter()) {
        if head_comp.name != current_comp.name {
            return false;
        }
        // Backlog/pending: tolerate patch attr differences (deprecated attr being stripped).
        if !is_backlog_component(&head_comp.name)
            && head_comp.patch_mode() != current_comp.patch_mode()
        {
            return false;
        }

        let head_content = normalize_component_content_for_absorb(head_comp.content(head_body));
        let current_content =
            normalize_component_content_for_absorb(current_comp.content(current_body));
        if head_content == current_content {
            continue;
        }

        match head_comp.name.as_str() {
            "exchange" => {
                if !is_safe_user_follow_up_exchange_growth(&head_content, &current_content) {
                    return false;
                }
                saw_exchange = true;
            }
            _ => return false,
        }
    }

    saw_exchange
}

fn prompt_classifier_post_commit_drift_kind(
    head_doc: &str,
    current_doc: &str,
) -> Option<PostCommitLocalDriftKind> {
    let prompt_bearing_body = |content: &str| {
        agent_doc_frontmatter::frontmatter::parse(content)
            .map(|(_, body)| body.to_string())
            .unwrap_or_else(|_| content.to_string())
    };
    let norm =
        |content: &str| normalize_committed_exchange_artifacts(&prompt_bearing_body(content));
    let diff_text =
        agent_doc_diff::unified_diff_from_contents(&norm(head_doc), &norm(current_doc))?;
    let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff_text);
    if changes.is_empty() {
        return None;
    }
    let has_explicit_prompt_target = changes
        .iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
        .any(|change| {
            change
                .text
                .lines()
                .any(line_looks_like_explicit_post_commit_prompt_directive)
        });
    let has_content_edit = changes
        .iter()
        .any(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::ContentEdit);
    let has_recovery_artifact = changes
        .iter()
        .any(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact);
    classify_prompt_bearing_post_commit_drift(
        !changes.is_empty(),
        has_explicit_prompt_target,
        has_content_edit,
        has_recovery_artifact,
    )
}

pub fn classify_post_commit_local_drift(
    head_doc: &str,
    current_doc: &str,
) -> Option<PostCommitLocalDriftKind> {
    let contents_equal = head_doc == current_doc;
    let transient_only = !contents_equal
        && normalize_transient_agent_doc_markers(current_doc)
            == normalize_transient_agent_doc_markers(head_doc);
    let re_heading_only = !contents_equal
        && !transient_only
        && normalize_post_commit_re_heading_drift(current_doc)
            == normalize_post_commit_re_heading_drift(head_doc);
    let safe_user_follow_up = !contents_equal
        && !transient_only
        && !re_heading_only
        && (is_safe_user_only_follow_up_after_committed_head(head_doc, current_doc)
            || (if let Ok(Some(cleaned_head)) =
                agent_doc_template::strip_conversation_tail_outside_exchange(head_doc)
            {
                is_safe_user_only_follow_up_after_committed_head(&cleaned_head, current_doc)
            } else {
                false
            }));
    let prompt_classifier_kind =
        (!contents_equal && !transient_only && !re_heading_only && !safe_user_follow_up)
            .then(|| prompt_classifier_post_commit_drift_kind(head_doc, current_doc))
            .flatten();

    classify_post_commit_local_drift_from_checks(
        contents_equal,
        transient_only,
        re_heading_only,
        safe_user_follow_up,
        prompt_classifier_kind,
    )
}

pub fn doc_stem(file: &Path) -> String {
    doc_stem_or(file, "doc")
}

pub fn doc_stem_or(file: &Path, fallback: &str) -> String {
    file.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback)
        .to_string()
}

pub fn agent_doc_commit_message_for_file(file: &Path, timestamp: &str) -> String {
    format!("agent-doc({}): {}", doc_stem_or(file, "unknown"), timestamp)
}

pub fn agent_doc_branch_name_for_file(file: &Path) -> String {
    format!("agent-doc/{}", doc_stem_or(file, "session"))
}

pub fn parent_submodule_pointer_commit_message(message: &str) -> String {
    format!("{message} (submodule pointer)")
}

pub fn short_oid(value: Option<&str>) -> String {
    value
        .map(|oid| oid.chars().take(12).collect::<String>())
        .filter(|oid| !oid.is_empty())
        .unwrap_or_else(|| "<missing>".to_string())
}

pub fn parent_pointer_recovery_hint(file_display: &str) -> String {
    format!(
        "Use `agent-doc commit {file_display}` to finish the missing parent pointer commit, then re-run `agent-doc session-check {file_display}`."
    )
}

pub fn parent_submodule_pointer_message(
    relative_path: &str,
    parent_head: Option<&str>,
    submodule_head: &str,
    file_display: &str,
) -> String {
    format!(
        "parent submodule pointer is not committed for {relative_path} (parent HEAD {}, submodule HEAD {}). The response patchback crossed the submodule repo but not the parent commit boundary. {}",
        short_oid(parent_head),
        short_oid(Some(submodule_head)),
        parent_pointer_recovery_hint(file_display)
    )
}

/// Parse checkpoint tag lines into `RecoveryTag`s, newest-first.
///
/// `tag_lines` is raw `git tag -l agent-doc/<stem>/*` output; `meta` resolves
/// `(short_sha, date, subject)` for each accepted tag name so parsing stays
/// independent from a live repository.
pub fn parse_recovery_tags(
    tag_lines: &str,
    meta: &mut dyn FnMut(&str) -> (String, String, String),
) -> Vec<RecoveryTag> {
    let mut tags = Vec::new();
    for name in tag_lines
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
    {
        let Some((prefix, ord)) = name.rsplit_once('-') else {
            continue;
        };
        if !(prefix.ends_with("pre-auto-run") || prefix.ends_with("pre-compact")) {
            continue;
        }
        let Ok(ordinal) = ord.parse::<u64>() else {
            continue;
        };
        let slug = prefix.rsplit('/').next().unwrap_or(prefix).to_string();
        let (short_sha, date, subject) = meta(name);
        tags.push(RecoveryTag {
            name: name.to_string(),
            slug,
            ordinal,
            short_sha,
            date,
            subject,
        });
    }
    tags.sort_by(|a, b| b.date.cmp(&a.date).then(b.ordinal.cmp(&a.ordinal)));
    tags
}

fn render_git_streams(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, true) => stderr,
        (true, false) => stdout,
        (false, false) => format!("{} | {}", stderr, stdout),
        (true, true) => "no git output".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PostCommitLocalDriftKind, agent_doc_branch_name_for_file,
        agent_doc_commit_message_for_file, classify_post_commit_local_drift,
        classify_post_commit_local_drift_from_checks, classify_prompt_bearing_post_commit_drift,
        commit_retry_backoff, doc_stem, doc_stem_or, is_index_lock_contention_text,
        is_safe_user_only_follow_up_after_committed_head,
        line_looks_like_explicit_post_commit_prompt_directive, output_has_index_lock_contention,
        parent_pointer_recovery_hint, parent_submodule_pointer_commit_message,
        parent_submodule_pointer_message, parse_porcelain_path, parse_recovery_tags,
        parse_submodule_paths, relative_to_root, render_git_streams, short_oid,
        tracked_modified_paths_from_porcelain,
    };
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::process::{ExitStatus, Output};

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[test]
    fn relative_to_root_strips_prefix_for_normal_paths() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/project/src/main.rs");
        let rel = relative_to_root(file, root);
        assert_eq!(rel, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn relative_to_root_returns_original_when_no_common_prefix() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/other/path/file.rs");
        let rel = relative_to_root(file, root);
        assert_eq!(rel, PathBuf::from("/other/path/file.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn relative_to_root_handles_symlinked_path() {
        let real_dir = tempfile::TempDir::new().unwrap();
        let link_dir = tempfile::TempDir::new().unwrap();
        let real_root = real_dir.path();
        let link_path = link_dir.path().join("link");

        let subdir = real_root.join("tasks");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join("doc.md"), "content").unwrap();
        std::os::unix::fs::symlink(real_root, &link_path).unwrap();

        let file_via_symlink = link_path.join("tasks/doc.md");
        assert!(file_via_symlink.exists());

        let rel = relative_to_root(&file_via_symlink, real_root);
        assert_eq!(rel, PathBuf::from("tasks/doc.md"));
    }

    #[test]
    fn index_lock_contention_matches_git_lock_messages() {
        assert!(is_index_lock_contention_text(
            "fatal: Unable to create '/repo/.git/index.lock': File exists."
        ));
        assert!(is_index_lock_contention_text(
            "error: could not write index.lock"
        ));
        assert!(!is_index_lock_contention_text(
            "fatal: not a git repository"
        ));
    }

    #[test]
    fn render_git_streams_prefers_meaningful_output() {
        assert_eq!(render_git_streams(b"fatal\n", b""), "fatal");
        assert_eq!(render_git_streams(b"", b"ok\n"), "ok");
        assert_eq!(render_git_streams(b"fatal\n", b"hint\n"), "fatal | hint");
        assert_eq!(render_git_streams(b" \n", b"\n"), "no git output");
    }

    #[cfg(unix)]
    #[test]
    fn output_has_index_lock_contention_checks_both_streams() {
        let stderr_lock = Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: b"fatal: Unable to create .git/index.lock".to_vec(),
        };
        assert!(output_has_index_lock_contention(&stderr_lock));

        let stdout_lock = Output {
            status: exit_status(1),
            stdout: b"could not write index.lock".to_vec(),
            stderr: Vec::new(),
        };
        assert!(output_has_index_lock_contention(&stdout_lock));

        let unrelated = Output {
            status: exit_status(1),
            stdout: b"fatal: not a git repository".to_vec(),
            stderr: Vec::new(),
        };
        assert!(!output_has_index_lock_contention(&unrelated));
    }

    #[test]
    fn commit_retry_backoff_doubles_from_one_hundred_milliseconds() {
        assert_eq!(commit_retry_backoff(0).as_millis(), 50);
        assert_eq!(commit_retry_backoff(1).as_millis(), 100);
        assert_eq!(commit_retry_backoff(3).as_millis(), 400);
    }

    #[test]
    fn porcelain_path_parses_tracked_status_path() {
        assert_eq!(
            parse_porcelain_path(" M src/lib.rs"),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(
            parse_porcelain_path("M  src/main.rs"),
            Some("src/main.rs".to_string())
        );
    }

    #[test]
    fn porcelain_path_ignores_untracked_and_empty_paths() {
        assert_eq!(parse_porcelain_path("?? scratch.md"), None);
        assert_eq!(parse_porcelain_path(" M   "), None);
        assert_eq!(parse_porcelain_path("M"), None);
    }

    #[test]
    fn porcelain_path_uses_destination_for_rename_or_copy() {
        assert_eq!(
            parse_porcelain_path("R  old/name.rs -> new/name.rs"),
            Some("new/name.rs".to_string())
        );
        assert_eq!(
            parse_porcelain_path("C  old/name.rs -> new/name.rs"),
            Some("new/name.rs".to_string())
        );
    }

    #[test]
    fn porcelain_path_trims_surrounding_quotes() {
        assert_eq!(
            parse_porcelain_path(" M \"src/lib.rs\""),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(
            parse_porcelain_path("R  \"old name.rs\" -> \"new name.rs\""),
            Some("new name.rs".to_string())
        );
    }

    #[test]
    fn tracked_modified_paths_from_porcelain_filters_sorts_and_dedupes() {
        let mut submodules = HashSet::new();
        submodules.insert("vendor/tool".to_string());
        let paths = tracked_modified_paths_from_porcelain(
            " M src/lib.rs\n\
             ?? scratch.md\n\
             R  old.rs -> new.rs\n\
             M  vendor/tool\n\
             M  src/lib.rs\n",
            &submodules,
        );
        assert_eq!(paths, vec!["new.rs".to_string(), "src/lib.rs".to_string()]);
    }

    #[test]
    fn parse_submodule_paths_reads_git_status_shapes() {
        let paths = parse_submodule_paths(
            " 0123456789abcdef vendor/clean (v1.0)\n\
             +abcdef0123456789 vendor/dirty (heads/main)\n\
             -fedcba9876543210 vendor/missing\n\
             U1111111111111111 vendor/conflict\n",
        );
        assert!(paths.contains("vendor/clean"));
        assert!(paths.contains("vendor/dirty"));
        assert!(paths.contains("vendor/missing"));
        assert!(paths.contains("vendor/conflict"));
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn explicit_post_commit_prompt_directive_detects_operator_prompts() {
        assert!(line_looks_like_explicit_post_commit_prompt_directive(
            "- [ ] [#abc] fix this regression"
        ));
        assert!(line_looks_like_explicit_post_commit_prompt_directive(
            "\u{276f} run tests"
        ));
        assert!(line_looks_like_explicit_post_commit_prompt_directive(
            "Can you check this?"
        ));
        assert!(line_looks_like_explicit_post_commit_prompt_directive(
            "- [x] build + install"
        ));
        assert!(!line_looks_like_explicit_post_commit_prompt_directive(
            "- [ ] [#abc] completed closeout bookkeeping"
        ));
    }

    #[test]
    fn prompt_bearing_post_commit_drift_classifies_prompt_only_changes() {
        assert_eq!(
            classify_prompt_bearing_post_commit_drift(false, false, false, false),
            None
        );
        assert_eq!(
            classify_prompt_bearing_post_commit_drift(true, true, false, false),
            Some(PostCommitLocalDriftKind::UserFollowUp)
        );
        assert_eq!(
            classify_prompt_bearing_post_commit_drift(true, true, true, false),
            Some(PostCommitLocalDriftKind::WorkingTreeEdits)
        );
        assert_eq!(
            classify_prompt_bearing_post_commit_drift(true, false, false, false),
            Some(PostCommitLocalDriftKind::WorkingTreeEdits)
        );
    }

    #[test]
    fn post_commit_local_drift_policy_honors_normalization_and_prompt_classifier() {
        assert_eq!(
            classify_post_commit_local_drift_from_checks(true, false, false, false, None),
            None
        );
        assert_eq!(
            classify_post_commit_local_drift_from_checks(false, true, false, true, None),
            None
        );
        assert_eq!(
            classify_post_commit_local_drift_from_checks(false, false, true, true, None),
            None
        );
        assert_eq!(
            classify_post_commit_local_drift_from_checks(false, false, false, true, None),
            Some(PostCommitLocalDriftKind::UserFollowUp)
        );
        assert_eq!(
            classify_post_commit_local_drift_from_checks(
                false,
                false,
                false,
                false,
                Some(PostCommitLocalDriftKind::UserFollowUp)
            ),
            Some(PostCommitLocalDriftKind::UserFollowUp)
        );
        assert_eq!(
            classify_post_commit_local_drift_from_checks(false, false, false, false, None),
            Some(PostCommitLocalDriftKind::WorkingTreeEdits)
        );
    }

    #[test]
    fn safe_user_only_follow_up_after_committed_head_allows_exchange_only_growth() {
        let head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        let current = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            \u{276f} follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";

        assert!(is_safe_user_only_follow_up_after_committed_head(
            head, current
        ));
    }

    #[test]
    fn post_commit_drift_uses_prompt_classifier_for_queue_directive() {
        let head = "---\nagent_doc_session: test\n---\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: done\n\
            Completed.\n\
            <!-- /agent:exchange -->\n\n\
            ## Queue\n\n\
            <!-- agent:queue -->\n\
            <!-- /agent:queue -->\n\n\
            ## Backlog\n\n\
            <!-- agent:backlog -->\n\
            <!-- /agent:backlog -->\n";
        let current = "---\nagent_doc_session: test\n---\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: done\n\
            Completed.\n\
            <!-- /agent:exchange -->\n\n\
            ## Queue\n\n\
            <!-- agent:queue auto -->\n\
            preset #spec-test-build-install-commit-push\n\
            - do [#nexttop]\n\
            <!-- /agent:queue -->\n\n\
            ## Backlog\n\n\
            <!-- agent:backlog -->\n\
            - [ ] [#nexttop] Fix stale status.\n\
            <!-- /agent:backlog -->\n";

        assert_eq!(
            classify_post_commit_local_drift(head, current),
            Some(PostCommitLocalDriftKind::UserFollowUp)
        );
    }

    #[test]
    fn post_commit_drift_keeps_inline_corrections_as_working_tree_edits() {
        let head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: report\n\
            The service returned 401.\n\
            More analysis.\n\
            <!-- /agent:exchange -->\n";
        let current = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: report\n\
            The service returned 503.\n\
            More analysis.\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_post_commit_local_drift(head, current),
            Some(PostCommitLocalDriftKind::WorkingTreeEdits)
        );
    }

    #[test]
    fn doc_stem_uses_file_stem_or_doc_fallback() {
        assert_eq!(doc_stem(Path::new("tasks/plan.md")), "plan");
        assert_eq!(doc_stem(Path::new(".")), "doc");
        assert_eq!(doc_stem_or(Path::new("."), "unknown"), "unknown");
    }

    #[test]
    fn commit_and_branch_policy_use_expected_stem_fallbacks() {
        assert_eq!(
            agent_doc_commit_message_for_file(Path::new("tasks/session.md"), "2026-07-01"),
            "agent-doc(session): 2026-07-01"
        );
        assert_eq!(
            agent_doc_commit_message_for_file(Path::new("."), "2026-07-01"),
            "agent-doc(unknown): 2026-07-01"
        );
        assert_eq!(
            agent_doc_branch_name_for_file(Path::new("tasks/session.md")),
            "agent-doc/session"
        );
        assert_eq!(
            agent_doc_branch_name_for_file(Path::new(".")),
            "agent-doc/session"
        );
        assert_eq!(
            parent_submodule_pointer_commit_message("agent-doc(session): 2026-07-01"),
            "agent-doc(session): 2026-07-01 (submodule pointer)"
        );
    }

    #[test]
    fn short_oid_truncates_and_labels_missing_values() {
        assert_eq!(short_oid(Some("0123456789abcdef")), "0123456789ab");
        assert_eq!(short_oid(Some("")), "<missing>");
        assert_eq!(short_oid(None), "<missing>");
    }

    #[test]
    fn parent_submodule_pointer_message_renders_recovery_hint() {
        let message = parent_submodule_pointer_message(
            "src/agent-doc",
            Some("aaaaaaaaaaaabbbb"),
            "bbbbbbbbbbbbcccc",
            "tasks/doc.md",
        );
        assert!(message.contains("parent submodule pointer is not committed for src/agent-doc"));
        assert!(message.contains("parent HEAD aaaaaaaaaaaa"));
        assert!(message.contains("submodule HEAD bbbbbbbbbbbb"));
        assert_eq!(
            parent_pointer_recovery_hint("tasks/doc.md"),
            "Use `agent-doc commit tasks/doc.md` to finish the missing parent pointer commit, then re-run `agent-doc session-check tasks/doc.md`."
        );
    }

    #[test]
    fn parse_recovery_tags_filters_and_sorts_newest_first() {
        let lines = "agent-doc/doc/pre-auto-run-1\n\
                     agent-doc/doc/pre-auto-run-2\n\
                     agent-doc/doc/pre-compact-1\n\
                     agent-doc/doc/unrelated-tag\n\
                     v1.0.0\n";
        let mut meta = |name: &str| -> (String, String, String) {
            let date = if name.ends_with("pre-auto-run-2") {
                "2026-06-02"
            } else if name.ends_with("pre-auto-run-1") {
                "2026-06-01"
            } else {
                "2026-05-30"
            };
            (
                "abc1234".to_string(),
                date.to_string(),
                "checkpoint".to_string(),
            )
        };
        let tags = parse_recovery_tags(lines, &mut meta);
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "agent-doc/doc/pre-auto-run-2");
        assert_eq!(tags[0].slug, "pre-auto-run");
        assert_eq!(tags[0].ordinal, 2);
        assert_eq!(tags[1].name, "agent-doc/doc/pre-auto-run-1");
        assert_eq!(tags[2].slug, "pre-compact");
        assert!(tags.iter().all(|t| t.name.contains("pre-")));
    }

    #[test]
    fn parse_recovery_tags_skips_malformed_ordinals() {
        let mut meta =
            |_: &str| -> (String, String, String) { (String::new(), String::new(), String::new()) };
        let tags = parse_recovery_tags(
            "agent-doc/doc/pre-auto-run-latest\nagent-doc/doc/pre-compact-3\n",
            &mut meta,
        );
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "agent-doc/doc/pre-compact-3");
    }

    #[test]
    fn parse_recovery_tags_orders_same_date_by_ordinal_desc() {
        let mut meta = |_: &str| -> (String, String, String) {
            (
                "abc1234".to_string(),
                "2026-06-02".to_string(),
                "checkpoint".to_string(),
            )
        };
        let tags = parse_recovery_tags(
            "agent-doc/doc/pre-auto-run-1\nagent-doc/doc/pre-auto-run-3\n",
            &mut meta,
        );
        assert_eq!(tags[0].ordinal, 3);
        assert_eq!(tags[1].ordinal, 1);
    }

    #[test]
    fn parse_recovery_tags_empty_when_no_checkpoints() {
        let mut meta =
            |_: &str| -> (String, String, String) { (String::new(), String::new(), String::new()) };
        assert!(parse_recovery_tags("v1.0.0\nrelease-2\n", &mut meta).is_empty());
    }
}
