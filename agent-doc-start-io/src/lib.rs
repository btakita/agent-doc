//! Start command runtime I/O for agent-doc.

use agent_doc_config::terminal_host::{
    ResolvedTerminalHost, ResolvedTerminalPolicy, TerminalHostReport, TerminalSessionState,
    external_host_available, resolve_terminal_policy,
};
use agent_doc_controller::status::LaunchMode;
use agent_doc_frontmatter::frontmatter;
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_session_registry_io::registration as sessions;
use agent_doc_supervisor::{
    lifecycle::start_session_retryable_during_recycle,
    session_owner::{
        ExistingPaneConflictFacts, ExistingSessionPaneAction,
        format_existing_pane_conflict_error as format_existing_pane_conflict_error_from_facts,
    },
};
use agent_doc_supervisor_process_io::{SupervisorLaunchLog, SupervisorStderrRedirect};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// Optional tmux socket override used by headless integrations and isolated tests.
pub const AGENT_DOC_TMUX_SOCKET_ENV: &str = "AGENT_DOC_TMUX_SOCKET";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmuxEnsureOutcome {
    pub session_name: String,
    pub pane_id: String,
    pub attach_command: String,
    pub created: bool,
    pub attached: bool,
    pub resolution: String,
    pub document_pane: Option<String>,
    pub terminal_host: ResolvedTerminalHost,
    pub terminal_host_reason: String,
    pub auto_start_tmux: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LiveRegistryTarget {
    session_name: String,
    pane_id: String,
    document_match: bool,
}

/// Ensure the project has one tmux session without requiring a TTY.
///
/// A live project registry entry wins over every cold-start name. Otherwise the
/// explicit name, project binding, and `0` fallback are consulted in that order.
pub fn ensure_tmux_session(
    file: &Path,
    explicit_session: Option<&str>,
) -> Result<TmuxEnsureOutcome> {
    let tmux = tmux_for_environment();
    ensure_tmux_session_with_ide(&tmux, file, explicit_session, false)
}

/// IDE-originated ensure. The boolean is a capability observation, not a host
/// override: frontmatter and project/global config still own host selection.
pub fn ensure_tmux_session_for_ide(
    file: &Path,
    explicit_session: Option<&str>,
) -> Result<TmuxEnsureOutcome> {
    let tmux = tmux_for_environment();
    ensure_tmux_session_with_ide(&tmux, file, explicit_session, true)
}

pub fn ensure_tmux_session_with(
    tmux: &tmux_router::Tmux,
    file: &Path,
    explicit_session: Option<&str>,
) -> Result<TmuxEnsureOutcome> {
    ensure_tmux_session_with_ide(tmux, file, explicit_session, false)
}

fn ensure_tmux_session_with_ide(
    tmux: &tmux_router::Tmux,
    file: &Path,
    explicit_session: Option<&str>,
    ide_available: bool,
) -> Result<TmuxEnsureOutcome> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let report = terminal_host_report(tmux);
    if !report.classification.tmux_installed {
        anyhow::bail!("{}", report.reason);
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| agent_doc_project_config_io::project_root_for_doc(&canonical))
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    let reconciled_server_before_lookup = if tmux.running() {
        reconcile_tmux_server_identity(tmux, &project_root)?;
        true
    } else {
        false
    };

    if let Some(target) = live_registry_target(tmux, &project_root, &canonical)? {
        let state = if is_session_attached(tmux, &target.session_name) {
            TerminalSessionState::Attached
        } else {
            TerminalSessionState::Detached
        };
        let policy = terminal_policy_for_file(&canonical, &report, ide_available, state)?;
        if let Some(failure) = policy.failure.as_deref() {
            anyhow::bail!("{failure}");
        }
        return Ok(tmux_ensure_outcome(
            tmux,
            target.session_name,
            target.pane_id.clone(),
            false,
            "live_registry",
            target.document_match.then_some(target.pane_id),
            &policy,
        ));
    }

    let configured = agent_doc_project_config_io::load_project_for_doc(&canonical).tmux_session;
    let (session_name, resolution) =
        resolve_tmux_session_name(explicit_session, configured.as_deref())?;
    let session_exists = tmux.session_exists(&session_name);
    let state = if session_exists {
        if is_session_attached(tmux, &session_name) {
            TerminalSessionState::Attached
        } else {
            TerminalSessionState::Detached
        }
    } else {
        TerminalSessionState::Missing
    };
    let policy = terminal_policy_for_file(&canonical, &report, ide_available, state)?;
    if let Some(failure) = policy.failure.as_deref() {
        anyhow::bail!("{failure}");
    }
    if !session_exists && !policy.auto_start_tmux {
        anyhow::bail!(
            "tmux session '{}' is absent and terminal auto-start is disabled; start it manually, then retry",
            session_name
        );
    }
    let mut created = false;
    let pane_id = if session_exists {
        first_session_pane(tmux, &session_name)?
    } else {
        match tmux.new_session(&session_name, &project_root) {
            Ok(pane_id) => {
                created = true;
                pane_id
            }
            Err(_error) if tmux.session_exists(&session_name) => {
                // Another concurrent ensure won the create race.
                first_session_pane(tmux, &session_name)?
            }
            Err(error) => return Err(error),
        }
    };
    if !reconciled_server_before_lookup {
        reconcile_tmux_server_identity(tmux, &project_root)?;
    }

    Ok(tmux_ensure_outcome(
        tmux,
        session_name,
        pane_id,
        created,
        resolution,
        None,
        &policy,
    ))
}

fn reconcile_tmux_server_identity(tmux: &tmux_router::Tmux, project_root: &Path) -> Result<()> {
    let outcome = agent_doc_session_registry_io::tmux_server::reconcile_tmux_server_identity_in(
        project_root,
        tmux,
    )?;
    if outcome.server_replaced {
        let stale_editor_sockets_removed =
            agent_doc_ipc_io::prune_stale_editor_sockets(project_root)?;
        eprintln!(
            "[tmux-server] replacement detected; removed {} stale registry row(s) and {} stale editor socket(s)",
            outcome.stale_rows_removed, stale_editor_sockets_removed
        );
    }
    Ok(())
}

/// From a non-tmux `agent-doc start`, create/reuse the project session and
/// submit the same start request inside a pane. `None` means no bootstrap was
/// needed because this process is already inside tmux.
pub fn bootstrap_start_inside_tmux_if_needed(
    file: &Path,
    force: bool,
    route_owned: bool,
    route_owned_reap_policy: agent_doc_supervisor::route_owned::RouteOwnedReapPolicy,
    resume: Option<&agent_doc_harness::ResumeRequest>,
) -> Result<Option<TmuxEnsureOutcome>> {
    if agent_doc_tmux_io::in_tmux() {
        return Ok(None);
    }

    prepare_start_document_for_tmux_bootstrap(file)?;
    let outcome = ensure_tmux_session(file, None)?;
    if outcome.document_pane.is_some() && !force {
        eprintln!(
            "[start] {} already has a live supervisor in pane {}; session '{}' reused",
            file.display(),
            outcome.document_pane.as_deref().unwrap_or(&outcome.pane_id),
            outcome.session_name,
        );
        return Ok(Some(outcome));
    }

    let tmux = tmux_for_environment();
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| agent_doc_project_config_io::project_root_for_doc(&canonical))
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let pane_id = if outcome.created {
        outcome.pane_id.clone()
    } else {
        tmux.new_window(&outcome.session_name, &project_root)
            .with_context(|| {
                format!(
                    "failed to provision a start pane in tmux session {}",
                    outcome.session_name
                )
            })?
    };
    let command = start_reexec_command(file, force, route_owned, route_owned_reap_policy, resume)?;
    tmux.send_keys(&pane_id, &command)
        .with_context(|| format!("failed to re-exec agent-doc start in pane {pane_id}"))?;
    eprintln!(
        "[start] dispatched into tmux session '{}' pane {}; attach with: {}",
        outcome.session_name, pane_id, outcome.attach_command,
    );
    Ok(Some(outcome))
}

/// Validate document admission and persist a missing session identity before an
/// outside-tmux caller hands the full start lifecycle to a tmux pane.
///
/// The inner start admission repeats these reads through the authoritative
/// model. This narrow outer pass exists so invalid documents fail synchronously
/// and generated identities are durable before the dispatching process exits.
fn prepare_start_document_for_tmux_bootstrap(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let _ = agent_doc_run_io::repair_document_frontmatter_on_disk(file);
    let start_document = resolve_start_admission_document(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let content = start_document.content;
    agent_doc_frontmatter_io::session::require_agent_doc_document(&content, file)?;
    let (updated_content, _) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&content, file)?;
    if updated_content == content {
        return Ok(());
    }
    if start_document
        .authority
        .needs_post_start_document_model_ensure()
    {
        anyhow::bail!(
            "cannot change the session UUID for {} while editor authority is attached but the live document model is unavailable; save or reload the editor buffer, then retry start",
            file.display()
        );
    }
    agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated_content)
        .with_context(|| format!("failed to write {}", file.display()))
}

fn tmux_for_environment() -> tmux_router::Tmux {
    tmux_router::Tmux {
        server_socket: std::env::var(AGENT_DOC_TMUX_SOCKET_ENV)
            .ok()
            .filter(|socket| !socket.trim().is_empty()),
    }
}

fn terminal_host_report(tmux: &tmux_router::Tmux) -> TerminalHostReport {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let tmux_installed = tmux
        .cmd()
        .arg("-V")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    agent_doc_config::terminal_host::classify(
        &env,
        tmux_installed,
        tmux_installed && tmux.running(),
    )
}

fn terminal_policy_for_file(
    file: &Path,
    report: &TerminalHostReport,
    ide_available: bool,
    session_state: TerminalSessionState,
) -> Result<ResolvedTerminalPolicy> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_with_source(
        file,
        "start-terminal-policy",
    )
    .with_context(|| format!("failed to resolve terminal policy from {}", file.display()))?
    .into_content();
    let (document, _) = frontmatter::parse(&content)
        .with_context(|| format!("failed to parse terminal policy from {}", file.display()))?;
    let project = agent_doc_project_config_io::load_project_for_doc(file);
    let global = agent_doc_config::load()?;
    let command_available = project
        .terminal
        .as_ref()
        .and_then(|config| config.command.as_deref())
        .is_some_and(|command| !command.trim().is_empty())
        || global
            .terminal
            .as_ref()
            .and_then(|config| config.command.as_deref())
            .is_some_and(|command| !command.trim().is_empty())
        || std::env::var("TERMINAL").is_ok_and(|terminal| !terminal.trim().is_empty());
    let external_available = external_host_available(report, command_available);

    Ok(resolve_terminal_policy(
        document.terminal_host,
        project.terminal.as_ref(),
        global.terminal.as_ref(),
        report.resolved_terminal_host,
        ide_available,
        external_available,
        session_state,
    ))
}

fn live_registry_target(
    tmux: &tmux_router::Tmux,
    project_root: &Path,
    canonical_file: &Path,
) -> Result<Option<LiveRegistryTarget>> {
    let registry = agent_doc_session_registry_io::load_in(project_root)?;
    let mut targets = registry
        .values()
        .filter(|entry| tmux.pane_alive(&entry.pane))
        .filter_map(|entry| {
            let session_name = tmux
                .pane_session(&entry.pane)
                .ok()
                .filter(|name| !name.is_empty())?;
            let entry_path = Path::new(&entry.file);
            let entry_path = if entry_path.is_absolute() {
                entry_path.to_path_buf()
            } else {
                project_root.join(entry_path)
            };
            let entry_path = std::fs::canonicalize(&entry_path).unwrap_or(entry_path);
            Some(LiveRegistryTarget {
                session_name,
                pane_id: entry.pane.clone(),
                document_match: !entry.file.is_empty() && entry_path == canonical_file,
            })
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| {
        (
            !target.document_match,
            target.session_name.clone(),
            target.pane_id.clone(),
        )
    });
    Ok(targets.into_iter().next())
}

fn resolve_tmux_session_name(
    explicit: Option<&str>,
    configured: Option<&str>,
) -> Result<(String, &'static str)> {
    let (name, resolution) = if let Some(explicit) = explicit {
        (explicit, "explicit")
    } else if let Some(configured) = configured {
        (configured, "project_config")
    } else {
        ("0", "default")
    };
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("tmux session name must not be empty");
    }
    Ok((name.to_string(), resolution))
}

fn first_session_pane(tmux: &tmux_router::Tmux, session_name: &str) -> Result<String> {
    tmux.list_session_panes(session_name)
        .into_iter()
        .next()
        .with_context(|| format!("tmux session {session_name} exists but has no live pane"))
}

fn tmux_ensure_outcome(
    tmux: &tmux_router::Tmux,
    session_name: String,
    pane_id: String,
    created: bool,
    resolution: &str,
    document_pane: Option<String>,
    policy: &ResolvedTerminalPolicy,
) -> TmuxEnsureOutcome {
    let tmux_prefix = tmux.server_socket.as_deref().map_or_else(
        || "tmux".to_string(),
        |socket| format!("tmux -L {} -f /dev/null", shell_escape(socket)),
    );
    let attached = is_session_attached(tmux, &session_name);
    let default_attach_command = format!(
        "{tmux_prefix} attach-session -t {}",
        shell_escape(&session_name)
    );
    let attach_command = policy.attach_command.as_deref().map_or_else(
        || default_attach_command.clone(),
        |template| {
            template
                .replace("{session}", &shell_escape(&session_name))
                .replace("{tmux_command}", &default_attach_command)
        },
    );
    TmuxEnsureOutcome {
        attach_command,
        session_name,
        pane_id,
        created,
        attached,
        resolution: resolution.to_string(),
        document_pane,
        terminal_host: policy.host,
        terminal_host_reason: policy.reason.clone(),
        auto_start_tmux: policy.auto_start_tmux,
    }
}

fn is_session_attached(tmux: &tmux_router::Tmux, session_name: &str) -> bool {
    tmux.cmd()
        .args(["list-clients", "-t", session_name, "-F", "#{client_name}"])
        .output()
        .is_ok_and(|output| {
            output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        })
}

fn start_reexec_command(
    file: &Path,
    force: bool,
    route_owned: bool,
    route_owned_reap_policy: agent_doc_supervisor::route_owned::RouteOwnedReapPolicy,
    resume: Option<&agent_doc_harness::ResumeRequest>,
) -> Result<String> {
    let executable =
        std::env::current_exe().context("failed to resolve the agent-doc executable")?;
    let mut args = vec![
        shell_escape_os(executable.as_os_str()),
        "start".to_string(),
        shell_escape_os(file.as_os_str()),
    ];
    if force {
        args.push("--force".to_string());
    }
    match resume {
        None => args.push("--fresh".to_string()),
        Some(agent_doc_harness::ResumeRequest::Latest) => {}
        Some(agent_doc_harness::ResumeRequest::Id(id)) => {
            args.push("--resume".to_string());
            args.push(shell_escape(id));
        }
    }
    if route_owned {
        args.push("--route-owned".to_string());
        args.push("--route-owned-reap-policy".to_string());
        args.push(route_owned_reap_policy.to_string());
    }
    Ok(args.join(" "))
}

fn shell_escape_os(value: &OsStr) -> String {
    shell_escape(&value.to_string_lossy())
}

fn shell_escape(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'/' | b'_' | b'-' | b'.' | b':' | b'@' | b'%' | b'+' | b'='
                )
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Char-boundary-safe 8-char session prefix for logs and status lines.
///
/// A document may carry a legacy, migrated, or hand-edited `session:`
/// frontmatter value that is shorter than 8 bytes (including empty) or whose
/// 8th byte falls inside a multibyte UTF-8 character. A bare `&session_id[..8]`
/// panics on either — and because this runs inside the pane-spawned
/// `agent-doc start --route-owned` CLI (no FFI `catch_unwind`, no panic hook),
/// that panic surfaces as a raw backtrace in the freshly created pane: the
/// "agent-doc start crash". `str::get` returns `None` instead of panicking.
fn session_id_short(session_id: &str) -> &str {
    session_id.get(..8).unwrap_or(session_id)
}

pub struct StartRuntime {
    pub session_id: String,
    pub fm: frontmatter::Frontmatter,
    pub global_config: agent_doc_config::Config,
    pub canonical: PathBuf,
    pub project_root: PathBuf,
    pub session_log: Option<std::fs::File>,
    pub stderr_redirect: SupervisorStderrRedirect,
    pub harness: agent_doc_harness::HarnessConfig,
    pub pane_id: String,
    pub supervisor_instance_id: String,
    pub actor_record: agent_doc_controller::actor::ActorRecord,
    pub post_start_document_model_ensure: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartRuntimeAdmission {
    NewSession,
    SupervisorReexecPreservingChild,
}

impl StartRuntimeAdmission {
    fn preserves_session_lifecycle(self) -> bool {
        matches!(self, Self::SupervisorReexecPreservingChild)
    }
}

fn validate_supervisor_reentry_actor(
    canonical: &Path,
    session_id: &str,
    pane_id: &str,
    record: agent_doc_controller::actor::ActorRecord,
) -> Result<agent_doc_controller::actor::ActorRecord> {
    if record.session_id != session_id
        || record.pane_id != pane_id
        || record.state == agent_doc_controller::actor::ActorState::Closed
    {
        anyhow::bail!(
            "cannot reenter supervisor for {}: inherited session={} pane={}, authoritative session={} pane={} generation={} state={}",
            canonical.display(),
            session_id,
            pane_id,
            record.session_id,
            record.pane_id,
            record.generation,
            record.state.as_str()
        );
    }
    Ok(record)
}

#[derive(Debug)]
struct SessionIdentityRekey {
    previous_session_id: String,
    owner: agent_doc_session_registry_io::SessionIdentityOwner,
}

#[derive(Debug)]
struct ResolvedStartSessionIdentity {
    content: String,
    session_id: String,
    rekey: Option<SessionIdentityRekey>,
}

fn publish_session_identity_observation_with(
    project_root: &Path,
    file: &Path,
    session_id: &str,
    publish: &mut impl FnMut(&Path, &agent_doc_state_backbone::StateEvent) -> Result<bool>,
) -> Result<()> {
    let resolved_file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        project_root.join(file)
    };
    let canonical = std::fs::canonicalize(&resolved_file).unwrap_or(resolved_file);
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "document-session-identity:{}:{}",
            document_hash,
            agent_doc_hash::content_hash(session_id)
        ),
        agent_doc_state_backbone::StateFact::DocumentSessionIdentityObserved {
            document_hash,
            canonical_path: canonical.display().to_string(),
            session_id: session_id.to_string(),
        },
    );
    publish(project_root, &event)?;
    Ok(())
}

fn resolve_start_session_identity_with_publisher(
    project_root: &Path,
    file: &Path,
    content: String,
    session_id: String,
    mut publish: impl FnMut(&Path, &agent_doc_state_backbone::StateEvent) -> Result<bool>,
) -> Result<ResolvedStartSessionIdentity> {
    if agent_doc_session_registry_io::durable_session_identity_claim_in(
        project_root,
        &session_id,
        file,
    )?
    .is_none()
    {
        let compatibility_claim = agent_doc_session_registry_io::session_identity_claim_in(
            project_root,
            &session_id,
            file,
        )?;
        let compatibility_owner = match compatibility_claim {
            agent_doc_session_registry_io::SessionIdentityClaim::OwnedByDocument(owner)
            | agent_doc_session_registry_io::SessionIdentityClaim::Conflicting(owner) => {
                Some(owner)
            }
            agent_doc_session_registry_io::SessionIdentityClaim::Unclaimed => None,
        };
        if let Some(owner) = compatibility_owner
            && !owner.file.is_empty()
        {
            publish_session_identity_observation_with(
                project_root,
                Path::new(&owner.file),
                &session_id,
                &mut publish,
            )?;
        }
    }
    publish_session_identity_observation_with(project_root, file, &session_id, &mut publish)?;
    let Some(claim) = agent_doc_session_registry_io::durable_session_identity_claim_in(
        project_root,
        &session_id,
        file,
    )?
    else {
        anyhow::bail!(
            "session identity observation for {} was published but is absent from the durable projection",
            file.display()
        );
    };
    let agent_doc_session_registry_io::SessionIdentityClaim::Conflicting(owner) = claim else {
        return Ok(ResolvedStartSessionIdentity {
            content,
            session_id,
            rekey: None,
        });
    };

    let previous_session_id = session_id;
    let session_id = uuid::Uuid::new_v4().to_string();
    let content = frontmatter::set_session_id(&content, &session_id)?;
    publish_session_identity_observation_with(project_root, file, &session_id, &mut publish)?;
    Ok(ResolvedStartSessionIdentity {
        content,
        session_id,
        rekey: Some(SessionIdentityRekey {
            previous_session_id,
            owner,
        }),
    })
}

fn resolve_start_session_identity(
    project_root: &Path,
    file: &Path,
    content: String,
    session_id: String,
) -> Result<ResolvedStartSessionIdentity> {
    resolve_start_session_identity_with_publisher(
        project_root,
        file,
        content,
        session_id,
        agent_doc_controller_io::project_controller::publish_state_event,
    )
}

pub fn log_event(log: &mut Option<std::fs::File>, msg: &str) {
    if let Some(f) = log {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

pub fn start_console_status(
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
    message: impl AsRef<str>,
) {
    let message = message.as_ref();
    let printed = !route_owned || agent_doc_tmux_commands::input_diag::verbose_enabled();
    log_event(
        session_log,
        &format!(
            "start_console_status route_owned={} printed={} message={:?}",
            route_owned, printed, message
        ),
    );
    if printed {
        eprintln!("{message}");
    }
}

pub fn open_session_log(file: &Path, session_id: &str) -> Option<std::fs::File> {
    let path = agent_doc_supervisor_io::startup_miss::supervisor_session_log_path(file, session_id)
        .ok()
        .flatten()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    agent_doc_log_time::format_log_timestamp(now)
}

fn current_pane_id_from_env() -> Option<String> {
    std::env::var("TMUX_PANE")
        .ok()
        .filter(|pane| !pane.trim().is_empty())
}

fn disk_document_allows_pre_admission_pane_guard(file: &Path) -> bool {
    agent_doc_frontmatter_io::session::read_session_id(file).is_some()
}

struct StartAdmissionLaunchLog<'a> {
    session_log: &'a mut Option<std::fs::File>,
    route_owned: bool,
}

impl SupervisorLaunchLog for StartAdmissionLaunchLog<'_> {
    fn log_event(&mut self, msg: &str) {
        log_event(self.session_log, msg);
    }

    fn start_console_status(&mut self, message: &str) {
        start_console_status(self.session_log, self.route_owned, message);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartAdmissionReadAuthority {
    CurrentDocument,
    DiskMetadataBootstrapEditorModelUnavailable,
}

impl StartAdmissionReadAuthority {
    fn needs_post_start_document_model_ensure(self) -> bool {
        matches!(
            self,
            StartAdmissionReadAuthority::DiskMetadataBootstrapEditorModelUnavailable
        )
    }
}

struct StartAdmissionDocument {
    content: String,
    authority: StartAdmissionReadAuthority,
}

fn start_admission_fallback_for_current_text(
    current: &agent_doc_crdt_relay_io::CurrentText,
) -> Option<StartAdmissionReadAuthority> {
    match current {
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            Some(StartAdmissionReadAuthority::DiskMetadataBootstrapEditorModelUnavailable)
        }
        agent_doc_crdt_relay_io::CurrentText::Detached
        | agent_doc_crdt_relay_io::CurrentText::Current { .. } => None,
    }
}

fn current_text_label(current: &agent_doc_crdt_relay_io::CurrentText) -> &'static str {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Detached => "detached",
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            "editor_attached_model_missing"
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => "editor_sync_pending",
        agent_doc_crdt_relay_io::CurrentText::Current { .. } => "current",
    }
}

fn start_admission_current_text_for_fallback(
    file: &Path,
) -> Option<agent_doc_crdt_relay_io::CurrentText> {
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
        file,
        "prepare_start_runtime",
    ) {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) => {
            start_admission_local_current_text_for_fallback(file)
                .or(Some(agent_doc_crdt_relay_io::CurrentText::Detached))
        }
        Ok(Some(current)) => Some(current),
        Ok(None) => start_admission_local_current_text_for_fallback(file)
            .or(Some(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)),
        Err(controller_err) => match agent_doc_crdt_relay_io::current_text_for_file(file) {
            Ok(current) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "start_admission_current_text_controller_unavailable file={} fallback=local_relay current_state={} controller_error={}",
                        file.display(),
                        current_text_label(&current),
                        format!("{controller_err:#}").replace('\n', "\\n"),
                    ),
                );
                Some(current)
            }
            Err(local_err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "start_admission_current_text_unavailable file={} controller_error={} local_relay_error={}",
                        file.display(),
                        format!("{controller_err:#}").replace('\n', "\\n"),
                        format!("{local_err:#}").replace('\n', "\\n"),
                    ),
                );
                None
            }
        }
    }
}

fn start_admission_local_current_text_for_fallback(
    file: &Path,
) -> Option<agent_doc_crdt_relay_io::CurrentText> {
    match agent_doc_crdt_relay_io::current_text_for_file(file) {
        Ok(current) if start_admission_fallback_for_current_text(&current).is_some() => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "start_admission_current_text_local_relay file={} current_state={}",
                    file.display(),
                    current_text_label(&current),
                ),
            );
            return Some(current);
        }
        Ok(_) => {}
        Err(local_err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "start_admission_current_text_local_relay_error file={} error={}",
                    file.display(),
                    format!("{local_err:#}").replace('\n', "\\n"),
                ),
            );
        }
    }
    None
}

fn resolve_start_admission_disk_metadata_bootstrap(
    file: &Path,
    current: agent_doc_crdt_relay_io::CurrentText,
    original_error: &str,
) -> Result<StartAdmissionDocument> {
    let Some(authority) = start_admission_fallback_for_current_text(&current) else {
        anyhow::bail!(
            "start admission disk metadata bootstrap requires missing editor model state"
        );
    };
    let content = agent_doc_document_realtime_io::resolve_disk_current_document_content(
        file,
        "prepare_start_runtime_metadata_bootstrap",
    )
    .with_context(|| {
        format!(
            "prepare_start_runtime: failed to read disk metadata bootstrap {}",
            file.display()
        )
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "start_admission_disk_metadata_bootstrap file={} current_state={} original_error={}",
            file.display(),
            current_text_label(&current),
            original_error.replace('\n', "\\n")
        ),
    );
    Ok(StartAdmissionDocument { content, authority })
}

fn resolve_start_admission_document(file: &Path) -> Result<StartAdmissionDocument> {
    match agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "prepare_start_runtime",
    ) {
        Ok(content) => Ok(StartAdmissionDocument {
            content,
            authority: StartAdmissionReadAuthority::CurrentDocument,
        }),
        Err(resolve_err) => {
            let Some(current) = start_admission_current_text_for_fallback(file) else {
                return Err(resolve_err);
            };
            if start_admission_fallback_for_current_text(&current).is_none() {
                return Err(resolve_err);
            }
            let original_error = format!("{resolve_err:#}");
            resolve_start_admission_disk_metadata_bootstrap(file, current, &original_error)
        }
    }
}

pub fn prepare_start_runtime(file: &Path, force: bool, route_owned: bool) -> Result<StartRuntime> {
    prepare_start_runtime_with_admission(
        file,
        force,
        route_owned,
        StartRuntimeAdmission::NewSession,
    )
}

/// Prepare a replacement supervisor that will adopt a child preserved across
/// `execve`.
///
/// This is a transport reentry, not a new session lifecycle. The existing
/// controller actor must still own the same document/session/pane, and its
/// generation and runtime state are retained.
pub fn prepare_start_runtime_reentry(
    file: &Path,
    force: bool,
    route_owned: bool,
) -> Result<StartRuntime> {
    prepare_start_runtime_with_admission(
        file,
        force,
        route_owned,
        StartRuntimeAdmission::SupervisorReexecPreservingChild,
    )
}

fn prepare_start_runtime_with_admission(
    file: &Path,
    force: bool,
    route_owned: bool,
    admission: StartRuntimeAdmission,
) -> Result<StartRuntime> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let admission_tmux = tmux_router::Tmux::default_server();
    if disk_document_allows_pre_admission_pane_guard(file)
        && let Some(pane_id) = current_pane_id_from_env()
        && let Some(other) = agent_doc_sync_io::sync::pane_owned_document_other_than(
            &admission_tmux,
            &pane_id,
            &canonical,
        )
    {
        let message = format!(
            "start_cross_document_owner_pane_refused file={} pane={} pane_owns={} phase=pre_admission",
            file.display(),
            pane_id,
            other
        );
        agent_doc_ops_log_io::log_op(file, &message);
        anyhow::bail!(
            "current tmux pane {} already owns another agent-doc document: {}. Run `agent-doc start {}` from a different shell/tmux pane, or use the editor sync/route action to provision a separate owner.",
            pane_id,
            other,
            file.display()
        );
    }

    let _ = agent_doc_run_io::repair_document_frontmatter_on_disk(file);
    let start_document = resolve_start_admission_document(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let post_start_document_model_ensure = start_document
        .authority
        .needs_post_start_document_model_ensure();
    let content = start_document.content;
    agent_doc_frontmatter_io::session::require_agent_doc_document(&content, file)?;
    let (updated_content, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&content, file)?;
    let assigned_missing_session_uuid = updated_content != content;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .ok()
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf())
        });
    let resolved_identity =
        resolve_start_session_identity(&project_root, file, updated_content, session_id)?;
    let updated_content = resolved_identity.content;
    let session_id = resolved_identity.session_id;
    let rekeyed_session_identity = resolved_identity.rekey;
    let document_identity_changed =
        assigned_missing_session_uuid || rekeyed_session_identity.is_some();
    if document_identity_changed && post_start_document_model_ensure {
        anyhow::bail!(
            "cannot change the session UUID for {} while editor authority is attached but the live document model is unavailable; save or reload the editor buffer, then retry start",
            file.display()
        );
    }
    if updated_content != content {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
    }

    // Lazily/CP is the durable current-document authority. Retire the legacy
    // append-only queue recovery journal instead of replaying it over that
    // frontier; replay could not distinguish a crash-lost add from an
    // operator-authored deletion and was the direct resurrection source.
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let (fm, _body) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &updated_content,
        file,
        &rc.ssh_context(),
    )?;
    let global_config = agent_doc_config::load().unwrap_or_default();
    let mut session_log = open_session_log(&canonical, &session_id);
    if assigned_missing_session_uuid {
        start_console_status(
            &mut session_log,
            route_owned,
            format!("Generated session UUID: {session_id}"),
        );
    }
    if let Some(rekey) = rekeyed_session_identity {
        let message = format!(
            "session_identity_rekeyed file={} previous_session_id={} new_session_id={} owner_file={} owner_pane={} owner_registered={}",
            file.display(),
            rekey.previous_session_id,
            session_id,
            rekey.owner.file,
            rekey.owner.pane,
            rekey.owner.started,
        );
        log_event(&mut session_log, &message);
        agent_doc_ops_log_io::log_op(file, &message);
        start_console_status(
            &mut session_log,
            route_owned,
            format!(
                "[start] copied session identity belonged to {}; assigned a fresh identity",
                rekey.owner.file
            ),
        );
    }

    if !admission.preserves_session_lifecycle() {
        close_stale_start_actors(&project_root, &mut session_log, route_owned);
    }

    let harness = agent_doc_harness::HarnessConfig::from_context(&fm, &global_config);
    let stderr_redirect = {
        let mut launch_log = StartAdmissionLaunchLog {
            session_log: &mut session_log,
            route_owned,
        };
        SupervisorStderrRedirect::maybe_start(&project_root, &harness, route_owned, &mut launch_log)
    };
    report_harness_resolution(&fm, &global_config, &harness, &mut session_log, route_owned);

    ensure_inside_tmux(file)?;
    let tmux = tmux_router::Tmux::default_server();
    let pane_id = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux)
        .context("failed to query current tmux pane")?;

    if let Some(diagnostic) = agent_doc_run_io::recursive_codex_start_invocation_diagnostic(
        file,
        &session_id,
        &harness.binary,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "start_recursive_self_owned_pane_refused file={} pane={} session_id={}",
                file.display(),
                pane_id,
                session_id
            ),
        );
        anyhow::bail!("{}", diagnostic);
    }

    if let Some(other) =
        agent_doc_sync_io::sync::pane_owned_document_other_than(&tmux, &pane_id, &canonical)
    {
        let message = format!(
            "start_cross_document_owner_pane_refused file={} pane={} pane_owns={} session_id={}",
            file.display(),
            pane_id,
            other,
            session_id
        );
        log_event(&mut session_log, &message);
        agent_doc_ops_log_io::log_op(file, &message);
        anyhow::bail!(
            "current tmux pane {} already owns another agent-doc document: {}. Run `agent-doc start {}` from a different shell/tmux pane, or use the editor sync/route action to provision a separate owner.",
            pane_id,
            other,
            file.display()
        );
    }

    clear_superseded_startup_miss(file, &mut session_log, route_owned)?;
    let unresolved_startup_miss = agent_doc_supervisor_io::startup_miss::load_startup_miss(file)
        .ok()
        .flatten();

    if !force {
        if let Some(action) = existing_session_pane_action(&tmux, &session_id, file, &pane_id)? {
            match action {
                ExistingSessionPaneAction::Refuse(conflicting_pane) => {
                    if let Some(miss) = unresolved_startup_miss.as_ref()
                        && miss.pane_id == conflicting_pane
                    {
                        let miss_ts =
                            agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
                        anyhow::bail!(
                            "startup-miss from {} still belongs to alive pane {} for {}.\n\n{}",
                            miss_ts,
                            conflicting_pane,
                            file.display(),
                            format_existing_pane_conflict_error(
                                &tmux,
                                file,
                                &pane_id,
                                &conflicting_pane
                            )
                        );
                    }
                    anyhow::bail!(
                        "{}",
                        format_existing_pane_conflict_error(
                            &tmux,
                            file,
                            &pane_id,
                            &conflicting_pane
                        )
                    );
                }
            }
        }
    } else {
        start_console_status(
            &mut session_log,
            route_owned,
            format!(
                "[start] --force: bypassing existing session pane reuse for {}",
                file.display()
            ),
        );
    }

    if let Some(expected_session) = agent_doc_project_config_io::project_tmux_session()
        && !relocate_if_wrong_session(&tmux, &pane_id, &expected_session)
    {
        rebind_project_tmux_session_if_expected_dead(&tmux, &pane_id, &expected_session);
    }

    let prior_entry = agent_doc_session_registry_io::lookup_entry(&session_id)?;
    let pane_window = agent_doc_tmux_io::target_window_id(&tmux, &pane_id).unwrap_or_default();
    let supervisor_instance_id = if admission.preserves_session_lifecycle() {
        prior_entry
            .as_ref()
            .map(|entry| entry.supervisor_instance_id.trim())
            .filter(|instance_id| !instance_id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let actor_record = if admission.preserves_session_lifecycle() {
        agent_doc_controller_io::project_controller::ensure_controller_running(
            &project_root,
            LaunchMode::Lazy,
        )?;
        let record = agent_doc_controller_io::project_controller::authoritative_actor_binding(
            &project_root,
            &canonical,
        )?
        .with_context(|| {
            format!(
                "cannot reenter supervisor for {} without an authoritative actor binding",
                canonical.display()
            )
        })?;
        let record = validate_supervisor_reentry_actor(&canonical, &session_id, &pane_id, record)?;
        log_event(
            &mut session_log,
            &format!(
                "supervisor_reexec_session_reentry file={} pane={} session={} generation={} state={} lifecycle=preserved",
                file.display(),
                pane_id,
                session_id_short(&session_id),
                record.generation,
                record.state.as_str()
            ),
        );
        record
    } else {
        let start_generation = {
            let generations = agent_doc_session_actor_io::next_generation(&canonical, &session_id)
                .unwrap_or(agent_doc_supervisor::OwnershipGeneration {
                    prior_generation: 0,
                    new_generation: 1,
                });
            log_event(
                &mut session_log,
                &agent_doc_supervisor::format_transition_event(
                    agent_doc_supervisor::OwnershipTransitionEvent {
                        caller: "start",
                        reason: "session_start",
                        prior_generation: generations.prior_generation,
                        new_generation: generations.new_generation,
                        old_pane: prior_entry.as_ref().map(|entry| entry.pane.as_str()),
                        new_pane: &pane_id,
                        old_window: prior_entry.as_ref().and_then(|entry| {
                            (!entry.window.is_empty()).then_some(entry.window.as_str())
                        }),
                        new_window: Some(pane_window.as_str()),
                    },
                ),
            );
            generations.new_generation
        };
        log_event(
            &mut session_log,
            &format!(
                "session_start file={} pane={} session={} generation={}",
                file.display(),
                pane_id,
                session_id_short(&session_id),
                start_generation
            ),
        );
        let record = start_controller_session(StartControllerSessionInput {
            file,
            canonical: &canonical,
            project_root: &project_root,
            session_id: &session_id,
            pane_id: &pane_id,
            pane_window: &pane_window,
            start_generation,
            session_log: &mut session_log,
        })?;
        log_event(
            &mut session_log,
            &format!(
                "controller_session_start generation={} state={}",
                record.generation,
                record.state.as_str()
            ),
        );
        start_console_status(
            &mut session_log,
            route_owned,
            format!(
                "Registered session {} -> pane {}",
                session_id_short(&session_id),
                pane_id
            ),
        );
        record
    };
    publish_start_supervisor_registry(StartSupervisorRegistryPublication {
        file,
        canonical: &canonical,
        project_root: &project_root,
        session_id: &session_id,
        pane_id: &pane_id,
        pane_window: &pane_window,
        supervisor_instance_id: &supervisor_instance_id,
        session_log: &mut session_log,
        route_owned,
    });

    if !admission.preserves_session_lifecycle() {
        fire_session_start_hooks(file, &session_id, &fm, &global_config, &harness);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "supervisor_host_gate file={} hosting=in-process",
            file.display()
        ),
    );
    log_event(&mut session_log, "supervisor_host_gate hosting=in-process");

    Ok(StartRuntime {
        session_id,
        fm,
        global_config,
        canonical,
        project_root,
        session_log,
        stderr_redirect,
        harness,
        pane_id,
        supervisor_instance_id,
        actor_record,
        post_start_document_model_ensure,
    })
}

fn close_stale_start_actors(
    project_root: &Path,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) {
    match agent_doc_controller_io::project_controller::close_stale_starting_actors_for_caller(
        project_root,
        Duration::from_secs(3600),
        false,
        "start",
    ) {
        Ok((closed, kept)) if closed > 0 => start_console_status(
            session_log,
            route_owned,
            format!("[start] actors: {closed} stale starting closed, {kept} still active"),
        ),
        Ok(_) => {}
        Err(e) => start_console_status(
            session_log,
            route_owned,
            format!("[start] actor gc warning: {e}"),
        ),
    }
    match agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
        project_root,
        false,
        "start",
        "stale_dead_pane_actor",
    ) {
        Ok((closed, kept)) if closed > 0 => start_console_status(
            session_log,
            route_owned,
            format!("[start] actors: {closed} stale dead-pane closed, {kept} still active"),
        ),
        Ok(_) => {}
        Err(e) => start_console_status(
            session_log,
            route_owned,
            format!("[start] dead-pane actor gc warning: {e}"),
        ),
    }
}

fn report_harness_resolution(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    harness: &agent_doc_harness::HarnessConfig,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) {
    let (source, _resolved_name) = if fm.agent.is_some() {
        ("frontmatter", fm.agent.as_deref().unwrap_or("?"))
    } else if global_config.default_agent.is_some() {
        (
            "config",
            global_config.default_agent.as_deref().unwrap_or("?"),
        )
    } else {
        ("fallback", "claude")
    };
    let env_harness = agent_doc_model_tier::detect_harness();
    start_console_status(
        session_log,
        route_owned,
        format!(
            "[start] harness resolved: binary={} source={} env={}",
            harness.binary, source, env_harness
        ),
    );
    if env_harness != "default" && env_harness != harness.binary {
        start_console_status(
            session_log,
            route_owned,
            format!(
                "[start] WARNING: harness mismatch - from_context resolved {} (via {}) but env detect_harness returned {}",
                harness.binary, source, env_harness
            ),
        );
    }
}

fn ensure_inside_tmux(file: &Path) -> Result<()> {
    if agent_doc_tmux_io::in_tmux() {
        return Ok(());
    }
    let tmux = tmux_for_environment();
    let report = terminal_host_report(&tmux);
    anyhow::bail!(
        "start for {} is still outside tmux after bootstrap: {}. Retry `agent-doc start {}`; inspect `agent-doc env --json` if the host cannot provision a pane",
        file.display(),
        report.reason,
        file.display(),
    )
}

fn clear_superseded_startup_miss(
    file: &Path,
    session_log: &mut Option<std::fs::File>,
    route_owned: bool,
) -> Result<()> {
    if let Some((miss, supersession)) =
        agent_doc_supervisor_io::startup_miss::take_superseded_startup_miss(
            agent_doc_supervisor_io::startup_miss::session_registry_lookup(),
            file,
        )?
    {
        let miss_ts = agent_doc_supervisor::startup_miss::format_timestamp(miss.timestamp);
        start_console_status(
            session_log,
            route_owned,
            format!(
                "[start] clearing stale startup-miss on pane {} from {} for {} because newer registered owner {} already took over",
                miss.pane_id,
                miss_ts,
                file.display(),
                supersession.registered_pane
            ),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "start_startup_miss_cleared_superseded file={} stale_pane={} registered_pane={} miss_timestamp={} latest_start_timestamp={}",
                file.display(),
                miss.pane_id,
                supersession.registered_pane,
                miss_ts,
                supersession.latest_start_timestamp
            ),
        );
    }
    Ok(())
}

pub fn existing_session_pane_action(
    tmux: &tmux_router::Tmux,
    session_id: &str,
    file: &Path,
    current_pane: &str,
) -> Result<Option<ExistingSessionPaneAction>> {
    let entry = agent_doc_session_registry_io::lookup_entry(session_id)?;
    let live_owner = agent_doc_sync_io::sync::find_normal_path_owner_pane_excluding_quiet(
        tmux,
        file,
        session_id,
        Some(current_pane),
    );
    let entry_ref = entry.as_ref();
    let effective_entry = if let Some(entry) = entry_ref {
        if tmux.pane_alive(&entry.pane)
            && let Some(other) =
                agent_doc_sync_io::sync::pane_owned_document_other_than(tmux, &entry.pane, file)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "start_stale_registry_cross_document_pane_ignored file={} pane={} pane_owns={} session_id={}",
                    file.display(),
                    entry.pane,
                    other,
                    session_id
                ),
            );
            None
        } else {
            entry_ref
        }
    } else {
        None
    };
    Ok(existing_session_pane_action_from_entry(
        tmux,
        current_pane,
        effective_entry,
        live_owner.as_deref(),
    ))
}

pub fn existing_session_pane_action_from_entry(
    tmux: &tmux_router::Tmux,
    current_pane: &str,
    entry: Option<&tmux_router::RegistryEntry>,
    live_owner: Option<&str>,
) -> Option<ExistingSessionPaneAction> {
    let registry_pane = entry.map(|entry| entry.pane.as_str());
    let registry_pane_alive = registry_pane
        .map(|pane| tmux.pane_alive(pane))
        .unwrap_or(false);
    agent_doc_supervisor::session_owner::existing_session_pane_action(
        current_pane,
        registry_pane,
        registry_pane_alive,
        live_owner,
    )
}

pub fn format_existing_pane_conflict_error(
    tmux: &tmux_router::Tmux,
    file: &Path,
    current_pane: &str,
    conflicting_pane: &str,
) -> String {
    let conflict_session = tmux.pane_session(conflicting_pane).unwrap_or_default();
    let conflict_window =
        agent_doc_tmux_io::target_window_id(tmux, conflicting_pane).unwrap_or_default();
    let current_session = tmux.pane_session(current_pane).unwrap_or_default();
    let current_window =
        agent_doc_tmux_io::target_window_id(tmux, current_pane).unwrap_or_default();
    let document = file.display().to_string();
    format_existing_pane_conflict_error_from_facts(&ExistingPaneConflictFacts {
        document: &document,
        current_pane,
        conflicting_pane,
        conflict_session: &conflict_session,
        conflict_window: &conflict_window,
        current_session: &current_session,
        current_window: &current_window,
    })
}

pub fn relocate_if_wrong_session(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    expected_session: &str,
) -> bool {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(s) => s,
        Err(_) => return true,
    };
    if actual_session == expected_session {
        return true;
    }
    eprintln!(
        "[start] pane {} is in session '{}', expected '{}' — auto-relocating to project session",
        pane_id, actual_session, expected_session
    );
    if let Some(anchor) = tmux.active_pane(expected_session) {
        match tmux_router::PaneMoveOp::new(tmux, pane_id, &anchor)
            .allow_cross_session("auto-relocate to project session on start")
            .join("-dh")
        {
            Ok(()) => {
                eprintln!(
                    "[start] relocated pane {} → session '{}'",
                    pane_id, expected_session
                );
                true
            }
            Err(e) => {
                eprintln!(
                    "[start] WARNING: relocation failed ({}); pane {} will register in session '{}'",
                    e, pane_id, actual_session
                );
                false
            }
        }
    } else {
        eprintln!(
            "[start] WARNING: no active pane found in session '{}'; \
             pane {} will register in session '{}'",
            expected_session, pane_id, actual_session
        );
        false
    }
}

pub fn rebind_project_tmux_session_if_expected_dead(
    tmux: &tmux_router::Tmux,
    pane_id: &str,
    expected_session: &str,
) {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(session) => session,
        Err(_) => return,
    };
    if actual_session == expected_session || tmux.session_alive(expected_session) {
        return;
    }
    match agent_doc_project_config_io::update_project_tmux_session(&actual_session) {
        Ok(()) => eprintln!(
            "[start] configured project session '{}' is dead — rebound tmux_session to '{}'",
            expected_session, actual_session
        ),
        Err(e) => eprintln!(
            "[start] WARNING: configured project session '{}' is dead but failed to persist tmux_session '{}': {}",
            expected_session, actual_session, e
        ),
    }
}

struct StartControllerSessionInput<'a> {
    file: &'a Path,
    canonical: &'a Path,
    project_root: &'a Path,
    session_id: &'a str,
    pane_id: &'a str,
    pane_window: &'a str,
    start_generation: u64,
    session_log: &'a mut Option<std::fs::File>,
}

struct StartSupervisorRegistryPublication<'a> {
    file: &'a Path,
    canonical: &'a Path,
    project_root: &'a Path,
    session_id: &'a str,
    pane_id: &'a str,
    pane_window: &'a str,
    supervisor_instance_id: &'a str,
    session_log: &'a mut Option<std::fs::File>,
    route_owned: bool,
}

fn publish_start_supervisor_registry(input: StartSupervisorRegistryPublication<'_>) {
    let StartSupervisorRegistryPublication {
        file,
        canonical,
        project_root,
        session_id,
        pane_id,
        pane_window,
        supervisor_instance_id,
        session_log,
        route_owned,
    } = input;
    let canonical_file = canonical.to_string_lossy().to_string();
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    match sessions::register_start_supervisor_in(
        project_root,
        session_id,
        pane_id,
        &canonical_file,
        std::process::id(),
        pane_window,
        &cwd,
        supervisor_instance_id,
    ) {
        Ok(()) => {
            log_event(
                session_log,
                &format!(
                    "start_registry_publish file={} pane={} session={} project_root={}",
                    canonical.display(),
                    pane_id,
                    session_id,
                    project_root.display()
                ),
            );
        }
        Err(err) => {
            let message = format!(
                "start_registry_publish_failed file={} pane={} session={} project_root={} err={err:#}",
                canonical.display(),
                pane_id,
                session_id,
                project_root.display()
            );
            agent_doc_ops_log_io::log_op(file, &message);
            log_event(session_log, &message);
            start_console_status(
                session_log,
                route_owned,
                format!("[start] warning: durable registry refresh failed: {err}"),
            );
        }
    }
}

fn start_controller_session(
    input: StartControllerSessionInput<'_>,
) -> Result<agent_doc_controller::actor::ActorRecord> {
    let StartControllerSessionInput {
        file,
        canonical,
        project_root,
        session_id,
        pane_id,
        pane_window,
        start_generation,
        session_log,
    } = input;
    agent_doc_controller_io::project_controller::ensure_controller_running(
        project_root,
        LaunchMode::Lazy,
    )?;
    let start_request = agent_doc_controller_io::project_controller::StartSessionRequest {
        file: canonical.to_path_buf(),
        session_id: session_id.to_string(),
        pane_id: pane_id.to_string(),
        window_id: pane_window.to_string(),
        generation: start_generation,
    };
    let mut attempts_used = 0usize;
    const MAX_START_SESSION_RECYCLE_RETRIES: usize = 2;
    loop {
        match agent_doc_controller_io::project_controller::start_session(
            project_root,
            start_request.clone(),
        ) {
            Ok(record) => break Ok(record),
            Err(err) => {
                let recycle_status =
                    agent_doc_controller_io::project_controller::supervisor_recycle_status_for_file(
                        file,
                    )
                    .unwrap_or_default();
                let recycle_pending = matches!(
                    recycle_status.phase,
                    agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
                );
                if !start_session_retryable_during_recycle(
                    recycle_pending,
                    attempts_used,
                    MAX_START_SESSION_RECYCLE_RETRIES,
                ) {
                    break Err(err);
                }
                attempts_used += 1;
                let reason = recycle_status
                    .reason
                    .unwrap_or_else(|| "unknown".to_string());
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "start_session_recycle_retry file={} pane={} attempt={} reason={} err={}",
                        file.display(),
                        pane_id,
                        attempts_used,
                        reason,
                        err
                    ),
                );
                log_event(
                    session_log,
                    &format!("start_session_recycle_retry attempt={attempts_used} reason={reason}"),
                );
                let settled = agent_doc_controller_io::project_controller::
                    wait_for_supervisor_recycle_settle_for_file(file)
                        .map(|projection| {
                            matches!(
                                projection.phase,
                                agent_doc_state_backbone::SupervisorRecyclePhase::Settled
                            )
                        })
                        .unwrap_or(false);
                if !settled {
                    break Err(err);
                }
                agent_doc_controller_io::project_controller::ensure_controller_running(
                    project_root,
                    LaunchMode::Lazy,
                )?;
            }
        }
    }
}

fn fire_session_start_hooks(
    file: &Path,
    session_id: &str,
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    harness: &agent_doc_harness::HarnessConfig,
) {
    let harness_name = agent_doc_model_tier::harness_key_for_agent_name(&harness.binary);
    let resolved_model = fm.resolve_harness_model(&harness_name).map(|s| {
        agent_doc_model_tier::canonical_model_name(s, &harness_name, &global_config.model)
    });
    agent_doc_hooks_io::fire_doc_hooks(
        &fm.hooks,
        "session_start",
        file,
        session_id,
        &fm.agent,
        &resolved_model,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn session_id_short_never_panics_on_short_or_multibyte_sessions() {
        // Full UUID: normal 8-char prefix.
        assert_eq!(
            session_id_short("bec81dd7-71a1-488e-b716-8a0622713142"),
            "bec81dd7"
        );
        // Shorter than 8 bytes (legacy/hand-edited) — must not panic.
        assert_eq!(session_id_short("abc"), "abc");
        assert_eq!(session_id_short(""), "");
        // Multibyte char straddling byte 8 — `str::get(..8)` returns None, so we
        // fall back to the whole id instead of panicking on a char boundary.
        let multibyte = "sessioné-xyz"; // 'é' is 2 bytes, spanning bytes 7..9
        assert_eq!(session_id_short(multibyte), multibyte);
    }

    #[test]
    fn copied_document_session_identity_is_rekeyed_before_start_routing() {
        let dir = tempfile::tempdir().unwrap();
        let owner_file = dir.path().join("original.md");
        let copy_file = dir.path().join("copy.md");
        let copied_session = "38ba225e-ceae-4baa-aa17-8a7edec36148";
        let mut registry = tmux_router::Registry::new();
        let owner_key = tmux_router::registry::canonical_registry_key_in(
            dir.path(),
            &owner_file.display().to_string(),
        );
        registry.insert(
            owner_key,
            tmux_router::RegistryEntry {
                pane: "%88".to_string(),
                pid: 88,
                cwd: dir.path().display().to_string(),
                started: "2026-08-02T01:29:54Z".to_string(),
                session_id: copied_session.to_string(),
                file: owner_file.display().to_string(),
                window: "@27".to_string(),
                supervisor_instance_id: "supervisor-88".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();
        let content =
            format!("---\nagent_doc_session: {copied_session}\nagent: codex\n---\n\n# Copy\n");

        let resolved = resolve_start_session_identity_with_publisher(
            dir.path(),
            &copy_file,
            content,
            copied_session.to_string(),
            agent_doc_controller_io::project_controller::append_state_event_for_test,
        )
        .unwrap();

        assert_ne!(resolved.session_id, copied_session);
        assert_eq!(
            frontmatter::session_id_from_content(&resolved.content).as_deref(),
            Some(resolved.session_id.as_str())
        );
        let rekey = resolved.rekey.unwrap();
        assert_eq!(rekey.previous_session_id, copied_session);
        assert_eq!(rekey.owner.file, owner_file.display().to_string());
        assert!(rekey.owner.started.starts_with("state-event:"));

        agent_doc_session_registry_io::save_in(dir.path(), &tmux_router::Registry::new()).unwrap();
        let owner_content =
            format!("---\nagent_doc_session: {copied_session}\nagent: codex\n---\n\n# Original\n");
        let owner_after_registry_recycle = resolve_start_session_identity_with_publisher(
            dir.path(),
            &owner_file,
            owner_content,
            copied_session.to_string(),
            agent_doc_controller_io::project_controller::append_state_event_for_test,
        )
        .unwrap();
        assert_eq!(owner_after_registry_recycle.session_id, copied_session);
        assert!(owner_after_registry_recycle.rekey.is_none());
    }

    #[test]
    fn start_console_status_suppresses_route_owned_stderr_by_default() {
        let mut log = tempfile::tempfile().unwrap();
        let mut cloned = Some(log.try_clone().unwrap());
        start_console_status(&mut cloned, true, "[start] harness resolved: binary=codex");
        drop(cloned);

        use std::io::{Read, Seek, SeekFrom};
        log.seek(SeekFrom::Start(0)).unwrap();
        let mut content = String::new();
        log.read_to_string(&mut content).unwrap();
        assert!(
            content.contains("start_console_status route_owned=true printed=false"),
            "{content}"
        );
        assert!(content.contains("[start] harness resolved: binary=codex"));
    }

    #[test]
    fn start_admission_fallback_is_limited_to_editor_model_unavailable() {
        assert_eq!(
            start_admission_fallback_for_current_text(
                &agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
            ),
            Some(StartAdmissionReadAuthority::DiskMetadataBootstrapEditorModelUnavailable)
        );
        assert_eq!(
            start_admission_fallback_for_current_text(
                &agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
            ),
            Some(StartAdmissionReadAuthority::DiskMetadataBootstrapEditorModelUnavailable)
        );
        assert_eq!(
            start_admission_fallback_for_current_text(
                &agent_doc_crdt_relay_io::CurrentText::Detached
            ),
            None
        );
        assert_eq!(
            start_admission_fallback_for_current_text(
                &agent_doc_crdt_relay_io::CurrentText::Current {
                    text: "live".to_string(),
                    live_editors: 1,
                    delivery_converged: true,
                    delivery_version: 1,
                    semantics: None,
                }
            ),
            None
        );
    }

    #[test]
    fn start_admission_bootstraps_metadata_from_disk_when_editor_model_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let disk = "---\nsession: start-admission-test\n---\n\n# Session\n";
        let mut handle = std::fs::File::create(&file).unwrap();
        handle.write_all(disk.as_bytes()).unwrap();
        drop(handle);

        let pid = std::process::id();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: pid.into(),
                tag: format!("test-editor-{pid}"),
            }]);

        let document = resolve_start_admission_document(&file).unwrap();

        assert_eq!(document.content, disk);
        assert_eq!(
            document.authority,
            StartAdmissionReadAuthority::DiskMetadataBootstrapEditorModelUnavailable
        );
        assert!(document.authority.needs_post_start_document_model_ensure());
    }

    fn reentry_actor(
        session_id: &str,
        pane_id: &str,
        generation: u64,
        state: agent_doc_controller::actor::ActorState,
    ) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: "/tmp/reentry.md".to_string(),
            session_id: session_id.to_string(),
            generation,
            pane_id: pane_id.to_string(),
            window_id: "@7".to_string(),
            harness: "claude".to_string(),
            state,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "supervisor".to_string(),
                reason: "turn_started".to_string(),
                timestamp: 1,
                prior_generation: generation,
                new_generation: generation,
            },
        }
    }

    #[test]
    fn surviving_child_reentry_preserves_actor_generation_and_state() {
        let record = reentry_actor(
            "session-a",
            "%26",
            532,
            agent_doc_controller::actor::ActorState::Busy,
        );

        let retained = validate_supervisor_reentry_actor(
            Path::new("/tmp/reentry.md"),
            "session-a",
            "%26",
            record.clone(),
        )
        .unwrap();

        assert_eq!(retained, record);
        assert_eq!(retained.generation, 532);
        assert_eq!(
            retained.state,
            agent_doc_controller::actor::ActorState::Busy
        );
    }

    #[test]
    fn surviving_child_reentry_rejects_binding_drift_without_replacement() {
        for record in [
            reentry_actor(
                "session-other",
                "%26",
                532,
                agent_doc_controller::actor::ActorState::Busy,
            ),
            reentry_actor(
                "session-a",
                "%27",
                532,
                agent_doc_controller::actor::ActorState::Busy,
            ),
            reentry_actor(
                "session-a",
                "%26",
                532,
                agent_doc_controller::actor::ActorState::Closed,
            ),
        ] {
            assert!(
                validate_supervisor_reentry_actor(
                    Path::new("/tmp/reentry.md"),
                    "session-a",
                    "%26",
                    record,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn start_registry_publication_uses_project_root_after_controller_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/professional/sampleportal.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: efs\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let mut session_log = Some(tempfile::tempfile().unwrap());

        publish_start_supervisor_registry(StartSupervisorRegistryPublication {
            file: &doc,
            canonical: &doc,
            project_root: dir.path(),
            session_id: "efs",
            pane_id: "%26",
            pane_window: "@3",
            supervisor_instance_id: "sup-efs",
            session_log: &mut session_log,
            route_owned: true,
        });

        let registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        let key =
            tmux_router::registry::canonical_registry_key_in(dir.path(), &doc.to_string_lossy());
        let entry = registry
            .get(&key)
            .expect("start registry publication should write the project-root registry");
        assert_eq!(entry.session_id, "efs");
        assert_eq!(entry.pane, "%26");
        assert_eq!(entry.window, "@3");
        assert_eq!(entry.supervisor_instance_id, "sup-efs");
    }

    #[test]
    fn tmux_session_name_precedence_is_explicit_then_project_then_default() {
        assert_eq!(
            resolve_tmux_session_name(Some("explicit"), Some("configured")).unwrap(),
            ("explicit".to_string(), "explicit")
        );
        assert_eq!(
            resolve_tmux_session_name(None, Some("configured")).unwrap(),
            ("configured".to_string(), "project_config")
        );
        assert_eq!(
            resolve_tmux_session_name(None, None).unwrap(),
            ("0".to_string(), "default")
        );
        assert!(resolve_tmux_session_name(Some("  "), None).is_err());
    }

    #[test]
    fn tmux_ensure_is_idempotent_on_an_isolated_headless_server() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nagent_doc_session: tmux-ensure\n---\n").unwrap();
        let socket = format!("agent-doc-ensure-{}", uuid::Uuid::new_v4());
        let tmux = tmux_router::tmux::IsolatedTmux::new(&socket);

        let first = ensure_tmux_session_with(&tmux, &file, Some("headless")).unwrap();
        let second = ensure_tmux_session_with(&tmux, &file, Some("headless")).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert!(!first.attached);
        assert!(!second.attached);
        assert_eq!(first.session_name, "headless");
        assert_eq!(first.session_name, second.session_name);
        assert_eq!(first.pane_id, second.pane_id);
        assert_eq!(tmux.list_session_panes("headless").len(), 1);
        assert!(first.attach_command.contains("attach-session -t headless"));
    }

    #[test]
    fn ide_tmux_ensure_uses_project_attach_template() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"
[terminal]
host = "ide"
attach_command = "custom-attach --session {session} --command {tmux_command}"
"#,
        )
        .unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nagent_doc_session: ide-policy\n---\n").unwrap();
        let socket = format!("agent-doc-ensure-{}", uuid::Uuid::new_v4());
        let tmux = tmux_router::tmux::IsolatedTmux::new(&socket);

        let outcome = ensure_tmux_session_with_ide(&tmux, &file, Some("ide-policy"), true)
            .expect("IDE ensure must use the binary-owned policy");

        assert_eq!(outcome.terminal_host, ResolvedTerminalHost::Ide);
        assert!(
            outcome
                .attach_command
                .starts_with("custom-attach --session ide-policy --command tmux -L")
        );
        assert!(
            outcome
                .attach_command
                .contains("attach-session -t ide-policy")
        );
    }

    #[test]
    fn tmux_ensure_respects_disabled_auto_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"
[terminal]
host = "none"
auto_start_tmux = false
"#,
        )
        .unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nagent_doc_session: manual-tmux\n---\n").unwrap();
        let socket = format!("agent-doc-ensure-{}", uuid::Uuid::new_v4());
        let tmux = tmux_router::tmux::IsolatedTmux::new(&socket);

        let error = ensure_tmux_session_with_ide(&tmux, &file, Some("manual-tmux"), true)
            .expect_err("missing session must not be created");

        assert!(error.to_string().contains("auto-start is disabled"));
        assert!(!tmux.session_exists("manual-tmux"));
    }

    #[test]
    fn reexec_command_preserves_start_lifecycle_flags() {
        let command = start_reexec_command(
            Path::new("tasks/a document.md"),
            true,
            true,
            agent_doc_supervisor::route_owned::RouteOwnedReapPolicy::KeepAlive,
            Some(&agent_doc_harness::ResumeRequest::Id(
                "conversation-id".into(),
            )),
        )
        .unwrap();

        assert!(command.contains("start 'tasks/a document.md'"));
        assert!(command.contains("--force"));
        assert!(command.contains("--resume conversation-id"));
        assert!(command.contains("--route-owned-reap-policy keep-alive"));
    }
}
