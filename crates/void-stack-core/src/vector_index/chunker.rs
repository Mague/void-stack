//! Code chunking: function-aware and line-based strategies.

/// Lines per chunk (target).
pub(crate) const CHUNK_LINES: usize = 40;
/// Min lines for a chunk to be indexed.
const MIN_CHUNK_LINES: usize = 5;
/// Max lines for a single function chunk before splitting.
const MAX_FUNCTION_LINES: usize = 150;
/// Max lines per sub-chunk when splitting large functions.
const MAX_CHUNK_FOR_FUNCTIONS: usize = 80;

/// A code chunk with metadata.
#[derive(Clone)]
pub(crate) struct Chunk {
    pub file_path: String,
    pub text: String,
    pub line_start: usize,
    pub line_end: usize,
}

/// Prepend a compact structural summary (who imports this file, what it imports)
/// to a chunk's text so the embedding captures dependency context. Caps at
/// 3 names per list to keep the prefix under ~2 lines and avoid dominating
/// the chunk semantics.
pub(crate) fn enrich_chunk_with_context(
    chunk: &mut Chunk,
    imports: &[String],
    imported_by: &[String],
) {
    if imports.is_empty() && imported_by.is_empty() {
        return;
    }

    let short = |p: &str| -> String {
        p.rsplit('/')
            .next()
            .unwrap_or(p)
            .trim_end_matches(".rs")
            .trim_end_matches(".ts")
            .trim_end_matches(".tsx")
            .trim_end_matches(".js")
            .trim_end_matches(".jsx")
            .trim_end_matches(".py")
            .trim_end_matches(".go")
            .trim_end_matches(".dart")
            .to_string()
    };

    let mut context_lines = Vec::new();

    if !imported_by.is_empty() {
        let names: Vec<String> = imported_by.iter().take(3).map(|s| short(s)).collect();
        context_lines.push(format!("// Used by: {}", names.join(", ")));
    }

    if !imports.is_empty() {
        let names: Vec<String> = imports.iter().take(3).map(|s| short(s)).collect();
        context_lines.push(format!("// Imports: {}", names.join(", ")));
    }

    chunk.text = format!("{}\n{}", context_lines.join("\n"), chunk.text);
}

/// Chunk a file using function-aware boundaries when possible.
/// Falls back to blank-line chunking for unsupported extensions.
pub(crate) fn chunk_file(file_path: &str, content: &str) -> Vec<Chunk> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < MIN_CHUNK_LINES {
        if lines.is_empty() {
            return vec![];
        }
        return vec![Chunk {
            file_path: file_path.to_string(),
            text: format!("// {}\n{}", file_path, content),
            line_start: 1,
            line_end: lines.len(),
        }];
    }

    // Try function-aware chunking for supported languages
    let supported = matches!(
        ext,
        "rs" | "go" | "py" | "dart" | "js" | "jsx" | "ts" | "tsx"
    );
    if supported {
        let chunks = chunk_by_functions(file_path, &lines, ext);
        if !chunks.is_empty() {
            return chunks;
        }
    }

    // Fallback: original blank-line chunking
    chunk_by_lines(file_path, &lines)
}

/// Detect if a line is a function/method signature.
fn is_function_start(line: &str, ext: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
        return false;
    }
    match ext {
        "dart" => {
            (trimmed.starts_with("void ")
                || trimmed.starts_with("Future<")
                || trimmed.starts_with("Widget ")
                || trimmed.starts_with("Stream<")
                || trimmed.starts_with("String ")
                || trimmed.starts_with("int ")
                || trimmed.starts_with("bool ")
                || trimmed.starts_with("double ")
                || trimmed.starts_with("List<")
                || trimmed.starts_with("Map<"))
                && trimmed.contains('(')
        }
        "rs" => {
            trimmed.starts_with("pub fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("async fn ")
                || trimmed.starts_with("pub(crate) fn ")
                || trimmed.starts_with("pub(super) fn ")
        }
        "go" => trimmed.starts_with("func "),
        "py" => trimmed.starts_with("def ") || trimmed.starts_with("async def "),
        "js" | "jsx" | "ts" | "tsx" => {
            trimmed.starts_with("function ")
                || trimmed.starts_with("async function ")
                || trimmed.starts_with("export function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("export default function ")
                || (trimmed.contains('(')
                    && trimmed.ends_with('{')
                    && !trimmed.starts_with("if ")
                    && !trimmed.starts_with("if(")
                    && !trimmed.starts_with("else ")
                    && !trimmed.starts_with("for ")
                    && !trimmed.starts_with("for(")
                    && !trimmed.starts_with("while ")
                    && !trimmed.starts_with("while(")
                    && !trimmed.starts_with("switch ")
                    && !trimmed.starts_with("switch("))
        }
        _ => false,
    }
}

/// Split file into chunks at function boundaries.
fn chunk_by_functions(file_path: &str, lines: &[&str], ext: &str) -> Vec<Chunk> {
    let mut fn_starts: Vec<usize> = Vec::new();
    let mut brace_depth: i32 = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }

        let open = trimmed.chars().filter(|&c| c == '{').count() as i32;
        let close = trimmed.chars().filter(|&c| c == '}').count() as i32;

        // Function starts at depth 0 or 1 (class methods in OOP languages)
        if brace_depth <= 1 && is_function_start(lines[i], ext) {
            fn_starts.push(i);
        }

        brace_depth = (brace_depth + open - close).max(0);
    }

    if fn_starts.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut current_start = 0usize;

    for (idx, &fn_start) in fn_starts.iter().enumerate() {
        let fn_end = if idx + 1 < fn_starts.len() {
            fn_starts[idx + 1]
        } else {
            lines.len()
        };

        let fn_len = fn_end - fn_start;

        // Emit preamble before this function (imports, class decl, etc.)
        if fn_start > current_start {
            let preamble = &lines[current_start..fn_start];
            let non_empty = preamble.iter().filter(|l| !l.trim().is_empty()).count();
            if non_empty >= 3 {
                chunks.push(make_chunk(file_path, lines, current_start, fn_start));
            }
        }

        if fn_len <= MAX_FUNCTION_LINES {
            // Whole function in one chunk
            chunks.push(make_chunk(file_path, lines, fn_start, fn_end));
        } else {
            // Large function: split with signature context in continuations
            let signature = lines[fn_start].trim().to_string();
            let mut sub_start = fn_start;
            while sub_start < fn_end {
                let sub_end = (sub_start + MAX_CHUNK_FOR_FUNCTIONS).min(fn_end);
                let mut text = format!("// {}\n", file_path);
                if sub_start > fn_start {
                    text.push_str(&format!("// (continued) {}\n", signature));
                }
                text.push_str(&lines[sub_start..sub_end].join("\n"));

                // Don't create tiny trailing sub-chunks
                let remaining = fn_end - sub_end;
                let actual_end = if remaining > 0 && remaining < MIN_CHUNK_LINES {
                    fn_end
                } else {
                    sub_end
                };
                if actual_end != sub_end {
                    text.push('\n');
                    text.push_str(&lines[sub_end..actual_end].join("\n"));
                }

                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    text,
                    line_start: sub_start + 1,
                    line_end: if actual_end != sub_end {
                        actual_end
                    } else {
                        sub_end
                    },
                });
                sub_start = if actual_end != sub_end {
                    actual_end
                } else {
                    sub_end
                };
            }
        }

        current_start = fn_end;
    }

    // Trailing content after last function
    if current_start < lines.len() {
        let trailing = &lines[current_start..];
        let non_empty = trailing.iter().filter(|l| !l.trim().is_empty()).count();
        if non_empty >= 2 {
            chunks.push(make_chunk(file_path, lines, current_start, lines.len()));
        }
    }

    chunks
}

fn make_chunk(file_path: &str, lines: &[&str], start: usize, end: usize) -> Chunk {
    Chunk {
        file_path: file_path.to_string(),
        text: format!("// {}\n{}", file_path, lines[start..end].join("\n")),
        line_start: start + 1,
        line_end: end,
    }
}

/// Original line-based chunking (fallback for unsupported extensions).
fn chunk_by_lines(file_path: &str, lines: &[&str]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < lines.len() {
        let mut end = (start + CHUNK_LINES).min(lines.len());

        // Try to break at a blank line near the target
        if end < lines.len() {
            let search_start = (start + CHUNK_LINES - 10).max(start);
            let search_end = (start + CHUNK_LINES + 10).min(lines.len());
            for i in (search_start..search_end).rev() {
                if lines[i].trim().is_empty() {
                    end = i + 1;
                    break;
                }
            }
        }

        // Don't create tiny trailing chunks
        if lines.len() - end < MIN_CHUNK_LINES {
            end = lines.len();
        }

        let chunk_text = lines[start..end].join("\n");
        chunks.push(Chunk {
            file_path: file_path.to_string(),
            text: format!("// {}\n{}", file_path, chunk_text),
            line_start: start + 1,
            line_end: end,
        });

        start = end;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── enrich_chunk_with_context ───────────────────────────

    fn sample_chunk() -> Chunk {
        Chunk {
            file_path: "src/foo.rs".to_string(),
            text: "fn foo() {}".to_string(),
            line_start: 1,
            line_end: 1,
        }
    }

    #[test]
    fn test_enrich_noop_when_no_context() {
        let mut chunk = sample_chunk();
        let original = chunk.text.clone();
        enrich_chunk_with_context(&mut chunk, &[], &[]);
        assert_eq!(
            chunk.text, original,
            "empty imports/imported_by must leave the text untouched"
        );
    }

    #[test]
    fn test_enrich_prepends_used_by_and_imports() {
        let mut chunk = sample_chunk();
        let imports = vec!["src/util/helpers.rs".to_string()];
        let imported_by = vec!["src/main.rs".to_string(), "lib/app.ts".to_string()];
        enrich_chunk_with_context(&mut chunk, &imports, &imported_by);

        let mut lines = chunk.text.lines();
        assert_eq!(
            lines.next(),
            Some("// Used by: main, app"),
            "imported_by names must be shortened (basename, no extension)"
        );
        assert_eq!(
            lines.next(),
            Some("// Imports: helpers"),
            "imports must follow the Used by line"
        );
        assert_eq!(
            lines.next(),
            Some("fn foo() {}"),
            "original text must be preserved after the context prefix"
        );
    }

    #[test]
    fn test_enrich_caps_names_at_three() {
        let mut chunk = sample_chunk();
        let imported_by: Vec<String> = (0..5).map(|i| format!("src/mod{}.rs", i)).collect();
        enrich_chunk_with_context(&mut chunk, &[], &imported_by);

        let first_line = chunk.text.lines().next().unwrap();
        assert_eq!(
            first_line, "// Used by: mod0, mod1, mod2",
            "only the first 3 names must be listed"
        );
    }

    // ── chunk_file: small/empty inputs ──────────────────────

    #[test]
    fn test_empty_content_yields_no_chunks() {
        let chunks = chunk_file("src/empty.rs", "");
        assert!(chunks.is_empty(), "empty file must produce zero chunks");
    }

    #[test]
    fn test_tiny_file_yields_single_chunk() {
        let content = "fn a() {}\nfn b() {}\nfn c() {}";
        let chunks = chunk_file("src/tiny.rs", content);
        assert_eq!(chunks.len(), 1, "files under MIN_CHUNK_LINES stay whole");
        let c = &chunks[0];
        assert_eq!(c.line_start, 1);
        assert_eq!(c.line_end, 3);
        assert!(
            c.text.starts_with("// src/tiny.rs\n"),
            "chunk text must be prefixed with the file path header"
        );
        assert!(c.text.contains(content), "original content must be kept");
    }

    // ── is_function_start ───────────────────────────────────

    #[test]
    fn test_is_function_start_by_language() {
        // Rust
        assert!(is_function_start("pub fn run() {", "rs"));
        assert!(is_function_start("    async fn go() {", "rs"));
        assert!(is_function_start("pub(crate) fn inner() {", "rs"));
        assert!(!is_function_start("// fn commented() {", "rs"));
        assert!(!is_function_start("let x = 1;", "rs"));
        // Go
        assert!(is_function_start("func main() {", "go"));
        assert!(!is_function_start("var x = 1", "go"));
        // Python
        assert!(is_function_start("def handler(req):", "py"));
        assert!(is_function_start("async def poll():", "py"));
        assert!(!is_function_start("# def not_code", "py"));
        // JS/TS
        assert!(is_function_start("export function render() {", "ts"));
        assert!(is_function_start("const f = (x) => {", "ts"));
        assert!(!is_function_start("if (cond) {", "ts"));
        assert!(!is_function_start("while (true) {", "js"));
        // Dart
        assert!(is_function_start("void main() {", "dart"));
        assert!(is_function_start("Future<void> load() async {", "dart"));
        assert!(!is_function_start("class Foo {", "dart"));
        // Unsupported extension
        assert!(!is_function_start("fn looks_like_rust() {", "txt"));
    }

    // ── function-aware chunking ─────────────────────────────

    #[test]
    fn test_rust_functions_chunked_at_boundaries() {
        let content = "\
fn alpha() {
    let a = 1;
    let b = 2;
    let c = 3;
    let _ = a + b + c;
}

fn beta() {
    let z = 9;
    let _ = z;
}";
        let chunks = chunk_file("src/two.rs", content);
        assert_eq!(chunks.len(), 2, "one chunk per function expected");
        assert_eq!(chunks[0].line_start, 1, "alpha starts at line 1");
        assert_eq!(chunks[0].line_end, 7, "alpha chunk ends before beta");
        assert_eq!(chunks[1].line_start, 8, "beta starts at line 8");
        assert_eq!(chunks[1].line_end, 11, "beta runs to EOF");
        assert!(chunks[0].text.contains("fn alpha()"));
        assert!(chunks[1].text.contains("fn beta()"));
        assert!(
            !chunks[0].text.contains("fn beta()"),
            "functions must not bleed into each other's chunks"
        );
    }

    #[test]
    fn test_preamble_before_first_function_is_emitted() {
        let content = "\
use std::fs;
use std::io;
use std::path::Path;

fn work() {
    let _ = (fs::read, io::stdin);
    let _ = Path::new(\".\");
}";
        let chunks = chunk_file("src/pre.rs", content);
        assert_eq!(chunks.len(), 2, "preamble chunk + function chunk");
        assert!(
            chunks[0].text.contains("use std::fs;"),
            "first chunk must be the import preamble"
        );
        assert_eq!(chunks[0].line_start, 1);
        assert!(chunks[1].text.contains("fn work()"));
    }

    #[test]
    fn test_large_function_split_with_signature_context() {
        // One function of ~201 lines (> MAX_FUNCTION_LINES = 150).
        let mut src = String::from("fn big_one() {\n");
        for i in 0..199 {
            src.push_str(&format!("    let _x{} = {};\n", i, i));
        }
        src.push('}');

        let chunks = chunk_file("src/big.rs", &src);
        assert!(
            chunks.len() >= 2,
            "a 200+ line function must be split, got {} chunk(s)",
            chunks.len()
        );
        assert!(
            chunks[0].text.contains("fn big_one()"),
            "first sub-chunk carries the signature"
        );
        assert!(
            chunks[1].text.contains("// (continued) fn big_one() {"),
            "continuation sub-chunks must repeat the signature as context"
        );
        // Sub-chunks must be contiguous over the whole function.
        assert_eq!(chunks[0].line_start, 1);
        for pair in chunks.windows(2) {
            assert_eq!(
                pair[1].line_start,
                pair[0].line_end + 1,
                "sub-chunks must be contiguous"
            );
        }
        assert_eq!(
            chunks.last().unwrap().line_end,
            src.lines().count(),
            "last sub-chunk must reach EOF"
        );
    }

    // ── line-based fallback ─────────────────────────────────

    #[test]
    fn test_unsupported_extension_uses_line_chunking() {
        // 100 lines of prose, no functions, no blank lines.
        let content = (1..=100)
            .map(|i| format!("line number {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_file("notes.txt", &content);

        assert!(
            chunks.len() > 1,
            "100 lines must span multiple chunks (CHUNK_LINES = {})",
            CHUNK_LINES
        );
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks.last().unwrap().line_end, 100);
        for pair in chunks.windows(2) {
            assert_eq!(
                pair[1].line_start,
                pair[0].line_end + 1,
                "line chunks must cover the file contiguously"
            );
        }
        for c in &chunks {
            assert!(
                c.text.starts_with("// notes.txt\n"),
                "every chunk gets the file header"
            );
        }
    }

    #[test]
    fn test_line_chunking_avoids_tiny_trailing_chunk() {
        // 42 lines: naive split would leave a 2-line tail (< MIN_CHUNK_LINES),
        // so the last chunk must absorb it.
        let content = (1..=42)
            .map(|i| format!("row {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_file("data.csv", &content);
        assert_eq!(
            chunks.last().unwrap().line_end,
            42,
            "trailing lines must be merged into the final chunk"
        );
        let last = chunks.last().unwrap();
        assert!(
            last.line_end - last.line_start + 1 >= 5,
            "no tiny trailing chunk allowed, got {}..{}",
            last.line_start,
            last.line_end
        );
    }
}
