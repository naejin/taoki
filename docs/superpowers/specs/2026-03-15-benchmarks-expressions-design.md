# Benchmarks & Top-Level Expression Enhancement Design

## Problem

Taoki claims 70-90% token savings but has no automated way to verify this. There are no speed benchmarks or completeness checks. Additionally, the `index` tool misses structurally important top-level expressions in Python and TypeScript (e.g., `app = Flask(__name__)`, `module.exports`, middleware registration), which means Claude doesn't see key architectural patterns when indexing files.

## Goal

1. Enhance `index` to capture top-level expressions in Python and TypeScript/JavaScript
2. Build a two-tier benchmark suite: `cargo bench` (criterion) for speed/correctness during development, plus a shell script for real-world token savings measurement

## Components

### 1. Top-Level Expression Enhancement

**New section type:** `Section::Expression` added to the `Section` enum in `index/mod.rs`. Displayed after Constants, before Functions in formatted output.

**Python extractor (`python.rs`) — new captures:**

- Named assignments at module level that aren't ALL_CAPS constants: `app = Flask(__name__)`, `router = APIRouter()`, `db = SQLAlchemy()`. These are lowercase/mixed-case identifiers with a value assignment.
- Function calls as expression statements that represent configuration: method calls on known patterns like `app.register_blueprint(...)`, `app.use(...)`. Captured as a one-line summary.
- `if __name__ == "__main__"` blocks: collapsed to a single line with line range, similar to test collapsing.

**TypeScript/JavaScript extractor (`typescript.rs`) — new captures:**

- Non-const top-level assignments: `module.exports = ...`, `exports.foo = ...`
- Method call chains at top level: `app.use(...)`, `app.get(...)`, `router.post(...)` — Express/Fastify middleware and route patterns. Captured as a one-line summary.
- IIFE patterns and `addEventListener` calls at top level.

**What is NOT captured (noise filter):**

- Bare `print()` / `console.log()` calls
- Standalone string literals (already handled as docstrings in Python)
- Comments (not expressions)
- Bare function calls with no observable structural pattern (e.g., `run()`)

**Output format:** Uses existing `SkeletonEntry` structure. `section: Section::Expression`, `text` contains a compact summary (e.g., `app = Flask(__name__)`), truncated to 80 chars.

### 2. Criterion Benchmarks (`cargo bench`)

**New dev dependency:** `criterion = "0.5"` in `Cargo.toml`, with `[[bench]]` entries and `harness = false`.

**Three benchmark files in `benches/`:**

**`benches/speed.rs`** — Tool latency:

- `index_source()` per language (Rust, Python, TypeScript, JavaScript, Go, Java) on ~200-line synthetic files
- `extract_public_api()` per language on the same files
- `build_code_map()` on a temp repo with 10-20 files (cold)
- `build_code_map()` on the same repo again (cache hit)

**`benches/completeness.rs`** — Structural coverage:

- For each language, a synthetic source file containing every structural pattern: imports, classes, functions, constants, traits/interfaces, enums, modules, top-level expressions (for Python/TS), test code
- Assertions that every expected symbol name appears in the `index_source()` output
- Assertions that every expected symbol appears in `extract_public_api()` output
- Assertions that test code is collapsed (shown as line range, not full body)
- Assertions that top-level expressions appear for Python and TypeScript

**`benches/token_ratio.rs`** — Token efficiency:

- For each language, measure `index_source()` output byte count vs input source byte count
- Report the compression ratio per language
- Use synthetic files of realistic size (200-500 lines)
- Also measure `extract_public_api()` output size vs source size

### 3. Real-World Benchmark Script

**File:** `scripts/benchmark.sh`

**Repos:**

| Repo | Language | Why |
|------|----------|-----|
| `pallets/flask` | Python | Well-structured, moderate size, heavy top-level expressions |
| `expressjs/express` | JavaScript | CommonJS patterns, middleware chains |
| `BurntSushi/ripgrep` | Rust | Large Rust project, good speed stress test |

**Flow:**

1. Clone repos into a temp directory (shallow `--depth 1`)
2. Build taoki in release mode if not already built
3. For each repo, invoke the taoki binary's `code_map` and `index` tools via MCP JSON-RPC over stdin/stdout
4. Measure and report:
   - Token ratio: total index output bytes / total source bytes, per language
   - Speed: wall-clock time for full `code_map` (cold), then again (cached)
   - File coverage: files indexed vs skipped
5. Output a markdown table to stdout
6. Clean up temp directory

**Example output:**

```
## Taoki Benchmark Results

| Repo | Language | Files | Source KB | Index KB | Reduction | Cold (ms) | Cached (ms) |
|------|----------|-------|-----------|----------|-----------|-----------|-------------|
| flask | Python | 42 | 180 | 28 | 84% | 320 | 12 |
| express | JS | 35 | 95 | 18 | 81% | 210 | 8 |
| ripgrep | Rust | 68 | 410 | 52 | 87% | 480 | 15 |
```

**Not run in CI** — requires network access. Intended for local use and updating README claims.

## What Doesn't Change

- MCP protocol / tool interface (tools return the same JSON structure)
- Cache format (`code-map.json`, `deps.json`)
- Existing 29 unit tests
- Distribution pipeline
- Supported languages list

## Execution Order

1. Top-level expressions first (changes what benchmarks measure)
2. Criterion benchmarks second (validates the extractors)
3. Benchmark script third (produces real-world numbers)
