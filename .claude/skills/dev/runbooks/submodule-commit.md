# Submodule Commit + Parent Update

Procedure for committing changes in a git submodule and updating the parent repo pointer.

## Steps

1. `cd src/<submodule>` — always commit from within the submodule directory
2. Stage and commit changes inside the submodule
3. Push the submodule: `git push`
4. Return to parent: `cd ../..`
5. Update parent pointer: `git add src/<submodule> && git commit -m "update <submodule> to <description>"`
6. Push parent (if applicable)

## Rules

- **Never** stage files across repo boundaries in a single commit
- **Never** commit submodule changes from the parent directory
- For nested submodules, commit inside-out (deepest first)
