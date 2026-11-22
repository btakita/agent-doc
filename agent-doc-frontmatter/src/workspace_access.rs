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
    /// A listed non-owner member. Their connection needs per-request
    /// authorization (phase 2 of the design), which has not landed, so the
    /// controller still refuses them rather than granting owner powers.
    MemberAccessNotEnabled { role: Role },
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
            ControllerPeerRefusal::MemberAccessNotEnabled { role } => format!(
                "project controller client {principal} is a workspace {} but controller access for non-owner members is not enabled yet (needs per-request authorization)",
                role.as_str()
            ),
        })
    }
}

/// Admission rule for a peer on the project controller socket. `owner` is the
/// controller's own effective user. Owners (the controller's user and any
/// member with [`Role::Owner`]) are admitted; everyone else is refused.
pub fn controller_peer_decision(
    owner: Principal,
    workspace: &WorkspaceConfig,
    peer: Principal,
) -> ControllerPeerDecision {
    match role_of(owner, workspace, peer) {
        Some(Role::Owner) => ControllerPeerDecision::Admit {
            principal: peer,
            role: Role::Owner,
        },
        Some(role) => ControllerPeerDecision::Refuse {
            principal: peer,
            refusal: ControllerPeerRefusal::MemberAccessNotEnabled { role },
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
    fn multi_user_admits_owners_and_refuses_other_members_until_per_request_authz() {
        let workspace = multi_user();
        assert!(controller_peer_decision(OWNER, &workspace, BOB).is_admitted());
        assert_eq!(
            controller_peer_decision(OWNER, &workspace, ALICE),
            ControllerPeerDecision::Refuse {
                principal: ALICE,
                refusal: ControllerPeerRefusal::MemberAccessNotEnabled { role: Role::Editor },
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
}
