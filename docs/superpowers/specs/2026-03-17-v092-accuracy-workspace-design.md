# Taoki v0.9.2 — Accuracy & Workspace Improvements

**Date:** 2026-03-17
**Version:** 0.9.1 → 0.9.2

## Summary

Six targeted fixes addressing accuracy gaps and a significant functional limitation (workspace dependency resolution). All changes are backward-compatible — cache auto-invalidation handles the transition transparently.

## Motivation

Evaluation of the three tools (`index`, `code_map`, `dependencies`) across multiple repos and all 6 supported languages revealed:

- The `dependencies` tool is non-functional on Rust workspace projects (the majority of non-trivial Rust codebases)
- Java enum methods are silently dropped from skeletons
- Body insight `→ calls:` lines mix domain-relevant calls with stdlib plumbing, reducing signal
- Method call names lack receiver context, making them ambiguous
- `pub(crate)` items are invisible in `code_map` summaries
- Test section line ranges are inaccurate

None of these are regressions — they are gaps in the original implementation discovered through systematic evaluation.

## Backward Compatibility

All changes are safe for existing users updating from any prior version:

- **Cache auto-invalidation:** `CACHE_VERSION` bumps 5→6, `DEPS_VERSION` bumps 1→2. Existing caches at `.cache/taoki/` are silently discarded on first run and rebuilt. No user action needed, no errors.
- **Output format evolution:** The `index` tool's `→ calls:` line splits into `→ calls:` (free/scoped) and `→ methods:` (method calls with receiver context). This is display text consumed by LLMs, not a structured API. No downstream tooling parses this format.
- **Dependency graph structure:** The cached `deps.json` gains workspace resolution data. The version bump ensures old caches are rebuilt cleanly.
- **No new dependencies.** Workspace resolution uses simple string matching on `Cargo.toml` files — no TOML parser crate.

## Change 1: Workspace-Aware Dependency Resolution

**Files:** `src/deps.rs`
**Issues:** Rust `crate::` imports unresolved in workspaces; cross-crate imports (`maki_providers::Model`) classified as external; duplicate internal deps

### Problem

`resolve_rust()` assumes a single-crate layout where `crate::` maps to `src/`. In a workspace:

- `crate::cancel::CancelToken` in `maki-agent/src/agent.rs` should resolve to `maki-agent/src/cancel.rs`, not `src/cancel.rs`
- `maki_providers::Model` should resolve to `maki-providers/src/model.rs`, not be classified as external
- `taoki::mcp` in `src/main.rs` (binary importing its own lib crate) should resolve to `src/mcp.rs`

### Design

#### 1. Crate Map Construction

Add `build_crate_map(root: &Path) -> HashMap<String, PathBuf>`:

- Walk `root` for `**/Cargo.toml` files (using the `ignore` crate's `WalkBuilder`, same as `codemap.rs`)
- For each `Cargo.toml`, extract the crate name from the `[package]` section only:
  1. Find the `[package]` section header
  2. Read lines until the next `[` section header (or EOF)
  3. Within that range, match `name = "..."` using regex: `r#"^\s*name\s*=\s*"([^"]+)""#`
  - This avoids matching `name` fields in `[[bin]]`, `[dependencies]`, `[package.metadata]`, etc.
  - Virtual workspace `Cargo.toml` files (which have `[workspace]` but no `[package]`) are skipped gracefully — no `[package]` section means no match
- Map the crate name (with `-` normalized to `_`) to the directory containing that `Cargo.toml`
- Example: `maki-providers/Cargo.toml` with `name = "maki-providers"` → `{"maki_providers" => "maki-providers/"}`

#### 2. Find Crate Root for a File

Add `find_crate_root<'a>(file: &str, crate_map: &'a HashMap<String, PathBuf>) -> Option<(&'a str, &'a Path)>`:

- Iterate crate map entries and find the one whose directory path is a **prefix** of the file path (using `file.starts_with(crate_dir)`)
- If multiple crate dirs match (nested crates), pick the **longest** prefix (most specific)
- Returns `(crate_name, crate_dir)` for the nearest enclosing crate
- Example: `maki-agent/src/tools/bash.rs` → `("maki_agent", "maki-agent/")`

#### 3. Enhanced Rust Resolution

Replace `resolve_rust()` with `resolve_rust_workspace()`:

```
resolve_rust_workspace(
    import_path: &str,
    current_file: &str,
    all_files: &[String],
    crate_map: &HashMap<String, PathBuf>,
) -> Option<String>
```

Resolution logic:

- **`crate::foo::bar`**: Find the crate root for `current_file`, resolve `foo::bar` relative to that crate's `src/` directory
- **`some_crate::foo::bar`**: Look up `some_crate` in `crate_map`, resolve `foo::bar` relative to that crate's `src/` directory
- **Fallback**: If no crate map exists (single-crate repo), use existing `resolve_rust()` logic (unchanged behavior)

The candidate path generation is identical to the current approach — try `src/{path}.rs` then `src/{path}/mod.rs` — just rooted at the correct crate directory instead of always at repo root.

#### 4. Plumbing

- `build_deps_graph()` calls `build_crate_map(root)` once, passes it to the resolver
- `resolve_import()` gains an `Option<&HashMap<String, PathBuf>>` parameter for the crate map — `None` for non-Rust languages, `Some(&map)` for Rust. This keeps the existing signature compatible during incremental development and makes the fallback path explicit.
- `resolve_rust_workspace()` is called when `crate_map` is `Some`; falls back to existing `resolve_rust()` when `None` or when the map is empty
- For non-`crate::` imports (e.g., `maki_providers::Model`), the first path segment is looked up in the crate map. External deps like `serde::Serialize` will miss the map and return `None` (external) — this adds one HashMap lookup per external import, which is negligible.
- `extract_imports()` signature unchanged

#### 5. Dedup Fix

In `query_deps()`, add dedup for internal `depends_on`:

```rust
let mut depends_on: Vec<String> = /* existing code */;
depends_on.sort();
depends_on.dedup();
```

### Tests

- Workspace with two crates: `crate::` resolves within each crate
- Cross-crate import: `crate_b::types::Foo` resolves to `crate-b/src/types.rs`
- Single-crate repo: behavior unchanged (regression test)
- Binary importing own lib: `taoki::mcp` resolves to `src/mcp.rs`
- Duplicate import dedup in `query_deps`
- Virtual workspace `Cargo.toml` (no `[package]`) is skipped — no entry in crate map
- `Cargo.toml` with `[[bin]] name = "my-binary"` does not pollute the crate map
- File outside any workspace member: `find_crate_root` returns `None`, falls back to repo-root resolution
- Nested crates (e.g., workspace member with `examples/tool/Cargo.toml`): `find_crate_root` picks the longest prefix, resolving files in `examples/tool/src/` to the nested crate, not the parent

## Change 2: Java Enum Method Extraction

**Files:** `src/index/languages/java.rs`

### Problem

`extract_enum()` at `java.rs:187-211` iterates the enum body but only collects `enum_constant` nodes. Methods like `canEdit()` inside `Role` are silently dropped.

### Design

Replace the current single-purpose loop with a single-pass iteration that dispatches on `child.kind()`, collecting constants first, then methods and fields — same pattern as `extract_class_body()`:

```rust
let mut constants = Vec::new();
let mut members = Vec::new();
let mut cursor = body.walk();
for child in body.children(&mut cursor) {
    match child.kind() {
        "enum_constant" => {
            let cname = child.child_by_field_name("name")
                .map(|n| node_text(n, source)).unwrap_or("_");
            constants.push(cname.to_string());
        }
        "method_declaration" | "constructor_declaration" => {
            let sig = self.method_signature(child, source);
            let lr = line_range(...);
            members.push(format!("{sig} {lr}"));
            let insights = body::analyze_body(child, source, Language::Java);
            for line in insights.format_lines() {
                members.push(format!("  {line}"));
            }
        }
        "field_declaration" => {
            if members.len() < FIELD_TRUNCATE_THRESHOLD {
                let text = self.field_text(child, source);
                let lr = line_range(...);
                members.push(format!("{text} {lr}"));
            }
        }
        _ => {}
    }
}
// Combine: constants first, then methods/fields
constants.extend(members);
entry.children = constants;
```

Constants are listed first (they define the enum's domain), then methods/fields follow. `FIELD_TRUNCATE_THRESHOLD` applies to the members list (same as `extract_class_body`).

### Tests

- Java enum with methods: `Role { ADMIN; boolean canEdit() { ... } }` → skeleton shows both variants and method with body insights
- Java enum with no methods: unchanged behavior (constants only)
- Java enum with fields and constructor: all captured
- Java enum with >8 fields: truncation applies to members (not constants)

## Change 3: Test Range End-Line Fix

**Files:** `src/index/mod.rs`

### Problem

`build_skeleton:497` pushes only `child.start_position().row + 1` to `test_lines`. In `format_skeleton`, the range `min(starts)..max(starts)` misses the last test function's body. Example: Python tests at lines 140-142 and 145-148 show as `tests: [140-145]` instead of `tests: [140-148]`.

### Design

Change `test_lines` from `Vec<usize>` to `Vec<(usize, usize)>` (start, end pairs):

```rust
// build_skeleton, line 497:
test_lines.push((
    child.start_position().row + 1,
    child.end_position().row + 1,
));
```

In `format_skeleton`:

```rust
if !test_lines.is_empty() {
    let min = test_lines.iter().map(|(s, _)| *s).min().unwrap();
    let max = test_lines.iter().map(|(_, e)| *e).max().unwrap();
    // ...
}
```

### Tests

- Existing test assertions that check `tests: [N]` ranges need updating to reflect correct end lines
- Add a test with multiple test functions verifying the range spans from first start to last end

## Change 4: `pub(crate)` in Code Map

**Files:** `src/index/languages/rust.rs`

### Problem

`extract_public_api()` at `rust.rs:283` checks `vis_prefix(child, source) == "pub"`, excluding `pub(crate)` items. Files like `body.rs` (1457 lines, all `pub(crate)`) show as `public_types: (none) - public_functions: (none)`.

### Design

Change the visibility check from exact match to prefix match:

```rust
// rust.rs:283 and :290
if vis_prefix(child, source).starts_with("pub") {
```

This captures `pub`, `pub(crate)`, and `pub(super)` — all meaningful API surface within a repo. No marker or prefix is added to the output; the one-liner's purpose is discoverability, not visibility documentation.

**Naming:** The function `extract_public_api` and the field names `public_types`/`public_functions` in code_map output are not renamed. "Public" here means "visible API" in the context of an agent navigating a repo — the code_map output is consumed by LLMs, not compiled. The CLAUDE.md description of code_map should update from "public API summary" to "visible API summary" to reflect this.

### Tests

- File with only `pub(crate)` items: types and functions appear in code_map
- File with mix of `pub` and `pub(crate)`: both appear
- File with `pub(super)`: also captured
- Private items (no visibility keyword): still excluded

## Change 5: Split Calls/Methods in Body Insights

**Files:** `src/index/body.rs`, `src/index/mod.rs`

### Problem

`→ calls:` mixes free/scoped calls (domain logic) with method calls (often plumbing like `push`, `clone`, `iter`). With a budget of 12, plumbing frequently crowds out signal.

### Design

#### Struct Changes

```rust
pub(crate) struct BodyInsights {
    pub(crate) calls: Vec<String>,        // free/scoped calls only
    pub(crate) method_calls: Vec<String>,  // method calls only (NEW)
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
    pub(crate) try_count: usize,
}
```

#### Extraction Changes

`extract_calls()` already separates into `primary` (BTreeSet) and `methods` (BTreeSet). Change the return type to `(Vec<String>, Vec<String>)`:

```rust
fn extract_calls(body: Node, source: &[u8], lang: Language) -> (Vec<String>, Vec<String>) {
    // ... existing walk_body logic ...
    let calls: Vec<String> = primary.into_iter().map(String::from).collect();
    let methods: Vec<String> = methods.into_iter().map(String::from).collect();
    (calls, methods)
}
```

#### Display

```rust
// In format_lines():
if !self.calls.is_empty() {
    // "→ calls: HashMap::new, build_code_map, Ok"
    lines.push(format!("→ calls: {}", ...));
}
if !self.method_calls.is_empty() {
    // "→ methods: clone, get, is_empty, push"
    lines.push(format!("→ methods: {}", ...));
}
```

#### Wiring in `analyze_body`

```rust
pub(crate) fn analyze_body(node: Node, source: &[u8], lang: Language) -> BodyInsights {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return BodyInsights::default(),
    };
    let (calls, method_calls) = extract_calls(body, source, lang);
    let match_arms = extract_match_arms(body, source, lang);
    let (error_returns, try_count) = extract_error_returns(body, source, lang);
    BodyInsights { calls, method_calls, match_arms, error_returns, try_count }
}
```

#### Budget

- `MAX_CALLS` (12) applies to free/scoped calls
- Add `MAX_METHODS: usize = 8` for method calls
- Total budget increases slightly (12+8=20 vs 12) but each tier is more focused

#### Interaction with Change 6

After Change 6, the `methods` BTreeSet will contain strings like `items.push` instead of `push`. This affects deduplication: `self.a.push(x)` and `self.b.push(x)` become `a.push` and `b.push` (two entries), while `self.items.push(x)` called twice still deduplicates to one `items.push`. This is the desired behavior — the receiver context adds meaningful disambiguation.

### Tests

- Function with mixed calls: verify `→ calls:` contains only free/scoped, `→ methods:` contains only method calls
- Function with only free calls: no `→ methods:` line emitted
- Function with only method calls: no `→ calls:` line emitted
- Budget overflow: verify truncation with `...` in each tier independently

## Change 6: Method Receiver Context

**Files:** `src/index/body.rs`

### Problem

`resp.Body.Close()` → `Close`. One-word method names like `Close`, `Do`, `get` are ambiguous without their receiver.

### Design

**Rule:** Include one level of receiver context when the receiver expression is itself a compound access (depth >= 2 in the AST). When the receiver is a simple identifier (`self`, `this`, a local variable), no prefix is added. The check is purely structural: does the receiver node's kind indicate a compound expression?

In `extract_callee_name()`:

**Rust** (`field_expression`):
```rust
"field_expression" => {
    let field = func.child_by_field_name("field")?;
    let value = func.child_by_field_name("value")?;
    let prefix = match value.kind() {
        "field_expression" => {
            value.child_by_field_name("field")
                .map(|f| node_text(f, source))
        }
        _ => None,
    };
    let name = match prefix {
        Some(p) => format!("{}.{}", p, node_text(field, source)),
        None => node_text(field, source).to_string(),
    };
    Some((name, true))  // note: name is now owned
}
```

**Python** `attribute` → check if `object` is also `attribute`, take its `attribute` field
**TypeScript/JS** `member_expression` → check if `object` is also `member_expression`, take its `property`
**Go** `selector_expression` → check if `operand` is also `selector_expression`, take its `field`
**Java** `method_invocation` → check if `object` is `field_access`, take its `field` child (Java's tree-sitter uses `field_access` with a `field` field name for the rightmost segment)

**Examples:**
- `self.items.push(x)` → `items.push` (receiver `self.items` is `field_expression`, prefix = `items`)
- `items.push(x)` → `push` (receiver `items` is `identifier`, no prefix)
- `resp.Body.Close()` → `Body.Close` (receiver `resp.Body` is `selector_expression`)
- `foo()` → `foo` (not a method call, unchanged)
- `a.b.c.d()` → `c.d` (only one level of receiver, not the full chain)

**Known limitation — Go package calls:** In Go, `fmt.Println("hello")` parses as a `selector_expression` and is classified as a method call (`is_method = true`). The operand `fmt` is an `identifier`, so no prefix is added — the result is `Println`. This is a pre-existing classification issue (package-qualified free functions look like method calls in Go's AST). Change 6 does not make it worse, and the correct fix (distinguishing packages from values) would require type information that tree-sitter doesn't provide.

**Return type:** `extract_callee_name` changes from `(&'a str, bool)` to `(String, bool)` since we may construct a new string with the prefix. This is not a performance concern — `truncate()` in `extract_calls` already allocates a `String` unconditionally for each call name.

### Tests

- Chained field access: `a.b.c()` → `b.c` (all 6 languages)
- Simple receiver: `x.foo()` → `foo`
- Self receiver: `self.bar()` → `bar` (Rust), `this.bar()` → `bar` (TS/Java)
- Double chain: `self.client.get()` → `client.get`
- Triple chain: `a.b.c.d()` → `c.d` (only one level)
- Free call: `baz()` → `baz` (unchanged)
- Go package call: `fmt.Println()` → `Println` (no prefix, known limitation)
- Truncation: combined `prefix.name` exceeding 40 chars is truncated correctly
- Each language gets at least one test

## Release Checklist

| File | Change |
|------|--------|
| `Cargo.toml` | version: 0.9.1 → 0.9.2 |
| `.claude-plugin/plugin.json` | version: 0.9.1 → 0.9.2 |
| `src/codemap.rs:37` | `CACHE_VERSION`: 5 → 6 |
| `src/deps.rs:28` | `DEPS_VERSION`: 1 → 2 |
| `CLAUDE.md` | Update: "public API summary" → "visible API summary"; document `→ calls:` / `→ methods:` split; document workspace dependency resolution; note receiver context in body insights |
| `README.md` | Update version and changelog |

## Implementation Order

1. **Change 3** (test range fix) — smallest, isolated, warms up the test infrastructure
2. **Change 2** (Java enum methods) — small, isolated
3. **Change 4** (`pub(crate)` in code_map) — one-line change + tests
4. **Change 5** (split calls/methods) — body.rs refactor, touches format_lines and extract_calls
5. **Change 6** (method receiver context) — builds on Change 5's refactor
6. **Change 1** (workspace deps) — largest, most independent

7. **Release**: version bumps, cache version bumps, doc updates
