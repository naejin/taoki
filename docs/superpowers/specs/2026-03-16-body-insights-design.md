# Body Insights: Function-Level Implementation Analysis

## Problem

Taoki's skeleton output gives function signatures, type definitions, and line numbers, but says nothing about what happens inside function bodies. An LLM reading skeleton output can navigate code and understand module boundaries, but cannot determine:

- What other functions a given function delegates to (call graph)
- What dispatch/routing logic exists (match/switch patterns)
- What error conditions a function signals (error construction sites)

This forces a full `Read` of every function body, defeating much of the token savings.

## Solution

Add three new kinds of body-level extraction to `SkeletonEntry`, powered by tree-sitter traversal of function bodies:

1. **Call graph** — unique function/method names called within a body
2. **Match/switch arms** — dispatch target + arm patterns (without bodies)
3. **Error returns** — error types constructed or raised/thrown

## Data Model

### New struct: `BodyInsights`

```rust
pub(crate) struct BodyInsights {
    pub(crate) calls: Vec<String>,
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
}

pub(crate) struct MatchInsight {
    pub(crate) target: String,
    pub(crate) arms: Vec<String>,
}
```

### `BodyInsights` shared formatting

`BodyInsights` provides a `format_lines() -> Vec<String>` method that produces the `→`-prefixed strings. This single method is used by both rendering paths (top-level entries and method children), ensuring consistent formatting:

```rust
impl BodyInsights {
    pub(crate) fn format_lines(&self) -> Vec<String> { ... }
    pub(crate) fn is_empty(&self) -> bool { ... }
}
```

### SkeletonEntry changes

Add one field:

```rust
pub(crate) struct SkeletonEntry {
    // ... existing fields ...
    pub(crate) insights: BodyInsights,
}
```

`BodyInsights` defaults to empty vecs. `SkeletonEntry::new()` initializes it empty.

### Two rendering paths (unified formatting)

- **Top-level functions** (`Section::Function`): `format_skeleton()` renders `entry.insights` by calling `insights.format_lines()` and writing each line at 4-space indent after the entry's children.
- **Methods inside impl/trait/class blocks**: Language extractors call `analyze_body()` per method, then call `insights.format_lines()` and append the resulting strings (with 2-space prefix for extra indent) to the `children: Vec<String>`. This keeps the method representation as `Vec<String>` without requiring a type change.

Both paths use the same `BodyInsights::format_lines()` method, so output is always consistent.

## Output Format

### Top-level functions

```
fns:
  build_code_map(root: &Path, globs: &[String], detail_files: &[String]) -> Result<String, CodeMapError> [405-587]
    → calls: walk_files, hash_file, load_cache, save_cache, extract_all, compute_tags
    → match: method → "initialize", "ping", "tools/list", "tools/call"
    → errors: CodeMapError::PathNotFound, CodeMapError::Io
```

### Methods in impl/trait/class blocks

Methods already appear as children (strings). Body insights for methods are appended as additional indented child strings with the `→` prefix:

```
impls:
  Language [35-68]
    pub from_extension(ext: &str) -> Option<Self> [36-46]
      → match: ext → "rs", "py", "pyi", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "java"
    pub(crate) ts_language(&self) -> tree_sitter::Language [48-57]
      → match: *self → Rust, Python, TypeScript, JavaScript, Go, Java
    extractor(&self) -> &dyn LanguageExtractor [59-67]
      → match: *self → Rust, Python, TypeScript, JavaScript, Go, Java
```

The `→` prefix distinguishes insight lines from structural children (method signatures, fields).

## Extraction Logic

### Architecture

New module: `src/index/body.rs`

Contains:
- `analyze_body(node: Node, source: &[u8], lang: Language) -> BodyInsights` — main entry point
- `extract_calls(node: Node, source: &[u8], lang: Language) -> Vec<String>` — recursive call finder
- `extract_match_arms(node: Node, source: &[u8], lang: Language) -> Vec<MatchInsight>` — match/switch finder
- `extract_error_returns(node: Node, source: &[u8], lang: Language) -> Vec<String>` — error site finder
- `walk_body(node: Node, visitor: &mut impl FnMut(Node))` — recursive body walker, shared utility

### Integration points

1. **`build_skeleton()` in `index/mod.rs`**: Inside the existing loop over `root.children()`, after calling `extractor.extract_nodes(child, source, &attrs)` and getting entries, for each entry where `entry.section == Section::Function`, call `analyze_body(child, source, lang)` on the current `child` Node (which is still in scope) and set `entry.insights`. This requires adding a `lang: Language` parameter to `build_skeleton()` — all call sites (`index_source`, `extract_all`) already have the `Language` available.

2. **Language extractors' `extract_methods()`**: After building each method's signature string, call `analyze_body()` on the method AST node's body. Call `insights.format_lines()` and append the resulting strings (with 2-space prefix) to the children vec.

3. **`format_skeleton()`**: After rendering each entry's children, if `entry.insights` is non-empty, call `entry.insights.format_lines()` and write each line at 4-space indent.

### Call graph extraction

Recursively walk the function body. At each node, check for call-like node kinds. Extract the callee name (final identifier segment only). Deduplicate and sort.

| Language | Call node kind | Callee extraction |
|----------|---------------|-------------------|
| Rust | `call_expression` | `function` field → last segment of identifier/scoped_identifier; for `field_expression` → `field` child |
| Python | `call` | `function` field → `identifier` text or `attribute` field of `attribute` node |
| TypeScript/JS | `call_expression` | `function` field → `identifier` text or `property` field of `member_expression` |
| Go | `call_expression` | `function` field → `identifier` text or `field` of `selector_expression` |
| Java | `method_invocation` | `name` field text |

**Filtering**: Include all unique callee names. Don't attempt to distinguish project-local vs external — that requires cross-file knowledge the index tool doesn't have. The consumer (LLM) can cross-reference with `code_map` output.

**Truncation**: If more than 12 unique calls, show first 12 and append `...` to signal truncation.

### Match/switch arm extraction

Find match/switch nodes in function bodies. For each, extract:
- **Target**: The expression being matched on (truncated to 30 chars)
- **Arms**: The pattern of each arm (truncated to 30 chars each)

| Language | Match node kind | Target field | Arm node kind | Pattern field |
|----------|----------------|--------------|---------------|---------------|
| Rust | `match_expression` | `value` field (scrutinee) | `match_arm` | First named children before `=>` (fallback: iterate named children if `pattern` field unavailable — verify against tree-sitter-rust 0.23 grammar at implementation time) |
| Python | `match_statement` | `subject` field | `case_clause` | First named child (pattern). Note: `match_statement` requires tree-sitter-python 0.23+ (PEP 634 support added ~0.21). Gracefully produce no results if grammar lacks support. |
| TypeScript/JS | `switch_statement` | `value` field | `switch_case` in `switch_body` | `value` field (or "default") |
| Go | `expression_switch_statement` | `value` field | `expression_case` | `value` field text; also `type_switch_statement` → `type_case` |
| Java | `switch_expression` / `switch_statement` | `condition` field → unwrap inner `parenthesized_expression` to get meaningful target text | `switch_block_statement_group` → `switch_label` | label value text |

**Truncation**: If more than 10 arms, show first 10 and append `...`.

### Error return extraction

Find error construction/signaling sites in function bodies.

| Language | Pattern | Extraction |
|----------|---------|------------|
| Rust | `call_expression` where callee is `Err` | Inner expression text (e.g., `CodeMapError::PathNotFound`) |
| Rust | `macro_invocation` where macro is `bail!`, `anyhow!`, `panic!`, `todo!`, `unimplemented!` | Macro name |
| Rust | `try_expression` (`?` operator) | Count of `?` usages, shown as "N error propagations via ?" rather than listing each |
| Python | `raise_statement` | First child (exception expression), extract type name only |
| TypeScript/JS | `throw_statement` | Child expression; if `new_expression`, extract the constructor name |
| Go | `call_expression` where callee is `errors.New` or `fmt.Errorf` in a return statement | "errors.New" / "fmt.Errorf" + first string arg if short |
| Java | `throw_statement` | Child expression; if `object_creation_expression`, extract the type name |

**Deduplication**: Same error type/expression appearing multiple times is shown once.

**Truncation**: Each error expression truncated to 40 chars. Max 8 unique errors shown.

## Body walker utility

A shared recursive walker that descends into the function body AST, visiting every node. Skips nested function definitions (closures, inner functions) to avoid mixing scopes.

```rust
fn walk_body(node: Node, visitor: &mut impl FnMut(Node)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Skip nested function definitions
        if is_function_def(child) {
            continue;
        }
        visitor(child);
        walk_body(child, visitor);
    }
}
```

`is_function_def()` checks for:
- Rust: `function_item`, `closure_expression`
- Python: `function_definition`, `lambda`
- TypeScript/JS: `function_declaration`, `arrow_function`, `function`
- Go: `func_literal`, `function_declaration`
- Java: `method_declaration`, `lambda_expression`

## Finding the function body node

Each language represents function bodies differently:

| Language | Function node kind | Body field/child |
|----------|-------------------|-----------------|
| Rust | `function_item` | `body` field (yields `block`) |
| Python | `function_definition` | `body` field (yields `block`) |
| TypeScript/JS | `function_declaration`, `method_definition`, `arrow_function` | `body` field |
| Go | `function_declaration`, `method_declaration` | `body` field (yields `block`) |
| Java | `method_declaration`, `constructor_declaration` | `body` field (yields `block`) |

The `analyze_body()` function first extracts the body node via `child_by_field_name("body")`, then runs the three extractors on it.

## Edge cases

- **Arrow function expression bodies (TS/JS)**: Concise arrow functions like `x => x + 1` have an expression body, not a block. `walk_body()` handles both — when the body is a single expression, it is still walked for calls.
- **Async/await patterns (TS/JS)**: `await fetch(...)` wraps a `call_expression` in an `await_expression`. The recursive walker naturally finds the inner `call_expression`, so async patterns work without special handling.
- **Double-counting in nested calls**: `foo(bar(x))` has two `call_expression` nodes, one nested inside the other's arguments. The walker visits both, which is correct — both `foo` and `bar` are extracted. Deduplication prevents repeated entries.
- **Codemap inline skeletons**: `build_code_map()`'s `detail_files` parameter includes inline skeletons via `extract_all()`. These will automatically include body insights as a consequence of the skeleton format change. No changes to `codemap.rs` are needed.

## Scope and constraints

- Only analyzes function/method bodies. Does not analyze top-level expressions, const initializers, or field defaults.
- Does not recurse into nested function definitions (closures, lambdas, inner functions).
- All output is best-effort — if tree-sitter can't parse a node, it's silently skipped.
- Token overhead target: ~15-25% increase over current skeleton size. The insights should remain much smaller than the full source.
- No new tree-sitter dependencies needed. All node kinds are available in the pinned 0.23 grammars.

## Files to create/modify

| File | Action |
|------|--------|
| `src/index/body.rs` | **Create**: `BodyInsights`, `MatchInsight`, `analyze_body()`, `extract_calls()`, `extract_match_arms()`, `extract_error_returns()`, `walk_body()` |
| `src/index/mod.rs` | **Modify**: Add `pub(crate) mod body;` declaration. Add `insights` field to `SkeletonEntry`. Update `SkeletonEntry::new()`. Update `format_skeleton()` to render insights. Call `analyze_body()` in `build_skeleton()`. |
| `src/index/languages/rust.rs` | **Modify**: In `extract_methods()`, call `analyze_body()` for each method and append insight strings to children. |
| `src/index/languages/python.rs` | **Modify**: In `extract_class()` method extraction loop, call `analyze_body()` and append insight strings. |
| `src/index/languages/typescript.rs` | **Modify**: In `extract_class()` method extraction loop, call `analyze_body()` and append insight strings. |
| `src/index/languages/go.rs` | **No changes needed**. Go methods are top-level `method_declaration` nodes, so they get insights through `build_skeleton()` directly. Interface methods have no bodies. |
| `src/index/languages/java.rs` | **Modify**: In class body method extraction, call `analyze_body()` and append insight strings. |

## Testing strategy

- Unit tests in `body.rs` for each extraction function, per language.
- Use inline source code strings (consistent with existing test patterns using `tempfile`).
- Test cases for each language covering: simple calls, chained/nested calls, match/switch with multiple arms, error construction, nested functions (should not leak), truncation behavior.
- Existing tests in `index/mod.rs` (the `*_all_sections` tests) should continue passing — the new insights are additive.

## Determinism and formatting rules

- **`format_lines()` ordering**: Always `calls`, then `match`, then `errors`. Empty sections are omitted entirely.
- **Calls sorting**: Lexicographic, case-sensitive sort before truncation. Ensures stable output regardless of AST traversal order.
- **Error returns ordering**: Preserve source order (reflects the function’s failure sequence). Deduplicate but don’t sort.
- **Match/switch arms ordering**: Preserve source order (reflects dispatch semantics — common cases first, default last).
- **Truncation style**: Use `…` (Unicode ellipsis) appended after the max character count, consistent with existing `truncate()` in `index/mod.rs`. Example: `"CodeMapError::PathNotFo…"` at 30 chars.
- **`?` propagation count**: Omit when zero. When nonzero, include as a single item in the errors line: `5× ?` alongside named error types.
- **`analyze_body()` input node**: Receives the function declaration node itself (e.g., `function_item`), not the body. Internally extracts the body via `child_by_field_name("body")`.
- **Method insight placement**: Insight lines are appended immediately after the method signature string in the children vec, before any other synthetic children.

## Testing strategy additions

- One negative test per language confirming nested functions/closures do not leak calls, match arms, or errors into the parent function’s insights.
