//! Pure multi-user workspace access model (`#ghrecvpolicy`).
//!
//! Design: `specs/proposals/multi-user-access-control.md`. This module owns
//! the principal, membership, and role types, the `[workspace]` and
//! `[receive]` project-config sections, and the pure decisions every channel
//! shares. File, socket, and process lookups stay in the I/O adapters
//! (`agent-doc-project-config-io::workspace_access`, the controller socket).
//!
//! Every channel resolves to one [`Principal`]: the OS user behind the
//! connection or the process. Agent sessions and editor replicas act as the OS
//! user that runs them. The workspace owner is the OS user that owns the
//! project `.agent-doc` store (the user the project controller runs as).
//!
//! Single-user mode is the default and keeps today's behaviour: the owner is
//! the only principal with a role, so the controller admits only the owner and
//! the recipient `[receive]` policy never applies (every sender is the owner).

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// One authenticated actor: an OS user. Agent sessions, editor replicas, and
/// CLI processes act as the OS user that runs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Principal {
    pub uid: u32,
}

impl Principal {
    pub const fn os_user(uid: u32) -> Self {
        Self { uid }
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "uid {}", self.uid)
    }
}

/// `[workspace] mode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceMode {
    /// One operator. Only the owner has a role. Today's behaviour.
    #[default]
    #[serde(rename = "single-user", alias = "single_user", alias = "single")]
    SingleUser,
    /// Listed members get the role their entry names.
    #[serde(rename = "multi-user", alias = "multi_user", alias = "multi")]
    MultiUser,
}

/// A member's role in a multi-user workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full control: controller administration, every session, every document.
    Owner,
    /// Edits documents, drives only sessions it owns, and sends into other
    /// members' documents subject to their `[receive]` policy.
    Editor,
    /// Reads documents and status. Never writes.
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Editor => "editor",
            Self::Viewer => "viewer",
        }
    }
}

/// One `[[workspace.members]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceMember {
    pub uid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub role: Role,
}

/// `[workspace]` section of `.agent-doc/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub mode: WorkspaceMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<WorkspaceMember>,
}

impl WorkspaceConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// The role `principal` holds in a workspace owned by `owner`, or `None` when
/// it is not a member. The owner always holds [`Role::Owner`]. Member entries
/// count only in [`WorkspaceMode::MultiUser`]; a single-user workspace ignores
/// them, so listing members never widens access until the mode says so.
pub fn role_of(
    owner: Principal,
    workspace: &WorkspaceConfig,
    principal: Principal,
) -> Option<Role> {
    if principal == owner {
        return Some(Role::Owner);
    }
    match workspace.mode {
        WorkspaceMode::SingleUser => None,
        WorkspaceMode::MultiUser => workspace
            .members
            .iter()
            .find(|member| member.uid == principal.uid)
            .map(|member| member.role),
    }
}

/// Why the project controller refused a connecting peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerPeerRefusal {
    /// Single-user workspace and the peer is not the owner (today's rule).
    NotOwner,
    /// Multi-user workspace and the peer is not a listed member.
    NotMember,
}

/// The project controller's admission decision for one socket peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerPeerDecision {
    Admit {
        principal: Principal,
        role: Role,
    },
    Refuse {
        principal: Principal,
        refusal: ControllerPeerRefusal,
    },
}

impl ControllerPeerDecision {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admit { .. })
    }

    /// Operator-facing refusal text, `None` when admitted.
    pub fn refusal_message(&self) -> Option<String> {
        let Self::Refuse { principal, refusal } = self else {
            return None;
        };
        Some(match refusal {
            ControllerPeerRefusal::NotOwner => format!(
                "project controller client does not belong to the current user ({principal} is not the workspace owner; [workspace] mode is single-user)"
            ),
            ControllerPeerRefusal::NotMember => format!(
                "project controller client {principal} is not a member of this multi-user workspace"
            ),
        })
    }
}

/// Admission rule for a peer on the project controller socket. `owner` is the
/// controller's own effective user. Every principal with a role is admitted
/// with that role: the owner always, and in multi-user mode each listed member.
/// Everyone else is refused. Admission is not authorization: each request is
/// then checked against [`controller_request_decision`] (phase 2), which
/// denies non-owners anything the command table does not grant their role.
pub fn controller_peer_decision(
    owner: Principal,
    workspace: &WorkspaceConfig,
    peer: Principal,
) -> ControllerPeerDecision {
    match role_of(owner, workspace, peer) {
        Some(role) => ControllerPeerDecision::Admit {
            principal: peer,
            role,
        },
        None => ControllerPeerDecision::Refuse {
            principal: peer,
            refusal: match workspace.mode {
                WorkspaceMode::SingleUser => ControllerPeerRefusal::NotOwner,
                WorkspaceMode::MultiUser => ControllerPeerRefusal::NotMember,
            },
        },
    }
}

/// What a project-controller command needs from the requesting principal
/// (phase 2). The project owner holds every capability; the table decides what
/// the other roles may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ControllerCapability {
    /// Read documents, projections, and status. Every role.
    Observe,
    /// Edit document text through the editor plane. `editor` and `owner`.
    WriteDocument,
    /// Take a document's session: claim, start, or attach a pane. An `editor`
    /// may take a session no other principal owns, and becomes its owner.
    ClaimSession,
    /// Drive an existing session: dispatch, queue control, lifecycle, pane
    /// focus, closeout. An `editor` may drive only a session it owns.
    DriveSession,
    /// Controller administration, cross-session layout, repair, and every
    /// command the table does not list. `owner` only.
    Administer,
}

impl ControllerCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::WriteDocument => "write-document",
            Self::ClaimSession => "claim-session",
            Self::DriveSession => "drive-session",
            Self::Administer => "administer",
        }
    }

    /// Whether this capability targets one document's session.
    pub fn targets_session(self) -> bool {
        matches!(self, Self::ClaimSession | Self::DriveSession)
    }
}

impl Role {
    /// Whether this role may use `capability` at all. Session-scoped
    /// capabilities still need [`controller_request_decision`]'s ownership
    /// check.
    pub fn grants(self, capability: ControllerCapability) -> bool {
        use ControllerCapability as C;
        match self {
            Self::Owner => true,
            Self::Editor => !matches!(capability, C::Administer),
            Self::Viewer => matches!(capability, C::Observe),
        }
    }
}

/// The per-command capability table. A command missing from it needs
/// [`ControllerCapability::Administer`], so a new command is owner-only until
/// someone classifies it here (deny by default).
pub const CONTROLLER_COMMAND_CAPABILITIES: &[(&str, ControllerCapability)] = {
    use ControllerCapability::{ClaimSession, DriveSession, Observe, WriteDocument};
    &[
        // Observe: status, projections, and current text.
        ("api", Observe),
        ("status", Observe),
        ("handoff_status", Observe),
        ("session_status", Observe),
        ("inspect_actor", Observe),
        ("tmux_focus_state", Observe),
        ("main_layout_eligibility", Observe),
        ("pane_layout_retained_columns", Observe),
        ("tmux_layout_sync_state", Observe),
        ("document_state_projection", Observe),
        ("document_closeout_projection", Observe),
        ("turn_admission_projection", Observe),
        ("queue_context_clear_status", Observe),
        ("queue_drain_stall_status", Observe),
        ("route_submit_status", Observe),
        ("visible_write_commit_candidate_status", Observe),
        ("visible_write_commit_candidate_patch_status", Observe),
        ("visible_write_materialized_carry_forward_status", Observe),
        ("reliable_sync_status", Observe),
        ("supervisor_recycle_status", Observe),
        ("editor_command_status", Observe),
        ("crdt_current_text", Observe),
        ("crdt_revision", Observe),
        ("preflight_read_projection", Observe),
        ("state_subscribe", Observe),
        // WriteDocument: the editor replica plane.
        ("crdt_replica", WriteDocument),
        ("crdt_cp_write", WriteDocument),
        ("crdt_text_adopt", WriteDocument),
        ("crdt_commit_barrier", WriteDocument),
        ("editor_op_capture_update", WriteDocument),
        ("editor_surface_observe", WriteDocument),
        ("editor_surface_forget", WriteDocument),
        ("editor_view_snapshot_observe", WriteDocument),
        ("editor_view_client_retire", WriteDocument),
        ("editor_view_presentation_receipt", WriteDocument),
        ("document_path_transition_observe", WriteDocument),
        ("reliable_sync", WriteDocument),
        ("reliable_sync_outbox", WriteDocument),
        // ClaimSession: establishes the session owner.
        ("session_owner_claim", ClaimSession),
        ("start_session", ClaimSession),
        ("attach_pane", ClaimSession),
        // DriveSession: one document's session, owner-checked.
        ("register_supervisor", DriveSession),
        ("mark_lifecycle", DriveSession),
        ("set_actor_harness", DriveSession),
        ("supervisor_heartbeat", DriveSession),
        ("actor_binding", DriveSession),
        ("dispatch", DriveSession),
        ("editor_route", DriveSession),
        ("focus_document_pane", DriveSession),
        ("queue_control", DriveSession),
        ("route_submit_started", DriveSession),
        ("route_submit_settled", DriveSession),
        ("route_submit_blocked", DriveSession),
        ("queue_context_clear_started", DriveSession),
        ("queue_context_clear_deferred", DriveSession),
        ("queue_context_clear_settled", DriveSession),
        ("queue_drain_stall_continuation_recorded", DriveSession),
        ("queue_drain_stall_continuation_cleared", DriveSession),
        ("supervisor_recycle_requested", DriveSession),
        ("supervisor_recycle_started", DriveSession),
        ("supervisor_recycle_settled", DriveSession),
        ("supervisor_recycle_settle_ready_after_start", DriveSession),
        ("supervisor_recycle_wait_settled", DriveSession),
        ("closeout_owner_claim", DriveSession),
        ("closeout_owner_release", DriveSession),
        ("record_owner_pane_wedge", DriveSession),
        ("clear_owner_pane_wedge", DriveSession),
        ("resume_claimant", DriveSession),
        ("commit_document", DriveSession),
    ]
};

/// The capability `command` needs; unlisted commands need
/// [`ControllerCapability::Administer`].
pub fn controller_command_capability(command: &str) -> ControllerCapability {
    CONTROLLER_COMMAND_CAPABILITIES
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, capability)| *capability)
        .unwrap_or(ControllerCapability::Administer)
}

/// What the controller knows about the session a request targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTarget {
    /// The principal recorded as the session's owner at claim/start, when a
    /// non-owner took it. `None` means the workspace owner's (or no) session.
    pub recorded_owner: Option<Principal>,
    /// Whether the document already has a session bound (a registry entry).
    pub bound: bool,
}

/// How the session-owner ledger changes when an allowed request succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOwnerUpdate {
    Keep,
    /// A non-owner took the session: record it.
    Record(Principal),
    /// An owner took the session: drop any earlier non-owner record, so the
    /// previous owner cannot drive the new session.
    Clear,
}

/// Why a request from an admitted principal was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRequestRefusal {
    /// The role does not grant the command's capability at all.
    RoleLacksCapability {
        role: Role,
        capability: ControllerCapability,
    },
    /// A session-scoped command that names no document or pane the controller
    /// can resolve.
    NoSessionTarget { capability: ControllerCapability },
    /// The target session belongs to another principal.
    SessionOwnedByOther { owner: Principal },
}

/// The per-request authorization decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRequestDecision {
    Allow(SessionOwnerUpdate),
    Refuse {
        principal: Principal,
        command_capability: ControllerCapability,
        refusal: ControllerRequestRefusal,
    },
}

impl ControllerRequestDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow(_))
    }

    /// Operator-facing refusal text, `None` when allowed.
    pub fn refusal_message(&self, command: &str) -> Option<String> {
        let Self::Refuse {
            principal,
            command_capability,
            refusal,
        } = self
        else {
            return None;
        };
        Some(match refusal {
            ControllerRequestRefusal::RoleLacksCapability { role, capability } => format!(
                "project controller refused `{command}` from {principal}: a workspace {} cannot {} (needs the owner role)",
                role.as_str(),
                capability.as_str()
            ),
            ControllerRequestRefusal::NoSessionTarget { capability } => format!(
                "project controller refused `{command}` from {principal}: {} needs a document or pane the controller can resolve",
                capability.as_str()
            ),
            ControllerRequestRefusal::SessionOwnedByOther { owner } => format!(
                "project controller refused `{command}` ({}) from {principal}: the session belongs to {owner}",
                command_capability.as_str()
            ),
        })
    }
}

/// Authorize one controller request. `owner` is the workspace owner, `role`
/// the admitted principal's role, and `session` the target session when the
/// command is session-scoped and the controller could resolve one.
///
/// Owners may do anything. Other roles get only what [`Role::grants`] allows;
/// an `editor` may claim a session nobody else holds and drive only sessions
/// it owns, so one principal never drives another principal's session, pane,
/// or claimed queue item.
pub fn controller_request_decision(
    owner: Principal,
    principal: Principal,
    role: Role,
    command: &str,
    session: Option<SessionTarget>,
) -> ControllerRequestDecision {
    let capability = controller_command_capability(command);
    let refuse = |refusal| ControllerRequestDecision::Refuse {
        principal,
        command_capability: capability,
        refusal,
    };
    if role == Role::Owner {
        return ControllerRequestDecision::Allow(
            if capability == ControllerCapability::ClaimSession {
                SessionOwnerUpdate::Clear
            } else {
                SessionOwnerUpdate::Keep
            },
        );
    }
    if !role.grants(capability) {
        return refuse(ControllerRequestRefusal::RoleLacksCapability { role, capability });
    }
    if !capability.targets_session() {
        return ControllerRequestDecision::Allow(SessionOwnerUpdate::Keep);
    }
    let Some(session) = session else {
        return refuse(ControllerRequestRefusal::NoSessionTarget { capability });
    };
    match session.recorded_owner {
        Some(recorded) if recorded == principal => {
            ControllerRequestDecision::Allow(SessionOwnerUpdate::Keep)
        }
        Some(recorded) => refuse(ControllerRequestRefusal::SessionOwnedByOther { owner: recorded }),
        None if capability == ControllerCapability::ClaimSession && !session.bound => {
            ControllerRequestDecision::Allow(SessionOwnerUpdate::Record(principal))
        }
        None => refuse(ControllerRequestRefusal::SessionOwnedByOther { owner }),
    }
}

/// The session-owner ledger (`.agent-doc/session-owners.json`): the
/// principal that took each document's session, keyed by the registry key.
/// Only sessions a non-owner took are listed; an absent key is the workspace
/// owner's. A single-user project never writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOwners {
    #[serde(default)]
    pub sessions: std::collections::BTreeMap<String, Principal>,
}

impl SessionOwners {
    pub fn owner_of(&self, key: &str) -> Option<Principal> {
        self.sessions.get(key).copied()
    }

    /// Apply `update` for `key`; returns whether the ledger changed.
    pub fn apply(&mut self, key: &str, update: SessionOwnerUpdate) -> bool {
        match update {
            SessionOwnerUpdate::Keep => false,
            SessionOwnerUpdate::Record(principal) => {
                self.sessions.insert(key.to_string(), principal) != Some(principal)
            }
            SessionOwnerUpdate::Clear => self.sessions.remove(key).is_some(),
        }
    }
}

/// `[receive] auto_create_components` (GH #40): which managed components a
/// sender may create on a recipient's document when it is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AutoCreateComponents {
    /// `true` (and the unset default): any component.
    #[default]
    All,
    /// `false` or `[]`: none; the sender must ask the recipient to add it.
    Nothing,
    /// A list such as `["backlog"]`: only these (canonical names).
    Only(BTreeSet<String>),
}

impl AutoCreateComponents {
    pub fn allows(&self, component: &str) -> bool {
        match self {
            Self::All => true,
            Self::Nothing => false,
            Self::Only(names) => names.contains(&canonical_receive_component(component)),
        }
    }

    /// The policy as written in TOML, for refusal messages.
    pub fn describe(&self) -> String {
        match self {
            Self::All => "true".to_string(),
            Self::Nothing => "false".to_string(),
            Self::Only(names) => format!(
                "[{}]",
                names
                    .iter()
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// Canonical spelling of a component name in a `[receive]` list: lowercase,
/// the legacy `pending` is `backlog`, and the runnable `run`/`loop` openers
/// are the parser's `queue`.
pub fn canonical_receive_component(name: &str) -> String {
    let name = name
        .trim()
        .trim_start_matches("agent:")
        .to_ascii_lowercase();
    match name.as_str() {
        "pending" => "backlog".to_string(),
        "run" | "loop" => "queue".to_string(),
        _ => name,
    }
}

impl Serialize for AutoCreateComponents {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_bool(true),
            Self::Nothing => serializer.serialize_bool(false),
            Self::Only(names) => names.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AutoCreateComponents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PolicyVisitor;

        impl<'de> Visitor<'de> for PolicyVisitor {
            type Value = AutoCreateComponents;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a boolean or a list of component names")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(if value {
                    AutoCreateComponents::All
                } else {
                    AutoCreateComponents::Nothing
                })
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = seq.next_element::<String>()? {
                    let name = canonical_receive_component(&name);
                    if !name.is_empty() {
                        names.insert(name);
                    }
                }
                Ok(if names.is_empty() {
                    AutoCreateComponents::Nothing
                } else {
                    AutoCreateComponents::Only(names)
                })
            }
        }

        deserializer.deserialize_any(PolicyVisitor)
    }
}

/// `[receive]` section of `.agent-doc/config.toml`: what this workspace lets
/// other principals materialize on its documents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_create_components: Option<AutoCreateComponents>,
}

impl ReceiveConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// The effective policy; unset is permissive (single-user default).
    pub fn auto_create_policy(&self) -> AutoCreateComponents {
        self.auto_create_components.clone().unwrap_or_default()
    }
}

/// The recipient-side auto-create decision for one missing component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoCreateDecision {
    Allow,
    Refuse {
        sender: Principal,
        component: String,
        policy: AutoCreateComponents,
    },
}

impl AutoCreateDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// May `sender` auto-create the missing `component` on a document in the
/// recipient workspace owned by `recipient_owner`? The recipient's owners are
/// never limited by their own `[receive]` policy; every other sender is.
/// Unset policy is permissive. A sender-side opt-out (`--no-create-backlog`)
/// applies before this and is never overridden by it.
pub fn component_auto_create_decision(
    recipient_owner: Principal,
    workspace: &WorkspaceConfig,
    receive: &ReceiveConfig,
    sender: Principal,
    component: &str,
) -> AutoCreateDecision {
    if role_of(recipient_owner, workspace, sender) == Some(Role::Owner) {
        return AutoCreateDecision::Allow;
    }
    let policy = receive.auto_create_policy();
    if policy.allows(component) {
        AutoCreateDecision::Allow
    } else {
        AutoCreateDecision::Refuse {
            sender,
            component: canonical_receive_component(component),
            policy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: Principal = Principal::os_user(1000);
    const ALICE: Principal = Principal::os_user(1001);
    const BOB: Principal = Principal::os_user(1002);
    const MALLORY: Principal = Principal::os_user(1666);

    fn multi_user() -> WorkspaceConfig {
        WorkspaceConfig {
            mode: WorkspaceMode::MultiUser,
            members: vec![
                WorkspaceMember {
                    uid: ALICE.uid,
                    name: Some("alice".into()),
                    role: Role::Editor,
                },
                WorkspaceMember {
                    uid: BOB.uid,
                    name: None,
                    role: Role::Owner,
                },
            ],
        }
    }

    #[test]
    fn single_user_default_admits_only_the_owner() {
        let workspace = WorkspaceConfig::default();
        assert!(controller_peer_decision(OWNER, &workspace, OWNER).is_admitted());
        let refused = controller_peer_decision(OWNER, &workspace, ALICE);
        assert_eq!(
            refused,
            ControllerPeerDecision::Refuse {
                principal: ALICE,
                refusal: ControllerPeerRefusal::NotOwner,
            }
        );
        // The pre-existing refusal text stays recognizable.
        assert!(
            refused
                .refusal_message()
                .unwrap()
                .contains("does not belong to the current user")
        );
    }

    #[test]
    fn single_user_ignores_listed_members() {
        let mut workspace = multi_user();
        workspace.mode = WorkspaceMode::SingleUser;
        assert_eq!(role_of(OWNER, &workspace, BOB), None);
        assert!(!controller_peer_decision(OWNER, &workspace, BOB).is_admitted());
    }

    #[test]
    fn multi_user_admits_members_with_their_role_and_refuses_strangers() {
        let workspace = multi_user();
        assert_eq!(
            controller_peer_decision(OWNER, &workspace, BOB),
            ControllerPeerDecision::Admit {
                principal: BOB,
                role: Role::Owner,
            }
        );
        assert_eq!(
            controller_peer_decision(OWNER, &workspace, ALICE),
            ControllerPeerDecision::Admit {
                principal: ALICE,
                role: Role::Editor,
            }
        );
        assert_eq!(
            controller_peer_decision(OWNER, &workspace, MALLORY),
            ControllerPeerDecision::Refuse {
                principal: MALLORY,
                refusal: ControllerPeerRefusal::NotMember,
            }
        );
    }

    #[test]
    fn workspace_and_receive_sections_parse_from_toml() {
        #[derive(Deserialize)]
        struct Sections {
            #[serde(default)]
            workspace: WorkspaceConfig,
            #[serde(default)]
            receive: ReceiveConfig,
        }
        let parsed: Sections = toml::from_str(
            r#"
[workspace]
mode = "multi-user"
[[workspace.members]]
uid = 1001
name = "alice"
role = "editor"

[receive]
auto_create_components = ["pending", "Run"]
"#,
        )
        .unwrap();
        assert_eq!(parsed.workspace.mode, WorkspaceMode::MultiUser);
        assert_eq!(parsed.workspace.members[0].role, Role::Editor);
        assert_eq!(
            parsed.receive.auto_create_components,
            Some(AutoCreateComponents::Only(BTreeSet::from([
                "backlog".to_string(),
                "queue".to_string()
            ])))
        );

        for (raw, expected) in [
            ("true", AutoCreateComponents::All),
            ("false", AutoCreateComponents::Nothing),
            ("[]", AutoCreateComponents::Nothing),
        ] {
            let parsed: Sections =
                toml::from_str(&format!("[receive]\nauto_create_components = {raw}\n")).unwrap();
            assert_eq!(parsed.receive.auto_create_policy(), expected, "{raw}");
        }
        let unset: Sections = toml::from_str("").unwrap();
        assert_eq!(unset.workspace.mode, WorkspaceMode::SingleUser);
        assert_eq!(
            unset.receive.auto_create_policy(),
            AutoCreateComponents::All
        );
        assert!(toml::from_str::<Sections>("[receive]\nauto_create_components = 3\n").is_err());
    }

    #[test]
    fn auto_create_policy_round_trips_through_toml() {
        for policy in [
            AutoCreateComponents::All,
            AutoCreateComponents::Nothing,
            AutoCreateComponents::Only(BTreeSet::from(["backlog".to_string()])),
        ] {
            let receive = ReceiveConfig {
                auto_create_components: Some(policy.clone()),
            };
            let text = toml::to_string(&receive).unwrap();
            let back: ReceiveConfig = toml::from_str(&text).unwrap();
            assert_eq!(back.auto_create_policy(), policy, "{text}");
        }
    }

    // ---- AccessWorld: deterministic multi-principal scenarios -------------
    //
    // SimWorld-style model: two workspaces with distinct owners, several
    // principals, and the ordered operations a real session performs. Every
    // outcome comes from the same pure rules production calls; nothing is
    // mocked.

    struct Workspace {
        owner: Principal,
        config: WorkspaceConfig,
        receive: ReceiveConfig,
        components: BTreeSet<String>,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Created,
        AlreadyPresent,
        Refused,
    }

    impl Workspace {
        fn new(owner: Principal, receive: ReceiveConfig) -> Self {
            Self {
                owner,
                config: WorkspaceConfig::default(),
                receive,
                components: BTreeSet::from(["exchange".to_string()]),
            }
        }

        fn ensure_component(&mut self, sender: Principal, component: &str) -> Outcome {
            let component = canonical_receive_component(component);
            if self.components.contains(&component) {
                return Outcome::AlreadyPresent;
            }
            match component_auto_create_decision(
                self.owner,
                &self.config,
                &self.receive,
                sender,
                &component,
            ) {
                AutoCreateDecision::Allow => {
                    self.components.insert(component);
                    Outcome::Created
                }
                AutoCreateDecision::Refuse { .. } => Outcome::Refused,
            }
        }
    }

    fn receive(policy: Option<AutoCreateComponents>) -> ReceiveConfig {
        ReceiveConfig {
            auto_create_components: policy,
        }
    }

    #[test]
    fn access_world_unset_policy_keeps_single_author_auto_create() {
        let mut recipient = Workspace::new(OWNER, receive(None));
        assert_eq!(
            recipient.ensure_component(ALICE, "pending"),
            Outcome::Created
        );
        assert_eq!(
            recipient.ensure_component(ALICE, "backlog"),
            Outcome::AlreadyPresent
        );
        assert_eq!(
            recipient.ensure_component(MALLORY, "review"),
            Outcome::Created
        );
    }

    #[test]
    fn access_world_backlog_only_policy_limits_senders_not_the_recipient() {
        let only_backlog = AutoCreateComponents::Only(BTreeSet::from(["backlog".to_string()]));
        let mut recipient = Workspace::new(OWNER, receive(Some(only_backlog)));
        assert_eq!(
            recipient.ensure_component(ALICE, "review"),
            Outcome::Refused
        );
        assert_eq!(
            recipient.ensure_component(ALICE, "pending"),
            Outcome::Created
        );
        // The recipient is never limited by its own policy.
        assert_eq!(
            recipient.ensure_component(OWNER, "review"),
            Outcome::Created
        );
        assert_eq!(
            recipient.ensure_component(ALICE, "review"),
            Outcome::AlreadyPresent
        );
    }

    #[test]
    fn access_world_strict_policy_refuses_every_other_sender_until_recipient_adds_it() {
        let mut recipient = Workspace::new(OWNER, receive(Some(AutoCreateComponents::Nothing)));
        assert_eq!(
            recipient.ensure_component(ALICE, "backlog"),
            Outcome::Refused
        );
        assert_eq!(
            recipient.ensure_component(MALLORY, "backlog"),
            Outcome::Refused
        );
        assert!(!recipient.components.contains("backlog"));
        assert_eq!(
            recipient.ensure_component(OWNER, "backlog"),
            Outcome::Created
        );
        assert_eq!(
            recipient.ensure_component(ALICE, "backlog"),
            Outcome::AlreadyPresent
        );
    }

    #[test]
    fn access_world_co_owner_members_bypass_policy_only_in_multi_user_mode() {
        let mut recipient = Workspace::new(OWNER, receive(Some(AutoCreateComponents::Nothing)));
        recipient.config = multi_user();
        recipient.config.mode = WorkspaceMode::SingleUser;
        assert_eq!(recipient.ensure_component(BOB, "backlog"), Outcome::Refused);
        recipient.config.mode = WorkspaceMode::MultiUser;
        assert_eq!(
            recipient.ensure_component(ALICE, "backlog"),
            Outcome::Refused
        );
        assert_eq!(recipient.ensure_component(BOB, "backlog"), Outcome::Created);
    }

    #[test]
    fn access_world_two_workspaces_apply_their_own_recipient_policy() {
        let alice_ws = Workspace::new(ALICE, receive(Some(AutoCreateComponents::Nothing)));
        let owner_ws = Workspace::new(OWNER, receive(None));
        let mut worlds = [alice_ws, owner_ws];
        // OWNER sending into Alice's strict workspace is refused; Alice sending
        // into OWNER's permissive workspace is allowed.
        assert_eq!(
            worlds[0].ensure_component(OWNER, "backlog"),
            Outcome::Refused
        );
        assert_eq!(
            worlds[1].ensure_component(ALICE, "backlog"),
            Outcome::Created
        );
    }

    #[test]
    fn refusal_carries_canonical_component_and_policy() {
        let decision = component_auto_create_decision(
            OWNER,
            &WorkspaceConfig::default(),
            &receive(Some(AutoCreateComponents::Only(BTreeSet::from([
                "backlog".to_string(),
            ])))),
            ALICE,
            "agent:Review",
        );
        assert_eq!(
            decision,
            AutoCreateDecision::Refuse {
                sender: ALICE,
                component: "review".to_string(),
                policy: AutoCreateComponents::Only(BTreeSet::from(["backlog".to_string()])),
            }
        );
        let AutoCreateDecision::Refuse { policy, .. } = decision else {
            unreachable!()
        };
        assert_eq!(policy.describe(), "[\"backlog\"]");
    }

    // --- Phase 2: per-request authorization and session ownership. ---

    const CAROL: Principal = Principal::os_user(1003);

    fn team() -> WorkspaceConfig {
        let mut workspace = multi_user();
        workspace.members.push(WorkspaceMember {
            uid: CAROL.uid,
            name: Some("carol".into()),
            role: Role::Viewer,
        });
        workspace.members.push(WorkspaceMember {
            uid: MALLORY.uid,
            name: Some("mallory".into()),
            role: Role::Editor,
        });
        workspace
    }

    #[test]
    fn unlisted_commands_need_the_owner_role() {
        assert_eq!(
            controller_command_capability("shutdown"),
            ControllerCapability::Administer
        );
        assert_eq!(
            controller_command_capability("a_command_added_tomorrow"),
            ControllerCapability::Administer
        );
        for role in [Role::Editor, Role::Viewer] {
            assert!(
                !controller_request_decision(OWNER, ALICE, role, "a_command_added_tomorrow", None)
                    .is_allowed()
            );
        }
        assert!(
            controller_request_decision(OWNER, BOB, Role::Owner, "a_command_added_tomorrow", None)
                .is_allowed()
        );
    }

    #[test]
    fn command_table_lists_each_command_once() {
        let mut seen = BTreeSet::new();
        for (name, _) in CONTROLLER_COMMAND_CAPABILITIES {
            assert!(seen.insert(*name), "{name} listed twice");
        }
    }

    #[test]
    fn roles_grant_capabilities_in_order() {
        use ControllerCapability as C;
        let all = [
            C::Observe,
            C::WriteDocument,
            C::ClaimSession,
            C::DriveSession,
            C::Administer,
        ];
        assert!(all.iter().all(|cap| Role::Owner.grants(*cap)));
        assert_eq!(
            all.iter().filter(|cap| Role::Editor.grants(**cap)).count(),
            4
        );
        assert!(!Role::Editor.grants(C::Administer));
        assert_eq!(
            all.iter()
                .filter(|cap| Role::Viewer.grants(**cap))
                .copied()
                .collect::<Vec<_>>(),
            vec![C::Observe]
        );
    }

    #[test]
    fn viewer_reads_but_never_writes_or_drives() {
        let unbound = Some(SessionTarget {
            recorded_owner: None,
            bound: false,
        });
        assert!(
            controller_request_decision(OWNER, CAROL, Role::Viewer, "status", None).is_allowed()
        );
        for command in ["crdt_cp_write", "start_session", "dispatch", "shutdown"] {
            let decision =
                controller_request_decision(OWNER, CAROL, Role::Viewer, command, unbound);
            assert!(
                matches!(
                    decision,
                    ControllerRequestDecision::Refuse {
                        refusal: ControllerRequestRefusal::RoleLacksCapability {
                            role: Role::Viewer,
                            ..
                        },
                        ..
                    }
                ),
                "{command}: {decision:?}"
            );
            assert!(
                decision
                    .refusal_message(command)
                    .unwrap()
                    .contains("a workspace viewer cannot")
            );
        }
    }

    #[test]
    fn session_scoped_commands_without_a_target_are_refused_for_non_owners() {
        assert_eq!(
            controller_request_decision(OWNER, ALICE, Role::Editor, "dispatch", None),
            ControllerRequestDecision::Refuse {
                principal: ALICE,
                command_capability: ControllerCapability::DriveSession,
                refusal: ControllerRequestRefusal::NoSessionTarget {
                    capability: ControllerCapability::DriveSession,
                },
            }
        );
        // The owner needs no target.
        assert_eq!(
            controller_request_decision(OWNER, OWNER, Role::Owner, "dispatch", None),
            ControllerRequestDecision::Allow(SessionOwnerUpdate::Keep)
        );
    }

    /// SessionWorld: several principals claim, drive, and retake sessions on
    /// one workspace; the ledger is the only shared state, exactly as the
    /// controller keeps it.
    struct SessionWorld {
        owner: Principal,
        workspace: WorkspaceConfig,
        ledger: SessionOwners,
        bound: BTreeSet<String>,
    }

    impl SessionWorld {
        fn new() -> Self {
            Self {
                owner: OWNER,
                workspace: team(),
                ledger: SessionOwners::default(),
                bound: BTreeSet::new(),
            }
        }

        fn request(&mut self, principal: Principal, command: &str, doc: &str) -> bool {
            let role = role_of(self.owner, &self.workspace, principal).expect("admitted member");
            let session = Some(SessionTarget {
                recorded_owner: self.ledger.owner_of(doc),
                bound: self.bound.contains(doc),
            });
            match controller_request_decision(self.owner, principal, role, command, session) {
                ControllerRequestDecision::Allow(update) => {
                    self.ledger.apply(doc, update);
                    if controller_command_capability(command) == ControllerCapability::ClaimSession
                    {
                        self.bound.insert(doc.to_string());
                    }
                    true
                }
                ControllerRequestDecision::Refuse { .. } => false,
            }
        }
    }

    #[test]
    fn session_world_one_principal_never_drives_another_principals_session() {
        let mut world = SessionWorld::new();
        // The owner runs its own document; editors cannot touch it.
        assert!(world.request(OWNER, "start_session", "tasks/owner.md"));
        assert!(!world.request(ALICE, "dispatch", "tasks/owner.md"));
        assert!(!world.request(ALICE, "start_session", "tasks/owner.md"));
        assert!(!world.request(ALICE, "queue_control", "tasks/owner.md"));

        // Alice claims a free document and drives it.
        assert!(world.request(ALICE, "session_owner_claim", "tasks/alice.md"));
        assert_eq!(world.ledger.owner_of("tasks/alice.md"), Some(ALICE));
        assert!(world.request(ALICE, "start_session", "tasks/alice.md"));
        assert!(world.request(ALICE, "dispatch", "tasks/alice.md"));
        assert!(world.request(ALICE, "queue_control", "tasks/alice.md"));

        // Mallory, another editor, can neither drive nor take Alice's session.
        assert!(!world.request(MALLORY, "dispatch", "tasks/alice.md"));
        assert!(!world.request(MALLORY, "focus_document_pane", "tasks/alice.md"));
        assert!(!world.request(MALLORY, "attach_pane", "tasks/alice.md"));
        // Carol only watches.
        assert!(world.request(CAROL, "session_status", "tasks/alice.md"));
        assert!(!world.request(CAROL, "dispatch", "tasks/alice.md"));

        // The owner can always drive, and retaking the session clears Alice's
        // record so she cannot drive the owner's new session.
        assert!(world.request(OWNER, "dispatch", "tasks/alice.md"));
        assert!(world.request(OWNER, "start_session", "tasks/alice.md"));
        assert_eq!(world.ledger.owner_of("tasks/alice.md"), None);
        assert!(!world.request(ALICE, "dispatch", "tasks/alice.md"));

        // A co-owner member (Bob) acts as the owner and records nothing.
        assert!(world.request(BOB, "dispatch", "tasks/alice.md"));
        assert!(world.ledger.sessions.is_empty());
    }

    #[test]
    fn session_owner_ledger_reports_whether_it_changed() {
        let mut ledger = SessionOwners::default();
        assert!(!ledger.apply("tasks/a.md", SessionOwnerUpdate::Keep));
        assert!(ledger.apply("tasks/a.md", SessionOwnerUpdate::Record(ALICE)));
        assert!(!ledger.apply("tasks/a.md", SessionOwnerUpdate::Record(ALICE)));
        assert!(ledger.apply("tasks/a.md", SessionOwnerUpdate::Record(MALLORY)));
        assert_eq!(ledger.owner_of("tasks/a.md"), Some(MALLORY));
        assert!(ledger.apply("tasks/a.md", SessionOwnerUpdate::Clear));
        assert!(!ledger.apply("tasks/a.md", SessionOwnerUpdate::Clear));
    }
}
