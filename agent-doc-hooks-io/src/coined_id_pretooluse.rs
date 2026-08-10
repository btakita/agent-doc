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
//! Scope is deliberately narrow, in two directions. Only writes that PERSIST a
//! tag are inspected: `Edit`/`Write` file content and `git commit` messages.
//! Reads, searches, and ordinary shell commands are never blocked. Anything
//! unrecognized is allowed — a hook that guesses wrong costs the operator a
//! turn, so it fails open.
//!
//! And within a file write, only PROSE is inspected
//! (`#hookhashfalsepositive`, `#hookhashjsprivatefield`). A commit message is
//! prose end to end; a source file is prose only in its comments. Scanning code
//! made every language that spells something `#word` a false block — C
//! preprocessor directives, CSS id selectors, ES2022 private class fields — and
//! patching each incident with another extension list did not converge.
//! `agent_doc_turn::prose_scope::prose_only` blanks the code instead, and
//! returns an unrecognized format untouched so narrowing can never open a hole.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How many times to re-read the ledger before concluding it is unreadable.
///
/// The session document and the session registry are rewritten continuously by
/// the write pipeline, the CRDT relay, and every other live pane, so a failed
/// read is far more often a moment of contention than a real absence. Five
/// attempts at 20ms rides out a rewrite without making a `PreToolUse` hook
/// perceptibly slow.
const LEDGER_READ_ATTEMPTS: u32 = 5;
const LEDGER_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(20);

/// What the guard could learn about the document governing this tool call.
///
/// The three cases exist because collapsing them is the defect (`#coinedpretooluseguard`):
/// "no document governs this call" and "a document governs it but its ledger
/// could not be read" used to produce the same `None`, and `None` meant *allow*.
/// A guard that opens whenever its ledger is momentarily unreadable is not a
/// guard — and it opens precisely under the contention where ids are most
/// likely to be flying around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentIds {
    /// No agent-doc document governs this call: no pane scope, not a project,
    /// or this pane owns no session document. Nothing to check against, so a
    /// write is none of the guard's business.
    Ungoverned,
    /// Tracked ids read from the governing document (and its `.done.md` archives).
    Known(BTreeSet<String>),
    /// A document governs this call and its ledger could not be read. The guard
    /// fails CLOSED here, but only for text that actually carries an id.
    Unavailable { file: PathBuf, cause: String },
}

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
///
/// `project_root`, when given, widens that universe with the project's agent
/// instruction anchors on the deny path only (`#hookhashanchortags`). `None`
/// skips it entirely, which is what the pure unit tests use.
pub fn pretooluse_decision(
    tool_name: &str,
    tool_input: &serde_json::Value,
    ids: &DocumentIds,
    project_root: Option<&Path>,
) -> PreToolUseDecision {
    if matches!(ids, DocumentIds::Ungoverned) {
        return PreToolUseDecision::Allow;
    }
    let Some(text) = persisted_text_for_tool(tool_name, tool_input) else {
        return PreToolUseDecision::Allow;
    };
    let scan_text = coined_id_scan_text(tool_name, tool_input, &text);
    // With an unreadable ledger every tag is unvouched-for by definition, so the
    // empty set is the honest comparison basis. It also keeps the fail-closed
    // blast radius exactly as small as it should be: text carrying no id-shaped
    // token is still allowed, because there is nothing a ledger could have said
    // about it. Code was already blanked above, so a header full of `#include`,
    // a class body of `#private` fields, and a stylesheet of `#id` selectors
    // are not collateral either.
    let empty = BTreeSet::new();
    let known = match ids {
        DocumentIds::Known(known) => known,
        _ => &empty,
    };
    let coined = agent_doc_turn::coined_ids::coined_ids(&scan_text, known);
    if coined.is_empty() {
        return PreToolUseDecision::Allow;
    }
    // Second pass, deliberately lazy (`#hookhashanchortags`). An anchor like
    // `#preflightinbinary` or `#drain-no-defer` names a documented invariant in
    // AGENTS.md, a SKILL, a runbook, or a spec — that is tracked work, and
    // quoting it by name is the correct thing to do, so blocking it is pure
    // noise. But those files are hundreds of kilobytes and this runs on every
    // Edit, so they are read ONLY once something is already about to be
    // blocked. The overwhelmingly common call carries no id at all and returned
    // above without touching the disk.
    let coined = match project_root {
        Some(root) => {
            let anchors = agent_doc_fs::instruction_surface_anchors(root);
            coined
                .into_iter()
                .filter(|tag| !anchors.contains(tag))
                .collect::<Vec<_>>()
        }
        None => coined,
    };
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
    if let DocumentIds::Unavailable { file, cause } = ids {
        return PreToolUseDecision::Deny {
            reason: format!(
                "[agent-doc] blocked: this {tool_name} would write id(s) {names} into {target}, and \
                 the ledger that would vouch for them could not be read after \
                 {LEDGER_READ_ATTEMPTS} attempts ({file}: {cause}). Refusing rather than allowing: \
                 a guard that opens whenever its ledger is momentarily unreadable does not guard \
                 anything, and contention is exactly when ids get coined. Retry once the document \
                 settles, or drop the tag from the text.",
                file = file.display(),
            ),
        };
    }
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
    // A commit message is pure prose, so it is scanned whole. A file write is
    // scanned only where the file carries prose (`#hookhashfalsepositive`,
    // `#hookhashjsprivatefield`): comments, plus the entire text of formats
    // whose comment syntax we do not claim to know.
    if tool_name == "Bash" {
        return Cow::Borrowed(text);
    }
    agent_doc_turn::prose_scope::prose_only(text, target_extension(tool_input).as_deref())
}

fn target_extension(tool_input: &serde_json::Value) -> Option<String> {
    tool_input
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_string)
}

/// Tracked ids for a document: every component EXCEPT `exchange`.
///
/// `exchange` holds responses, so including it would let an id the agent just
/// wrote in prose vouch for the same id being written into source.
pub fn known_ids_for_document(file: &Path) -> Result<BTreeSet<String>, String> {
    let content =
        std::fs::read_to_string(file).map_err(|err| format!("reading the document: {err}"))?;
    let components = agent_doc_element::element::parse(&content)
        .map_err(|err| format!("parsing the document: {err}"))?;
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
    Ok(known)
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

/// Resolve the ids this call must be checked against, retrying transient
/// failures before giving up (`#coinedpretooluseguard`).
///
/// Every `Ungoverned` return below is a case where no ledger could exist:
/// no pane scope, no project root, no registry on disk, or a registry that read
/// cleanly and holds no document for this pane. Everything else — a registry
/// that exists but would not open, a registered document that will not read or
/// parse — is `Unavailable`, because the ledger *should* have answered and did
/// not. That distinction is the whole fix; before it, all of them were `None`
/// and `None` meant allow.
pub fn document_ids(cwd: &Path, pane: Option<&str>) -> DocumentIds {
    let Some(pane) = pane else {
        return DocumentIds::Ungoverned;
    };
    let Some(root) = agent_doc_fs::find_project_root(cwd) else {
        return DocumentIds::Ungoverned;
    };
    // A project with no registry on disk has no ledger to be unavailable, so an
    // ordinary repo can never be denied by this guard.
    if !agent_doc_session_registry_io::registry_path_in(&root).exists() {
        return DocumentIds::Ungoverned;
    }

    let mut subject = root.clone();
    let mut cause = "the ledger did not resolve".to_string();
    for attempt in 0..LEDGER_READ_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(LEDGER_RETRY_BACKOFF);
        }
        let registry = match agent_doc_session_registry_io::load_in(&root) {
            Ok(registry) => registry,
            Err(err) => {
                cause = format!("opening the session registry: {err}");
                continue;
            }
        };
        // The registry read cleanly. If it holds nothing for this pane, the pane
        // genuinely owns no document — that is an answer, not a failure.
        let Some(entry) = registry.values().find(|entry| entry.pane == pane) else {
            return DocumentIds::Ungoverned;
        };
        let file = PathBuf::from(&entry.file);
        let file = if file.is_absolute() {
            file
        } else {
            root.join(file)
        };
        subject = file.clone();
        match known_ids_for_document(&file) {
            Ok(known) => return DocumentIds::Known(known),
            Err(err) => cause = err,
        }
    }
    DocumentIds::Unavailable {
        file: subject,
        cause,
    }
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
    let ids = document_ids(&cwd, pane.as_deref());
    let root = agent_doc_fs::find_project_root(&cwd);
    let decision = pretooluse_decision(tool_name, tool_input, &ids, root.as_deref());
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
        let decision = pretooluse_decision("Edit", &input, &DocumentIds::Known(known(&["fr79"])), None);
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
            pretooluse_decision("Edit", &input, &DocumentIds::Known(known(&["fr79"])), None),
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
            pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None),
            PreToolUseDecision::Allow
        );
    }

    #[test]
    fn c_family_source_still_blocks_real_coined_ids() {
        let input = json!({
            "file_path": "/repo/include/wire.hpp",
            "content": "#include <cstdint>\n// #codecfix is not tracked\n"
        });

        match pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None) {
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
            match pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None) {
                PreToolUseDecision::Deny { reason } => {
                    assert!(reason.contains("#include"), "{reason}");
                }
                other => panic!("expected deny, got {other:?}"),
            }
        }
    }

    /// `#hookhashjsprivatefield`: ES2022 private class fields. Both shapes
    /// slip past `extract_tags`'s glued-to-a-word rule — `this.#entries` because
    /// the preceding character is `.`, and an indented `#hlc;` because it is a
    /// space — so only prose scoping keeps them out.
    #[test]
    fn javascript_private_class_fields_do_not_block_a_write() {
        let input = json!({
            "file_path": "/repo/src/seq-crdt.js",
            "content": "export class SeqCrdt {\n  #hlc;\n  #peer;\n  #entries = new Map()\n  size() {\n    return this.#entries.size\n  }\n}\n"
        });

        assert_eq!(
            pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None),
            PreToolUseDecision::Allow
        );
    }

    /// TypeScript is the same language family and must not need its own incident.
    #[test]
    fn typescript_private_class_fields_do_not_block_an_edit() {
        let input = json!({
            "file_path": "/repo/src/wire.ts",
            "new_string": "  #hlc: Hlc\n  peer(): string {\n    return this.#peer\n  }\n"
        });

        assert_eq!(
            pretooluse_decision("Edit", &input, &DocumentIds::Known(known(&[])), None),
            PreToolUseDecision::Allow
        );
    }

    /// A coined id in a JS comment is exactly what the guard is for, so prose
    /// scoping must not turn the whole file into a blind spot.
    #[test]
    fn a_coined_id_in_a_javascript_comment_still_blocks() {
        let input = json!({
            "file_path": "/repo/src/seq-crdt.js",
            "content": "// tracked by #codecfix\nclass A {{ #x }}\n"
        });

        match pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#codecfix"), "{reason}");
                assert!(!reason.contains("#x"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// CSS id selectors, named by `#hookhashfalsepositive` alongside the C case.
    #[test]
    fn css_id_selectors_do_not_block_a_write() {
        let input = json!({
            "file_path": "/repo/site/app.css",
            "content": "#mainpanel .row {{ color: red }}\n#sidebar {{ width: 20rem }}\n"
        });

        assert_eq!(
            pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None),
            PreToolUseDecision::Allow
        );
    }

    /// `#hookhashanchortags`: an anchor defined in an instruction surface names
    /// a documented invariant, so quoting it is correct and must not block.
    /// Reproduced live 2026-08-09 — a closeout warned about `#ci-no-closeout-wait`
    /// after the response cited the AGENTS.md rule by name.
    #[test]
    fn instruction_surface_anchors_are_tracked_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "- **External CI is observed, never awaited (`#ci-no-closeout-wait`)**\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("runbooks")).unwrap();
        std::fs::write(
            dir.path().join("runbooks/commit.md"),
            "Preflight is binary-owned (`#preflightinbinary`).\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/skills/agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".claude/skills/agent-doc/SKILL.md"),
            "Do not defer a drainable item (`#drain-no-defer`).\n",
        )
        .unwrap();

        let input = json!({
            "file_path": "/repo/src/notes.md",
            "content": "Followed #ci-no-closeout-wait, #preflightinbinary and #drain-no-defer.\n"
        });

        // Without the project root the anchors are invisible and every one blocks.
        match pretooluse_decision("Write", &input, &DocumentIds::Known(known(&[])), None) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#ci-no-closeout-wait"), "{reason}");
            }
            other => panic!("expected deny without a root, got {other:?}"),
        }

        assert_eq!(
            pretooluse_decision(
                "Write",
                &input,
                &DocumentIds::Known(known(&[])),
                Some(dir.path())
            ),
            PreToolUseDecision::Allow
        );
    }

    /// Widening the universe must not blunt the guard: an id that appears in no
    /// instruction surface still blocks, and the reason names only that id.
    #[test]
    fn an_id_absent_from_every_instruction_surface_still_blocks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "Anchored on `#preflightinbinary`.\n",
        )
        .unwrap();

        let input = json!({
            "file_path": "/repo/src/notes.md",
            "content": "Per #preflightinbinary, and also #inventedrightnow.\n"
        });

        match pretooluse_decision(
            "Write",
            &input,
            &DocumentIds::Known(known(&[])),
            Some(dir.path()),
        ) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#inventedrightnow"), "{reason}");
                assert!(!reason.contains("#preflightinbinary"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// A project with no instruction surfaces at all must not panic, and must
    /// keep behaving exactly as it did before anchors existed.
    #[test]
    fn a_project_without_instruction_surfaces_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let input = json!({
            "file_path": "/repo/src/notes.md",
            "content": "Coined #inventedrightnow here.\n"
        });

        match pretooluse_decision(
            "Write",
            &input,
            &DocumentIds::Known(known(&[])),
            Some(dir.path()),
        ) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#inventedrightnow"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// The harvest is on the deny path only. Text carrying no id must never
    /// touch the instruction surfaces, because this runs on every Edit.
    #[test]
    fn text_without_an_id_never_reads_the_instruction_surfaces() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where AGENTS.md would go: reading it would fail, and the
        // decision must not depend on that either way.
        std::fs::create_dir_all(dir.path().join("AGENTS.md")).unwrap();

        let input = json!({
            "file_path": "/repo/src/notes.md",
            "content": "No tags at all in this sentence.\n"
        });

        assert_eq!(
            pretooluse_decision(
                "Write",
                &input,
                &DocumentIds::Known(known(&[])),
                Some(dir.path())
            ),
            PreToolUseDecision::Allow
        );
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
            pretooluse_decision("Edit", &input, &DocumentIds::Known(known(&[])), None),
            PreToolUseDecision::Allow
        );
    }

    /// The second durable surface: commit messages.
    #[test]
    fn a_git_commit_carrying_a_coined_id_is_blocked() {
        let input = json!({"command": "git commit -q -m 'fix(queue): #madeup thing'"});
        match pretooluse_decision("Bash", &input, &DocumentIds::Known(known(&[])), None) {
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
                pretooluse_decision(
                    "Bash",
                    &json!({ "command": command }),
                    &DocumentIds::Known(known(&[])),
                None
                ),
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
                pretooluse_decision(
                    tool,
                    &json!({"pattern": "#madeup"}),
                    &DocumentIds::Known(known(&[])),
                    None
                ),
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

    /// No document governs the call, so there is no id universe to check
    /// against. Fail open — the guard is none of an unrelated repo's business.
    #[test]
    fn an_ungoverned_call_never_blocks() {
        let input = json!({"file_path": "/x.rs", "new_string": "// #madeup"});
        assert_eq!(
            pretooluse_decision("Edit", &input, &DocumentIds::Ungoverned, None),
            PreToolUseDecision::Allow
        );
    }

    fn unavailable() -> DocumentIds {
        DocumentIds::Unavailable {
            file: PathBuf::from("/repo/plan.md"),
            cause: "reading the document: Resource temporarily unavailable".to_string(),
        }
    }

    /// The defect this rung exists for: a governing document whose ledger cannot
    /// be read used to be indistinguishable from no document at all, and both
    /// meant allow. An id that nothing can vouch for must be refused, not waved
    /// through because the ledger happened to be busy.
    #[test]
    fn an_unreadable_ledger_refuses_a_tagged_write() {
        let input = json!({
            "file_path": "/repo/src/rpc.rs",
            "new_string": "// #madeup — coined while the ledger was unreadable"
        });
        match pretooluse_decision("Edit", &input, &unavailable(), None) {
            PreToolUseDecision::Deny { reason } => {
                assert!(reason.contains("#madeup"), "{reason}");
                assert!(reason.contains("/repo/plan.md"), "{reason}");
                assert!(
                    reason.contains("could not be read"),
                    "the reason must say the ledger was unreadable, not that the id is \
                     untracked — they are different failures: {reason}"
                );
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    /// Failing closed must stay narrow. Text carrying no id-shaped token has
    /// nothing a ledger could have vouched for, so an unreadable ledger is
    /// irrelevant to it — otherwise every write during a document rewrite would
    /// be refused and the guard would be unusable.
    #[test]
    fn an_unreadable_ledger_still_allows_an_untagged_write() {
        let input = json!({
            "file_path": "/repo/src/rpc.rs",
            "new_string": "fn tick() { drain(); }"
        });
        assert_eq!(
            pretooluse_decision("Edit", &input, &unavailable(), None),
            PreToolUseDecision::Allow
        );
    }

    /// The C-family sanitization runs before the ledger is consulted, so an
    /// unreadable ledger must not resurrect the preprocessor false positive.
    #[test]
    fn an_unreadable_ledger_still_allows_c_preprocessor_directives() {
        let input = json!({
            "file_path": "/repo/include/wire.hpp",
            "content": "#ifndef WIRE_HPP\n#define WIRE_HPP\n#include <cstdint>\n#endif\n"
        });
        assert_eq!(
            pretooluse_decision("Write", &input, &unavailable(), None),
            PreToolUseDecision::Allow
        );
    }

    /// A read-only tool cannot make a tag durable, so an unreadable ledger is
    /// not a reason to refuse it either.
    #[test]
    fn an_unreadable_ledger_still_allows_read_only_tools() {
        assert_eq!(
            pretooluse_decision("Grep", &json!({"pattern": "#madeup"}), &unavailable(), None),
            PreToolUseDecision::Allow
        );
    }

    /// A project with no registry on disk has no ledger that could be
    /// unavailable, so an ordinary repo is never denied by this guard.
    #[test]
    fn a_project_without_a_registry_is_ungoverned() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        assert_eq!(
            document_ids(dir.path(), Some("%1")),
            DocumentIds::Ungoverned
        );
    }

    /// A registered document that will not read is UNAVAILABLE, not ungoverned.
    /// Before the fix this was the widest hole: the registry pointed at a file,
    /// the read failed for a moment, and the guard silently switched itself off.
    #[test]
    fn a_registered_document_that_cannot_be_read_is_unavailable() {
        assert_eq!(
            known_ids_for_document(Path::new("/nonexistent/definitely/not/here.md")),
            Err("reading the document: No such file or directory (os error 2)".to_string())
        );
    }
}
