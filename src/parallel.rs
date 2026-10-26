//! # Module: parallel
//!
//! ## Spec
//! - Orchestrates fan-out of multiple Claude tasks across isolated git worktrees inside tmux panes.
//! - For each task: optionally creates a git worktree (`worktree::create`), writes the task prompt to `.agent-doc-prompt.txt`, launches `claude -p --output-format json` as a new tmux pane's own process (`agent_doc_tmux_io::launch_command_in_pane_at`, never typed into the pane's fresh shell), and stashes the pane out of the user's view. The pane ends when `claude` exits.
//! - Frontmatter `env` exports are prepended unexpanded; the pane's `/bin/sh` expands `$(passage ...)`, so secret values never reach the tmux argv, the process list, the ops log, or scrollback.
//! - With `--no-worktree`: all tasks run in the project root directory (no isolation).
//! - Polls for pane death at a fixed 2-second interval; enforces a per-task timeout (kills remaining panes when exceeded).
//! - Collects results: extracts `"result"` text from Claude's JSON output, reads the stderr log, and optionally reads `git diff` from each worktree.
//! - Disposes of each worktree it created after results are collected (`#orchworktreeleak`): removes the ones holding only orchestrate scaffolding, keeps and names the ones holding authored work. The timeout path breaks into collection rather than returning, so killed tasks reach the same decision.
//! - Unwinds every worktree it created when the fan-out aborts mid-spawn (worktree creation, prompt write, pane creation, or pane launch failure), instead of stranding them.
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
//! - worktree disposal is delegated to `worktree::classify_worktree_residue`; see that module's evals
//! - format_results: mixed output/diff/error results → markdown with correct section headers and collapsible blocks

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_doc_frontmatter::frontmatter;

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
    /// `None` under `--no-worktree`: the task ran in the project root, which is
    /// never ours to remove.
    worktree: Option<crate::worktree::WorktreeInfo>,
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
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "parallel_command_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    let (fm, _body) = frontmatter::parse(&content)?;

    // tmux_session frontmatter field is deprecated — use current tmux session
    let session_name = {
        let tmux = agent_doc_tmux_io::configured_tmux();
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
    let tmux = agent_doc_tmux_io::configured_tmux();
    let mut task_states: Vec<TaskState> = Vec::new();

    for (i, task) in config.tasks.iter().enumerate() {
        let mut created_worktree: Option<crate::worktree::WorktreeInfo> = None;
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
            let worktree_info = match crate::worktree::create(&project_root, &agent_doc_session, i)
            {
                Ok(info) => info,
                Err(err) => {
                    // `#orchworktreeleak`: bailing out mid-fan-out used to strand
                    // every worktree already created for this run.
                    remove_task_worktrees(&project_root, &task_states);
                    return Err(err);
                }
            };
            eprintln!("[parallel]   Worktree: {}", worktree_info.path.display());
            created_worktree = Some(worktree_info);
            created_worktree
                .as_ref()
                .expect("worktree just assigned")
                .path
                .clone()
        };

        // Write prompt to file in task directory
        let prompt_path = task_cwd.join(PROMPT_FILENAME);
        if let Err(err) = std::fs::write(&prompt_path, &task.prompt) {
            abort_spawn(&project_root, &task_states, created_worktree.as_ref());
            return Err(err)
                .with_context(|| format!("failed to write prompt to {}", prompt_path.display()));
        }

        // Create tmux pane in the session
        let pane_id = match tmux.auto_start(&session_name, &task_cwd) {
            Ok(pane_id) => pane_id,
            Err(err) => {
                abort_spawn(&project_root, &task_states, created_worktree.as_ref());
                return Err(err)
                    .with_context(|| format!("failed to create tmux pane for task {}", i + 1));
            }
        };
        eprintln!("[parallel]   Pane: {}", pane_id);

        let env_prefix = agent_doc_config::env::shell_export_prefix(&fm.env);
        let cmd_str = fanout_task_command(&env_prefix, config.model.as_deref());

        // Run the task as the pane's own process; never type it into the
        // pane's freshly spawned shell (see `fanout_task_command`).
        if let Err(err) = launch_fanout_task(&tmux, file, &pane_id, &task_cwd, &cmd_str) {
            abort_spawn(&project_root, &task_states, created_worktree.as_ref());
            return Err(err)
                .with_context(|| format!("failed to launch task {} in pane {}", i + 1, pane_id));
        }

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
            worktree: created_worktree,
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

    // Step 5b: Dispose of the worktrees (`#orchworktreeleak`).
    //
    // Results are already collected above, so scratch worktrees have nothing
    // left to give. This runs on the timeout path too: the timeout loop kills
    // panes and breaks into collection rather than returning, so a killed task's
    // worktree reaches the same decision as a finished one.
    dispose_task_worktrees(&project_root, &task_states);

    // Step 6: Format and print results as markdown
    let markdown = format_results(&results);
    println!("{}", markdown);

    eprintln!("[parallel] Done. {} tasks completed.", results.len());
    Ok(())
}

/// Remove each run-created worktree that holds nothing the task authored.
///
/// A worktree carrying real work is kept and named, so the operator can find it
/// — silently deleting a task's only copy of its output would be worse than the
/// leak this fixes.
fn dispose_task_worktrees(project_root: &Path, tasks: &[TaskState]) {
    for task in tasks {
        let Some(worktree) = task.worktree.as_ref() else {
            continue;
        };
        let residue = match crate::worktree::residue(&worktree.path, worktree.base_commit.as_deref())
        {
            Ok(residue) => residue,
            Err(err) => {
                eprintln!(
                    "[parallel]   Warning: keeping worktree {} — could not read its state: {}",
                    worktree.path.display(),
                    err
                );
                continue;
            }
        };
        match crate::worktree::classify_worktree_residue(&residue) {
            crate::worktree::WorktreeDisposition::Remove => {
                eprintln!(
                    "[parallel]   Removing worktree for task {}: {}",
                    task.index + 1,
                    worktree.path.display()
                );
                remove_worktree_reporting(project_root, worktree);
            }
            crate::worktree::WorktreeDisposition::Retain { reason } => {
                eprintln!(
                    "[parallel]   Keeping worktree for task {} ({}): {} [branch {}]",
                    task.index + 1,
                    reason,
                    worktree.path.display(),
                    worktree.branch
                );
            }
        }
    }
}

/// Unwind every worktree this run created, for an abort before any result exists.
fn abort_spawn(
    project_root: &Path,
    tasks: &[TaskState],
    in_flight: Option<&crate::worktree::WorktreeInfo>,
) {
    if let Some(worktree) = in_flight {
        remove_worktree_reporting(project_root, worktree);
    }
    remove_task_worktrees(project_root, tasks);
}

/// Unconditionally remove the worktrees recorded on `tasks`.
fn remove_task_worktrees(project_root: &Path, tasks: &[TaskState]) {
    for task in tasks {
        if let Some(worktree) = task.worktree.as_ref() {
            remove_worktree_reporting(project_root, worktree);
        }
    }
}

fn remove_worktree_reporting(project_root: &Path, worktree: &crate::worktree::WorktreeInfo) {
    if let Err(err) = crate::worktree::remove(project_root, &worktree.path, &worktree.branch) {
        eprintln!(
            "[parallel]   Warning: failed to remove worktree {}: {}",
            worktree.path.display(),
            err
        );
    }
}

/// The `/bin/sh` command a fan-out pane runs as its own process: the
/// frontmatter env exports, then `claude -p` reading the task prompt and
/// writing its result and stderr log into the task directory.
///
/// `env_prefix` stays unexpanded (`export KEY="$(passage show ...)" && `): the
/// pane's `/bin/sh` runs the substitution, so a secret value lives only in
/// that shell's and `claude`'s environment, never in the tmux argv, the
/// process list, the ops log (which records the command's hash), or the
/// scrollback. The command ends the pane when `claude` exits
/// ([`agent_doc_tmux_io::PaneLaunchExit::EndWithCommand`]), the completion
/// signal `run` polls for; the typed form needed a trailing `; exit` for that.
fn fanout_task_command(env_prefix: &str, model: Option<&str>) -> String {
    let mut cmd_parts = vec!["claude -p --output-format json".to_string()];
    if let Some(model) = model {
        cmd_parts.push(format!("--model {model}"));
    }
    cmd_parts.push(format!(
        "< {PROMPT_FILENAME} > {RESULT_FILENAME} 2>{LOG_FILENAME}"
    ));
    format!("{env_prefix}{}", cmd_parts.join(" "))
}

/// Launch `command` as `pane`'s own process in `task_cwd` (the directory its
/// redirects are relative to), logged to `file`'s ops log as
/// `transform=parallel_task_launch key=launch` with the command's length and
/// hash only.
///
/// A fan-out pane is created just before this, so its interactive shell may
/// not be reading its command line yet: an rc step that reads the terminal
/// first (zsh's `zsh-newuser-install` with no `~/.zshrc`) swallowed the first
/// typed byte of a command. Launching it as the pane process has no such race.
fn launch_fanout_task(
    tmux: &(impl agent_doc_tmux_io::TmuxCommandRunner + ?Sized),
    file: &Path,
    pane_id: &str,
    task_cwd: &Path,
    command: &str,
) -> Result<()> {
    agent_doc_tmux_io::input_diag::log_pane_launch(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "parallel.fanout",
        &format!("pane:{pane_id}"),
        command,
        Some("claude"),
        "parallel_task_launch",
    );
    agent_doc_tmux_io::launch_command_in_pane_at(
        tmux,
        pane_id,
        task_cwd,
        command,
        agent_doc_tmux_io::PaneLaunchExit::EndWithCommand,
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// `#orchworktreeleak` — the disposal RULE is unit-tested in `worktree`;
    /// these pin the WIRING, which is what was actually missing: `parallel::run`
    /// collected results and returned without ever calling it.
    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@test.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        std::fs::write(root.join("README.md"), "# test\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();
        dir
    }

    fn task_state(index: usize, worktree: crate::worktree::WorktreeInfo) -> TaskState {
        TaskState {
            index,
            description: format!("task {index}"),
            worktree_path: worktree.path.clone(),
            worktree: Some(worktree),
            pane_id: format!("%{index}"),
            completed: true,
        }
    }

    #[test]
    fn disposal_removes_a_scratch_worktree_and_keeps_one_holding_work() {
        let dir = git_repo();
        let root = dir.path();

        let scratch = crate::worktree::create(root, "abcdefgh1234", 0).unwrap();
        for name in crate::worktree::SCAFFOLDING_FILENAMES {
            std::fs::write(scratch.path.join(name), "scratch\n").unwrap();
        }
        let authored = crate::worktree::create(root, "abcdefgh1234", 1).unwrap();
        std::fs::write(authored.path.join("README.md"), "# task output\n").unwrap();

        let scratch_path = scratch.path.clone();
        let authored_path = authored.path.clone();
        let tasks = vec![task_state(0, scratch), task_state(1, authored)];

        dispose_task_worktrees(root, &tasks);

        assert!(
            !scratch_path.exists(),
            "a worktree holding only scaffolding must be removed"
        );
        assert!(
            authored_path.exists(),
            "a worktree holding the task's output must be kept"
        );
    }

    #[test]
    fn a_no_worktree_task_is_never_disposed_of() {
        let dir = git_repo();
        let root = dir.path();
        let tasks = vec![TaskState {
            index: 0,
            description: "task 0".to_string(),
            worktree_path: root.to_path_buf(),
            worktree: None,
            pane_id: "%0".to_string(),
            completed: true,
        }];

        dispose_task_worktrees(root, &tasks);

        assert!(
            root.join("README.md").exists(),
            "--no-worktree runs in the project root, which is never ours to remove"
        );
    }

    #[test]
    fn aborting_mid_spawn_unwinds_every_worktree_already_created() {
        let dir = git_repo();
        let root = dir.path();

        let already = crate::worktree::create(root, "abcdefgh1234", 0).unwrap();
        // The in-flight one holds real edits on purpose: an abort is not a
        // disposal decision, so it unwinds regardless of residue.
        let in_flight = crate::worktree::create(root, "abcdefgh1234", 1).unwrap();
        std::fs::write(in_flight.path.join("README.md"), "# half-written\n").unwrap();

        let already_path = already.path.clone();
        let in_flight_path = in_flight.path.clone();
        let tasks = vec![task_state(0, already)];

        abort_spawn(root, &tasks, Some(&in_flight));

        assert!(!already_path.exists(), "{}", already_path.display());
        assert!(!in_flight_path.exists(), "{}", in_flight_path.display());
    }

    #[test]
    fn fanout_task_command_keeps_the_secret_reference_unexpanded_and_ends_the_pane() {
        let env: agent_doc_config::env::EnvMap = [(
            "API_KEY".to_string(),
            Some("$(passage show team/api-key)".to_string()),
        )]
        .into_iter()
        .collect();

        let command = fanout_task_command(
            &agent_doc_config::env::shell_export_prefix(&env),
            Some("opus"),
        );

        assert_eq!(
            command,
            "export API_KEY=\"$(passage show team/api-key)\" && claude -p --output-format json --model opus < .agent-doc-prompt.txt > .agent-doc-result.json 2>.agent-doc-result.log"
        );
        assert!(
            !command.contains("exit"),
            "the launch ends the pane with the command; no typed `; exit` is needed"
        );
    }

    /// A fan-out pane's shell reads the terminal before its first command line
    /// (zsh with no `~/.zshrc`), and the task still runs whole as the pane's
    /// own process: the frontmatter env prefix expands `$(passage ...)` inside
    /// the pane only, `claude` gets the prompt and the secret, the pane ends
    /// when it exits (the completion signal `run` polls), and nothing was
    /// typed for the shell to swallow. Isolated tmux server with a clean
    /// `HOME` only.
    #[cfg(unix)]
    #[test]
    fn fanout_task_runs_as_the_pane_process_under_a_terminal_reading_shell() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        let project = dir.path().join("proj");
        let task_cwd = project.join("task-0");
        for path in [&home, &bin, &project.join(".agent-doc"), &task_cwd] {
            std::fs::create_dir_all(path).unwrap();
        }
        let doc = project.join("session.md");
        std::fs::write(&doc, "---\nagent_doc_session: fanout\n---\n").unwrap();
        for (name, body) in [
            (
                "passage",
                "#!/bin/sh\n[ \"$1 $2\" = 'show team/api-key' ] && printf s3cret-value\n",
            ),
            (
                "claude",
                "#!/bin/sh\nprintf '{\"result\":\"prompt=%s key=%s\"}\\n' \"$(cat)\" \"$API_KEY\"\n",
            ),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(task_cwd.join(PROMPT_FILENAME), "hello").unwrap();
        let shell = agent_doc_tmux_io::write_terminal_reading_test_shell(dir.path());
        let tmux = agent_doc_tmux_io::isolated_test_tmux_with_default_shell(
            "agent-doc-parallel-fanout-launch",
            &shell,
            &home,
        );
        let pane = tmux.auto_start("main", &task_cwd).unwrap();
        let env: agent_doc_config::env::EnvMap = [
            ("PATH".to_string(), Some(format!("{}:$PATH", bin.display()))),
            (
                "API_KEY".to_string(),
                Some("$(passage show team/api-key)".to_string()),
            ),
        ]
        .into_iter()
        .collect();
        let command = fanout_task_command(&agent_doc_config::env::shell_export_prefix(&env), None);
        assert!(!command.contains("s3cret-value"));

        launch_fanout_task(&*tmux, &doc, &pane, &task_cwd, &command).unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        while tmux.pane_alive(&pane) {
            assert!(
                Instant::now() < deadline,
                "the fan-out pane {pane} never ended with its task"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(task_cwd.join(RESULT_FILENAME))
                .ok()
                .as_deref()
                .and_then(extract_result_text)
                .as_deref(),
            Some("prompt=hello key=s3cret-value"),
            "claude ran in the task directory with the pane-expanded secret; log: {:?}",
            std::fs::read_to_string(task_cwd.join(LOG_FILENAME)).ok()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("consumed")).unwrap_or_default(),
            "",
            "nothing was typed for the pane's shell to swallow"
        );
        let logged = walk_text(&project.join(".agent-doc"));
        assert!(
            logged.contains("source=parallel.fanout")
                && logged.contains("transform=parallel_task_launch key=launch"),
            "the launch is recorded in the session's ops log: {logged}"
        );
        assert!(
            !logged.contains("s3cret-value") && !logged.contains("passage"),
            "the ops log carries the command's hash, never its text or a secret: {logged}"
        );
    }

    fn walk_text(dir: &Path) -> String {
        let mut text = String::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return text;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                text.push_str(&walk_text(&path));
            } else if let Ok(contents) = std::fs::read_to_string(&path) {
                text.push_str(&contents);
            }
        }
        text
    }
}
