use std::fmt::Write;
use std::path::Path;

use tree_sitter::{Node, Parser};

mod languages;
pub(crate) mod body;

pub(crate) const FIELD_TRUNCATE_THRESHOLD: usize = 8;
pub(crate) const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;
const MINIFIED_AVG_LINE_LEN: usize = 500;

/// Returns true if the source appears to be minified or bundled code.
///
/// Detected by average line length exceeding 500 characters, which indicates
/// machine-generated code with no useful structure to extract.
pub fn is_minified(source: &[u8]) -> bool {
    if source.is_empty() {
        return false;
    }
    let newlines = source.iter().filter(|&&b| b == b'\n').count();
    let lines = if newlines == 0 {
        1
    } else if source.last() == Some(&b'\n') {
        newlines // trailing newline doesn't start a new line
    } else {
        newlines + 1 // no trailing newline means last line isn't counted
    };
    source.len() / lines > MINIFIED_AVG_LINE_LEN
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[allow(dead_code)]
    #[error("unsupported file type: {0}")]
    UnsupportedLanguage(String),
    #[allow(dead_code)]
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

    pub(crate) fn ts_language(&self) -> tree_sitter::Language {
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

pub(crate) fn line_range(start: usize, end: usize) -> String {
    if start == end {
        format!("[{start}]")
    } else {
        format!("[{start}-{end}]")
    }
}

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

pub(crate) fn node_text<'a>(node: Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

pub(crate) fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node.children(&mut cursor).find(|c| c.kind() == kind);
    result
}

pub(crate) fn prefixed(vis: &str, rest: std::fmt::Arguments<'_>) -> String {
    if vis.is_empty() {
        format!("{rest}")
    } else {
        format!("{vis} {rest}")
    }
}

pub(crate) fn has_test_attr(attrs: &[Node], source: &[u8]) -> bool {
    attrs.iter().any(|a| {
        let text = node_text(*a, source);
        text == "#[test]" || text == "#[cfg(test)]" || text.ends_with("::test]")
    })
}

pub(crate) fn vis_prefix<'a>(node: Node<'a>, source: &'a [u8]) -> &'a str {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return node_text(child, source);
        }
    }
    ""
}

pub(crate) fn relevant_attr_texts(attrs: &[Node], source: &[u8]) -> Vec<String> {
    attrs
        .iter()
        .filter_map(|a| {
            let text = node_text(*a, source);
            (text.contains("derive") || text.contains("cfg")).then(|| text.to_string())
        })
        .collect()
}

pub(crate) fn fn_signature(node: Node, source: &[u8]) -> Option<String> {
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(n, source))?;
    let params = find_child(node, "parameters")
        .map(|n| node_text(n, source))
        .unwrap_or("()");
    let ret = node
        .child_by_field_name("return_type")
        .map(|n| {
            let t = node_text(n, source);
            if t.starts_with("->") {
                format!(" {t}")
            } else {
                format!(" -> {t}")
            }
        })
        .unwrap_or_default();
    Some(format!("{name}{params}{ret}"))
}

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

impl Section {
    pub(crate) fn header(self) -> &'static str {
        match self {
            Self::Import => "imports:",
            Self::Constant => "consts:",
            Self::Expression => "exprs:",
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
    pub(crate) insights: self::body::BodyInsights,
    pub(crate) doc: Option<String>,
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
            insights: self::body::BodyInsights::default(),
            doc: None,
        }
    }
}

pub struct PublicApi {
    pub types: Vec<String>,
    pub functions: Vec<String>,
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
    /// Strip language-specific doc comment prefix from a single line.
    /// Returns None if the line is empty after stripping.
    fn strip_doc_prefix(&self, _text: &str) -> Option<String> {
        None
    }
    /// Extract the first line of the doc comment for a node.
    /// Default: walk backward through prev_sibling, collect doc comments,
    /// reverse, take the first one, call strip_doc_prefix.
    fn extract_doc_line(&self, node: Node, source: &[u8]) -> Option<String> {
        let mut doc_nodes = Vec::new();
        let mut prev = node.prev_sibling();
        while let Some(p) = prev {
            if self.is_attr(p) {
                prev = p.prev_sibling();
                continue;
            }
            if self.is_doc_comment(p, source) {
                doc_nodes.push(p);
                prev = p.prev_sibling();
            } else {
                break;
            }
        }
        if doc_nodes.is_empty() {
            return None;
        }
        // Backward walk finds topmost last — reverse to get first doc line
        doc_nodes.reverse();
        let text = node_text(doc_nodes[0], source);
        let stripped = self.strip_doc_prefix(text)?;
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(truncate(trimmed, 120))
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

pub(crate) fn doc_comment_start_line(
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

pub(crate) fn detect_module_doc(
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
    test_lines: &[(usize, usize)],
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
                if let Some(ref doc) = entry.doc {
                    let _ = writeln!(out, "    /// {doc}");
                }
                for child in &entry.children {
                    let _ = writeln!(out, "    {child}");
                }
                for line in entry.insights.format_lines() {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
    }

    if !test_lines.is_empty() {
        let min = test_lines.iter().map(|(s, _)| *s).min().unwrap();
        let max = test_lines.iter().map(|(_, e)| *e).max().unwrap();
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

#[allow(dead_code)]
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

    Ok(build_skeleton(root, source, extractor, lang))
}

pub fn extract_all(source: &[u8], lang: Language) -> Result<(PublicApi, String), IndexError> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .map_err(|_| IndexError::ParseFailed)?;

    let tree = parser.parse(source, None).ok_or(IndexError::ParseFailed)?;
    let root = tree.root_node();
    let extractor = lang.extractor();

    let api = extractor.extract_public_api(root, source);
    let skeleton = build_skeleton(root, source, extractor, lang);
    Ok((api, skeleton))
}

fn build_skeleton(root: Node, source: &[u8], extractor: &dyn LanguageExtractor, lang: Language) -> String {
    let module_doc = detect_module_doc(root, source, extractor);
    let mut entries = Vec::new();
    let mut test_lines: Vec<(usize, usize)> = Vec::new();

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if extractor.is_attr(child) || extractor.is_doc_comment(child, source) {
            continue;
        }
        let attrs = extractor.collect_preceding_attrs(child);
        if extractor.is_test_node(child, source, &attrs) {
            test_lines.push((child.start_position().row + 1, child.end_position().row + 1));
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
                entry.doc = extractor.extract_doc_line(child, source);
            }
            // Analyze body for top-level functions and Go methods (which use Section::Impl)
            let is_function = entry.section == Section::Function;
            let is_go_method = lang == Language::Go && child.kind() == "method_declaration";
            if is_function || is_go_method {
                entry.insights = body::analyze_body(child, source, lang);
            }
            entries.push(entry);
        }
    }

    format_skeleton(&entries, &test_lines, module_doc)
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

/// Process the input string.
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
            "/// Process the input string.",
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

/** The main handler. */
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
            "/// The main handler.",
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

    #[test]
    fn extract_all_returns_api_and_skeleton() {
        let src = "pub struct Foo {}\npub fn bar() {}\nfn private() {}\n";
        let (api, skeleton) = extract_all(src.as_bytes(), Language::Rust).unwrap();
        assert!(api.types.contains(&"Foo".to_string()));
        assert!(api.functions.iter().any(|f| f.starts_with("bar(")));
        assert!(!api.functions.iter().any(|f| f.starts_with("private(")));
        assert!(skeleton.contains("types:"));
        assert!(skeleton.contains("Foo"));
        assert!(skeleton.contains("fns:"));
        assert!(skeleton.contains("bar()"));
    }

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

    #[test]
    fn rust_doc_comment_extracted() {
        let src = "\
/// Fetches user from the database.
pub fn fetch_user(id: &str) -> User { todo!() }

/// Configuration for the service.
pub struct Config {
    pub host: String,
}

fn no_doc() {}
";
        let out = idx(src, Language::Rust);
        has(&out, &[
            "/// Fetches user from the database.",
            "/// Configuration for the service.",
        ]);
        lacks(&out, &["/// no_doc"]);
    }

    #[test]
    fn rust_doc_multiline_takes_first() {
        let src = "\
/// Summary line here.
/// More details on second line.
/// Even more details.
pub fn documented() {}
";
        let out = idx(src, Language::Rust);
        has(&out, &["/// Summary line here."]);
        lacks(&out, &["More details", "Even more"]);
    }

    #[test]
    fn rust_doc_with_attrs() {
        let src = "\
/// Does the thing.
#[derive(Debug)]
pub struct Thing {}
";
        let out = idx(src, Language::Rust);
        has(&out, &["/// Does the thing."]);
    }

    #[test]
    fn rust_no_doc_no_line() {
        let src = "pub fn bare() {}\n";
        let out = idx(src, Language::Rust);
        lacks(&out, &["///"]);
    }

    #[test]
    fn rust_empty_doc_comment_ignored() {
        let src = "///\n///   \npub fn blank_doc() {}\n";
        let out = idx(src, Language::Rust);
        lacks(&out, &["/// \n"]);
        // The output should contain the function but no doc line
        has(&out, &["pub blank_doc()"]);
    }

    #[test]
    fn rust_block_doc_comment_ignored() {
        // Rust /** */ block doc comments are intentionally not extracted.
        // They are extremely rare in practice (/// is the overwhelming convention)
        // and is_doc_comment only matches line_comment nodes, not block_comment.
        let src = "/** Block doc. */\npub fn block_doc() {}\n";
        let out = idx(src, Language::Rust);
        has(&out, &["pub block_doc()"]);
        lacks(&out, &["/// Block doc"]);
    }

    #[test]
    fn ts_doc_comment_extracted() {
        let src = "\
/** Handles incoming requests. */
export function handleRequest(req: Request): Response { return req; }

/**
 * Application configuration.
 * Loaded from environment variables.
 */
export interface Config {
    port: number;
    host: string;
}

function undocumented() {}
";
        let out = idx(src, Language::TypeScript);
        has(&out, &[
            "/// Handles incoming requests.",
            "/// Application configuration.",
        ]);
        lacks(&out, &["/// undocumented", "Loaded from"]);
    }

    #[test]
    fn ts_line_comment_blocks_doc_attribution() {
        let src = "\
/** Doc for something else. */
function foo() {}
// unrelated comment
export function bar() {}
";
        let out = idx(src, Language::TypeScript);
        has(&out, &["/// Doc for something else."]);
        // bar must NOT get foo's doc — the // comment is a barrier
        lacks(&out, &["bar\n        /// Doc"]);
    }

    #[test]
    fn go_doc_comment_extracted() {
        let src = "\
package main

// FetchUser retrieves a user by ID.
func FetchUser(id string) (*User, error) { return nil, nil }

// Config holds application settings.
type Config struct {
    Host string
    Port int
}

// not adjacent — blank line separates

func Bare() {}
";
        let out = idx(src, Language::Go);
        has(&out, &[
            "/// FetchUser retrieves a user by ID.",
            "/// Config holds application settings.",
        ]);
        lacks(&out, &["/// not adjacent", "/// Bare"]);
    }

    #[test]
    fn java_doc_comment_extracted() {
        let src = "\
package com.example;

/** Handles user operations. */
public class UserService {
    public void doStuff() {}
}

/**
 * Represents a user in the system.
 * Contains identity and role information.
 */
public record User(String name, String role) {}

public class Bare {}
";
        let out = idx(src, Language::Java);
        has(&out, &[
            "/// Handles user operations.",
            "/// Represents a user in the system.",
        ]);
        lacks(&out, &["/// Bare", "Contains identity"]);
    }

    #[test]
    fn java_line_comment_blocks_doc_attribution() {
        let src = "\
package com.example;

/** Doc for SomeOtherClass. */
class SomeOtherClass {}
// TODO: remove this
public class MyClass {}
";
        let out = idx(src, Language::Java);
        has(&out, &["/// Doc for SomeOtherClass."]);
        // MyClass must NOT get SomeOtherClass's doc
        lacks(&out, &["MyClass\n        /// Doc"]);
    }

    #[test]
    fn python_raw_and_single_quote_docstrings() {
        let src = r#"
def raw_doc():
    r"""Raw docstring content."""
    pass

def single_quote_doc():
    '''Single-quoted docstring.'''
    pass
"#;
        let out = idx(src, Language::Python);
        has(&out, &[
            "/// Raw docstring content.",
            "/// Single-quoted docstring.",
        ]);
    }

    #[test]
    fn python_doc_comment_extracted() {
        let src = r#"
def fetch_user(user_id: str) -> User:
    """Fetch a user from the database."""
    pass

class Config:
    """Application configuration."""
    host: str
    port: int

def bare():
    pass

def multiline_doc():
    """
    Summary on second line.
    More details here.
    """
    pass
"#;
        let out = idx(src, Language::Python);
        has(&out, &[
            "/// Fetch a user from the database.",
            "/// Application configuration.",
            "/// Summary on second line.",
        ]);
        lacks(&out, &["/// bare", "More details"]);
    }

    #[test]
    fn python_empty_docstring_ignored() {
        let src = "def empty():\n    \"\"\"   \"\"\"\n    pass\n";
        let out = idx(src, Language::Python);
        has(&out, &["empty()"]);
        lacks(&out, &["///"]);
    }

    #[test]
    fn test_range_includes_last_test_body() {
        // Two test functions — the range should span from first start to last END
        let src = r#"
def add(a, b):
    return a + b

def test_add():
    assert add(1, 2) == 3

def test_subtract():
    result = add(5, 3)
    assert result == 2
"#;
        let out = idx(src, Language::Python);
        // test_add starts at line 5, test_subtract ends at line 10
        // The tests range must include line 10, not stop at line 8
        has(&out, &["tests: [5-10]"]);
    }

    #[test]
    fn doc_comment_truncated_at_120() {
        let long_doc = format!("/// {}", "a".repeat(130));
        let src = format!("{long_doc}\npub fn long_doc() {{}}\n");
        let out = idx(&src, Language::Rust);
        assert!(out.contains("..."), "expected truncation in:\n{out}");
        // The doc line in output should be <= 120 chars (excluding the "    /// " prefix)
        for line in out.lines() {
            if line.contains("/// ") && line.contains("...") {
                let doc_content = line.trim().strip_prefix("/// ").unwrap();
                assert!(doc_content.chars().count() <= 120,
                    "doc too long ({} chars): {doc_content}", doc_content.chars().count());
            }
        }
    }

    #[test]
    fn rust_pub_crate_in_public_api() {
        let src = r#"
pub(crate) struct Foo {
    pub(crate) field: i32,
}

pub(crate) fn bar() -> bool { true }

pub struct Visible;

pub(super) fn baz() -> i32 { 42 }

fn private_fn() {}

struct Private;
"#;
        let (types, functions) =
            extract_public_api(src.as_bytes(), Language::Rust).unwrap();
        // pub(crate) items should now appear
        assert!(types.contains(&"Foo".to_string()), "missing pub(crate) struct Foo");
        assert!(types.contains(&"Visible".to_string()), "missing pub struct Visible");
        assert!(!types.contains(&"Private".to_string()), "private struct should be excluded");
        // pub(crate) fn should appear
        assert!(functions.iter().any(|f| f.contains("bar")), "missing pub(crate) fn bar");
        // pub(super) fn should also appear
        assert!(functions.iter().any(|f| f.contains("baz")), "missing pub(super) fn baz");
        // private fn should not
        assert!(!functions.iter().any(|f| f.contains("private_fn")), "private fn should be excluded");
    }

    #[test]
    fn java_enum_with_methods() {
        let src = r#"
public enum Role {
    ADMIN,
    EDITOR,
    VIEWER;

    public boolean canEdit() {
        return this == ADMIN || this == EDITOR;
    }

    public String label() {
        return name().toLowerCase();
    }
}
"#;
        let out = idx(src, Language::Java);
        // Constants should appear
        assert!(out.contains("ADMIN"), "missing ADMIN constant");
        assert!(out.contains("EDITOR"), "missing EDITOR constant");
        assert!(out.contains("VIEWER"), "missing VIEWER constant");
        // Methods should appear with signatures
        assert!(out.contains("public boolean canEdit()"), "missing canEdit method");
        assert!(out.contains("public String label()"), "missing label method");
    }

    #[test]
    fn java_enum_no_methods_unchanged() {
        let src = r#"
public enum Color {
    RED,
    GREEN,
    BLUE;
}
"#;
        let out = idx(src, Language::Java);
        assert!(out.contains("RED"));
        assert!(out.contains("GREEN"));
        assert!(out.contains("BLUE"));
    }

    #[test]
    fn java_enum_with_fields_and_constructor() {
        let src = r#"
public enum Planet {
    MERCURY(3.303e+23, 2.4397e6),
    VENUS(4.869e+24, 6.0518e6);

    private final double mass;
    private final double radius;

    Planet(double mass, double radius) {
        this.mass = mass;
        this.radius = radius;
    }

    public double surfaceGravity() {
        return 6.67300E-11 * mass / (radius * radius);
    }
}
"#;
        let out = idx(src, Language::Java);
        assert!(out.contains("MERCURY"), "missing MERCURY");
        assert!(out.contains("VENUS"), "missing VENUS");
        assert!(out.contains("private final double mass"), "missing mass field");
        assert!(out.contains("Planet(double mass, double radius)"), "missing constructor");
        assert!(out.contains("public double surfaceGravity()"), "missing method");
    }

    #[test]
    fn minified_single_long_line() {
        let source = "a".repeat(600);
        assert!(is_minified(source.as_bytes()));
    }

    #[test]
    fn minified_high_avg_line_length() {
        let line = "a".repeat(600);
        let source = format!("{}\n{}\n", line, line);
        assert!(is_minified(source.as_bytes()));
    }

    #[test]
    fn not_minified_normal_code() {
        let source = "fn main() {\n    println!(\"hello\");\n}\n";
        assert!(!is_minified(source.as_bytes()));
    }

    #[test]
    fn not_minified_empty() {
        assert!(!is_minified(b""));
    }

    #[test]
    fn not_minified_two_long_lines_no_trailing_newline() {
        // Two 300-char lines with one newline, no trailing newline.
        // Avg line length is 300, well under 500 threshold.
        let source = format!("{}\n{}", "a".repeat(300), "b".repeat(300));
        assert!(!is_minified(source.as_bytes()));
    }

    #[test]
    fn not_minified_single_line_under_threshold() {
        let source = "a".repeat(400);
        assert!(!is_minified(source.as_bytes()));
    }
}
