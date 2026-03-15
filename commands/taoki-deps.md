---
allowed-tools: mcp__taoki__dependencies
description: Show what depends on a file and what it depends on
---

Call the `mcp__taoki__dependencies` tool on the specified file path.

After receiving the dependencies, present:
- Files this file depends on (imports)
- Files that depend on this file (used_by / reverse dependencies)
- External packages used
- Impact assessment: how many files would be affected by changes
