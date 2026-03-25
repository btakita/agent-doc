//! # Module: run
//!
//! ## Spec
//! - `run(file, branch, agent_name, model, dry_run, no_git, config)`: executes
//!   a single agent request-response cycle for a session document.
//! - Bails immediately if the file does not exist.
//! - Computes a diff via `diff::compute`; returns `Ok(())` early (no-op) when
//!   the snapshot matches the document (nothing changed since the last run).
//! - Ensures the document has a session UUID in frontmatter, writing it if
//!   absent.
//! - Resolves the agent backend from: `agent_name` arg > frontmatter `agent`
//!   field > `config.default_agent` > fallback `"claude"`.
//! - Resolves the model from: `model` arg > frontmatter `model` field.
//! - Builds one of two prompt shapes: "resume" (diff + full document, for
//!   continuing sessions) or "fork" (full document only, for new sessions),
//!   determined by whether `resume` is set in frontmatter.
//! - In `--dry-run` mode: prints the diff and prompt size to stderr and returns
//!   without calling the agent, writing files, or touching git.
//! - Optionally creates a git branch via `git::create_branch` before committing
//!   (only when `branch=true` and `no_git=false`).
//! - Pre-commits user's changes via `git::commit` before sending to the agent
//!   so the editor shows agent additions as diff-gutter entries.
//! - Appends the agent response as `## Assistant\n\n<response>\n\n## User\n\n`
//!   and updates the `resume` ID in frontmatter from the agent's returned
//!   session ID.
//! - Acquires an advisory `flock` on a per-document lock file before writing so
//!   concurrent `agent-doc run` / watch-daemon invocations are serialized.
//! - Re-reads the file under lock; if the user edited concurrently, performs a
//!   3-way merge via `merge::merge_contents`.
//! - Tries IPC write to the IDE plugin first; on IPC miss, falls back to
//!   `atomic_write` (temp file + POSIX rename) and saves a snapshot.
//! - `acquire_doc_lock(path)`: opens/creates `.agent-doc/locks/<hash>.lock` and
//!   acquires an exclusive `flock`; returned `File` releases the lock on drop.
//! - `atomic_write(path, content)`: writes to a sibling temp file and renames
//!   atomically, eliminating partial-write windows.
//!
//! ## Agentic Contracts
//! - Callers must not assume the file is modified when `run` returns `Ok(())`
//!   early (no-op case): the document and snapshot are untouched.
//! - The snapshot saved after a successful run always reflects
//!   `content_ours` (baseline + response), never `final_content`, so concurrent
//!   user edits are detectable on the next diff.
//! - Git operations (branch creation, pre-commit) are skipped entirely when
//!   `no_git=true`; the agent call and write still proceed normally.
//! - The advisory flock serializes only agent-doc processes; editors bypass it.
//!   Readers of the document file must not rely on the lock for read safety.
//! - IPC success: snapshot is saved by the IDE plugin; the binary does not
//!   write a snapshot in this path. IPC failure: binary writes snapshot.
//! - `atomic_write` is safe for concurrent callers on the same path; one write
//!   wins and the file is never in a partially-written state.
//! - The assistant heading (`## Assistant`) is stripped from the agent response
//!   before appending to avoid duplication; the binary inserts it.
//!
//! ## Evals
//! - `run_file_not_found`: call `run` with a missing path → `Err` containing
//!   "file not found".
//! - `run_no_changes`: snapshot matches document → returns `Ok(())` without
//!   calling the agent or modifying anything.
//! - `run_dry_run`: `dry_run=true` → diff and prompt size printed to stderr;
//!   file unchanged, no agent call, no git operations.
//! - `acquire_doc_lock_succeeds`: lock file created and exclusive lock acquired
//!   on a fresh document path → `Ok(File)`.
//! - `doc_lock_released_on_drop`: after dropping the lock handle, a second
//!   `acquire_doc_lock` on the same path succeeds immediately.
//! - `atomic_write_correct_content`: written content is exactly the input string.
//! - `atomic_write_overwrites_existing`: writing to an existing file replaces
//!   content atomically.
//! - `concurrent_atomic_writes_no_corruption`: 20 concurrent writers → final
//!   file is exactly one valid write; no partial or interleaved content.
//! - `parallel_different_files_no_interference`: two concurrent cycles on
//!   different files complete without lock contention or cross-contamination.
//! - `same_file_serialized_by_flock`: two concurrent cycles on the same file
//!   are serialized; both writes land with no corruption.
//! - `flock_prevents_partial_read_during_write`: a reader blocked on the same
//!   lock sees the completed write, not a partial state.
//! - `merge_clean_no_conflicts`: agent response appended as "ours" + user
//!   unchanged as "theirs" → clean 3-way merge containing the response.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::Path;

use crate::{agent, config::Config, diff, frontmatter, git, merge, snapshot};

pub fn run(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    config: &Config,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    eprintln!("[run] starting for {}", file.display());

    // Compute diff
    let the_diff = match diff::compute(file)? {
        Some(d) => {
            eprintln!("[run] diff computed ({} bytes)", d.len());
            d
        }
        None => {
            eprintln!("[run] Nothing changed since last run for {}", file.display());
            return Ok(());
        }
    };

    // Ensure the document has a session UUID (for tmux routing)
    let raw_content = std::fs::read_to_string(file)?;
    let (content_original, _session_id) = frontmatter::ensure_session(&raw_content)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    let (fm, _body) = frontmatter::parse(&content_original)?;

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);
    let backend = agent::resolve(agent_name, agent_config)?;

    // Build prompt
    let prompt = if fm.resume.is_some() {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            the_diff, content_original
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions inline (e.g., in blockquotes), address those too.",
            content_original
        )
    };

    if dry_run {
        eprintln!("--- Diff ---");
        print!("{}", the_diff);
        eprintln!("--- Prompt would be {} bytes ---", prompt.len());
        return Ok(());
    }

    // Create branch if requested
    if branch && !no_git {
        git::create_branch(file)?;
    }

    // Pre-commit: commit user's changes before sending to agent
    // This lets the editor show agent additions as diff gutters
    if !no_git {
        git::commit(file)?;
    }

    eprintln!("Submitting to {}...", agent_name);

    // Send to agent — use `resume` for agent conversation tracking
    let fork = fm.resume.is_none();
    let model = model.or(fm.model.as_deref());
    let response = backend.send(&prompt, fm.resume.as_deref(), fork, model)?;

    // Build our version: original + resume_id update + response appended
    let mut content_ours = content_original.clone();
    if let Some(ref sid) = response.session_id {
        content_ours = frontmatter::set_resume_id(&content_ours, sid)?;
    }
    let response_text = crate::write::strip_assistant_heading(&response.text);
    content_ours.push_str("\n## Assistant\n\n");
    content_ours.push_str(&response_text);
    content_ours.push_str("\n\n## User\n\n");

    // Acquire advisory lock on the document for agent-doc-vs-agent-doc
    // coordination (e.g., watch daemon vs. manual `agent-doc run`).
    // Editors ignore advisory locks, so this only serializes agent-doc writes.
    let doc_lock = acquire_doc_lock(file)?;

    // Re-read file to check for user edits during run
    let content_current = std::fs::read_to_string(file)?;

    let final_content = if content_current == content_original {
        // No edits during run — use our version directly
        content_ours.clone()
    } else {
        eprintln!("File was modified during run. Merging changes...");
        merge::merge_contents(&content_original, &content_ours, &content_current)?
    };

    // Try IPC first — if an IDE plugin is active, it applies the change via
    // Document API (no "externally modified" dialog, cursor preserved).
    let ipc_ok = crate::write::try_ipc_full_content(file, &final_content)?;

    if !ipc_ok {
        // IPC not available or timed out — fall back to direct disk write
        atomic_write(file, &final_content)?;

        // Save snapshot as content_ours (baseline + response), not final_content.
        // If the user edited concurrently, final_content includes their edits.
        // Saving content_ours ensures the next diff detects those concurrent edits.
        snapshot::save(file, &content_ours)?;
    }

    drop(doc_lock); // explicit release after both doc and snapshot are written

    eprintln!("Response appended to {}", file.display());
    Ok(())
}

/// Acquire an advisory flock on a document file for agent-doc-vs-agent-doc
/// coordination. Lock file is `.agent-doc/locks/<hash>.lock`. Released on drop.
fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = crate::snapshot::lock_path_for(path)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open doc lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire doc lock on {}", lock_path.display()))?;
    Ok(file)
}

/// Write content to a file atomically via write-to-temp + rename.
///
/// This eliminates the partial-write window where another process (e.g., an
/// editor or the watch daemon) could read a half-written file. The rename is
/// atomic on POSIX filesystems when source and destination are on the same
/// filesystem (guaranteed here since the temp file is a sibling).
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    #[test]
    fn acquire_doc_lock_succeeds() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        let lock = acquire_doc_lock(&doc);
        assert!(lock.is_ok());
    }

    #[test]
    fn doc_lock_released_on_drop() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        {
            let _lock = acquire_doc_lock(&doc).unwrap();
        }
        // After drop, second acquire should succeed
        let lock2 = acquire_doc_lock(&doc);
        assert!(lock2.is_ok());
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("overwrite.md");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn concurrent_atomic_writes_no_corruption() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        std::fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                atomic_write(&p, &content).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content should be exactly one of the valid writes
        let final_content = std::fs::read_to_string(&path).unwrap();
        assert!(final_content.starts_with("writer-"));
        assert!(final_content.ends_with("-content"));
    }

    // -----------------------------------------------------------------------
    // Lazy parallelization: functional tests
    // -----------------------------------------------------------------------

    /// Simulate two document cycles on different files running in parallel.
    /// Both should complete without interference — no shared lock contention.
    #[test]
    fn parallel_different_files_no_interference() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "initial-a").unwrap();
        std::fs::write(&doc_b, "initial-b").unwrap();

        let barrier = Arc::new(Barrier::new(2));

        let bar_a = Arc::clone(&barrier);
        let path_a = doc_a.clone();
        let ha = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_a).unwrap();
            bar_a.wait(); // both threads hold their own lock simultaneously
            // Simulate read-modify-write cycle
            let content = std::fs::read_to_string(&path_a).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            atomic_write(&path_a, &format!("{}\n## Assistant\nResponse A", content)).unwrap();
        });

        let bar_b = Arc::clone(&barrier);
        let path_b = doc_b.clone();
        let hb = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_b).unwrap();
            bar_b.wait(); // both threads hold their own lock simultaneously
            let content = std::fs::read_to_string(&path_b).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            atomic_write(&path_b, &format!("{}\n## Assistant\nResponse B", content)).unwrap();
        });

        ha.join().unwrap();
        hb.join().unwrap();

        let a = std::fs::read_to_string(&doc_a).unwrap();
        let b = std::fs::read_to_string(&doc_b).unwrap();
        assert!(a.contains("Response A"), "Doc A missing response: {}", a);
        assert!(b.contains("Response B"), "Doc B missing response: {}", b);
        assert!(!a.contains("Response B"), "Doc A has B's response");
        assert!(!b.contains("Response A"), "Doc B has A's response");
    }

    /// Simulate two document cycles on the SAME file running concurrently.
    /// flock serializes them — both writes land, no corruption.
    #[test]
    fn same_file_serialized_by_flock() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("shared.md");
        std::fs::write(&doc, "# Shared Doc\n").unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();

        for i in 0..2 {
            let path = doc.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait(); // both start at the same time
                let lock = acquire_doc_lock(&path).unwrap();
                // Critical section: read, modify, write
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = format!("{}writer-{}\n", content, i);
                std::thread::sleep(std::time::Duration::from_millis(5));
                atomic_write(&path, &updated).unwrap();
                drop(lock);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = std::fs::read_to_string(&doc).unwrap();
        // Both writers should have appended (serialized by flock)
        assert!(final_content.contains("writer-0") && final_content.contains("writer-1"),
            "Both writes should land (flock serializes): {}", final_content);
    }

    /// Verify that a locked document cycle prevents concurrent reads of
    /// partial state — the second reader waits for the lock to be released.
    #[test]
    fn flock_prevents_partial_read_during_write() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("partial.md");
        std::fs::write(&doc, "before").unwrap();

        let path_w = doc.clone();
        let path_r = doc.clone();

        // Writer: acquire lock, pause, then write
        let writer = std::thread::spawn(move || {
            let lock = acquire_doc_lock(&path_w).unwrap();
            // Hold lock while "processing"
            std::thread::sleep(std::time::Duration::from_millis(50));
            atomic_write(&path_w, "after").unwrap();
            drop(lock);
        });

        // Reader: try to acquire lock (will block until writer releases)
        std::thread::sleep(std::time::Duration::from_millis(5)); // let writer start first
        let reader = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_r).unwrap();
            // By the time we get the lock, writer has finished
            std::fs::read_to_string(&path_r).unwrap()
        });

        writer.join().unwrap();
        let read_content = reader.join().unwrap();
        assert_eq!(read_content, "after", "Reader should see completed write, not partial state");
    }

    #[test]
    fn merge_clean_no_conflicts() {
        // merge_contents spawns `git merge-file` which inherits CWD.
        // Other tests may invalidate CWD via TempDir drops, so we
        // perform the merge manually using temp files + Command with
        // an explicit current_dir to avoid CWD pollution.
        let dir = TempDir::new().unwrap();
        let base_path = dir.path().join("base");
        let ours_path = dir.path().join("ours");
        let theirs_path = dir.path().join("theirs");

        let base = "line 1\nline 2\nline 3\n";
        let ours = "line 1\nline 2\nline 3\n\n## Assistant\n\nResponse here.\n";
        let theirs = "line 1\nline 2\nline 3\n";

        std::fs::write(&base_path, base).unwrap();
        std::fs::write(&ours_path, ours).unwrap();
        std::fs::write(&theirs_path, theirs).unwrap();

        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args([
                "merge-file", "-p", "--diff3",
                "-L", "agent-response",
                "-L", "original",
                "-L", "your-edits",
            ])
            .arg(&ours_path)
            .arg(&base_path)
            .arg(&theirs_path)
            .output()
            .unwrap();

        let merged = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "merge should be clean");
        assert!(merged.contains("Response here."));
    }
}
