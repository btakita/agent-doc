---
session: db08a891-c339-4a49-9f49-2d8a1f30ed10
resume: null
agent: null
model: null
branch: null
---
# Resume Session

## Status

**Resume**: `resume/BrianTakita.md` — committed and pushed to [btakita/resume](https://github.com/btakita/resume)
**PDF**: auto-generated via GitHub Action, deployed to [btakita.github.io/resume](https://btakita.github.io/resume/)
**LinkedIn**: [linkedin.com/in/briantakita](https://www.linkedin.com/in/briantakita/)
**Corky**: v0.11.0 released — [GitHub](https://github.com/btakita/corky/releases/tag/v0.11.0) / [crates.io](https://crates.io/crates/corky/0.11.0) / [PyPI](https://pypi.org/project/corky/0.11.0/)

## Completed

### Resume

- Markdown resume created from LinkedIn PDF export + new content
- Resume repo: public at `btakita/resume`, submodule of agent-loop
- PDF pipeline: Makefile (`make pdf`) → pandoc + weasyprint with `resume.css`
- GitHub Action: triggers on .md/.css/Makefile changes, builds PDF, commits, deploys to Pages
- GitHub Pages: landing page at btakita.github.io/resume with PDF download


**Content changes applied from hand-edited PDF:**
- Contact info reordered (LinkedIn/Email/Github + Location/Website/Phone)
- Skills section added (broad technology list)
- All experience entries have duration in parentheses
- Open Source dates updated to "2024 – Present"
- corky/agent-doc simplified to one-liners
- Other contributions converted to bullet list
- Languages section added ("English: Native")

## User

In resume/BrianTakita, reduce the margins to the same margins as resume/Brian-hand-edit
