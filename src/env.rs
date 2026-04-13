//! # Module: env
//!
//! ## Spec
//! - `expand_values(env)` takes an order-preserving map of env var name → raw value
//!   (potentially containing `$(command)` or `$VAR` shell expressions) and returns a
//!   `Vec<(String, String)>` of expanded values in document order.
//! - Each value is expanded through a single `sh -c` invocation that exports vars
//!   sequentially, so later values can reference earlier keys.
//! - Returns an empty `Vec` when the input is empty (no shell spawn).
//! - `shell_export_prefix(env)` returns a string of the form
//!   `export K1="V1" && export K2="V2" && ` using **unexpanded** values, suitable for
//!   prepending to a `tmux send-keys` command so the target pane's shell expands
//!   `$(passage ...)` internally — keeps secrets out of the tmux visual layer.
//!
//! ## Agentic Contracts
//! - `expand_values` performs synchronous shell I/O; failures propagate via
//!   `anyhow::Result`.
//! - The order of returned pairs matches the input `IndexMap` iteration order.
//! - `shell_export_prefix` is purely textual — it does NOT touch the shell or
//!   expand anything. It escapes double quotes in values with `\"`.
//!
//! ## Evals
//! - expand_values_plain: `{FOO: "bar", BAZ: "qux"}` → ordered pairs of literal strings
//! - expand_values_shell_expansion: `{GREETING: "$(echo hello)"}` → `[(GREETING, "hello")]`
//! - expand_values_cross_reference: `{BASE: "world", DERIVED: "$BASE"}` → `DERIVED` = `"world"`
//! - expand_values_empty: empty map → empty Vec, no shell spawned
//! - shell_export_prefix_empty: empty map → empty string
//! - shell_export_prefix_basic: two entries → `export K1="v1" && export K2="v2" && `
//! - shell_export_prefix_quotes_in_value: value with `"` → escaped with `\"`

use anyhow::{Context, Result};
use indexmap::IndexMap;

/// Expand an env var map through the shell, returning pairs in document order.
///
/// Values may contain `$(command)` and `$VAR` — the shell expands them. Later
/// values can reference earlier keys because all vars are exported sequentially
/// within the same shell invocation before being printed.
pub fn expand_values(env: &IndexMap<String, String>) -> Result<Vec<(String, String)>> {
    if env.is_empty() {
        return Ok(Vec::new());
    }
    let mut script = String::new();
    let keys: Vec<&String> = env.keys().collect();
    for (key, value) in env {
        script.push_str(&format!("export {}={}\n", key, value));
    }
    for key in &keys {
        script.push_str(&format!("printf '%s\\0' \"${{{}}}\"\n", key));
    }
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .output()
        .context("failed to spawn shell for env expansion")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("env expansion failed: {}", stderr.trim());
    }
    let stdout = String::from_utf8(output.stdout)
        .context("env expansion output is not valid UTF-8")?;
    let values: Vec<&str> = stdout.split('\0').collect();
    let mut result = Vec::with_capacity(keys.len());
    for (i, key) in keys.iter().enumerate() {
        let value = values.get(i).unwrap_or(&"");
        result.push(((*key).clone(), value.to_string()));
    }
    Ok(result)
}

/// Build a shell `export K="V" && ...` prefix suitable for prepending to a
/// `tmux send-keys` command. Values are passed **unexpanded** so secrets like
/// `$(passage ...)` are resolved by the target pane's shell, never appearing
/// in the send-keys argument list or tmux scrollback.
pub fn shell_export_prefix(env: &IndexMap<String, String>) -> String {
    if env.is_empty() {
        return String::new();
    }
    let mut prefix = String::new();
    for (key, value) in env {
        let escaped = value.replace('"', "\\\"");
        prefix.push_str(&format!("export {}=\"{}\" && ", key, escaped));
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_values_plain() {
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        env.insert("BAZ".to_string(), "qux".to_string());
        let result = expand_values(&env).unwrap();
        assert_eq!(
            result,
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string()),
            ]
        );
    }

    #[test]
    fn expand_values_shell_expansion() {
        let mut env = IndexMap::new();
        env.insert("GREETING".to_string(), "$(echo hello)".to_string());
        let result = expand_values(&env).unwrap();
        assert_eq!(result[0].1, "hello");
    }

    #[test]
    fn expand_values_cross_reference() {
        let mut env = IndexMap::new();
        env.insert("BASE".to_string(), "world".to_string());
        env.insert("DERIVED".to_string(), "$BASE".to_string());
        let result = expand_values(&env).unwrap();
        assert_eq!(result[0], ("BASE".to_string(), "world".to_string()));
        assert_eq!(result[1], ("DERIVED".to_string(), "world".to_string()));
    }

    #[test]
    fn expand_values_empty() {
        let env = IndexMap::new();
        let result = expand_values(&env).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn shell_export_prefix_empty() {
        let env = IndexMap::new();
        assert_eq!(shell_export_prefix(&env), "");
    }

    #[test]
    fn shell_export_prefix_basic() {
        let mut env = IndexMap::new();
        env.insert("K1".to_string(), "v1".to_string());
        env.insert("K2".to_string(), "v2".to_string());
        assert_eq!(
            shell_export_prefix(&env),
            "export K1=\"v1\" && export K2=\"v2\" && "
        );
    }

    #[test]
    fn shell_export_prefix_quotes_in_value() {
        let mut env = IndexMap::new();
        env.insert("K".to_string(), "a\"b".to_string());
        assert_eq!(shell_export_prefix(&env), "export K=\"a\\\"b\" && ");
    }

    #[test]
    fn shell_export_prefix_unexpanded_passage() {
        let mut env = IndexMap::new();
        env.insert(
            "TOKEN".to_string(),
            "$(passage btak/secret)".to_string(),
        );
        let prefix = shell_export_prefix(&env);
        assert!(prefix.contains("$(passage btak/secret)"));
        assert!(!prefix.contains("export TOKEN=\"\""));
    }
}
