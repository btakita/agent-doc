//! Project-controller socket admission through the workspace access model
//! (`#ghrecvpolicy` phase 1, `specs/proposals/multi-user-access-control.md`).
//!
//! The kernel reports each connecting peer's effective user (`SO_PEERCRED` on
//! Linux). [`controller_peer_decision`] decides from that principal, the
//! controller's own user (the workspace owner), and the project's
//! `[workspace]` section. The default single-user workspace admits only the
//! owner, which is exactly the rule this replaced.
//!
//! The owner, the overwhelmingly common peer, is admitted without touching the
//! filesystem. Any other peer reads `[workspace]` through a cache keyed on the
//! config file's modification time and length, so a membership edit takes
//! effect on the next connection without a controller restart. A config that
//! cannot be read or parsed falls back to single-user, the most restrictive
//! mode.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use agent_doc_frontmatter::workspace_access::{
    ControllerPeerDecision, Principal, Role, WorkspaceConfig, controller_peer_decision,
};
use parking_lot::Mutex;

type ConfigStamp = (Option<SystemTime>, u64);

pub(crate) struct ControllerWorkspaceAccess {
    config_path: PathBuf,
    cache: Mutex<Option<(ConfigStamp, WorkspaceConfig)>>,
}

impl ControllerWorkspaceAccess {
    pub(crate) fn new(project_root: &Path) -> Self {
        Self {
            config_path: agent_doc_project_config_io::project_config_path_for_root(project_root),
            cache: Mutex::new(None),
        }
    }

    /// Admission decision for a socket peer. `owner` is the controller's own
    /// effective user.
    pub(crate) fn decide(&self, owner: Principal, peer: Principal) -> ControllerPeerDecision {
        if peer == owner {
            return ControllerPeerDecision::Admit {
                principal: peer,
                role: Role::Owner,
            };
        }
        controller_peer_decision(owner, &self.workspace(), peer)
    }

    fn workspace(&self) -> WorkspaceConfig {
        let Ok(metadata) = std::fs::metadata(&self.config_path) else {
            return WorkspaceConfig::default();
        };
        let stamp = (metadata.modified().ok(), metadata.len());
        let mut cache = self.cache.lock();
        if let Some((cached_stamp, workspace)) = cache.as_ref()
            && *cached_stamp == stamp
        {
            return workspace.clone();
        }
        let workspace = std::fs::read_to_string(&self.config_path)
            .ok()
            .and_then(|content| {
                agent_doc_frontmatter::project_config::parse_project_toml(&content).ok()
            })
            .map(|config| config.workspace)
            .unwrap_or_default();
        *cache = Some((stamp, workspace.clone()));
        workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_frontmatter::workspace_access::ControllerPeerRefusal;

    const OWNER: Principal = Principal::os_user(1000);
    const ALICE: Principal = Principal::os_user(1001);
    const BOB: Principal = Principal::os_user(1002);

    fn project(config: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        if let Some(config) = config {
            std::fs::write(dir.path().join(".agent-doc/config.toml"), config).unwrap();
        }
        dir
    }

    #[test]
    fn single_user_default_admits_the_owner_and_refuses_everyone_else() {
        let dir = project(None);
        let access = ControllerWorkspaceAccess::new(dir.path());
        assert!(access.decide(OWNER, OWNER).is_admitted());
        assert_eq!(
            access.decide(OWNER, ALICE),
            ControllerPeerDecision::Refuse {
                principal: ALICE,
                refusal: ControllerPeerRefusal::NotOwner,
            }
        );
    }

    #[test]
    fn membership_edits_take_effect_without_a_restart() {
        let dir = project(Some("[workspace]\nmode = \"single-user\"\n"));
        let access = ControllerWorkspaceAccess::new(dir.path());
        assert!(!access.decide(OWNER, BOB).is_admitted());

        // A longer file guarantees a new stamp even on coarse-mtime filesystems.
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[workspace]\nmode = \"multi-user\"\n\n[[workspace.members]]\nuid = 1002\nrole = \"owner\"\n\n[[workspace.members]]\nuid = 1001\nrole = \"editor\"\n",
        )
        .unwrap();
        assert!(access.decide(OWNER, BOB).is_admitted());
        assert_eq!(
            access.decide(OWNER, ALICE),
            ControllerPeerDecision::Refuse {
                principal: ALICE,
                refusal: ControllerPeerRefusal::MemberAccessNotEnabled { role: Role::Editor },
            }
        );
    }

    #[test]
    fn unparseable_config_falls_back_to_single_user() {
        let dir = project(Some(
            "[workspace]\nmode = \"multi-user\"\n[[workspace.members]]\nuid = 1002\nrole = \"owner\"\nmode = \n",
        ));
        let access = ControllerWorkspaceAccess::new(dir.path());
        assert_eq!(
            access.decide(OWNER, BOB),
            ControllerPeerDecision::Refuse {
                principal: BOB,
                refusal: ControllerPeerRefusal::NotOwner,
            }
        );
    }
}
