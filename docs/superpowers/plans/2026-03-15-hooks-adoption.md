# Hooks & Adoption Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hooks to make Claude and subagents consistently use Taoki tools before reading source files, plus polish items (new command, uninstall script, docs).

**Architecture:** Three command-based hooks in `hooks/hooks.json` (SessionStart for awareness, PreToolUse on Read for source file nudge, PreToolUse on Glob for code_map nudge). Two shell scripts for the PreToolUse hooks read stdin JSON and output structured hook responses.

**Tech Stack:** Bash, JSON (Claude Code hooks API)

---

## Chunk 0: Branch setup

### Task 0: Create the feature branch

- [ ] **Step 1: Create the branch from current master**

```bash
git checkout -b feat/hooks-adoption
```

---

## Chunk 1: Hooks system

### Task 1: Create hooks.json and hook scripts

**Files:**
- Create: `hooks/hooks.json`
- Create: `hooks/check-read.sh`
- Create: `hooks/check-glob.sh`

- [ ] **Step 1: Create hooks/hooks.json**

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "echo 'You have structural code intelligence tools available via the taoki plugin. Before reading source files, use: mcp__taoki__code_map (repo overview with tags), mcp__taoki__index (file skeleton with line numbers — 70-90% fewer tokens than reading), mcp__taoki__dependencies (import/export graph for impact analysis). Always call code_map first when exploring a codebase, and index before reading any source file.'"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Read",
        "hooks": [
          {
            "type": "command",
            "command": "bash ${CLAUDE_PLUGIN_ROOT}/hooks/check-read.sh"
          }
        ]
      },
      {
        "matcher": "Glob",
        "hooks": [
          {
            "type": "command",
            "command": "bash ${CLAUDE_PLUGIN_ROOT}/hooks/check-glob.sh"
          }
        ]
      }
    ]
  }
}
```

- [ ] **Step 2: Create hooks/check-read.sh**

```bash
#!/usr/bin/env bash
# PreToolUse hook for Read — nudges toward mcp__taoki__index for source files.
# Reads tool input JSON from stdin, checks file extension, outputs hook response.
# Always allows the operation — never blocks.

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"//;s/"$//')

if [ -z "$FILE_PATH" ]; then
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
  exit 0
fi

EXT="${FILE_PATH##*.}"

case "$EXT" in
  rs|py|pyi|ts|tsx|js|jsx|mjs|cjs|go|java)
    cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Consider calling mcp__taoki__index on this file first to get its structure with line numbers, then Read only the specific sections you need. This typically saves 70-90% of tokens."}}
EOF
    ;;
  *)
    echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}'
    ;;
esac
```

- [ ] **Step 3: Create hooks/check-glob.sh**

```bash
#!/usr/bin/env bash
# PreToolUse hook for Glob — nudges toward mcp__taoki__code_map for codebase exploration.
# Always allows the operation.

cat <<'EOF'
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"Tip: mcp__taoki__code_map gives you a one-line-per-file summary with heuristic tags like [entry-point], [tests], [data-models] — often faster than globbing for project structure."}}
EOF
```

- [ ] **Step 4: Make scripts executable**

Run: `chmod +x hooks/check-read.sh hooks/check-glob.sh`

- [ ] **Step 5: Verify hook scripts work**

Test check-read.sh with a source file:
```bash
echo '{"tool_input":{"file_path":"src/main.rs"}}' | bash hooks/check-read.sh
```
Expected: JSON with `additionalContext` containing the nudge message.

Test check-read.sh with a non-source file:
```bash
echo '{"tool_input":{"file_path":"README.md"}}' | bash hooks/check-read.sh
```
Expected: JSON with `permissionDecision: "allow"` and no `additionalContext`.

Test check-glob.sh:
```bash
echo '{}' | bash hooks/check-glob.sh
```
Expected: JSON with `additionalContext` containing the code_map tip.

- [ ] **Step 6: Commit**

```bash
git add hooks/
git commit -m "feat: add hooks for automatic Taoki tool adoption"
```

---

## Chunk 2: Skill description and new command

### Task 2: Update skill description for better triggering

**Files:**
- Modify: `skills/taoki-workflow.md:1-4`

- [ ] **Step 1: Update the description field**

In `skills/taoki-workflow.md`, replace the `description` line in the frontmatter:

Old:
```
description: "Use when starting ANY coding task in a project. Triggers on: implementing features, fixing bugs, refactoring, understanding code, exploring a repo, finding files to modify, planning changes, investigating issues. Use BEFORE Glob, Grep, Read, or Edit."
```

New:
```
description: "Use BEFORE reading source files. Call code_map for repo overview, index for file structure with line numbers, dependencies for impact analysis. Saves 70-90% tokens vs reading full files."
```

- [ ] **Step 2: Commit**

```bash
git add skills/taoki-workflow.md
git commit -m "feat: improve skill description for better triggering"
```

### Task 3: Create /taoki-deps command

**Files:**
- Create: `commands/taoki-deps.md`

- [ ] **Step 1: Create the command file**

```markdown
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
```

- [ ] **Step 2: Commit**

```bash
git add commands/taoki-deps.md
git commit -m "feat: add /taoki-deps command for dependency analysis"
```

---

## Chunk 3: Uninstall script and documentation

### Task 4: Create uninstall script

**Files:**
- Create: `scripts/uninstall.sh`

- [ ] **Step 1: Create scripts/uninstall.sh**

```bash
#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="$HOME/.claude/plugins/taoki"

# Colors
if [ -t 1 ]; then
  BOLD='\033[1m'
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  RESET='\033[0m'
else
  BOLD='' GREEN='' RED='' RESET=''
fi

info()  { echo -e "${BOLD}taoki:${RESET} $1"; }
error() { echo -e "${RED}error:${RESET} $1" >&2; }

if [ ! -d "$INSTALL_DIR" ]; then
  error "Taoki is not installed at $INSTALL_DIR"
  exit 1
fi

# Unregister plugin
if command -v claude >/dev/null 2>&1; then
  info "Unregistering plugin from Claude Code..."
  claude plugin remove taoki 2>/dev/null || true
fi

# Remove install directory
info "Removing $INSTALL_DIR..."
rm -rf "$INSTALL_DIR"

echo ""
info "${GREEN}Taoki uninstalled successfully.${RESET}"
```

- [ ] **Step 2: Make executable and verify syntax**

Run: `chmod +x scripts/uninstall.sh && bash -n scripts/uninstall.sh && echo "Syntax OK"`
Expected: `Syntax OK`

- [ ] **Step 3: Commit**

```bash
git add scripts/uninstall.sh
git commit -m "feat: add uninstall script"
```

### Task 5: Update release pipeline to include hooks

**Files:**
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Add hooks/ to both Package steps**

In `.github/workflows/release.yml`, in both the "Package (Unix)" and "Package (Windows)" steps, add a line to copy the hooks directory into the staging area. Add after the `cp -r skills staging/taoki/` line:

```bash
          cp -r hooks staging/taoki/
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: include hooks directory in release artifacts"
```

### Task 6: Update CLAUDE.md

**Note:** The test count changed from 29 to 31 in the benchmarks/expressions branch (2 new expression tests). Verify with `cargo test` before updating.

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update test count**

Change `# 29 unit tests` to `# 31 unit tests` (2 new expression tests were added).

- [ ] **Step 2: Add Hooks subsection**

After the "Distribution" section and before "Warning", add:

```markdown
## Hooks

Three hooks in `hooks/hooks.json` enforce Taoki tool usage:

- **SessionStart:** Injects a message at session start reminding Claude about the three code intelligence tools and when to use them.
- **PreToolUse (Read):** When Claude is about to Read a source file (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, etc.), injects a nudge suggesting `mcp__taoki__index` first. Does not block — always allows the Read.
- **PreToolUse (Glob):** When Claude uses Glob, injects a tip about `mcp__taoki__code_map` as an alternative for structural exploration. Does not block.

All hooks use command type (shell scripts) for zero-latency, deterministic behavior. Hook scripts are in `hooks/`.
```

- [ ] **Step 3: Update release artifacts list in Distribution section**

In the Distribution section, update the "Release artifacts include" bullet to add `hooks/`:

Old: `Release artifacts include: .claude-plugin/, commands/, skills/, scripts/run.sh, scripts/run.cmd, and the binary at target/release/taoki.`

New: `Release artifacts include: .claude-plugin/, commands/, skills/, hooks/, scripts/run.sh, scripts/run.cmd, and the binary at target/release/taoki.`

- [ ] **Step 4: Add exprs section note**

In the Architecture section, after the `index/` bullet point, add a note:

```
The `index` tool outputs sections: `imports:`, `consts:`, `exprs:` (top-level expressions for Python/TypeScript), `types:`, `traits:`, `impls:`, `fns:`, `classes:`, `mod:`, `macros:`, and `tests:`.
```

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with hooks, expression sections, and test count"
```

### Task 7: Update README.md with uninstall instructions

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add Uninstall section**

After the "Update" section and before "How It Works", add:

```markdown
## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/uninstall.sh | bash
```

Or manually: `rm -rf ~/.claude/plugins/taoki`
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add uninstall instructions to README"
```
