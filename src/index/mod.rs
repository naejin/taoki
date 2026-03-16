use std::fmt::Write;
use std::path::Path;

use tree_sitter::{Node, Parser};

mod languages;
pub(crate) mod body;

pub(crate) const FIELD_TRUNCATE_THRESHOLD: usize = 8;
pub(crate) const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

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
                for line in entry.insights.format_lines() {
                    let _ = writeln!(out, "    {line}");
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
}
