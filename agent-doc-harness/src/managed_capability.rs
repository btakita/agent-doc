//! Managed capability contract policy for harness launches.

use agent_doc_config::Config;
use agent_doc_frontmatter::frontmatter::{CodexNetworkAccess, Frontmatter};
use agent_doc_turn_executor::codex_launch::{add_dirs_from_args, resolve_codex_network_access};

pub fn managed_capability_contract_required(
    args: &[String],
    fm: &Frontmatter,
    global_config: &Config,
    harness: &str,
) -> bool {
    if harness == "opencode" {
        return resolve_codex_network_access(
            fm.codex_network_access,
            global_config.codex_network_access,
        ) == CodexNetworkAccess::Enabled
            || !fm.required_ssh_targets.is_empty();
    }
    if harness != "codex" {
        return false;
    }
    resolve_codex_network_access(fm.codex_network_access, global_config.codex_network_access)
        == CodexNetworkAccess::Enabled
        || !fm.required_ssh_targets.is_empty()
        || !add_dirs_from_args(args).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_capability_contract_required_requires_network_ssh_or_writable_roots() {
        let config = Config::default();
        let mut fm = Frontmatter::default();
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "claude"
        ));

        fm.codex_network_access = Some(CodexNetworkAccess::Enabled);
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "claude"
        ));

        fm.codex_network_access = None;
        let config = Config {
            codex_network_access: Some(CodexNetworkAccess::Enabled),
            ..Default::default()
        };
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "claude"
        ));

        let config = Config::default();
        fm.required_ssh_targets = vec!["example-host".to_string()];
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "claude"
        ));

        fm.required_ssh_targets.clear();
        let add_dir_args = [
            "exec".to_string(),
            "--json".to_string(),
            "--add-dir".to_string(),
            "/tmp/example".to_string(),
        ];
        assert!(managed_capability_contract_required(
            &add_dir_args,
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &add_dir_args,
            &fm,
            &config,
            "opencode"
        ));
        assert!(!managed_capability_contract_required(
            &add_dir_args,
            &fm,
            &config,
            "claude"
        ));

        let equals_add_dir_args = ["exec".to_string(), "--add-dir=/tmp/example".to_string()];
        assert!(managed_capability_contract_required(
            &equals_add_dir_args,
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &equals_add_dir_args,
            &fm,
            &config,
            "opencode"
        ));
    }
}
