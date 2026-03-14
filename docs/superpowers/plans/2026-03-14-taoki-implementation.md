# Taoki Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Claude Code plugin with a Rust MCP server providing two code-intelligence tools: `index` (single-file structural skeleton) and `code_map` (repo-wide structural map with blake3 caching).

**Architecture:** Port maki-code-index's tree-sitter extraction into a standalone MCP server binary. The `index` tool is a direct port of maki's skeleton extraction. The `code_map` tool is new — it reuses tree-sitter extraction but only keeps public types/functions, adds blake3 caching, and outputs a flat per-file summary. The MCP server is hand-rolled JSON-RPC over stdio (4 methods: initialize, notifications/initialized, tools/list, tools/call), using Content-Length framing (not line-delimited JSON).

**Tech Stack:** Rust, tree-sitter 0.26, tree-sitter language grammars 0.23, blake3, ignore crate, globset, serde/serde_json, thiserror

**Spec:** `doc/specs/2026-03-14-taoki-design.md`

**Reference implementation:** `/home/daylon/projects/maki/maki/maki-code-index/src/` (direct port source for index tool)

**Platform scope:** macOS/Linux only for v0.1.0 (bash launcher). Windows support is out of scope unless explicitly added (PowerShell launcher + .mcp.json selection).

---

## Chunk 1: Project Scaffold + Core Types

### Task 1: Project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.claude-plugin/plugin.json`
- Create: `.mcp.json`
- Create: `scripts/run.sh`
- Create: `.gitignore`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "taoki"
version = "0.1.0"
edition = "2021"

[dependencies]
blake3 = "1"
fs2 = "0.4"
globset = "0.4"
ignore = "0.4"
rayon = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tree-sitter = "0.26"
tree-sitter-rust = "0.23"
tree-sitter-python = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-go = "0.23"
tree-sitter-java = "0.23"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create .claude-plugin/plugin.json**

```json
{
  "name": "taoki",
  "version": "0.1.0",
  "description": "Code indexing and structural mapping for Claude Code",
  "author": {
    "name": "Daylon"
  },
  "keywords": ["code-index", "code-map", "tree-sitter"]
}
```

- [ ] **Step 3: Create .mcp.json**

```json
{
  "taoki": {
    "command": "${CLAUDE_PLUGIN_ROOT}/scripts/run.sh",
    "args": []
  }
}
```

- [ ] **Step 4: Create scripts/run.sh**

```bash
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"
if [ ! -f "$BIN" ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
fi
exec "$BIN" "$@"
```

Make executable: `chmod +x scripts/run.sh`

- [ ] **Step 5: Create .gitignore**

```
/target
.cache/taoki/
```

- [ ] **Step 6: Create minimal src/main.rs**

```rust
fn main() {
    eprintln!("taoki MCP server starting");
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: BUILD SUCCESS

- [ ] **Step 8: Commit**

```bash
git init && git add -A && git commit -m "feat: project scaffold with Cargo.toml, plugin manifest, and build script"
```

---

### Task 2: Core types and helpers (index module)

Port `common.rs` from maki-code-index. This defines the shared types and formatting used by all language extractors.

**Files:**
- Create: `src/index/mod.rs`
- Create: `src/index/languages/mod.rs`
- Modify: `src/main.rs` (add `mod index;`)

- [ ] **Step 1: Write test for line_range helper**

Create `src/index/mod.rs` with:

```rust
pub(crate) fn line_range(start: usize, end: usize) -> String {
    if start == end {
        format!("[{start}]")
    } else {
        format!("[{start}-{end}]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_range_single() {
        assert_eq!(line_range(5, 5), "[5]");
    }

    #[test]
    fn line_range_span() {
        assert_eq!(line_range(5, 10), "[5-10]");
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test line_range`
Expected: 2 tests PASS

- [ ] **Step 3: Write test for truncate helper**

Add to `src/index/mod.rs` tests:

```rust
    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello", 60), "hello");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let long = "a".repeat(70);
        let result = truncate(&long, 60);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 60);
    }

    #[test]
    fn truncate_preserves_boundaries() {
        let long = format!("{}{}", "a".repeat(55), "b".repeat(10));
        let result = truncate(&long, 60);
        assert!(result.ends_with("..."));
    }
```

- [ ] **Step 4: Implement truncate**

Add above tests in `src/index/mod.rs`:

```rust
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let boundary = s
        .char_indices()
        .nth(max_chars.saturating_sub(3))
        .map_or(s.len(), |(i, _)| i);
    format!("{}...", &s[..boundary])
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test truncate`
Expected: 3 tests PASS

- [ ] **Step 6: Add all core types and remaining helpers**

Add to `src/index/mod.rs` (above the tests module):

```rust
use std::fmt::Write;
use std::path::Path;

use tree_sitter::{Node, Parser};

mod languages;

pub(crate) const FIELD_TRUNCATE_THRESHOLD: usize = 8;
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("unsupported file type: {0}")]
    UnsupportedLanguage(String),
    #[error("file too large ({size} bytes, max {max})")]
    FileTooLarge { size: u64, max: u64 },
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: tree-sitter failed to parse file")]
    ParseFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Java,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            _ => None,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    fn extractor(&self) -> &dyn LanguageExtractor {
        match self {
            Self::Rust => &languages::rust::RustExtractor,
            Self::Python => &languages::python::PythonExtractor,
            Self::TypeScript | Self::JavaScript => &languages::typescript::TsJsExtractor,
            Self::Go => &languages::go::GoExtractor,
            Self::Java => &languages::java::JavaExtractor,
        }
    }
}

pub(crate) fn node_text<'a>(node: Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

pub(crate) fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

pub(crate) fn prefixed(vis: &str, rest: std::fmt::Arguments<'_>) -> String {
    if vis.is_empty() {
        format!("{rest}")
    } else {
        format!("{vis} {rest}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Import,    // "imports:"
    Constant,  // "consts:"
    Type,      // "types:"
    Trait,     // "traits:"
    Impl,      // "impls:"
    Function,  // "fns:"
    Class,     // "classes:"
    Module,    // "mod:" — spec places this after fns
    Macro,     // "macros:"
    // Note: Test is NOT in this enum. Tests are handled separately
    // via test_lines in format_skeleton to collapse them to line numbers.
}

impl Section {
    pub(crate) fn header(self) -> &'static str {
        match self {
            Self::Import => "imports:",
            Self::Constant => "consts:",
            Self::Type => "types:",
            Self::Trait => "traits:",
            Self::Impl => "impls:",
            Self::Function => "fns:",
            Self::Class => "classes:",
            Self::Module => "mod:",
            Self::Macro => "macros:",
        }
    }
}

pub(crate) struct SkeletonEntry {
    pub(crate) section: Section,
    pub(crate) line_start: usize,
    pub(crate) line_end: usize,
    pub(crate) text: String,
    pub(crate) children: Vec<String>,
    pub(crate) attrs: Vec<String>,
}

impl SkeletonEntry {
    pub(crate) fn new(section: Section, node: Node, text: String) -> Self {
        Self {
            section,
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            text,
            children: Vec::new(),
            attrs: Vec::new(),
        }
    }
}

pub(crate) struct PublicApi {
    pub(crate) types: Vec<String>,
    pub(crate) functions: Vec<String>,
}

pub(crate) trait LanguageExtractor {
    fn extract_nodes(&self, node: Node, source: &[u8], attrs: &[Node]) -> Vec<SkeletonEntry>;
    fn is_test_node(&self, node: Node, source: &[u8], attrs: &[Node]) -> bool;
    fn is_doc_comment(&self, node: Node, source: &[u8]) -> bool;
    fn is_module_doc(&self, node: Node, source: &[u8]) -> bool;
    fn extract_public_api(&self, root: Node, source: &[u8]) -> PublicApi;
    fn is_attr(&self, _node: Node) -> bool {
        false
    }
    fn collect_preceding_attrs<'a>(&self, node: Node<'a>) -> Vec<Node<'a>> {
        let mut attrs = Vec::new();
        let mut prev = node.prev_sibling();
        while let Some(p) = prev {
            if self.is_attr(p) {
                attrs.push(p);
            } else {
                break;
            }
            prev = p.prev_sibling();
        }
        attrs.reverse();
        attrs
    }
}
```

- [ ] **Step 7: Add format_skeleton and import consolidation**

Add to `src/index/mod.rs` (after LanguageExtractor trait):

```rust
fn doc_comment_start_line(
    node: Node,
    source: &[u8],
    extractor: &dyn LanguageExtractor,
) -> Option<usize> {
    let mut earliest: Option<usize> = None;
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if extractor.is_attr(p) {
            prev = p.prev_sibling();
            continue;
        }
        if extractor.is_doc_comment(p, source) {
            earliest = Some(p.start_position().row + 1);
            prev = p.prev_sibling();
        } else {
            break;
        }
    }
    earliest
}

fn detect_module_doc(
    root: Node,
    source: &[u8],
    extractor: &dyn LanguageExtractor,
) -> Option<(usize, usize)> {
    let mut cursor = root.walk();
    let mut start = None;
    let mut end = None;
    for child in root.children(&mut cursor) {
        if extractor.is_module_doc(child, source) {
            let line = child.start_position().row + 1;
            if start.is_none() {
                start = Some(line);
            }
            let end_pos = child.end_position();
            let end_line = if end_pos.column == 0 {
                end_pos.row
            } else {
                end_pos.row + 1
            };
            end = Some(end_line);
        } else if !extractor.is_attr(child) && !child.is_extra() {
            break;
        }
    }
    start.map(|s| (s, end.unwrap()))
}

pub(crate) fn format_skeleton(
    entries: &[SkeletonEntry],
    test_lines: &[usize],
    module_doc: Option<(usize, usize)>,
) -> String {
    use std::collections::BTreeMap;

    let mut out = String::new();

    if let Some((start, end)) = module_doc {
        let _ = writeln!(out, "module doc: {}", line_range(start, end));
    }

    let mut grouped: BTreeMap<Section, Vec<&SkeletonEntry>> = BTreeMap::new();
    for entry in entries {
        grouped.entry(entry.section).or_default().push(entry);
    }

    for (section, items) in &grouped {
        if section == &Section::Import {
            format_imports(&mut out, items);
        } else {
            let sep = if out.is_empty() { "" } else { "\n" };
            let _ = writeln!(out, "{sep}{}", section.header());
            for entry in items {
                for attr in &entry.attrs {
                    let _ = writeln!(out, "  {attr}");
                }
                let _ = writeln!(
                    out,
                    "  {} {}",
                    entry.text,
                    line_range(entry.line_start, entry.line_end)
                );
                for child in &entry.children {
                    let _ = writeln!(out, "    {child}");
                }
            }
        }
    }

    if !test_lines.is_empty() {
        let min = *test_lines.iter().min().unwrap();
        let max = *test_lines.iter().max().unwrap();
        let sep = if out.is_empty() { "" } else { "\n" };
        let _ = writeln!(out, "{sep}tests: {}", line_range(min, max));
    }

    out
}

fn format_imports(out: &mut String, entries: &[&SkeletonEntry]) {
    if entries.is_empty() {
        return;
    }

    let min_line = entries.iter().map(|e| e.line_start).min().unwrap();
    let max_line = entries.iter().map(|e| e.line_end).max().unwrap();

    let sep = if out.is_empty() { "" } else { "\n" };
    let _ = writeln!(out, "{sep}imports: {}", line_range(min_line, max_line));

    let mut consolidated: Vec<(String, Vec<String>)> = Vec::new();
    for entry in entries {
        let text = &entry.text;
        let (root, parts) = match text.split_once("::") {
            Some((root, rest)) => {
                let rest = rest.trim();
                if rest.starts_with('{') && rest.ends_with('}') {
                    let inner = &rest[1..rest.len() - 1];
                    let items: Vec<String> = inner
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (root.to_string(), items)
                } else {
                    (root.to_string(), vec![rest.to_string()])
                }
            }
            None => {
                consolidated.push((text.clone(), Vec::new()));
                continue;
            }
        };

        if let Some(existing) = consolidated.iter_mut().find(|(r, _)| *r == root) {
            existing.1.extend(parts);
        } else {
            consolidated.push((root, parts));
        }
    }

    for (root, parts) in &consolidated {
        if parts.is_empty() {
            let _ = writeln!(out, "  {root}");
        } else if parts.len() == 1 {
            let _ = writeln!(out, "  {root}::{}", parts[0]);
        } else {
            let _ = writeln!(out, "  {root}::{{{}}}", parts.join(", "));
        }
    }
}
```

- [ ] **Step 8: Add index_file and index_source functions**

Add to `src/index/mod.rs` (after Language impl):

```rust
pub fn index_file(path: &Path) -> Result<String, IndexError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = Language::from_extension(ext)
        .ok_or_else(|| IndexError::UnsupportedLanguage(format!(".{ext}")))?;

    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(IndexError::FileTooLarge {
            size: meta.len(),
            max: MAX_FILE_SIZE,
        });
    }

    let source = std::fs::read(path)?;
    index_source(&source, lang)
}

pub fn index_source(source: &[u8], lang: Language) -> Result<String, IndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .map_err(|_| IndexError::ParseFailed)?;

    let tree = parser.parse(source, None).ok_or(IndexError::ParseFailed)?;
    let root = tree.root_node();
    let extractor = lang.extractor();

    let module_doc = detect_module_doc(root, source, extractor);
    let mut entries = Vec::new();
    let mut test_lines: Vec<usize> = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if extractor.is_attr(child) || extractor.is_doc_comment(child, source) {
            continue;
        }
        let attrs = extractor.collect_preceding_attrs(child);
        if extractor.is_test_node(child, source, &attrs) {
            test_lines.push(child.start_position().row + 1);
            continue;
        }
        for (i, mut entry) in extractor
            .extract_nodes(child, source, &attrs)
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                if let Some(doc_start) = doc_comment_start_line(child, source, extractor) {
                    entry.line_start = entry.line_start.min(doc_start);
                }
            }
            entries.push(entry);
        }
    }

    Ok(format_skeleton(&entries, &test_lines, module_doc))
}
```

- [ ] **Step 9: Create stub language modules**

Create `src/index/languages/mod.rs`:

```rust
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod typescript;
```

Create stubs for each language file (e.g. `src/index/languages/rust.rs`):

```rust
use tree_sitter::Node;
use crate::index::{LanguageExtractor, PublicApi, SkeletonEntry};

pub(crate) struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn extract_nodes(&self, _node: Node, _source: &[u8], _attrs: &[Node]) -> Vec<SkeletonEntry> {
        Vec::new()
    }
    fn is_test_node(&self, _node: Node, _source: &[u8], _attrs: &[Node]) -> bool { false }
    fn is_doc_comment(&self, _node: Node, _source: &[u8]) -> bool { false }
    fn is_module_doc(&self, _node: Node, _source: &[u8]) -> bool { false }
    fn extract_public_api(&self, _root: Node, _source: &[u8]) -> PublicApi {
        PublicApi { types: Vec::new(), functions: Vec::new() }
    }
}
```

(Same pattern for python.rs, typescript.rs, go.rs, java.rs with their respective struct names: PythonExtractor, TsJsExtractor, GoExtractor, JavaExtractor)

- [ ] **Step 10: Update main.rs and verify compilation**

Update `src/main.rs`:

```rust
mod index;

fn main() {
    eprintln!("taoki MCP server starting");
}
```

**Important structure note:** The `languages/` directory must be inside `src/index/` since it's a submodule of `index`. This means `index` must be a directory module:
- `src/index/mod.rs` (all the index code)
- `src/index/languages/mod.rs`
- `src/index/languages/rust.rs` (etc.)

Run: `cargo build`
Expected: BUILD SUCCESS

- [ ] **Step 11: Run existing tests**

Run: `cargo test`
Expected: All helper tests pass

- [ ] **Step 12: Commit**

```bash
git add -A && git commit -m "feat: core types, helpers, and format_skeleton with stub extractors"
```

---

## Chunk 2: Language Extractors

### Task 3: Rust extractor

Port from `/home/daylon/projects/maki/maki/maki-code-index/src/rust.rs`.

**Files:**
- Create: `src/index/languages/rust.rs` (replace stub)

- [ ] **Step 1: Write test for Rust extraction**

Add to `src/index/mod.rs` tests module:

```rust
    fn idx(source: &str, lang: Language) -> String {
        index_source(source.as_bytes(), lang).unwrap()
    }

    fn has(output: &str, needles: &[&str]) {
        for n in needles {
            assert!(output.contains(n), "missing {n:?} in:\n{output}");
        }
    }

    fn lacks(output: &str, needles: &[&str]) {
        for n in needles {
            assert!(!output.contains(n), "unexpected {n:?} in:\n{output}");
        }
    }

    #[test]
    fn rust_all_sections() {
        let src = "\
//! Module doc
use std::collections::HashMap;
use std::io;

const MAX: usize = 1024;
static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub port: u16,
}

enum Color { Red, Green }

pub trait Handler {
    fn handle(&self, req: Request) -> Response;
}

impl Display for Foo {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, \"Foo\")
    }
}

impl Config {
    pub fn new(name: String) -> Self { todo!() }
}

pub fn process(input: &str) -> Result<String, Error> { todo!() }

pub mod utils;

macro_rules! my_macro { () => {}; }
";
        let out = idx(src, Language::Rust);
        has(&out, &[
            "module doc:",
            "imports:",
            "std::",
            "consts:",
            "MAX: usize",
            "static COUNTER: AtomicU64",
            "types:",
            "#[derive(Debug, Clone)]",
            "pub struct Config",
            "traits:",
            "pub Handler",
            "impls:",
            "Display for Foo",
            "Config",
            "fns:",
            "pub process(input: &str)",
            "mod:",
            "pub utils",
            "macros:",
            "my_macro!",
        ]);
    }

    #[test]
    fn rust_test_module_collapsed() {
        let src = "fn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}\n";
        let out = idx(src, Language::Rust);
        has(&out, &["tests:"]);
        lacks(&out, &["it_works"]);
    }
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cargo test rust_all_sections`
Expected: FAIL (stub extractor returns nothing)

- [ ] **Step 3: Implement Rust extractor**

Replace `src/index/languages/rust.rs` with the full port from maki and implement `extract_public_api` for Rust (public types and `pub` functions). Key helpers needed from index module (add these with `pub(crate)` visibility in `src/index/mod.rs` if not already present):
- `has_test_attr(attrs, source)` — checks for `#[test]`, `#[cfg(test)]`, `::test]`
- `vis_prefix(node, source)` — returns `"pub"` or `""`
- `relevant_attr_texts(attrs, source)` — filters to derive/cfg attrs only
- `fn_signature(node, source)` — builds `name(params) -> ret`

The extractor handles: `use_declaration`, `struct_item`, `enum_item`, `union_item`, `function_item`, `trait_item`, `impl_item`, `const_item`, `static_item`, `mod_item`, `macro_definition`, `type_item`.

Port the exact code from maki's `src/rust.rs`, replacing `crate::common::` imports with `crate::index::`.

- [ ] **Step 4: Run tests**

Run: `cargo test rust_`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: Rust language extractor"
```

---

### Task 4: Python extractor

Port from `/home/daylon/projects/maki/maki/maki-code-index/src/python.rs`.

**Files:**
- Create: `src/index/languages/python.rs` (replace stub)

- [ ] **Step 1: Write test**

Add to `src/index/mod.rs` tests:

```rust
    #[test]
    fn python_all_sections() {
        let src = "\
\"\"\"Module docstring.\"\"\"

import os
from typing import Optional

MAX_RETRIES = 3

@dataclass
class MyClass:
    x: int = 0

class AuthService:
    def __init__(self, secret: str):
        self.secret = secret
    @staticmethod
    def validate(token: str) -> bool:
        return True

def process(data: list) -> dict:
    return {}
";
        let out = idx(src, Language::Python);
        has(&out, &[
            "module doc:",
            "imports:",
            "os",
            "typing::Optional",
            "consts:",
            "MAX_RETRIES",
            "classes:",
            "MyClass",
            "AuthService",
            "__init__(self, secret: str)",
            "validate(token: str) -> bool",
            "fns:",
            "process(data: list) -> dict",
        ]);
    }
```

- [ ] **Step 2: Implement Python extractor**

Port the exact code from maki's `src/python.rs` and implement `extract_public_api` for Python (top-level classes and functions). Handles: `import_statement`, `import_from_statement`, `class_definition`, `function_definition`, `decorated_definition`, `expression_statement` (ALL_CAPS assignments only).

- [ ] **Step 3: Run tests**

Run: `cargo test python_`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: Python language extractor"
```

---

### Task 5: TypeScript/JavaScript extractor

Port from `/home/daylon/projects/maki/maki/maki-code-index/src/typescript.rs`.

**Files:**
- Create: `src/index/languages/typescript.rs` (replace stub)

- [ ] **Step 1: Write tests**

Add to `src/index/mod.rs` tests:

```rust
    #[test]
    fn ts_all_sections() {
        let src = "\
import { Request, Response } from 'express';

export interface Config {
    port: number;
    host: string;
}

export type ID = string | number;

export enum Direction { Up, Down }

export const PORT: number = 3000;

export class Service {
    process(input: string): string { return input; }
}

export function handler(req: Request): Response { return new Response(); }
";
        let out = idx(src, Language::TypeScript);
        has(&out, &[
            "imports:",
            "{ Request, Response } from 'express'",
            "types:",
            "export interface Config",
            "port: number",
            "export enum Direction",
            "consts:",
            "PORT",
            "classes:",
            "export Service",
            "fns:",
            "export handler(req: Request)",
        ]);
    }

    #[test]
    fn js_function() {
        let out = idx(
            "function hello(name) {\n    console.log(name);\n}\n",
            Language::JavaScript,
        );
        has(&out, &["fns:", "hello(name)"]);
    }
```

- [ ] **Step 2: Implement TS/JS extractor**

Port the exact code from maki's `src/typescript.rs` and implement `extract_public_api` for TS/JS (exported types/functions/classes). Shared `TsJsExtractor` for both languages. Handles: `import_statement`, `class_declaration`, `function_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration`, `lexical_declaration` (const only), `export_statement` (delegates to inner).

- [ ] **Step 3: Run tests**

Run: `cargo test ts_ js_`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: TypeScript/JavaScript language extractor"
```

---

### Task 6: Go extractor

Port from `/home/daylon/projects/maki/maki/maki-code-index/src/go.rs`.

**Files:**
- Create: `src/index/languages/go.rs` (replace stub)

- [ ] **Step 1: Write test**

Add to `src/index/mod.rs` tests:

```rust
    #[test]
    fn go_all_sections() {
        let src = r#"
package main

import (
	"fmt"
	"os"
)

const MaxRetries = 3

type Point struct {
	X int
	Y int
}

type Reader interface {
	Read(p []byte) (int, error)
}

func (p *Point) Distance() float64 {
	return 0
}

func main() {
	fmt.Println("hello")
}
"#;
        let out = idx(src, Language::Go);
        has(&out, &[
            "imports:",
            "fmt",
            "os",
            "consts:",
            "MaxRetries",
            "types:",
            "struct Point",
            "X int",
            "traits:",
            "Reader",
            "Read(p []byte) (int, error)",
            "impls:",
            "(p *Point) Distance() float64",
            "fns:",
            "main()",
        ]);
    }
```

- [ ] **Step 2: Implement Go extractor**

Port from maki's `src/go.rs` and implement `extract_public_api` for Go (exported identifiers start with uppercase). Handles: `import_declaration`, `type_declaration` (struct_type → Type, interface_type → Trait), `const_declaration`, `var_declaration`, `function_declaration`, `method_declaration`.

- [ ] **Step 3: Run tests**

Run: `cargo test go_`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: Go language extractor"
```

---

### Task 7: Java extractor

Port from `/home/daylon/projects/maki/maki/maki-code-index/src/java.rs`.

**Files:**
- Create: `src/index/languages/java.rs` (replace stub)

- [ ] **Step 1: Write test**

Add to `src/index/mod.rs` tests:

```rust
    #[test]
    fn java_all_sections() {
        let src = r#"
package com.example;

import java.util.List;
import java.io.IOException;

public class Service {
    private String name;
    public Service(String name) { this.name = name; }
    public void process(List<String> items) throws IOException {}
}

public interface Handler {
    void handle(String request);
}

public enum Direction {
    UP, DOWN, LEFT, RIGHT
}
"#;
        let out = idx(src, Language::Java);
        has(&out, &[
            "imports:",
            "java::{util::List, io::IOException}",
            "mod:",
            "com.example",
            "classes:",
            "public class Service",
            "private String name",
            "public Service(String name)",
            "traits:",
            "public interface Handler",
            "types:",
            "public enum Direction",
            "UP",
        ]);
    }
```

- [ ] **Step 2: Implement Java extractor**

Port from maki's `src/java.rs` and implement `extract_public_api` for Java (public classes/interfaces/enums and public methods). Handles: `import_declaration`, `package_declaration`, `class_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration`, `annotation_type_declaration`.

- [ ] **Step 3: Run tests**

Run: `cargo test java_`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: Java language extractor"
```

---

## Chunk 3: Code Map Tool

### Task 8: code_map module

New tool — repo-wide structural map with blake3 caching.

**Files:**
- Create: `src/codemap.rs`
- Modify: `src/main.rs` (add `mod codemap;`)

- [ ] **Step 1: Write test for public API extraction**

Create `src/codemap.rs` with test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extracts_public_types_and_functions_from_rust() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub struct Foo {}\npub fn bar() {}\nfn private() {}\n").unwrap();

        let result = build_code_map(dir.path(), &[]).unwrap();
        assert!(result.contains("lib.rs"));
        assert!(result.contains("Foo"));
        assert!(result.contains("bar()"));
        assert!(!result.contains("private"));
    }
}
```

- [ ] **Step 2: Define code_map types and cache schema**

Add to `src/codemap.rs`:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::{self, Language};

#[derive(Debug, thiserror::Error)]
pub enum CodeMapError {
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    version: u32,
    files: HashMap<String, CacheEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    hash: String,
    lines: usize,
    public_types: Vec<String>,
    public_functions: Vec<String>,
}

const CACHE_VERSION: u32 = 1;
const CACHE_DIR: &str = ".cache/taoki";
const CACHE_FILE: &str = "code-map.json";
```

- [ ] **Step 3: Implement file walking and glob filtering**

Add function to walk directory using `ignore` crate, filter by globs:

```rust
fn walk_files(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>, CodeMapError> {
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    if !root.exists() {
        return Err(CodeMapError::PathNotFound(root.to_path_buf()));
    }

    let glob_set = if globs.is_empty() {
        None
    } else {
        let mut builder = GlobSetBuilder::new();
        for g in globs {
            if let Ok(glob) = Glob::new(g) {
                builder.add(glob);
            }
        }
        builder.build().ok()
    };

    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .standard_filters(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if Language::from_extension(ext).is_none() {
            continue;
        }
        if let Some(ref gs) = glob_set {
            let rel = path.strip_prefix(root).unwrap_or(path);
            if !gs.is_match(rel) {
                continue;
            }
        }
        files.push(path.to_path_buf());
    }

    files.sort();
    Ok(files)
}
```

- [ ] **Step 4: Implement public API extraction in index module**

Add a dedicated extraction path in `src/index/mod.rs` that does not parse visibility from formatted text. Extend the extractor trait to surface public API items directly.

Add to `src/index/mod.rs`:

```rust
pub struct PublicApi {
    pub types: Vec<String>,
    pub functions: Vec<String>,
}

pub(crate) trait LanguageExtractor {
    fn extract_nodes(&self, node: Node, source: &[u8], attrs: &[Node]) -> Vec<SkeletonEntry>;
    fn is_test_node(&self, node: Node, source: &[u8], attrs: &[Node]) -> bool;
    fn is_doc_comment(&self, node: Node, source: &[u8]) -> bool;
    fn is_module_doc(&self, node: Node, source: &[u8]) -> bool;
    fn is_attr(&self, _node: Node) -> bool {
        false
    }
    fn collect_preceding_attrs<'a>(&self, node: Node<'a>) -> Vec<Node<'a>> { /* unchanged */ }

    /// Extract only public types and function signatures for code_map.
    /// Must be implemented per-language to avoid visibility heuristics.
    fn extract_public_api(
        &self,
        root: Node,
        source: &[u8],
    ) -> PublicApi;
}

pub fn extract_public_api(source: &[u8], lang: Language) -> Result<(Vec<String>, Vec<String>), IndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .map_err(|_| IndexError::ParseFailed)?;

    let tree = parser.parse(source, None).ok_or(IndexError::ParseFailed)?;
    let root = tree.root_node();
    let extractor = lang.extractor();
    let api = extractor.extract_public_api(root, source);
    Ok((api.types, api.functions))
}
```

Update each language extractor to implement `extract_public_api` by reusing its AST logic, returning only public types and public function signatures (including Go export rules). This avoids fragile string parsing and keeps `code_map` consistent across languages.

- [ ] **Step 5: Implement blake3 hashing and cache load/save**

Add to `src/codemap.rs`:

```rust
fn hash_file(path: &Path) -> std::io::Result<String> {
    let data = std::fs::read(path)?;
    Ok(blake3::hash(&data).to_hex().to_string())
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(CACHE_DIR).join(CACHE_FILE)
}

fn load_cache(root: &Path) -> Cache {
    let path = cache_path(root);
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or(Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        }),
        Err(_) => Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        },
    }
}

fn save_cache(root: &Path, cache: &Cache) {
    use fs2::FileExt;
    let path = cache_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let lock_path = path.with_extension("lock");
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("warning: could not open cache lock: {e}");
            return;
        }
    };
    if lock_file.lock_exclusive().is_err() {
        eprintln!("warning: could not lock cache file");
        return;
    }
    if let Ok(data) = serde_json::to_string_pretty(cache) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            eprintln!("warning: could not write cache temp file");
        }
    }
    let _ = lock_file.unlock();
}
```

- [ ] **Step 6: Implement build_code_map**

```rust
pub fn build_code_map(root: &Path, globs: &[String]) -> Result<String, CodeMapError> {
    let files = walk_files(root, globs)?;
    let mut cache = load_cache(root);

    // Invalidate cache if version changed
    if cache.version != CACHE_VERSION {
        cache = Cache {
            version: CACHE_VERSION,
            files: HashMap::new(),
        };
    }

    let mut new_files: HashMap<String, CacheEntry> = HashMap::new();
    let mut results: Vec<(String, usize, Vec<String>, Vec<String>, bool)> = Vec::new();

    for file_path in &files {
        let rel = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let hash = match hash_file(file_path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Check cache
        if let Some(cached) = cache.files.get(&rel) {
            if cached.hash == hash {
                results.push((
                    rel.clone(),
                    cached.lines,
                    cached.public_types.clone(),
                    cached.public_functions.clone(),
                    false,
                ));
                new_files.insert(rel, CacheEntry {
                    hash,
                    lines: cached.lines,
                    public_types: cached.public_types.clone(),
                    public_functions: cached.public_functions.clone(),
                });
                continue;
            }
        }

        // Parse file
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = Language::from_extension(ext) else {
            continue;
        };

        let source = match std::fs::read(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines = source.iter().filter(|&&b| b == b'\n').count() + 1;

        let (public_types, public_functions) =
            match index::extract_public_api(&source, lang) {
                Ok(api) => api,
                Err(_) => {
                    // Parse error — include with marker
                    results.push((rel.clone(), lines, Vec::new(), Vec::new(), true));
                    continue;
                }
            };

        new_files.insert(
            rel.clone(),
            CacheEntry {
                hash,
                lines,
                public_types: public_types.clone(),
                public_functions: public_functions.clone(),
            },
        );

        results.push((rel, lines, public_types, public_functions, false));
    }

    // Update and save cache
    cache.files = new_files;
    save_cache(root, &cache);

    // Sort by path and format output
    results.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    for (path, lines, types, fns, parse_error) in &results {
        if *parse_error {
            out.push_str(&format!("- {path} ({lines} lines) (parse error)\n"));
            continue;
        }
        let types_str = if types.is_empty() {
            "(none)".to_string()
        } else {
            types.join(", ")
        };
        let fns_str = if fns.is_empty() {
            "(none)".to_string()
        } else {
            fns.join(", ")
        };
        out.push_str(&format!(
            "- {path} ({lines} lines) - public_types: {types_str} - public_functions: {fns_str}\n"
        ));
    }

    Ok(out)
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test codemap`
Expected: PASS

- [ ] **Step 8: Add caching test**

```rust
    #[test]
    fn caching_reuses_results() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        fs::write(&file, "pub struct Foo {}\n").unwrap();

        // First call — builds cache
        let r1 = build_code_map(dir.path(), &[]).unwrap();
        assert!(dir.path().join(".cache/taoki/code-map.json").exists());

        // Second call — uses cache (same result)
        let r2 = build_code_map(dir.path(), &[]).unwrap();
        assert_eq!(r1, r2);

        // Modify file — cache miss
        fs::write(&file, "pub struct Bar {}\n").unwrap();
        let r3 = build_code_map(dir.path(), &[]).unwrap();
        assert!(r3.contains("Bar"));
        assert!(!r3.contains("Foo"));
    }
```

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 10: Commit**

```bash
git add -A && git commit -m "feat: code_map tool with blake3 caching and gitignore-aware walking"
```

---

## Chunk 4: MCP Server

### Task 9: MCP stdio server

Hand-rolled JSON-RPC 2.0 over stdin/stdout. 4 methods: initialize, notifications/initialized, tools/list, tools/call.

**Files:**
- Rewrite: `src/main.rs`

- [ ] **Step 1: Define MCP protocol types**

Create `src/mcp.rs` with JSON-RPC 2.0 structures:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ToolContent {
    pub r#type: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "is_false")]
    #[serde(rename = "isError")]
    pub is_error: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}
```

- [ ] **Step 2: Implement tool definitions for tools/list**

Add to `src/mcp.rs`:

```rust
pub fn tool_definitions() -> Value {
    serde_json::json!({
        "tools": [
            {
                "name": "index",
                "description": "Return a compact structural skeleton of a source file: imports, type definitions, function signatures, and their line numbers. ~70-90% fewer tokens than reading the full file. Use this to understand a file's architecture before reading specific sections with the Read tool. Supports: Rust, Python, TypeScript, JavaScript, Go, Java.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source file to index"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "code_map",
                "description": "Build an incremental structural map of a codebase. Returns one line per file with public types and public function signatures. Use this FIRST when you need to understand a repository's structure or find which files are relevant to a task. Results are cached (blake3 hash) so repeated calls are near-instant. Supports glob patterns to narrow scope.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the repository root to scan"
                        },
                        "globs": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional glob patterns to filter files (e.g. [\"src/**/*.rs\"]). Defaults to all supported file types."
                        }
                    },
                    "required": ["path"]
                }
            }
        ]
    })
}
```

- [ ] **Step 3: Implement request dispatch**

Add to `src/mcp.rs`:

```rust
use crate::index;
use crate::codemap;

pub fn handle_request(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    match req.method.as_str() {
        "initialize" => Some(handle_initialize(req)),
        "notifications/initialized" => None, // No response for notifications
        "tools/list" => Some(handle_tools_list(req)),
        "tools/call" => Some(handle_tools_call(req)),
        _ => Some(JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("method not found: {}", req.method),
        )),
    }
}

fn handle_initialize(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse::success(
        req.id.clone(),
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "taoki",
                "version": "0.1.0"
            }
        }),
    )
}

fn handle_tools_list(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse::success(req.id.clone(), tool_definitions())
}

fn handle_tools_call(req: &JsonRpcRequest) -> JsonRpcResponse {
    let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = req.params.get("arguments").cloned().unwrap_or_default();

    let result = match tool_name {
        "index" => call_index(&arguments),
        "code_map" => call_code_map(&arguments),
        _ => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: format!("unknown tool: {tool_name}"),
            }],
            is_error: true,
        },
    };

    JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
}

fn call_index(args: &Value) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameter: path".to_string(),
            }],
            is_error: true,
        };
    }

    match index::index_file(std::path::Path::new(path)) {
        Ok(skeleton) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: skeleton,
            }],
            is_error: false,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: e.to_string(),
            }],
            is_error: true,
        },
    }
}

fn call_code_map(args: &Value) -> ToolResult {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: "missing required parameter: path".to_string(),
            }],
            is_error: true,
        };
    }

    let globs: Vec<String> = args
        .get("globs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match codemap::build_code_map(std::path::Path::new(path), &globs) {
        Ok(map) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: map,
            }],
            is_error: false,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent {
                r#type: "text".to_string(),
                text: e.to_string(),
            }],
            is_error: true,
        },
    }
}
```

- [ ] **Step 4: Implement main.rs stdio loop**

Rewrite `src/main.rs`:

```rust
mod codemap;
mod index;
mod mcp;

use std::io::{self, Read, Write};

fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<String>> {
    let mut headers = String::new();
    let mut buf = [0u8; 1];
    while reader.read(&mut buf)? == 1 {
        headers.push(buf[0] as char);
        if headers.ends_with("\r\n\r\n") {
            break;
        }
    }
    if headers.is_empty() {
        return Ok(None);
    }
    let mut content_len = None;
    for line in headers.split("\r\n") {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_len = rest.trim().parse::<usize>().ok();
            break;
        }
    }
    let Some(len) = content_len else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"));
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).to_string()))
}

fn write_message<W: Write>(writer: &mut W, msg: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{}", msg.len(), msg)?;
    writer.flush()
}

fn main() {
    eprintln!("taoki: MCP server starting");

    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    loop {
        let raw = match read_message(&mut stdin) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                eprintln!("taoki: read error: {e}");
                break;
            }
        };

        let req: mcp::JsonRpcRequest = match serde_json::from_str(&raw) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("taoki: parse error: {e}");
                let resp = mcp::JsonRpcResponse::error(None, -32700, format!("parse error: {e}"));
                let json = serde_json::to_string(&resp).unwrap();
                let _ = write_message(&mut stdout, &json);
                continue;
            }
        };

        eprintln!("taoki: received {}", req.method);

        if let Some(resp) = mcp::handle_request(&req) {
            let json = serde_json::to_string(&resp).unwrap();
            let _ = write_message(&mut stdout, &json);
        }
    }

    eprintln!("taoki: MCP server shutting down");
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build`
Expected: BUILD SUCCESS

- [ ] **Step 6: Integration test — send JSON-RPC to binary**

Run manually (or as a test script):

```bash
msg='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
printf 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg" | cargo run 2>/dev/null
```

Expected: JSON response with serverInfo.name == "taoki"

```bash
msg='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
printf 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg" | cargo run 2>/dev/null
```

Expected: JSON response with two tools

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: MCP stdio server with JSON-RPC dispatch"
```

---

## Chunk 5: Slash Commands and Final Polish

### Task 10: Slash commands

**Files:**
- Create: `commands/taoki-map.md`
- Create: `commands/taoki-index.md`

- [ ] **Step 1: Create taoki-map command**

Create `commands/taoki-map.md`:

```markdown
---
allowed-tools: mcp__taoki__code_map, mcp__taoki__index
description: Build a structural map of this repository
---

Call the `mcp__taoki__code_map` tool to build a structural map of this repository.

If arguments are provided, use them as glob patterns. Otherwise, default to all supported file types.

After receiving the code map, provide a concise summary of the repository's architecture:
- Key modules and their responsibilities
- Main types and how they relate
- Entry points and public API surface
```

- [ ] **Step 2: Create taoki-index command**

Create `commands/taoki-index.md`:

```markdown
---
allowed-tools: mcp__taoki__index
description: Show the structural skeleton of a source file
---

Call the `mcp__taoki__index` tool on the specified file path.

After receiving the index, present the file structure and highlight:
- The main types and their purpose
- Key functions and what they do
- Notable patterns (traits, impls, test coverage)
```

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: slash commands for taoki-map and taoki-index"
```

---

### Task 11: Build and verify end-to-end

- [ ] **Step 1: Build release binary**

Run: `cargo build --release`
Expected: BUILD SUCCESS, binary at `target/release/taoki`

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 3: Test run.sh launcher**

Run: `./scripts/run.sh` (with stdin piped)

```bash
msg='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
printf 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg" | ./scripts/run.sh 2>/dev/null
```

Expected: Valid JSON-RPC response

- [ ] **Step 4: Test index tool end-to-end**

```bash
msg='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"index","arguments":{"path":"'$(pwd)/src/main.rs'"}}}'
printf 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg" | ./scripts/run.sh 2>/dev/null
```

Expected: JSON with skeleton of main.rs

- [ ] **Step 5: Test code_map tool end-to-end**

```bash
msg='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"code_map","arguments":{"path":"'$(pwd)'"}}}'
printf 'Content-Length: %s\r\n\r\n%s' "${#msg}" "$msg" | ./scripts/run.sh 2>/dev/null
```

Expected: JSON with code map of taoki repo itself

- [ ] **Step 6: Final commit**

```bash
git add -A && git commit -m "chore: verify end-to-end build and tests"
```
