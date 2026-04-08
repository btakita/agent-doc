# Module Harness Audit

Check coverage and drift of module-level harness docs across a directory.

## Steps

1. Run `module-harness inventory <dir>` to collect all evals across modules
2. For each eval, check if a matching test exists
3. Report coverage: tested vs aspirational evals
4. Flag drift: public symbols not mentioned in spec
