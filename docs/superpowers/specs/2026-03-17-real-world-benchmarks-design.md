# Real-World Project Benchmarks

## Goal

Validate taoki's claims of universality and accuracy against real open-source projects before each release. Provide users a published summary of results in the README so they can trust the tool works on codebases like theirs.

## Project Selection

15 projects across all 6 supported languages, plus one mixed-language repo. Each pinned to a specific git commit SHA for reproducibility.

### Rust (3)
- **ripgrep** — medium, idiomatic Rust, traits and generics
- **tokio** — large, async runtime, deep module hierarchy, macros
- **serde** — medium, proc macros, derive-heavy

### Python (3)
- **flask** — medium, classic Python, decorators, class-based views
- **fastapi** — medium, heavy type annotations, Pydantic models, async
- **black** — medium, AST manipulation, deeply nested code

### TypeScript (3)
- **next.js** — large, mixed TS/JS, monorepo
- **zod** — small/medium, generic-heavy type validation
- **trpc** — medium, monorepo, heavy generics, packages

### Go (2)
- **caddy** — large, plugin architecture, interfaces
- **cobra** — small/medium, clean Go patterns, widely used CLI library

### Java (2)
- **guava** — large, Google core libraries, heavy generics, annotations
- **spring-boot** — large, annotations, deep class hierarchies

### Mixed-language (1)
- **deno** — large, Rust + TypeScript + JavaScript

## Metrics & Pass/Fail Criteria

### A) No crashes/errors
- For every supported file in a project, call `index_source()` and `extract_all()`
- Track: files parsed successfully, files that errored, files skipped (unsupported extension / over 2MB)
- **Pass threshold: ≥99.5% parse success rate per project**

### B) Structural completeness
- For every file, call `extract_public_api()` and verify non-empty result
- Files >50 lines with 0 public types AND 0 public functions are flagged (excluding test files)
- **Pass threshold: ≤1% of substantial files (>50 lines) return completely empty API**

### C) Token efficiency
- For every file, compute `1.0 - (skeleton_bytes / source_bytes)`
- Report per-project average
- **Pass threshold: ≥50% average byte reduction per project**

### Overall
All projects must pass all three criteria. Binary exit code 0 if all pass, 1 if any fail.

## Implementation

### File structure

```
tools/
  benchmark.rs          # Rust binary
  repos.json            # project list: name, URL, SHA, language
```

### Cargo.toml addition

```toml
[[bin]]
name = "benchmark"
path = "tools/benchmark.rs"
```

### repos.json format

```json
[
  {
    "name": "ripgrep",
    "url": "https://github.com/BurntSushi/ripgrep",
    "sha": "<pinned-commit>",
    "lang": "Rust"
  }
]
```

### Binary behavior

```
cargo run --bin benchmark              # run all projects
cargo run --bin benchmark -- --update-pins   # update all SHAs to latest HEAD
```

Flow:
1. Read `repos.json`
2. For each project: clone (or reuse) into `tools/.cache/repos/<name>/`, checkout pinned SHA
3. Walk all supported files (reuse `codemap::walk_files_public`)
4. For each file: call `index_source()`, `extract_all()`, `extract_public_api()`, collect metrics
5. Compute per-project and aggregate stats
6. Print detailed results to stdout
7. Write summary table into `README.md` between `<!-- BENCH:START -->` and `<!-- BENCH:END -->` markers

### Commit pinning

- SHAs pinned in `repos.json` for reproducibility
- `--update-pins` fetches latest default branch HEAD for each repo, updates `repos.json`, and re-runs the benchmark

### Cache directory

`tools/.cache/repos/` holds cloned repos. Gitignored. Repos are reused across runs — the benchmark does `git fetch` + `git checkout <sha>` each time.

## README Integration

A new "## Benchmarks" section in README.md, placed before the Changelog:

```markdown
## Benchmarks

Tested against 15 open-source projects (run `cargo run --bin benchmark` to reproduce):

<!-- BENCH:START -->
| Project | Language | Files | Parsed | Parse % | Empty API | Reduction | Status |
|---------|----------|-------|--------|---------|-----------|-----------|--------|
| ripgrep | Rust | 142 | 142 | 100% | 0 | 78% | PASS |
| ... | | | | | | | |
<!-- BENCH:END -->

*Results from v0.x.x against pinned commits. Run `cargo run --bin benchmark -- --update-pins` to refresh.*
```

- "Empty API" = count of non-test files >50 lines with zero extracted types/functions
- "Reduction" = average byte reduction (skeleton vs source)
- "Status" = PASS/FAIL based on all three thresholds
- Script replaces only content between markers; surrounding text is static
