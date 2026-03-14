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
    /// Whether this function has test coverage (cross-referenced with lcov/cobertura).
    /// `Some(true)` = covered, `Some(false)` = in report but 0% covered, `None` = no report.
    pub has_coverage: Option<bool>,
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
        self.functions
            .iter()
            .filter(|f| f.complexity >= threshold)
            .collect()
    }
}

/// Analyze complexity for a source file.
pub fn analyze_file(content: &str, lang: Language) -> FileComplexity {
    match lang {
        Language::Python => analyze_python(content),
        Language::JavaScript | Language::TypeScript => analyze_javascript(content),
        Language::Go | Language::Rust => analyze_brace_lang(content, lang),
        Language::Dart => analyze_brace_lang(content, lang),
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
                has_coverage: None,
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
    after
        .split('(')
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string()
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
            let open_on_line = lines[i..]
                .iter()
                .take(2)
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

            for (j, line) in lines.iter().enumerate().skip(i) {
                for ch in line.chars() {
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
                has_coverage: None,
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
        let after = t
            .strip_prefix("async function ")
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
    if !t.starts_with("if ")
        && !t.starts_with("for ")
        && !t.starts_with("while ")
        && !t.starts_with("switch ")
        && !t.starts_with("//")
        && !t.starts_with("return ")
    {
        let check = t.strip_prefix("async ").unwrap_or(t);
        if let Some(paren) = check.find('(') {
            let before = check[..paren].trim();
            // Should look like a method name (word chars, possibly with get/set prefix)
            if !before.is_empty()
                && before.chars().all(|c| c.is_alphanumeric() || c == '_')
                && (t.contains('{') || t.ends_with('{'))
                && !matches!(
                    before,
                    "if" | "for"
                        | "while"
                        | "switch"
                        | "catch"
                        | "class"
                        | "new"
                        | "return"
                        | "import"
                        | "export"
                )
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
        if t.starts_with("if ")
            || t.starts_with("if(")
            || t.starts_with("} else if")
            || t.starts_with("else if")
        {
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

// ── Go / Rust / Dart (brace-based languages) ────────────────

fn analyze_brace_lang(content: &str, lang: Language) -> FileComplexity {
    let lines: Vec<&str> = content.lines().collect();
    let mut functions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if let Some(name) = detect_brace_function(trimmed, lang) {
            let start_line = i + 1;

            // Find function body via brace matching
            let mut depth: i32 = 0;
            let mut body_start = i;
            let mut body_end = i;
            let mut found_open = false;

            for (j, line) in lines.iter().enumerate().skip(i) {
                for ch in line.chars() {
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

            if body_end <= body_start {
                body_end = i + 1;
            }

            let body_lines = &lines[i..body_end];
            let loc = body_lines.iter().filter(|l| !l.trim().is_empty()).count();
            let complexity = 1 + count_brace_branches(body_lines, lang);

            functions.push(FunctionComplexity {
                name,
                line: start_line,
                complexity,
                loc,
                has_coverage: None,
            });

            i = body_end;
        } else {
            i += 1;
        }
    }

    FileComplexity { functions }
}

fn detect_brace_function(line: &str, lang: Language) -> Option<String> {
    let t = line.trim();
    if t.starts_with("//") || t.starts_with("/*") || t.is_empty() {
        return None;
    }

    match lang {
        Language::Go => {
            // func Name( or func (receiver) Name(
            if t.starts_with("func ") && t.contains('(') {
                let after_func = t.strip_prefix("func ")?;
                // Skip receiver: (r *Receiver)
                let rest = if after_func.starts_with('(') {
                    let close = after_func.find(')')?;
                    after_func[close + 1..].trim()
                } else {
                    after_func
                };
                let name = rest.split('(').next()?.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            None
        }
        Language::Rust => {
            // fn name( / pub fn name( / async fn name( / pub async fn name(
            let stripped = t
                .strip_prefix("pub(crate) ")
                .or_else(|| t.strip_prefix("pub(super) "))
                .or_else(|| t.strip_prefix("pub "))
                .unwrap_or(t);
            let stripped = stripped.strip_prefix("async ").unwrap_or(stripped);
            if stripped.starts_with("fn ") && stripped.contains('(') {
                let after_fn = stripped.strip_prefix("fn ")?;
                let name = after_fn.split(&['(', '<'][..]).next()?.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            None
        }
        Language::Dart => {
            // Similar to JS but also includes method patterns
            // void name(, Future<T> name(, static name(
            if t.starts_with("class ")
                || t.starts_with("abstract ")
                || t.starts_with("import ")
                || t.starts_with("export ")
                || t.starts_with("if ")
                || t.starts_with("for ")
                || t.starts_with("while ")
                || t.starts_with("return ")
            {
                return None;
            }
            let check = t.strip_prefix("static ").unwrap_or(t);
            let check = check.strip_prefix("async ").unwrap_or(check);
            if let Some(paren) = check.find('(') {
                let before = &check[..paren];
                let parts: Vec<&str> = before.split_whitespace().collect();
                if !parts.is_empty() && parts.len() <= 3 {
                    let name = parts.last()?;
                    if !name.is_empty()
                        && (t.contains('{') || t.contains("=>") || t.contains(") async"))
                        && !matches!(*name, "if" | "for" | "while" | "switch" | "catch")
                    {
                        return Some(name.to_string());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn count_brace_branches(lines: &[&str], lang: Language) -> usize {
    let mut count = 0;
    for line in lines {
        let t = line.trim();
        if t.starts_with("//") || t.is_empty() {
            continue;
        }

        // Common branching keywords
        if t.starts_with("if ")
            || t.starts_with("if(")
            || t.starts_with("} else if")
            || t.starts_with("else if")
        {
            count += 1;
        }
        if t.starts_with("for ") || t.starts_with("for(") {
            count += 1;
        }
        if t.starts_with("while ") || t.starts_with("while(") {
            count += 1;
        }
        if t.starts_with("case ") {
            count += 1;
        }

        // Language-specific
        match lang {
            Language::Go => {
                if t.starts_with("select {") || t == "select {" {
                    count += 1;
                }
            }
            Language::Rust => {
                // match arms
                if t.contains("=>") && !t.starts_with("//") && !t.starts_with("fn ") {
                    // Rough: count lines with => as match arms
                    count += 1;
                }
                if t.starts_with("if let ") {
                    count += 1;
                }
                if t.starts_with("while let ") {
                    count += 1;
                }
            }
            Language::Dart => {
                if t.starts_with("catch") || t.starts_with("on ") {
                    count += 1;
                }
            }
            _ => {}
        }

        // Ternary (Go doesn't have it, Dart and Rust don't really either)
        if lang == Language::Dart && t.contains('?') && t.contains(':') && !t.starts_with("//") {
            for (i, _) in t.match_indices('?') {
                if i + 1 < t.len() {
                    let next = t.as_bytes().get(i + 1);
                    if next != Some(&b'.') && next != Some(&b'?') {
                        count += 1;
                        break;
                    }
                }
            }
        }

        count += count_logical_operators_no_python(t);
    }
    count
}

/// Count && and || but not Python's "and"/"or".
fn count_logical_operators_no_python(line: &str) -> usize {
    line.matches("&&").count() + line.matches("||").count()
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
        assert!(
            complex.complexity >= 8,
            "complexity was {}",
            complex.complexity
        );
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
        assert!(
            complex.complexity >= 5,
            "complexity was {}",
            complex.complexity
        );
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

    // ── Rust tests ───────────────────────────────────────

    #[test]
    fn test_rust_simple_function() {
        let code = r#"
fn hello() {
    println!("hello");
}

fn complex(x: i32) -> i32 {
    if x > 0 {
        for i in 0..x {
            if i % 2 == 0 {
                println!("{}", i);
            }
        }
    }
    match x {
        0 => 0,
        1 => 1,
        _ => x * 2,
    }
}
"#;
        let fc = analyze_file(code, Language::Rust);
        assert_eq!(fc.functions.len(), 2);
        assert_eq!(fc.functions[0].name, "hello");
        assert_eq!(fc.functions[0].complexity, 1);
        assert!(
            fc.functions[1].complexity >= 5,
            "Rust complex fn was {}",
            fc.functions[1].complexity
        );
    }

    #[test]
    fn test_rust_if_let_while_let() {
        let code = r#"
fn process(opt: Option<i32>) {
    if let Some(v) = opt {
        while let Some(x) = get_next() {
            println!("{} {}", v, x);
        }
    }
}
"#;
        let fc = analyze_file(code, Language::Rust);
        assert_eq!(fc.functions.len(), 1);
        // 1 base + if let + while let = 3
        assert!(
            fc.functions[0].complexity >= 3,
            "if let/while let was {}",
            fc.functions[0].complexity
        );
    }

    #[test]
    fn test_go_simple() {
        let code = r#"
func handler(w http.ResponseWriter, r *http.Request) {
    if r.Method == "GET" {
        for _, item := range items {
            if item.Active {
                fmt.Fprintf(w, "%s", item.Name)
            }
        }
    }
}
"#;
        let fc = analyze_file(code, Language::Go);
        assert_eq!(fc.functions.len(), 1);
        assert!(
            fc.functions[0].complexity >= 4,
            "Go fn was {}",
            fc.functions[0].complexity
        );
    }

    #[test]
    fn test_dart_function() {
        let code = r#"
void main() {
    if (args.isEmpty) {
        return;
    }
    for (var arg in args) {
        if (arg.startsWith('-')) {
            print('flag: $arg');
        }
    }
}
"#;
        let fc = analyze_file(code, Language::Dart);
        assert_eq!(fc.functions.len(), 1);
        assert!(
            fc.functions[0].complexity >= 4,
            "Dart fn was {}",
            fc.functions[0].complexity
        );
    }

    // ── FileComplexity methods ────────────────────────────

    #[test]
    fn test_file_complexity_average() {
        let fc = FileComplexity {
            functions: vec![
                FunctionComplexity {
                    name: "a".into(),
                    line: 1,
                    complexity: 2,
                    loc: 5,
                    has_coverage: None,
                },
                FunctionComplexity {
                    name: "b".into(),
                    line: 10,
                    complexity: 8,
                    loc: 20,
                    has_coverage: None,
                },
            ],
        };
        assert!((fc.average() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_file_complexity_average_empty() {
        let fc = FileComplexity { functions: vec![] };
        assert_eq!(fc.average(), 0.0);
    }

    #[test]
    fn test_file_complexity_max() {
        let fc = FileComplexity {
            functions: vec![
                FunctionComplexity {
                    name: "low".into(),
                    line: 1,
                    complexity: 2,
                    loc: 5,
                    has_coverage: None,
                },
                FunctionComplexity {
                    name: "high".into(),
                    line: 10,
                    complexity: 15,
                    loc: 30,
                    has_coverage: None,
                },
                FunctionComplexity {
                    name: "mid".into(),
                    line: 50,
                    complexity: 7,
                    loc: 15,
                    has_coverage: None,
                },
            ],
        };
        let max = fc.max_complexity().unwrap();
        assert_eq!(max.name, "high");
        assert_eq!(max.complexity, 15);
    }

    #[test]
    fn test_file_complexity_complex_functions() {
        let fc = FileComplexity {
            functions: vec![
                FunctionComplexity {
                    name: "simple".into(),
                    line: 1,
                    complexity: 2,
                    loc: 5,
                    has_coverage: None,
                },
                FunctionComplexity {
                    name: "moderate".into(),
                    line: 10,
                    complexity: 8,
                    loc: 20,
                    has_coverage: None,
                },
                FunctionComplexity {
                    name: "complex".into(),
                    line: 50,
                    complexity: 15,
                    loc: 30,
                    has_coverage: None,
                },
            ],
        };
        let above_10 = fc.complex_functions(10);
        assert_eq!(above_10.len(), 1);
        assert_eq!(above_10[0].name, "complex");

        let above_5 = fc.complex_functions(5);
        assert_eq!(above_5.len(), 2);
    }

    #[test]
    fn test_python_nested_functions() {
        let code = r#"
def outer():
    def inner():
        if True:
            pass
    inner()
"#;
        let fc = analyze_file(code, Language::Python);
        assert!(!fc.functions.is_empty());
    }

    #[test]
    fn test_python_async_def() {
        let code = r#"
async def fetch_data(url):
    if url:
        for retry in range(3):
            try:
                result = await get(url)
            except Exception:
                pass
    return None
"#;
        let fc = analyze_file(code, Language::Python);
        assert_eq!(fc.functions.len(), 1);
        assert_eq!(fc.functions[0].name, "fetch_data");
        assert!(
            fc.functions[0].complexity >= 4,
            "async def was {}",
            fc.functions[0].complexity
        );
    }

    #[test]
    fn test_js_class_methods() {
        let code = r#"
class UserService {
    constructor(db) {
        this.db = db;
    }

    async getUser(id) {
        if (!id) {
            throw new Error("missing id");
        }
        return this.db.find(id);
    }

    deleteUser(id) {
        if (!id) return false;
        return this.db.delete(id);
    }
}
"#;
        let fc = analyze_file(code, Language::JavaScript);
        assert!(
            fc.functions.len() >= 2,
            "should find class methods, found {}",
            fc.functions.len()
        );
    }

    #[test]
    fn test_logical_operators() {
        let code = r#"
function validate(a, b, c) {
    if (a > 0 && b > 0 || c < 0) {
        return true;
    }
    return false;
}
"#;
        let fc = analyze_file(code, Language::JavaScript);
        assert_eq!(fc.functions.len(), 1);
        // 1 base + if + && + || = 4
        assert!(
            fc.functions[0].complexity >= 4,
            "logical ops was {}",
            fc.functions[0].complexity
        );
    }
}
