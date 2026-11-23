//! File-backed adapter for the multi-user workspace access model
//! (`#ghrecvpolicy`). The rules live in
//! [`agent_doc_frontmatter::workspace_access`]; this module resolves the
//! current process principal, a recipient document's workspace owner and
//! policy, and turns refusals into errors.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use agent_doc_frontmatter::workspace_access::{
    AutoCreateDecision, Principal, ReceiveConfig, SessionOwnerUpdate, SessionOwners,
    WorkspaceConfig, component_auto_create_decision,
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

thread_local! {
    /// The principal a project-controller request thread acts for
    /// (`#ghrecvpolicy` phase 2). The controller runs as the workspace owner,
    /// so without this a non-owner's request would create components as the
    /// owner. `None` outside a served request.
    static SENDER_PRINCIPAL_OVERRIDE: std::cell::Cell<Option<Principal>> =
        const { std::cell::Cell::new(None) };
}

/// Restores the previous sender when a served request leaves its scope.
pub struct SenderPrincipalScope(Option<Principal>);

impl Drop for SenderPrincipalScope {
    fn drop(&mut self) {
        SENDER_PRINCIPAL_OVERRIDE.set(self.0);
    }
}

/// Act as `principal` on this thread until the scope drops. The project
/// controller enters it for each request a non-owner sends.
pub fn enter_sender_principal(principal: Option<Principal>) -> SenderPrincipalScope {
    SenderPrincipalScope(SENDER_PRINCIPAL_OVERRIDE.replace(principal))
}

/// The principal that a component creation on this thread is attributed to:
/// the controller request's principal when one is in scope, else this
/// process's effective user.
pub fn sender_principal() -> Principal {
    SENDER_PRINCIPAL_OVERRIDE
        .get()
        .unwrap_or_else(current_principal)
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

/// Refuse a write of `after` over `before` on `target` when it creates a
/// managed component that the recipient's `[receive]` policy does not let
/// `sender` create (GH #40). This covers every scaffold the binary adds
/// while maintaining a document (`agent:run` from route dispatch,
/// `agent:review`, `agent:done`, `agent:todo`, `agent:recap`, ...).
///
/// A write that creates no component returns before any file I/O, and a
/// sender who owns the recipient workspace never reads its config, so the
/// single-user default pays only a marker scan.
pub fn enforce_created_components(
    sender: Principal,
    target: &Path,
    before: &str,
    after: &str,
) -> Result<()> {
    let created = agent_doc_element::created_components::created_component_names(before, after);
    if created.is_empty() {
        return Ok(());
    }
    let (_, owner) = recipient_owner_for_doc(target);
    if sender == owner {
        return Ok(());
    }
    let recipient = recipient_workspace_for_doc(target);
    for component in &created {
        enforce_component_auto_create_in(&recipient, sender, target, component)?;
    }
    Ok(())
}

/// [`enforce_created_components`] for this thread's [`sender_principal`]:
/// the write-boundary guard the binary's document writers call.
pub fn enforce_sender_created_components(target: &Path, before: &str, after: &str) -> Result<()> {
    enforce_created_components(sender_principal(), target, before, after)
}

/// The session-owner ledger for a project store.
pub fn session_owners_path(store_root: &Path) -> PathBuf {
    store_root.join(".agent-doc").join("session-owners.toml")
}

/// Load the session-owner ledger; a missing or unreadable ledger is empty
/// (every session is the workspace owner's).
pub fn load_session_owners(store_root: &Path) -> SessionOwners {
    std::fs::read_to_string(session_owners_path(store_root))
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

/// Apply `update` to `key` in the store's ledger and persist it. Returns
/// whether the ledger changed. A `Clear` on a store with no ledger file does
/// no I/O beyond one existence check, so owners never create the file.
pub fn update_session_owner(
    store_root: &Path,
    key: &str,
    update: SessionOwnerUpdate,
) -> Result<bool> {
    let path = session_owners_path(store_root);
    if matches!(update, SessionOwnerUpdate::Keep)
        || (matches!(update, SessionOwnerUpdate::Clear) && !path.exists())
    {
        return Ok(false);
    }
    let mut owners = load_session_owners(store_root);
    if !owners.apply(key, update) {
        return Ok(false);
    }
    let rendered = toml::to_string(&owners)?;
    let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    std::fs::write(&tmp, rendered)
        .map_err(|err| anyhow::anyhow!("failed to write {}: {err}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .map_err(|err| anyhow::anyhow!("failed to replace {}: {err}", path.display()))?;
    Ok(true)
}

/// The workspace owner of a project store: the OS user owning its
/// `.agent-doc` directory.
pub fn store_owner(store_root: &Path) -> Option<Principal> {
    path_owner(&store_root.join(".agent-doc"))
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
    fn created_component_guard_skips_io_when_nothing_is_created() {
        // No store at all: a write that creates nothing never resolves one.
        let doc = std::path::Path::new("/nonexistent/agent-doc/doc.md");
        let sender = Principal::os_user(current_principal().uid.wrapping_add(7919));
        enforce_created_components(sender, doc, "a\n", "a b\n").unwrap();
    }

    #[test]
    fn created_component_guard_applies_the_recipient_policy_to_each_new_component() {
        let dir = workspace("[receive]\nauto_create_components = [\"backlog\"]\n");
        let doc = dir.path().join("doc.md");
        let before = "# Doc\n";
        std::fs::write(&doc, before).unwrap();
        let sender = Principal::os_user(current_principal().uid.wrapping_add(7919));
        let with_backlog = "# Doc\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
        enforce_created_components(sender, &doc, before, with_backlog).unwrap();
        let with_run = "# Doc\n\n<!-- agent:run go -->\n- go\n<!-- /agent:run -->\n";
        let err = enforce_created_components(sender, &doc, before, with_run)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no agent:queue component"), "{err}");
        // The owner is never limited.
        enforce_created_components(current_principal(), &doc, before, with_run).unwrap();
    }

    #[test]
    fn sender_principal_follows_the_request_scope() {
        let alice = Principal::os_user(current_principal().uid.wrapping_add(1));
        assert_eq!(sender_principal(), current_principal());
        {
            let _scope = enter_sender_principal(Some(alice));
            assert_eq!(sender_principal(), alice);
            {
                let _inner = enter_sender_principal(None);
                assert_eq!(sender_principal(), current_principal());
            }
            assert_eq!(sender_principal(), alice);
        }
        assert_eq!(sender_principal(), current_principal());
    }

    #[test]
    fn session_owner_ledger_is_never_created_by_an_owner_clear() {
        let dir = workspace("");
        let path = session_owners_path(dir.path());
        assert!(!update_session_owner(dir.path(), "a.md", SessionOwnerUpdate::Clear).unwrap());
        assert!(!path.exists());
        let alice = Principal::os_user(1001);
        assert!(
            update_session_owner(dir.path(), "a.md", SessionOwnerUpdate::Record(alice)).unwrap()
        );
        assert_eq!(
            load_session_owners(dir.path()).owner_of("a.md"),
            Some(alice)
        );
        assert!(update_session_owner(dir.path(), "a.md", SessionOwnerUpdate::Clear).unwrap());
        assert_eq!(load_session_owners(dir.path()).owner_of("a.md"), None);
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
