//! Preflight debounce and editor-typing wait adapters.

use anyhow::Result;
use std::path::Path;

pub fn preflight_debounce_ms(file: &Path) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|content| {
            agent_doc_frontmatter::frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.debounce_ms)
        })
        .unwrap_or(2000)
}

pub fn wait_for_typing_idle_before_mutation(file: &Path) -> Result<()> {
    let debounce_ms = preflight_debounce_ms(file);
    let max_wait = agent_doc_debounce::preflight_debounce_max_wait(debounce_ms);
    let poll = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    let file_str = file.to_string_lossy();

    loop {
        let typing_active = agent_doc_debounce::is_typing_via_file(&file_str, debounce_ms);
        if !typing_active {
            return Ok(());
        }
        if start.elapsed() >= max_wait {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_visible_mutation_deferred_active_typing file={} debounce_ms={} timeout_ms={}",
                    file.display(),
                    debounce_ms,
                    max_wait.as_millis()
                ),
            );
            anyhow::bail!(
                "preflight deferred for {}: editor typing did not settle within {}ms; retry after typing stops",
                file.display(),
                max_wait.as_millis()
            );
        }
        std::thread::sleep(poll);
    }
}

pub fn wait_for_preflight_debounce(file: &Path) {
    let debounce_ms = preflight_debounce_ms(file);
    let debounce = std::time::Duration::from_millis(debounce_ms);
    let max_wait = agent_doc_debounce::preflight_debounce_max_wait(debounce_ms);
    let poll = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    let file_str = file.to_string_lossy();
    tracing::debug!(debounce_ms, file = %file.display(), "preflight debounce starting");

    loop {
        let idle_for = std::fs::metadata(file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .unwrap_or(debounce);

        let typing_active = agent_doc_debounce::is_typing_via_file(&file_str, debounce_ms);
        tracing::trace!(
            idle_ms = idle_for.as_millis() as u64,
            typing_active,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "preflight debounce poll"
        );

        if idle_for >= debounce && !typing_active {
            tracing::debug!(
                idle_ms = idle_for.as_millis() as u64,
                waited_ms = start.elapsed().as_millis() as u64,
                "preflight debounce settled"
            );
            break;
        }
        if start.elapsed() >= max_wait {
            if typing_active {
                tracing::warn!(
                    waited_ms = start.elapsed().as_millis() as u64,
                    "preflight debounce timeout (typing still active)"
                );
                eprintln!(
                    "[preflight] typing indicator active but timeout after {:.1}s — proceeding",
                    start.elapsed().as_secs_f64()
                );
            } else {
                tracing::warn!(
                    waited_ms = start.elapsed().as_millis() as u64,
                    "preflight debounce timeout (mtime not settled)"
                );
                eprintln!(
                    "[preflight] mtime debounce timeout after {:.1}s — proceeding",
                    start.elapsed().as_secs_f64()
                );
            }
            break;
        }
        std::thread::sleep(poll);
    }
}
