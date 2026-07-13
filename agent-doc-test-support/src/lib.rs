//! Shared test-only helpers for agent-doc crates that need process-global
//! locks, temporary git documents, and fake editor IPC listeners.

use std::path::{Path, PathBuf};
use std::sync::MutexGuard;
use std::time::{Duration, Instant};

thread_local! {
    static PROCESS_GLOBAL_LOCK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// RAII guard for the process-global test lock. Reentrant within a thread: a
/// nested `env_lock()` returns a guard that holds no inner `MutexGuard`, so the
/// outer guard owns the actual lock for the whole nesting.
pub struct ProcessGlobalLockGuard {
    _guard: Option<MutexGuard<'static, ()>>,
}

impl Drop for ProcessGlobalLockGuard {
    fn drop(&mut self) {
        PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
            let current = depth.get();
            debug_assert!(current > 0, "process-global test lock depth underflow");
            depth.set(current.saturating_sub(1));
        });
    }
}

/// Acquire the process-global env lock, serializing env-mutating tests within
/// this crate's test binary. Reentrant on the same thread.
pub fn env_lock() -> ProcessGlobalLockGuard {
    let already_held = PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current > 0
    });
    if already_held {
        return ProcessGlobalLockGuard { _guard: None };
    }

    let guard = agent_doc_harness::prompt_source::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ProcessGlobalLockGuard {
        _guard: Some(guard),
    }
}

static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
static TMUX_INJECT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
static ROUTE_BIN_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_START_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn tmux_inject_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_INJECT_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn route_bin_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ROUTE_BIN_ENV_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct ScopedCurrentDir {
    prev_cwd: PathBuf,
    _env_guard: ProcessGlobalLockGuard,
}

impl ScopedCurrentDir {
    pub fn set(path: &Path) -> Self {
        let env_guard = env_lock();
        let prev_cwd = std::env::current_dir().unwrap_or_else(|_| path.to_path_buf());
        std::env::set_current_dir(path).unwrap();
        Self {
            prev_cwd,
            _env_guard: env_guard,
        }
    }
}

impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
    }
}

pub fn test_registry_entry(pane: &str, file: &str, cwd: &Path) -> tmux_router::RegistryEntry {
    tmux_router::RegistryEntry {
        pane: pane.to_string(),
        pid: 1234,
        cwd: cwd.to_string_lossy().to_string(),
        started: "2026-01-01T00:00:00Z".to_string(),
        session_id: "test-session".to_string(),
        file: file.to_string(),
        window: "@1".to_string(),
        supervisor_instance_id: String::new(),
    }
}

pub fn wait_for_pane_contains(
    iso: &tmux_router::IsolatedTmux,
    pane: &str,
    needle: &str,
    timeout: Duration,
) -> String {
    let start = Instant::now();
    let poll = Duration::from_millis(100);
    let mut last = String::new();
    while start.elapsed() < timeout {
        last = agent_doc_tmux_io::capture_pane(iso, pane).unwrap_or_default();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(poll);
    }
    last
}

pub fn pane_capture_contains_wrapped(capture: &str, needle: &str) -> bool {
    capture.contains(needle) || capture.replace(['\r', '\n'], "").contains(needle)
}

pub fn send_keys_with_retry(iso: &tmux_router::IsolatedTmux, pane: &str, text: &str) {
    let start = Instant::now();
    let timeout = Duration::from_secs(3);
    let poll = Duration::from_millis(100);
    let mut last_err = None;

    while start.elapsed() < timeout {
        match iso.send_keys(pane, text) {
            Ok(()) => return,
            Err(err) => last_err = Some(err.to_string()),
        }
        std::thread::sleep(poll);
    }

    panic!(
        "failed to send keys to pane {} after {:.1}s: {}",
        pane,
        start.elapsed().as_secs_f64(),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    );
}

pub fn pane_current_command(iso: &tmux_router::IsolatedTmux, pane: &str) -> Option<String> {
    agent_doc_tmux_io::target_current_command(iso, pane)
}

pub fn publish_editor_text_via_crdt_relay(file: &Path, editor_id: &str, content: &str) {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    mark_test_local_crdt_relay(&canonical);
    seed_reliable_sync_open(&canonical, editor_id);
    let canonical_key = canonical.to_string_lossy().to_string();
    assert!(
        agent_doc_plugin_owner::try_acquire_plugin_owner(
            &canonical_key,
            editor_id,
            std::process::id(),
        ),
        "test setup should acquire a live plugin-owner lease"
    );
    let identity = format!("{editor_id}:{canonical_key}");
    let (client_id, bootstrap) =
        agent_doc_crdt_relay_io::register_replica_for_file(&canonical, &identity)
            .expect("test editor should register through CRDT relay")
            .expect("test editor should be attached under plugin-owner authority");
    let replica = agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)
        .expect("test editor should decode relay bootstrap");
    let replica_text = replica.text();
    replica.apply_local_edit(0, replica_text.len() as u32, content);
    agent_doc_crdt_relay_io::relay_replica_update_for_file(
        &canonical,
        &identity,
        &replica.encode_state(),
    )
    .expect("test editor should publish through CRDT relay")
    .expect("test editor relay update should be accepted under plugin-owner authority");
}

fn mark_test_local_crdt_relay(file: &Path) {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
        .or_else(|| file.parent().map(Path::to_path_buf))
    else {
        return;
    };
    let agent_doc_dir = project_root.join(".agent-doc");
    let _ = std::fs::create_dir_all(&agent_doc_dir);
    let _ = std::fs::write(agent_doc_dir.join("test-local-crdt-relay"), "");
}

pub fn wait_for_shell(iso: &tmux_router::IsolatedTmux, pane: &str, timeout: Duration) -> bool {
    const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(cmd) = pane_current_command(iso, pane)
            && IDLE_SHELLS.contains(&cmd.as_str())
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Create a mock agent script: blocks for delay, then prints a prompt on its own line.
/// Uses `cat` to keep the process alive after showing the prompt.
pub fn mock_agent_script(delay_ms: u64) -> String {
    format!(
        r#"exec /bin/sh -c 'printf "Starting agent...\n"; sleep {}; printf "❯ \n"; cat'"#,
        delay_ms as f64 / 1000.0
    )
}

fn write_executable_script(base: &Path, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join(name);
    std::fs::write(&script, content).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

pub fn write_mock_registered_agent_doc(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc",
        "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  [ -z \"$CMD\" ] && continue\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
    )
}

pub fn write_mock_registered_agent_doc_with_prefix(
    base: &Path,
    name: &str,
    prefix: &str,
) -> PathBuf {
    write_executable_script(
        base,
        name,
        format!(
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf '{prefix}:%s\\n' \"$CMD\"\ndone\n",
        ),
    )
}

pub fn write_mock_registered_agent_doc_extra_line_detector(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-extra-line-detector",
        "#!/bin/bash\nprintf \"> \\n\"\nIFS= read -r CMD || exit 0\nprintf 'GOT:%s\\n' \"$CMD\"\nif IFS= read -r -t 0.5 EXTRA; then\n  printf 'EXTRA:%s\\n' \"$EXTRA\"\nfi\ncat\n",
    )
}

pub fn write_mock_busy_registered_agent_doc(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-busy",
        "#!/bin/sh\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
    )
}

pub fn write_mock_active_codex_turn_registered_agent_doc(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-active-codex-turn",
        "#!/bin/sh\nprintf 'Working...\\n'\ni=0\nwhile [ \"$i\" -lt 20 ]; do\n  printf 'Working (1m 34s - esc to interrupt)\\n'\n  i=$((i + 1))\ndone\nprintf '\\n> Write tests for @filename\\ngpt-5 high - ~/work/btakita/agent-loop - Context 41%% used\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
    )
}

pub fn write_mock_busy_registered_agent_doc_ignores_interrupt(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-busy-ignore-int",
        "#!/bin/sh\ntrap '' INT\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
    )
}

pub fn write_mock_busy_opencode_recovers_on_escape(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-busy-opencode",
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf '⬝⬝■■■■■■  esc interrupt\n'
while IFS= read -r -n1 ch; do
  stty sane
  printf '> \n'
  while IFS= read -r CMD; do
    printf 'GOT:%s\n' "$CMD"
  done
  exit 0
done
"#,
    )
}

pub fn write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-busy-recovers-on-ctrl-g",
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf 'Working...\n'
printf 'reverse-i-search: bugs enter accept · esc cancel\n'
while IFS= read -r -n1 ch; do
  if [[ "$ch" == $'\a' ]]; then
    stty sane
    printf '› \n'
    printf 'gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n'
    while IFS= read -r CMD; do
      printf 'GOT:%s\n' "$CMD"
    done
    exit 0
  fi
done
"#,
    )
}

pub fn write_mock_start_agent_doc(base: &Path) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-start",
        "#!/bin/sh\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
    )
}

pub fn write_mock_delayed_start_agent_doc(base: &Path, delay_secs: u64) -> PathBuf {
    write_executable_script(
        base,
        "agent-doc-start-delayed",
        format!(
            "#!/bin/sh\nsleep {}\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
            delay_secs
        ),
    )
}

pub fn launch_mock_registered_agent_doc(
    iso: &tmux_router::IsolatedTmux,
    pane: &str,
    script: &Path,
    file: &Path,
) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(
            iso,
            pane,
            &format!("exec {} {}", script.display(), file.display()),
        );
    }
    let launch_command = format!("exec {} {}", script.display(), file.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}

pub fn launch_mock_agent_doc_without_file_arg(
    iso: &tmux_router::IsolatedTmux,
    pane: &str,
    script: &Path,
) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(iso, pane, &format!("exec {}", script.display()));
    }
    let launch_command = format!("exec {}", script.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}

pub fn wait_for_mock_agent_prompt(
    iso: &tmux_router::IsolatedTmux,
    pane: &str,
    launch_command: &str,
) -> String {
    let start = Instant::now();
    let timeout = Duration::from_secs(20);
    let poll = Duration::from_millis(100);
    let mut last_submit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut last = String::new();

    while start.elapsed() < timeout {
        last = agent_doc_tmux_io::capture_pane(iso, pane).unwrap_or_default();
        if last.lines().any(|line| line.trim() == ">") {
            return last;
        }
        if last.contains(launch_command) && last_submit.elapsed() >= Duration::from_millis(500) {
            let _ = iso.send_keys_raw(pane, "Enter");
            last_submit = Instant::now();
        }
        std::thread::sleep(poll);
    }

    last
}

pub fn wait_for_process_pid(pattern: &str, timeout: Duration) -> u32 {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(output) = std::process::Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            && output.status.success()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let pid = line.trim();
                if pid.is_empty() {
                    continue;
                }
                if let Ok(parsed) = pid.parse::<u32>() {
                    return parsed;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for process matching pattern: {pattern}");
}

pub fn seed_live_plugin_owner_lease(file: &str) {
    let pid = std::process::id();
    seed_live_plugin_owner_lease_for_editor(file, &format!("test-editor-{pid}"));
}

pub fn seed_live_plugin_owner_lease_for_editor(file: &str, editor_id: &str) {
    seed_live_plugin_owner_lease_without_reliable_sync(file, editor_id);
    seed_reliable_sync_open(Path::new(file), editor_id);
}

pub fn seed_live_plugin_owner_lease_without_reliable_sync(file: &str, editor_id: &str) {
    mark_test_local_crdt_relay(Path::new(file));
    let pid = std::process::id();
    assert!(
        agent_doc_plugin_owner::try_acquire_plugin_owner(file, editor_id, pid),
        "test setup should acquire a live plugin-owner lease"
    );
}

/// Model the "phantom editor" state a controller/supervisor recycle leaves
/// behind: a JB editor genuinely had the document open (a live plugin-owner
/// lease + a registered CRDT replica), but after the controller recycled the
/// editor never re-registered its replica. The lease survives — so
/// `editor_attached()` still reports a live editor — while the recovered hub
/// carries the canonical text with ZERO live replicas (`live_count() == 0`,
/// delivery converged). This is the exact input to `#stale-lease-cpc-authority`.
///
/// Registers a live replica (which also acquires the plugin-owner lease and marks
/// the test-local CRDT relay), then deregisters it so the hub keeps the canonical
/// but reports zero live editors. The lease is left held.
pub fn seed_durable_open_zero_live_replica(file: &Path, editor_id: &str) {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    mark_test_local_crdt_relay(&canonical);
    seed_reliable_sync_open(&canonical, editor_id);
    let canonical_key = canonical.to_string_lossy().to_string();
    let identity = format!("{editor_id}:{canonical_key}");
    agent_doc_crdt_relay_io::register_replica_for_file(&canonical, &identity)
        .expect("test editor should register through CRDT relay")
        .expect("test editor should attach under durable reliable-sync authority");
    let removed = agent_doc_crdt_relay_io::deregister_replica_for_file(&canonical, &identity)
        .expect("test editor deregister should succeed");
    assert!(
        removed,
        "deregistering the only live replica should drop the hub mirror to zero live editors"
    );
}

fn seed_reliable_sync_open(file: &Path, tag: &str) {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .unwrap()
        .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
            document_hash,
            pid: std::process::id().into(),
            tag: tag.to_string(),
        }]);
}

pub fn patch_with_heading(heading: &str) -> agent_doc_template::PatchBlock {
    agent_doc_template::PatchBlock::new("exchange", format!("{heading}\n\nbody line one\n"))
}

pub fn init_repo_with_doc(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["init", "-q", "--initial-branch=main"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["add", name])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();
    path
}

pub fn drift_baseline() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

pub fn drift_content_ours() -> String {
    // baseline + a substantial `### Re:` response (well over the 100-byte
    // stale-drift threshold) so adopting it is a real wedge.
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Implemented the fix and verified it end to end. The response body is long\n",
        "enough to clear the stale-snapshot-reset-drift threshold so the wedge shape\n",
        "is genuinely detected by the recovery discriminator under test here.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

pub fn start_live_prompt_drift_receipt_listener(
    project_root: &Path,
    visible_write_content: String,
) -> std::thread::JoinHandle<()> {
    let root = project_root.to_path_buf();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::thread::spawn(move || {
        let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
            let v: serde_json::Value = serde_json::from_str(msg).ok()?;
            let patch_id = v
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                let _ = std::fs::write(file_path, &visible_write_content);
                let _ =
                    agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                        Path::new(file_path),
                        patch_id,
                        &visible_write_content,
                        "test_live_prompt_drift_listener",
                    );
            }
            Some(
                serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                    .to_string(),
            )
        });
    })
}

pub fn start_receipt_without_content_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
    let root = project_root.to_path_buf();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::thread::spawn(move || {
        let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
            let v: serde_json::Value = serde_json::from_str(msg).ok()?;
            let patch_id = v
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            Some(
                serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                    .to_string(),
            )
        });
    })
}

pub fn wait_for_live_prompt_drift_listener(project_root: &Path) {
    for _ in 0..100 {
        if agent_doc_ipc_io::is_listener_active(project_root) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("fake socket listener did not start within 1s");
}

pub fn compact_convergence_source() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #a\n",
        "### Re: do #a — opus-4-8\n\n",
        "A long historical response body that compaction will archive and replace\n",
        "with a summary marker so the exchange shrinks substantially past the\n",
        "stale-drift threshold and a genuine convergence patch is produced.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#a]\n",
        "- do [#b]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

pub fn compact_convergence_compacted() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "*Compacted. Content archived to `.agent-doc/archives/test.md`*\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#a]\n",
        "- do [#b]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

pub fn queue_consume_convergence_source() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

pub fn queue_consume_convergence_target() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- ~~do [#fix]~~\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}
