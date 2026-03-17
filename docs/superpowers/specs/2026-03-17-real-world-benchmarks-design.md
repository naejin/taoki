# Real-World Project Benchmarks

## Goal

Validate taoki's claims of universality and accuracy against real open-source projects before each release. Run manually before tagging a release. Provide users a published summary of results in the README so they can trust the tool works on codebases like theirs.

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

### Go (3)
- **caddy** — large, plugin architecture, interfaces
- **cobra** — small/medium, clean Go patterns, widely used CLI library
- **hugo** — large, complex template system, deep package hierarchy

### Java (2)
- **guava** — large, Google core libraries, heavy generics, annotations
- **spring-boot** — large, annotations, deep class hierarchies

### Mixed-language (1)
- **deno** — large, Rust + TypeScript + JavaScript

## Metrics & Pass/Fail Criteria

All metrics use a single `extract_all()` call per file. This returns `(PublicApi, String)` — the public API summary and the full structural skeleton — in one tree-sitter parse pass. No redundant parsing.

### A) No crashes/errors
- For every supported file in a project, call `extract_all()`
- Track: files parsed successfully, files that errored, files skipped (unsupported extension / over 2MB)
- **Pass threshold: ≥99.5% parse success rate per project**

### B) Structural completeness
- Check that the skeleton string (second element of `extract_all()`) is non-empty
- Files >50 lines with a completely empty skeleton are flagged (excluding test files detected by `is_test_filename`)
- Empty skeleton means zero structural content — no imports, no types, no functions, nothing. This is distinct from empty *public API*, which is normal for many languages (Python helpers, Go internal packages, JS files without exports)
- **Pass threshold: ≤1% of substantial files (>50 lines, non-test) return completely empty skeleton**

### C) Token efficiency
- For every file, compute `1.0 - (skeleton_bytes / source_bytes)` where `skeleton_bytes` is the length of the skeleton string from `extract_all()`
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
required-features = ["benchmark"]

[features]
benchmark = []
```

The `required-features` gate ensures `cargo build` does not compile the benchmark binary by default. Run with `cargo run --bin benchmark --features benchmark`.

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

The `lang` field is cosmetic (display only). File language detection uses extensions via `Language::from_extension()`, not this field. For mixed-language repos like deno, use a descriptive value like `"Rust, TS, JS"`.

### Binary behavior

```
cargo run --bin benchmark --features benchmark                       # run all projects
cargo run --bin benchmark --features benchmark -- --update-pins      # update SHAs only
```

`--update-pins` only updates `repos.json` with latest default branch HEADs. It does not run the benchmark. This keeps operations separate — update pins, review the diff, then run the benchmark.

On first run (or when SHAs are placeholder values), `--update-pins` must be run first to populate real commit SHAs.

### Benchmark flow

1. Read `repos.json`
2. For each project: shallow clone (`git clone --depth 1`) or reuse existing clone in `tools/.cache/repos/<name>/`. Fetch pinned SHA with `git fetch origin <sha> --depth 1` and `git checkout <sha>`
3. Walk all supported files (reuse `codemap::walk_files_public`). File counts reflect `.gitignore` filtering — matching what users see in practice
4. For each file: call `extract_all()` once, collect metrics from both returned values
5. Compute per-project and aggregate stats
6. Print detailed results to stdout
7. Write summary table into `README.md` between `<!-- BENCH:START -->` and `<!-- BENCH:END -->` markers

### Commit pinning

- SHAs pinned in `repos.json` for reproducibility
- `--update-pins` fetches latest default branch HEAD for each repo via `git ls-remote` and updates `repos.json`
- Developers should review the `repos.json` diff before committing updated pins

### Cache directory

`tools/.cache/repos/` holds shallow-cloned repos. Gitignored. Repos are reused across runs — the benchmark does `git fetch` + `git checkout <sha>` each time.

## README Integration

A new "## Benchmarks" section in README.md, placed before the Changelog:

```markdown
## Benchmarks

Tested against 15 open-source projects (run `cargo run --bin benchmark --features benchmark` to reproduce):

<!-- BENCH:START -->
| Project | Language | Files | Parsed | Parse % | Empty Skeletons | Reduction | Status |
|---------|----------|-------|--------|---------|-----------------|-----------|--------|
| ripgrep | Rust | 142 | 142 | 100% | 0 | 78% | PASS |
| ... | | | | | | | |
<!-- BENCH:END -->

*Results from v0.x.x against pinned commits. Run `cargo run --bin benchmark --features benchmark -- --update-pins` to refresh pins.*
```

- "Empty Skeletons" = count of non-test files >50 lines with completely empty skeleton output
- "Reduction" = average byte reduction (skeleton from `extract_all()` vs source)
- "Status" = PASS/FAIL based on all three thresholds
- Script replaces only content between markers; surrounding text is static
