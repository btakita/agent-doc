//! Project-local controller shell.
//!
//! Phase A intentionally keeps this controller out of the `start`, `route`, and
//! `sync` normal paths. It establishes the singleton socket, launch lock,
//! lazy connect-or-launch helper, status protocol, and bootstrap state that
//! later phases can move session actor authority behind.

use anyhow::{Context, Result};
use fs2::FileExt;
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOCKET_FILE: &str = "controller.sock";
const STATE_FILE: &str = "controller-state.json";
const LOCK_FILE: &str = "controller-launch.lock";
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const CONNECT_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Managed,
    Lazy,
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Lazy => "lazy",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "managed" => Ok(Self::Managed),
            "lazy" => Ok(Self::Lazy),
            other => anyhow::bail!("unknown controller launch mode: {other}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerBootstrap {
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub launch_mode: LaunchMode,
    pub bootstrap_epoch: u64,
    pub pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatus {
    pub active: bool,
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub launch_mode: Option<LaunchMode>,
    pub bootstrap_epoch: Option<u64>,
    pub pid: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ControllerRequest {
    command: String,
}

pub struct LaunchLock {
    _file: File,
}

impl LaunchLock {
    pub fn acquire(project_root: &Path) -> Result<Self> {
        let path = launch_lock_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        file.try_lock_exclusive().with_context(|| {
            format!("controller launch already in progress: {}", path.display())
        })?;
        Ok(Self { _file: file })
    }
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

pub fn state_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(STATE_FILE)
}

pub fn launch_lock_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc/locks").join(LOCK_FILE)
}

pub fn read_bootstrap(project_root: &Path) -> Result<Option<ControllerBootstrap>> {
    let path = state_path(project_root);
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))
        .map(Some)
}

fn write_bootstrap(project_root: &Path, launch_mode: LaunchMode) -> Result<ControllerBootstrap> {
    let bootstrap = ControllerBootstrap {
        project_root: project_root.to_path_buf(),
        socket_path: socket_path(project_root),
        launch_mode,
        bootstrap_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        pid: std::process::id(),
    };
    let path = state_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&bootstrap)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(bootstrap)
}

fn connect(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = socket_path(project_root).to_fs_name::<GenericFilePath>()?;
    interprocess::local_socket::ConnectOptions::new()
        .name(name)
        .connect_sync()
        .context("failed to connect to project controller")
}

fn request(project_root: &Path, command: &str) -> Result<String> {
    let stream = connect(project_root)?;
    let (reader_half, mut writer_half) = stream.split();
    let mut request = serde_json::to_string(&serde_json::json!({ "command": command }))?;
    request.push('\n');
    writer_half.write_all(request.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    reader
        .read_line(&mut response)
        .context("failed to read project controller response")?;
    Ok(response.trim().to_string())
}

pub fn status(project_root: &Path) -> Result<ControllerStatus> {
    match request(project_root, "status") {
        Ok(response) => {
            let mut status: ControllerStatus = serde_json::from_str(&response)
                .context("failed to parse project controller status response")?;
            status.active = true;
            Ok(status)
        }
        Err(_) => {
            let bootstrap = read_bootstrap(project_root)?;
            Ok(ControllerStatus {
                active: false,
                project_root: project_root.to_path_buf(),
                socket_path: socket_path(project_root),
                launch_mode: bootstrap.as_ref().map(|state| state.launch_mode),
                bootstrap_epoch: bootstrap.as_ref().map(|state| state.bootstrap_epoch),
                pid: bootstrap.as_ref().map(|state| state.pid),
            })
        }
    }
}

pub fn connect_or_launch(
    project_root: &Path,
    launch_mode: LaunchMode,
) -> Result<interprocess::local_socket::Stream> {
    if let Ok(stream) = connect(project_root) {
        return Ok(stream);
    }

    let _lock = LaunchLock::acquire(project_root)?;
    if let Ok(stream) = connect(project_root) {
        return Ok(stream);
    }

    launch_detached(project_root, launch_mode)?;
    wait_for_controller(project_root)
}

fn launch_detached(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate current agent-doc binary")?;
    Command::new(exe)
        .arg("controller")
        .arg("serve")
        .arg("--project-root")
        .arg(project_root)
        .arg("--launch-mode")
        .arg(launch_mode.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch project controller")?;
    Ok(())
}

fn wait_for_controller(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    let start = Instant::now();
    loop {
        if let Ok(stream) = connect(project_root) {
            return Ok(stream);
        }
        if start.elapsed() >= CONNECT_WAIT {
            anyhow::bail!(
                "timed out waiting for project controller at {}",
                socket_path(project_root).display()
            );
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

pub fn serve(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let sock = socket_path(project_root);
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bootstrap = write_bootstrap(project_root, launch_mode)?;
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;

    loop {
        let mut should_stop = false;
        let stream = listener
            .accept()
            .context("failed to accept project controller client")?;
        let (reader_half, mut writer_half) = stream.split();
        let mut reader = BufReader::new(reader_half);
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            let response = handle_request(&line, &bootstrap, &mut should_stop)?;
            writer_half.write_all(response.as_bytes())?;
            writer_half.write_all(b"\n")?;
            writer_half.flush()?;
            line.clear();
            if should_stop {
                let _ = std::fs::remove_file(&sock);
                return Ok(());
            }
        }
    }
}

fn handle_request(
    line: &str,
    bootstrap: &ControllerBootstrap,
    should_stop: &mut bool,
) -> Result<String> {
    let request: ControllerRequest = serde_json::from_str(line.trim())?;
    match request.command.as_str() {
        "status" => Ok(serde_json::to_string(&ControllerStatus {
            active: true,
            project_root: bootstrap.project_root.clone(),
            socket_path: bootstrap.socket_path.clone(),
            launch_mode: Some(bootstrap.launch_mode),
            bootstrap_epoch: Some(bootstrap.bootstrap_epoch),
            pid: Some(bootstrap.pid),
        })?),
        "shutdown" => {
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        other => Ok(serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": format!("unknown controller command: {other}")
        }))?),
    }
}

fn project_root_from_arg(root: Option<&Path>) -> Result<PathBuf> {
    let cwd;
    let start = match root {
        Some(path) => path,
        None => {
            cwd = std::env::current_dir()?;
            &cwd
        }
    };
    crate::snapshot::find_project_root(start)
        .or_else(|| {
            if start.join(".git").exists() || start.join(".agent-doc").exists() {
                Some(start.to_path_buf())
            } else {
                None
            }
        })
        .with_context(|| format!("no project root found from {}", start.display()))
}

pub fn run_status(root: Option<&Path>, ensure: bool) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    if ensure {
        let _stream = connect_or_launch(&project_root, LaunchMode::Lazy)?;
    }
    println!("{}", serde_json::to_string_pretty(&status(&project_root)?)?);
    Ok(())
}

pub fn run_serve(root: Option<&Path>, launch_mode: &str) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    serve(&project_root, LaunchMode::parse(launch_mode)?)
}

pub fn run_shutdown(root: Option<&Path>) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    println!("{}", request(&project_root, "shutdown")?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_paths_are_project_local() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            socket_path(dir.path()),
            dir.path().join(".agent-doc/controller.sock")
        );
        assert_eq!(
            launch_lock_path(dir.path()),
            dir.path().join(".agent-doc/locks/controller-launch.lock")
        );
        assert_eq!(
            state_path(dir.path()),
            dir.path().join(".agent-doc/controller-state.json")
        );
    }

    #[test]
    fn singleton_launch_lock_rejects_concurrent_holder() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchLock::acquire(dir.path()).unwrap();
        let second = LaunchLock::acquire(dir.path());
        assert!(second.is_err());
        drop(first);
        assert!(LaunchLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn bootstrap_state_round_trips_launch_mode_and_epoch() {
        let dir = tempfile::TempDir::new().unwrap();
        let written = write_bootstrap(dir.path(), LaunchMode::Lazy).unwrap();
        let read = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(read.launch_mode, LaunchMode::Lazy);
        assert_eq!(read.bootstrap_epoch, written.bootstrap_epoch);
        assert_eq!(
            read.socket_path,
            dir.path().join(".agent-doc/controller.sock")
        );
    }
}
