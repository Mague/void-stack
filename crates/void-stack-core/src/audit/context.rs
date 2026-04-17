//! Multi-language syntactic context detection for audit enrichment.
//!
//! All detectors work on any supported language — a Rust-only detector
//! is useless since <30% of typical findings come from Rust.

use super::findings::ModuleRole;

/// Detect the programming language from a file path extension.
pub fn detect_language(file_path: &str) -> &'static str {
    let normalized = file_path.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or("").to_lowercase();
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        return "dockerfile";
    }
    if name == "makefile" {
        return "makefile";
    }

    let ext = normalized.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "go" => "go",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "dart" => "dart",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" | "rake" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "cs" => "csharp",
        "vue" => "vue",
        "svelte" => "svelte",
        "lua" => "lua",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        _ => "unknown",
    }
}

/// Classify the module role from its file path. First match wins.
pub fn detect_module_role(file_path: &str) -> ModuleRole {
    let n = file_path.replace('\\', "/");

    // Tests
    let test_markers = [
        "/tests/",
        "/test/",
        "/__tests__/",
        "/spec/",
        "/test_driver/",
        "_test.rs",
        "_test.go",
        "_test.py",
        "_spec.rb",
        "_spec.exs",
        ".test.js",
        ".test.ts",
        ".test.tsx",
        ".test.jsx",
        ".spec.js",
        ".spec.ts",
        ".spec.tsx",
        ".spec.jsx",
        "_test.dart",
    ];
    if test_markers.iter().any(|m| n.contains(m)) {
        return ModuleRole::Test;
    }
    // test_*.py
    if let Some(fname) = n.rsplit('/').next() {
        if fname.starts_with("test_") && fname.ends_with(".py") {
            return ModuleRole::Test;
        }
        if fname.ends_with("Test.java")
            || fname.ends_with("Test.kt")
            || fname.ends_with("Tests.java")
        {
            return ModuleRole::Test;
        }
    }

    // Generated
    let gen_markers = [
        ".pb.rs",
        ".pb.go",
        ".pb.dart",
        "_pb2.py",
        "_pb2_grpc.py",
        ".generated.",
        ".gen.",
        ".g.dart",
        ".freezed.dart",
        ".d.ts",
        "/generated/",
        "/node_modules/",
        "/__pycache__/",
    ];
    if gen_markers.iter().any(|m| n.contains(m)) {
        return ModuleRole::Generated;
    }

    // Migrations
    if n.contains("/migrations/") || n.contains("/migrate/") || n.contains("/db/migrate/") {
        return ModuleRole::Migration;
    }

    // I18n
    let i18n_markers = [
        "/i18n/",
        "/i18n.",
        "/locales/",
        "/translations/",
        "/l10n/",
        ".arb",
        ".po",
        ".ftl",
    ];
    if i18n_markers.iter().any(|m| n.contains(m)) {
        return ModuleRole::I18n;
    }

    // Examples
    let ex_markers = [
        "/examples/",
        "/example/",
        "/fixtures/",
        "/fixture/",
        "/samples/",
        "/demos/",
    ];
    if ex_markers.iter().any(|m| n.contains(m)) {
        return ModuleRole::Example;
    }

    // Audit
    if n.contains("/audit/") || n.contains("/security/") {
        return ModuleRole::Audit;
    }

    // CLI
    if n.contains("/cli/") || n.contains("/commands/") || n.contains("/bin/") {
        return ModuleRole::CLI;
    }

    ModuleRole::Core
}

/// Detect whether the finding line sits in a static/const initialization
/// context. Multi-language: each language has its own heuristic.
pub fn detect_const_context(file_content: &str, line_number: usize, file_path: &str) -> bool {
    let lang = detect_language(file_path);
    let lines: Vec<&str> = file_content.lines().collect();
    if line_number == 0 || line_number > lines.len() {
        return false;
    }
    let start = line_number.saturating_sub(6);
    let end = (line_number + 1).min(lines.len());
    let window: String = lines[start..end].join("\n");

    match lang {
        "rust" => {
            window.contains("static ")
                || window.contains("const ")
                || window.contains("OnceLock")
                || window.contains("Lazy::new")
                || window.contains("LazyLock::new")
                || window.contains("lazy_static!")
                || window.contains("once_cell::")
        }
        "python" => window.lines().any(|l| {
            let t = l.trim_start();
            l.len() == t.len()
                && t.contains('=')
                && t.split('=').next().is_some_and(|name| {
                    let n = name.trim();
                    !n.is_empty()
                        && n.chars()
                            .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
                })
        }),
        "javascript" | "typescript" => window.lines().any(|l| {
            let t = l.trim_start();
            let no_indent = l == t || l.starts_with("export ");
            no_indent && (t.starts_with("const ") || t.starts_with("export const "))
        }),
        "go" => {
            window.contains("regexp.MustCompile")
                || window.contains("var (")
                || (window.trim_start().starts_with("var ") && window.contains('='))
        }
        "dart" => {
            (window.contains("final ") || window.contains("const "))
                && (window.contains("RegExp(")
                    || window.contains("= r'")
                    || window.contains("= r\""))
        }
        "java" | "kotlin" => {
            window.contains("static final")
                || window.contains("Pattern.compile")
                || window.contains("const val")
        }
        "ruby" => window.lines().any(|l| {
            let t = l.trim_start();
            l.len() == t.len()
                && t.chars().next().is_some_and(|c| c.is_uppercase())
                && t.contains('=')
        }),
        "php" => window.contains("const ") || window.contains("define("),
        _ => false,
    }
}

/// Extract surrounding lines around a finding.
pub fn surrounding_lines(file_content: &str, line_number: usize, window: usize) -> String {
    if line_number == 0 {
        return String::new();
    }
    let lines: Vec<&str> = file_content.lines().collect();
    let start = line_number.saturating_sub(window + 1);
    let end = (line_number + window).min(lines.len());
    lines[start..end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_covers_extensions() {
        assert_eq!(detect_language("foo.rs"), "rust");
        assert_eq!(detect_language("foo.py"), "python");
        assert_eq!(detect_language("foo.go"), "go");
        assert_eq!(detect_language("foo.js"), "javascript");
        assert_eq!(detect_language("foo.ts"), "typescript");
        assert_eq!(detect_language("foo.dart"), "dart");
        assert_eq!(detect_language("foo.java"), "java");
        assert_eq!(detect_language("foo.kt"), "kotlin");
        assert_eq!(detect_language("foo.rb"), "ruby");
        assert_eq!(detect_language("foo.php"), "php");
        assert_eq!(detect_language("Dockerfile"), "dockerfile");
        assert_eq!(detect_language("Makefile"), "makefile");
        assert_eq!(detect_language("foo.xyz"), "unknown");
    }

    #[test]
    fn test_module_role_rust_test() {
        assert_eq!(detect_module_role("src/foo_test.rs"), ModuleRole::Test);
    }

    #[test]
    fn test_module_role_go_test() {
        assert_eq!(detect_module_role("pkg/foo_test.go"), ModuleRole::Test);
    }

    #[test]
    fn test_module_role_python_test() {
        assert_eq!(detect_module_role("tests/test_foo.py"), ModuleRole::Test);
    }

    #[test]
    fn test_module_role_jest_spec() {
        assert_eq!(detect_module_role("src/foo.spec.ts"), ModuleRole::Test);
    }

    #[test]
    fn test_module_role_dart_generated() {
        assert_eq!(detect_module_role("lib/foo.g.dart"), ModuleRole::Generated);
    }

    #[test]
    fn test_module_role_grpc_python() {
        assert_eq!(
            detect_module_role("proto/foo_pb2.py"),
            ModuleRole::Generated
        );
    }

    #[test]
    fn test_module_role_migration() {
        assert_eq!(
            detect_module_role("db/migrations/001.sql"),
            ModuleRole::Migration
        );
    }

    #[test]
    fn test_module_role_i18n() {
        assert_eq!(detect_module_role("locales/en.ftl"), ModuleRole::I18n);
    }

    #[test]
    fn test_module_role_core_default() {
        assert_eq!(detect_module_role("src/lib.rs"), ModuleRole::Core);
    }

    #[test]
    fn test_const_context_rust_static() {
        assert!(detect_const_context(
            "static FOO: Regex = Regex::new(\"x\").unwrap();",
            1,
            "foo.rs"
        ));
    }

    #[test]
    fn test_const_context_python_module_const() {
        assert!(detect_const_context(
            "PATTERN = re.compile(r'\\d+')",
            1,
            "foo.py"
        ));
    }

    #[test]
    fn test_const_context_js_module_const() {
        assert!(detect_const_context("const PATTERN = /\\d+/;", 1, "foo.js"));
    }

    #[test]
    fn test_const_context_go_must_compile() {
        assert!(detect_const_context(
            "var pattern = regexp.MustCompile(`\\d+`)",
            1,
            "foo.go"
        ));
    }

    #[test]
    fn test_const_context_dart_final_regexp() {
        assert!(detect_const_context(
            "final pattern = RegExp(r'\\d+');",
            1,
            "foo.dart"
        ));
    }

    #[test]
    fn test_const_context_java_pattern_compile() {
        assert!(detect_const_context(
            "static final Pattern P = Pattern.compile(\"x\");",
            1,
            "Foo.java"
        ));
    }

    #[test]
    fn test_const_context_false_in_function() {
        assert!(!detect_const_context(
            "    let re = Regex::new(\"x\").unwrap();",
            1,
            "foo.rs"
        ));
    }
}
