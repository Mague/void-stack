//! Dead-code candidates: structural-graph nodes with zero incoming CALLS
//! edges that are not entrypoints, tests, trait-impl methods, registered
//! handlers, or build scripts. Static analysis over the call graph — treat
//! results as CANDIDATES, not verdicts (reflection, macros and dynamic
//! dispatch are invisible to it).

#![cfg(feature = "structural")]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::model::Project;
use crate::runner::local::strip_win_prefix;
use crate::structural::open_db;

#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeCandidate {
    pub qualified_name: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub language: String,
    /// `high` = private symbol with zero callers; `medium` = exported/pub
    /// symbol with zero INTERNAL callers (external crates may use it).
    pub confidence: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeReport {
    pub candidates: Vec<DeadCodeCandidate>,
    pub total_found: usize,
    pub nodes_scanned: usize,
    /// Zero graph callers BUT textually referenced elsewhere (macro bodies,
    /// attribute strings, struct literals — all invisible to the call
    /// graph). Possibly called; excluded from candidates.
    pub uncertain_possibly_referenced: usize,
    pub caveats: Vec<String>,
}

/// Trait-impl / interface methods that are invoked by the runtime or via
/// dynamic dispatch — zero direct CALLS edges says nothing about them.
const TRAIT_LIKE_NAMES: &[&str] = &[
    "fmt",
    "eq",
    "ne",
    "hash",
    "clone",
    "default",
    "drop",
    "from",
    "into",
    "try_from",
    "deserialize",
    "serialize",
    "next",
    "poll",
    "deref",
    "deref_mut",
    "as_ref",
    "as_mut",
    "partial_cmp",
    "cmp",
    "to_string",
    "build",
    "visit",
    "index",
    "add",
    "sub",
    "mul",
    "call",
    "call_once",
    "call_mut",
    "new",
];

/// Attribute markers (the lines just above a Rust item) that mean the
/// symbol is registered by a macro/framework, not called directly.
/// Derive macros whose types are constructed/consumed by frameworks, not
/// by direct calls: clap parses into them, serde deserializes into them.
const FRAMEWORK_DERIVES: &[&str] = &[
    "Parser",
    "Subcommand",
    "Args",
    "ValueEnum",
    "Deserialize",
    "Serialize",
];

const REGISTRATION_MARKERS: &[&str] = &[
    "#[tool",
    "#[tauri::command",
    "#[test",
    "#[tokio::test",
    "#[get",
    "#[post",
    "#[put",
    "#[delete",
    "#[route",
    "#[handler",
    "#[no_mangle",
    "#[wasm_bindgen",
    "#[pyfunction",
];

pub fn find_dead_code(project: &Project, max_results: usize) -> Result<DeadCodeReport, String> {
    let conn = open_db(project)?;
    let root = PathBuf::from(strip_win_prefix(&project.path));

    // Every way an edge can reference a callee: exact qn, Type::name, bare.
    // Conservative on purpose: ANY reference (even an ambiguous bare one)
    // makes a node "alive" — false negatives beat false positives here.
    let called: HashSet<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT target_qualified FROM edges WHERE kind = 'CALLS'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.flatten().collect()
    };

    let nodes = {
        let mut stmt = conn
            .prepare(
                "SELECT name, qualified_name, file_path, line_start, kind, language, \
                 parent_name, is_test FROM nodes WHERE kind IN ('Function', 'Class')",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? as usize,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)? != 0,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.flatten().collect::<Vec<_>>()
    };
    let nodes_scanned = nodes.len();

    // Handler/route registrations from API contracts (vector feature):
    // a producer contract at file:line means that symbol is wired into a
    // routing table even if nothing calls it directly.
    #[cfg(feature = "vector")]
    let contract_files: HashSet<String> = crate::vector_index::project_contracts(project)
        .into_iter()
        .map(|c| c.file)
        .collect();
    #[cfg(not(feature = "vector"))]
    let contract_files: HashSet<String> = HashSet::new();

    let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();
    let mut candidates: Vec<DeadCodeCandidate> = Vec::new();

    for (name, qn, file, line, kind, language, parent, is_test) in nodes {
        if is_test || kind == "Test" {
            continue;
        }
        // Entrypoints and build scripts.
        if name == "main" || file.ends_with("build.rs") || file.contains("/bin/") {
            continue;
        }
        // Trait-impl / interface methods.
        if TRAIT_LIKE_NAMES.contains(&name.as_str()) {
            continue;
        }
        // Test files entirely (helpers inside tests/ are exercised by tests).
        let norm = file.replace('\\', "/");
        if norm.contains("/tests/") || norm.contains("/test/") || norm.starts_with("tests/") {
            continue;
        }
        // Wired into a routing table / contract.
        if contract_files.contains(&norm) {
            continue;
        }

        // Alive if anything references it (exact, typed, or bare).
        if called.contains(&qn) || called.contains(&name) {
            continue;
        }
        if let Some(p) = &parent
            && called.contains(&format!("{}::{}", p, name))
        {
            continue;
        }

        // Visibility/attributes from the source line.
        let lines = file_cache.entry(norm.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&norm))
                .map(|c| c.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        });
        let decl = lines
            .get(line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        let above: String = lines
            [line.saturating_sub(6).min(lines.len())..line.saturating_sub(1).min(lines.len())]
            .join("\n");
        if REGISTRATION_MARKERS.iter().any(|m| above.contains(m)) {
            continue;
        }
        // clap/serde derive types: the framework macro is the caller.
        if above.contains("derive(") && FRAMEWORK_DERIVES.iter().any(|d| above.contains(d)) {
            continue;
        }

        let exported = match language.as_str() {
            "rust" => decl.trim_start().starts_with("pub"),
            "go" => name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
            "dart" => !name.starts_with('_'),
            "javascript" | "typescript" => decl.contains("export"),
            _ => true, // unknown visibility — stay conservative
        };

        candidates.push(DeadCodeCandidate {
            confidence: if exported { "medium" } else { "high" },
            qualified_name: qn,
            name,
            file: norm,
            line,
            kind,
            language,
        });
    }

    // "Possibly called" pass: macro bodies (format!(...)), attribute
    // strings (#[serde(default = "f")]) and struct literals produce NO
    // call edges — a candidate whose name is textually referenced
    // anywhere else is uncertain, not dead. Demoted out of the report.
    let all_files: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT file_path FROM nodes")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.flatten().collect()
    };
    // Build a global identifier-occurrence map in ONE pass over all graph
    // files, then each candidate is an O(1) lookup. The previous approach
    // re-scanned every file for every candidate — O(candidates × files ×
    // lines), which took many minutes on large repos (iunci-flutter hung
    // ~10 min). This is the difference between that and a second.
    let mut token_total: HashMap<String, usize> = HashMap::new();
    for file in &all_files {
        let norm = file.replace('\\', "/");
        let lines = file_cache.entry(norm.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&norm))
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        });
        for line in lines.iter() {
            tokenize_idents(line, &mut token_total);
        }
    }

    let mut uncertain = 0usize;
    candidates.retain(|c| {
        let total = token_total.get(&c.name).copied().unwrap_or(0);
        // Occurrences on the candidate's own declaration line don't count as
        // a reference (that's the definition itself).
        let decl_count = file_cache
            .get(&c.file.replace('\\', "/"))
            .and_then(|lines| lines.get(c.line.saturating_sub(1)))
            .map(|l| count_word(l, &c.name))
            .unwrap_or(0);
        if total > decl_count {
            uncertain += 1; // referenced elsewhere (macro/attr/literal) → not dead
            false
        } else {
            true
        }
    });

    // High confidence first, then by file for stable grouping.
    candidates.sort_by(|a, b| {
        a.confidence
            .cmp(b.confidence) // "high" < "medium" lexicographically
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });
    let total_found = candidates.len();
    candidates.truncate(max_results);

    Ok(DeadCodeReport {
        candidates,
        total_found,
        nodes_scanned,
        uncertain_possibly_referenced: uncertain,
        caveats: vec![
            "Static call-graph analysis: reflection, macro invocations and dynamic dispatch are invisible.".into(),
            "Rust: trait impls and macro-registered items (#[tool], #[tauri::command]) are excluded by heuristic, not proof.".into(),
            "medium = exported/pub with no internal callers — external consumers may still use it.".into(),
        ],
    })
}

/// Tokenize a line into identifier words (`[alphanumeric_]+`) and tally each
/// into `into`. Word boundaries match [`count_word`] so the global tally and
/// per-line counts agree.
fn tokenize_idents(line: &str, into: &mut HashMap<String, usize>) {
    let mut cur = String::new();
    for ch in line.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else if !cur.is_empty() {
            *into.entry(std::mem::take(&mut cur)).or_insert(0) += 1;
        }
    }
    if !cur.is_empty() {
        *into.entry(cur).or_insert(0) += 1;
    }
}

/// Count whole-word occurrences of `needle` in `haystack`.
fn count_word(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !haystack[..abs]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
        let after = abs + needle.len();
        let after_ok = after >= haystack.len()
            || !haystack[after..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
        if before_ok && after_ok {
            n += 1;
        }
        start = abs + needle.len();
    }
    n
}

/// Compact markdown, grouped by file.
pub fn render_dead_code_markdown(r: &DeadCodeReport) -> String {
    let mut md = format!(
        "# Dead-code candidates ({} shown / {} found, {} nodes scanned)\n",
        r.candidates.len(),
        r.total_found,
        r.nodes_scanned
    );
    let mut by_file: Vec<(&str, Vec<&DeadCodeCandidate>)> = Vec::new();
    for c in &r.candidates {
        match by_file.last_mut() {
            Some((f, v)) if *f == c.file => v.push(c),
            _ => by_file.push((c.file.as_str(), vec![c])),
        }
    }
    for (file, items) in by_file {
        md.push_str(&format!("\n## {}\n", file));
        for c in items {
            md.push_str(&format!(
                "- `{}` (line {}) — {} confidence\n",
                c.name, c.line, c.confidence
            ));
        }
    }
    if r.total_found > r.candidates.len() {
        md.push_str(&format!(
            "\n(+{} more)\n",
            r.total_found - r.candidates.len()
        ));
    }
    if r.uncertain_possibly_referenced > 0 {
        md.push_str(&format!(
            "\n{} more symbols have zero graph callers but ARE textually referenced \
             (macros, attributes, struct literals) — treated as possibly called, not listed.\n",
            r.uncertain_possibly_referenced
        ));
    }
    md.push_str("\n## Caveats\n");
    for c in &r.caveats {
        md.push_str(&format!("- {}\n", c));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dead_code_detection_fixture() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"fn main() { used_helper(); }

fn used_helper() { println!("hi"); }

fn unused_private() { println!("never"); }

pub fn unused_public() { println!("api"); }
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("lib_test.rs"),
            "#[test]\nfn test_something() { assert!(true); }\n",
        )
        .unwrap();

        let project = Project {
            name: format!("deadcode-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        crate::structural::build_structural_graph(&project, true).unwrap();

        let report = find_dead_code(&project, 50).unwrap();
        let names: Vec<(&str, &str)> = report
            .candidates
            .iter()
            .map(|c| (c.name.as_str(), c.confidence))
            .collect();

        assert!(
            names.contains(&("unused_private", "high")),
            "private zero-caller fn must be high confidence: {:?}",
            names
        );
        assert!(
            names.contains(&("unused_public", "medium")),
            "pub zero-caller fn must be medium confidence: {:?}",
            names
        );
        assert!(
            !names.iter().any(|(n, _)| *n == "used_helper"),
            "called fn must not appear: {:?}",
            names
        );
        assert!(
            !names.iter().any(|(n, _)| *n == "main"),
            "entrypoint must not appear: {:?}",
            names
        );
        assert!(
            !names.iter().any(|(n, _)| *n == "test_something"),
            "tests must not appear: {:?}",
            names
        );

        let md = render_dead_code_markdown(&report);
        assert!(md.contains("main.rs"));
        assert!(md.contains("Caveats"));
    }

    /// Calls inside macro bodies are invisible to the call graph; a clap
    /// derive struct is constructed by the framework. Neither may be
    /// flagged as dead.
    #[test]
    fn test_macro_calls_and_derive_types_not_flagged() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"#[derive(Debug, Parser)]
struct CliArgs {
    verbose: bool,
}

fn main() {
    println!("{}", format_delta(3));
}

fn format_delta(x: i32) -> String { format!("{:+}", x) }

fn truly_dead() {}
"#,
        )
        .unwrap();

        let project = Project {
            name: format!("deadcode-macro-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        crate::structural::build_structural_graph(&project, true).unwrap();

        let report = find_dead_code(&project, 50).unwrap();
        let names: Vec<&str> = report.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !names.contains(&"format_delta"),
            "macro-internal call must keep the symbol out of candidates: {:?}",
            names
        );
        assert!(
            !names.contains(&"CliArgs"),
            "clap derive struct must be excluded: {:?}",
            names
        );
        assert!(
            names.contains(&"truly_dead"),
            "genuinely unreferenced symbol still flagged: {:?}",
            names
        );
    }
}
