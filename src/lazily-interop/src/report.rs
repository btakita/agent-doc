use serde_json::to_string_pretty;

use crate::types::{MutationProof, RunReport, RunStatus};

pub fn console_report(report: &RunReport) -> String {
    let mut output = format!(
        "{} {} seed={} peers={} emitted={} delivered={} staged={}",
        match report.status {
            RunStatus::Pass => "PASS",
            RunStatus::Fail => "FAIL",
        },
        report.scenario,
        report.seed,
        report.peers.join(","),
        report.coverage.frames_emitted,
        report.coverage.frames_delivered,
        report.staged_peers.len()
    );
    for failure in &report.failures {
        output.push_str(&format!("\n  {failure}"));
    }
    if !report.staged_peers.is_empty() {
        output.push_str("\n  staged:");
        for staged in &report.staged_peers {
            output.push_str(&format!("\n    {}: {}", staged.peer, staged.reason));
        }
    }
    output.push_str(&format!("\n  replay: {}", report.replay));
    output
}

pub fn json_report(report: &RunReport) -> String {
    to_string_pretty(report).expect("RunReport always serializes")
}

pub fn junit_report(report: &RunReport) -> String {
    let failures = report.failures.len();
    let mut output = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <testsuite name=\"lazily-interop\" tests=\"1\" failures=\"{failures}\">\n\
         <testcase classname=\"semantic\" name=\"{}\">",
        xml_escape(&report.scenario)
    );
    if let Some(failure) = report.failures.first() {
        output.push_str(&format!(
            "<failure type=\"{}\" message=\"{}\">{}</failure>",
            failure.category,
            xml_escape(&failure.message),
            xml_escape(&failure.to_string())
        ));
    }
    output.push_str("</testcase>\n</testsuite>\n");
    output
}

pub fn mutation_console(proofs: &[MutationProof]) -> String {
    let mut output = String::new();
    for proof in proofs {
        output.push_str(&format!(
            "{} {:<24} expected={} observed={} culprit={}\n",
            if proof.caught { "CAUGHT" } else { "ESCAPED" },
            proof.mutation,
            proof.expected_category,
            proof.observed_category,
            proof.culprit
        ));
    }
    output.push_str(&format!(
        "{}/{} required mutations caught",
        proofs.iter().filter(|proof| proof.caught).count(),
        proofs.len()
    ));
    output
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_junit_attributes() {
        assert_eq!(xml_escape("a&\"b"), "a&amp;&quot;b");
    }
}
