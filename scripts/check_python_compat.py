#!/usr/bin/env python3
"""Refuse Python syntax newer than the repo's declared minimum (gh #117).

Why this exists: `scripts/audit-actions-artifacts.py` once carried
`f"{reason.replace('|', '\\|')}"`. A backslash inside an f-string *expression*
is legal only from Python 3.12 (PEP 701), so the file was a SyntaxError on
3.11 — and because `make check` imports it via `artifact-purge-check`, the
whole verification suite died before clippy/test on every pre-3.12 machine,
while a 3.12+ runner stayed green.

The floor is `requires-python` in `pyproject.toml` (`>=3.8`). Every
git-tracked `*.py` must parse under it. Two layers, so the check bites on
whatever interpreter runs `make check`:

1. `compile()` under the running interpreter. On a pre-3.12 interpreter this
   alone rejects every PEP 701 construct.
2. On 3.12+ (where those constructs parse), a tokenize scan of every f-string
   replacement field for the PEP 701-only forms: a backslash in a nested
   string, a `#` comment, a newline inside a single-quoted f-string, and reuse
   of the enclosing f-string's quote.

Exit codes: 0 = ok, 1 = violation found.
"""

from __future__ import annotations

import io
import re
import subprocess
import sys
import tokenize
from pathlib import Path
from typing import List, Optional, Tuple

ROOT = Path(__file__).resolve().parent.parent
PEP701_VERSION = (3, 12)
STRING_PREFIX = re.compile(r"^[A-Za-z]*")


def declared_minimum(pyproject: Path) -> Tuple[int, int]:
    """Read `requires-python = ">=X.Y"` without tomllib (absent before 3.11)."""
    text = pyproject.read_text(encoding="utf-8")
    match = re.search(r'^requires-python\s*=\s*">=\s*(\d+)\.(\d+)', text, re.MULTILINE)
    if match is None:
        raise SystemExit(f"[python-compat] no `requires-python = \">=X.Y\"` in {pyproject}")
    return int(match.group(1)), int(match.group(2))


def _quote_of(token_text: str) -> str:
    body = token_text[STRING_PREFIX.match(token_text).end():]
    return body[:3] if body[:3] in ('"""', "'''") else body[:1]


def pep701_violations(source: str) -> List[Tuple[int, str]]:
    """Return (line, reason) for f-string constructs that need Python 3.12+.

    Only meaningful on a 3.12+ tokenizer, which emits FSTRING_START/END and
    tokenizes replacement fields; older tokenizers return no findings here
    (their `compile()` already rejects these constructs).
    """
    fstring_start = getattr(tokenize, "FSTRING_START", None)
    fstring_end = getattr(tokenize, "FSTRING_END", None)
    if fstring_start is None:
        return []
    found: List[Tuple[int, str]] = []
    # Each open f-string: [quote, replacement-field brace depth].
    stack: List[list] = []
    for tok in tokenize.generate_tokens(io.StringIO(source).readline):
        in_expr = bool(stack) and stack[-1][1] > 0
        outer = stack[-1][0] if stack else ""
        line = tok.start[0]
        if in_expr:
            if tok.type == tokenize.COMMENT:
                found.append((line, "comment inside an f-string expression"))
            elif tok.type == tokenize.NL and len(outer) == 1:
                found.append((line, "newline inside a single-quoted f-string expression"))
            elif tok.type in (tokenize.STRING, fstring_start):
                quote = _quote_of(tok.string)
                if tok.type == tokenize.STRING and "\\" in tok.string:
                    found.append((line, "backslash inside an f-string expression"))
                if quote == outer or (len(outer) == 1 and quote[:1] == outer):
                    found.append((line, "f-string expression reuses the enclosing quote"))
        if tok.type == fstring_start:
            stack.append([_quote_of(tok.string), 0])
        elif tok.type == fstring_end and stack:
            stack.pop()
        elif tok.type == tokenize.OP and stack and tok.string in ("{", "}"):
            stack[-1][1] += 1 if tok.string == "{" else -1
    return found


def check_source(name: str, source: str, minimum: Tuple[int, int]) -> List[str]:
    try:
        compile(source, name, "exec", dont_inherit=True)
    except SyntaxError as exc:
        return [f"{name}:{exc.lineno}: SyntaxError under Python {sys.version.split()[0]}: {exc.msg}"]
    if minimum >= PEP701_VERSION:
        return []
    floor = f"{minimum[0]}.{minimum[1]}"
    return [
        f"{name}:{line}: {reason} (needs Python 3.12+, floor is {floor})"
        for line, reason in pep701_violations(source)
    ]


def tracked_python_files() -> List[Path]:
    out = subprocess.run(
        ["git", "ls-files", "-z", "--", "*.py"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout.decode("utf-8")
    return [ROOT / rel for rel in out.split("\0") if rel]


def self_test() -> str:
    minimum = (3, 8)
    clean = 'x = {"a": 1}\nprint(f"{x[\'a\']:>4} {x!r} {{lit}} \\n")\nprint(f"""{x["a"]}""")\n'
    assert check_source("clean.py", clean, minimum) == [], check_source("clean.py", clean, minimum)
    bad_samples = {
        "backslash": "r = 'a'\nprint(f\"{r.replace('|', '\\\\|')}\")\n",
        "comment": "x = 1\nprint(f'''{x # note\n}''')\n",
        "same-quote": 'd = {"a": 1}\nprint(f"{d["a"]}")\n',
        "nested-fstring-quote": 'x = 1\nprint(f"{f"{x}"}")\n',
    }
    for label, sample in bad_samples.items():
        problems = check_source(f"{label}.py", sample, minimum)
        assert problems, f"{label}: PEP 701 construct not refused"
        if sys.version_info >= PEP701_VERSION:
            assert check_source(f"{label}.py", sample, (3, 12)) == [], f"{label}: refused despite a 3.12 floor"
    assert declared_minimum(ROOT / "pyproject.toml") < PEP701_VERSION
    return "[self-test] check_python_compat: ok"


def main(argv: Optional[List[str]] = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if args == ["--self-test"]:
        print(self_test())
        return 0
    minimum = declared_minimum(ROOT / "pyproject.toml")
    files = [Path(a).resolve() for a in args] if args else tracked_python_files()
    problems: List[str] = []
    for path in files:
        problems.extend(
            check_source(str(path.relative_to(ROOT)), path.read_text(encoding="utf-8"), minimum)
        )
    for problem in problems:
        print(f"[python-compat] {problem}")
    if problems:
        return 1
    print(f"[python-compat] {len(files)} file(s) parse under Python >={minimum[0]}.{minimum[1]}: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
