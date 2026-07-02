use std::path::Path;

pub fn closeout_latency_message(file: &Path, total_ms: u128, phases: &[(String, u128)]) -> String {
    let phase_text = phases
        .iter()
        .map(|(phase, elapsed)| format!("{phase}:{elapsed}ms"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "closeout_latency file={} total_ms={} phases={}",
        file.display(),
        total_ms,
        phase_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closeout_latency_message_lists_phase_timings() {
        let message = closeout_latency_message(
            Path::new("tasks/doc.md"),
            300,
            &[
                ("git_commit".to_string(), 12),
                ("session_check".to_string(), 4),
            ],
        );

        assert!(message.contains("closeout_latency file=tasks/doc.md total_ms=300"));
        assert!(message.contains("git_commit:12ms,session_check:4ms"));
    }
}
