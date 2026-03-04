---
description: Query, author, and validate Existence Lang ontology terms
user-invocable: true
argument-hint: "<command> [args]"
---

# existence

CLI for the Existence Lang ontology framework — lookup, search, author, lint, and scope ontology terms.

## Invocation

```
/existence lookup <term>     — Read a term's full definition
/existence search <query>    — Search terms by relevance
/existence new <term>        — Author a new term (interactive)
/existence lint              — Validate ontology nodes
/existence scope [ring]      — List terms by ring level
```

Arguments: `<command>` is required. Run `/existence` with no arguments to show this help summary.

## Ontology Path Resolution

Before running any command, resolve the ontology path for the `--ontology` flag:

1. Check if `existence.toml` exists in the current working directory — if so, use `.`
2. Check if `existence-lang/ontology/` exists relative to the project root — if so, use that path
3. Fall back to `~/.existence/sources/` (the CLI's default resolution)

If options 1 or 2 match, pass `--ontology <resolved-path>` to every `existence` command. If only option 3 applies, omit the flag (the CLI handles it).

## Binary Resolution

Locate the `existence` binary:

1. Check `which existence` — use if found on PATH
2. Check `existence-lang/existence/target/release/existence` relative to the project root
3. Check `existence-lang/existence/target/debug/existence` relative to the project root
4. If none found, suggest `cargo install --path existence-lang/existence/` or `cargo build --release -p existence`

## Commands

### lookup — Read a term's full definition

```
/existence lookup <term>
```

1. Run `existence lookup <term>` via Bash
2. Display the full node definition (Ontology, Axiology, Epistemology sections)
3. If the command fails (term not found), automatically run `existence search <term>` and display similar terms as suggestions

### search — Search terms by relevance

```
/existence search <query>
```

1. Run `existence search <query>` via Bash
2. Display the ranked results
3. Optionally pass `--limit N` if the user specifies a count

### new — Author a new ontology term (interactive workflow)

```
/existence new <term>
```

This is the main interactive authoring workflow:

#### Step 1: Scaffold

- Check if `src/<term>.md` already exists in the ontology directory
- If it does not exist, create the file with the node template from SPEC.md:
  ```markdown
  # {Term}

  ## [Ontology](./ontology.md)

  {definition}

  ## [Axiology](./axiology.md)

  {value}

  ## [Epistemology](./epistemology.md)

  ### [Cultural](./culture.md) Definition

  {external references}

  ### [Pattern](./pattern.md) Expression

  {concrete examples}
  ```

#### Step 2: Research and Draft

- Read 3-5 existing term files from the ontology to absorb the voice, depth, and linking style (e.g., `existence.md`, `entity.md`, `abstraction.md`, `scope.md`)
- Using the existence kernel lens, draft the full definition:
  - **Ontology** — What the term IS. Start from Existence scope, narrow by context. Link to related terms via `[term](./term.md)`.
  - **Axiology** — Why it matters. Value, significance, implications for practice.
  - **Epistemology** — How we know/validate it. Include Cultural Definition (Wiktionary/Wikipedia blockquotes with `<a href="..." target="_blank">` links) and Pattern Expression (concrete examples at different scales).
- Definitions should be broad (Existence scope) by default, with notes on narrower applications

#### Step 3: Write and Present

- Write the drafted content to `src/<term>.md`
- Display the full draft in the console for user review
- Run `existence lint src/<term>.md` to check for structural issues

#### Step 4: Iterate

- The user may edit the file directly or provide feedback in the console
- On subsequent `/existence new <term>` invocations for the same term, re-read the file, diff against expectations, and refine
- Continue iterating until the user is satisfied

#### Step 5: Commit

- When the user confirms the term is ready, commit the new file within the ontology submodule
- If the term should be assigned to a ring, note that ring assignment lives in `existence.toml`, not the node file

### lint — Validate ontology nodes

```
/existence lint
/existence lint <path>
```

1. Run `existence lint` (or `existence lint <path>` for a specific file/directory) via Bash
2. Report all issues found
3. For common problems (missing required sections, broken links), offer to fix them:
   - Missing `## [Ontology]` — add the section header with a placeholder
   - Missing `## [Axiology]` — add the section header with a placeholder
   - Missing `## [Epistemology]` — add the section header with a placeholder
4. If the user accepts fixes, apply them and re-run lint to verify

### scope — List terms by ring level

```
/existence scope
/existence scope <ring>
```

1. Run `existence scope` (all rings) or `existence scope <ring>` (specific ring) via Bash
2. Display the ring structure with term counts and term lists
3. If a specific ring is requested, show that ring's terms with their one-line definitions

## Node Specification Reference

Each ontology node follows this structure (from SPEC.md):

| Section | Question | Required |
|---------|----------|----------|
| Ontology | What IS it? | Yes |
| Axiology | Why does it MATTER? | Yes |
| Ethics | What SHOULD we consider? | No |
| Epistemology | How do we KNOW it? | Yes |

**Conventions:**
- One file = one concept = one node in the graph
- Links use relative markdown: `[term](./term.md)`
- External refs use HTML anchors: `<a href="..." target="_blank">`
- External definitions are blockquoted (`>`)
- Definitions start broad (Existence scope), narrow by context
- Ring assignment is in `existence.toml`, not the node file

## Ring Structure

- **Ring 0 (kernel)** — 14 universal terms, always loaded. The existential scope.
- **Ring 1 (software)** — DDD bridge terms, immediately useful for software projects.
- **Ring 2+ (extended)** — Broader terms spanning philosophy, cognition, linguistics, etc.

## Success Criteria

- Commands execute and output is displayed clearly
- New terms follow SPEC.md structure and match existing node voice/style
- Lint passes after authoring or fixing nodes
- Related terms are properly cross-linked via `[term](./term.md)`
