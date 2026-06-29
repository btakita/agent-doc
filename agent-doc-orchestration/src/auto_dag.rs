//! `agent-doc auto-dag <FILE>` command wrapper.
//!
//! Pure Auto-DAG analysis/rendering lives in `agent-doc-document`; orchestration
//! owns only file IO and terminal output for the command.

use anyhow::{Context, Result};

pub use agent_doc_document::auto_dag::{
    AutoDag, DagItem, Lane, analyze, classify, render_mermaid, render_nested_list,
};

/// `agent-doc auto-dag <FILE>` entry point.
pub fn run(file: &std::path::Path, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("auto-dag: read {}", file.display()))?;
    let dag = analyze(&content)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&dag)?);
    } else {
        println!("# Auto-DAG: completion work-graph for {}\n", file.display());
        println!("{}", render_mermaid(&dag));
        println!("## Completion order\n");
        print!("{}", render_nested_list(&dag));
    }
    Ok(())
}
