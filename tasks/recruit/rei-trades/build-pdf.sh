#!/usr/bin/env bash
# Regenerate the REItrades proposal PDF from the markdown report.
# Title renders once (body H1 only) — no metadata-title injection, which is what
# caused the duplicated header in the first ad-hoc build.
set -euo pipefail
cd "$(dirname "$0")"

SRC="reitrades-v1.2-estimate-report.md"
OUT="REItrades V1.2 - Proposal & Estimate.pdf"
# Emit HTML into the local dir so the relative --css path (proposal.css) resolves
# against the document base URL when WeasyPrint loads it.
HTML=".build-proposal.html"
trap 'rm -f "$HTML"' EXIT

# pandoc: standalone HTML, our CSS, GitHub-style tables. No --metadata title,
# so the default template does not emit a second <h1 class="title">.
pandoc "$SRC" \
  --from gfm \
  --to html5 \
  --standalone \
  --metadata title:"REItrades V1.2 — Proposal & Estimate" \
  --css proposal.css \
  --output "$HTML" \
  --template default 2>/dev/null || \
pandoc "$SRC" --from gfm --to html5 --standalone --css proposal.css --output "$HTML"

# Strip any injected visible title block (belt-and-suspenders against template drift):
# remove the <header id="title-block-header">...</header> pandoc adds when title is set.
python3 - "$HTML" <<'PY'
import re, sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
s = re.sub(r'<header id="title-block-header">.*?</header>', '', s, flags=re.DOTALL)
open(p, "w", encoding="utf-8").write(s)
PY

weasyprint "$HTML" "$OUT"
echo "wrote: $OUT"
