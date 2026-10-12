//! Pass 2 of `build_graph`: turn every parsed import into an edge —
//! symbol imports (C# namespaces, Swift modules, GDScript class names) to
//! the files that declare the symbol and whose types the importer names,
//! C/C++ includes by path or file name, everything else through
//! `resolve_import`.

use std::collections::{HashMap, HashSet};

use super::{ParseResult, ParsedFile, resolve_import};
use crate::analyzer::graph::{ImportEdge, Language};

/// Who declares each symbol, and which types each file declares.
struct SymbolIndex<'a> {
    declared: HashMap<(Language, &'a str), Vec<&'a str>>,
    types_of: HashMap<&'a str, &'a [String]>,
}

impl<'a> SymbolIndex<'a> {
    fn new(parsed: &'a [ParsedFile]) -> Self {
        let mut declared: HashMap<(Language, &str), Vec<&str>> = HashMap::new();
        let mut types_of: HashMap<&str, &[String]> = HashMap::new();
        for (rel_path, lang, result) in parsed {
            for sym in &result.declares {
                declared
                    .entry((*lang, sym.as_str()))
                    .or_default()
                    .push(rel_path.as_str());
            }
            types_of.insert(rel_path.as_str(), &result.type_names);
        }
        SymbolIndex { declared, types_of }
    }

    /// Files declaring `sym` that `from` depends on, or `None` when nobody
    /// in the project declares it. A target counts only when the importer
    /// names one of its types (files declaring no recognisable type count).
    fn targets(
        &self,
        lang: Language,
        sym: &str,
        from: &str,
        result: &ParseResult,
    ) -> Option<Vec<&'a str>> {
        let targets = self.declared.get(&(lang, sym))?;
        Some(
            targets
                .iter()
                .copied()
                .filter(|t| *t != from && self.referenced(result, t))
                .collect(),
        )
    }

    fn referenced(&self, result: &ParseResult, target: &str) -> bool {
        let (Some(ids), Some(types)) = (&result.identifiers, self.types_of.get(target)) else {
            return true;
        };
        types.is_empty() || types.iter().any(|t| ids.contains(t))
    }
}

/// Resolve every import of every parsed file. Returns the edges and the
/// external packages seen.
pub(super) fn resolve_edges(
    parsed: &[ParsedFile],
    known_modules: &HashSet<String>,
) -> (Vec<ImportEdge>, HashSet<String>) {
    let symbols = SymbolIndex::new(parsed);
    let by_basename = basename_index(known_modules);
    let mut edges = Vec::new();
    let mut external_deps = HashSet::new();

    for (rel_path, lang, result) in parsed {
        let mut linked: HashSet<&str> = HashSet::new();
        for sym in &result.implicit_imports {
            if let Some(targets) = symbols.targets(*lang, sym, rel_path, result) {
                link_internal(&mut edges, &mut linked, rel_path, targets);
            }
        }
        for imp in &result.imports {
            if let Some(targets) = symbols.targets(*lang, &imp.module_path, rel_path, result) {
                link_internal(&mut edges, &mut linked, rel_path, targets);
                continue;
            }
            let resolved = if *lang == Language::Cpp {
                resolve_cpp_include(&imp.module_path, rel_path, known_modules, &by_basename)
            } else {
                resolve_import(
                    &imp.module_path,
                    rel_path,
                    imp.is_relative,
                    known_modules,
                    *lang,
                )
            };
            let is_external = resolved.is_none() && !imp.is_relative;
            if is_external {
                external_deps.insert(package_name(&imp.module_path).to_string());
            }
            edges.push(ImportEdge {
                from: rel_path.clone(),
                to: resolved.unwrap_or_else(|| imp.module_path.clone()),
                is_external,
            });
        }
    }
    (edges, external_deps)
}

/// Add one internal edge per target not linked from `from` yet.
fn link_internal<'a>(
    edges: &mut Vec<ImportEdge>,
    linked: &mut HashSet<&'a str>,
    from: &str,
    targets: Vec<&'a str>,
) {
    for target in targets {
        if linked.insert(target) {
            edges.push(ImportEdge {
                from: from.to_string(),
                to: target.to_string(),
                is_external: false,
            });
        }
    }
}

/// The package an external import belongs to: `<vector>` → `vector`,
/// `pkg/sub` → `pkg`, `a.b.c` → `a.b.c` when there is no `/`.
fn package_name(module_path: &str) -> &str {
    module_path
        .trim_start_matches('<')
        .trim_end_matches('>')
        .split('/')
        .next()
        .or_else(|| module_path.split('.').next())
        .unwrap_or(module_path)
}

/// File-name index for C/C++ include resolution (Unreal resolves module
/// includes from several roots, so `Components/Health.h` may live at
/// `Source/Game/Public/Components/Health.h`).
fn basename_index(known_modules: &HashSet<String>) -> HashMap<&str, Vec<&str>> {
    let mut by_basename: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in known_modules {
        let base = m.rsplit('/').next().unwrap_or(m);
        by_basename.entry(base).or_default().push(m.as_str());
    }
    for paths in by_basename.values_mut() {
        paths.sort_unstable();
    }
    by_basename
}

/// Resolve a C/C++ `#include`: `<...>` is a system header (never a project
/// file); a quoted path resolves next to the including file, then to any
/// project file whose path ends with it (first in path order).
fn resolve_cpp_include(
    include: &str,
    from_file: &str,
    known_modules: &std::collections::HashSet<String>,
    by_basename: &std::collections::HashMap<&str, Vec<&str>>,
) -> Option<String> {
    if include.starts_with('<') {
        return None;
    }
    let dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let sibling = if dir.is_empty() {
        include.to_string()
    } else {
        format!("{}/{}", dir, include)
    };
    if known_modules.contains(&sibling) {
        return Some(sibling);
    }
    let base = include.rsplit('/').next().unwrap_or(include);
    let suffix = format!("/{}", include);
    by_basename
        .get(base)?
        .iter()
        .find(|m| m.ends_with(&suffix) || **m == include)
        .map(|m| m.to_string())
}
