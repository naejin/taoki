# Benchmarks & Top-Level Expression Enhancement Design

## Problem

Taoki claims 70-90% token savings but has no automated way to verify this. There are no speed benchmarks or completeness checks. Additionally, the `index` tool misses structurally important top-level expressions in Python and TypeScript (e.g., `app = Flask(__name__)`, `module.exports`, middleware registration), which means Claude doesn't see key architectural patterns when indexing files.

## Goal

1. Enhance `index` to capture top-level expressions in Python and TypeScript/JavaScript
2. Build a two-tier benchmark suite: `cargo bench` (criterion) for speed/correctness during development, plus a shell script for real-world token savings measurement

## Components

### 1. Top-Level Expression Enhancement

**New section type:** `Section::Expression` added to the `Section` enum in `index/mod.rs`. Insert between `Constant` and `Type` in the enum declaration (since derive `Ord` controls display order). Section header: `"exprs:"`. Display order becomes: Import, Constant, Expression, Type, Trait, Impl, Function, Class, Module, Macro.

**Note:** This changes the `index` tool output format (adds a new `exprs:` section). The `code_map` tool output is unaffected — it uses `extract_public_api`, not `index_source`. No cache format changes.

**Python extractor (`python.rs`) — new captures:**

- Named assignments at module level that aren't ALL_CAPS constants: `app = Flask(__name__)`, `router = APIRouter()`, `db = SQLAlchemy()`. These are lowercase/mixed-case identifiers with a value assignment. Dunder variables (`__all__`, `__version__`) are also captured.
- Method call expressions at module level (tree-sitter node: `expression_statement` containing a `call_expression` where the function is a `member_expression` / `attribute` — i.e., has a `.` in it): `app.register_blueprint(...)`, `db.init_app(...)`. This is the concrete rule: **any call expression with a dotted receiver at module level**.
- `if __name__ == "__main__"` blocks: this is an `if_statement` node (not an expression), matched by checking that the condition contains `__name__` and `"__main__"`. Collapsed to a single line with line range, similar to test collapsing. Requires adding `"if_statement"` as a match arm in `extract_nodes`.

**TypeScript/JavaScript extractor (`typescript.rs`) — new captures:**

- `expression_statement` containing `assignment_expression`: `module.exports = ...`, `exports.foo = ...`. This is the tree-sitter node type for non-declaration assignments.
- `let`/`var` declarations at module level (`lexical_declaration` with kind `let` or `var`): `let app = express()`. Currently only `const` is captured.
- Method call expressions at module level (same dotted-receiver rule as Python): `app.use(...)`, `app.get(...)`, `router.post(...)`.
- `export default` expressions: `export default class ...`, `export default function ...`, and `export default <expression>`.

**What is NOT captured (noise filter):**

- Simple function calls without a dotted receiver: `print("hello")`, `run()`, `setup()` — bare `call_expression` nodes where the function is a plain identifier, not a member access.
- Dotted calls on known noisy receivers: `console.log(...)`, `console.warn(...)`, `process.exit(...)`, `logging.debug(...)`, `logging.info(...)`, `logging.warning(...)` — excluded via a skip list of receiver identifiers: `console`, `process`, `logging`, `log`, `logger`, `Math`, `Object`, `Array`, `JSON`.
- Standalone string literals (already handled as docstrings in Python)
- Comments (not expressions)

**Output format:** Uses existing `SkeletonEntry` structure. `section: Section::Expression`, `text` contains a compact summary (e.g., `app = Flask(__name__)`), truncated to 80 chars.

### 2. Criterion Benchmarks (`cargo bench`)

**New dev dependency:** `criterion = "0.5"` in `Cargo.toml`, with `[[bench]]` entries and `harness = false`.

**Three benchmark files in `benches/`:**

**`benches/speed.rs`** — Tool latency:

- `index_source()` per language (Rust, Python, TypeScript, JavaScript, Go, Java) on ~200-line synthetic files
- `extract_public_api()` per language on the same files
- `build_code_map()` on a temp repo with 10-20 files (cold)
- `build_code_map()` on the same repo again (cache hit)

**Completeness tests** (inline `#[cfg(test)]`, not criterion — these are correctness checks, not benchmarks):

- New unit tests alongside existing ones in `index/mod.rs` and the language-specific files
- For Python and TypeScript, synthetic source files containing every structural pattern including top-level expressions
- Assert that every expected symbol name appears in the `index_source()` output
- Assert that top-level expressions appear in the `exprs:` section for Python and TypeScript
- Assert that test code is collapsed

**`benches/token_ratio.rs`** — Byte efficiency:

- For each language, measure `index_source()` output byte count vs input source byte count
- Report the compression ratio per language (note: byte ratio, not tokenizer-based — a reasonable proxy for token savings)
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
3. For each repo, invoke the taoki binary's `code_map` tool via MCP JSON-RPC over stdin/stdout using bare JSONL framing (send `{"jsonrpc":"2.0","id":1,"method":"initialize",...}` first to bootstrap, then `tools/call`). For `index`, invoke per-file on a sample of files.
4. Measure and report:
   - Byte ratio: total index output bytes / total source bytes, per language (proxy for token savings)
   - `code_map` speed: wall-clock time for full scan (cold), then again (cached)
   - File coverage: files indexed vs skipped
5. Output a markdown table to stdout
6. Clean up temp directory

**Example output:**

```
## Taoki Benchmark Results

| Repo | Language | Files | Source KB | Index KB | Byte Reduction | code_map Cold (ms) | code_map Cached (ms) |
|------|----------|-------|-----------|----------|----------------|---------------------|----------------------|
| flask | Python | 42 | 180 | 28 | 84% | 320 | 12 |
| express | JS | 35 | 95 | 18 | 81% | 210 | 8 |
| ripgrep | Rust | 68 | 410 | 52 | 87% | 480 | 15 |
```

**Not run in CI** — requires network access. Intended for local use and updating README claims.

**Note on `index` caching:** `index` uses an in-memory `INDEX_CACHE` that only persists within a single process. There is no meaningful "cached" mode across separate invocations. The script measures `index` speed as a per-file aggregate, not cold vs cached.

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
