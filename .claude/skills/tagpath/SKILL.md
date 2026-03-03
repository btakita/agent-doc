---
description: Parse, search, and analyze tag-based identifiers across naming conventions
user-invocable: true
argument-hint: "<command> [args]"
---

# tagpath

Tag Path — parse, lint, and search tag-based identifiers across languages.

## Invocation

```
/tagpath <command> [args]
```

Run the given tagpath command via Bash. If no command is given, show this help summary.

## Commands

### parse — Decompose an identifier into canonical tags

```sh
tagpath parse <NAME> [-c <CONVENTION>] [-f text|json]
```

Detects naming convention, extracts tags, role, and shape.

```
tagpath parse createUserProfile
# Convention: camelCase
# Tags: [create, user, profile]
# Role: factory
```

### alias — Generate all convention variants of an identifier

```sh
tagpath alias <NAME> [-c <CONVENTION>] [-f text|json]
```

Produces the identifier in all 6 naming conventions (or a single target).

```
tagpath alias person_name
# snake_case:       person_name
# camelCase:        personName
# PascalCase:       PersonName
# kebab-case:       person-name
# UPPER_SNAKE_CASE: PERSON_NAME
# Ada_Case:         Person_Name
```

### prose — Human-readable description of an identifier

```sh
tagpath prose <NAME> [-f text|json]
```

Converts identifiers to natural English using role/shape detection.

```
tagpath prose create_user_profile   # "Creates a user profile"
tagpath prose is_valid_email        # "Checks if email is valid"
tagpath prose user_name_a           # "Array of user names"
```

### search — Semantic search across naming conventions

```sh
tagpath search <QUERY> <PATH> [-f text|json]
```

Finds identifiers matching a tag query regardless of naming convention. Searching for `create_user` also finds `createUser`, `CreateUser`, etc.

### lint — Validate identifiers against .naming.toml rules

```sh
tagpath lint [PATH] [-f text|json]
```

Checks identifiers against context rules defined in `.naming.toml`. Reports violations with expected conventions and suggested fixes.

### extract — Extract identifiers from source files

```sh
tagpath extract <PATH> [--ast] [-f text|json]
```

Scans files recursively and extracts all identifiers with location, convention, context, role, and shape. Use `--ast` for tree-sitter-based extraction (14 languages supported).

### graph — Build tag co-occurrence graph

```sh
tagpath graph [PATH] [-q <QUERY>] [-f text|dot|json]
```

Builds a directed graph where nodes are tags and edges connect sequential tag pairs. Use `-q` to filter to a subgraph around specific tags.

### init — Initialize .naming.toml config

```sh
tagpath init [-l <LANG>] [-p <PRESET>]
```

Generates a `.naming.toml` in the current directory from a language preset (39 languages available) and/or convention preset.

## Workflow

1. Run the user's command via Bash: `tagpath <command> [args]`
2. Present the output to the user
3. If the command fails (e.g., tagpath not installed), suggest `cargo install tagpath` or `cargo install --path tagpath/`

## Key Concepts

- Every identifier is a **path** through an ordered sequence of **tags**
- `personName` = `person_name` = `PersonName` = `person-name` → canonical `[person, name]`
- `__` separates namespace dimensions: `auth0__user__validate` → 3 dimensions
- Role detection: `create_*` (factory), `use_*` (hook), `set_*` (setter), `is_*` (predicate)
- Shape detection: `*_a` (array), `*_r` (record), `*_m` (map), `*$` (signal)
- 6 conventions: snake_case, camelCase, PascalCase, kebab-case, UPPER_SNAKE_CASE, Ada_Case

## Use Cases

- **Rename refactoring**: Use `alias` to find all convention variants of an identifier before renaming
- **Code review**: Use `parse` to understand what an identifier means
- **Convention enforcement**: Use `lint` with `.naming.toml` to validate naming consistency
- **Cross-language search**: Use `search` to find semantically equivalent identifiers across file types
- **Documentation**: Use `prose` to generate human-readable descriptions of identifiers
- **Architecture analysis**: Use `graph` to visualize tag relationships in a codebase
