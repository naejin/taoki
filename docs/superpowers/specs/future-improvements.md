# Future Improvements (Backlog)

Items deferred from the 2026-03-15 improvements session. Each should get its own spec/plan cycle when ready.

## Tool Enhancements (Rust code changes)

### Batch index mode
Currently `index` operates on a single file. Add a mode that accepts a glob pattern and returns indexes for all matching files in one MCP call. This would reduce round-trips when Claude needs to understand multiple related files.

### code_map summary mode for large repos
For repos with 500+ files, `code_map` output can be overwhelming. Add a `--summary` or `depth` parameter that groups files by directory and shows only top-level structure, with the ability to drill into specific directories.

## Agent Adoption (Phase 2)

### Hook refinement based on usage data
After deploying the initial hooks, observe how agents interact with them. Tune the PreToolUse hook prompts based on whether agents are over-triggering (calling Taoki on non-code files) or under-triggering (still skipping Taoki for source files).
