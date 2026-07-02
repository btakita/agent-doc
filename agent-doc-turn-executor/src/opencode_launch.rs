//! Pure OpenCode launch argument policy.

pub fn default_base_args() -> Vec<String> {
    vec!["run".to_string()]
}

pub fn opencode_run_args(
    base_args: &[String],
    prompt: &str,
    session_id: Option<&str>,
    fork: bool,
    model: Option<&str>,
) -> Vec<String> {
    let mut args = base_args.to_vec();
    if let Some(sid) = session_id {
        args.push("--session".to_string());
        args.push(sid.to_string());
    } else if fork {
        args.push("--continue".to_string());
        args.push("--fork".to_string());
    }

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }

    args.push(prompt.to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn default_base_args_use_opencode_run_policy() {
        assert_eq!(default_base_args(), strings(&["run"]));
    }

    #[test]
    fn opencode_run_args_fresh_model() {
        let args = opencode_run_args(
            &default_base_args(),
            "hello",
            None,
            false,
            Some("zai/glm-5"),
        );
        assert_eq!(args, strings(&["run", "--model", "zai/glm-5", "hello"]));
    }

    #[test]
    fn opencode_run_args_session_resume() {
        let args = opencode_run_args(&default_base_args(), "hello", Some("sess-1"), false, None);
        assert_eq!(args, strings(&["run", "--session", "sess-1", "hello"]));
    }

    #[test]
    fn opencode_run_args_fork_last_session() {
        let args = opencode_run_args(&default_base_args(), "hello", None, true, None);
        assert_eq!(args, strings(&["run", "--continue", "--fork", "hello"]));
    }
}
