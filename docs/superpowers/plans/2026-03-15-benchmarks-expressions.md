# Benchmarks & Top-Level Expressions Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enhance the index tool to capture top-level expressions in Python and TypeScript/JavaScript, then build a two-tier benchmark suite to measure speed, completeness, and byte efficiency.

**Architecture:** Add `Section::Expression` to the section enum, extend the Python and TypeScript extractors to capture structurally interesting top-level expressions (assignments, dotted method calls, `if __name__`), then build criterion benchmarks for speed/ratio and a shell script for real-world repo measurements.

**Tech Stack:** Rust, tree-sitter, criterion (benchmarking), bash

---

## Chunk 0: Branch setup

### Task 0: Create the feature branch

- [ ] **Step 1: Create the branch from current master**

```bash
git checkout -b feat/benchmarks-expressions
```

All subsequent tasks commit to this branch.

---

## Chunk 1: Section::Expression enum variant

### Task 1: Add Section::Expression to the enum

**Files:**
- Modify: `src/index/mod.rs:155-181`

- [ ] **Step 1: Add the Expression variant**

In `src/index/mod.rs`, insert `Expression` between `Constant` and `Type` in the `Section` enum (line 158-159):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Import,
    Constant,
    Expression,
    Type,
    Trait,
    Impl,
    Function,
    Class,
    Module,
    Macro,
}
```

And add the header in the `header()` method:

```rust
Self::Expression => "exprs:",
```

Insert this line between `Self::Constant => "consts:",` and `Self::Type => "types:",`.

- [ ] **Step 2: Verify it compiles and existing tests pass**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
Expected: all 29 tests pass (Expression is defined but not used yet)

- [ ] **Step 3: Commit**

```bash
git add src/index/mod.rs
git commit -m "feat: add Section::Expression variant for top-level expressions"
```

---

## Chunk 2: Python top-level expressions

### Task 2: Add Python expression extraction

**Files:**
- Modify: `src/index/languages/python.rs:99-157`

- [ ] **Step 1: Write the test**

Add this test at the end of the `tests` module in `src/index/languages/python.rs` (after the `python_test_functions_collapsed` test, before the closing `}`):

```rust
    #[test]
    fn python_top_level_expressions() {
        let src = "\
from flask import Flask

app = Flask(__name__)
__version__ = '1.0'
db = SQLAlchemy()

app.register_blueprint(auth_bp)
db.init_app(app)

MAX_SIZE = 100

print('hello')
run()
console.log('test')
logging.info('started')

if __name__ == '__main__':
    app.run(debug=True)
";
        let out = index_source(src.as_bytes(), Language::Python).unwrap();

        // Named assignments (non-ALL_CAPS) should appear in exprs:
        assert!(out.contains("exprs:"), "missing exprs section in:\n{out}");
        assert!(out.contains("app = Flask(__name__)"), "missing app assignment in:\n{out}");
        assert!(out.contains("__version__"), "missing __version__ in:\n{out}");
        assert!(out.contains("db = SQLAlchemy()"), "missing db assignment in:\n{out}");

        // Dotted method calls should appear in exprs:
        assert!(out.contains("app.register_blueprint"), "missing register_blueprint in:\n{out}");
        assert!(out.contains("db.init_app"), "missing init_app in:\n{out}");

        // ALL_CAPS should still be in consts:
        assert!(out.contains("consts:"), "missing consts section in:\n{out}");
        assert!(out.contains("MAX_SIZE"), "missing MAX_SIZE in:\n{out}");

        // Noise should NOT appear
        assert!(!out.contains("print("), "print() should be filtered in:\n{out}");
        assert!(!out.contains("run()"), "run() should be filtered in:\n{out}");
        assert!(!out.contains("logging.info"), "logging.info should be filtered in:\n{out}");

        // if __name__ == '__main__' should be collapsed with line range
        assert!(out.contains("if __name__"), "missing if __name__ block in:\n{out}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test python_top_level_expressions -- --nocapture`
Expected: FAIL — `exprs:` section doesn't exist yet

- [ ] **Step 3: Add the noisy receiver skip list and extraction methods**

In `src/index/languages/python.rs`, add these constants and methods to `PythonExtractor` (inside the `impl PythonExtractor` block, after `extract_assignment`):

```rust
    const NOISY_RECEIVERS: &'static [&'static str] = &[
        "console", "process", "logging", "log", "logger", "Math", "Object", "Array", "JSON",
    ];

    fn extract_expression_assignment(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is an "assignment" inside an "expression_statement"
        let left = node.child(0)?;
        let name = node_text(left, source);

        // ALL_CAPS are handled by extract_assignment as constants
        if name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            return None;
        }

        let text = truncate(node_text(node, source).trim(), 80);
        let parent = node.parent()?;
        Some(SkeletonEntry::new(Section::Expression, parent, text.to_string()))
    }

    fn extract_dotted_call(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is a "call_expression" inside an "expression_statement"
        let func = node.child_by_field_name("function")?;
        if func.kind() != "attribute" {
            return None; // Not a dotted call
        }
        // Check if receiver is in the noisy skip list
        let receiver = func.child_by_field_name("object").map(|n| node_text(n, source)).unwrap_or("");
        if Self::NOISY_RECEIVERS.contains(&receiver) {
            return None;
        }
        let text = truncate(node_text(node, source).trim(), 80);
        let parent = node.parent()?;
        Some(SkeletonEntry::new(Section::Expression, parent, text.to_string()))
    }

    fn extract_if_name_main(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is an "if_statement"
        let condition = node.child_by_field_name("condition")?;
        let cond_text = node_text(condition, source);
        if cond_text.contains("__name__") && cond_text.contains("__main__") {
            let lr = line_range(node.start_position().row + 1, node.end_position().row + 1);
            Some(SkeletonEntry::new(
                Section::Expression,
                node,
                format!("if __name__ == \"__main__\" {lr}"),
            ))
        } else {
            None
        }
    }
```

- [ ] **Step 4: Update extract_nodes to handle new node types**

In the `extract_nodes` method of `impl LanguageExtractor for PythonExtractor`, replace the `"expression_statement"` match arm and add `"if_statement"`:

Replace:
```rust
            "expression_statement" => node
                .child(0)
                .filter(|c| c.kind() == "assignment")
                .and_then(|c| self.extract_assignment(c, source)),
            _ => None,
```

With:
```rust
            "expression_statement" => {
                if let Some(child) = node.child(0) {
                    match child.kind() {
                        "assignment" => {
                            self.extract_assignment(child, source)
                                .or_else(|| self.extract_expression_assignment(child, source))
                        }
                        "call" => self.extract_dotted_call(child, source),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            "if_statement" => self.extract_if_name_main(node, source),
            _ => None,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test python_top_level_expressions -- --nocapture`
Expected: PASS

Run: `cargo test`
Expected: all 30 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/python.rs src/index/mod.rs
git commit -m "feat: add top-level expression extraction for Python"
```

### Task 3: Add TypeScript/JavaScript expression extraction

**Files:**
- Modify: `src/index/languages/typescript.rs:17-210`

- [ ] **Step 1: Write the test**

Add this test at the end of the `tests` module in `src/index/languages/typescript.rs`:

```rust
    #[test]
    fn ts_top_level_expressions() {
        let src = "\
import express from 'express';

const app = express();
let server;
var config = {};

module.exports = { app };
exports.handler = handler;

app.use(middleware());
app.get('/api', handler);
router.post('/login', auth);

console.log('starting');
process.exit(1);

export default class App {}
";
        let out = index_source(src.as_bytes(), Language::TypeScript).unwrap();

        // const should be in consts:
        assert!(out.contains("consts:"), "missing consts section in:\n{out}");
        assert!(out.contains("app"), "missing app const in:\n{out}");

        // let/var should be in exprs:
        assert!(out.contains("exprs:"), "missing exprs section in:\n{out}");
        assert!(out.contains("server"), "missing let server in:\n{out}");
        assert!(out.contains("config"), "missing var config in:\n{out}");

        // Assignment expressions should be in exprs:
        assert!(out.contains("module.exports"), "missing module.exports in:\n{out}");
        assert!(out.contains("exports.handler"), "missing exports.handler in:\n{out}");

        // Dotted method calls should be in exprs:
        assert!(out.contains("app.use"), "missing app.use in:\n{out}");
        assert!(out.contains("app.get"), "missing app.get in:\n{out}");
        assert!(out.contains("router.post"), "missing router.post in:\n{out}");

        // Noise should NOT appear
        assert!(!out.contains("console.log"), "console.log should be filtered in:\n{out}");
        assert!(!out.contains("process.exit"), "process.exit should be filtered in:\n{out}");

        // export default should appear
        assert!(out.contains("App"), "missing export default class App in:\n{out}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test ts_top_level_expressions -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add extraction methods to TsJsExtractor**

Add these constants and methods to the `impl TsJsExtractor` block (after `extract_export_statement`):

```rust
    const NOISY_RECEIVERS: &'static [&'static str] = &[
        "console", "process", "logging", "log", "logger", "Math", "Object", "Array", "JSON",
    ];

    fn extract_assignment_expression(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is an "expression_statement" containing an "assignment_expression"
        let child = node.child(0)?;
        if child.kind() != "assignment_expression" {
            return None;
        }
        let text = truncate(node_text(child, source).trim(), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }

    fn extract_dotted_call(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is an "expression_statement" containing a "call_expression"
        let child = node.child(0)?;
        if child.kind() != "call_expression" {
            return None;
        }
        let func = child.child_by_field_name("function")?;
        if func.kind() != "member_expression" {
            return None;
        }
        let receiver = func.child_by_field_name("object").map(|n| node_text(n, source)).unwrap_or("");
        if Self::NOISY_RECEIVERS.contains(&receiver) {
            return None;
        }
        let text = truncate(node_text(child, source).trim(), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }

    fn extract_export_default(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        // node is an "export_statement" that we couldn't handle in extract_export_statement
        // Check if it's an "export default"
        let mut cursor = node.walk();
        let has_default = node.children(&mut cursor).any(|c| node_text(c, source) == "default");
        if !has_default {
            return None;
        }
        let text = truncate(node_text(node, source).trim().trim_end_matches(';'), 80);
        Some(SkeletonEntry::new(Section::Expression, node, text.to_string()))
    }
```

- [ ] **Step 4: Update extract_nodes and extract_lexical_declaration**

In `extract_lexical_declaration`, change the `else` branch to capture `let`/`var`:

```rust
    fn extract_lexical_declaration(&self, node: Node, source: &[u8]) -> Option<SkeletonEntry> {
        let kind_text = node.child(0).map(|n| node_text(n, source)).unwrap_or("");
        if kind_text == "const" {
            self.extract_const(node, source)
        } else {
            // let/var declarations
            let decl = find_child(node, "variable_declarator")?;
            let name = decl.child_by_field_name("name").map(|n| node_text(n, source))?;
            let type_str = decl.child_by_field_name("type").map(|n| ts_return_type(n, source)).unwrap_or_default();
            let val_str = decl.child_by_field_name("value")
                .map(|n| format!(" = {}", truncate(node_text(n, source), 60)))
                .unwrap_or_default();
            let ep = self.export_prefix(node);
            Some(SkeletonEntry::new(
                Section::Expression,
                node,
                format!("{ep}{kind_text} {name}{type_str}{val_str}"),
            ))
        }
    }
```

In `extract_nodes`, add `"expression_statement"`, `"variable_declaration"`, and update `"export_statement"`:

Replace:
```rust
            "export_statement" => self.extract_export_statement(node, source),
            _ => None,
```

With:
```rust
            "expression_statement" => {
                self.extract_assignment_expression(node, source)
                    .or_else(|| self.extract_dotted_call(node, source))
            }
            "variable_declaration" => self.extract_lexical_declaration(node, source),
            "export_statement" => {
                self.extract_export_statement(node, source)
                    .or_else(|| self.extract_export_default(node, source))
            }
            _ => None,
```

**Note:** `var` declarations in tree-sitter-javascript parse as `"variable_declaration"`, not `"lexical_declaration"`. The `extract_lexical_declaration` method handles both by checking the keyword (`const` vs `let`/`var`). Adding the `"variable_declaration"` match arm routes `var` declarations through the same method.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test ts_top_level_expressions -- --nocapture`
Expected: PASS

Run: `cargo test`
Expected: all 31 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/index/languages/typescript.rs
git commit -m "feat: add top-level expression extraction for TypeScript/JavaScript"
```

---

## Chunk 3: Criterion benchmarks

### Task 4: Add criterion dependency and speed benchmarks

**Files:**
- Modify: `Cargo.toml`
- Create: `benches/speed.rs`

- [ ] **Step 1: Add criterion to Cargo.toml**

Add to `[dev-dependencies]` section:

```toml
criterion = { version = "0.5", features = ["html_reports"] }
```

Add at the end of `Cargo.toml`:

```toml

[[bench]]
name = "speed"
harness = false

[[bench]]
name = "token_ratio"
harness = false
```

- [ ] **Step 2: Create benches/speed.rs**

```rust
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::fs;
use std::path::Path;

// We need access to index_source and extract_public_api
// These are pub functions in the taoki crate
use taoki::index::{index_source, extract_public_api, Language};

fn bench_index_source(c: &mut Criterion) {
    let samples: Vec<(&str, Language, &str)> = vec![
        ("Rust", Language::Rust, include_str!("fixtures/sample.rs")),
        ("Python", Language::Python, include_str!("fixtures/sample.py")),
        ("TypeScript", Language::TypeScript, include_str!("fixtures/sample.ts")),
        ("JavaScript", Language::JavaScript, include_str!("fixtures/sample.js")),
        ("Go", Language::Go, include_str!("fixtures/sample.go")),
        ("Java", Language::Java, include_str!("fixtures/sample.java")),
    ];

    let mut group = c.benchmark_group("index_source");
    for (name, lang, source) in &samples {
        group.bench_function(*name, |b| {
            b.iter(|| index_source(black_box(source.as_bytes()), *lang))
        });
    }
    group.finish();

    let mut group = c.benchmark_group("extract_public_api");
    for (name, lang, source) in &samples {
        group.bench_function(*name, |b| {
            b.iter(|| extract_public_api(black_box(source.as_bytes()), *lang))
        });
    }
    group.finish();
}

fn bench_code_map(c: &mut Criterion) {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    // Create 15 small source files
    for i in 0..15 {
        let ext = match i % 3 {
            0 => "rs",
            1 => "py",
            _ => "ts",
        };
        let path = dir.path().join(format!("file_{i}.{ext}"));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "// file {i}").unwrap();
        writeln!(f, "pub fn func_{i}() {{}}").unwrap();
    }
    // Initialize git so .gitignore works
    std::process::Command::new("git").args(["init"]).current_dir(dir.path()).output().ok();

    let mut group = c.benchmark_group("code_map");

    group.bench_function("cold", |b| {
        b.iter(|| {
            // Remove cache before each iteration
            let _ = std::fs::remove_dir_all(dir.path().join(".cache"));
            taoki::codemap::build_code_map(dir.path(), &[])
        })
    });

    // Pre-warm the cache
    let _ = taoki::codemap::build_code_map(dir.path(), &[]);

    group.bench_function("cached", |b| {
        b.iter(|| taoki::codemap::build_code_map(dir.path(), &[]))
    });

    group.finish();
}

criterion_group!(benches, bench_index_source, bench_code_map);
criterion_main!(benches);
```

**Note:** Benchmarks need access to `index_source`, `extract_public_api`, and `Language`, which are `pub` but the `index` module is private (`mod index` in `main.rs`). We need a `lib.rs` to expose them.

**Approach:** Move all `mod` declarations from `main.rs` to a new `src/lib.rs`. `main.rs` then imports via `use taoki::mcp;`. All internal `crate::` references in the source modules continue to work because `crate::` resolves to the lib crate.

- [ ] **Step 3: Create src/lib.rs**

```rust
pub mod codemap;
pub mod deps;
pub mod index;
pub mod mcp;
```

- [ ] **Step 4: Update src/main.rs**

Remove the four `mod` declarations at the top of `main.rs`:

```rust
mod codemap;
mod deps;
mod index;
mod mcp;
```

Replace them with a single `use` import:

```rust
use taoki::mcp;
```

That's the only module `main.rs` uses directly. `codemap`, `deps`, and `index` are used internally by `mcp`.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Create benchmark fixture files**

Create `benches/fixtures/` directory and 6 sample files (~200 lines each) covering all structural patterns. These are synthetic but realistic.

Create `benches/fixtures/sample.rs`:
```rust
//! Sample Rust module for benchmarking

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Arc;

const MAX_RETRIES: u32 = 3;
const BUFFER_SIZE: usize = 4096;
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub port: u16,
    pub max_connections: usize,
    pub timeout_ms: u64,
    pub debug: bool,
}

pub enum Status {
    Active,
    Inactive,
    Error(String),
}

pub trait Handler: Send + Sync {
    fn handle(&self, request: &Request) -> Response;
    fn name(&self) -> &str;
}

pub trait Middleware {
    fn process(&self, req: &mut Request) -> bool;
}

impl Config {
    pub fn new(name: String, port: u16) -> Self {
        Self {
            name, port, max_connections: 100, timeout_ms: 5000, debug: false,
        }
    }
    pub fn with_debug(mut self) -> Self { self.debug = true; self }
}

impl Default for Config {
    fn default() -> Self { Self::new("default".into(), 8080) }
}

pub struct Server {
    config: Config,
    handlers: Vec<Box<dyn Handler>>,
}

impl Server {
    pub fn new(config: Config) -> Self { Self { config, handlers: vec![] } }
    pub fn add_handler(&mut self, handler: Box<dyn Handler>) { self.handlers.push(handler); }
    pub fn start(&self) -> io::Result<()> { Ok(()) }
}

pub fn process_request(data: &[u8]) -> Result<Vec<u8>, io::Error> { Ok(data.to_vec()) }
pub fn validate_input(input: &str) -> bool { !input.is_empty() }
fn internal_helper() -> u64 { 42 }

pub mod utils {
    pub fn format_bytes(bytes: usize) -> String { format!("{bytes}B") }
}

macro_rules! log_event {
    ($msg:expr) => { eprintln!("[LOG] {}", $msg) };
}

pub struct Request { pub path: String, pub method: String }
pub struct Response { pub status: u16, pub body: Vec<u8> }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_config_default() { let c = Config::default(); assert_eq!(c.port, 8080); }
    #[test]
    fn test_validate() { assert!(validate_input("hello")); }
}
```

Create `benches/fixtures/sample.py`:
```python
"""Sample Python module for benchmarking."""

import os
import sys
from typing import Optional, Dict, List
from dataclasses import dataclass, field
from pathlib import Path

MAX_RETRIES = 3
DEFAULT_TIMEOUT = 30
BUFFER_SIZE = 4096

app = Flask(__name__)
db = SQLAlchemy()
router = APIRouter()
__version__ = '2.1.0'
__all__ = ['Config', 'Server', 'process_request']

app.register_blueprint(auth_bp)
db.init_app(app)

@dataclass
class Config:
    name: str
    port: int = 8080
    debug: bool = False
    max_connections: int = 100
    timeout_ms: int = 5000

class Server:
    def __init__(self, config: Config):
        self.config = config
        self._handlers = []

    def add_handler(self, handler) -> None:
        self._handlers.append(handler)

    def start(self) -> None:
        pass

    @staticmethod
    def validate(token: str) -> bool:
        return len(token) > 0

class RequestHandler:
    def handle(self, request: dict) -> dict:
        return {'status': 200}

    def _internal_method(self):
        pass

def process_request(data: bytes) -> bytes:
    return data

def validate_input(input_str: str) -> bool:
    return bool(input_str)

def _helper() -> int:
    return 42

def format_bytes(size: int) -> str:
    return f"{size}B"

if __name__ == '__main__':
    app.run(debug=True)

def test_config():
    c = Config(name='test')
    assert c.port == 8080

class TestServer:
    def test_start(self):
        pass
```

Create `benches/fixtures/sample.ts`:
```typescript
import { Request, Response, NextFunction } from 'express';
import { createServer } from 'http';
import type { Config as BaseConfig } from './types';

export const MAX_RETRIES: number = 3;
export const DEFAULT_PORT = 8080;

let serverInstance: Server | null = null;
var legacyConfig = {};

export interface Config extends BaseConfig {
    name: string;
    port: number;
    debug: boolean;
    maxConnections: number;
    timeoutMs: number;
}

export type Handler = (req: Request, res: Response) => void;
export type Middleware = (req: Request, res: Response, next: NextFunction) => void;

export enum Status {
    Active = 'active',
    Inactive = 'inactive',
    Error = 'error',
}

export class Server {
    private config: Config;
    private handlers: Handler[] = [];

    constructor(config: Config) { this.config = config; }
    addHandler(handler: Handler): void { this.handlers.push(handler); }
    start(): Promise<void> { return Promise.resolve(); }
    static validate(token: string): boolean { return token.length > 0; }
}

export function processRequest(data: Buffer): Buffer { return data; }
export function validateInput(input: string): boolean { return input.length > 0; }
function internalHelper(): number { return 42; }

export default class App {
    run(): void {}
}

describe('Server', () => {
    it('should start', () => { expect(true).toBe(true); });
    test('validates input', () => { expect(validateInput('hi')).toBe(true); });
});
```

Create `benches/fixtures/sample.js`:
```javascript
const express = require('express');
const { createServer } = require('http');

const MAX_RETRIES = 3;
const DEFAULT_PORT = 8080;

let app = express();
var config = {};

module.exports = { processRequest, validateInput };
exports.handler = handler;

app.use(express.json());
app.get('/api/health', (req, res) => res.json({ ok: true }));
app.post('/api/data', handler);
router.use('/auth', authMiddleware);

class Server {
    constructor(config) { this.config = config; }
    addHandler(handler) { this.handlers.push(handler); }
    start() { return Promise.resolve(); }
}

function processRequest(data) { return data; }
function validateInput(input) { return !!input; }
function internalHelper() { return 42; }

describe('Server', () => {
    it('should start', () => { expect(true).toBe(true); });
});
```

Create `benches/fixtures/sample.go`:
```go
package main

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"sync"
)

const MaxRetries = 3
const BufferSize = 4096

type Config struct {
	Name           string
	Port           int
	Debug          bool
	MaxConnections int
	TimeoutMs      int
}

type Status int

const (
	Active Status = iota
	Inactive
	Error
)

type Handler interface {
	Handle(r *http.Request) *http.Response
	Name() string
}

type Middleware interface {
	Process(r *http.Request) bool
}

type Server struct {
	config   Config
	handlers []Handler
	mu       sync.Mutex
}

func NewServer(config Config) *Server {
	return &Server{config: config}
}

func (s *Server) AddHandler(h Handler) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.handlers = append(s.handlers, h)
}

func (s *Server) Start() error {
	return nil
}

func ProcessRequest(data []byte) ([]byte, error) {
	return data, nil
}

func ValidateInput(input string) bool {
	return len(input) > 0
}

func internalHelper() int {
	return 42
}

func main() {
	fmt.Println("starting server")
}
```

Create `benches/fixtures/sample.java`:
```java
package com.example.server;

import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.io.IOException;

public class Config {
    private String name;
    private int port;
    private boolean debug;
    private int maxConnections;

    public Config(String name, int port) {
        this.name = name;
        this.port = port;
        this.debug = false;
        this.maxConnections = 100;
    }

    public String getName() { return name; }
    public int getPort() { return port; }
    public boolean isDebug() { return debug; }
}

public interface Handler {
    Map<String, Object> handle(Map<String, Object> request);
    String name();
}

public interface Middleware {
    boolean process(Map<String, Object> request);
}

public enum Status {
    ACTIVE, INACTIVE, ERROR
}

public class Server {
    private Config config;
    private List<Handler> handlers;

    public Server(Config config) { this.config = config; }
    public void addHandler(Handler handler) { handlers.add(handler); }
    public void start() throws IOException {}
    public static boolean validate(String token) { return !token.isEmpty(); }
}

public class RequestProcessor {
    public byte[] processRequest(byte[] data) { return data; }
    public boolean validateInput(String input) { return input != null && !input.isEmpty(); }
}
```

- [ ] **Step 7: Run benchmarks**

Run: `cargo bench`
Expected: benchmark results printed for each language

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml src/lib.rs src/main.rs benches/
git commit -m "feat: add criterion speed benchmarks with fixture files"
```

### Task 5: Add token ratio benchmarks

**Files:**
- Create: `benches/token_ratio.rs`

- [ ] **Step 1: Create benches/token_ratio.rs**

```rust
use criterion::{Criterion, criterion_group, criterion_main, BenchmarkId};

use taoki::index::{index_source, extract_public_api, Language};

fn measure_ratios(c: &mut Criterion) {
    let samples: Vec<(&str, Language, &str)> = vec![
        ("Rust", Language::Rust, include_str!("fixtures/sample.rs")),
        ("Python", Language::Python, include_str!("fixtures/sample.py")),
        ("TypeScript", Language::TypeScript, include_str!("fixtures/sample.ts")),
        ("JavaScript", Language::JavaScript, include_str!("fixtures/sample.js")),
        ("Go", Language::Go, include_str!("fixtures/sample.go")),
        ("Java", Language::Java, include_str!("fixtures/sample.java")),
    ];

    let mut group = c.benchmark_group("byte_ratio");
    for (name, lang, source) in &samples {
        group.bench_with_input(BenchmarkId::new("index", name), source, |b, src| {
            b.iter(|| {
                let output = index_source(src.as_bytes(), *lang).unwrap();
                let ratio = 1.0 - (output.len() as f64 / src.len() as f64);
                assert!(ratio > 0.5, "{name}: byte reduction {:.0}% is below 50% threshold", ratio * 100.0);
                output
            })
        });
    }
    group.finish();

    // Also benchmark extract_public_api ratio
    let mut group2 = c.benchmark_group("byte_ratio_api");
    for (name, lang, source) in &samples {
        group2.bench_with_input(BenchmarkId::new("public_api", name), source, |b, src| {
            b.iter(|| {
                let (types, funcs) = extract_public_api(src.as_bytes(), *lang).unwrap();
                let output_len: usize = types.iter().chain(funcs.iter()).map(|s| s.len()).sum();
                output_len
            })
        });
    }
    group2.finish();

    // Print summary table
    println!("\n## Byte Efficiency Summary\n");
    println!("| Language | Source bytes | Index bytes | Reduction |");
    println!("|----------|-------------|-------------|-----------|");
    for (name, lang, source) in &samples {
        let output = index_source(source.as_bytes(), *lang).unwrap();
        let reduction = 1.0 - (output.len() as f64 / source.len() as f64);
        println!("| {name} | {} | {} | {:.0}% |", source.len(), output.len(), reduction * 100.0);
    }
}

criterion_group!(benches, measure_ratios);
criterion_main!(benches);
```

- [ ] **Step 2: Run it**

Run: `cargo bench --bench token_ratio`
Expected: prints benchmark results and a summary table

- [ ] **Step 3: Commit**

```bash
git add benches/token_ratio.rs
git commit -m "feat: add byte efficiency benchmarks"
```

---

## Chunk 4: Real-world benchmark script

### Task 6: Create scripts/benchmark.sh

**Files:**
- Create: `scripts/benchmark.sh`

- [ ] **Step 1: Create the benchmark script**

```bash
#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"

# Build if needed
if [ ! -f "$BIN" ]; then
  echo "Building taoki in release mode..."
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
fi

TMPDIR_BENCH=""
cleanup() { [ -n "$TMPDIR_BENCH" ] && rm -rf "$TMPDIR_BENCH"; }
trap cleanup EXIT

# Timing helper (macOS date doesn't support %N)
if date +%s%N >/dev/null 2>&1; then
  now_ms() { echo $(( $(date +%s%N) / 1000000 )); }
else
  now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }
fi

TMPDIR_BENCH="$(mktemp -d)"

# Repos to benchmark
REPOS=(
  "pallets/flask"
  "expressjs/express"
  "BurntSushi/ripgrep"
)
LABELS=("flask" "express" "ripgrep")
LANGS=("Python" "JS" "Rust")

send_jsonrpc() {
  local input="$1"
  echo "$input" | "$BIN" 2>/dev/null
}

echo "## Taoki Benchmark Results"
echo ""
echo "| Repo | Language | Files | Source KB | Index KB | Byte Reduction | code_map Cold (ms) | code_map Cached (ms) |"
echo "|------|----------|-------|-----------|----------|----------------|---------------------|----------------------|"

for i in "${!REPOS[@]}"; do
  REPO="${REPOS[$i]}"
  LABEL="${LABELS[$i]}"
  LANG="${LANGS[$i]}"
  REPO_DIR="$TMPDIR_BENCH/$LABEL"

  # Clone
  echo "Cloning $REPO..." >&2
  git clone --depth 1 "https://github.com/$REPO.git" "$REPO_DIR" 2>/dev/null

  # Count source files and sizes
  EXTENSIONS=""
  case "$LANG" in
    Python) EXTENSIONS="py" ;;
    JS) EXTENSIONS="js|mjs|cjs" ;;
    Rust) EXTENSIONS="rs" ;;
  esac

  FILE_COUNT=0
  SOURCE_BYTES=0
  INDEX_BYTES=0

  while IFS= read -r file; do
    size=$(wc -c < "$file")
    SOURCE_BYTES=$((SOURCE_BYTES + size))
    FILE_COUNT=$((FILE_COUNT + 1))

    # Run index on each file via MCP
    INDEX_REQ=$(cat <<JSONEOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"path":"$file"}}}
JSONEOF
)
    RESULT=$(echo "$INDEX_REQ" | "$BIN" 2>/dev/null || true)
    # Extract the text content length from the JSON response
    CONTENT=$(echo "$RESULT" | grep -o '"text":"[^"]*"' | tail -1 | sed 's/"text":"//;s/"$//' || echo "")
    if [ -n "$CONTENT" ]; then
      INDEX_BYTES=$((INDEX_BYTES + ${#CONTENT}))
    fi
  done < <(find "$REPO_DIR" -type f | grep -E "\\.($EXTENSIONS)$" | head -100)

  # code_map cold
  MAP_REQ=$(cat <<JSONEOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"code_map","arguments":{"path":"$REPO_DIR"}}}
JSONEOF
)

  # Remove cache for cold run
  rm -rf "$REPO_DIR/.cache/taoki"

  COLD_START=$(now_ms)
  echo "$MAP_REQ" | "$BIN" >/dev/null 2>&1 || true
  COLD_END=$(now_ms)
  COLD_MS=$(( COLD_END - COLD_START ))

  # Cached run
  CACHED_START=$(now_ms)
  echo "$MAP_REQ" | "$BIN" >/dev/null 2>&1 || true
  CACHED_END=$(now_ms)
  CACHED_MS=$(( CACHED_END - CACHED_START ))

  # Calculate reduction
  SOURCE_KB=$((SOURCE_BYTES / 1024))
  INDEX_KB=$((INDEX_BYTES / 1024))
  if [ "$SOURCE_BYTES" -gt 0 ]; then
    REDUCTION=$(( (SOURCE_BYTES - INDEX_BYTES) * 100 / SOURCE_BYTES ))
  else
    REDUCTION=0
  fi

  echo "| $LABEL | $LANG | $FILE_COUNT | $SOURCE_KB | $INDEX_KB | ${REDUCTION}% | $COLD_MS | $CACHED_MS |"
done
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/benchmark.sh`

- [ ] **Step 3: Verify syntax**

Run: `bash -n scripts/benchmark.sh && echo "Syntax OK"`
Expected: `Syntax OK`

- [ ] **Step 4: Commit**

```bash
git add scripts/benchmark.sh
git commit -m "feat: add real-world benchmark script for token savings measurement"
```

### Task 7: Run the benchmark script and verify

- [ ] **Step 1: Run the benchmark**

Run: `bash scripts/benchmark.sh`
Expected: a markdown table with results for flask, express, and ripgrep

- [ ] **Step 2: Verify results look reasonable**

Check that byte reduction is in the 60-90% range for all languages. If any repo fails or produces 0%, investigate.

- [ ] **Step 3: Commit any fixes if needed**
