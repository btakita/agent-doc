//! In-place binary pickup for `zed-lsp` (`editors/SPEC.md` section 13).
//!
//! Zed starts `agent-doc zed-lsp` once per worktree and never restarts it on
//! its own. When the agent-doc binary the server was started from is replaced
//! (`make install`, `cargo install`, a package upgrade), the server hands its
//! session over to the new image with `execve(2)`:
//!
//! - the PID, and with it the controller identity `zed-lsp:<pid>:<hash>` and
//!   the process-exit watch, are unchanged, so the controller observes no
//!   editor departure;
//! - stdin and stdout are the same pipes, so Zed keeps its LSP session;
//! - every unparsed input byte, every open document's CRDT replica (encoded
//!   state plus client id), its visible shadow, its in-flight `applyEdit`
//!   target, and the next server request id travel in a private handoff file.
//!
//! The replacement is admitted only after it proves, as a child process, that
//! it parses and validates that exact file and answers with
//! [`HANDOFF_CHECK_OK`]. An older binary without the protocol answers nothing
//! and is refused. Any refusal (or a failed `execve`) leaves the running image
//! serving unchanged and falls back to a restart prompt.

use crate::OpenDocument;
use agent_doc_merge::crdt_sync::ReplicaState;
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Schema tag of the handoff file.
pub(crate) const HANDOFF_SCHEMA: &str = "agent_doc_zed_lsp_handoff/v1";
/// Set on the replacement image: path of the handoff file to resume from.
pub(crate) const HANDOFF_ENV: &str = "AGENT_DOC_ZED_LSP_HANDOFF";
/// Set on the probe child: path of the handoff file to validate, then exit.
pub(crate) const HANDOFF_CHECK_ENV: &str = "AGENT_DOC_ZED_LSP_HANDOFF_CHECK";
/// The probe child's only stdout line on success.
pub(crate) const HANDOFF_CHECK_OK: &str = "agent_doc_zed_lsp_handoff/v1 ok";

/// How often the launch binary is examined for replacement.
pub(crate) const BINARY_CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// Upper bound on the replacement's validation run.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffDocument {
    pub uri: String,
    pub file: PathBuf,
    pub project_root: PathBuf,
    pub identity: String,
    pub client_id: u64,
    pub replica_state_b64: String,
    /// `agent_doc_hash::content_hash` of the replica text the state encodes.
    pub replica_text_hash: String,
    pub shadow: String,
    pub pending_apply_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoffState {
    pub schema: String,
    /// The handing-over process; `execve` keeps it, so a resume must match.
    pub from_pid: u32,
    /// `CARGO_PKG_VERSION` of the image that wrote the file.
    pub from_version: String,
    pub next_request_id: u64,
    pub shutdown_requested: bool,
    /// Bytes read from Zed and not yet cut into a frame.
    pub pending_input_b64: String,
    pub documents: Vec<HandoffDocument>,
}

impl HandoffDocument {
    pub(crate) fn capture(document: &OpenDocument) -> Self {
        Self {
            uri: document.uri.clone(),
            file: document.file.clone(),
            project_root: document.project_root.clone(),
            identity: document.identity.clone(),
            client_id: document.client_id,
            replica_state_b64: BASE64_STANDARD.encode(document.replica.encode_state()),
            replica_text_hash: agent_doc_hash::content_hash(&document.replica.text()),
            shadow: document.shadow.clone(),
            pending_apply_target: document.pending_apply_target.clone(),
        }
    }

    /// Rebuild the open document, proving the replica decodes to the exact
    /// text the handing-over image held.
    pub(crate) fn restore(&self) -> Result<OpenDocument> {
        let state = BASE64_STANDARD
            .decode(&self.replica_state_b64)
            .with_context(|| format!("handoff replica state for {} is not base64", self.uri))?;
        let replica = ReplicaState::from_encoded(self.client_id, &state)
            .with_context(|| format!("handoff replica state for {} does not decode", self.uri))?;
        let restored_hash = agent_doc_hash::content_hash(&replica.text());
        if restored_hash != self.replica_text_hash {
            bail!(
                "handoff replica for {} restored text {restored_hash}, expected {}",
                self.uri,
                self.replica_text_hash
            );
        }
        Ok(OpenDocument {
            uri: self.uri.clone(),
            file: self.file.clone(),
            project_root: self.project_root.clone(),
            identity: self.identity.clone(),
            client_id: self.client_id,
            replica,
            shadow: self.shadow.clone(),
            pending_apply_target: self.pending_apply_target.clone(),
        })
    }
}

/// The state a resuming image starts from.
pub(crate) struct ResumedSession {
    pub next_request_id: u64,
    pub shutdown_requested: bool,
    pub pending_input: Vec<u8>,
    pub documents: Vec<OpenDocument>,
}

impl HandoffState {
    /// Validate and rebuild. `expected_pid` is `Some` when resuming (the file
    /// must come from this very process) and `None` in the probe child.
    pub(crate) fn restore(&self, expected_pid: Option<u32>) -> Result<ResumedSession> {
        if self.schema != HANDOFF_SCHEMA {
            bail!("unsupported handoff schema {}", self.schema);
        }
        if let Some(pid) = expected_pid
            && pid != self.from_pid
        {
            bail!(
                "handoff file belongs to pid {}, not this process {pid}",
                self.from_pid
            );
        }
        let pending_input = BASE64_STANDARD
            .decode(&self.pending_input_b64)
            .context("handoff pending input is not base64")?;
        let documents = self
            .documents
            .iter()
            .map(HandoffDocument::restore)
            .collect::<Result<Vec<_>>>()?;
        Ok(ResumedSession {
            next_request_id: self.next_request_id,
            shutdown_requested: self.shutdown_requested,
            pending_input,
            documents,
        })
    }
}

pub(crate) fn read_handoff_file(path: &Path) -> Result<HandoffState> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read handoff file {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("handoff file {} is not valid JSON", path.display()))
}

/// Write the handoff file privately (`0600`, never over an existing file).
pub(crate) fn write_handoff_file(state: &HandoffState) -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!(
        "agent-doc-zed-lsp-handoff-{}-{nanos}.json",
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("failed to create handoff file {}", path.display()))?;
    let bytes = serde_json::to_vec(state)?;
    std::io::Write::write_all(&mut file, &bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("failed to write handoff file {}", path.display()))?;
    Ok(path)
}

/// The probe child's entry point: validate the file without serving.
pub(crate) fn check_handoff_file(path: &Path) -> Result<String> {
    let state = read_handoff_file(path)?;
    let session = state.restore(None)?;
    Ok(format!(
        "{HANDOFF_CHECK_OK} documents={} pending_input_bytes={}",
        session.documents.len(),
        session.pending_input.len()
    ))
}

/// Identity of an executable file: a replaced binary has a new inode, and an
/// install still in progress changes size or mtime between observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BinaryIdentity {
    dev: u64,
    ino: u64,
    len: u64,
    modified_ns: i128,
}

impl BinaryIdentity {
    pub(crate) fn of_path(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
            Some(Self {
                dev: metadata.dev(),
                ino: metadata.ino(),
                len: metadata.len(),
                modified_ns: i128::from(metadata.mtime()) * 1_000_000_000
                    + i128::from(metadata.mtime_nsec()),
            })
        }
        #[cfg(not(unix))]
        {
            let modified_ns = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_nanos() as i128)
                .unwrap_or_default();
            Some(Self {
                dev: 0,
                ino: 0,
                len: metadata.len(),
                modified_ns,
            })
        }
    }

    /// The image this process is executing. On Linux `/proc/self/exe` names
    /// it even after the launch path has been replaced or unlinked.
    fn running(launch_path: &Path) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            let _ = launch_path;
            Self::of_path(Path::new("/proc/self/exe"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::of_path(launch_path)
        }
    }
}

/// What a binary examination concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BinaryObservation {
    /// Not due yet, unchanged, or still settling.
    Quiet,
    /// The launch path holds a different, settled executable.
    Replaced(BinaryIdentity),
}

/// Watches the path this server was started from.
pub(crate) struct BinaryWatch {
    path: PathBuf,
    running: BinaryIdentity,
    /// A replacement seen once; it must be seen unchanged again to settle.
    candidate: Option<BinaryIdentity>,
    /// Replacements already refused; never retried until the path changes again.
    refused: Vec<BinaryIdentity>,
    next_check: Instant,
}

impl BinaryWatch {
    /// Watch the canonical launch path, or `None` when the running image
    /// cannot be identified with it (an unsupported host, or a launch path
    /// that was already replaced before this process looked).
    pub(crate) fn for_current_exe(now: Instant) -> Option<Self> {
        let path = std::env::current_exe().ok()?.canonicalize().ok()?;
        Self::new(path, now)
    }

    pub(crate) fn new(path: PathBuf, now: Instant) -> Option<Self> {
        let running = BinaryIdentity::running(&path)?;
        let at_path = BinaryIdentity::of_path(&path)?;
        if (running.dev, running.ino) != (at_path.dev, at_path.ino) {
            return None;
        }
        Some(Self {
            path,
            running,
            candidate: None,
            refused: Vec::new(),
            next_check: now + BINARY_CHECK_INTERVAL,
        })
    }

    /// A watch over `path` whose running image is whatever `path` holds now.
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf, now: Instant) -> Self {
        let running = BinaryIdentity::of_path(&path).expect("test launch path is executable");
        Self {
            path,
            running,
            candidate: None,
            refused: Vec::new(),
            next_check: now,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn observe(&mut self, now: Instant) -> BinaryObservation {
        if now < self.next_check {
            return BinaryObservation::Quiet;
        }
        self.next_check = now + BINARY_CHECK_INTERVAL;
        let Some(current) = BinaryIdentity::of_path(&self.path) else {
            // Mid-install the path can be briefly absent or not executable.
            self.candidate = None;
            return BinaryObservation::Quiet;
        };
        if current == self.running || self.refused.contains(&current) {
            self.candidate = None;
            return BinaryObservation::Quiet;
        }
        if self.candidate == Some(current) {
            self.candidate = None;
            return BinaryObservation::Replaced(current);
        }
        self.candidate = Some(current);
        BinaryObservation::Quiet
    }

    pub(crate) fn refuse(&mut self, identity: BinaryIdentity) {
        if !self.refused.contains(&identity) {
            self.refused.push(identity);
        }
    }
}

/// Run the replacement as a child against the exact handoff file. It must
/// exit successfully and print [`HANDOFF_CHECK_OK`] first.
pub(crate) fn probe_replacement(candidate: &Path, handoff: &Path) -> Result<()> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(candidate)
        .args(std::env::args_os().skip(1))
        .env(HANDOFF_CHECK_ENV, handoff)
        .env_remove(HANDOFF_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {}", candidate.display()))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("replacement did not validate the handoff within {PROBE_TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut stdout);
    }
    if !status.success() {
        bail!("replacement refused the handoff ({status})");
    }
    if !stdout.starts_with(HANDOFF_CHECK_OK) {
        bail!("replacement does not speak {HANDOFF_SCHEMA}");
    }
    Ok(())
}

/// Replace this process image. Returns only on failure.
#[cfg(unix)]
pub(crate) fn exec_replacement(candidate: &Path, handoff: &Path) -> anyhow::Error {
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new(candidate)
        .args(std::env::args_os().skip(1))
        .env(HANDOFF_ENV, handoff)
        .env_remove(HANDOFF_CHECK_ENV)
        .exec();
    anyhow!("execve {} failed: {error}", candidate.display())
}

#[cfg(not(unix))]
pub(crate) fn exec_replacement(_candidate: &Path, _handoff: &Path) -> anyhow::Error {
    anyhow!("in-place binary handoff needs execve, which this host does not provide")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(client_id: u64, text: &str) -> OpenDocument {
        OpenDocument {
            uri: "file:///tmp/session.md".into(),
            file: PathBuf::from("/tmp/session.md"),
            project_root: PathBuf::from("/tmp"),
            identity: format!("zed-lsp:{}:abc", std::process::id()),
            client_id,
            replica: ReplicaState::from_text(client_id, text),
            shadow: text.to_string(),
            pending_apply_target: Some(format!("{text}remote\n")),
        }
    }

    fn state(documents: Vec<HandoffDocument>) -> HandoffState {
        HandoffState {
            schema: HANDOFF_SCHEMA.into(),
            from_pid: std::process::id(),
            from_version: env!("CARGO_PKG_VERSION").into(),
            next_request_id: 7,
            shutdown_requested: false,
            pending_input_b64: BASE64_STANDARD.encode(b"Content-Length: 40\r\n\r\n{\"jso"),
            documents,
        }
    }

    #[test]
    fn handoff_round_trips_every_session_fact() {
        let original = document(41, "---\nagent_doc_session: s\n---\nbody\n");
        let written = state(vec![HandoffDocument::capture(&original)]);
        let decoded: HandoffState =
            serde_json::from_slice(&serde_json::to_vec(&written).unwrap()).unwrap();
        let session = decoded.restore(Some(std::process::id())).unwrap();

        assert_eq!(session.next_request_id, 7);
        assert_eq!(session.pending_input, b"Content-Length: 40\r\n\r\n{\"jso");
        let restored = &session.documents[0];
        assert_eq!(restored.uri, original.uri);
        assert_eq!(restored.identity, original.identity);
        assert_eq!(restored.client_id, 41);
        assert_eq!(restored.shadow, original.shadow);
        assert_eq!(restored.pending_apply_target, original.pending_apply_target);
        assert_eq!(restored.replica.text(), original.replica.text());
    }

    /// The restored replica continues the same CRDT peer: an edit made after
    /// the handoff merges into a controller replica that holds everything the
    /// old image published, with no re-registration.
    #[test]
    fn restored_replica_continues_the_same_crdt_peer() {
        let editor = document(41, "alpha\n");
        let controller = ReplicaState::new(1);
        controller
            .apply_update(&editor.replica.encode_state())
            .unwrap();
        // The old image publishes one more local edit before handing over.
        let before = editor.replica.state_vector();
        editor.replica.apply_local_edit(6, 0, "beta\n");
        controller
            .apply_update(&editor.replica.diff(&before).unwrap())
            .unwrap();

        let resumed = HandoffDocument::capture(&editor).restore().unwrap();
        let before = resumed.replica.state_vector();
        resumed.replica.apply_local_edit(11, 0, "gamma\n");
        let update = resumed.replica.diff(&before).unwrap();
        controller.apply_update(&update).unwrap();

        assert_eq!(controller.text(), "alpha\nbeta\ngamma\n");
        assert_eq!(controller.text(), resumed.replica.text());
    }

    #[test]
    fn resume_refuses_another_process_schema_or_text() {
        let original = document(41, "text\n");
        let mut foreign = state(vec![HandoffDocument::capture(&original)]);
        foreign.from_pid = foreign.from_pid.wrapping_add(1);
        assert!(foreign.restore(Some(std::process::id())).is_err());
        // The probe child has its own pid and does not apply the pid fence.
        assert!(foreign.restore(None).is_ok());

        let mut schema = state(Vec::new());
        schema.schema = "agent_doc_zed_lsp_handoff/v0".into();
        assert!(schema.restore(None).is_err());

        let mut tampered = HandoffDocument::capture(&original);
        tampered.replica_text_hash = agent_doc_hash::content_hash("other\n");
        assert!(state(vec![tampered]).restore(None).is_err());
    }

    #[test]
    fn check_mode_validates_the_written_file() {
        let written = state(vec![HandoffDocument::capture(&document(3, "x\n"))]);
        let path = write_handoff_file(&written).unwrap();
        let verdict = check_handoff_file(&path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "handoff file must be private");
        }
        std::fs::remove_file(&path).unwrap();
        let verdict = verdict.unwrap();
        assert!(verdict.starts_with(HANDOFF_CHECK_OK), "{verdict}");
        assert!(verdict.contains("documents=1"), "{verdict}");
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, bytes: &[u8]) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn watch_for(path: &Path, now: Instant) -> BinaryWatch {
        BinaryWatch::for_test(path.to_path_buf(), now)
    }

    #[cfg(unix)]
    #[test]
    fn a_replacement_is_reported_only_once_it_settles() {
        let dir = tempfile::tempdir().unwrap();
        let launch = dir.path().join("agent-doc");
        write_executable(&launch, b"old");
        let start = Instant::now();
        let mut watch = watch_for(&launch, start);
        assert_eq!(watch.observe(start), BinaryObservation::Quiet);

        let next = dir.path().join("agent-doc.next");
        write_executable(&next, b"new image");
        std::fs::rename(&next, &launch).unwrap();
        let tick = |n: u32| start + BINARY_CHECK_INTERVAL * n;
        // Not due yet, then seen once (settling), then settled.
        assert_eq!(
            watch.observe(start + Duration::from_millis(10)),
            BinaryObservation::Quiet
        );
        assert_eq!(watch.observe(tick(1)), BinaryObservation::Quiet);
        let BinaryObservation::Replaced(identity) = watch.observe(tick(2)) else {
            panic!("a settled replacement must be reported");
        };

        // A refused replacement is never retried until the path changes again.
        watch.refuse(identity);
        assert_eq!(watch.observe(tick(3)), BinaryObservation::Quiet);
        assert_eq!(watch.observe(tick(4)), BinaryObservation::Quiet);
        let newer = dir.path().join("agent-doc.newer");
        write_executable(&newer, b"newer image");
        std::fs::rename(&newer, &launch).unwrap();
        assert_eq!(watch.observe(tick(5)), BinaryObservation::Quiet);
        assert!(matches!(
            watch.observe(tick(6)),
            BinaryObservation::Replaced(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_keeps_changing_or_is_not_executable_never_settles() {
        let dir = tempfile::tempdir().unwrap();
        let launch = dir.path().join("agent-doc");
        write_executable(&launch, b"old");
        let start = Instant::now();
        let mut watch = watch_for(&launch, start);
        for n in 1..6u32 {
            // An install still writing: the size changes between observations.
            write_executable(&dir.path().join("partial"), &vec![0u8; n as usize * 10]);
            std::fs::rename(dir.path().join("partial"), &launch).unwrap();
            assert_eq!(
                watch.observe(start + BINARY_CHECK_INTERVAL * n),
                BinaryObservation::Quiet
            );
        }
        std::fs::write(&launch, b"not executable").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launch, std::fs::Permissions::from_mode(0o644)).unwrap();
        for n in 6..9u32 {
            assert_eq!(
                watch.observe(start + BINARY_CHECK_INTERVAL * n),
                BinaryObservation::Quiet
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_probe_that_does_not_speak_the_protocol_is_refused() {
        // `true` exits 0 without printing the token, as an older agent-doc
        // that predates the handoff serves EOF on a null stdin and exits.
        let true_bin = ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.exists())
            .expect("a unix host provides true(1)");
        let handoff = write_handoff_file(&state(Vec::new())).unwrap();
        let verdict = probe_replacement(true_bin, &handoff);
        std::fs::remove_file(&handoff).unwrap();
        let error = verdict.expect_err("a silent success is not a handoff acceptance");
        assert!(error.to_string().contains(HANDOFF_SCHEMA), "{error}");
    }
}
