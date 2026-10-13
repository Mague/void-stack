//! Result shaping after fusion: what kind of chunk each hit is, how much
//! that kind is worth for this query, and how many hits one file may take.
//!
//! The index stores every chunk the same way; whether a chunk is a test,
//! a changelog entry or a block of `import`s is decided here, from its path
//! and text, so existing indexes benefit without being rebuilt.

use std::collections::HashMap;

/// What a chunk is, as far as ranking cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkKind {
    Code,
    Test,
    Doc,
    Changelog,
    /// Nothing but import / use / package / require lines.
    Imports,
}

impl ChunkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChunkKind::Code => "code",
            ChunkKind::Test => "test",
            ChunkKind::Doc => "doc",
            ChunkKind::Changelog => "changelog",
            ChunkKind::Imports => "imports",
        }
    }
}

/// Most chunks one file may contribute to a top-k (diversity).
pub(crate) const MAX_PER_FILE: usize = 2;

/// Classify a chunk by its path first (tests, docs, changelogs), then by
/// its text (a chunk that only imports things).
pub fn chunk_kind(file_path: &str, text: &str) -> ChunkKind {
    let p = file_path.replace('\\', "/").to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    if name.starts_with("changelog") || name.starts_with("history.md") || name == "news.md" {
        return ChunkKind::Changelog;
    }
    if is_test_path(&p, name) {
        return ChunkKind::Test;
    }
    if name.ends_with(".md")
        || name.ends_with(".mdx")
        || name.ends_with(".rst")
        || name.ends_with(".txt")
    {
        return ChunkKind::Doc;
    }
    if is_imports_only(text) {
        return ChunkKind::Imports;
    }
    if has_test_markers(text) {
        return ChunkKind::Test;
    }
    ChunkKind::Code
}

/// Inline tests in a source file: Rust `#[test]` / `mod tests`, Python
/// `def test_`, Go `func TestX`, JS/Dart `describe(` / `test(` / `it(`.
fn has_test_markers(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("#[test]")
            || t.starts_with("#[tokio::test")
            || t.starts_with("#[cfg(test)]")
            || t.starts_with("fn test_")
            || t.starts_with("async fn test_")
            || t.starts_with("def test_")
            || t.starts_with("async def test_")
            || (t.starts_with("func Test") && t.contains("*testing.T"))
            || t.starts_with("describe(")
            || t.starts_with("test(\"")
            || t.starts_with("test('")
            || t.starts_with("testWidgets(")
            || t.starts_with("it(\"")
            || t.starts_with("it('")
    })
}

fn is_test_path(p: &str, name: &str) -> bool {
    let in_test_dir = p.split('/').any(|seg| {
        matches!(
            seg,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "testdata"
        )
    });
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    in_test_dir
        || stem.ends_with("_test")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || stem.ends_with("_spec")
        || stem.ends_with("_tests")
        || (name.starts_with("test_") && name.ends_with(".py"))
}

/// True when every meaningful line imports, re-exports or declares a
/// module — the preamble block a chunker emits before the first function.
pub(crate) fn is_imports_only(text: &str) -> bool {
    let mut meaningful = 0usize;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || is_comment(t) || is_bracket_only(t) {
            continue;
        }
        if !is_import_line(t) {
            return false;
        }
        meaningful += 1;
    }
    meaningful >= 2
}

fn is_comment(t: &str) -> bool {
    t.starts_with("//")
        || t.starts_with('#') && !t.starts_with("#include")
        || t.starts_with("/*")
        || t.starts_with('*')
        || t.starts_with("--")
        || t.starts_with("\"\"\"")
}

fn is_bracket_only(t: &str) -> bool {
    t.chars()
        .all(|c| matches!(c, '}' | ')' | ']' | ';' | ',' | '{' | '('))
}

pub(crate) fn is_import_line(t: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "import ",
        "import{",
        "import(",
        "from ",
        "use ",
        "pub use ",
        "pub(crate) use ",
        "extern crate ",
        "mod ",
        "pub mod ",
        "pub(crate) mod ",
        "package ",
        "library ",
        "part ",
        "part of ",
        "export * ",
        "export {",
        "#include",
        "require ",
        "require_relative ",
        "using ",
        "alias ",
        "\"use client\"",
        "'use client'",
        "\"use strict\"",
        "'use strict'",
    ];
    if PREFIXES.iter().any(|p| t.starts_with(p)) {
        // `use` inside Rust also opens closures in some DSLs; a `mod x {`
        // block with a body is code, not an import.
        return !(t.starts_with("mod ") || t.starts_with("pub mod ")) || t.ends_with(';');
    }
    // Continuation lines of a multi-line import list: `Foo,` / `"pkg/x"`.
    let ident_list = t
        .trim_end_matches(',')
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .all(|s| {
            s.chars().all(|c| {
                c.is_alphanumeric()
                    || matches!(c, '_' | ':' | '{' | '}' | '*' | '"' | '/' | '.' | '-' | '@')
            })
        });
    (t.contains('"') && !t.contains('=') && !t.contains('('))
        || (ident_list && t.ends_with(','))
        || (t.starts_with("const ") && t.contains("require("))
}

/// Which kinds the query explicitly asks for — those are never penalized.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct QueryIntent {
    pub tests: bool,
    pub imports: bool,
    pub docs: bool,
    pub changelog: bool,
}

pub(crate) fn query_intent(query: &str) -> QueryIntent {
    let q = query.to_ascii_lowercase();
    let has = |words: &[&str]| {
        q.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| {
                words
                    .iter()
                    .any(|x| w == *x || (x.len() > 4 && w.starts_with(x)))
            })
    };
    QueryIntent {
        tests: has(&[
            "test", "tests", "testing", "spec", "specs", "prueba", "pruebas", "fixture", "mock",
        ]),
        imports: has(&[
            "import",
            "imports",
            "depend",
            "dependency",
            "dependencies",
            "require",
            "uses",
        ]),
        docs: has(&[
            "readme",
            "doc",
            "docs",
            "documentation",
            "documentacion",
            "install",
            "installation",
            "guide",
            "tutorial",
            "setup",
        ]),
        changelog: has(&[
            "changelog",
            "release",
            "releases",
            "version",
            "history",
            "cambios",
        ]),
    }
}

/// Multiplier applied to a fused score. `None` drops the hit entirely.
pub(crate) fn kind_weight(kind: ChunkKind, intent: QueryIntent) -> Option<f32> {
    match kind {
        ChunkKind::Code => Some(1.0),
        ChunkKind::Imports if intent.imports => Some(1.0),
        ChunkKind::Imports => None,
        ChunkKind::Test if intent.tests => Some(1.0),
        ChunkKind::Test => Some(0.6),
        ChunkKind::Changelog if intent.changelog => Some(1.0),
        ChunkKind::Changelog => Some(0.4),
        ChunkKind::Doc if intent.docs => Some(1.0),
        ChunkKind::Doc => Some(0.8),
    }
}

/// A fused candidate ready for reranking.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub id: i64,
    pub file: String,
    pub score: f32,
}

/// Sort by adjusted score, then keep at most [`MAX_PER_FILE`] hits per
/// file. If that leaves fewer than `top_k`, the overflow fills the gap in
/// score order — a narrow query on one file still gets `top_k` answers.
pub(crate) fn diversify(mut cands: Vec<Candidate>, top_k: usize) -> Vec<Candidate> {
    cands.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut per_file: HashMap<String, usize> = HashMap::new();
    let mut kept = Vec::with_capacity(top_k);
    let mut overflow = Vec::new();
    for c in cands {
        let n = per_file.entry(c.file.clone()).or_default();
        if *n < MAX_PER_FILE {
            *n += 1;
            kept.push(c);
        } else {
            overflow.push(c);
        }
        if kept.len() >= top_k {
            return kept;
        }
    }
    kept.extend(overflow.into_iter().take(top_k - kept.len()));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_path() {
        assert_eq!(chunk_kind("CHANGELOG.md", "## 1.0"), ChunkKind::Changelog);
        assert_eq!(
            chunk_kind("src/foo_test.go", "func TestX() {}"),
            ChunkKind::Test
        );
        assert_eq!(chunk_kind("tests/api.rs", "fn x() {}"), ChunkKind::Test);
        assert_eq!(chunk_kind("web/a.spec.ts", "it('x')"), ChunkKind::Test);
        assert_eq!(
            chunk_kind("test_models.py", "def test_a(): pass"),
            ChunkKind::Test
        );
        assert_eq!(
            chunk_kind("lib/a_test.dart", "void main() {}"),
            ChunkKind::Test
        );
        assert_eq!(chunk_kind("README.md", "# Title"), ChunkKind::Doc);
        assert_eq!(chunk_kind("src/main.rs", "fn main() {}"), ChunkKind::Code);
        // "contest.rs" is not a test file.
        assert_eq!(chunk_kind("src/contest.rs", "fn run() {}"), ChunkKind::Code);
        // Inline tests inside a source file are tests too.
        let inline = "    #[test]\n    fn parses_ok() {\n        assert!(parse(\"x\"));\n    }\n";
        assert_eq!(chunk_kind("src/parser.rs", inline), ChunkKind::Test);
        assert_eq!(
            chunk_kind("pkg/a.go", "func TestA(t *testing.T) {}"),
            ChunkKind::Test
        );
        assert_eq!(
            chunk_kind("lib/w.dart", "  testWidgets('x', (t) async {});"),
            ChunkKind::Test
        );
        // Calling a function named test… is not a test.
        assert_eq!(
            chunk_kind("src/a.rs", "fn run() {\n    test_helper();\n}"),
            ChunkKind::Code
        );
    }

    #[test]
    fn detects_import_only_chunks_across_languages() {
        let dart = "// Used by: main\nimport 'package:flutter/material.dart';\nimport 'package:x/y.dart';\n";
        assert_eq!(chunk_kind("lib/main.dart", dart), ChunkKind::Imports);
        let ts = "'use client'\nimport { a, b } from './x'\nimport React from 'react'\n";
        assert_eq!(chunk_kind("app/page.tsx", ts), ChunkKind::Imports);
        let rs = "use std::fs;\nuse crate::{\n    a,\n    b::c,\n};\n\npub mod x;\n";
        assert_eq!(chunk_kind("src/lib.rs", rs), ChunkKind::Imports);
        let go = "package main\n\nimport (\n    \"fmt\"\n    \"net/http\"\n)\n";
        assert_eq!(chunk_kind("main.go", go), ChunkKind::Imports);
        let py = "from a import b\nimport os\n";
        assert_eq!(chunk_kind("x.py", py), ChunkKind::Imports);
    }

    #[test]
    fn real_code_is_not_imports() {
        let rs = "use std::fs;\n\nfn main() {\n    fs::read(\"x\");\n}\n";
        assert_eq!(chunk_kind("src/main.rs", rs), ChunkKind::Code);
        let ts = "import x from 'y'\nexport function go() { return x() }\n";
        assert_eq!(chunk_kind("a.ts", ts), ChunkKind::Code);
        let rs_mod = "mod tests {\n    fn a() {}\n}\n";
        assert_eq!(chunk_kind("src/a.rs", rs_mod), ChunkKind::Code);
        // A single import line is too little to call the chunk noise.
        assert_eq!(chunk_kind("a.py", "import os\n"), ChunkKind::Code);
        let cfg = "const config = {\n  port: 8080,\n  host: \"localhost\",\n}\n";
        assert_eq!(chunk_kind("config.js", cfg), ChunkKind::Code);
    }

    #[test]
    fn intent_lifts_penalties() {
        let none = query_intent("authentication middleware flow");
        assert_eq!(kind_weight(ChunkKind::Test, none), Some(0.6));
        assert_eq!(kind_weight(ChunkKind::Imports, none), None);
        assert_eq!(kind_weight(ChunkKind::Changelog, none), Some(0.4));
        assert_eq!(kind_weight(ChunkKind::Doc, none), Some(0.8));
        let t = query_intent("unit tests for the parser");
        assert_eq!(kind_weight(ChunkKind::Test, t), Some(1.0));
        let i = query_intent("what does this module depend on");
        assert_eq!(kind_weight(ChunkKind::Imports, i), Some(1.0));
        let d = query_intent("installation instructions");
        assert_eq!(kind_weight(ChunkKind::Doc, d), Some(1.0));
    }

    #[test]
    fn diversify_caps_hits_per_file_then_backfills() {
        let c = |id, file: &str, score| Candidate {
            id,
            file: file.into(),
            score,
        };
        let cands = vec![
            c(1, "a.rs", 0.9),
            c(2, "a.rs", 0.8),
            c(3, "a.rs", 0.7),
            c(4, "b.rs", 0.6),
            c(5, "c.rs", 0.5),
        ];
        let ids: Vec<i64> = diversify(cands.clone(), 4).iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            vec![1, 2, 4, 5],
            "third a.rs hit yields to other files"
        );
        // Only one file has hits: backfill instead of returning fewer.
        let one = vec![c(1, "a.rs", 0.9), c(2, "a.rs", 0.8), c(3, "a.rs", 0.7)];
        assert_eq!(diversify(one, 3).len(), 3);
    }
}
