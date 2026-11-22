//! File-backed adapter for the multi-user workspace access model
//! (`#ghrecvpolicy`). The rules live in
//! [`agent_doc_frontmatter::workspace_access`]; this module resolves the
//! current process principal, a recipient document's workspace owner and
//! policy, and turns refusals into errors.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use agent_doc_frontmatter::workspace_access::{
    AutoCreateDecision, Principal, ReceiveConfig, WorkspaceConfig, component_auto_create_decision,
};

/// The principal this process acts as: its effective OS user.
pub fn current_principal() -> Principal {
    #[cfg(unix)]
    {
        // SAFETY: geteuid only reads this process's effective user id.
        Principal::os_user(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        // No uid model: every local process is the single owner.
        Principal::os_user(0)
    }
}

/// The OS user that owns `path`, when the platform reports one.
fn path_owner(path: &Path) -> Option<Principal> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path)
            .ok()
            .map(|metadata| Principal::os_user(metadata.uid()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// The workspace a document belongs to, seen from a sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientWorkspace {
    /// The project store that owns the document, when one resolves.
    pub root: Option<PathBuf>,
    /// The workspace owner: the OS user owning the store's `.agent-doc`
    /// directory, else the document, else its parent directory.
    pub owner: Principal,
    pub workspace: WorkspaceConfig,
    pub receive: ReceiveConfig,
}

/// Resolve the recipient workspace for `file` (which may not exist yet).
pub fn recipient_workspace_for_doc(file: &Path) -> RecipientWorkspace {
    let (root, owner) = recipient_owner_for_doc(file);
    let config = root
        .as_deref()
        .map(crate::load_project_for_root)
        .unwrap_or_default();
    RecipientWorkspace {
        root,
        owner,
        workspace: config.workspace,
        receive: config.receive,
    }
}

/// The project store that owns `file` and that workspace's owner, without
/// reading its config.
fn recipient_owner_for_doc(file: &Path) -> (Option<PathBuf>, Principal) {
    let root = crate::project_store_for_doc(file);
    let owner = root
        .as_deref()
        .and_then(|root| path_owner(&root.join(".agent-doc")))
        .or_else(|| path_owner(file))
        .or_else(|| file.parent().and_then(path_owner))
        .unwrap_or_else(current_principal);
    (root, owner)
}

/// Refuse when `sender` may not auto-create the missing `component` on
/// `target` under the recipient workspace's `[receive] auto_create_components`
/// policy (GH #40). The recipient's owners are never limited by it. A sender
/// who owns the recipient workspace skips the config read entirely, so a
/// single-user project pays nothing.
pub fn enforce_component_auto_create(
    sender: Principal,
    target: &Path,
    component: &str,
) -> Result<()> {
    let (_, owner) = recipient_owner_for_doc(target);
    if sender == owner {
        return Ok(());
    }
    let recipient = recipient_workspace_for_doc(target);
    enforce_component_auto_create_in(&recipient, sender, target, component)
}

/// [`enforce_component_auto_create`] against an already-resolved recipient.
pub fn enforce_component_auto_create_in(
    recipient: &RecipientWorkspace,
    sender: Principal,
    target: &Path,
    component: &str,
) -> Result<()> {
    match component_auto_create_decision(
        recipient.owner,
        &recipient.workspace,
        &recipient.receive,
        sender,
        component,
    ) {
        AutoCreateDecision::Allow => Ok(()),
        AutoCreateDecision::Refuse {
            sender,
            component,
            policy,
        } => bail!(
            "{} has no agent:{component} component and its workspace does not let {sender} create one ([receive] auto_create_components = {} in {}); ask the document's owner ({}) to add the component",
            target.display(),
            policy.describe(),
            recipient
                .root
                .as_deref()
                .map(|root| crate::project_config_path_for_root(root)
                    .display()
                    .to_string())
                .unwrap_or_else(|| "its project config".to_string()),
            recipient.owner,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_frontmatter::workspace_access::{AutoCreateComponents, Role, WorkspaceMode};

    fn workspace(config: &str) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/config.toml"), config).unwrap();
        dir
    }

    #[test]
    fn recipient_workspace_reads_owner_and_policy_from_the_store() {
        let dir = workspace(
            "[workspace]\nmode = \"multi-user\"\n[[workspace.members]]\nuid = 4242\nrole = \"owner\"\n\n[receive]\nauto_create_components = [\"backlog\"]\n",
        );
        let doc = dir.path().join("notes/doc.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Doc\n").unwrap();

        let recipient = recipient_workspace_for_doc(&doc);
        assert_eq!(recipient.owner, current_principal());
        assert_eq!(recipient.workspace.mode, WorkspaceMode::MultiUser);
        assert_eq!(recipient.workspace.members[0].role, Role::Owner);
        assert!(matches!(
            recipient.receive.auto_create_policy(),
            AutoCreateComponents::Only(_)
        ));
    }

    #[test]
    fn owner_sender_is_never_limited_by_its_own_policy() {
        let dir = workspace("[receive]\nauto_create_components = false\n");
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n").unwrap();
        enforce_component_auto_create(current_principal(), &doc, "backlog").unwrap();
    }

    #[test]
    fn distinct_sender_is_refused_by_a_strict_recipient_and_named_in_the_error() {
        let dir = workspace("[receive]\nauto_create_components = false\n");
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n").unwrap();
        let sender = Principal::os_user(current_principal().uid.wrapping_add(7919));
        let err = enforce_component_auto_create(sender, &doc, "pending")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no agent:backlog component"), "{err}");
        assert!(err.contains(&sender.to_string()), "{err}");
        assert!(err.contains("auto_create_components = false"), "{err}");
    }

    #[test]
    fn distinct_sender_is_allowed_by_an_unset_policy() {
        let dir = workspace("");
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n").unwrap();
        let sender = Principal::os_user(current_principal().uid.wrapping_add(7919));
        enforce_component_auto_create(sender, &doc, "review").unwrap();
    }
}
