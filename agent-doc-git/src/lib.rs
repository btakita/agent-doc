//! Pure git command and path policy helpers.

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

pub fn doc_stem(file: &Path) -> String {
    file.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc")
        .to_string()
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
        commit_retry_backoff, doc_stem, is_index_lock_contention_text,
        line_looks_like_explicit_post_commit_prompt_directive, output_has_index_lock_contention,
        parent_pointer_recovery_hint, parent_submodule_pointer_message, parse_porcelain_path,
        parse_recovery_tags, parse_submodule_paths, relative_to_root, render_git_streams,
        short_oid, tracked_modified_paths_from_porcelain,
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
    fn doc_stem_uses_file_stem_or_doc_fallback() {
        assert_eq!(doc_stem(Path::new("tasks/plan.md")), "plan");
        assert_eq!(doc_stem(Path::new(".")), "doc");
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
