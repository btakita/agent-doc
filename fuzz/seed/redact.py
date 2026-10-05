#!/usr/bin/env python3
"""Redact real agent-doc traffic into fuzz seeds (`#netadv7`).

Session documents and responses carry operator prose, private project names,
paths, and occasionally credentials. The fuzz corpus must keep their STRUCTURE
(frontmatter keys, component markers, patch blocks, headings, fences, list and
quote syntax, byte-length shape) and none of their CONTENT.

Every word outside a small structural allow-list is replaced by a deterministic
lorem word, every long digit run is zeroed, frontmatter values outside a small
enum allow-list become `sample`, and marker attribute values become `sample`.

    fuzz/seed/redact.py md   <in.md>  <out>     # markdown document / response
    fuzz/seed/redact.py yrs  <in.yrs> <out>     # legacy binary CRDT state

The `yrs` mode is length-preserving (letters -> 'x', digits -> '0' inside
alphanumeric runs), so varint length prefixes in the binary stay valid.
"""

import hashlib
import re
import sys

LOREM = (
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod "
    "tempor incididunt ut labore et dolore magna aliqua enim ad minim veniam "
    "quis nostrud exercitation ullamco laboris nisi aliquip ex ea commodo"
).split()

# Words that carry agent-doc structure or markdown semantics.
KEEP_WORDS = {
    "Re", "Queue", "prompt", "Session", "Summary", "Compacted", "agent", "doc",
    "exchange", "queue", "backlog", "pending", "status", "review", "icebox",
    "done", "boundary", "patch", "replace", "output", "mermaid", "rust", "bash",
    "json", "yaml", "toml", "text", "graph", "TD", "LR", "subgraph", "end",
    "true", "false", "null", "go", "start", "stop", "x", "X", "auto",
}

# Frontmatter values that are enums agent-doc dispatches on.
KEEP_VALUES = {
    "template", "append", "crdt", "disk", "claude", "codex", "opencode", "grok",
    "opus", "sonnet", "haiku", "true", "false", "start", "go", "stop", "auto",
    "live", "markdown", "null", "",
}

MARKER = re.compile(r"^(\s*/?)((?:agent|patch|replace):[A-Za-z0-9_:.-]+)(.*?)(\s*)$", re.S)
WORD = re.compile(r"[A-Za-z][A-Za-z0-9_']*")
DIGITS = re.compile(r"\d{5,}")


def lorem(word: str) -> str:
    if word in KEEP_WORDS:
        return word
    digest = hashlib.sha256(word.lower().encode()).digest()
    out = LOREM[digest[0] % len(LOREM)]
    return out.capitalize() if word[0].isupper() else out


def scrub_text(text: str) -> str:
    text = WORD.sub(lambda m: lorem(m.group(0)), text)
    text = DIGITS.sub(lambda m: "0" * len(m.group(0)), text)
    # Non-ASCII prose (names, emoji) carries no structure the parsers key on
    # beyond being multi-byte; keep one representative so UTF-8 paths stay hot.
    return re.sub(r"[^\x00-\x7f]+", "—", text)


def scrub_comment(inner: str) -> str:
    m = MARKER.match(inner)
    if not m:
        return scrub_text(inner)
    lead, name, attrs, trail = m.groups()
    attrs = re.sub(r"=(\"[^\"]*\"|'[^']*'|\S+)", "=sample", attrs)
    attrs = WORD.sub(lambda w: w.group(0) if "=" in attrs else lorem(w.group(0)), attrs)
    return f"{lead}{name}{attrs}{trail}"


def scrub_body(body: str) -> str:
    out = []
    pos = 0
    for m in re.finditer(r"<!--(.*?)-->", body, re.S):
        out.append(scrub_text(body[pos : m.start()]))
        out.append("<!--" + scrub_comment(m.group(1)) + "-->")
        pos = m.end()
    out.append(scrub_text(body[pos:]))
    return "".join(out)


def scrub_frontmatter(yaml: str) -> str:
    lines = []
    preset = 0
    for line in yaml.split("\n"):
        m = re.match(r"^(\s*)([^:#\s][^:]*|'[^']*'|\"[^\"]*\"):(\s*)(.*)$", line)
        if not m:
            lines.append(scrub_text(line))
            continue
        indent, key, gap, value = m.groups()
        if indent:
            preset += 1
            key = f"'#preset-{preset}'"
        bare = value.strip().strip("'\"")
        if key == "agent_doc_session" and value:
            value = "00000000-0000-4000-8000-000000000001"
        elif bare not in KEEP_VALUES and not re.fullmatch(r"\d{1,4}", bare):
            quote = value[0] if value[:1] in "'\"" else ""
            value = f"{quote}sample{quote}" if value else value
        lines.append(f"{indent}{key}:{gap}{value}")
    return "\n".join(lines)


def redact_markdown(doc: str) -> str:
    if doc.startswith("---\n"):
        end = doc.find("\n---", 4)
        if end != -1:
            return "---\n" + scrub_frontmatter(doc[4:end]) + scrub_body(doc[end:])
    return scrub_body(doc)


def redact_binary(blob: bytes) -> bytes:
    def run(m):
        chunk = m.group(0)
        if not re.search(rb"[A-Za-z]", chunk) or chunk == b"content":
            return chunk
        return bytes(b"x"[0] if c >= 0x41 else b"0"[0] for c in chunk)

    return re.sub(rb"[A-Za-z0-9]{3,}", run, blob)


def main() -> None:
    mode, src, dst = sys.argv[1:4]
    if mode == "md":
        with open(src, encoding="utf-8", errors="replace") as fh:
            text = fh.read()
        with open(dst, "w", encoding="utf-8") as fh:
            fh.write(redact_markdown(text))
    elif mode == "yrs":
        with open(src, "rb") as fh:
            blob = fh.read()
        with open(dst, "wb") as fh:
            fh.write(redact_binary(blob))
    else:
        raise SystemExit(f"unknown mode {mode}")


if __name__ == "__main__":
    main()
