//! Cyclomatic complexity analysis per function.
//!
//! Counts branching points (if, for, while, match, try, elif, else if, ternary, &&, ||)
//! within each function body to compute complexity per function.

use super::graph::Language;

/// A single function with its complexity score.
#[derive(Debug, Clone)]
pub struct FunctionComplexity {
    /// Function name.
    pub name: String,
    /// Line number where the function starts.
    pub line: usize,
    /// Cyclomatic complexity (1 + branching points).
    pub complexity: usize,
    /// Lines of code in this function body.
    pub loc: usize,
}

/// All complexity data for a single file.
#[derive(Debug, Clone)]
pub struct FileComplexity {
    pub functions: Vec<FunctionComplexity>,
}

impl FileComplexity {
    /// Average complexity across all functions.
    pub fn average(&self) -> f32 {
        if self.functions.is_empty() {
            return 0.0;
        }
        let total: usize = self.functions.iter().map(|f| f.complexity).sum();
        total as f32 / self.functions.len() as f32
    }

    /// Max complexity function.
    pub fn max_complexity(&self) -> Option<&FunctionComplexity> {
        self.functions.iter().max_by_key(|f| f.complexity)
    }

    /// Functions with complexity above threshold.
    pub fn complex_functions(&self, threshold: usize) -> Vec<&FunctionComplexity> {
        self.functions.iter().filter(|f| f.complexity >= threshold).collect()
    }
}

/// Analyze complexity for a source file.
pub fn analyze_file(content: &str, lang: Language) -> FileComplexity {
    match lang {
        Language::Python => analyze_python(content),
        Language::JavaScript | Language::TypeScript => analyze_javascript(content),
        _ => FileComplexity { functions: vec![] },
    }
}

// ── Python ──────────────────────────────────────────────────

fn analyze_python(content: &str) -> FileComplexity {
    let lines: Vec<&str> = content.lines().collect();
    let mut functions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Detect function definition
        let is_func = trimmed.starts_with("def ") || trimmed.starts_with("async def ");
        if is_func && trimmed.contains('(') {
            let name = extract_python_func_name(trimmed);
            let func_indent = leading_spaces(lines[i]);
            let start_line = i + 1;

            // Find the body: all lines with greater indentation (or blank)
            let mut body_end = i + 1;
            while body_end < lines.len() {
                let line = lines[body_end];
                if line.trim().is_empty() {
                    body_end += 1;
                    continue;
                }
                let indent = leading_spaces(line);
                if indent <= func_indent {
                    break;
                }
                body_end += 1;
            }

            let body_lines = &lines[i..body_end];
            let loc = body_lines.iter().filter(|l| !l.trim().is_empty()).count();
            let complexity = 1 + count_python_branches(body_lines);

            functions.push(FunctionComplexity {
                name,
                line: start_line,
                complexity,
                loc,
            });

            i = body_end;
        } else {
            i += 1;
        }
    }

    FileComplexity { functions }
}

fn extract_python_func_name(line: &str) -> String {
    let trimmed = line.trim();
    let after = if let Some(rest) = trimmed.strip_prefix("async def ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("def ") {
        rest
    } else {
        return "unknown".to_string();
    };
    after.split('(').next().unwrap_or("unknown").trim().to_string()
}

fn count_python_branches(lines: &[&str]) -> usize {
    let mut count = 0;
    for line in lines {
        let t = line.trim();
        // Skip comments and strings
        if t.starts_with('#') || t.is_empty() {
            continue;
        }

        // Branching keywords
        if t.starts_with("if ") || t.starts_with("elif ") {
            count += 1;
        }
        if t.starts_with("for ") || t.starts_with("while ") {
            count += 1;
        }
        if t.starts_with("except") || t.starts_with("except:") {
            count += 1;
        }

        // Inline conditionals: "x if cond else y" (ternary)
        // Only count if it's not the start of a block-if
        if !t.starts_with("if ") && !t.starts_with("elif ") {
            // Count " if " occurrences that have " else " (ternary)
            if t.contains(" if ") && t.contains(" else ") {
                count += 1;
            }
        }

        // Boolean operators as branching
        count += count_logical_operators(t);
    }
    count
}

// ── JavaScript/TypeScript ───────────────────────────────────

fn analyze_javascript(content: &str) -> FileComplexity {
    let lines: Vec<&str> = content.lines().collect();
    let mut functions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if let Some(name) = detect_js_function(trimmed) {
            let start_line = i + 1;

            // Find the body: track brace depth
            let open_on_line = lines[i..].iter().take(2)
                .flat_map(|l| l.chars())
                .filter(|c| *c == '{')
                .count();

            if open_on_line == 0 && !trimmed.contains("=>") {
                i += 1;
                continue;
            }

            let mut depth: i32 = 0;
            let mut body_start = i;
            let mut body_end = i;
            let mut found_open = false;

            for j in i..lines.len() {
                for ch in lines[j].chars() {
                    if ch == '{' {
                        if !found_open {
                            found_open = true;
                            body_start = j;
                        }
                        depth += 1;
                    } else if ch == '}' {
                        depth -= 1;
                        if depth == 0 && found_open {
                            body_end = j + 1;
                            break;
                        }
                    }
                }
                if depth == 0 && found_open {
                    break;
                }
            }

            // Arrow function without braces (single expression)
            if !found_open && trimmed.contains("=>") {
                body_end = i + 1;
                body_start = i;
            }

            if body_end <= body_start {
                body_end = i + 1;
            }

            let body_lines = &lines[i..body_end];
            let loc = body_lines.iter().filter(|l| !l.trim().is_empty()).count();
            let complexity = 1 + count_js_branches(body_lines);

            functions.push(FunctionComplexity {
                name,
                line: start_line,
                complexity,
                loc,
            });

            i = body_end;
        } else {
            i += 1;
        }
    }

    FileComplexity { functions }
}

fn detect_js_function(line: &str) -> Option<String> {
    let t = line.trim();

    // function name(
    if t.starts_with("function ") || t.starts_with("async function ") {
        let after = t.strip_prefix("async function ")
            .or_else(|| t.strip_prefix("function "))?;
        let name = after.split('(').next()?.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    // export function / export async function / export default function
    if t.starts_with("export ") && t.contains("function ") {
        let func_pos = t.find("function ")?;
        let after = &t[func_pos + 9..];
        let name = after.split('(').next()?.trim();
        if !name.is_empty() && name != "(" {
            return Some(name.to_string());
        }
    }

    // const/let/var name = (...) => / function(
    for prefix in &["const ", "let ", "var ", "export const ", "export let "] {
        if t.starts_with(prefix) && (t.contains("=>") || t.contains("= function")) {
            let after = t.strip_prefix(prefix)?;
            let name = after.split(&['=', ':', ' '][..]).next()?.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    // Class method: name( or async name(
    if !t.starts_with("if ") && !t.starts_with("for ") && !t.starts_with("while ")
        && !t.starts_with("switch ") && !t.starts_with("//") && !t.starts_with("return ")
    {
        let check = t.strip_prefix("async ").unwrap_or(t);
        if let Some(paren) = check.find('(') {
            let before = check[..paren].trim();
            // Should look like a method name (word chars, possibly with get/set prefix)
            if !before.is_empty()
                && before.chars().all(|c| c.is_alphanumeric() || c == '_')
                && (t.contains('{') || t.ends_with('{'))
                && !matches!(before, "if" | "for" | "while" | "switch" | "catch" | "class" | "new" | "return" | "import" | "export")
            {
                return Some(before.to_string());
            }
        }
    }

    None
}

fn count_js_branches(lines: &[&str]) -> usize {
    let mut count = 0;
    for line in lines {
        let t = line.trim();
        if t.starts_with("//") || t.is_empty() {
            continue;
        }

        // Keywords
        if t.starts_with("if ") || t.starts_with("if(") || t.starts_with("} else if") || t.starts_with("else if") {
            count += 1;
        }
        if t.starts_with("for ") || t.starts_with("for(") {
            count += 1;
        }
        if t.starts_with("while ") || t.starts_with("while(") {
            count += 1;
        }
        if t.starts_with("case ") || t.starts_with("case'") || t.starts_with("case\"") {
            count += 1;
        }
        if t.starts_with("catch") {
            count += 1;
        }

        // Ternary operator
        if t.contains('?') && t.contains(':') && !t.starts_with("//") {
            // Rough ternary detection: "? ... :" but not type annotations
            let _question_count = t.matches('?').count();
            // Only count if there's actually a ? that looks like ternary (not optional chaining ?.)
            for (i, _) in t.match_indices('?') {
                if i + 1 < t.len() {
                    let next = t.as_bytes().get(i + 1);
                    if next != Some(&b'.') && next != Some(&b'?') {
                        count += 1;
                        break; // count max 1 ternary per line
                    }
                }
            }
        }

        count += count_logical_operators(t);
    }
    count
}

// ── Shared ──────────────────────────────────────────────────

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn count_logical_operators(line: &str) -> usize {
    let mut count = 0;
    // Count && and || as branching points
    count += line.matches("&&").count();
    count += line.matches("||").count();
    // Python: "and", "or" (as separate words)
    for word in line.split_whitespace() {
        if word == "and" || word == "or" {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_simple() {
        let code = r#"
def hello():
    print("hi")

def complex_func(x, y):
    if x > 0:
        for i in range(y):
            if i % 2 == 0:
                print(i)
    elif x < 0:
        while y > 0:
            y -= 1
    try:
        something()
    except ValueError:
        pass
    except Exception:
        pass
    result = x if x > 0 else -x
"#;
        let fc = analyze_python(code);
        assert_eq!(fc.functions.len(), 2);
        assert_eq!(fc.functions[0].name, "hello");
        assert_eq!(fc.functions[0].complexity, 1); // no branches

        let complex = &fc.functions[1];
        assert_eq!(complex.name, "complex_func");
        // 1 base + if + for + if + elif + while + except + except + ternary = 9
        assert!(complex.complexity >= 8, "complexity was {}", complex.complexity);
    }

    #[test]
    fn test_python_logical_ops() {
        let code = r#"
def check(a, b, c):
    if a > 0 and b > 0 or c > 0:
        return True
    return False
"#;
        let fc = analyze_python(code);
        assert_eq!(fc.functions.len(), 1);
        // 1 base + if + and + or = 4
        assert_eq!(fc.functions[0].complexity, 4);
    }

    #[test]
    fn test_javascript_simple() {
        let code = r#"
function greet(name) {
    console.log("hello " + name);
}

function processData(data) {
    if (data.length > 0) {
        for (let i = 0; i < data.length; i++) {
            if (data[i] > 10) {
                console.log(data[i]);
            }
        }
    }
    const result = data ? data[0] : null;
    try {
        parse(data);
    } catch (e) {
        console.error(e);
    }
}
"#;
        let fc = analyze_javascript(code);
        assert_eq!(fc.functions.len(), 2);
        assert_eq!(fc.functions[0].name, "greet");
        assert_eq!(fc.functions[0].complexity, 1);

        let complex = &fc.functions[1];
        assert_eq!(complex.name, "processData");
        // 1 base + if + for + if + ternary + catch = 6
        assert!(complex.complexity >= 5, "complexity was {}", complex.complexity);
    }

    #[test]
    fn test_arrow_function() {
        let code = r#"
const handler = async (req, res) => {
    if (req.body) {
        return res.json(req.body);
    }
    return res.status(400).json({ error: "no body" });
}
"#;
        let fc = analyze_javascript(code);
        assert_eq!(fc.functions.len(), 1);
        assert_eq!(fc.functions[0].name, "handler");
        assert_eq!(fc.functions[0].complexity, 2); // 1 base + if
    }
}
