//! File-backed transcript context usage helpers (`#s760a`).
//!
//! This module owns the harness transcript locator/read layer. Pure token
//! parsing, context-window lookup, percentage math, and clear/no-clear policy
//! stay in [`crate::context_usage`].

use crate::context_usage::{
    Harness, TranscriptContextPctDiagnostic, UsedTokens, parse_claude_jsonl_used_tokens,
    parse_codex_jsonl_session_meta_cwd, transcript_context_pct_from_content,
};
use std::path::{Path, PathBuf};

/// Read cumulative token usage from a transcript file for the given harness
/// (`#s760a`). Claude -> parse JSONL; OpenCode -> `None` (unsupported until its
/// transcript store is confirmed live, so the caller fails safe and never
/// clears). Codex usage needs the event-provided context window, so callers use
/// [`transcript_context_pct_from_content`] through [`transcript_context_pct`] instead
/// of this raw token helper. A missing/unreadable file is `None`.
pub fn read_used_tokens(harness: Harness, transcript: &Path) -> Option<UsedTokens> {
    match harness {
        Harness::Claude => {
            let content = std::fs::read_to_string(transcript).ok()?;
            parse_claude_jsonl_used_tokens(&content)
        }
        Harness::Codex | Harness::OpenCode => {
            eprintln!(
                "[s760] raw transcript token reading unsupported for {harness:?}; ctx% None (never clears)"
            );
            None
        }
    }
}

/// Read a transcript and compute ctx% in one call (`#s760a` + `#s760b`). `None`
/// (never clear) on any failure: unreadable/empty transcript, unsupported
/// harness, or unknown model.
pub fn transcript_context_pct(harness: Harness, transcript: &Path, model: &str) -> Option<f64> {
    let content;
    let result = match harness {
        Harness::Claude | Harness::Codex => {
            content = std::fs::read_to_string(transcript).ok()?;
            transcript_context_pct_from_content(harness, &content, model)
        }
        Harness::OpenCode => transcript_context_pct_from_content(harness, "", model),
    };
    match result.diagnostic {
        Some(TranscriptContextPctDiagnostic::UnknownModel) => {
            eprintln!(
                "[s760] WARNING: unknown model {model:?}; context window unknown, ctx% None (never clears)"
            );
        }
        Some(TranscriptContextPctDiagnostic::UnsupportedHarness) => {
            eprintln!(
                "[s760] transcript context reading unsupported for {harness:?}; ctx% None (never clears)"
            );
        }
        None => {}
    }
    result.pct
}

/// Locate the active Claude Code session transcript as the most-recently-modified
/// `*.jsonl` under `projects_subdir` (`#s760c` live locator). The supervisor does
/// not track the managed harness's session id, so newest-mtime is the live signal
/// for "the transcript this session is writing". Returns `None` when the directory
/// is absent/unreadable or holds no `.jsonl` file, so the caller fails safe and
/// never clears.
pub fn latest_claude_transcript(projects_subdir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(projects_subdir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(best, _)| modified > *best) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

fn codex_session_meta_cwd_from_file(path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_codex_jsonl_session_meta_cwd(&content)
}

fn path_matches_project_dir(path: &Path, project_dir: &Path) -> bool {
    if path == project_dir {
        return true;
    }
    match (path.canonicalize(), project_dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Locate the newest Codex TUI session transcript for the current project.
/// Codex stores sessions under `~/.codex/sessions/<year>/<month>/<day>/`; the
/// first `session_meta` record carries `payload.cwd`, which is the stable
/// project match key.
pub fn latest_codex_transcript(home: &Path, project_dir: &Path) -> Option<PathBuf> {
    let root = home.join(".codex").join("sessions");
    let mut stack = vec![root];
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with("rollout-") {
                continue;
            }
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if newest.as_ref().is_some_and(|(best, _)| modified <= *best) {
                continue;
            }
            let Some(cwd) = codex_session_meta_cwd_from_file(&path) else {
                continue;
            };
            if path_matches_project_dir(&cwd, project_dir) {
                newest = Some((modified, path));
            }
        }
    }
    newest.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const FIXTURE: &str = r#"{"type":"user","message":{"role":"user","content":"hi"}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":2,"output_tokens":4205,"cache_read_input_tokens":243320,"cache_creation_input_tokens":2232,"server_tool_use":{"web_search_requests":0}}}}
"#;

    #[test]
    fn read_used_tokens_unsupported_harness_is_none() {
        let tmp = NamedTempFile::new().unwrap();
        assert!(read_used_tokens(Harness::Codex, tmp.path()).is_none());
        assert!(read_used_tokens(Harness::OpenCode, tmp.path()).is_none());
        assert!(
            read_used_tokens(Harness::Claude, Path::new("/no/such/transcript.jsonl")).is_none()
        );
    }

    #[test]
    fn transcript_context_pct_end_to_end() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(FIXTURE.as_bytes()).unwrap();
        let pct = transcript_context_pct(Harness::Claude, tmp.path(), "claude-opus-4-8").unwrap();
        assert_eq!(pct, 100.0);

        let mut small = NamedTempFile::new().unwrap();
        small
            .write_all(
                b"{\"message\":{\"usage\":{\"input_tokens\":50000,\"output_tokens\":10000}}}\n",
            )
            .unwrap();
        let pct = transcript_context_pct(Harness::Claude, small.path(), "sonnet").unwrap();
        assert_eq!(pct, 30.0);

        let mut codex = NamedTempFile::new().unwrap();
        codex
            .write_all(
                br#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20000,"cached_input_tokens":10000,"output_tokens":0},"model_context_window":100000}}}
"#,
            )
            .unwrap();
        assert_eq!(
            transcript_context_pct(Harness::Codex, codex.path(), "gpt-5"),
            Some(30.0)
        );
        assert!(transcript_context_pct(Harness::OpenCode, tmp.path(), "opus").is_none());
    }

    #[test]
    fn latest_claude_transcript_picks_newest_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        assert!(latest_claude_transcript(dir.path()).is_none());

        let older = dir.path().join("old-session.jsonl");
        let newer = dir.path().join("new-session.jsonl");
        std::fs::write(&older, b"{}").unwrap();
        std::fs::write(&newer, b"{}").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        filetime::set_file_mtime(&newer, filetime::FileTime::from_system_time(later)).unwrap();

        assert_eq!(latest_claude_transcript(dir.path()), Some(newer));

        let txt = dir.path().join("zzz.txt");
        std::fs::write(&txt, b"x").unwrap();
        let even_later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        filetime::set_file_mtime(&txt, filetime::FileTime::from_system_time(even_later)).unwrap();
        assert_eq!(
            latest_claude_transcript(dir.path()).unwrap().extension(),
            Some(std::ffi::OsStr::new("jsonl"))
        );
    }

    #[test]
    fn latest_claude_transcript_missing_dir_is_none() {
        assert!(latest_claude_transcript(Path::new("/no/such/projects/dir")).is_none());
    }

    #[test]
    fn latest_codex_transcript_picks_newest_matching_project() {
        let home = tempfile::tempdir().unwrap();
        let day = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("15");
        std::fs::create_dir_all(&day).unwrap();
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let older = day.join("rollout-old.jsonl");
        let newer = day.join("rollout-new.jsonl");
        let other = day.join("rollout-other.jsonl");
        let non_rollout = day.join("notes.jsonl");

        std::fs::write(
            &older,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &newer,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &other,
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/tmp/other\"}}\n",
        )
        .unwrap();
        std::fs::write(
            &non_rollout,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();

        let old_time = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(20);
        let other_time = std::time::SystemTime::now() + std::time::Duration::from_secs(30);
        let non_rollout_time = std::time::SystemTime::now() + std::time::Duration::from_secs(40);
        filetime::set_file_mtime(&older, filetime::FileTime::from_system_time(old_time)).unwrap();
        filetime::set_file_mtime(&newer, filetime::FileTime::from_system_time(new_time)).unwrap();
        filetime::set_file_mtime(&other, filetime::FileTime::from_system_time(other_time)).unwrap();
        filetime::set_file_mtime(
            &non_rollout,
            filetime::FileTime::from_system_time(non_rollout_time),
        )
        .unwrap();

        assert_eq!(latest_codex_transcript(home.path(), &project), Some(newer));
    }
}
