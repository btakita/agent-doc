//! `#coinedid` mid-turn guard — block a tool call that would make an invented
//! `#id` durable.
//!
//! The `session-check` guard names coined ids *after* the response commits. By
//! then the tag is already written into source or a commit message, which is the
//! damage that actually lasts: `#orphandrain` survives in git history describing
//! a feature that no longer exists, and no post-commit warning can unwrite it.
//!
//! `PreToolUse` fires at the moment the tag would become durable and, unlike a
//! supervisor watching pane output, receives the exact tool payload — so it can
//! name the file and the tag, and refuse the specific call. It also works when no
//! supervisor exists, which is precisely when things are already going wrong.
//!
//! Scope is deliberately narrow. Only writes that PERSIST a tag are inspected:
//! `Edit`/`Write` file content and `git commit` messages. Reads, searches, and
//! ordinary shell commands are never blocked. Anything unrecognized is allowed —
//! a hook that guesses wrong costs the operator a turn, so it fails open.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const C_FAMILY_EXTENSIONS: &[&str] = &[
    "c", "cc", "cpp", "cu", "cuh", "cxx", "h", "hh", "hpp", "hxx", "inc", "inl", "ipp", "m", "mm",
    "tpp",
];
const C_FAMILY_PREPROCESSOR_DIRECTIVES: &[&str] = &[
    "define",
    "elif",
    "elifdef",
    "elifndef",
    "else",
    "embed",
    "endif",
    "error",
    "if",
    "ifdef",
    "ifndef",
    "import",
    "include",
    "include_next",
    "line",
    "pragma",
    "undef",
    "warning",
];

/// Decision returned to the harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreToolUseDecision {
    Allow,
    Deny { reason: String },
}

/// Text a tool call would persist, if any.
///
/// Returns `None` for tools that cannot make a tag durable, so the common case
/// (reads, searches, tests) short-circuits without parsing.
pub fn persisted_text_for_tool(tool_name: &str, tool_input: &serde_json::Value) -> Option<String> {
    let field = |key: &str| {
        tool_input
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    match tool_name {
        // `new_string` is what lands in the file; `old_string` is existing content
        // and must not be scanned or an untouched pre-existing tag would block.
        "Edit" => field("new_string"),
        "Write" => field("content"),
        "NotebookEdit" => field("new_source"),
        "Bash" => {
            let command = field("command")?;
            is_commit_command(&command).then_some(command)
        }
        _ => None,
    }
}

/// Does this shell command record a commit message?
///
/// Only `git commit` persists prose into history. `git add`, `git status`, and a
/// command that merely mentions the word commit must not be inspected.
pub fn is_commit_command(command: &str) -> bool {
    command.split(['\n', ';', '&', '|']).any(|segment| -> bool {
        let mut words = segment.split_whitespace().skip_while(|word| {
            matches!(*word, "sudo" | "env" | "rtk" | "proxy") || word.contains('=')
        });
        if words.next() != Some("git") {
            return false;
        }
        // Global flags may take a VALUE (`git -C /repo commit`); consuming only
        // the flag would mistake that value for the subcommand.
        let mut rest = words.peekable();
        while let Some(word) = rest.peek() {
            if !word.starts_with('-') {
                break;
            }
            let takes_value = matches!(
                *word,
                "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path"
            );
            rest.next();
            if takes_value {
                rest.next();
            }
        }
        rest.next() == Some("commit")
    })
}

/// Decide whether a tool call may proceed.
///
/// `known_ids` is the tracked-id universe for the active document. Empty means
/// unknown (no active document resolved) — in that case nothing is blocked,
/// because a guard that cannot see the document cannot tell coined from tracked.
pub fn pretooluse_decision(
    tool_name: &str,
    tool_input: &serde_json::Value,
    known_ids: &BTreeSet<String>,
    document_resolved: bool,
) -> PreToolUseDecision {
    if !document_resolved {
        return PreToolUseDecision::Allow;
    }
    let Some(text) = persisted_text_for_tool(tool_name, tool_input) else {
        return PreToolUseDecision::Allow;
    };
    let scan_text = coined_id_scan_text(tool_name, tool_input, &text);
    let coined = agent_doc_turn::coined_ids::coined_ids(&scan_text, known_ids);
    if coined.is_empty() {
        return PreToolUseDecision::Allow;
    }
    let names = coined
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let target = if tool_name == "Bash" {
        "commit message".to_string()
    } else {
        tool_input
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("this file")
            .to_string()
    };
    PreToolUseDecision::Deny {
        reason: format!(
            "[agent-doc] blocked: this {tool_name} would write coined id(s) {names} into {target}, \
             but they are not tracked in agent:backlog, agent:queue, agent:done, or agent:review. \
             An id in source or a commit message with no tracked item resolves to nothing later. \
             File one first (`agent-doc write --commit <FILE> --backlog-add \"#<id> ...\"`), reuse \
             an existing id, or drop the tag from the text."
        ),
    }
}

fn coined_id_scan_text<'a>(
    tool_name: &str,
    tool_input: &serde_json::Value,
    text: &'a str,
) -> Cow<'a, str> {
    if tool_name == "Bash" || !is_c_family_target(tool_input) {
        return Cow::Borrowed(text);
    }

    let mut sanitized = None;
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let trimmed = line.trim_start_matches([' ', '\t']);
        let leading_len = line.len() - trimmed.len();
        let Some(after_hash) = trimmed.strip_prefix('#') else {
            offset += segment.len();
            continue;
        };
        let directive = after_hash
            .trim_start_matches([' ', '\t'])
            .chars()
            .take_while(|ch| ch.is_ascii_alphabetic() || *ch == '_')
            .collect::<String>();
        if C_FAMILY_PREPROCESSOR_DIRECTIVES.contains(&directive.as_str()) {
            sanitized
                .get_or_insert_with(|| text.to_string())
                .replace_range(offset + leading_len..offset + leading_len + 1, " ");
        }
        offset += segment.len();
    }

    sanitized.map_or(Cow::Borrowed(text), Cow::Owned)
}

fn is_c_family_target(tool_input: &serde_json::Value) -> bool {
    tool_input
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| C_FAMILY_EXTENSIONS.contains(&extension.as_str()))
}

/// Tracked ids for a document: every component EXCEPT `exchange`.
///
/// `exchange` holds responses, so including it would let an id the agent just
/// wrote in prose vouch for the same id being written into source.
pub fn known_ids_for_document(file: &Path) -> Option<BTreeSet<String>> {
    let content = std::fs::read_to_string(file).ok()?;
    let components = agent_doc_element::element::parse(&content).ok()?;
    let mut known = BTreeSet::new();
    for component in components
        .iter()
        .filter(|component| component.name != "exchange")
    {
        known.extend(agent_doc_turn::coined_ids::extract_tags(
            component.content(&content),
        ));
    }
    // Completed work is archived OUT of the live document into a `.done.md`
    // sibling, so without it the guard blocks the most common legitimate use of
    // an id in a code comment: citing work that already shipped. Observed live —
    // a real `#fr79` was blocked because it had been archived. The whole archive
    // counts; every id in it is tracked history by definition.
    for archive in done_archive_candidates(file) {
        if let Ok(archived) = std::fs::read_to_string(&archive) {
            known.extend(agent_doc_turn::coined_ids::extract_tags(&archived));
        }
    }
    Some(known)
}

/// Candidate `<stem>.done.md` archives for a document.
///
/// The archive is not always a directory sibling — `tasks/agent-doc/x.md` is
/// archived to `tasks/x.done.md` — so walk up to the project root. Bounded by
/// the root, and every candidate is optional.
pub fn done_archive_candidates(file: &Path) -> Vec<PathBuf> {
    let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let archive_name = format!("{stem}.done.md");
    let root = file
        .parent()
        .and_then(agent_doc_fs::find_project_root)
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut dir = file.parent().map(Path::to_path_buf);
    while let Some(current) = dir {
        out.push(current.join(&archive_name));
        if current == root {
            break;
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    out
}

/// Resolve the session document THIS pane owns.
///
/// Must be pane-scoped. Taking any registry entry looks like it works and is
/// wrong in both directions: a project holds many documents, so an id tracked in
/// document A reads as coined while checking document B (observed live — a real
/// `#fr79` was blocked against an unrelated document's id set), and a genuinely
/// coined id can be waved through by another document that happens to use it.
///
/// `$TMUX_PANE` is inherited from the pane running the harness, so it identifies
/// the owner exactly. Without it there is no trustworthy scope, and the guard
/// disables itself rather than guessing.
pub fn active_document_for(cwd: &Path, pane: Option<&str>) -> Option<PathBuf> {
    let pane = pane?;
    let root = agent_doc_fs::find_project_root(cwd)?;
    let registry = agent_doc_session_registry_io::load_in(&root).ok()?;
    let file = registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| PathBuf::from(&entry.file))?;
    let file = if file.is_absolute() {
        file
    } else {
        root.join(file)
    };
    file.exists().then_some(file)
}

/// `PreToolUse` entry point: read the harness payload on stdin, decide, and
/// report. Exit status 2 with the reason on stderr is how Claude Code blocks a
/// tool call; every other path exits 0 so the guard can never wedge a turn.
pub fn handle_pretooluse() -> anyhow::Result<()> {
    use std::io::Read;
    let mut payload = String::new();
    if std::io::stdin().read_to_string(&mut payload).is_err() {
        return Ok(());
    }
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return Ok(());
    };
    let tool_name = input
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let empty = serde_json::Value::Null;
    let tool_input = input.get("tool_input").unwrap_or(&empty);
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let pane = std::env::var("TMUX_PANE").ok();
    let known =
        active_document_for(&cwd, pane.as_deref()).and_then(|file| known_ids_for_document(&file));
    let decision = match &known {
        Some(known) => pretooluse_decision(tool_name, tool_input, known, true),
        None => PreToolUseDecision::Allow,
    };
    if let PreToolUseDecision::Deny { reason } = decision {
        eprintln!("{reason}");
        std::process::exit(2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn known(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    /// The durable case this exists for: writing a coined tag into source.
    #[test]
    fn an_edit_writing_a_coined_id_into_source_is_blocked() {
        let input = json!({
            "file_path": "/repo/src/rpc.rs",
            "new_string": "// `#orphandrain` — controller-side drain\nfn tick() {}"
        });
        let decision = pretooluse_decision("Edit", &input, &known(&["fr79"]), true);
        match decision {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#orphandrain"), "{reason}");
                assert!(reason.contains("/repo/src/rpc.rs"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// A tracked id must never be blocked, or the guard makes normal work harder.
    #[test]
    fn an_edit_referencing_a_tracked_id_is_allowed() {
        let input = json!({
            "file_path": "/repo/src/rpc.rs",
            "new_string": "// `#fr79` — orphan strike is wired"
        });
        assert_eq!(
            pretooluse_decision("Edit", &input, &known(&["fr79"]), true),
            PreToolUseDecision::Allow
        );
    }

    #[test]
    fn c_family_preprocessor_directives_are_not_coined_ids() {
        let input = json!({
            "file_path": "/repo/include/wire.hpp",
            "content": concat!(
                "#ifndef WIRE_HPP\n",
                "#define WIRE_HPP\n",
                "#include <cstdint>\n",
                "#if defined(__cplusplus)\n",
                "#pragma once\n",
                "#endif\n",
            )
        });

        assert_eq!(
            pretooluse_decision("Write", &input, &known(&[]), true),
            PreToolUseDecision::Allow
        );
    }

    #[test]
    fn c_family_source_still_blocks_real_coined_ids() {
        let input = json!({
            "file_path": "/repo/include/wire.hpp",
            "content": "#include <cstdint>\n// #codecfix is not tracked\n"
        });

        match pretooluse_decision("Write", &input, &known(&[]), true) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#codecfix"), "{reason}");
                assert!(!reason.contains("#include,"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn preprocessor_words_outside_directive_position_or_c_family_files_remain_ids() {
        for input in [
            json!({
                "file_path": "/repo/notes.md",
                "content": "#include is a tracked-work tag here"
            }),
            json!({
                "file_path": "/repo/src/wire.cpp",
                "content": "// #include is a tracked-work tag here"
            }),
        ] {
            match pretooluse_decision("Write", &input, &known(&[]), true) {
                PreToolUseDecision::Deny { reason } => {
                    assert!(reason.contains("#include"), "{reason}");
                }
                other => panic!("expected deny, got {other:?}"),
            }
        }
    }

    /// `old_string` is pre-existing content. Scanning it would block an edit that
    /// merely touches a line near a tag the turn did not introduce.
    #[test]
    fn a_coined_id_only_in_old_string_is_not_blocked() {
        let input = json!({
            "file_path": "/repo/src/rpc.rs",
            "old_string": "// `#legacytag` existing line",
            "new_string": "// rewritten line"
        });
        assert_eq!(
            pretooluse_decision("Edit", &input, &known(&[]), true),
            PreToolUseDecision::Allow
        );
    }

    /// The second durable surface: commit messages.
    #[test]
    fn a_git_commit_carrying_a_coined_id_is_blocked() {
        let input = json!({"command": "git commit -q -m 'fix(queue): #madeup thing'"});
        match pretooluse_decision("Bash", &input, &known(&[]), true) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#madeup"), "{reason}");
                assert!(reason.contains("commit message"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// Ordinary shell work must pass untouched even when it mentions a tag.
    #[test]
    fn non_commit_shell_commands_are_never_inspected() {
        for command in [
            "rg '#madeup' src/",
            "git add -A",
            "git status",
            "echo 'see #madeup'",
        ] {
            assert_eq!(
                pretooluse_decision("Bash", &json!({ "command": command }), &known(&[]), true),
                PreToolUseDecision::Allow,
                "must not block: {command}"
            );
        }
    }

    #[test]
    fn commit_detection_tolerates_prefixes_and_flags() {
        assert!(is_commit_command("git commit -m x"));
        assert!(is_commit_command("git -C /repo commit -m x"));
        assert!(is_commit_command("cd /repo && git commit -q -F -"));
        assert!(!is_commit_command("git add -A"));
        assert!(!is_commit_command("echo git commit"));
    }

    /// Read-only tools cannot make a tag durable.
    #[test]
    fn read_only_tools_are_not_inspected() {
        for tool in ["Read", "Grep", "Glob", "WebFetch"] {
            assert_eq!(
                pretooluse_decision(tool, &json!({"pattern": "#madeup"}), &known(&[]), true),
                PreToolUseDecision::Allow
            );
        }
    }

    /// The archive gap that produced a live false positive: completed work moves
    /// OUT of the document into `<stem>.done.md`, and citing shipped work is the
    /// most common legitimate reason to put an id in a code comment.
    #[test]
    fn ids_archived_to_a_done_sibling_are_known() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(
            &doc,
            "<!-- agent:backlog -->
- [ ] [#liveid] open
<!-- /agent:backlog -->
",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("plan.done.md"),
            "- 2026-01-01 [#archivedid] shipped long ago
",
        )
        .unwrap();

        let known = known_ids_for_document(&doc).unwrap();
        assert!(known.contains("liveid"), "live backlog id must be known");
        assert!(
            known.contains("archivedid"),
            "an archived id must be known, or citing shipped work is blocked"
        );
    }

    /// The archive is not always a directory sibling: `tasks/agent-doc/x.md` is
    /// archived to `tasks/x.done.md`, so candidates walk up toward the root.
    #[test]
    fn done_archive_candidates_walk_up_toward_the_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let nested = dir.path().join("tasks").join("agent-doc");
        std::fs::create_dir_all(&nested).unwrap();
        let doc = nested.join("bugs.md");
        std::fs::write(&doc, "body").unwrap();

        let candidates = done_archive_candidates(&doc);
        assert!(candidates.contains(&nested.join("bugs.done.md")));
        assert!(
            candidates.contains(&dir.path().join("tasks").join("bugs.done.md")),
            "must consider the parent-directory archive layout: {candidates:?}"
        );
    }

    /// With no resolvable document there is no id universe, so blocking would be
    /// a guess. Fail open — a wrong block costs the operator a turn.
    #[test]
    fn an_unresolved_document_never_blocks() {
        let input = json!({"file_path": "/x.rs", "new_string": "// #madeup"});
        assert_eq!(
            pretooluse_decision("Edit", &input, &known(&[]), false),
            PreToolUseDecision::Allow
        );
    }
}
