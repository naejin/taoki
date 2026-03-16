# Docstring Extraction for Index Tool

## Problem

Taoki's `index` tool extracts structural skeletons from source files — function signatures, type definitions, imports — but discards all documentation comments. This forces the coding agent to Read the full source file whenever it needs to understand what a function does, why a type exists, or what contracts an API enforces.

Docstrings are the single most information-dense artifact in a codebase that Taoki currently ignores. They capture intent, contracts, edge cases, and usage patterns in a form the author already compressed for a reader. Extracting them directly reduces the most expensive agent behavior: unnecessary file reads.

## Design

### Approach: First-line extraction

Extract the **first meaningful line** of each docstring and attach it to the skeleton entry. No structured tag parsing (no `@param`, `@returns`, etc.). The agent already has line ranges to drill deeper if needed.

This matches docstring conventions across all supported languages, where the first line is a summary sentence.

### Data Model

Add an optional `doc` field to `SkeletonEntry`:

```rust
pub(crate) struct SkeletonEntry {
    pub(crate) section: Section,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) text: String,
    pub(crate) children: Vec<String>,
    pub(crate) attrs: Vec<String>,
    pub(crate) doc: Option<String>,  // first line of docstring
}
```

### Extractor Trait

New method on `LanguageExtractor`:

```rust
fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String>;
```

Each language implements this with language-specific comment syntax stripping:

| Language | Node type | Doc marker | Strip rule |
|---|---|---|---|
| Rust | `line_comment` (sibling) | `///` | Strip `/// ` prefix |
| TypeScript/JS | `comment` (sibling) | `/**` | Strip `/** ` / ` * ` / ` */`, take first content line |
| Python | `expression_statement` > `string` (first child of body) | `"""` | Strip `"""` wrapper, take first non-empty line |
| Go | `comment` (sibling) | `//` | Strip `// ` prefix |
| Java | `block_comment` (sibling) | `/**` | Same as TypeScript/JS |

**Python exception:** Docstrings are the first statement inside the function/class body, not siblings. The Python extractor peeks into the body node rather than walking backward from siblings.

**All other languages:** Walk backward via `prev_sibling()` (same traversal as existing `doc_comment_start_line()`), collect all consecutive doc comment nodes, reverse the list (since backward walk finds topmost last), and extract the text from the first element. This yields the conventional summary line.

**Default trait implementation:** The sibling-walk logic is shared across Rust, TypeScript/JS, Go, and Java. Provide a default `extract_doc_line()` on `LanguageExtractor` that performs the backward walk, calls `is_doc_comment()` to find nodes, and delegates to a new `strip_doc_prefix(&self, text: &str) -> Option<String>` method for language-specific prefix removal. Only Python overrides `extract_doc_line()` entirely.

### Implementation Notes

**Go adjacency:** Go's `is_doc_comment()` treats all `//` comments as doc comments. In `extract_doc_line()`, verify the comment is adjacent to the item (comment end row + 1 >= item start row, with no blank-line gap). Go also uses `/* */` block comments — filter to `//` only, matching godoc convention.

**Python `decorated_definition`:** When `node` is a `decorated_definition`, unwrap to the inner `function_definition` or `class_definition` before looking for the docstring in the body.

**TypeScript/JS `export_statement`:** JSDoc comments are siblings of the `export_statement` wrapper, not the inner declaration. The backward sibling walk from `node` (which is the top-level child) works correctly without special handling.

**Universal `///` prefix in output:** All doc lines render with `///` regardless of source language. This is intentional — the index is a structural summary, not a source mirror.

### Call Site

In `index_source()`, after `extract_nodes()` returns entries for an item, the first entry gets `doc` populated:

```rust
if i == 0 {
    if let Some(doc_start) = doc_comment_start_line(child, source, extractor) {
        entry.line_start = entry.line_start.min(doc_start);
    }
    entry.doc = extractor.extract_doc_line(child, source);  // NEW
}
```

### Output Format

In `format_skeleton()`, doc lines render indented below the item header, prefixed with `///`:

```
fns:
  pub resolve_import(path: &str, repo_root: &Path) -> Option<String> [145-182]
    /// Best-effort resolution of import path to a file in the repo.
  pub build_dep_graph(repo: &Path) -> DepGraph [184-220]

types:
  pub struct Config [10-25]
    /// Application configuration loaded from environment.
    host: String
    port: u16
```

Doc line sits between the item header and its children, at the same indentation level as children. Items without docstrings render unchanged (no empty `///` lines).

### Scope

**Items that get docstrings:**
- Top-level functions
- Types (structs, enums, interfaces, type aliases)
- Traits
- Constants (only if documented)
- Classes

**Not methods:** Methods inside impls, classes, and traits are rendered as children (`Vec<String>`), not `SkeletonEntry` objects. They cannot carry docstrings without refactoring children to `Vec<SkeletonEntry>`. This is a deliberate scope limitation — method docs can be a follow-up if the top-level extraction proves valuable.

**Items that do NOT get docstrings:**
- Imports
- Module declarations
- Macro definitions
- Fields (children of types — too noisy)

**Truncation:** First line only, capped at 120 characters via existing `truncate()`. If the first line is empty (common in multi-line Python docstrings), take the next non-empty line.

### What does NOT change

- `code_map` output — enrichment summaries already serve a similar role at repo level
- Cache structure — docstrings are extracted at parse time alongside the skeleton, not cached separately. However, `CACHE_VERSION` must bump from 3 to 4 so that stale caches (whose skeletons lack `///` lines) auto-invalidate on first run
- `dependencies` output — unrelated
- `extract_public_api()` — returns type/function names only, no docstrings needed there

## Testing

- One new test per language extractor verifying `extract_doc_line()` returns correct content
- Update existing `*_all_sections` tests to include documented items and verify `///` lines appear in output
- Test that undocumented items produce no `///` line
- Test truncation at 120 chars
- Test Python multi-line docstring where summary is on line 2
- Test empty docstring (whitespace-only after prefix stripping)
- Test single-line JSDoc/Javadoc: `/** Does the thing. */`
- Test Go comment separated from item by blank line (should NOT be extracted)
