//! # Module: parallel
//!
//! ## Spec
//! - Orchestrates fan-out of multiple Claude tasks across isolated git worktrees inside tmux panes.
//! - For each task: optionally creates a git worktree (`worktree::create`), writes the task prompt to `.agent-doc-prompt.txt`, spawns `claude -p --output-format json` in a new tmux pane, and stashes the pane out of the user's view.
//! - With `--no-worktree`: all tasks run in the project root directory (no isolation).
//! - Polls for pane death at a fixed 2-second interval; enforces a per-task timeout (kills remaining panes when exceeded).
//! - Collects results: extracts `"result"` text from Claude's JSON output, reads the stderr log, and optionally reads `git diff` from each worktree.
//! - Formats all results as a markdown document (`## Deep Results` → `### Task N — …` sections with collapsible diff and stderr blocks) and prints to stdout.
//! - `--dry-run` prints the plan (project root, session, task list) and exits without spawning.
//!
//! ## Agentic Contracts
//! - `run(file, config)` — returns `Err` if the file is missing or the project root cannot be found; returns `Ok(())` after all tasks complete or time out.
//! - Result text is extracted from the last valid JSON line with a `"result"` field; raw JSON is used as fallback when extraction fails.
//! - Pane stashing is best-effort; stash failures emit a warning to stderr and do not abort the run.
//! - Output format is stable markdown; callers may parse the `### Task N —` header pattern.
//!
//! ## Evals
//! - dry_run: valid file + tasks + `dry_run: true` → prints plan to stderr, no panes spawned, exits Ok
//! - no_tasks: valid file + empty task list → emits warning to stderr, exits Ok immediately
//! - extract_result_text: Claude JSON with `"result"` field → extracted text returned; malformed JSON → raw content fallback
//! - timeout_kills_panes: tasks that exceed `timeout_secs` → panes killed, results collected with whatever output exists
//! - format_results: mixed output/diff/error results → markdown with correct section headers and collapsible blocks

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_doc_frontmatter::frontmatter;
use tmux_router::Tmux;

/// Configuration for a deep run.
pub struct ParallelConfig {
    /// Task descriptions/prompts — one Claude session per task.
    pub tasks: Vec<ParallelTask>,
    /// Model override (e.g., "opus", "sonnet").
    pub model: Option<String>,
    /// Skip git operations (reserved for future use).
    #[allow(dead_code)]
    pub no_git: bool,
    /// Skip worktree creation — run all tasks in the project root.
    pub no_worktree: bool,
    /// Timeout in seconds per task (kills pane if exceeded).
    pub timeout_secs: u64,
    /// Just print the plan and exit.
    pub dry_run: bool,
}

/// Parallel task descriptor with a user-facing label and concrete prompt text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelTask {
    pub description: String,
    pub prompt: String,
}

/// Tracks state for a single spawned task.
struct TaskState {
    index: usize,
    description: String,
    worktree_path: PathBuf,
    pane_id: String,
    completed: bool,
}

/// Collected result from a completed task.
struct TaskResult {
    index: usize,
    description: String,
    output: Option<String>,
    diff: Option<String>,
    error: Option<String>,
}

const PROMPT_FILENAME: &str = ".agent-doc-prompt.txt";
const RESULT_FILENAME: &str = ".agent-doc-result.json";
const LOG_FILENAME: &str = ".agent-doc-result.log";
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Run the deep orchestration pipeline.
pub fn run(file: &Path, config: ParallelConfig) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Step 1: Find project root
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_fs::find_project_root(&canonical).ok_or_else(|| {
        anyhow::anyhow!("no .agent-doc/ project root found for {}", file.display())
    })?;

    // Step 2: Read frontmatter for agent_doc_session
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _body) = frontmatter::parse(&content)?;

    // tmux_session frontmatter field is deprecated — use current tmux session
    let session_name = {
        let tmux = Tmux::default_server();
        tmux.current_session()
            .unwrap_or_else(|| "claude".to_string())
    };

    let agent_doc_session = fm.session.as_deref().unwrap_or("deep").to_string();

    eprintln!("[parallel] Project root: {}", project_root.display());
    eprintln!("[parallel] Tmux session: {}", session_name);
    eprintln!("[parallel] Tasks: {}", config.tasks.len());

    for (i, task) in config.tasks.iter().enumerate() {
        eprintln!("[parallel]   {}: {}", i + 1, task.description);
    }

    // Dry run: print plan and exit
    if config.dry_run {
        eprintln!("[parallel] Dry run — exiting without spawning tasks.");
        return Ok(());
    }

    if config.tasks.is_empty() {
        eprintln!("[parallel] No tasks provided.");
        return Ok(());
    }

    // Step 3: Create worktrees (unless --no-worktree) and spawn Claude in tmux panes
    let tmux = Tmux::default_server();
    let mut task_states: Vec<TaskState> = Vec::new();

    for (i, task) in config.tasks.iter().enumerate() {
        let task_cwd = if config.no_worktree {
            eprintln!(
                "[parallel] Task {}/{} (no worktree, using project root)",
                i + 1,
                config.tasks.len()
            );
            project_root.clone()
        } else {
            eprintln!(
                "[parallel] Creating worktree for task {}/{}...",
                i + 1,
                config.tasks.len()
            );
            let worktree_info = crate::worktree::create(&project_root, &agent_doc_session, i)?;
            eprintln!("[parallel]   Worktree: {}", worktree_info.path.display());
            worktree_info.path
        };

        // Write prompt to file in task directory
        let prompt_path = task_cwd.join(PROMPT_FILENAME);
        std::fs::write(&prompt_path, &task.prompt)
            .with_context(|| format!("failed to write prompt to {}", prompt_path.display()))?;

        // Create tmux pane in the session
        let pane_id = tmux
            .auto_start(&session_name, &task_cwd)
            .with_context(|| format!("failed to create tmux pane for task {}", i + 1))?;
        eprintln!("[parallel]   Pane: {}", pane_id);

        // Build the claude command
        let mut cmd_parts = Vec::new();
        cmd_parts.push("claude -p --output-format json".to_string());
        if let Some(ref model) = config.model {
            cmd_parts.push(format!("--model {}", model));
        }
        cmd_parts.push(format!(
            "< {} > {} 2>{}; exit",
            PROMPT_FILENAME, RESULT_FILENAME, LOG_FILENAME
        ));
        // Prepend frontmatter env exports (unexpanded — target shell handles $(passage ...)
        // so secrets never appear in the tmux send-keys argument list or scrollback).
        let env_prefix = agent_doc_config::env::shell_export_prefix(&fm.env);
        let cmd_str = format!("{}{}", env_prefix, cmd_parts.join(" "));

        // Send the command to the pane
        agent_doc_tmux_io::send_submitted_text_logged(
            &tmux,
            &pane_id,
            &cmd_str,
            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                None,
                agent_doc_orchestration::ops_log::log_op,
            ),
            "sessions.send_submitted_text",
        )
        .with_context(|| format!("failed to send keys to pane {} for task {}", pane_id, i + 1))?;

        // Stash the pane so it doesn't clutter the user's view
        if let Err(e) = tmux.stash_pane(&pane_id, &session_name) {
            eprintln!(
                "[parallel]   Warning: failed to stash pane {}: {}",
                pane_id, e
            );
        }

        task_states.push(TaskState {
            index: i,
            description: task.description.clone(),
            worktree_path: task_cwd,
            pane_id,
            completed: false,
        });
    }

    // Step 4: Poll for completion
    eprintln!("[parallel] Polling for task completion...");
    let start = Instant::now();
    let timeout = Duration::from_secs(config.timeout_secs);

    loop {
        let all_done = task_states.iter().all(|t| t.completed);
        if all_done {
            break;
        }

        // Check timeout
        if start.elapsed() > timeout {
            eprintln!(
                "[parallel] Timeout reached ({} seconds). Killing remaining panes...",
                config.timeout_secs
            );
            for task in &mut task_states {
                if !task.completed {
                    eprintln!(
                        "[parallel]   Killing pane {} (task {})",
                        task.pane_id,
                        task.index + 1
                    );
                    if let Err(e) = tmux.kill_pane(&task.pane_id) {
                        eprintln!(
                            "[parallel]   Warning: failed to kill pane {}: {}",
                            task.pane_id, e
                        );
                    }
                    task.completed = true;
                }
            }
            break;
        }

        // Check each pane
        let total = task_states.len();
        for task in &mut task_states {
            if task.completed {
                continue;
            }
            if !tmux.pane_alive(&task.pane_id) {
                task.completed = true;
                eprintln!("[parallel] Task {}/{} complete.", task.index + 1, total);
            }
        }

        let completed = task_states.iter().filter(|t| t.completed).count();
        if completed < task_states.len() {
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    // Step 5: Collect results
    eprintln!("[parallel] Collecting results...");
    let mut results: Vec<TaskResult> = Vec::new();

    for task in &task_states {
        let result_path = task.worktree_path.join(RESULT_FILENAME);
        let log_path = task.worktree_path.join(LOG_FILENAME);

        // Read JSON output
        let output = match std::fs::read_to_string(&result_path) {
            Ok(content) if !content.trim().is_empty() => {
                // Try to extract the "result" text from Claude's JSON output
                match extract_result_text(&content) {
                    Some(text) => Some(text),
                    None => Some(content),
                }
            }
            Ok(_) => None,
            Err(e) => {
                eprintln!(
                    "[parallel]   Warning: could not read result for task {}: {}",
                    task.index + 1,
                    e
                );
                // Try reading the log file for error info
                let log_content = std::fs::read_to_string(&log_path).ok();
                if let Some(ref log) = log_content
                    && !log.trim().is_empty()
                {
                    eprintln!("[parallel]   Log: {}", log.trim());
                }
                None
            }
        };

        // Read error log
        let error = std::fs::read_to_string(&log_path)
            .ok()
            .filter(|s| !s.trim().is_empty());

        // Get diff from worktree (skip if --no-worktree)
        let diff = if config.no_worktree {
            None
        } else {
            crate::worktree::diff(&task.worktree_path)
                .ok()
                .filter(|s| !s.trim().is_empty())
        };

        results.push(TaskResult {
            index: task.index,
            description: task.description.clone(),
            output,
            diff,
            error,
        });
    }

    // Step 6: Format and print results as markdown
    let markdown = format_results(&results);
    println!("{}", markdown);

    eprintln!("[parallel] Done. {} tasks completed.", results.len());
    Ok(())
}

/// Extract the result text from Claude's `--output-format json` output.
///
/// The JSON format is: `{"type":"result","result":"...","session_id":"..."}`
fn extract_result_text(json_str: &str) -> Option<String> {
    // Parse as JSON value — may be a single object or newline-delimited
    // Take the last line that parses as valid JSON with a "result" field
    for line in json_str.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(result) = val.get("result").and_then(|v| v.as_str())
        {
            return Some(result.to_string());
        }
    }
    None
}

/// Format collected results as markdown with collapsible sections.
fn format_results(results: &[TaskResult]) -> String {
    let mut out = String::new();
    out.push_str("## Deep Results\n\n");

    for result in results {
        out.push_str(&format!(
            "### Task {} — {}\n\n",
            result.index + 1,
            result.description
        ));

        // Agent output
        if let Some(ref output) = result.output {
            out.push_str(output);
            out.push_str("\n\n");
        } else {
            out.push_str("*No output captured.*\n\n");
        }

        // Diff (collapsible)
        if let Some(ref diff) = result.diff {
            out.push_str("<details>\n<summary>Git diff</summary>\n\n```diff\n");
            out.push_str(diff);
            out.push_str("\n```\n\n</details>\n\n");
        }

        // Errors (collapsible)
        if let Some(ref error) = result.error {
            out.push_str("<details>\n<summary>Stderr log</summary>\n\n```\n");
            out.push_str(error);
            out.push_str("\n```\n\n</details>\n\n");
        }
    }

    out
}
