# Taoki v1.3.0 Release Notes

## Highlights

Taoki now supports **three coding agents**: Claude Code, Gemini CLI, and OpenCode. A new interactive TUI installer detects which agents are available and lets you choose which to set up — replacing the old single-agent pipe-to-bash script.

## Changes

### Multi-Agent Interactive Installer

The install scripts (`install.sh` / `install.ps1`) are now full interactive TUI programs:

- **Agent selection** — checkbox UI to pick Claude Code, Gemini CLI, OpenCode (or any combination)
- **Scope selection** — global (all projects) or project-local for Gemini/OpenCode
- **Auto-detection** — pre-selects agents found on PATH
- **Per-agent setup:**
  - **Claude Code** — marketplace plugin install (unchanged from v1.2.0)
  - **Gemini CLI** — downloads binary to `~/.local/bin/taoki`, writes MCP config to `settings.json`, deploys instruction file, adds `@./taoki.md` import to `GEMINI.md`
  - **OpenCode** — downloads binary, writes MCP config to `opencode.json`, deploys instruction file, adds path to `instructions` array

The installer requires a TTY — scripts must be downloaded before running (not piped).

### Instruction Files

Two new files ship in release artifacts:
- `scripts/taoki-gemini.md` — Taoki tool guide deployed as `taoki.md` for Gemini CLI
- `scripts/taoki-opencode.md` — Taoki tool guide deployed as `taoki.md` for OpenCode

These describe the three tools (radar, xray, ripple), the recommended workflow, and usage rules.

### Robust JSON Config Handling

Install scripts manipulate agent config files (Gemini `settings.json`, OpenCode `opencode.json`) with:
- JSONC-aware comment stripping (preserves URLs containing `//`)
- Atomic writes via temp file + move
- Backup-on-parse-failure with manual fallback instructions
- UTF-8 without BOM on all PowerShell versions

### Install Command Change

Old (v1.2.0):
```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash
```

New (v1.3.0):
```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

Claude Code users can still install non-interactively via: `claude plugin marketplace add naejin/monet-plugins && claude plugin install taoki@monet-plugins`

### Version Pinning

Set `TAOKI_VERSION` environment variable to pin a specific release version (useful when GitHub API rate-limits apply):
```bash
TAOKI_VERSION=v1.3.0 bash /tmp/taoki-install.sh
```

## Breaking Changes

- `curl ... | bash` no longer works — the TUI requires a TTY. The script prints download-then-run instructions if piped.
- CLI argument version pinning (`bash -s -- v1.2.0`) is removed. Use `TAOKI_VERSION` env var instead.

## Stats

- **187 unit tests**, 0 clippy warnings
- **4 hooks** (unchanged from v1.2.0)
- **3 agents supported** (up from 1)

## Upgrading

Re-run the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

Or if installed via marketplace: `claude plugin install taoki@monet-plugins` (will fetch the new version automatically).

No cache changes — existing `.cache/taoki/` directories are unaffected.
