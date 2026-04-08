---
name: testing
description: Write and run automated tests for the rappstack/ctx-core ecosystem
user_invocable: true
---

# Testing Skill

Write and run automated tests for rappstack, ctx-core, and related TypeScript/Bun projects.

## Conventions

### Test Runner
- Use **Bun's native test runner** (`bun test`) with `bun:test` imports
- Colocate test files: `foo.test.ts` next to `foo.ts`
- Run from the package directory: `cd lib/<package> && bun test`

### No Mocks
- **Avoid mocks whenever possible** — test real logic with real objects
- Extract pure computation functions from memo callbacks and test those directly
- Use real `URL`, `Request`, and other built-in objects
- Only mock when the dependency chain is broken (e.g., missing exports between packages)

### Test Structure
```typescript
import { describe, test, expect, afterEach } from 'bun:test'

describe('function_name', () => {
  test('describes behavior, not implementation', () => {
    expect(actual).toBe(expected)
  })
})
```

### Naming
- Test files: `<source>.test.ts`
- Describe blocks: name of the function being tested
- Test names: describe the behavior ("returns undefined for null input", not "test null")

### Test Categories

**Unit tests:** Pure function logic, edge cases, null safety
```typescript
test('returns undefined for null request_url', () => {
  expect(compute(null)).toBeUndefined()
})
```

**Functional tests:** Test the exported API with realistic inputs
```typescript
test('computes full href with search params', () => {
  expect(request_url__href__compute('/foo', '?q=1', 'http://localhost:3000'))
    .toBe('http://localhost:3000/foo?q=1')
})
```

**Smoke tests:** Verify the build doesn't crash
```bash
bun run build  # should exit 0
```

### Known Limitations

**rebuildjs/rhonojs import chain:** Full memo-level integration tests may fail with `SyntaxError: export 'html_response__new' not found in 'rebuildjs/server'`. This is a dependency version mismatch. Workaround: extract pure computation logic and test that instead of importing through the full memo pipeline.

### Running Tests

```bash
# Single package
cd lib/domain--server && bun test

# Specific file
bun test request/index.test.ts

# All tests in workspace
cd rappstack-dev && bun test lib/*/
```
