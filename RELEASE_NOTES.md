# Taoki v1.3.1 Release Notes

## Highlights

Adds automated release validation to the CI pipeline, ensuring every release artifact is tested before it reaches users. Includes a local pre-release script for catching issues before tagging.

## Changes

### CI Release Validation

The release pipeline now gates artifact publication on two new validation stages:

- **Per-platform smoke test** -- after each build, the binary is verified on its native platform:
  - `--version` output matches the git tag
  - MCP `initialize` handshake succeeds (server identifies as taoki)
  - `tools/list` returns all 3 tools (radar, ripple, xray)
- **Artifact structure validation** -- after all builds complete, every archive is extracted and checked:
  - All 13 required files present (plugin.json, commands, skills, hooks, scripts)
  - `plugin.json` version matches the release tag
  - No `.mcp.json` leaked into the archive
- The **Create Release** job now depends on both build and validate passing -- a broken artifact can no longer reach users.

### Local Pre-Release Script

New `scripts/prerelease.sh` for running validation before tagging:

```bash
./scripts/prerelease.sh
```

Checks: version consistency (Cargo.toml, plugin.json, git tag), all release files present, shell script syntax, cargo clippy + test, MCP protocol smoke test, plugin.json schema.

## Stats

- **187 unit tests**, 0 clippy warnings
- **5 build targets** validated per release (linux x86_64/aarch64, macos x86_64/aarch64, windows x86_64)
- **13 required files** checked per artifact

## Upgrading

Re-run the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

Or if installed via marketplace: `claude plugin install taoki@monet-plugins`

No cache or protocol changes -- fully compatible with v1.3.0.
