//! Print the deterministic `completion_signal` for each NUL-separated text on
//! stdin, one verdict per line (`complete` / `inconclusive` / `incomplete`).
//!
//! Used by `scripts/steergate-model-eval` (`#steergatenamo`) to score the rules
//! tier on the same corpus as the pretrained end-of-turn models, so the
//! comparison runs the shipped classifier, not a Python re-implementation.
//!
//! ```sh
//! printf 'fix the\0fix the bug.\0' | cargo run -q -p agent-doc-debounce --example completion_signal
//! ```

use std::io::{Read, Write};

use agent_doc_debounce::edit_settle::{CompletionSignal, completion_signal};

fn main() -> std::io::Result<()> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for record in input.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        let verdict = match completion_signal(&text) {
            CompletionSignal::Complete => "complete",
            CompletionSignal::Inconclusive => "inconclusive",
            CompletionSignal::Incomplete => "incomplete",
        };
        writeln!(out, "{verdict}")?;
    }
    out.flush()
}
