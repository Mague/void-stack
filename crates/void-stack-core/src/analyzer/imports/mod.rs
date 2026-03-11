//! Import parsing for multiple languages.

pub mod python;
pub mod javascript;
pub mod golang;
pub mod dart;
pub mod rust_lang;

use std::path::Path;

use super::graph::*;
use crate::security;

/// Result of parsing a single file.
pub struct ParseResult {
    pub imports: Vec<RawImport>,
    pub class_count: usize,
    pub function_count: usize,
    pub loc: usize,
}

/// A raw import found in source code.
pub struct RawImport {
    pub module_path: String,
    pub is_relative: bool,
}

/// Trait for language-specific import parsers.
pub trait ImportParser {
    fn language(&self) -> Language;
    fn file_extensions(&self) -> &[&str];
    fn parse_file(&self, content: &str, file_path: &str) -> ParseResult;
}

/// Directories to skip during scanning.
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".venv", "venv", "env", "__pycache__", ".git",
    "target", "build", "dist", ".next", ".nuxt", "coverage", ".tox",
    "vendor", "eggs", ".eggs", ".mypy_cache", ".pytest_cache",
];

/// Detect the primary language for a directory.
pub fn detect_language(dir: &Path) -> Option<Language> {
    if dir.join("requirements.txt").exists()
        || dir.join("pyproject.toml").exists()
        || dir.join("setup.py").exists()
    {
        return Some(Language::Python);
    }
    // Fallback: check if there are .py files directly in the dir
    if let Ok(entries) = std::fs::read_dir(dir) {
        let has_py = entries.flatten().any(|e| {
            e.path().extension().map(|ext| ext == "py").unwrap_or(false)
        });
        if has_py {
            return Some(Language::Python);
        }
    }
    if dir.join("package.json").exists() {
        // Check if TS
        if dir.join("tsconfig.json").exists() {
            return Some(Language::TypeScript);
        }
        return Some(Language::JavaScript);
    }
    if dir.join("go.mod").exists() {
        return Some(Language::Go);
    }
    if dir.join("pubspec.yaml").exists() {
        return Some(Language::Dart);
    }
    if dir.join("Cargo.toml").exists() {
        return Some(Language::Rust);
    }
    None
}

/// Build a dependency graph for a project directory.
pub fn build_graph(dir: &Path) -> Option<DependencyGraph> {
    let lang = detect_language(dir)?;

    let parsers: Vec<Box<dyn ImportParser>> = match lang {
        Language::Python => vec![Box::new(python::PythonParser)],
        Language::JavaScript | Language::TypeScript => vec![Box::new(javascript::JsParser)],
        Language::Go => vec![Box::new(golang::GoParser)],
        Language::Dart => vec![Box::new(dart::DartParser)],
        Language::Rust => vec![Box::new(rust_lang::RustParser)],
    };

    let mut modules: Vec<ModuleNode> = Vec::new();
    let mut edges: Vec<ImportEdge> = Vec::new();
    let mut external_deps: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Walk the directory
    let dir_str = dir.to_string_lossy().replace('\\', "/");
    let mut file_paths: Vec<(String, String)> = Vec::new(); // (abs_path, rel_path)
    collect_files(dir, dir, &parsers, &mut file_paths);

    // Known project modules (for resolving internal vs external)
    let known_modules: std::collections::HashSet<String> = file_paths
        .iter()
        .map(|(_, rel)| rel.clone())
        .collect();

    for (abs_path, rel_path) in &file_paths {
        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parser = parsers.iter().find(|p| {
            p.file_extensions().iter().any(|ext| rel_path.ends_with(ext))
        });
        let parser = match parser {
            Some(p) => p,
            None => continue,
        };

        let result = parser.parse_file(&content, rel_path);
        let layer = classify_layer(rel_path, &content);

        modules.push(ModuleNode {
            path: rel_path.clone(),
            language: parser.language(),
            layer,
            loc: result.loc,
            class_count: result.class_count,
            function_count: result.function_count,
        });

        for imp in &result.imports {
            let resolved = resolve_import(&imp.module_path, rel_path, imp.is_relative, &known_modules, parser.language());
            let is_external = resolved.is_none() && !imp.is_relative;

            if is_external {
                let pkg = imp.module_path.split('/').next()
                    .or_else(|| imp.module_path.split('.').next())
                    .unwrap_or(&imp.module_path);
                external_deps.insert(pkg.to_string());
            }

            edges.push(ImportEdge {
                from: rel_path.clone(),
                to: resolved.unwrap_or_else(|| imp.module_path.clone()),
                is_external,
            });
        }
    }

    // ── Fan-in/fan-out refinement pass ────────────────────────────────────
    // For modules still classified as Unknown, use dependency graph position:
    // - High fan-in (many import me) → likely Model or Utility (foundational)
    // - High fan-out (I import many) → likely Controller or Service (orchestrator)
    // - Both low → likely Utility (standalone helper)
    refine_unknown_by_graph(&mut modules, &edges);

    Some(DependencyGraph {
        root_path: dir_str,
        primary_language: lang,
        modules,
        edges,
        external_deps,
    })
}

fn collect_files(
    base: &Path,
    dir: &Path,
    parsers: &[Box<dyn ImportParser>],
    out: &mut Vec<(String, String)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect_files(base, &path, parsers, out);
        } else if path.is_file() {
            // Skip sensitive files (credentials, secrets, .env)
            if security::is_sensitive_file(&path) {
                continue;
            }
            let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let matches = parsers.iter().any(|p| {
                p.file_extensions().iter().any(|pe| ext == *pe)
            });
            if matches {
                let abs = path.to_string_lossy().replace('\\', "/");
                let base_str = base.to_string_lossy().replace('\\', "/");
                let rel = abs.strip_prefix(&base_str)
                    .unwrap_or(&abs)
                    .trim_start_matches('/')
                    .to_string();
                out.push((abs, rel));
            }
        }
    }
}

/// Classify a module into an architectural layer based on path and content.
///
/// Uses a **scoring system**: content patterns are the primary signal (+3 per
/// match), directory/filename conventions add bonus weight (+2/+1). The layer
/// with the highest score wins. This makes classification dynamic — it works
/// across languages without hardcoding project-specific directory names.
fn classify_layer(path: &str, content: &str) -> ArchLayer {
    // 1. Deterministic path-level overrides (test, config, entry points)
    if let Some(layer) = classify_by_path_keywords(path) {
        return layer;
    }

    // 2. Score-based classification: content + path bonuses
    let scores = compute_layer_scores(path, content);

    // Find the layer with the highest score (ignore Unknown)
    let best = scores
        .iter()
        .filter(|(layer, _)| *layer != ArchLayer::Unknown)
        .max_by_key(|(_, score)| *score);

    match best {
        Some((layer, score)) if *score > 0 => *layer,
        _ => ArchLayer::Unknown,
    }
}

// ── Score-based classification engine ──────────────────────────────────────

/// A content pattern that contributes weight toward a layer classification.
struct ContentSignal {
    pattern: &'static str,
    layer: ArchLayer,
    weight: i32,
}

/// Content signals — the primary classification method.
/// These detect what the code *does*, not where it lives.
/// Covers: Rust, Python, JS/TS, Go, Dart/Flutter, Java/Kotlin,
///         and frameworks: Astro, React, Next.js, Vue, Angular, NestJS,
///         Express, FastAPI, Django, Flask, Gin, Echo, Fiber, Actix,
///         Rocket, Axum, Tauri, Shelf, dart_frog, Riverpod, Bloc.
const CONTENT_SIGNALS: &[ContentSignal] = &[
    // ── Controller signals (HTTP handlers, CLI commands, RPC, UI pages) ─────
    // Rust — Actix-web, Rocket, Axum, Tauri, MCP
    ContentSignal { pattern: "#[tauri::command]",   layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "CallToolResult",      layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "#[get(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[post(",             layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[put(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[delete(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[derive(Args",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[derive(Subcommand", layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[command(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "axum::Router",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "axum::extract",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "actix_web::web",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "HttpResponse",        layer: ArchLayer::Controller, weight: 2 },
    // Python — Flask, FastAPI, Django views
    ContentSignal { pattern: "@app.route(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@app.get(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@app.post(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@router.",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "APIRouter",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "def get(self, request",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "def post(self, request", layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@api_view",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "class Meta:\n        model", layer: ArchLayer::Controller, weight: 2 },
    // JS/TS — Express, Fastify, Koa, Hapi
    ContentSignal { pattern: "app.get(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.post(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.put(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.delete(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "router.get(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "router.post(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fastify.get(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fastify.post(",       layer: ArchLayer::Controller, weight: 3 },
    // NestJS
    ContentSignal { pattern: "@Controller(",        layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "@Get(",               layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Post(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Put(",               layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Delete(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Patch(",             layer: ArchLayer::Controller, weight: 3 },
    // Next.js
    ContentSignal { pattern: "export default function Page",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "getServerSideProps",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "getStaticProps",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "export async function GET(",  layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "export async function POST(", layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "NextRequest",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "NextResponse",        layer: ArchLayer::Controller, weight: 3 },
    // React — components are Controller/View
    ContentSignal { pattern: "useState(",           layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "useEffect(",          layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "return (<",           layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "return (\n",          layer: ArchLayer::Controller, weight: 1 },
    ContentSignal { pattern: "React.FC",            layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "JSX.Element",         layer: ArchLayer::Controller, weight: 2 },
    // Vue.js
    ContentSignal { pattern: "defineComponent(",    layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "<script setup",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "defineProps(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "<template>",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "createApp(",          layer: ArchLayer::Controller, weight: 2 },
    // Angular
    ContentSignal { pattern: "@Component(",         layer: ArchLayer::Controller, weight: 5 },
    ContentSignal { pattern: "@NgModule(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Directive(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Pipe(",              layer: ArchLayer::Controller, weight: 2 },
    // Astro
    ContentSignal { pattern: "Astro.props",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Astro.request",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Astro.redirect(",     layer: ArchLayer::Controller, weight: 3 },
    // Go — Gin, Echo, Fiber, Chi, stdlib
    ContentSignal { pattern: "http.HandleFunc(",    layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "gin.Context",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "echo.Context",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fiber.Ctx",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "chi.Router",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "http.Handler",        layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "func (w http.ResponseWriter", layer: ArchLayer::Controller, weight: 4 },
    // Dart/Flutter — shelf, dart_frog, UI widgets
    ContentSignal { pattern: "extends StatelessWidget",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "extends StatefulWidget",   layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Widget build(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Route(",                   layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "shelf.Router",             layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "onRequest(",               layer: ArchLayer::Controller, weight: 3 },
    // Java/Kotlin — Spring MVC
    ContentSignal { pattern: "@RestController",     layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "@GetMapping",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@PostMapping",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@PutMapping",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@DeleteMapping",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@RequestMapping",     layer: ArchLayer::Controller, weight: 3 },

    // ── Service signals (business logic, orchestration, state management) ───
    // Rust
    ContentSignal { pattern: "#[async_trait]",      layer: ArchLayer::Service, weight: 2 },
    ContentSignal { pattern: "impl ",               layer: ArchLayer::Service, weight: 1 },
    // Python
    ContentSignal { pattern: "class ",              layer: ArchLayer::Service, weight: 1 },
    // JS/TS — NestJS
    ContentSignal { pattern: "@Injectable(",        layer: ArchLayer::Service, weight: 4 },
    // Angular
    ContentSignal { pattern: "@Injectable({",       layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "providedIn:",         layer: ArchLayer::Service, weight: 3 },
    // Dart/Flutter — state management (Riverpod, Bloc, Provider)
    ContentSignal { pattern: "extends ChangeNotifier",  layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "extends Bloc<",           layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "extends Cubit<",          layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "StateNotifierProvider",   layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "riverpod",                layer: ArchLayer::Service, weight: 2 },
    ContentSignal { pattern: "FutureProvider",          layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "StreamProvider",          layer: ArchLayer::Service, weight: 3 },
    // React — custom hooks as services
    ContentSignal { pattern: "export function use",     layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "export const use",        layer: ArchLayer::Service, weight: 3 },
    // Vue — composables as services
    ContentSignal { pattern: "export function use",     layer: ArchLayer::Service, weight: 3 },
    // Go
    ContentSignal { pattern: "type Service struct",     layer: ArchLayer::Service, weight: 3 },
    // Java
    ContentSignal { pattern: "@Service",            layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "@Component",          layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "@Transactional",      layer: ArchLayer::Service, weight: 2 },

    // ── Repository signals (data access, ORM, SQL) ──────────────────────────
    // Rust — Diesel, SQLx, SeaORM
    ContentSignal { pattern: "diesel::",            layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sqlx::",              layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sea_orm::",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "#[derive(Queryable",  layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "#[derive(Insertable", layer: ArchLayer::Repository, weight: 4 },
    // Python — SQLAlchemy, Django ORM, Tortoise
    ContentSignal { pattern: "session.query(",      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "cursor.execute(",     layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "objects.filter(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.get(",        layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.create(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.all(",        layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "tortoise.models",     layer: ArchLayer::Repository, weight: 3 },
    // JS/TS — Prisma, Sequelize, Mongoose, TypeORM, Drizzle, Knex
    ContentSignal { pattern: "prisma.",             layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sequelize.",          layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "mongoose.",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "mongoose.model(",    layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "knex(",              layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "typeorm",            layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "drizzle(",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "getRepository(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "@Entity(",           layer: ArchLayer::Repository, weight: 3 },
    // Go — GORM, sqlx, database/sql
    ContentSignal { pattern: "sql.DB",             layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "gorm.",              layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "gorm.Model",         layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sqlx.DB",            layer: ArchLayer::Repository, weight: 3 },
    // Dart/Flutter — Drift (formerly Moor), Floor, sqflite, Isar
    ContentSignal { pattern: "extends DatabaseAccessor",  layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "@DriftDatabase(",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "@dao",                      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "FloorDatabase",             layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "openDatabase(",             layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "Isar.open(",                layer: ArchLayer::Repository, weight: 3 },
    // Java — Spring Data, JPA, Hibernate
    ContentSignal { pattern: "@Repository",        layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "JpaRepository",      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "CrudRepository",     layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "EntityManager",      layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "@Query(",            layer: ArchLayer::Repository, weight: 3 },
    // General SQL patterns
    ContentSignal { pattern: "SELECT ",            layer: ArchLayer::Repository, weight: 2 },
    ContentSignal { pattern: "INSERT INTO",        layer: ArchLayer::Repository, weight: 2 },
    ContentSignal { pattern: "CREATE TABLE",       layer: ArchLayer::Repository, weight: 3 },

    // ── Model signals (data structures, DTOs, schemas) ──────────────────────
    // Rust
    ContentSignal { pattern: "#[derive(Serialize",    layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "#[derive(Deserialize",  layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "pub struct ",           layer: ArchLayer::Model, weight: 1 },
    ContentSignal { pattern: "pub enum ",             layer: ArchLayer::Model, weight: 1 },
    // Python — Pydantic, Django, dataclasses, attrs
    ContentSignal { pattern: "class Meta:",           layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "(BaseModel):",          layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "(models.Model):",       layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@dataclass",            layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@attr.s",               layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "Field(",                layer: ArchLayer::Model, weight: 1 },
    // JS/TS — Zod, interfaces, types
    ContentSignal { pattern: "z.object(",             layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "z.string(",             layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "export interface ",     layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "export type ",          layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "new Schema(",           layer: ArchLayer::Model, weight: 3 },
    // NestJS
    ContentSignal { pattern: "@Schema(",              layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Prop(",                layer: ArchLayer::Model, weight: 2 },
    // Go — struct tags
    ContentSignal { pattern: "`json:\"",              layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "`xml:\"",               layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "`db:\"",                layer: ArchLayer::Model, weight: 2 },
    // Dart/Flutter — json_serializable, freezed
    ContentSignal { pattern: "@JsonSerializable(",    layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@freezed",              layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "factory ",              layer: ArchLayer::Model, weight: 1 },
    ContentSignal { pattern: "fromJson(",             layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "toJson(",               layer: ArchLayer::Model, weight: 2 },
    // Java — JPA, Lombok
    ContentSignal { pattern: "@Entity",               layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@Data",                 layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Table(",               layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Column(",              layer: ArchLayer::Model, weight: 2 },

    // ── Utility signals ─────────────────────────────────────────────────────
    ContentSignal { pattern: "pub fn ",               layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "function ",             layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "def ",                  layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "func ",                 layer: ArchLayer::Utility, weight: 1 },

    // ── Config signals ──────────────────────────────────────────────────────
    ContentSignal { pattern: "dotenv",                layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "env::",                 layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "process.env.",          layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "os.environ",            layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "os.Getenv(",            layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "Platform.environment",  layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "defineConfig(",         layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "vite.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "next.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "nuxt.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "astro.config",          layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "angular.json",          layer: ArchLayer::Config, weight: 3 },
];

/// Directory name bonus — universal conventions only (not project-specific).
const DIR_BONUS: &[(&[&str], ArchLayer, i32)] = &[
    (&["controllers", "controller", "routes", "routers", "handlers",
      "views", "endpoints", "api", "commands", "tools", "pages"],
      ArchLayer::Controller, 2),
    (&["services", "service", "usecases", "use_cases", "domain",
      "business", "logic"],
      ArchLayer::Service, 2),
    (&["repositories", "repository", "repos", "dao", "dal", "data",
      "db", "database", "persistence", "migration", "migrations"],
      ArchLayer::Repository, 2),
    (&["models", "model", "entities", "entity", "schemas", "schema",
      "types", "dto", "dtos", "proto"],
      ArchLayer::Model, 2),
    (&["utils", "util", "helpers", "helper", "common", "shared",
      "lib", "middleware"],
      ArchLayer::Utility, 2),
    (&["config", "configuration", "settings"],
      ArchLayer::Config, 2),
];

/// Compute weighted scores for each architectural layer.
fn compute_layer_scores(path: &str, content: &str) -> Vec<(ArchLayer, i32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<ArchLayer, i32> = HashMap::new();

    // Content signals (primary — weight 1-4 per match)
    for signal in CONTENT_SIGNALS {
        if content.contains(signal.pattern) {
            *scores.entry(signal.layer).or_insert(0) += signal.weight;
        }
    }

    // Directory name bonus (secondary — +2 for universal conventions)
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts {
        let p = part.to_lowercase();
        for (names, layer, bonus) in DIR_BONUS {
            if names.iter().any(|n| *n == p.as_str()) {
                *scores.entry(*layer).or_insert(0) += bonus;
            }
        }
    }

    scores.into_iter().collect()
}

// ── Deterministic overrides (path-only, no scoring needed) ─────────────────

fn classify_by_path_keywords(path: &str) -> Option<ArchLayer> {
    let lower = path.to_lowercase();

    // Test files — always deterministic
    if lower.contains("test") || lower.contains("spec") || lower.starts_with("tests/") {
        return Some(ArchLayer::Test);
    }

    // Config files by extension pattern
    let config_suffixes = ["config.py", "config.js", "config.ts", "config.rs",
                           "config.toml", "config.yaml", "config.yml"];
    if lower.contains(".env") || config_suffixes.iter().any(|s| lower.ends_with(s)) {
        return Some(ArchLayer::Config);
    }

    // Entry points and build scripts — always Utility
    if lower.ends_with("main.rs") || lower.ends_with("build.rs") || lower.ends_with("lib.rs") {
        return Some(ArchLayer::Utility);
    }

    None
}

/// Refine Unknown modules using fan-in/fan-out from the dependency graph.
/// This is the dynamic, language-agnostic classification — no hardcoded names.
fn refine_unknown_by_graph(modules: &mut [ModuleNode], edges: &[ImportEdge]) {
    use std::collections::HashMap;

    // Count fan-in (how many modules import me) and fan-out (how many I import)
    let mut fan_in: HashMap<&str, usize> = HashMap::new();
    let mut fan_out: HashMap<&str, usize> = HashMap::new();

    for edge in edges {
        if !edge.is_external {
            *fan_in.entry(edge.to.as_str()).or_insert(0) += 1;
            *fan_out.entry(edge.from.as_str()).or_insert(0) += 1;
        }
    }

    // Calculate median fan-in to set adaptive thresholds
    let mut fan_in_values: Vec<usize> = modules.iter()
        .map(|m| *fan_in.get(m.path.as_str()).unwrap_or(&0))
        .filter(|v| *v > 0)
        .collect();
    fan_in_values.sort_unstable();
    let median_fan_in = fan_in_values.get(fan_in_values.len() / 2).copied().unwrap_or(1);
    let high_fan_in_threshold = (median_fan_in * 2).max(3);

    for module in modules.iter_mut() {
        if module.layer != ArchLayer::Unknown {
            continue;
        }

        let fi = *fan_in.get(module.path.as_str()).unwrap_or(&0);
        let fo = *fan_out.get(module.path.as_str()).unwrap_or(&0);

        // High fan-in: many depend on me → foundational (Model or Utility)
        if fi >= high_fan_in_threshold && fo <= 1 {
            // Pure data provider — likely Model
            module.layer = ArchLayer::Model;
        } else if fi >= high_fan_in_threshold {
            // Widely used but also uses others — Utility
            module.layer = ArchLayer::Utility;
        } else if fo >= high_fan_in_threshold && fi <= 1 {
            // I import many, few import me — orchestrator (Controller)
            module.layer = ArchLayer::Controller;
        } else if fo > fi && fo >= 2 {
            // More outgoing than incoming — Service (coordinator)
            module.layer = ArchLayer::Service;
        }
        // Otherwise stays Unknown — genuinely unclassifiable
    }
}

/// Try to resolve an import path to a known project module.
fn resolve_import(
    import_path: &str,
    from_file: &str,
    is_relative: bool,
    known_modules: &std::collections::HashSet<String>,
    lang: Language,
) -> Option<String> {
    if is_relative {
        // Resolve relative to current file's directory
        let dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let cleaned = import_path.trim_start_matches("./");

        let candidates = match lang {
            Language::Python => {
                let dot_path = cleaned.replace('.', "/");
                vec![
                    format!("{}/{}.py", dir, dot_path),
                    format!("{}/{}/__init__.py", dir, dot_path),
                    format!("{}.py", dot_path),
                    format!("{}/__init__.py", dot_path),
                ]
            }
            Language::Go => {
                vec![
                    format!("{}/{}", dir, cleaned),
                    cleaned.to_string(),
                ]
            }
            Language::Dart => {
                vec![
                    format!("{}/{}", dir, cleaned),
                    cleaned.to_string(),
                ]
            }
            Language::Rust => {
                vec![
                    cleaned.to_string(),
                    format!("{}/{}", dir, cleaned),
                    format!("{}.rs", cleaned.trim_end_matches(".rs")),
                    format!("{}/mod.rs", cleaned.trim_end_matches("/mod.rs")),
                ]
            }
            _ => {
                vec![
                    format!("{}/{}", dir, cleaned),
                    format!("{}/{}.js", dir, cleaned),
                    format!("{}/{}.ts", dir, cleaned),
                    format!("{}/{}/index.js", dir, cleaned),
                    format!("{}/{}/index.ts", dir, cleaned),
                ]
            }
        };

        for candidate in &candidates {
            let normalized = candidate.trim_start_matches('/').to_string();
            if known_modules.contains(&normalized) {
                return Some(normalized);
            }
        }
        return None;
    }

    // Non-relative: try as project module
    match lang {
        Language::Python => {
            let module_path = import_path.replace('.', "/");
            let candidates = vec![
                format!("{}.py", module_path),
                format!("{}/__init__.py", module_path),
                format!("src/{}.py", module_path),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Go => {
            // Go external imports contain dots (github.com/...)
            // Internal packages might be resolved by directory
            let last = import_path.rsplit('/').next().unwrap_or(import_path);
            let candidates = vec![
                format!("{}.go", last),
                format!("{}/{}.go", last, last),
                format!("pkg/{}.go", last),
                format!("internal/{}.go", last),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Rust => {
            // crate:: paths are handled as relative; external crates won't resolve
            let path = import_path.replace("::", "/");
            let candidates = vec![
                format!("{}.rs", path),
                format!("{}/mod.rs", path),
                format!("src/{}.rs", path),
                format!("src/{}/mod.rs", path),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        Language::Dart => {
            // package: imports normalized to lib/
            let candidates = vec![
                import_path.to_string(),
                format!("{}.dart", import_path.trim_end_matches(".dart")),
            ];
            for c in &candidates {
                if known_modules.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        _ => {
            // JS/TS: non-relative without ./ is external
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_layer: content-based scoring ──────────────────────────────

    #[test]
    fn test_classify_rust_controller_tauri() {
        let content = r#"
            #[tauri::command]
            pub async fn list_services() -> Result<Vec<Service>, String> { todo!() }
        "#;
        assert_eq!(classify_layer("src/commands/services.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_rust_controller_actix() {
        let content = r#"
            #[get("/api/users")]
            async fn get_users() -> HttpResponse { HttpResponse::Ok().finish() }
        "#;
        assert_eq!(classify_layer("src/handlers/users.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_rust_service_impl() {
        let content = r#"
            #[async_trait]
            impl Runner for LocalRunner {
                async fn start(&self) -> Result<()> { todo!() }
                async fn stop(&self) -> Result<()> { todo!() }
            }
        "#;
        assert_eq!(classify_layer("src/runner/local.rs", content), ArchLayer::Service);
    }

    #[test]
    fn test_classify_rust_model() {
        let content = r#"
            #[derive(Serialize, Deserialize, Clone)]
            pub struct Project {
                pub name: String,
                pub services: Vec<Service>,
            }

            #[derive(Serialize, Deserialize)]
            pub enum RunnerType { Local, Docker, Ssh }
        "#;
        assert_eq!(classify_layer("src/model.rs", content), ArchLayer::Model);
    }

    #[test]
    fn test_classify_rust_repository_sqlx() {
        let content = r#"
            use sqlx::PgPool;
            pub async fn get_user(pool: &PgPool, id: i32) -> Result<User> {
                sqlx::query_as("SELECT * FROM users WHERE id = $1").fetch_one(pool).await
            }
        "#;
        assert_eq!(classify_layer("src/db/users.rs", content), ArchLayer::Repository);
    }

    // ── classify_layer: Python frameworks ──────────────────────────────────

    #[test]
    fn test_classify_python_fastapi() {
        let content = r#"
            from fastapi import APIRouter
            router = APIRouter()
            @router.get("/users")
            async def get_users(): pass
        "#;
        assert_eq!(classify_layer("app/routes/users.py", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_python_django_model() {
        let content = r#"
            from django.db import models
            class User(models.Model):
                name = models.CharField(max_length=100)
                class Meta:
                    db_table = 'users'
        "#;
        assert_eq!(classify_layer("app/models.py", content), ArchLayer::Model);
    }

    #[test]
    fn test_classify_python_sqlalchemy_repo() {
        let content = r#"
            def get_user(db):
                return db.session.query(User).filter(User.id == 1).first()
        "#;
        assert_eq!(classify_layer("app/repos/user_repo.py", content), ArchLayer::Repository);
    }

    // ── classify_layer: JS/TS frameworks ───────────────────────────────────

    #[test]
    fn test_classify_nestjs_controller() {
        let content = r#"
            @Controller('users')
            export class UsersController {
                @Get()
                findAll() { return this.usersService.findAll(); }
                @Post()
                create(@Body() dto: CreateUserDto) {}
            }
        "#;
        assert_eq!(classify_layer("src/users/users.controller.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_nestjs_service() {
        let content = r#"
            @Injectable()
            export class UsersService {
                constructor(private repo: UsersRepository) {}
                findAll() { return this.repo.find(); }
            }
        "#;
        assert_eq!(classify_layer("src/users/users.service.ts", content), ArchLayer::Service);
    }

    #[test]
    fn test_classify_nextjs_api_route() {
        let content = r#"
            import { NextRequest, NextResponse } from 'next/server';
            export async function GET(request: NextRequest) {
                return NextResponse.json({ users: [] });
            }
        "#;
        assert_eq!(classify_layer("app/api/users/route.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_react_component() {
        let content = r#"
            import { useState, useEffect } from 'react';
            export default function UserList() {
                const [users, setUsers] = useState([]);
                useEffect(() => { fetchUsers(); }, []);
                return (<div>{users.map(u => <p>{u.name}</p>)}</div>);
            }
        "#;
        assert_eq!(classify_layer("src/components/UserList.tsx", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_vue_component() {
        let content = r#"
            <template>
                <div>{{ message }}</div>
            </template>
            <script setup>
            import { ref } from 'vue'
            const message = ref('Hello')
            </script>
        "#;
        assert_eq!(classify_layer("src/components/Hello.vue", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_angular_component() {
        let content = r#"
            @Component({
                selector: 'app-user-list',
                templateUrl: './user-list.component.html'
            })
            export class UserListComponent implements OnInit { }
        "#;
        assert_eq!(classify_layer("src/app/user-list.component.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_prisma_repository() {
        let content = r#"
            import { PrismaClient } from '@prisma/client';
            const prisma = new PrismaClient();
            export async function getUsers() {
                return prisma.user.findMany();
            }
        "#;
        assert_eq!(classify_layer("src/repos/userRepo.ts", content), ArchLayer::Repository);
    }

    #[test]
    fn test_classify_zod_model() {
        let content = r#"
            import { z } from 'zod';
            export const UserSchema = z.object({
                id: z.string(),
                name: z.string(),
            });
            export type User = z.infer<typeof UserSchema>;
        "#;
        assert_eq!(classify_layer("src/schemas/user.ts", content), ArchLayer::Model);
    }

    // ── classify_layer: Go ─────────────────────────────────────────────────

    #[test]
    fn test_classify_go_handler() {
        let content = r#"
            func GetUsers(c *gin.Context) {
                users := service.GetAll()
                c.JSON(200, users)
            }
        "#;
        assert_eq!(classify_layer("handlers/users.go", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_go_gorm_repo() {
        let content = r#"
            func (r *UserRepo) FindAll(db *gorm.DB) []User {
                var users []User
                db.Find(&users)
                return users
            }
        "#;
        assert_eq!(classify_layer("repos/user_repo.go", content), ArchLayer::Repository);
    }

    // ── classify_layer: Dart/Flutter ───────────────────────────────────────

    #[test]
    fn test_classify_flutter_widget() {
        let content = r#"
            class HomePage extends StatefulWidget {
                @override
                State<HomePage> createState() => _HomePageState();
            }
        "#;
        assert_eq!(classify_layer("lib/pages/home_page.dart", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_flutter_bloc_service() {
        let content = r#"
            class UserBloc extends Bloc<UserEvent, UserState> {
                final UserRepository repo;
                UserBloc(this.repo) : super(UserInitial());
            }
        "#;
        assert_eq!(classify_layer("lib/blocs/user_bloc.dart", content), ArchLayer::Service);
    }

    #[test]
    fn test_classify_dart_json_model() {
        let content = r#"
            @JsonSerializable()
            class User {
                final String id;
                factory User.fromJson(Map<String, dynamic> json) => _$UserFromJson(json);
                Map<String, dynamic> toJson() => _$UserToJson(this);
            }
        "#;
        assert_eq!(classify_layer("lib/models/user.dart", content), ArchLayer::Model);
    }

    // ── classify_layer: deterministic overrides ────────────────────────────

    #[test]
    fn test_classify_test_file() {
        assert_eq!(classify_layer("tests/test_users.py", "def test_create(): pass"), ArchLayer::Test);
        assert_eq!(classify_layer("src/__tests__/user.spec.ts", "describe('User', () => {})"), ArchLayer::Test);
    }

    #[test]
    fn test_classify_config_file() {
        assert_eq!(classify_layer("src/config.py", "DEBUG = True"), ArchLayer::Config);
        assert_eq!(classify_layer(".env.production", "API_KEY=xxx"), ArchLayer::Config);
    }

    #[test]
    fn test_classify_entry_points() {
        assert_eq!(classify_layer("src/main.rs", "fn main() {}"), ArchLayer::Utility);
        assert_eq!(classify_layer("src/lib.rs", "pub mod model;"), ArchLayer::Utility);
        assert_eq!(classify_layer("build.rs", "fn main() {}"), ArchLayer::Utility);
    }

    // ── classify_layer: content wins over ambiguous path ───────────────────

    #[test]
    fn test_content_wins_over_path() {
        let content = r#"
            #[get("/health")]
            async fn health() -> HttpResponse { HttpResponse::Ok().finish() }
        "#;
        assert_eq!(classify_layer("src/misc/health.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_unknown_minimal_content() {
        assert_eq!(classify_layer("src/foo.rs", "// empty"), ArchLayer::Unknown);
    }

    // ── classify_layer: Astro & MCP ────────────────────────────────────────

    #[test]
    fn test_classify_astro_page() {
        let content = "const data = Astro.props;\nconst resp = Astro.request;";
        assert_eq!(classify_layer("src/pages/index.astro", content), ArchLayer::Controller);
    }

    #[test]
    fn test_classify_mcp_tool() {
        let content = r#"
            pub async fn handle(params: Value) -> CallToolResult {
                CallToolResult::success(vec![])
            }
        "#;
        assert_eq!(classify_layer("src/tools/analysis.rs", content), ArchLayer::Controller);
    }

    // ── refine_unknown_by_graph (fan-in/fan-out) ───────────────────────────

    #[test]
    fn test_fanin_refines_unknown_to_model() {
        // types.rs is imported by many modules (fan-in=8), exports nothing (fan-out=0)
        // Need enough modules so that median fan-in keeps threshold reachable
        let mut modules = vec![
            ModuleNode { path: "types.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 50, class_count: 0, function_count: 2 },
            ModuleNode { path: "a.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "b.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 80, class_count: 0, function_count: 3 },
            ModuleNode { path: "c.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 60, class_count: 0, function_count: 4 },
            ModuleNode { path: "d.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 40, class_count: 0, function_count: 2 },
            ModuleNode { path: "e.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 70, class_count: 0, function_count: 3 },
            ModuleNode { path: "f.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 50, class_count: 0, function_count: 2 },
            ModuleNode { path: "g.rs".into(), language: Language::Rust, layer: ArchLayer::Utility, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "h.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 90, class_count: 0, function_count: 4 },
        ];
        let edges = vec![
            ImportEdge { from: "a.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "b.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "c.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "d.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "e.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "f.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "g.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "h.rs".into(), to: "types.rs".into(), is_external: false },
            // Some cross-deps so median is reasonable
            ImportEdge { from: "a.rs".into(), to: "b.rs".into(), is_external: false },
            ImportEdge { from: "c.rs".into(), to: "d.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Model);
    }

    #[test]
    fn test_fanout_refines_unknown_to_controller() {
        let mut modules = vec![
            ModuleNode { path: "handler.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 200, class_count: 0, function_count: 10 },
            ModuleNode { path: "svc_a.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 50, class_count: 0, function_count: 3 },
            ModuleNode { path: "svc_b.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 50, class_count: 0, function_count: 3 },
            ModuleNode { path: "model.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
        ];
        let edges = vec![
            ImportEdge { from: "handler.rs".into(), to: "svc_a.rs".into(), is_external: false },
            ImportEdge { from: "handler.rs".into(), to: "svc_b.rs".into(), is_external: false },
            ImportEdge { from: "handler.rs".into(), to: "model.rs".into(), is_external: false },
            ImportEdge { from: "svc_a.rs".into(), to: "model.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Controller);
    }

    #[test]
    fn test_fanout_gt_fanin_refines_to_service() {
        let mut modules = vec![
            ModuleNode { path: "orch.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "dep_a.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "dep_b.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "caller.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 50, class_count: 0, function_count: 3 },
        ];
        let edges = vec![
            ImportEdge { from: "orch.rs".into(), to: "dep_a.rs".into(), is_external: false },
            ImportEdge { from: "orch.rs".into(), to: "dep_b.rs".into(), is_external: false },
            ImportEdge { from: "caller.rs".into(), to: "orch.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Service);
    }

    #[test]
    fn test_no_edges_stays_unknown() {
        let mut modules = vec![
            ModuleNode { path: "isolated.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 10, class_count: 0, function_count: 1 },
        ];
        refine_unknown_by_graph(&mut modules, &[]);
        assert_eq!(modules[0].layer, ArchLayer::Unknown);
    }

    #[test]
    fn test_already_classified_not_changed() {
        let mut modules = vec![
            ModuleNode { path: "service.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "dep.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
        ];
        let edges = vec![
            ImportEdge { from: "dep.rs".into(), to: "service.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Service);
    }

    // ── compute_layer_scores ───────────────────────────────────────────────

    #[test]
    fn test_scores_accumulate() {
        let content = "#[get(\"/a\")]\n#[post(\"/b\")]\n#[put(\"/c\")]";
        let scores = compute_layer_scores("src/api.rs", content);
        let ctrl = scores.iter().find(|(l, _)| *l == ArchLayer::Controller).map(|(_, s)| *s).unwrap_or(0);
        assert!(ctrl >= 9, "3 controller signals should give ≥9 pts, got {ctrl}");
    }

    #[test]
    fn test_dir_bonus_applied() {
        let scores = compute_layer_scores("controllers/user.rs", "");
        let ctrl = scores.iter().find(|(l, _)| *l == ArchLayer::Controller).map(|(_, s)| *s).unwrap_or(0);
        assert_eq!(ctrl, 2);
    }

    #[test]
    fn test_content_beats_dir() {
        // File in models/ but content is clearly controller
        let content = "@Controller('users')\n@Get()\n@Post()\n@Put()";
        let layer = classify_layer("models/user_controller.ts", content);
        assert_eq!(layer, ArchLayer::Controller);
    }
}
