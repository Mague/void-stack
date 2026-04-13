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
