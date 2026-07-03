//! Route startup debounce wait.

use agent_doc_debounce::TypingIndicatorStatus;
use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Wait for the file's mtime and editor typing indicator to settle.
///
/// Polls every 100ms, up to 10x the debounce duration as a safety cap. Route
/// must fail closed instead of proceeding through a visible document mutation
/// while the editor-side typing indicator is still active.
pub fn await_idle(file: &Path, debounce: Duration) -> Result<()> {
    await_idle_with_max_wait(file, debounce, debounce * 10)
}

pub fn await_idle_with_max_wait(file: &Path, debounce: Duration, max_wait: Duration) -> Result<()> {
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let debounce_ms = debounce.as_millis().min(u64::MAX as u128) as u64;
    let file_str = file.to_string_lossy();

    loop {
        let indicator = agent_doc_debounce::typing_indicator_status(&file_str, debounce_ms);

        // When an editor owns the typing lifecycle and its indicator reports
        // idle, the editor already debounced in-process before saving and
        // routing. Trust the cross-process typing signal and skip the redundant
        // mtime wait for editor pre-route saves.
        match indicator {
            TypingIndicatorStatus::Idle => {
                eprintln!(
                    "[route] debounce OK - editor typing indicator idle (skipping redundant mtime settle for editor pre-route save)"
                );
                return Ok(());
            }
            TypingIndicatorStatus::Active => {
                // Editor reports active typing - keep waiting regardless of mtime.
            }
            TypingIndicatorStatus::Absent => {
                let mtime = std::fs::metadata(file)
                    .and_then(|m| m.modified())
                    .with_context(|| format!("failed to stat {}", file.display()))?;
                let elapsed_since_edit = mtime.elapsed().unwrap_or(Duration::ZERO);
                if elapsed_since_edit >= debounce {
                    eprintln!(
                        "[route] debounce OK - file idle for {:.1}s and no editor typing indicator",
                        elapsed_since_edit.as_secs_f64(),
                    );
                    return Ok(());
                }
            }
        }

        if start.elapsed() >= max_wait {
            let elapsed_since_edit = std::fs::metadata(file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .unwrap_or(Duration::ZERO);
            anyhow::bail!(
                "route deferred for {}: document did not settle within {}ms (mtime_idle_ms={}, typing_active={}); retry after typing stops",
                file.display(),
                max_wait.as_millis(),
                elapsed_since_edit.as_millis(),
                indicator == TypingIndicatorStatus::Active
            );
        }

        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_debounce_fails_closed_while_typing_indicator_is_active() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "prompt in progress\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);

        let err =
            await_idle_with_max_wait(&doc, Duration::from_millis(500), Duration::from_millis(25))
                .expect_err("route must not proceed while the editor typing indicator is active");

        assert!(
            err.to_string().contains("typing_active=true"),
            "route debounce error should prove the active typing reason: {err}"
        );
    }

    #[test]
    fn route_debounce_allows_dispatch_after_typing_indicator_expires() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);

        await_idle_with_max_wait(&doc, Duration::from_millis(10), Duration::from_millis(1000))
            .expect("route should proceed after mtime and typing indicator are both idle");
    }

    #[test]
    fn route_dispatches_immediately_when_idle_typing_indicator_present_despite_fresh_mtime() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        agent_doc_debounce::document_changed(&doc_str);
        std::thread::sleep(Duration::from_millis(80));

        assert_eq!(
            agent_doc_debounce::typing_indicator_status(&doc_str, 50),
            agent_doc_debounce::TypingIndicatorStatus::Idle,
            "indicator should report idle after the debounce window elapses"
        );

        std::fs::write(&doc, "settled prompt\n").unwrap();

        let start = Instant::now();
        await_idle_with_max_wait(&doc, Duration::from_millis(50), Duration::from_millis(2000))
            .expect("an idle editor typing indicator must authorize immediate dispatch");
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "route must not re-impose the mtime debounce when the editor indicator is idle (elapsed {:?})",
            start.elapsed()
        );
    }
}
