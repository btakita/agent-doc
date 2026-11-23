//! Per-request authorization and session ownership for the project
//! controller (`#ghrecvpolicy` phase 2,
//! `specs/proposals/multi-user-access-control.md`).
//!
//! Socket admission ([`super::workspace_access`]) names the principal and role
//! behind a connection. This module attaches that identity to the request
//! thread, checks each command against the pure capability table
//! ([`controller_request_decision`]), and keeps the session-owner ledger: the
//! principal that claimed or started a document's session. One principal can
//! never drive another principal's session, pane, or claimed queue item.
//!
//! Single-user mode, the default, pays nothing: the controller's own user is
//! admitted as owner without file I/O, and an owner request skips this module
//! except for the rare claim/start/attach command, which consults the ledger
//! only once per controller lifetime to learn that no ledger exists.

use std::path::Path;

use agent_doc_frontmatter::workspace_access::{
    ControllerCapability, ControllerRequestDecision, Principal, Role, SessionOwnerUpdate,
    SessionTarget, controller_command_capability, controller_request_decision,
};
use agent_doc_project_config_io::workspace_access::{self as config_access, SenderPrincipalScope};

use super::workspace_access::ControllerWorkspaceAccess;

/// The principal and role socket admission granted a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmittedPrincipal {
    pub(crate) principal: Principal,
    pub(crate) role: Role,
}

thread_local! {
    /// The admitted principal of the connection this request thread serves,
    /// beside `CONTROLLER_REQUEST_PEER_PID`. `None` for in-process requests,
    /// which run as the controller's own user.
    static CONTROLLER_REQUEST_PRINCIPAL: std::cell::Cell<Option<AdmittedPrincipal>> =
        const { std::cell::Cell::new(None) };
}

/// Restores the previous request principal (and component-creation sender)
/// when a served request leaves its scope.
pub(crate) struct ControllerRequestPrincipalScope {
    previous: Option<AdmittedPrincipal>,
    _sender: SenderPrincipalScope,
}

impl Drop for ControllerRequestPrincipalScope {
    fn drop(&mut self) {
        CONTROLLER_REQUEST_PRINCIPAL.set(self.previous);
    }
}

/// Attach `admitted` to this request thread. A principal other than the
/// controller's own user also becomes the sender that component creation on
/// this thread is attributed to (GH #40), so a scaffold the controller adds on
/// a member's behalf follows the recipient's `[receive]` policy.
pub(crate) fn enter_controller_request_principal(
    controller_user: Principal,
    admitted: Option<AdmittedPrincipal>,
) -> ControllerRequestPrincipalScope {
    let sender = admitted
        .map(|admitted| admitted.principal)
        .filter(|principal| *principal != controller_user);
    ControllerRequestPrincipalScope {
        previous: CONTROLLER_REQUEST_PRINCIPAL.replace(admitted),
        _sender: config_access::enter_sender_principal(sender),
    }
}

/// The admitted principal of the request this thread serves.
pub(crate) fn controller_request_principal() -> Option<AdmittedPrincipal> {
    CONTROLLER_REQUEST_PRINCIPAL.get()
}

/// A ledger update to apply once the authorized request succeeds.
#[derive(Debug, Default)]
pub(crate) struct RequestGate {
    pending: Option<(String, SessionOwnerUpdate)>,
}

impl RequestGate {
    /// Record the session owner when `response` reports success. A claim or
    /// start that failed leaves the ledger untouched.
    pub(crate) fn finish(self, access: &ControllerWorkspaceAccess, response: &str) {
        let Some((key, update)) = self.pending else {
            return;
        };
        let succeeded = serde_json::from_str::<serde_json::Value>(response)
            .ok()
            .is_some_and(|value| value["ok"] == true);
        if !succeeded {
            return;
        }
        if let Err(err) = access.update_session_owner(&key, update) {
            eprintln!("[controller] session owner ledger update failed for {key}: {err:#}");
        }
    }
}

/// Authorize one request line from `admitted`. `command` is the line's
/// already-parsed command name. `Err` carries the operator-facing refusal.
pub(crate) fn authorize_controller_request(
    access: &ControllerWorkspaceAccess,
    owner: Principal,
    admitted: Option<AdmittedPrincipal>,
    command: &str,
    line: &str,
) -> Result<RequestGate, String> {
    // In-process requests and platforms without peer credentials act as the
    // controller's own user.
    let Some(admitted) = admitted else {
        return Ok(RequestGate::default());
    };
    let capability = controller_command_capability(command);
    if admitted.role == Role::Owner {
        // Owners are never limited. Only taking a session touches the ledger,
        // to drop an earlier member's record, and only when one can exist.
        if capability != ControllerCapability::ClaimSession || !access.session_ledger_may_exist() {
            return Ok(RequestGate::default());
        }
        let key = request_session_key(access.project_root(), line, false);
        return Ok(RequestGate {
            pending: key.map(|key| (key, SessionOwnerUpdate::Clear)),
        });
    }
    let key = capability
        .targets_session()
        .then(|| request_session_key(access.project_root(), line, true))
        .flatten();
    let session = key.as_deref().map(|key| {
        let recorded_owner =
            config_access::load_session_owners(access.project_root()).owner_of(key);
        // Only an unrecorded claim asks whether the registry already binds
        // the document (the workspace owner's session).
        let bound = recorded_owner.is_none()
            && capability == ControllerCapability::ClaimSession
            && registry_binds(access.project_root(), key);
        SessionTarget {
            recorded_owner,
            bound,
        }
    });
    let decision =
        controller_request_decision(owner, admitted.principal, admitted.role, command, session);
    match decision {
        ControllerRequestDecision::Allow(update) => Ok(RequestGate {
            pending: key
                .filter(|_| update != SessionOwnerUpdate::Keep)
                .map(|key| (key, update)),
        }),
        refused @ ControllerRequestDecision::Refuse { .. } => Err(refused
            .refusal_message(command)
            .unwrap_or_else(|| format!("project controller refused `{command}`"))),
    }
}

/// `agent-doc claim` by a principal other than the workspace owner: ask the
/// running project controller to record this principal as the document's
/// session owner before the claim binds anything. The controller takes the
/// principal from the connection's kernel credentials, never from the
/// request, and refuses when another principal holds the session. A member
/// never launches the owner's controller; with none running the claim fails
/// closed.
pub fn claim_session_owner(project_root: &Path, file: &Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let request = super::ControllerRequest {
        command: "session_owner_claim".to_string(),
        file: Some(file.to_path_buf()),
        session_id: None,
        pane_id: None,
        window_id: None,
        generation: None,
        state: None,
        caller: Some("claim".to_string()),
        reason: Some("session_owner_claim".to_string()),
        supervisor_pid: None,
        supervisor_socket: None,
        command_kind: None,
        diagnostic_payload: None,
        sequence: None,
    };
    super::rpc::request_existing_controller_with_timeout::<serde_json::Value>(
        project_root,
        request,
        std::time::Duration::from_secs(5),
    )
    .map(|_| ())
    .with_context(|| {
        format!(
            "claiming {} as a workspace member needs the project controller to record session ownership",
            file.display()
        )
    })
}

/// The registry key of the document a request targets: its `file`, else (for
/// a non-owner) the document the request's `pane_id` is bound to. `None` when
/// neither resolves to exactly one document.
fn request_session_key(project_root: &Path, line: &str, resolve_pane: bool) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(line.trim()).ok()?;
    if let Some(file) = value["file"]
        .as_str()
        .filter(|file| !file.trim().is_empty())
    {
        return Some(tmux_router::registry::canonical_registry_key_in(
            project_root,
            file,
        ));
    }
    if !resolve_pane {
        return None;
    }
    let pane = value["pane_id"].as_str()?.trim();
    if pane.is_empty() {
        return None;
    }
    let registry = agent_doc_session_registry_io::load_existing_in(project_root).ok()?;
    let mut keys = registry
        .values()
        .filter(|entry| entry.pane == pane && !entry.file.trim().is_empty())
        .map(|entry| tmux_router::registry::canonical_registry_key_in(project_root, &entry.file))
        .collect::<std::collections::BTreeSet<_>>();
    (keys.len() == 1).then(|| keys.pop_first()).flatten()
}

/// Whether the registry already binds a session for the document at `key`.
fn registry_binds(project_root: &Path, key: &str) -> bool {
    agent_doc_session_registry_io::load_existing_in(project_root)
        .map(|registry| {
            registry.iter().any(|(registry_key, entry)| {
                registry_key == key
                    || (!entry.file.trim().is_empty()
                        && tmux_router::registry::canonical_registry_key_in(
                            project_root,
                            &entry.file,
                        ) == key)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
thread_local! {
    /// Test-only: the effective uid the next accepted connection on this
    /// thread reports instead of its kernel credentials. Lets a single-uid
    /// test exercise admission and authorization for distinct principals over
    /// a real socket (the socket stays `0700`/`0600` until phase 3).
    static SIMULATED_PEER_EUID: std::cell::Cell<Option<u32>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn simulated_peer_euid() -> Option<u32> {
    SIMULATED_PEER_EUID.get()
}

#[cfg(test)]
pub(crate) fn simulate_peer_euid(uid: Option<u32>) {
    SIMULATED_PEER_EUID.set(uid);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    //! SimWorld-style tests: distinct simulated principals talk to a real
    //! controller runtime over a real Unix socket, through `serve_client`.

    use super::*;
    use crate::project_controller::{ControllerRuntime, test_bootstrap};
    use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    const ALICE: u32 = 4_001_001;
    const MALLORY: u32 = 4_001_002;
    const CAROL: u32 = 4_001_003;

    fn me() -> u32 {
        config_access::current_principal().uid
    }

    struct World {
        dir: tempfile::TempDir,
        runtime: Arc<ControllerRuntime>,
        listener: interprocess::local_socket::Listener,
        sock: std::path::PathBuf,
    }

    impl World {
        fn new(config: Option<&str>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
            if let Some(config) = config {
                std::fs::write(dir.path().join(".agent-doc/config.toml"), config).unwrap();
            }
            let bootstrap = test_bootstrap(&dir);
            let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();
            let sock = dir.path().join("authz.sock");
            let listener = ListenerOptions::new()
                .name(sock.clone().to_fs_name::<GenericFilePath>().unwrap())
                .create_sync()
                .unwrap();
            Self {
                dir,
                runtime,
                listener,
                sock,
            }
        }

        fn team() -> Self {
            Self::new(Some(&format!(
                "[workspace]\nmode = \"multi-user\"\n\n[[workspace.members]]\nuid = {ALICE}\nrole = \"editor\"\n\n[[workspace.members]]\nuid = {MALLORY}\nrole = \"editor\"\n\n[[workspace.members]]\nuid = {CAROL}\nrole = \"viewer\"\n"
            )))
        }

        /// Send one request line as `uid` over the real socket and return the
        /// controller's response (or the connection error when refused at
        /// admission).
        fn send(&self, uid: u32, request: serde_json::Value) -> Result<serde_json::Value, String> {
            let sock = self.sock.clone();
            let line = request.to_string();
            let client = std::thread::spawn(move || {
                let stream = interprocess::local_socket::Stream::connect(
                    sock.to_fs_name::<GenericFilePath>().unwrap(),
                )
                .unwrap();
                let (reader, mut writer) = stream.split();
                writer.write_all(line.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
                writer.flush().unwrap();
                let mut response = String::new();
                let _ = BufReader::new(reader).read_line(&mut response);
                response
            });
            let accepted =
                interprocess::local_socket::traits::Listener::accept(&self.listener).unwrap();
            simulate_peer_euid(Some(uid));
            let served = super::super::rpc::serve_client(
                accepted,
                &self.runtime,
                &AtomicBool::new(false),
                &self.sock,
            );
            simulate_peer_euid(None);
            let response = client.join().unwrap();
            served.map_err(|err| format!("{err:#}"))?;
            Ok(serde_json::from_str(response.trim()).unwrap_or(serde_json::Value::Null))
        }

        fn ledger(&self) -> agent_doc_frontmatter::workspace_access::SessionOwners {
            config_access::load_session_owners(self.dir.path())
        }
    }

    fn request(command: &str, file: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "command": command,
            "file": file,
            "session_id": null,
            "pane_id": null,
            "window_id": null,
            "generation": null,
            "state": null,
            "caller": null,
            "reason": null,
            "supervisor_pid": null,
            "supervisor_socket": null,
            "command_kind": null,
            "diagnostic_payload": null,
        })
    }

    fn refusal(response: &serde_json::Value) -> String {
        assert_eq!(response["ok"], false, "{response}");
        response["error"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn single_user_default_owner_requests_read_no_workspace_files() {
        let world = World::new(None);
        for command in ["status", "session_owner_claim", "a_command_added_tomorrow"] {
            let _ = world
                .send(me(), request(command, Some("tasks/doc.md")))
                .unwrap();
        }
        assert_eq!(world.runtime.workspace_access.config_reads(), 0);
        assert_eq!(world.runtime.workspace_access.ledger_checks(), 1);
        assert!(
            !config_access::session_owners_path(world.dir.path()).exists(),
            "the owner never creates the session-owner ledger"
        );
        // A stranger is refused at admission exactly as before.
        let err = world.send(ALICE, request("status", None)).unwrap_err();
        assert!(err.contains("does not belong to the current user"), "{err}");
    }

    #[test]
    fn members_are_admitted_and_limited_to_their_role() {
        let world = World::team();
        // A viewer observes but cannot claim or drive.
        let status = world.send(CAROL, request("status", None)).unwrap();
        assert_eq!(status["active"], true, "{status}");
        let err = refusal(
            &world
                .send(
                    CAROL,
                    request("session_owner_claim", Some("tasks/carol.md")),
                )
                .unwrap(),
        );
        assert!(
            err.contains("a workspace viewer cannot claim-session"),
            "{err}"
        );
        // An editor cannot administer the controller.
        let err = refusal(&world.send(ALICE, request("shutdown", None)).unwrap());
        assert!(
            err.contains("a workspace editor cannot administer"),
            "{err}"
        );
        let err = refusal(&world.send(ALICE, request("recycle_force", None)).unwrap());
        assert!(err.contains("cannot administer"), "{err}");
        // A stranger is still refused at admission.
        let err = world.send(4_009_999, request("status", None)).unwrap_err();
        assert!(err.contains("is not a member"), "{err}");
    }

    #[test]
    fn one_member_never_drives_another_members_session() {
        let world = World::team();
        // Alice claims a free document; the ledger records her.
        let claimed = world
            .send(
                ALICE,
                request("session_owner_claim", Some("tasks/alice.md")),
            )
            .unwrap();
        assert_eq!(claimed["ok"], true, "{claimed}");
        let alice_key =
            tmux_router::registry::canonical_registry_key_in(world.dir.path(), "tasks/alice.md");
        assert_eq!(
            world.ledger().owner_of(&alice_key),
            Some(Principal::os_user(ALICE))
        );

        // Mallory can neither take nor drive Alice's session.
        for command in [
            "session_owner_claim",
            "start_session",
            "dispatch",
            "queue_control",
        ] {
            let err = refusal(
                &world
                    .send(MALLORY, request(command, Some("tasks/alice.md")))
                    .unwrap(),
            );
            assert!(
                err.contains(&format!("the session belongs to uid {ALICE}")),
                "{command}: {err}"
            );
        }
        // Nor can she drive a document whose session the owner holds.
        let err = refusal(
            &world
                .send(MALLORY, request("dispatch", Some("tasks/owner.md")))
                .unwrap(),
        );
        assert!(
            err.contains(&format!("the session belongs to uid {}", me())),
            "{err}"
        );
        // A drive request that names no document is refused.
        let err = refusal(&world.send(MALLORY, request("dispatch", None)).unwrap());
        assert!(err.contains("needs a document or pane"), "{err}");

        // Alice drives her own session: the gate passes (the handler then
        // runs and reports its own errors for this bare request).
        let driven = world
            .send(ALICE, request("queue_control", Some("tasks/alice.md")))
            .unwrap();
        assert!(
            !driven["error"]
                .as_str()
                .unwrap_or_default()
                .contains("project controller refused"),
            "{driven}"
        );

        // The owner retakes the document; Alice's record is dropped, so she
        // cannot drive the owner's session.
        let retaken = world
            .send(me(), request("session_owner_claim", Some("tasks/alice.md")))
            .unwrap();
        assert_eq!(retaken["ok"], true, "{retaken}");
        assert_eq!(world.ledger().owner_of(&alice_key), None);
        let err = refusal(
            &world
                .send(ALICE, request("dispatch", Some("tasks/alice.md")))
                .unwrap(),
        );
        assert!(err.contains("the session belongs to"), "{err}");
    }

    #[test]
    fn a_failed_claim_records_no_owner() {
        let world = World::team();
        // `start_session` without its required fields fails in the handler;
        // the gate passed, but the ledger must stay empty.
        let response = world
            .send(ALICE, request("start_session", Some("tasks/alice.md")))
            .unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert!(world.ledger().sessions.is_empty());
    }

    #[test]
    fn controller_command_table_names_real_commands() {
        let rpc = include_str!("rpc.rs");
        let handler = &rpc[rpc
            .find("pub(crate) fn handle_request_locked(")
            .expect("controller request handler")..];
        for (command, _) in agent_doc_frontmatter::workspace_access::CONTROLLER_COMMAND_CAPABILITIES
        {
            if *command == "api" {
                assert!(handler.contains("request_value[\"command\"] == \"api\""));
                continue;
            }
            assert!(
                handler.contains(&format!("\"{command}\" =>")),
                "capability table lists `{command}`, which the controller does not dispatch"
            );
        }
    }
}
