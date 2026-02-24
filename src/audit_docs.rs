//! Audit instruction files against the codebase.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};

const LINE_BUDGET: usize = 1000;
static SKIP_PATHS: Lazy<std::collections::HashSet<&str>> =
    Lazy::new(|| [".env"].iter().copied().collect());

static IMPERATIVE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(use|add|create|run|do|don't|never|must|should|avoid|prefer|ensure|keep|set)\b").unwrap()
});

static TABLE_SEP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\|[\s:]*-+[\s:]*(\|[\s:]*-+[\s:]*)*\|?\s*$").unwrap()
});

const INFORMATIONAL_HEADINGS: &[&str] = &[
    "project structure",
    "directory layout",
    "architecture",
    "overview",
    "tech stack",
    "sources",
    "bibliography",
    "references",
    "available tools",
    "resources",
];

struct Issue {
    file: String,
    line: usize,
    end_line: usize,
    message: String,
    warning: bool,
}

/// Well-known project root marker files, in priority order.
const PROJECT_MARKERS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "go.mod",
    "Gemfile",
    "pom.xml",
    "build.gradle",
    "CMakeLists.txt",
    "Makefile",
    "flake.nix",
    "deno.json",
    "composer.json",
];

fn find_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Pass 1: Look for project marker files
    let mut dir = cwd.as_path();
    loop {
        for marker in PROJECT_MARKERS {
            if dir.join(marker).exists() {
                return dir.to_path_buf();
            }
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p,
            _ => break,
        }
    }

    // Pass 2: Look for .git directory
    dir = cwd.as_path();
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p,
            _ => break,
        }
    }

    // Pass 3: Fall back to CWD
    eprintln!("Warning: no project root marker found, using current directory");
    cwd
}

fn find_instruction_files(root: &Path) -> Vec<PathBuf> {
    let patterns = ["AGENTS.md", "README.md", "CLAUDE.md"];
    let mut found = std::collections::HashSet::new();

    for pattern in &patterns {
        let path = root.join(pattern);
        if path.exists() {
            found.insert(path);
        }
    }

    // .claude/**/SKILL.md
    if let Ok(entries) = glob::glob(&root.join(".claude/**/SKILL.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    // .claude/**/CLAUDE.md
    if let Ok(entries) = glob::glob(&root.join(".claude/**/CLAUDE.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    // .agents/**/SKILL.md
    if let Ok(entries) = glob::glob(&root.join(".agents/**/SKILL.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    // .agents/**/AGENTS.md
    if let Ok(entries) = glob::glob(&root.join(".agents/**/AGENTS.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    // src/**/AGENTS.md
    if let Ok(entries) = glob::glob(&root.join("src/**/AGENTS.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    // src/**/CLAUDE.md
    if let Ok(entries) = glob::glob(&root.join("src/**/CLAUDE.md").to_string_lossy()) {
        for entry in entries.flatten() {
            found.insert(entry);
        }
    }

    let mut result: Vec<PathBuf> = found.into_iter().collect();
    result.sort();
    result
}

/// Parse file paths from the Project Structure tree block.
fn extract_tree_paths(content: &str) -> Vec<(usize, String)> {
    let mut results = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut in_section = false;
    let mut in_block = false;
    let mut stack: Vec<(usize, String)> = Vec::new(); // (indent, dirname_with_slash)

    for (i, line) in lines.iter().enumerate() {
        let line_no = i + 1;
        if line.starts_with("## Project Structure") {
            in_section = true;
            continue;
        }
        if in_section && !in_block {
            if line.trim().starts_with("```") {
                in_block = true;
                continue;
            }
            if line.starts_with("## ") {
                break;
            }
            continue;
        }
        if !in_block {
            continue;
        }
        if line.trim().starts_with("```") {
            break;
        }

        let stripped = line.trim_end();
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = stripped.len() - stripped.trim_start().len();
        let mut name = trimmed.split('#').next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }

        // Strip symlink arrow notation
        if name.contains(" -> ") {
            name = format!("{}/", name.split(" -> ").next().unwrap_or("").trim());
        }

        // Pop deeper/equal entries from stack
        while stack.last().map(|(ind, _)| *ind >= indent).unwrap_or(false) {
            stack.pop();
        }

        if name.ends_with('/') {
            stack.push((indent, name));
        } else {
            let mut parts: Vec<String> = stack.iter().map(|(_, d)| d.clone()).collect();
            parts.push(name);
            let full = parts.join("");
            results.push((line_no, full));
        }
    }

    results
}

fn check_tree_paths(rel: &str, content: &str, root: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    let bracket_re = Regex::new(r"\[.*?]").unwrap();
    for (line_no, path) in extract_tree_paths(content) {
        if bracket_re.is_match(&path) {
            continue;
        }
        if SKIP_PATHS.contains(path.as_str()) {
            continue;
        }
        if !root.join(&path).exists() {
            issues.push(Issue {
                file: rel.to_string(),
                line: line_no,
                end_line: 0,
                message: format!("Referenced path does not exist: {}", path),
                warning: false,
            });
        }
    }
    issues
}

fn check_line_budget(files: &[PathBuf], root: &Path) -> (Vec<Issue>, Vec<(String, usize)>, usize) {
    let mut counts = Vec::new();
    let mut total = 0;
    for f in files {
        if let Ok(content) = std::fs::read_to_string(f) {
            let n = content.lines().count();
            let rel = f.strip_prefix(root).unwrap_or(f).to_string_lossy().to_string();
            counts.push((rel, n));
            total += n;
        }
    }
    let mut issues = Vec::new();
    if total > LINE_BUDGET {
        issues.push(Issue {
            file: "(all)".to_string(),
            line: 0,
            end_line: 0,
            message: format!("Over line budget: {} lines (max {})", total, LINE_BUDGET),
            warning: false,
        });
    }
    (issues, counts, total)
}

const SOURCE_DIRS: &[&str] = &["src", "lib", "app", "pkg", "cmd", "internal"];

const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "rb", "java", "kt",
    "c", "cpp", "h", "hpp", "cs", "swift", "zig", "hs", "ml", "ex",
    "exs", "clj", "scala", "lua", "php", "sh", "bash", "zsh",
];

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "build", "dist", ".git", "__pycache__",
    ".venv", "vendor", ".next", "out",
];

fn check_staleness(files: &[PathBuf], root: &Path) -> Vec<Issue> {
    let mut newest_mtime = std::time::SystemTime::UNIX_EPOCH;
    let mut newest_src = PathBuf::new();

    fn scan_sources(dir: &Path, newest: &mut std::time::SystemTime, newest_path: &mut PathBuf) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if SKIP_DIRS.contains(&name) {
                            continue;
                        }
                    }
                    scan_sources(&path, newest, newest_path);
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if SOURCE_EXTENSIONS.contains(&ext) {
                        if let Ok(meta) = path.metadata() {
                            if let Ok(mtime) = meta.modified() {
                                if mtime > *newest {
                                    *newest = mtime;
                                    *newest_path = path;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut found_any = false;
    for source_dir in SOURCE_DIRS {
        let dir = root.join(source_dir);
        if dir.exists() {
            found_any = true;
            scan_sources(&dir, &mut newest_mtime, &mut newest_src);
        }
    }

    if !found_any {
        return vec![];
    }

    let mut issues = Vec::new();
    for doc in files {
        if let Ok(meta) = doc.metadata() {
            if let Ok(doc_mtime) = meta.modified() {
                if doc_mtime < newest_mtime {
                    let rel = doc.strip_prefix(root).unwrap_or(doc).to_string_lossy().to_string();
                    let src_rel = newest_src
                        .strip_prefix(root)
                        .unwrap_or(&newest_src)
                        .to_string_lossy()
                        .to_string();
                    issues.push(Issue {
                        file: rel,
                        line: 0,
                        end_line: 0,
                        message: format!("Older than {} \u{2014} may be stale", src_rel),
                        warning: false,
                    });
                }
            }
        }
    }
    issues
}

fn is_agent_file(rel: &str) -> bool {
    let name = Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name == "AGENTS.md" || name == "SKILL.md" || name == "CLAUDE.md"
}

/// Return the heading level (1–6) and title text for a markdown heading line.
fn heading_level(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if rest.starts_with(' ') {
        Some((hashes, rest.trim()))
    } else {
        None
    }
}

/// A bullet line that is primarily a link or backtick-enclosed identifier.
fn is_link_bullet(line: &str) -> bool {
    let stripped = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
    match stripped {
        Some(rest) => rest.starts_with('[') || rest.starts_with('`'),
        None => false,
    }
}

/// A line that can appear within a link-heavy list block (link bullets, blanks,
/// or sub-section headings).
fn is_list_context(line: &str) -> bool {
    line.trim().is_empty()
        || line.starts_with("### ")
        || line.starts_with("#### ")
        || is_link_bullet(line)
}

fn check_actionable(rel: &str, content: &str) -> Vec<Issue> {
    if !is_agent_file(rel) {
        return vec![];
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut issues = Vec::new();

    // 1. Informational section headings
    for (i, line) in lines.iter().enumerate() {
        if let Some((level, title)) = heading_level(line) {
            let title_lower = title.to_lowercase();
            if INFORMATIONAL_HEADINGS.iter().any(|h| title_lower == *h) {
                let mut end = lines.len();
                for (j, line_j) in lines.iter().enumerate().skip(i + 1) {
                    if let Some((next_level, _)) = heading_level(line_j) {
                        if next_level <= level {
                            end = j;
                            break;
                        }
                    }
                }
                while end > i + 1 && lines[end - 1].trim().is_empty() {
                    end -= 1;
                }
                issues.push(Issue {
                    file: rel.to_string(),
                    line: i + 1,
                    end_line: end,
                    message: format!(
                        "Informational section \"{}\" \u{2014} consider moving to README.md",
                        title
                    ),
                    warning: true,
                });
            }
        }
    }

    // 2. Large fenced code blocks (> 8 lines) without imperative verb in 2 preceding lines
    {
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim().starts_with("```") {
                let start = i;
                i += 1;
                while i < lines.len() && !lines[i].trim().starts_with("```") {
                    i += 1;
                }
                let close = i;
                let block_lines = close - start - 1;
                if block_lines > 8 {
                    let check_start = start.saturating_sub(2);
                    let preceding = &lines[check_start..start];
                    let has_imperative = preceding.iter().any(|l| IMPERATIVE_RE.is_match(l));
                    if !has_imperative {
                        issues.push(Issue {
                            file: rel.to_string(),
                            line: start + 1,
                            end_line: if close < lines.len() {
                                close + 1
                            } else {
                                close
                            },
                            message: format!(
                                "Large code block ({} lines) without imperative context \u{2014} consider moving to README.md",
                                block_lines
                            ),
                            warning: true,
                        });
                    }
                }
            }
            i += 1;
        }
    }

    // 3. Large tables (> 5 non-separator rows)
    {
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim_start().starts_with('|') {
                let start = i;
                let mut rows = 0;
                while i < lines.len() && lines[i].trim_start().starts_with('|') {
                    if !TABLE_SEP_RE.is_match(lines[i].trim()) {
                        rows += 1;
                    }
                    i += 1;
                }
                if rows > 5 {
                    issues.push(Issue {
                        file: rel.to_string(),
                        line: start + 1,
                        end_line: i,
                        message: format!(
                            "Large table ({} rows) \u{2014} consider moving to README.md",
                            rows
                        ),
                        warning: true,
                    });
                }
                continue;
            }
            i += 1;
        }
    }

    // 4. Link-heavy bullet lists (> 10 consecutive link/backtick bullets)
    {
        let mut i = 0;
        while i < lines.len() {
            if is_link_bullet(lines[i]) {
                let start = i;
                let mut count = 0;
                while i < lines.len() && is_list_context(lines[i]) {
                    if is_link_bullet(lines[i]) {
                        count += 1;
                    }
                    i += 1;
                }
                let mut end = i;
                while end > start && lines[end - 1].trim().is_empty() {
                    end -= 1;
                }
                if count > 10 {
                    issues.push(Issue {
                        file: rel.to_string(),
                        line: start + 1,
                        end_line: end,
                        message: format!(
                            "Link-heavy list ({} items) \u{2014} consider moving to README.md",
                            count
                        ),
                        warning: true,
                    });
                }
                continue;
            }
            i += 1;
        }
    }

    issues
}

pub fn run(root_override: Option<&Path>) -> Result<()> {
    println!("Auditing docs...\n");

    let root = match root_override {
        Some(p) => p.to_path_buf(),
        None => find_root(),
    };
    let files = find_instruction_files(&root);
    let mut issues: Vec<Issue> = Vec::new();

    for doc in &files {
        let rel = doc
            .strip_prefix(&root)
            .unwrap_or(doc)
            .to_string_lossy()
            .to_string();
        if let Ok(content) = std::fs::read_to_string(doc) {
            issues.extend(check_tree_paths(&rel, &content, &root));
            issues.extend(check_actionable(&rel, &content));
        }
    }

    let (budget_issues, counts, total) = check_line_budget(&files, &root);
    issues.extend(budget_issues);
    issues.extend(check_staleness(&files, &root));

    for issue in &issues {
        let mut loc = format!("  {}", issue.file);
        if issue.line > 0 {
            if issue.end_line > issue.line {
                loc.push_str(&format!(":{}-{}", issue.line, issue.end_line));
            } else {
                loc.push_str(&format!(":{}", issue.line));
            }
        }
        let marker = if issue.warning { "\u{26a0}" } else { "\u{2717}" };
        println!("{:<50} {} {}", loc, marker, issue.message);
    }

    let mark = if total <= LINE_BUDGET {
        "\u{2713}"
    } else {
        "\u{2717}"
    };
    println!(
        "\nCombined instruction files: {} lines (budget: {}) {}",
        total, LINE_BUDGET, mark
    );
    for (name, n) in &counts {
        println!("  {}: {}", name, n);
    }

    let n = issues.len();
    if n > 0 {
        println!("\nFound {} issue(s)", n);
        std::process::exit(1);
    } else {
        println!("\nNo issues found \u{2713}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // --- extract_tree_paths ---

    #[test]
    fn extract_tree_paths_basic() {
        let content = "\
## Project Structure

```
src/
  main.rs
  lib.rs
```
";
        let paths = extract_tree_paths(content);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].1, "src/main.rs");
        assert_eq!(paths[1].1, "src/lib.rs");
    }

    #[test]
    fn extract_tree_paths_nested() {
        let content = "\
## Project Structure

```
src/
  agent/
    mod.rs
    claude.rs
  main.rs
```
";
        let paths = extract_tree_paths(content);
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].1, "src/agent/mod.rs");
        assert_eq!(paths[1].1, "src/agent/claude.rs");
        assert_eq!(paths[2].1, "src/main.rs");
    }

    #[test]
    fn extract_tree_paths_symlink() {
        let content = "\
## Project Structure

```
mail -> ../data/mail
src/
  main.rs
```
";
        let paths = extract_tree_paths(content);
        // symlink entries become directories (trailing /), so only main.rs is a file
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].1, "src/main.rs");
    }

    #[test]
    fn extract_tree_paths_with_comments() {
        let content = "\
## Project Structure

```
src/
  main.rs  # entry point
  lib.rs   # library
```
";
        let paths = extract_tree_paths(content);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].1, "src/main.rs");
        assert_eq!(paths[1].1, "src/lib.rs");
    }

    #[test]
    fn extract_tree_paths_no_section() {
        let content = "# Just a heading\n\nSome text.\n";
        let paths = extract_tree_paths(content);
        assert!(paths.is_empty());
    }

    #[test]
    fn extract_tree_paths_empty_block() {
        let content = "\
## Project Structure

```
```
";
        let paths = extract_tree_paths(content);
        assert!(paths.is_empty());
    }

    #[test]
    fn extract_tree_paths_stops_at_next_section() {
        let content = "\
## Project Structure

```
src/
  main.rs
```

## Other Section

Some text.
";
        let paths = extract_tree_paths(content);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].1, "src/main.rs");
    }

    #[test]
    fn extract_tree_paths_line_numbers() {
        let content = "\
## Project Structure

```
Cargo.toml
src/
  main.rs
```
";
        let paths = extract_tree_paths(content);
        assert_eq!(paths[0], (4, "Cargo.toml".to_string()));
        assert_eq!(paths[1], (6, "src/main.rs".to_string()));
    }

    // --- check_tree_paths ---

    #[test]
    fn check_tree_paths_existing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        let content = "\
## Project Structure

```
src/
  main.rs
```
";
        let issues = check_tree_paths("CLAUDE.md", content, root);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_tree_paths_missing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let content = "\
## Project Structure

```
src/
  missing.rs
```
";
        let issues = check_tree_paths("CLAUDE.md", content, root);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("missing.rs"));
        assert!(!issues[0].warning);
    }

    #[test]
    fn check_tree_paths_skips_brackets() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let content = "\
## Project Structure

```
src/
  [generated files]
```
";
        let issues = check_tree_paths("CLAUDE.md", content, root);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_tree_paths_skips_env() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let content = "\
## Project Structure

```
.env
```
";
        let issues = check_tree_paths("CLAUDE.md", content, root);
        assert!(issues.is_empty());
    }

    // --- check_line_budget ---

    #[test]
    fn check_line_budget_under() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("A.md"), "line1\nline2\nline3\n").unwrap();

        let files = vec![root.join("A.md")];
        let (issues, counts, total) = check_line_budget(&files, root);
        assert!(issues.is_empty());
        assert_eq!(total, 3);
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].0, "A.md");
        assert_eq!(counts[0].1, 3);
    }

    #[test]
    fn check_line_budget_over() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let content = "line\n".repeat(1001);
        fs::write(root.join("BIG.md"), &content).unwrap();

        let files = vec![root.join("BIG.md")];
        let (issues, _, total) = check_line_budget(&files, root);
        assert_eq!(total, 1001);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Over line budget"));
    }

    #[test]
    fn check_line_budget_multiple_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("A.md"), "a\nb\n").unwrap();
        fs::write(root.join("B.md"), "c\nd\ne\n").unwrap();

        let files = vec![root.join("A.md"), root.join("B.md")];
        let (_, counts, total) = check_line_budget(&files, root);
        assert_eq!(total, 5);
        assert_eq!(counts.len(), 2);
    }

    // --- check_staleness ---

    #[test]
    fn check_staleness_doc_newer_than_src() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        // Sleep briefly so doc mtime is strictly newer
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(root.join("CLAUDE.md"), "# Doc").unwrap();

        let files = vec![root.join("CLAUDE.md")];
        let issues = check_staleness(&files, root);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_staleness_doc_older_than_src() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(root.join("CLAUDE.md"), "# Doc").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(src.join("main.rs"), "fn main() {}").unwrap();

        let files = vec![root.join("CLAUDE.md")];
        let issues = check_staleness(&files, root);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("may be stale"));
    }

    #[test]
    fn check_staleness_no_src_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("CLAUDE.md"), "# Doc").unwrap();

        let files = vec![root.join("CLAUDE.md")];
        let issues = check_staleness(&files, root);
        assert!(issues.is_empty());
    }

    // --- is_agent_file ---

    #[test]
    fn is_agent_file_matches() {
        assert!(is_agent_file("AGENTS.md"));
        assert!(is_agent_file("SKILL.md"));
        assert!(is_agent_file("CLAUDE.md"));
        assert!(is_agent_file("src/AGENTS.md"));
        assert!(is_agent_file(".claude/skills/email/SKILL.md"));
        assert!(is_agent_file("nested/path/CLAUDE.md"));
    }

    #[test]
    fn is_agent_file_rejects() {
        assert!(!is_agent_file("README.md"));
        assert!(!is_agent_file("agents.md"));
        assert!(!is_agent_file("CHANGELOG.md"));
        assert!(!is_agent_file("src/main.rs"));
    }

    // --- heading_level ---

    #[test]
    fn heading_level_basic() {
        assert_eq!(heading_level("# Title"), Some((1, "Title")));
        assert_eq!(heading_level("## Section"), Some((2, "Section")));
        assert_eq!(heading_level("### Sub"), Some((3, "Sub")));
        assert_eq!(heading_level("###### Deep"), Some((6, "Deep")));
    }

    #[test]
    fn heading_level_rejects_invalid() {
        assert_eq!(heading_level("Not a heading"), None);
        assert_eq!(heading_level("##NoSpace"), None);
        assert_eq!(heading_level("####### Too deep"), None);
        assert_eq!(heading_level(""), None);
    }

    // --- is_link_bullet ---

    #[test]
    fn is_link_bullet_matches() {
        assert!(is_link_bullet("- [link](url)"));
        assert!(is_link_bullet("- `code` description"));
        assert!(is_link_bullet("* [link](url)"));
        assert!(is_link_bullet("* `code`"));
    }

    #[test]
    fn is_link_bullet_rejects() {
        assert!(!is_link_bullet("- plain text"));
        assert!(!is_link_bullet("not a bullet"));
        assert!(!is_link_bullet("  - indented"));
    }

    // --- is_list_context ---

    #[test]
    fn is_list_context_matches() {
        assert!(is_list_context(""));
        assert!(is_list_context("   "));
        assert!(is_list_context("### Sub heading"));
        assert!(is_list_context("#### Deep heading"));
        assert!(is_list_context("- [link](url)"));
    }

    #[test]
    fn is_list_context_rejects() {
        assert!(!is_list_context("- plain text"));
        assert!(!is_list_context("## Section"));
        assert!(!is_list_context("some paragraph"));
    }

    // --- check_actionable ---

    #[test]
    fn check_actionable_skips_non_agent_files() {
        let issues = check_actionable("README.md", "## Overview\n\nSome overview.\n");
        assert!(issues.is_empty());
    }

    #[test]
    fn check_actionable_informational_heading() {
        let content = "# Doc\n\n## Overview\n\nSome overview text.\n\n## Rules\n\nDo this.\n";
        let issues = check_actionable("CLAUDE.md", content);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Informational section"));
        assert!(issues[0].message.contains("Overview"));
        assert!(issues[0].warning);
    }

    #[test]
    fn check_actionable_no_informational_heading() {
        let content = "# Doc\n\n## Conventions\n\nUse serde.\n";
        let issues = check_actionable("AGENTS.md", content);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_actionable_large_code_block_without_context() {
        let mut lines = vec!["# Doc".to_string(), "".to_string()];
        lines.push("```rust".to_string());
        for i in 0..10 {
            lines.push(format!("let x{} = {};", i, i));
        }
        lines.push("```".to_string());
        let content = lines.join("\n");

        let issues = check_actionable("CLAUDE.md", &content);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Large code block"));
        assert!(issues[0].warning);
    }

    #[test]
    fn check_actionable_large_code_block_with_imperative() {
        let mut lines = vec![
            "# Doc".to_string(),
            "".to_string(),
            "Use the following pattern:".to_string(),
        ];
        lines.push("```rust".to_string());
        for i in 0..10 {
            lines.push(format!("let x{} = {};", i, i));
        }
        lines.push("```".to_string());
        let content = lines.join("\n");

        let issues = check_actionable("CLAUDE.md", &content);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_actionable_small_code_block_ok() {
        let content = "# Doc\n\n```\nfoo\nbar\n```\n";
        let issues = check_actionable("AGENTS.md", content);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_actionable_large_table() {
        let mut lines = vec!["# Doc".to_string(), "".to_string()];
        lines.push("| Col A | Col B |".to_string());
        lines.push("|-------|-------|".to_string());
        for i in 0..6 {
            lines.push(format!("| row{} | val{} |", i, i));
        }
        let content = lines.join("\n");

        let issues = check_actionable("CLAUDE.md", &content);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Large table"));
        assert!(issues[0].warning);
    }

    #[test]
    fn check_actionable_small_table_ok() {
        let content = "\
# Doc

| A | B |
|---|---|
| 1 | 2 |
| 3 | 4 |
";
        let issues = check_actionable("SKILL.md", content);
        assert!(issues.is_empty());
    }

    #[test]
    fn check_actionable_link_heavy_list() {
        let mut lines = vec!["# Doc".to_string(), "".to_string()];
        for i in 0..12 {
            lines.push(format!("- [link{}](https://example.com/{})", i, i));
        }
        let content = lines.join("\n");

        let issues = check_actionable("CLAUDE.md", &content);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("Link-heavy list"));
        assert!(issues[0].warning);
    }

    #[test]
    fn check_actionable_short_link_list_ok() {
        let mut lines = vec!["# Doc".to_string(), "".to_string()];
        for i in 0..5 {
            lines.push(format!("- [link{}](https://example.com/{})", i, i));
        }
        let content = lines.join("\n");

        let issues = check_actionable("AGENTS.md", &content);
        assert!(issues.is_empty());
    }

    // --- find_instruction_files ---

    #[test]
    fn find_instruction_files_root_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("CLAUDE.md"), "# Doc").unwrap();
        fs::write(root.join("README.md"), "# Readme").unwrap();
        fs::write(root.join("AGENTS.md"), "# Agents").unwrap();

        let files = find_instruction_files(root);
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.ends_with("CLAUDE.md")));
        assert!(files.iter().any(|f| f.ends_with("README.md")));
        assert!(files.iter().any(|f| f.ends_with("AGENTS.md")));
    }

    #[test]
    fn find_instruction_files_glob_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // .claude/**/SKILL.md
        fs::create_dir_all(root.join(".claude/skills/email")).unwrap();
        fs::write(root.join(".claude/skills/email/SKILL.md"), "# Skill").unwrap();

        // .claude/**/CLAUDE.md
        fs::create_dir_all(root.join(".claude/settings")).unwrap();
        fs::write(root.join(".claude/settings/CLAUDE.md"), "# Claude").unwrap();

        // src/**/CLAUDE.md
        fs::create_dir_all(root.join("src/agent")).unwrap();
        fs::write(root.join("src/agent/CLAUDE.md"), "# Agent").unwrap();

        // src/**/AGENTS.md
        fs::write(root.join("src/agent/AGENTS.md"), "# Agents").unwrap();

        let files = find_instruction_files(root);
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn find_instruction_files_empty() {
        let tmp = TempDir::new().unwrap();
        let files = find_instruction_files(tmp.path());
        assert!(files.is_empty());
    }

    #[test]
    fn find_instruction_files_sorted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("README.md"), "# R").unwrap();
        fs::write(root.join("CLAUDE.md"), "# C").unwrap();
        fs::write(root.join("AGENTS.md"), "# A").unwrap();

        let files = find_instruction_files(root);
        let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
        assert!(names.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn find_instruction_files_deduplicates() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("CLAUDE.md"), "# Doc").unwrap();

        let files = find_instruction_files(root);
        assert_eq!(files.len(), 1);
    }
}
