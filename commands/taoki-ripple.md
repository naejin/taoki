---
allowed-tools: mcp__taoki__ripple
description: Trace the ripple effect — what depends on a file and what it depends on
---

Call the `mcp__taoki__ripple` tool on the specified file path.

After receiving the ripple analysis, present:
- Files this file depends on (imports) with symbols
- Files that depend on this file (used_by / reverse dependencies)
- External packages used
- Impact assessment: how many files would be affected by changes
